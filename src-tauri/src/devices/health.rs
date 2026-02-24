// Device health monitoring — detects disconnected devices and triggers reconnection

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::Config;
use crate::devices::DeviceManager;
use crate::notifications;
use crate::recording::video::VideoCaptureManager;

/// Info about a disconnected device, sent to the frontend
#[derive(Debug, Clone, Serialize)]
pub struct DisconnectedDeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String, // "midi", "audio", "video"
}

/// Managed state holding the current set of disconnected devices
pub struct DeviceHealthState {
    pub disconnected: HashMap<String, DisconnectedDeviceInfo>,
}

impl DeviceHealthState {
    pub fn new() -> Self {
        Self {
            disconnected: HashMap::new(),
        }
    }
}

impl Default for DeviceHealthState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Lightweight enumerators (names only, for health checks)
// ============================================================================

/// Enumerate currently-connected MIDI port names (lightweight, no device details).
fn enumerate_midi_port_names() -> HashSet<String> {
    let mut names = HashSet::new();
    if let Ok(midi_in) = midir::MidiInput::new("sacho-health") {
        let ports = midi_in.ports();
        for port in &ports {
            if let Ok(name) = midi_in.port_name(port) {
                names.insert(name);
            }
        }
    }
    names
}

/// Enumerate currently-connected audio input device names (lightweight).
fn enumerate_audio_device_names() -> HashSet<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let mut names = HashSet::new();
    let host = cpal::default_host();
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                names.insert(name);
            }
        }
    }
    names
}

/// Enumerate currently-connected video device names via GStreamer DeviceMonitor.
/// Lightweight — names only, no capability probing or resolution sampling.
/// Only called when there are disconnected video devices (zero overhead otherwise).
fn enumerate_video_device_names() -> HashSet<String> {
    use gstreamer::prelude::*;
    gstreamer::init().ok();
    let monitor = gstreamer::DeviceMonitor::new();
    monitor.add_filter(Some("Video/Source"), None);
    monitor.add_filter(Some("Source/Video"), None);
    if monitor.start().is_err() {
        return HashSet::new();
    }
    let devices = monitor.devices();
    monitor.stop();
    let mut names = HashSet::new();
    for device in devices {
        names.insert(device.display_name().to_string());
    }
    names
}

// ============================================================================
// Health check core logic
// ============================================================================

/// Check which active MIDI and audio devices are currently disconnected.
/// Video is NOT checked here — it uses frame counter stall detection in the
/// health check loop instead, avoiding expensive GStreamer DeviceMonitor calls.
///
/// Returns a set of device IDs that are disconnected.
pub fn check_active_device_health(app: &AppHandle) -> HashSet<String> {
    let config = app.state::<RwLock<Config>>();
    let config = config.read();
    let device_manager = app.state::<RwLock<DeviceManager>>();
    let dm = device_manager.read();

    // Collect all active device IDs by type
    let mut active_midi_ids: HashSet<String> = HashSet::new();
    for id in config
        .selected_midi_devices
        .iter()
        .chain(config.trigger_midi_devices.iter())
    {
        active_midi_ids.insert(id.clone());
    }

    let mut active_audio_ids: HashSet<String> = HashSet::new();
    for id in config
        .selected_audio_devices
        .iter()
        .chain(config.trigger_audio_devices.iter())
    {
        active_audio_ids.insert(id.clone());
    }

    // Early exit if nothing is active
    if active_midi_ids.is_empty() && active_audio_ids.is_empty() {
        return HashSet::new();
    }

    let mut disconnected = HashSet::new();

    // Check MIDI devices: IDs are "midi-{index}", names come from DeviceManager cache.
    // We enumerate current port names and check if the cached name is still present.
    if !active_midi_ids.is_empty() {
        let current_midi_names = enumerate_midi_port_names();
        for id in &active_midi_ids {
            // Find the cached name for this ID
            if let Some(device) = dm.midi_devices.iter().find(|d| &d.id == id) {
                if !current_midi_names.contains(&device.name) {
                    disconnected.insert(id.clone());
                }
            }
            // If the device isn't in the cache at all, it's stale — treat as disconnected
            else {
                disconnected.insert(id.clone());
            }
        }
    }

    // Check audio devices: IDs = device names
    if !active_audio_ids.is_empty() {
        let current_audio_names = enumerate_audio_device_names();
        for id in &active_audio_ids {
            if !current_audio_names.contains(id) {
                disconnected.insert(id.clone());
            }
        }
    }

    disconnected
}

/// Resolve a device ID to a `DisconnectedDeviceInfo` using the DeviceManager cache.
fn resolve_device_info(
    id: &str,
    dm: &DeviceManager,
    config: &Config,
) -> Option<DisconnectedDeviceInfo> {
    // Check MIDI
    if let Some(device) = dm.midi_devices.iter().find(|d| d.id == id) {
        return Some(DisconnectedDeviceInfo {
            id: id.to_string(),
            name: device.name.clone(),
            device_type: "midi".to_string(),
        });
    }
    // Check audio (ID = name)
    if config.selected_audio_devices.contains(&id.to_string())
        || config.trigger_audio_devices.contains(&id.to_string())
    {
        return Some(DisconnectedDeviceInfo {
            id: id.to_string(),
            name: id.to_string(),
            device_type: "audio".to_string(),
        });
    }
    // Check video
    if let Some(device) = dm.video_devices.iter().find(|d| d.id == id) {
        return Some(DisconnectedDeviceInfo {
            id: id.to_string(),
            name: device.name.clone(),
            device_type: "video".to_string(),
        });
    }
    None
}

// ============================================================================
// Health check background thread
// ============================================================================

/// Event payload for `device-health-changed`
#[derive(Clone, Serialize)]
struct DeviceHealthChangedPayload {
    disconnected_devices: Vec<DisconnectedDeviceInfo>,
}

/// Event payload for `_device-needs-restart` (internal, triggers frontend round-trip)
#[derive(Clone, Serialize)]
struct DeviceNeedsRestartPayload {
    device_types: Vec<String>,
}

/// Video stall detection state for one pipeline
struct VideoStallState {
    last_frame_count: u64,
    /// True once we've seen at least one frame (avoids false positives during startup)
    has_seen_frames: bool,
    /// How many consecutive health checks the counter hasn't changed
    stall_ticks: u32,
}

/// Background thread that polls device health every 1 second.
///
/// MIDI and audio are checked via lightweight re-enumeration.
/// Video is checked via frame counter stall detection (zero overhead — just
/// reads an AtomicU64 that the appsink callback increments on every frame).
pub fn health_check_loop(
    app: AppHandle,
    capture_state: Arc<Mutex<crate::recording::monitor::CaptureState>>,
    video_manager: Arc<Mutex<VideoCaptureManager>>,
    stop_flag: Arc<AtomicBool>,
) {
    let mut previous_disconnected: HashSet<String> = HashSet::new();
    // Video frame counter tracking: device_id -> stall state
    let mut video_stall: HashMap<String, VideoStallState> = HashMap::new();
    // Tick counter for rate-limiting video reconnection enumeration
    let mut tick_count: u32 = 0;

    println!("[Health] Device health checker started");

    while !stop_flag.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_secs(1));

        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        // Check MIDI + audio via enumeration
        let mut current_disconnected = check_active_device_health(&app);

        // Skip video stall detection while pipelines are intentionally stopped
        // (e.g. during encoder test or auto-select). The test commands set status
        // to Initializing before stopping pipelines and reset to Idle afterward.
        let is_initializing = {
            let rs = app.state::<RwLock<crate::recording::RecordingState>>();
            let status = rs.read().status.clone();
            status == crate::recording::RecordingStatus::Initializing
        };

        // Check video via frame counter stall detection
        if is_initializing {
            // Reset stall state so restarted pipelines get a clean slate
            video_stall.clear();
        } else {
            let config = app.state::<RwLock<Config>>();
            let config = config.read();
            let active_video_ids: HashSet<String> =
                config.selected_video_devices.iter().cloned().collect();

            if !active_video_ids.is_empty() {
                let frame_counts = video_manager.lock().get_frame_counts();

                // Clean up stall state for devices no longer active
                video_stall.retain(|id, _| active_video_ids.contains(id));

                for id in &active_video_ids {
                    let count = frame_counts.get(id).copied().unwrap_or(0);
                    let state = video_stall.entry(id.clone()).or_insert(VideoStallState {
                        last_frame_count: 0,
                        has_seen_frames: false,
                        stall_ticks: 0,
                    });

                    if count > state.last_frame_count {
                        // Frames are flowing — device is healthy
                        state.has_seen_frames = true;
                        state.stall_ticks = 0;
                    } else if state.has_seen_frames {
                        // Counter hasn't changed but we've seen frames before — stalling
                        state.stall_ticks += 1;
                    }
                    // If we haven't seen frames yet (pipeline still starting), don't count stalls

                    state.last_frame_count = count;

                    // Device is disconnected if stalled for 3+ seconds
                    if state.stall_ticks >= 3 {
                        current_disconnected.insert(id.clone());
                    }

                    // If device has no pipeline at all (not in frame_counts),
                    // it might have been removed — but that's handled by the
                    // config cleanup, not health checks
                }
            }
        }

        // Video reconnection detection: for devices already known to be disconnected,
        // use periodic GStreamer enumeration to check if they've come back.
        // Only runs when there ARE disconnected video devices (zero overhead otherwise).
        // Rate-limited to every 3 ticks (3 seconds) to minimize VCAMDS log noise.
        {
            let health_state = app.state::<RwLock<DeviceHealthState>>();
            let disconnected_videos: Vec<String> = health_state
                .read()
                .disconnected
                .iter()
                .filter(|(_, info)| info.device_type == "video")
                .map(|(id, _)| id.clone())
                .collect();

            if !disconnected_videos.is_empty() && tick_count % 3 == 0 {
                let dm = app.state::<RwLock<DeviceManager>>();
                let dm_read = dm.read();
                let video_names = enumerate_video_device_names();
                for id in &disconnected_videos {
                    if let Some(device) = dm_read.video_devices.iter().find(|d| d.id == *id) {
                        if video_names.contains(&device.name) {
                            // Device is back — remove from current_disconnected so it
                            // appears in newly_reconnected downstream
                            current_disconnected.remove(id);
                        }
                    }
                }
            }
        }

        tick_count = tick_count.wrapping_add(1);

        // Detect changes
        let newly_disconnected: HashSet<String> = current_disconnected
            .difference(&previous_disconnected)
            .cloned()
            .collect();
        let newly_reconnected: HashSet<String> = previous_disconnected
            .difference(&current_disconnected)
            .cloned()
            .collect();

        if newly_disconnected.is_empty() && newly_reconnected.is_empty() {
            previous_disconnected = current_disconnected;
            continue;
        }

        // Update managed health state
        let health_state = app.state::<RwLock<DeviceHealthState>>();
        let config = app.state::<RwLock<Config>>();
        let config_read = config.read();
        let dm = app.state::<RwLock<DeviceManager>>();
        let dm_read = dm.read();

        // Handle newly disconnected devices
        let mut newly_disconnected_names: Vec<String> = Vec::new();
        for id in &newly_disconnected {
            if let Some(info) = resolve_device_info(id, &dm_read, &config_read) {
                println!(
                    "[Health] Device disconnected: {} ({}, {})",
                    info.name, info.id, info.device_type
                );
                newly_disconnected_names.push(info.name.clone());

                // Clear pre-roll buffers for the disconnected device
                match info.device_type.as_str() {
                    "midi" => {
                        let mut state = capture_state.lock();
                        state.midi_preroll.remove_events_for_device(&info.name);
                    }
                    "audio" => {
                        let mut state = capture_state.lock();
                        for preroll in state.audio_prerolls.iter_mut() {
                            if preroll.device_name() == info.name {
                                preroll.clear();
                            }
                        }
                    }
                    "video" => {
                        video_manager.lock().clear_preroll_for_device(&info.id);
                    }
                    _ => {}
                }

                health_state.write().disconnected.insert(id.clone(), info);
            }
        }

        // Send desktop notification for newly disconnected devices
        if !newly_disconnected_names.is_empty() {
            notifications::notify_device_disconnected(&app, &newly_disconnected_names);
        }

        // Handle newly reconnected devices
        let mut reconnected_types: HashSet<String> = HashSet::new();
        for id in &newly_reconnected {
            if let Some(info) = health_state.write().disconnected.remove(id) {
                println!(
                    "[Health] Device reconnected: {} ({}, {})",
                    info.name, info.id, info.device_type
                );
                reconnected_types.insert(info.device_type);
            }

            // Reset video stall state so reconnected device gets a clean slate
            if let Some(state) = video_stall.get_mut(id) {
                state.stall_ticks = 0;
                state.has_seen_frames = false;
                state.last_frame_count = 0;
            }
        }

        // Emit health changed event
        let all_disconnected: Vec<DisconnectedDeviceInfo> =
            health_state.read().disconnected.values().cloned().collect();
        let _ = app.emit(
            "device-health-changed",
            DeviceHealthChangedPayload {
                disconnected_devices: all_disconnected,
            },
        );

        // If devices reconnected, emit restart event for the frontend round-trip
        if !reconnected_types.is_empty() {
            let device_types: Vec<String> = reconnected_types.into_iter().collect();
            println!(
                "[Health] Requesting pipeline restart for: {:?}",
                device_types
            );
            let _ = app.emit(
                "_device-needs-restart",
                DeviceNeedsRestartPayload { device_types },
            );
        }

        previous_disconnected = current_disconnected;
    }

    println!("[Health] Device health checker stopped");
}
