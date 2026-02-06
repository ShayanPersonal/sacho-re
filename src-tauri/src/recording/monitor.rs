// MIDI monitoring service that triggers automatic recording

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::io::Write;
use std::collections::VecDeque;
use parking_lot::{RwLock, Mutex};
use midir::{MidiInput, MidiInputConnection};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Manager, Emitter};

use crate::config::Config;
use crate::devices::DeviceManager;
use crate::encoding::VideoCodec;
use crate::recording::RecordingState;
use crate::recording::midi::TimestampedMidiEvent;
use crate::recording::preroll::{MidiPrerollBuffer, AudioPrerollBuffer};
use crate::recording::video::VideoCaptureManager;
use crate::session::{SessionMetadata, SessionDatabase, MidiFileInfo, AudioFileInfo};
use crate::notifications;

/// Audio buffer for a device
pub struct AudioBuffer {
    pub device_name: String,
    pub samples: VecDeque<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Shared state for recording capture
pub struct CaptureState {
    pub is_recording: bool,
    /// True while starting (prevents duplicate triggers, keeps pre-roll active)
    pub is_starting: bool,
    pub session_path: Option<PathBuf>,
    pub start_time: Option<Instant>,
    pub midi_events: Vec<(String, TimestampedMidiEvent)>, // (device_name, event)
    pub audio_buffers: Vec<AudioBuffer>,
    /// Pre-roll buffer for MIDI events (used when not recording)
    pub midi_preroll: MidiPrerollBuffer,
    /// Pre-roll buffers for audio (one per device, used when not recording)
    pub audio_prerolls: Vec<AudioPrerollBuffer>,
    /// Pre-roll duration in seconds
    pub pre_roll_secs: u32,
    /// MIDI timestamp offset in microseconds (equals sync_preroll_duration)
    /// This is added to real-time MIDI timestamps to align with pre-roll content
    pub midi_timestamp_offset_us: u64,
}

impl CaptureState {
    pub fn new(pre_roll_secs: u32) -> Self {
        Self {
            is_recording: false,
            is_starting: false,
            session_path: None,
            start_time: None,
            midi_events: Vec::new(),
            audio_buffers: Vec::new(),
            midi_preroll: MidiPrerollBuffer::new(pre_roll_secs),
            audio_prerolls: Vec::new(),
            pre_roll_secs,
            midi_timestamp_offset_us: 0,
        }
    }
    
    /// Check if we should capture to pre-roll (not recording, or starting)
    pub fn should_use_preroll(&self) -> bool {
        !self.is_recording || self.is_starting
    }
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            is_recording: false,
            is_starting: false,
            session_path: None,
            start_time: None,
            midi_events: Vec::new(),
            audio_buffers: Vec::new(),
            midi_preroll: MidiPrerollBuffer::new(2),
            audio_prerolls: Vec::new(),
            pre_roll_secs: 2,
            midi_timestamp_offset_us: 0,
        }
    }
}

// We can't store cpal::Stream in the struct because it's not Send
// Use a thread-local approach instead
// 
// IMPORTANT: This means start() and stop() MUST be called from the same thread
// for audio streams to be properly cleaned up. Since MidiMonitor is behind an
// Arc<Mutex<>>, the Tauri command handlers should always call from the same thread.
use std::cell::RefCell;
thread_local! {
    static AUDIO_STREAMS: RefCell<Vec<cpal::Stream>> = RefCell::new(Vec::new());
}

/// Manages background MIDI monitoring and automatic recording
pub struct MidiMonitor {
    trigger_connections: Vec<MidiInputConnection<()>>,
    capture_connections: Vec<MidiInputConnection<()>>,
    app_handle: AppHandle,
    last_event_time: Arc<RwLock<Option<Instant>>>,
    is_monitoring: Arc<RwLock<bool>>,
    capture_state: Arc<Mutex<CaptureState>>,
    video_manager: Arc<Mutex<VideoCaptureManager>>,
    /// Handle for the video poller background thread
    video_poller_handle: Option<std::thread::JoinHandle<()>>,
    /// Handle for the idle checker background thread
    idle_checker_handle: Option<std::thread::JoinHandle<()>>,
}

impl MidiMonitor {
    /// Create a new MIDI monitor
    pub fn new(app_handle: AppHandle) -> Self {
        // Get pre-roll duration from config
        let pre_roll_secs = {
            let config_state = app_handle.state::<RwLock<Config>>();
            let config = config_state.read();
            config.pre_roll_secs.min(5)
        };
        
        Self {
            trigger_connections: Vec::new(),
            capture_connections: Vec::new(),
            app_handle,
            last_event_time: Arc::new(RwLock::new(None)),
            is_monitoring: Arc::new(RwLock::new(false)),
            capture_state: Arc::new(Mutex::new(CaptureState::default())),
            video_manager: Arc::new(Mutex::new(VideoCaptureManager::new(pre_roll_secs))),
            video_poller_handle: None,
            idle_checker_handle: None,
        }
    }
    
    /// Get a reference to the video manager
    pub fn video_manager(&self) -> Arc<Mutex<VideoCaptureManager>> {
        self.video_manager.clone()
    }
    
    /// Start monitoring MIDI ports based on config
    pub fn start(&mut self) -> anyhow::Result<()> {
        // Stop any existing monitoring
        self.stop();
        
        let config = self.app_handle.state::<RwLock<Config>>();
        let config = config.read();
        
        // Update pre-roll duration from config
        {
            let pre_roll = config.pre_roll_secs.min(5);
            let mut state = self.capture_state.lock();
            state.pre_roll_secs = pre_roll;
            state.midi_preroll.set_duration(pre_roll);
        }
        
        println!("[Sacho] Trigger MIDI devices: {:?}", config.trigger_midi_devices);
        println!("[Sacho] Record MIDI devices: {:?}", config.selected_midi_devices);
        println!("[Sacho] Pre-roll: {} seconds", config.pre_roll_secs);
        
        let midi_in = MidiInput::new("sacho-enum")?;
        let ports = midi_in.ports();
        
        // Build port info map
        let mut port_info: Vec<(usize, String)> = Vec::new();
        for (idx, port) in ports.iter().enumerate() {
            if let Ok(name) = midi_in.port_name(port) {
                port_info.push((idx, name));
            }
        }
        
        println!("[Sacho] Available MIDI ports: {:?}", port_info);
        
        // Connect to trigger devices
        for (port_index, port_name) in &port_info {
            let device_id = format!("midi-{}", port_index);
            
            if config.trigger_midi_devices.contains(&device_id) {
                println!("[Sacho] Connecting trigger: {} ({})", port_name, device_id);
                
                let midi_in = MidiInput::new("sacho-trigger")?;
                let ports = midi_in.ports();
                
                if let Some(port) = ports.get(*port_index) {
                    let app_handle = self.app_handle.clone();
                    let last_event_time = self.last_event_time.clone();
                    let capture_state = self.capture_state.clone();
                    let video_manager = self.video_manager.clone();
                    let port_name_clone = port_name.clone();
                    
                    match midi_in.connect(
                        port,
                        "sacho-trigger",
                        move |timestamp_us, message, _| {
                            // Store event (to pre-roll buffer if not fully recording, to main buffer otherwise)
                            {
                                let mut state = capture_state.lock();
                                
                                // Use pre-roll if not recording OR if recording is starting (video init)
                                if state.should_use_preroll() {
                                    // Store in pre-roll buffer with driver timestamp for accurate timing
                                    let event = TimestampedMidiEvent {
                                        timestamp_us: 0,
                                        data: message.to_vec(),
                                    };
                                    state.midi_preroll.push(port_name_clone.clone(), event, timestamp_us);
                                } else {
                                    // Recording is active, store with proper timestamp
                                    // Add midi_timestamp_offset_us to align with pre-roll content
                                    let rel_time = state.start_time
                                        .map(|st| st.elapsed().as_micros() as u64 + state.midi_timestamp_offset_us)
                                        .unwrap_or(state.midi_timestamp_offset_us);
                                    state.midi_events.push((
                                        port_name_clone.clone(),
                                        TimestampedMidiEvent {
                                            timestamp_us: rel_time,
                                            data: message.to_vec(),
                                        }
                                    ));
                                }
                            }
                            
                            // Check for note-on to trigger recording
                            if message.len() >= 3 {
                                let status = message[0] & 0xF0;
                                let velocity = message[2];
                                
                                if status == 0x90 && velocity > 0 {
                                    handle_midi_trigger(&app_handle, &last_event_time, &capture_state, &video_manager);
                                }
                            }
                        },
                        (),
                    ) {
                        Ok(conn) => {
                            self.trigger_connections.push(conn);
                            println!("[Sacho] Connected to trigger: {}", port_name);
                        }
                        Err(e) => {
                            println!("[Sacho] Failed to connect trigger {}: {}", port_name, e);
                        }
                    }
                }
            }
        }
        
        // Connect to record devices (that aren't already triggers)
        for (port_index, port_name) in &port_info {
            let device_id = format!("midi-{}", port_index);
            
            // Skip if already connected as trigger
            if config.trigger_midi_devices.contains(&device_id) {
                continue;
            }
            
            if config.selected_midi_devices.contains(&device_id) {
                println!("[Sacho] Connecting record device: {} ({})", port_name, device_id);
                
                let midi_in = MidiInput::new("sacho-record")?;
                let ports = midi_in.ports();
                
                if let Some(port) = ports.get(*port_index) {
                    let capture_state = self.capture_state.clone();
                    let last_event_time = self.last_event_time.clone();
                    let port_name_clone = port_name.clone();
                    
                    match midi_in.connect(
                        port,
                        "sacho-record",
                        move |timestamp_us, message, _| {
                            let mut state = capture_state.lock();
                            
                            // Update last event time for idle detection (even during pre-roll)
                            if message.len() >= 3 {
                                let status = message[0] & 0xF0;
                                if status == 0x90 || status == 0x80 {
                                    *last_event_time.write() = Some(Instant::now());
                                }
                            }
                            
                            // Use pre-roll if not recording OR if recording is starting (video init)
                            if state.should_use_preroll() {
                                // Store in pre-roll buffer with driver timestamp for accurate timing
                                state.midi_preroll.push(
                                    port_name_clone.clone(),
                                    TimestampedMidiEvent {
                                        timestamp_us: 0,
                                        data: message.to_vec(),
                                    },
                                    timestamp_us,
                                );
                            } else {
                                // Recording is active, store with proper timestamp
                                // Add midi_timestamp_offset_us to align with pre-roll content
                                let rel_time = state.start_time
                                    .map(|st| st.elapsed().as_micros() as u64 + state.midi_timestamp_offset_us)
                                    .unwrap_or(state.midi_timestamp_offset_us);
                                state.midi_events.push((
                                    port_name_clone.clone(),
                                    TimestampedMidiEvent {
                                        timestamp_us: rel_time,
                                        data: message.to_vec(),
                                    }
                                ));
                            }
                        },
                        (),
                    ) {
                        Ok(conn) => {
                            self.capture_connections.push(conn);
                            println!("[Sacho] Connected to record device: {}", port_name);
                        }
                        Err(e) => {
                            println!("[Sacho] Failed to connect record {}: {}", port_name, e);
                        }
                    }
                }
            }
        }
        
        // Set up audio capture for selected devices
        println!("[Sacho] Audio devices: {:?}", config.selected_audio_devices);
        
        let host = cpal::default_host();
        let pre_roll_secs = config.pre_roll_secs.min(5);
        
        if let Ok(audio_devices) = host.input_devices() {
            for device in audio_devices {
                if let Ok(device_name) = device.name() {
                    // Check if this device is selected (audio devices use name as ID)
                    if config.selected_audio_devices.contains(&device_name) {
                        println!("[Sacho] Setting up audio capture: {}", device_name);
                        
                        if let Ok(supported_config) = device.default_input_config() {
                            let sample_rate = supported_config.sample_rate().0;
                            let channels = supported_config.channels();
                            
                            // Create audio buffer AND pre-roll buffer for this device
                            let buffer_index = {
                                let mut state = self.capture_state.lock();
                                
                                // Main recording buffer
                                state.audio_buffers.push(AudioBuffer {
                                    device_name: device_name.clone(),
                                    samples: VecDeque::new(),
                                    sample_rate,
                                    channels,
                                });
                                
                                // Pre-roll buffer (captures audio before trigger)
                                state.audio_prerolls.push(AudioPrerollBuffer::new(
                                    device_name.clone(),
                                    sample_rate,
                                    channels,
                                    pre_roll_secs,
                                ));
                                
                                state.audio_buffers.len() - 1
                            };
                            
                            let capture_state = self.capture_state.clone();
                            
                            match device.build_input_stream(
                                &supported_config.into(),
                                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                    let mut state = capture_state.lock();
                                    
                                    // Use pre-roll buffer when not recording (or still starting)
                                    if state.should_use_preroll() {
                                        // Store in pre-roll buffer (rolling window of last N seconds)
                                        if let Some(preroll) = state.audio_prerolls.get_mut(buffer_index) {
                                            preroll.push_samples(data);
                                        }
                                    } else {
                                        // Recording is active, store in main buffer
                                        if let Some(buffer) = state.audio_buffers.get_mut(buffer_index) {
                                            buffer.samples.extend(data.iter().copied());
                                        }
                                    }
                                },
                                |err| {
                                    println!("[Sacho] Audio error: {}", err);
                                },
                                None,
                            ) {
                                Ok(stream) => {
                                    if stream.play().is_ok() {
                                        AUDIO_STREAMS.with(|streams| {
                                            streams.borrow_mut().push(stream);
                                        });
                                        println!("[Sacho] Audio capture ready: {} ({}Hz, {}ch, {}s pre-roll)", 
                                            device_name, sample_rate, channels, pre_roll_secs);
                                    }
                                }
                                Err(e) => {
                                    println!("[Sacho] Failed to create audio stream for {}: {}", device_name, e);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        
        // Start video capture for selected video devices
        let video_count = {
            let selected_video = config.selected_video_devices.clone();
            let pre_roll = config.pre_roll_secs.min(5);
            drop(config); // Release config lock before video operations
            
            // Look up codec and name for each selected video device
            let device_manager = self.app_handle.state::<RwLock<DeviceManager>>();
            let devices = device_manager.read();
            
            // Get user-selected codecs from config
            let config = self.app_handle.state::<RwLock<Config>>();
            let config_read = config.read();
            let codec_overrides = &config_read.video_device_codecs;
            
            let video_with_info: Vec<(String, String, VideoCodec)> = selected_video
                .iter()
                .filter_map(|device_id| {
                    // Find the device
                    let device = devices.video_devices.iter().find(|d| &d.id == device_id)?;
                    
                    // Use user-selected codec if set, otherwise use preferred
                    let override_codec = codec_overrides.get(device_id).copied();
                    let preferred = device.preferred_codec();
                    
                    let codec = override_codec
                        .filter(|c| device.supported_codecs.contains(c))
                        .or(preferred);
                    
                    println!("[Sacho] Video device {}: override={:?}, preferred={:?}, using={:?}", 
                        device_id, override_codec, preferred, codec);
                    
                    let codec = codec?;
                    Some((device_id.clone(), device.name.clone(), codec))
                })
                .collect();
            
            // Get encoding mode for raw video
            let encoding_mode = config_read.video_encoding_mode.clone();
            
            drop(config_read);
            drop(devices); // Release device manager lock
            
            let mut video_mgr = self.video_manager.lock();
            video_mgr.set_preroll_duration(pre_roll);
            video_mgr.set_encoding_mode(encoding_mode);
            
            if !video_with_info.is_empty() {
                if let Err(e) = video_mgr.start(&video_with_info) {
                    println!("[Sacho] Failed to start video capture: {}", e);
                }
            }
            video_mgr.pipeline_count()
        };
        
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        let has_any_device = midi_count > 0 || audio_count > 0 || video_count > 0;
        
        if has_any_device {
            *self.is_monitoring.write() = true;
            
            // Only start idle checker if we have MIDI triggers (auto-stop on idle)
            if !self.trigger_connections.is_empty() {
                self.start_idle_checker();
            }
            
            // Start video polling thread
            if video_count > 0 {
                self.start_video_poller();
            }
            
            println!("[Sacho] Monitoring active ({} MIDI, {} audio, {} video)", 
                midi_count, audio_count, video_count);
        } else {
            println!("[Sacho] No devices configured");
        }
        
        Ok(())
    }
    
    /// Start background thread to poll video frames
    fn start_video_poller(&mut self) {
        let is_monitoring = self.is_monitoring.clone();
        let video_manager = self.video_manager.clone();
        
        let handle = std::thread::Builder::new()
            .name("sacho-video-poller".into())
            .spawn(move || {
                while *is_monitoring.read() {
                    {
                        let mut mgr = video_manager.lock();
                        mgr.poll();
                    }
                    std::thread::sleep(Duration::from_millis(10)); // Poll at ~100Hz
                }
            })
            .expect("Failed to spawn video poller thread");
        
        self.video_poller_handle = Some(handle);
    }
    
    /// Stop monitoring
    pub fn stop(&mut self) {
        // Signal background threads to stop
        *self.is_monitoring.write() = false;
        
        // Wait for background threads to finish (with timeout to avoid hanging)
        if let Some(handle) = self.video_poller_handle.take() {
            // Give the thread a moment to notice the flag change
            let _ = handle.join();
        }
        if let Some(handle) = self.idle_checker_handle.take() {
            let _ = handle.join();
        }
        
        self.trigger_connections.clear();
        self.capture_connections.clear();
        
        // Clear audio streams
        AUDIO_STREAMS.with(|streams| {
            streams.borrow_mut().clear();
        });
        
        // Stop video capture
        self.video_manager.lock().stop();
        
        // Clear audio buffers and pre-roll buffers
        let mut state = self.capture_state.lock();
        state.audio_buffers.clear();
        state.audio_prerolls.clear();
    }
    
    /// Manually start recording (same as MIDI trigger but without waiting for MIDI)
    pub fn manual_start_recording(&self) -> Result<(), String> {
        // Check that at least one device is active
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        let video_count = self.video_manager.lock().pipeline_count();
        
        if midi_count == 0 && audio_count == 0 && video_count == 0 {
            return Err("No devices selected. Configure at least one MIDI, audio, or video device before recording.".to_string());
        }
        
        // Atomically check and set is_starting to prevent race conditions
        {
            let mut state = self.capture_state.lock();
            if state.is_recording || state.is_starting {
                return Err("Already recording".to_string());
            }
            state.is_starting = true;
        }
        
        println!("[Sacho] Manual recording start requested");
        
        // Clear any stale idle timer so the idle checker doesn't immediately stop us.
        // Without this, a stale last_event_time from a previous MIDI event
        // can cause the idle checker to see "idle for > N seconds" and stop
        // the recording within 1 second of starting.
        // Setting to None means manual recordings run until explicitly stopped
        // (idle timeout only applies when MIDI events set last_event_time).
        *self.last_event_time.write() = None;
        
        // Start recording (synchronous for manual start so caller knows when it's ready)
        start_recording(&self.app_handle, &self.capture_state, &self.video_manager);
        
        Ok(())
    }
    
    /// Manually stop recording
    pub fn manual_stop_recording(&self) -> Result<(), String> {
        let is_recording = {
            let state = self.capture_state.lock();
            state.is_recording
        };
        
        if !is_recording {
            return Err("Not currently recording".to_string());
        }
        
        println!("[Sacho] Manual recording stop requested");
        stop_recording(&self.app_handle, &self.capture_state, &self.video_manager);
        
        Ok(())
    }
    
    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.capture_state.lock().is_recording
    }
    
    /// Start idle timeout checker thread
    fn start_idle_checker(&mut self) {
        let app_handle = self.app_handle.clone();
        let last_event_time = self.last_event_time.clone();
        let is_monitoring = self.is_monitoring.clone();
        let capture_state = self.capture_state.clone();
        let video_manager = self.video_manager.clone();
        
        let handle = std::thread::Builder::new()
            .name("sacho-idle-checker".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    
                    if !*is_monitoring.read() {
                        break;
                    }
                    
                    let config = app_handle.state::<RwLock<Config>>();
                    let idle_timeout = config.read().idle_timeout_secs;
                    
                    let is_recording = capture_state.lock().is_recording;
                    
                    if is_recording {
                        if let Some(last_time) = *last_event_time.read() {
                            if last_time.elapsed() >= Duration::from_secs(idle_timeout as u64) {
                                println!("[Sacho] Idle timeout ({} sec), stopping recording", idle_timeout);
                                stop_recording(&app_handle, &capture_state, &video_manager);
                            }
                        }
                    }
                }
            })
            .expect("Failed to spawn idle checker thread");
        
        self.idle_checker_handle = Some(handle);
    }
}

impl Drop for MidiMonitor {
    fn drop(&mut self) {
        // Ensure monitoring is stopped and resources are cleaned up
        self.stop();
    }
}

/// Handle MIDI trigger event
fn handle_midi_trigger(
    app_handle: &AppHandle, 
    last_event_time: &Arc<RwLock<Option<Instant>>>,
    capture_state: &Arc<Mutex<CaptureState>>,
    video_manager: &Arc<Mutex<VideoCaptureManager>>,
) {
    // Update last event time
    *last_event_time.write() = Some(Instant::now());
    
    // Check if the global recording state allows starting
    // (e.g., we're not in Initializing mode from a device config change)
    {
        let recording_state = app_handle.state::<RwLock<crate::recording::RecordingState>>();
        let state = recording_state.read();
        if state.status == crate::recording::RecordingStatus::Initializing {
            // Silently ignore MIDI triggers during device reinitialization
            return;
        }
    }
    
    // Atomically check and set is_starting to prevent race conditions
    let should_start = {
        let mut state = capture_state.lock();
        if state.is_recording || state.is_starting {
            false
        } else {
            state.is_starting = true;
            true
        }
    };
    
    if should_start {
        println!("[Sacho] MIDI trigger -> starting recording (async)");
        
        // Spawn recording start on a separate thread so MIDI callback isn't blocked
        // This allows pre-roll to continue capturing during video initialization
        let app_handle = app_handle.clone();
        let capture_state = capture_state.clone();
        let video_manager = video_manager.clone();
        std::thread::spawn(move || {
            start_recording(&app_handle, &capture_state, &video_manager);
        });
    }
}

/// Start recording
fn start_recording(
    app_handle: &AppHandle, 
    capture_state: &Arc<Mutex<CaptureState>>,
    video_manager: &Arc<Mutex<VideoCaptureManager>>,
) {
    let config = app_handle.state::<RwLock<Config>>();
    let config_read = config.read().clone();
    
    let timestamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let session_path = config_read.storage_path.join(&timestamp);
    
    if let Err(e) = std::fs::create_dir_all(&session_path) {
        println!("[Sacho] Failed to create session folder: {}", e);
        // Reset is_starting flag so future recording attempts can work
        capture_state.lock().is_starting = false;
        return;
    }
    
    // Capture the instant BEFORE video starts - this is our sync reference point
    // The video pre-roll duration is relative to this instant
    let video_start_instant = Instant::now();
    
    // Start video recording (this captures pre-roll and begins file writing)
    let video_preroll_duration = {
        let mut mgr = video_manager.lock();
        match mgr.start_recording(&session_path) {
            Ok(duration) => {
                println!("[Sacho] Video recording started with {:?} pre-roll", duration);
                Some(duration)
            }
            Err(e) => {
                println!("[Sacho] Failed to start video recording: {}", e);
                None
            }
        }
    };
    
    // Capture a single trigger instant for consistent timing across all streams
    let trigger_instant = Instant::now();
    
    // Initialize capture state and drain pre-roll buffers
    {
        let mut state = capture_state.lock();
        
        // Calculate the actual audio pre-roll duration from the first audio buffer
        // This tells us how much audio we captured before the trigger
        let configured_preroll = Duration::from_secs(state.pre_roll_secs as u64);
        let audio_preroll_duration = state.audio_prerolls.first().map(|_preroll| {
            configured_preroll
        });
        
        // SYNC FIX: Calculate the correct audio pre-roll to align with video
        // 
        // video_preroll_duration = time from first video frame capture to when video.rs STARTED
        // (measured using first_frame.wall_time.elapsed() at the moment video processing began)
        // 
        // delay_since_video_start = time elapsed from when video started to NOW
        // This includes the time video took to process AND any time to reach this point
        //
        // Total audio pre-roll = video_preroll + delay_since_video_start
        // This ensures the first video frame and first audio sample represent the same moment
        let delay_since_video_start = video_start_instant.elapsed();
        
        let sync_preroll_duration = match (audio_preroll_duration, video_preroll_duration) {
            (Some(audio_dur), Some(video_dur)) => {
                // Add the delay since video STARTED to get the correct audio pre-roll
                // This accounts for the ~340ms that video processing takes
                let adjusted_video_dur = video_dur + delay_since_video_start;
                // Use the minimum to avoid requesting more audio than we have
                let sync_dur = audio_dur.min(adjusted_video_dur);
                
                println!("[Sacho] SYNC: video_preroll={:?}, delay={:?}, adjusted={:?}, audio={:?}, using={:?}", 
                    video_dur, delay_since_video_start, adjusted_video_dur, audio_dur, sync_dur);
                Some(sync_dur)
            }
            (Some(audio_dur), None) => Some(audio_dur), // No video, use audio
            (None, Some(video_dur)) => Some(video_dur + delay_since_video_start), // No audio, use adjusted video
            (None, None) => None,
        };
        
        // Drain pre-roll MIDI buffer with sync duration
        // This ensures MIDI timestamps align with the synchronized pre-roll start
        let preroll_events = state.midi_preroll.drain_with_audio_sync(sync_preroll_duration);
        let midi_preroll_count = preroll_events.len();
        
        // Add pre-roll MIDI events first
        state.midi_events.clear();
        for (device_name, event) in preroll_events {
            state.midi_events.push((device_name, event));
        }
        
        // Drain audio pre-roll buffers into main buffers
        // Pre-roll audio goes BEFORE any new samples
        // Note: Audio samples collected after trigger_instant will be added during recording
        // SYNC FIX: Only drain samples matching the sync duration (not full audio buffer)
        let mut audio_preroll_samples = 0;
        for i in 0..state.audio_prerolls.len() {
            let preroll_samples = if let Some(sync_dur) = sync_preroll_duration {
                // Drain only the samples that match the sync duration
                state.audio_prerolls[i].drain_duration(sync_dur)
            } else {
                state.audio_prerolls[i].drain()
            };
            audio_preroll_samples += preroll_samples.len();
            
            if let Some(buffer) = state.audio_buffers.get_mut(i) {
                // Clear main buffer and add pre-roll samples first
                buffer.samples.clear();
                buffer.samples.extend(preroll_samples.into_iter());
            }
        }
        
        // Set the session path and start time to the same trigger instant
        state.session_path = Some(session_path.clone());
        state.start_time = Some(trigger_instant);
        
        // Set MIDI timestamp offset to sync_preroll_duration
        // Real-time MIDI events need this offset added to align with pre-roll content
        state.midi_timestamp_offset_us = sync_preroll_duration
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        
        // Switch from "starting" to "recording" - now new events go directly to midi_events
        state.is_starting = false;
        state.is_recording = true;
        
        println!("[Sacho] Recording started with {} pre-roll MIDI events, {} pre-roll audio samples (sync pre-roll: {:?})", 
            midi_preroll_count, audio_preroll_samples, sync_preroll_duration);
    }
    
    // Update recording state
    let active_devices = {
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.status = crate::recording::RecordingStatus::Recording;
        state.started_at = Some(chrono::Utc::now());
        state.current_session_path = Some(session_path.clone());
        state.active_midi_devices = config_read.selected_midi_devices.clone();
        state.active_audio_devices = config_read.selected_audio_devices.clone();
        state.active_video_devices = config_read.selected_video_devices.clone();
        
        // Collect device names for notification
        let mut devices = state.active_midi_devices.clone();
        devices.extend(state.active_audio_devices.clone());
        devices.extend(state.active_video_devices.clone());
        devices
    };
    
    // Send desktop notification
    if config_read.notify_recording_start {
        notifications::notify_recording_started(app_handle, &active_devices);
    }
    
    let _ = app_handle.emit("recording-started", session_path.to_string_lossy().to_string());
    println!("[Sacho] Recording started: {:?}", session_path);
}

/// Collected audio data from a buffer
struct CollectedAudio {
    device_name: String,
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

/// Stop recording and save files
fn stop_recording(
    app_handle: &AppHandle, 
    capture_state: &Arc<Mutex<CaptureState>>,
    video_manager: &Arc<Mutex<VideoCaptureManager>>,
) {
    // First, extract what we need from capture_state
    let (session_path, midi_events, audio_data, duration_secs) = {
        let mut state = capture_state.lock();
        if !state.is_recording {
            return;
        }
        
        let duration = state.start_time
            .map(|st| st.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        
        let path = state.session_path.take();
        let events = std::mem::take(&mut state.midi_events);
        
        // Collect audio samples from buffers (aligned to frame boundary)
        let audio: Vec<CollectedAudio> = state.audio_buffers.iter_mut().map(|buf| {
            // Ensure sample count is a multiple of channels to prevent WAV write errors
            let total = buf.samples.len();
            let aligned = (total / buf.channels as usize) * buf.channels as usize;
            if aligned < total {
                // Truncate to drop partial frame
                buf.samples.truncate(aligned);
            }
            CollectedAudio {
                device_name: buf.device_name.clone(),
                samples: buf.samples.drain(..).collect(),
                sample_rate: buf.sample_rate,
                channels: buf.channels,
            }
        }).collect();
        
        state.is_recording = false;
        state.is_starting = false;
        state.start_time = None;
        state.midi_timestamp_offset_us = 0;
        
        (path, events, audio, duration)
    };
    
    let Some(session_path) = session_path else {
        // Even if no session path, update recording state to idle
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.status = crate::recording::RecordingStatus::Idle;
        state.started_at = None;
        state.current_session_path = None;
        state.elapsed_seconds = 0;
        state.active_midi_devices.clear();
        state.active_audio_devices.clear();
        state.active_video_devices.clear();
        return;
    };
    
    // Update recording state to idle immediately (before slow file operations)
    {
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.status = crate::recording::RecordingStatus::Idle;
        state.started_at = None;
        state.current_session_path = Some(session_path.clone());
        state.elapsed_seconds = 0;
        // Keep device info for now, will be cleared after save
    }
    
    // Stop video recording and get video files
    let video_files = {
        let mut mgr = video_manager.lock();
        mgr.stop_recording()
    };
    
    println!("[Sacho] Stopping recording, {} MIDI events, {} audio streams, {} video files", 
        midi_events.len(), audio_data.len(), video_files.len());
    
    // Write MIDI files (one per device)
    let midi_files = if !midi_events.is_empty() {
        write_midi_files(&session_path, &midi_events)
    } else {
        Vec::new()
    };
    
    // Write audio files based on configured format
    let config = app_handle.state::<RwLock<Config>>();
    let audio_format = config.read().audio_format.clone();
    
    let mut audio_files = Vec::new();
    for (i, audio) in audio_data.iter().enumerate() {
        if audio.samples.is_empty() {
            continue;
        }
        
        let extension = match audio_format {
            crate::config::AudioFormat::Wav => "wav",
            crate::config::AudioFormat::Flac => "flac",
            crate::config::AudioFormat::Opus => "opus",
        };
        
        let filename = if audio_data.len() == 1 {
            format!("recording.{}", extension)
        } else {
            format!("recording_{}.{}", i + 1, extension)
        };
        
        let result = match audio_format {
            crate::config::AudioFormat::Wav => write_wav_file(&session_path, &filename, audio),
            crate::config::AudioFormat::Flac => write_flac_file(&session_path, &filename, audio),
            crate::config::AudioFormat::Opus => write_opus_file(&session_path, &filename, audio),
        };
        
        match result {
            Ok(info) => {
                println!("[Sacho] Wrote audio: {} ({} samples)", filename, audio.samples.len());
                audio_files.push(info);
            }
            Err(e) => {
                println!("[Sacho] Failed to write audio {}: {}", filename, e);
            }
        }
    }
    
    // Calculate max duration across all streams for padding
    let audio_max_duration = audio_files.iter()
        .map(|f| f.duration_secs)
        .fold(0.0f64, |a, b| a.max(b));
    
    let video_max_duration = video_files.iter()
        .map(|f| f.duration_secs)
        .fold(0.0f64, |a, b| a.max(b));
    
    let target_duration = duration_secs.max(audio_max_duration).max(video_max_duration);
    
    // Pad audio files if needed (append silence to match target duration)
    for audio_info in audio_files.iter_mut() {
        if audio_info.duration_secs < target_duration - 0.1 {
            let padding_secs = target_duration - audio_info.duration_secs;
            if let Err(e) = pad_audio_file(&session_path, audio_info, padding_secs) {
                println!("[Sacho] Failed to pad audio file {}: {}", audio_info.filename, e);
            } else {
                println!("[Sacho] Padded audio {} with {:.2}s of silence", 
                    audio_info.filename, padding_secs);
            }
        }
    }
    
    // Update overall duration to match the longest stream
    let duration_secs = target_duration;
    
    // Clear remaining recording state (session path and devices)
    {
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.current_session_path = None;
        state.active_midi_devices.clear();
        state.active_audio_devices.clear();
        state.active_video_devices.clear();
    }
    
    // Create and save metadata
    // Use folder name as session ID (for consistency with similarity calculation)
    let session_id = session_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    
    let metadata = SessionMetadata {
        id: session_id,
        timestamp: chrono::Utc::now(),
        duration_secs,
        path: session_path.clone(),
        audio_files,
        midi_files,
        video_files,
        tags: Vec::new(),
        notes: String::new(),
        is_favorite: false,
        midi_features: None,
        similarity_coords: None,
        cluster_id: None,
    };
    
    if let Err(e) = crate::session::save_metadata(&metadata) {
        println!("[Sacho] Failed to save metadata: {}", e);
    }
    
    let db = app_handle.state::<SessionDatabase>();
    if let Err(e) = db.upsert_session(&metadata) {
        println!("[Sacho] Failed to index session: {}", e);
    }
    
    // Send desktop notification
    let config = app_handle.state::<RwLock<Config>>();
    if config.read().notify_recording_stop {
        let folder_name = session_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("session");
        notifications::notify_recording_stopped(app_handle, duration_secs, folder_name);
    }
    
    let _ = app_handle.emit("recording-stopped", serde_json::to_string(&metadata).unwrap_or_default());
    println!("[Sacho] Recording stopped, duration: {} sec", duration_secs);
}

/// Write MIDI events to a Standard MIDI File
fn write_midi_files(session_path: &PathBuf, events: &[(String, TimestampedMidiEvent)]) -> Vec<MidiFileInfo> {
    use std::collections::HashMap;
    
    // Group events by device name
    let mut events_by_device: HashMap<&str, Vec<&TimestampedMidiEvent>> = HashMap::new();
    for (device_name, event) in events {
        events_by_device.entry(device_name.as_str()).or_default().push(event);
    }
    
    let mut midi_files = Vec::new();
    let device_count = events_by_device.len();
    
    for (device_name, device_events) in events_by_device.into_iter() {
        // Create safe filename from device name
        let safe_name = device_name
            .replace(" ", "_")
            .replace("/", "_")
            .replace("\\", "_")
            .replace(":", "_");
        
        let filename = if device_count == 1 {
            "recording.mid".to_string()
        } else {
            format!("midi_{}.mid", safe_name)
        };
        
        let midi_path = session_path.join(&filename);
        
        match write_single_midi_file(&midi_path, &device_events) {
            Ok(size) => {
                midi_files.push(MidiFileInfo {
                    filename,
                    device_name: device_name.to_string(),
                    event_count: device_events.len(),
                    size_bytes: size,
                });
            }
            Err(e) => {
                println!("[Sacho] Failed to write MIDI for {}: {}", device_name, e);
            }
        }
    }
    
    midi_files
}

/// Write a single MIDI file for one device
fn write_single_midi_file(midi_path: &PathBuf, events: &[&TimestampedMidiEvent]) -> anyhow::Result<u64> {
    let mut file = std::fs::File::create(midi_path)?;
    
    // MIDI Header: MThd
    file.write_all(b"MThd")?;
    file.write_all(&[0, 0, 0, 6])?; // Header length
    file.write_all(&[0, 0])?; // Format 0
    file.write_all(&[0, 1])?; // 1 track
    file.write_all(&[0x01, 0xE0])?; // 480 ticks per quarter note
    
    // Build track data
    let mut track_data: Vec<u8> = Vec::new();
    let ticks_per_us = 480.0 / 500000.0; // Assuming 120 BPM (500000 us per beat)
    
    let mut last_tick: u64 = 0;
    
    for event in events {
        let tick = (event.timestamp_us as f64 * ticks_per_us) as u64;
        let delta = tick.saturating_sub(last_tick);
        last_tick = tick;
        
        // Write variable-length delta time
        write_variable_length(&mut track_data, delta as u32);
        
        // Write MIDI event data
        track_data.extend_from_slice(&event.data);
    }
    
    // End of track
    write_variable_length(&mut track_data, 0);
    track_data.extend_from_slice(&[0xFF, 0x2F, 0x00]);
    
    // Write track chunk
    file.write_all(b"MTrk")?;
    let track_len = track_data.len() as u32;
    file.write_all(&track_len.to_be_bytes())?;
    file.write_all(&track_data)?;
    
    let size = std::fs::metadata(midi_path)?.len();
    Ok(size)
}

/// Write variable-length quantity for MIDI
fn write_variable_length(data: &mut Vec<u8>, mut value: u32) {
    let mut bytes = Vec::new();
    bytes.push((value & 0x7F) as u8);
    value >>= 7;
    
    while value > 0 {
        bytes.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
    
    bytes.reverse();
    data.extend(bytes);
}

/// Write audio samples to a WAV file
fn write_wav_file(
    session_path: &PathBuf, 
    filename: &str, 
    audio: &CollectedAudio
) -> anyhow::Result<AudioFileInfo> {
    let wav_path = session_path.join(filename);
    
    let spec = hound::WavSpec {
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    
    let mut writer = hound::WavWriter::create(&wav_path, spec)?;
    
    for sample in &audio.samples {
        writer.write_sample(*sample)?;
    }
    
    writer.finalize()?;
    
    let size = std::fs::metadata(&wav_path)?.len();
    let duration_secs = audio.samples.len() as f64 / (audio.sample_rate as f64 * audio.channels as f64);
    
    Ok(AudioFileInfo {
        filename: filename.to_string(),
        device_name: audio.device_name.clone(),
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        duration_secs,
        size_bytes: size,
    })
}

/// Write audio samples to a FLAC file using flacenc (pure Rust)
fn write_flac_file(
    session_path: &PathBuf, 
    filename: &str, 
    audio: &CollectedAudio
) -> anyhow::Result<AudioFileInfo> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use flacenc::bitsink::ByteSink;
    
    let flac_path = session_path.join(filename);
    
    // Convert f32 samples to i32 (24-bit range for professional quality)
    let samples_i32: Vec<i32> = audio.samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32)
        .collect();
    
    // Create FLAC encoder config
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| anyhow::anyhow!("FLAC config error: {:?}", e))?;
    
    // Create source from samples
    let source = flacenc::source::MemSource::from_samples(
        &samples_i32,
        audio.channels as usize,
        24, // bits per sample
        audio.sample_rate as usize,
    );
    
    // Encode to FLAC stream
    let flac_stream = flacenc::encode_with_fixed_block_size(
        &config,
        source,
        config.block_size,
    ).map_err(|e| anyhow::anyhow!("FLAC encoding error: {:?}", e))?;
    
    // Write to ByteSink using BitRepr trait
    let mut sink = ByteSink::new();
    let _ = flac_stream.write(&mut sink);
    
    // Write to file
    std::fs::write(&flac_path, sink.as_slice())?;
    
    let size = std::fs::metadata(&flac_path)?.len();
    let duration_secs = audio.samples.len() as f64 / (audio.sample_rate as f64 * audio.channels as f64);
    
    Ok(AudioFileInfo {
        filename: filename.to_string(),
        device_name: audio.device_name.clone(),
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        duration_secs,
        size_bytes: size,
    })
}

/// Write audio samples to an Opus file in an Ogg container (256 kbps)
fn write_opus_file(
    session_path: &PathBuf,
    filename: &str,
    audio: &CollectedAudio,
) -> anyhow::Result<AudioFileInfo> {
    use ogg::writing::{PacketWriter, PacketWriteEndInfo};

    let opus_path = session_path.join(filename);

    // Opus only supports specific sample rates: 8000, 12000, 16000, 24000, 48000 Hz.
    // Resample to the nearest supported rate (almost always 48000).
    let target_rate = nearest_opus_sample_rate(audio.sample_rate);
    let channels = audio.channels.min(2); // Opus simple API supports mono/stereo

    let samples = if audio.sample_rate != target_rate {
        resample_linear(&audio.samples, audio.sample_rate, target_rate, audio.channels)
    } else {
        audio.samples.clone()
    };

    // Create Opus encoder at 256 kbps
    let opus_channels = if channels == 1 {
        opus::Channels::Mono
    } else {
        opus::Channels::Stereo
    };
    let mut encoder = opus::Encoder::new(target_rate, opus_channels, opus::Application::Audio)
        .map_err(|e| anyhow::anyhow!("Opus encoder init error: {}", e))?;
    encoder
        .set_bitrate(opus::Bitrate::Bits(256_000))
        .map_err(|e| anyhow::anyhow!("Opus set bitrate error: {}", e))?;

    // Pre-skip: number of samples the decoder must discard at the start
    let pre_skip = encoder
        .get_lookahead()
        .map_err(|e| anyhow::anyhow!("Opus get lookahead error: {}", e))? as u16;

    // Frame size: 20 ms of audio per channel (960 samples at 48 kHz)
    let frame_size = (target_rate as usize * 20) / 1000;
    let frame_samples = frame_size * channels as usize;

    let file = std::fs::File::create(&opus_path)?;
    let mut packet_writer = PacketWriter::new(file);
    let serial: u32 = 1;

    // --- OpusHead identification header (RFC 7845 §5.1) ---
    let opus_head = build_opus_head(channels as u8, pre_skip, audio.sample_rate);
    packet_writer.write_packet(
        opus_head,
        serial,
        PacketWriteEndInfo::EndPage,
        0,
    )?;

    // --- OpusTags comment header (RFC 7845 §5.2) ---
    let opus_tags = build_opus_tags();
    packet_writer.write_packet(
        opus_tags,
        serial,
        PacketWriteEndInfo::EndPage,
        0,
    )?;

    // --- Audio data packets ---
    let mut granule_pos: u64 = 0;
    let mut output_buf = vec![0u8; 4000]; // Max Opus packet size
    let mut pos = 0;

    while pos < samples.len() {
        // Prepare one frame, zero-padding the final frame if needed
        let remaining = samples.len() - pos;
        let input: Vec<f32> = if remaining < frame_samples {
            let mut padded = samples[pos..].to_vec();
            padded.resize(frame_samples, 0.0);
            padded
        } else {
            samples[pos..pos + frame_samples].to_vec()
        };

        let encoded_len = encoder
            .encode_float(&input, &mut output_buf)
            .map_err(|e| anyhow::anyhow!("Opus encode error: {}", e))?;

        granule_pos += frame_size as u64;

        let is_last = pos + frame_samples >= samples.len();
        let end_info = if is_last {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };

        packet_writer.write_packet(
            output_buf[..encoded_len].to_vec(),
            serial,
            end_info,
            granule_pos + pre_skip as u64,
        )?;

        pos += frame_samples;
    }

    let size = std::fs::metadata(&opus_path)?.len();
    let duration_secs =
        audio.samples.len() as f64 / (audio.sample_rate as f64 * audio.channels as f64);

    Ok(AudioFileInfo {
        filename: filename.to_string(),
        device_name: audio.device_name.clone(),
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        duration_secs,
        size_bytes: size,
    })
}

/// Build the OpusHead identification header per RFC 7845 §5.1
fn build_opus_head(channels: u8, pre_skip: u16, original_sample_rate: u32) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");   // Magic signature
    head.push(1);                          // Version
    head.push(channels);                   // Channel count
    head.extend_from_slice(&pre_skip.to_le_bytes());           // Pre-skip
    head.extend_from_slice(&original_sample_rate.to_le_bytes()); // Input sample rate
    head.extend_from_slice(&0i16.to_le_bytes());               // Output gain (0 dB)
    head.push(0);                          // Channel mapping family (0 = mono/stereo)
    head
}

/// Build the OpusTags comment header per RFC 7845 §5.2
fn build_opus_tags() -> Vec<u8> {
    let vendor = b"Sacho";
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes()); // No user comments
    tags
}

/// Return the nearest Opus-supported sample rate for the given input rate.
/// Opus supports: 8000, 12000, 16000, 24000, 48000 Hz.
fn nearest_opus_sample_rate(sample_rate: u32) -> u32 {
    const OPUS_RATES: [u32; 5] = [8000, 12000, 16000, 24000, 48000];
    *OPUS_RATES
        .iter()
        .min_by_key(|&&r| (r as i64 - sample_rate as i64).abs())
        .unwrap()
}

/// Simple linear-interpolation resampler for converting between sample rates.
/// Sufficient for lossy Opus encoding; avoids an extra dependency.
fn resample_linear(
    samples: &[f32],
    from_rate: u32,
    to_rate: u32,
    channels: u16,
) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ch = channels as usize;
    let num_frames = samples.len() / ch;
    let ratio = to_rate as f64 / from_rate as f64;
    let new_num_frames = (num_frames as f64 * ratio).ceil() as usize;
    let mut output = Vec::with_capacity(new_num_frames * ch);

    for i in 0..new_num_frames {
        let src_pos = i as f64 / ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let idx1 = idx.min(num_frames - 1);
        let idx2 = (idx + 1).min(num_frames - 1);

        for c in 0..ch {
            let s1 = samples[idx1 * ch + c];
            let s2 = samples[idx2 * ch + c];
            output.push(s1 + (s2 - s1) * frac);
        }
    }
    output
}

/// Pad an audio file with silence to extend its duration
/// For WAV files, we append silence samples
/// For FLAC/Opus files, we skip padding (would require decode/re-encode)
fn pad_audio_file(
    session_path: &PathBuf,
    audio_info: &mut AudioFileInfo,
    padding_secs: f64,
) -> anyhow::Result<()> {
    let file_path = session_path.join(&audio_info.filename);
    
    // Calculate number of silence samples needed
    let silence_samples = (padding_secs * audio_info.sample_rate as f64 * audio_info.channels as f64) as usize;
    
    if audio_info.filename.ends_with(".wav") {
        // For WAV, we can read, extend, and rewrite
        let mut reader = hound::WavReader::open(&file_path)?;
        let spec = reader.spec();
        
        // Collect existing samples
        let mut samples: Vec<f32> = reader.samples::<f32>()
            .filter_map(|s| s.ok())
            .collect();
        
        // Add silence
        samples.extend(std::iter::repeat(0.0f32).take(silence_samples));
        
        drop(reader); // Close the reader before writing
        
        // Rewrite file with extended content
        let mut writer = hound::WavWriter::create(&file_path, spec)?;
        for sample in &samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        
        // Update audio info
        let new_size = std::fs::metadata(&file_path)?.len();
        audio_info.duration_secs += padding_secs;
        audio_info.size_bytes = new_size;
        
    } else if audio_info.filename.ends_with(".flac") {
        // For FLAC, we would need to decode, add silence, and re-encode
        // This is expensive and not implemented yet
        println!("[Sacho] FLAC padding not implemented, audio may be shorter than video");
    } else if audio_info.filename.ends_with(".opus") {
        // For Opus, we would need to decode, add silence, and re-encode
        // This is expensive and not implemented yet
        println!("[Sacho] Opus padding not implemented, audio may be shorter than video");
    }
    
    Ok(())
}
