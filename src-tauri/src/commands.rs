// Tauri IPC commands

use std::sync::Arc;
use crate::config::Config;
use crate::devices::{AudioDevice, MidiDevice, VideoDevice, DeviceManager};
use crate::recording::{RecordingState, RecordingStatus, MidiMonitor};
use crate::session::{SessionDatabase, SessionSummary, SessionMetadata, SessionFilter, SimilarityPoint};
use crate::similarity;
use crate::autostart::{self, AutostartInfo};
use parking_lot::{RwLock, Mutex};
use tauri::{State, Emitter};
use serde::{Deserialize, Serialize};

// ============================================================================
// Device Commands
// ============================================================================

#[tauri::command]
pub fn get_audio_devices(
    device_manager: State<'_, RwLock<DeviceManager>>
) -> Vec<AudioDevice> {
    device_manager.read().audio_devices.clone()
}

#[tauri::command]
pub fn get_midi_devices(
    device_manager: State<'_, RwLock<DeviceManager>>
) -> Vec<MidiDevice> {
    device_manager.read().midi_devices.clone()
}

#[tauri::command]
pub fn get_video_devices(
    device_manager: State<'_, RwLock<DeviceManager>>
) -> Vec<VideoDevice> {
    device_manager.read().video_devices.clone()
}

/// Validate that a video device configuration will work at runtime.
/// Checks if the stored GStreamer device has exact caps for the requested mode.
#[tauri::command]
pub fn validate_video_device_config(
    device_id: String,
    codec: String,
    width: u32,
    height: u32,
    fps: f64,
) -> bool {
    crate::devices::enumeration::validate_video_config(&device_id, &codec, width, height, fps)
}

// ============================================================================
// Recording Commands
// ============================================================================

#[tauri::command]
pub fn get_recording_state(
    state: State<'_, RwLock<RecordingState>>
) -> RecordingState {
    state.read().clone()
}

/// Manual recording now uses the same MidiMonitor infrastructure as MIDI-triggered recording
/// This ensures all device types (MIDI, audio, video) are captured consistently

#[tauri::command]
pub fn start_recording(
    recording_state: State<'_, RwLock<RecordingState>>,
    midi_monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
) -> Result<String, String> {
    // Check if we're in a state that allows recording
    {
        let state = recording_state.read();
        if state.status == RecordingStatus::Initializing {
            return Err("Devices are being reinitialized, please wait".to_string());
        }
        if state.status == RecordingStatus::Recording {
            return Err("Already recording".to_string());
        }
        if state.status == RecordingStatus::Stopping {
            return Err("Recording is stopping, please wait".to_string());
        }
    }
    
    // Use the MidiMonitor's manual start method which captures all device types
    let monitor = midi_monitor.lock();
    monitor.manual_start_recording()?;
    
    Ok("Recording started".to_string())
}

#[tauri::command]
pub fn stop_recording(
    midi_monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
) -> Result<(), String> {
    // Use the MidiMonitor's manual stop method which handles all device types
    // and saves MIDI, audio, and video files properly
    let monitor = midi_monitor.lock();
    monitor.manual_stop_recording()?;
    
    Ok(())
}

// ============================================================================
// Session Commands
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct SessionFilterParams {
    pub search: Option<String>,
    pub favorites_only: Option<bool>,
    pub has_audio: Option<bool>,
    pub has_midi: Option<bool>,
    pub has_video: Option<bool>,
    pub has_notes: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[tauri::command]
pub fn get_sessions(
    db: State<'_, SessionDatabase>,
    filter: SessionFilterParams,
) -> Result<Vec<SessionSummary>, String> {
    let filter = SessionFilter {
        search_query: filter.search,
        favorites_only: filter.favorites_only.unwrap_or(false),
        has_audio: filter.has_audio,
        has_midi: filter.has_midi,
        has_video: filter.has_video,
        has_notes: filter.has_notes,
        limit: filter.limit,
        offset: filter.offset,
        ..Default::default()
    };
    
    db.query_sessions(&filter)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_session_detail(
    config: State<'_, RwLock<Config>>,
    session_id: String,
) -> Result<Option<SessionMetadata>, String> {
    let config = config.read();
    
    // Session ID equals folder name, so construct path directly (O(1) instead of O(n))
    let session_path = config.storage_path.join(&session_id);
    let metadata_path = session_path.join("metadata.json");
    
    if !metadata_path.exists() {
        return Ok(None);
    }
    
    let contents = std::fs::read_to_string(&metadata_path)
        .map_err(|e| e.to_string())?;
    let mut metadata: SessionMetadata = serde_json::from_str(&contents)
        .map_err(|e| e.to_string())?;
    
    // Check file integrity (detect interrupted recordings)
    use crate::recording::monitor;
    
    let mut has_corrupt_files = false;
    
    // Check MIDI files
    for midi_file in &mut metadata.midi_files {
        let midi_path = session_path.join(&midi_file.filename);
        if midi_path.exists() && monitor::midi_file_needs_repair(&midi_path) {
            midi_file.needs_repair = true;
            has_corrupt_files = true;
        }
    }
    
    // Check audio files
    for audio_file in &metadata.audio_files {
        let audio_path = session_path.join(&audio_file.filename);
        if audio_path.exists() {
            let needs_repair = if audio_file.filename.ends_with(".wav") {
                monitor::wav_file_needs_repair(&audio_path)
            } else if audio_file.filename.ends_with(".flac") {
                monitor::flac_file_needs_repair(&audio_path)
            } else {
                false
            };
            if needs_repair { has_corrupt_files = true; }
        }
    }
    
    // Check video files
    for video_file in &metadata.video_files {
        let video_path = session_path.join(&video_file.filename);
        if video_path.exists() && monitor::video_file_needs_repair(&video_path) {
            has_corrupt_files = true;
        }
    }
    
    // Detect interrupted sessions: metadata has no file entries but session dir has media files
    if metadata.midi_files.is_empty() && metadata.audio_files.is_empty() && metadata.video_files.is_empty() {
        if let Ok(entries) = std::fs::read_dir(&session_path) {
            let has_media = entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.ends_with(".mid") || name.ends_with(".wav") || name.ends_with(".flac")
                    || name.ends_with(".mkv")
            });
            if has_media { has_corrupt_files = true; }
        }
    }
    
    // If any files are corrupt/missing, add a repair flag via a placeholder MIDI entry
    // (the frontend checks midi_files for needs_repair to show the banner)
    if has_corrupt_files && !metadata.midi_files.iter().any(|f| f.needs_repair) {
        metadata.midi_files.push(crate::session::MidiFileInfo {
            filename: String::new(),
            device_name: String::new(),
            event_count: 0,
            size_bytes: 0,
            needs_repair: true,
        });
    }
    
    Ok(Some(metadata))
}

#[tauri::command]
pub fn repair_session(
    config: State<'_, RwLock<Config>>,
    db: State<'_, SessionDatabase>,
    session_id: String,
) -> Result<SessionMetadata, String> {
    let config = config.read();
    let session_path = config.storage_path.join(&session_id);
    
    if !session_path.exists() {
        return Err(format!("Session folder not found: {}", session_id));
    }
    
    // Load existing metadata (may have empty file lists from initial write)
    let metadata_path = session_path.join("metadata.json");
    let mut metadata: SessionMetadata = if metadata_path.exists() {
        let contents = std::fs::read_to_string(&metadata_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&contents).map_err(|e| e.to_string())?
    } else {
        return Err("No metadata.json found".to_string());
    };
    
    // Scan the session directory for actual files and rebuild file lists
    let entries = std::fs::read_dir(&session_path).map_err(|e| e.to_string())?;
    
    let mut midi_files = Vec::new();
    let mut audio_files = Vec::new();
    let mut video_files = Vec::new();
    
    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        
        if fname.ends_with(".mid") {
            // Repair MIDI header if needed
            let needs_repair = crate::recording::monitor::midi_file_needs_repair(&path);
            let event_count = if needs_repair {
                match crate::recording::monitor::repair_midi_file_on_disk(&path) {
                    Ok(count) => count,
                    Err(e) => {
                        println!("[Sacho] Failed to repair MIDI {}: {}", fname, e);
                        0
                    }
                }
            } else {
                // Try to get event count from existing metadata
                metadata.midi_files.iter()
                    .find(|f| f.filename == fname)
                    .map(|f| f.event_count)
                    .unwrap_or(0)
            };
            
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            
            // Extract device name from filename: "midi_Device_Name.mid" -> "Device Name"
            let device_name = metadata.midi_files.iter()
                .find(|f| f.filename == fname)
                .map(|f| f.device_name.clone())
                .unwrap_or_else(|| {
                    fname.trim_start_matches("midi_")
                        .trim_end_matches(".mid")
                        .replace('_', " ")
                });
            
            midi_files.push(crate::session::MidiFileInfo {
                filename: fname,
                device_name,
                event_count,
                size_bytes: size,
                needs_repair: false,
            });
        } else if fname.ends_with(".wav") {
            // Check if WAV needs repair
            let needs_repair = crate::recording::monitor::wav_file_needs_repair(&path);
            
            if needs_repair {
                match crate::recording::monitor::repair_wav_file(&path) {
                    Ok((channels, sample_rate, duration_secs, size_bytes)) => {
                        let device_name = metadata.audio_files.iter()
                            .find(|f| f.filename == fname)
                            .map(|f| f.device_name.clone())
                            .unwrap_or_else(|| {
                                fname.trim_start_matches("recording")
                                    .trim_start_matches('_')
                                    .trim_end_matches(".wav")
                                    .to_string()
                            });
                        audio_files.push(crate::session::AudioFileInfo {
                            filename: fname,
                            device_name: if device_name.is_empty() { "Unknown".to_string() } else { device_name },
                            channels,
                            sample_rate,
                            duration_secs,
                            size_bytes,
                        });
                    }
                    Err(e) => {
                        println!("[Sacho] Failed to repair WAV {}: {}", fname, e);
                        // Still add it with whatever info we have
                        if let Some(existing) = metadata.audio_files.iter().find(|f| f.filename == fname) {
                            audio_files.push(existing.clone());
                        }
                    }
                }
            } else if let Some(existing) = metadata.audio_files.iter().find(|f| f.filename == fname) {
                audio_files.push(existing.clone());
            } else {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let device_name = fname.trim_start_matches("recording")
                    .trim_start_matches('_')
                    .trim_end_matches(".wav")
                    .to_string();
                audio_files.push(crate::session::AudioFileInfo {
                    filename: fname,
                    device_name: if device_name.is_empty() { "Unknown".to_string() } else { device_name },
                    channels: 0,
                    sample_rate: 0,
                    duration_secs: 0.0,
                    size_bytes: size,
                });
            }
        } else if fname.ends_with(".flac") {
            // Check if FLAC needs repair
            let needs_repair = crate::recording::monitor::flac_file_needs_repair(&path);
            
            if needs_repair {
                match crate::recording::monitor::repair_flac_file(&path) {
                    Ok((channels, sample_rate, duration_secs, size_bytes)) => {
                        let device_name = metadata.audio_files.iter()
                            .find(|f| f.filename == fname)
                            .map(|f| f.device_name.clone())
                            .unwrap_or_else(|| {
                                fname.trim_start_matches("recording")
                                    .trim_start_matches('_')
                                    .trim_end_matches(".flac")
                                    .to_string()
                            });
                        audio_files.push(crate::session::AudioFileInfo {
                            filename: fname,
                            device_name: if device_name.is_empty() { "Unknown".to_string() } else { device_name },
                            channels,
                            sample_rate,
                            duration_secs,
                            size_bytes,
                        });
                    }
                    Err(e) => {
                        println!("[Sacho] Failed to repair FLAC {}: {}", fname, e);
                        if let Some(existing) = metadata.audio_files.iter().find(|f| f.filename == fname) {
                            audio_files.push(existing.clone());
                        }
                    }
                }
            } else if let Some(existing) = metadata.audio_files.iter().find(|f| f.filename == fname) {
                audio_files.push(existing.clone());
            } else {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let device_name = fname.trim_start_matches("recording")
                    .trim_start_matches('_')
                    .trim_end_matches(".flac")
                    .to_string();
                audio_files.push(crate::session::AudioFileInfo {
                    filename: fname,
                    device_name: if device_name.is_empty() { "Unknown".to_string() } else { device_name },
                    channels: 0,
                    sample_rate: 0,
                    duration_secs: 0.0,
                    size_bytes: size,
                });
            }
        } else if fname.ends_with(".mkv") {
            // Check if video needs repair
            let needs_repair = crate::recording::monitor::video_file_needs_repair(&path);
            
            if needs_repair {
                match crate::recording::monitor::repair_video_file(&path) {
                    Ok((duration_secs, size_bytes)) => {
                        let device_name = metadata.video_files.iter()
                            .find(|f| f.filename == fname)
                            .map(|f| f.device_name.clone())
                            .unwrap_or_else(|| {
                                fname.trim_start_matches("video_")
                                    .trim_end_matches(".mkv")
                                    .replace('_', " ")
                            });
                        let (width, height, fps) = metadata.video_files.iter()
                            .find(|f| f.filename == fname)
                            .map(|f| (f.width, f.height, f.fps))
                            .unwrap_or((0, 0, 0.0));
                        video_files.push(crate::session::VideoFileInfo {
                            filename: fname,
                            device_name: if device_name.is_empty() { "Unknown".to_string() } else { device_name },
                            width,
                            height,
                            fps,
                            duration_secs,
                            size_bytes,
                            has_audio: false,
                        });
                    }
                    Err(e) => {
                        println!("[Sacho] Failed to repair video {}: {}", fname, e);
                        if let Some(existing) = metadata.video_files.iter().find(|f| f.filename == fname) {
                            video_files.push(existing.clone());
                        }
                    }
                }
            } else if let Some(existing) = metadata.video_files.iter().find(|f| f.filename == fname) {
                video_files.push(existing.clone());
            } else {
                // Video file exists but wasn't in metadata - add with what we know
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let device_name = fname.trim_start_matches("video_")
                    .trim_end_matches(".mkv")
                    .replace('_', " ");
                video_files.push(crate::session::VideoFileInfo {
                    filename: fname,
                    device_name: if device_name.is_empty() { "Unknown".to_string() } else { device_name },
                    width: 0,
                    height: 0,
                    fps: 0.0,
                    duration_secs: 0.0,
                    size_bytes: size,
                    has_audio: false,
                });
            }
        }
    }
    
    // Update metadata with discovered/repaired files
    metadata.midi_files = midi_files;
    metadata.audio_files = audio_files;
    metadata.video_files = video_files;
    
    // Update duration from the longest file
    let max_audio_dur = metadata.audio_files.iter()
        .map(|f| f.duration_secs)
        .fold(0.0f64, f64::max);
    let max_video_dur = metadata.video_files.iter()
        .map(|f| f.duration_secs)
        .fold(0.0f64, f64::max);
    let max_dur = max_audio_dur.max(max_video_dur);
    if max_dur > 0.0 {
        metadata.duration_secs = max_dur;
    }
    
    // Save repaired metadata
    if let Err(e) = crate::session::save_metadata(&metadata) {
        return Err(format!("Failed to save repaired metadata: {}", e));
    }
    
    // Update the database
    if let Err(e) = db.upsert_session(&metadata) {
        println!("[Sacho] Failed to update DB after repair: {}", e);
    }
    
    println!("[Sacho] Repaired session {}: {} MIDI, {} audio, {} video files",
        session_id, metadata.midi_files.len(), metadata.audio_files.len(), metadata.video_files.len());
    
    Ok(metadata)
}

#[tauri::command]
pub fn delete_session(
    db: State<'_, SessionDatabase>,
    config: State<'_, RwLock<Config>>,
    session_id: String,
) -> Result<(), String> {
    let config = config.read();
    
    // Remove from database first (if this fails, filesystem stays intact)
    db.delete_session(&session_id)
        .map_err(|e| e.to_string())?;
    
    // Session ID equals folder name, so construct path directly (O(1) instead of O(n))
    let session_path = config.storage_path.join(&session_id);
    if session_path.exists() {
        std::fs::remove_dir_all(&session_path).map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

#[tauri::command]
pub fn update_session_favorite(
    db: State<'_, SessionDatabase>,
    config: State<'_, RwLock<Config>>,
    session_id: String,
    is_favorite: bool,
) -> Result<(), String> {
    // Update database
    db.update_favorite(&session_id, is_favorite)
        .map_err(|e| e.to_string())?;
    
    // Also update the metadata.json file on disk (O(1) lookup by folder name)
    let config = config.read();
    let metadata_path = config.storage_path.join(&session_id).join("metadata.json");
    
    if metadata_path.exists() {
        let contents = std::fs::read_to_string(&metadata_path)
            .map_err(|e| e.to_string())?;
        let mut metadata: SessionMetadata = serde_json::from_str(&contents)
            .map_err(|e| e.to_string())?;
        
        metadata.is_favorite = is_favorite;
        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| e.to_string())?;
        std::fs::write(&metadata_path, json)
            .map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

#[tauri::command]
pub fn update_session_notes(
    db: State<'_, SessionDatabase>,
    config: State<'_, RwLock<Config>>,
    session_id: String,
    notes: String,
) -> Result<(), String> {
    // Update database
    db.update_notes(&session_id, &notes)
        .map_err(|e| e.to_string())?;
    
    // Also update the metadata.json file on disk (O(1) lookup by folder name)
    let config = config.read();
    let metadata_path = config.storage_path.join(&session_id).join("metadata.json");
    
    if metadata_path.exists() {
        let contents = std::fs::read_to_string(&metadata_path)
            .map_err(|e| e.to_string())?;
        let mut metadata: SessionMetadata = serde_json::from_str(&contents)
            .map_err(|e| e.to_string())?;
        
        metadata.notes = notes;
        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| e.to_string())?;
        std::fs::write(&metadata_path, json)
            .map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

// ============================================================================
// Config Commands
// ============================================================================

#[tauri::command]
pub fn get_config(
    config: State<'_, RwLock<Config>>
) -> Config {
    config.read().clone()
}

#[tauri::command]
pub fn update_config(
    app: tauri::AppHandle,
    config: State<'_, RwLock<Config>>,
    recording_state: State<'_, RwLock<RecordingState>>,
    monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
    mut new_config: Config,
) -> Result<(), String> {
    // Validate and clamp config values to safe ranges
    new_config.validate();

    // Check if device-related settings changed before updating
    let (device_settings_changed, preset_or_mode_changed) = {
        let current = config.read();
        let dev_changed = current.selected_midi_devices != new_config.selected_midi_devices
            || current.trigger_midi_devices != new_config.trigger_midi_devices
            || current.trigger_audio_devices != new_config.trigger_audio_devices
            || current.audio_trigger_thresholds != new_config.audio_trigger_thresholds
            || current.selected_video_devices != new_config.selected_video_devices
            || current.video_device_configs != new_config.video_device_configs
            || current.selected_audio_devices != new_config.selected_audio_devices
            || current.pre_roll_secs != new_config.pre_roll_secs
            || current.encode_during_preroll != new_config.encode_during_preroll
            // Encoding mode changes the entire encoder pipeline (different GStreamer
            // elements, codec, container), so it requires a full pipeline restart.
            || current.video_encoding_mode != new_config.video_encoding_mode;
        let preset_changed = current.encoder_preset_levels != new_config.encoder_preset_levels;
        (dev_changed, preset_changed)
    };
    
    // If devices changed, check if we're currently recording
    if device_settings_changed {
        let state = recording_state.read();
        if state.status == RecordingStatus::Recording {
            return Err("Cannot change device settings while recording".to_string());
        }
        
        // Set status to Initializing to prevent recording attempts during reset
        drop(state);
        {
            let mut state = recording_state.write();
            state.status = RecordingStatus::Initializing;
        }
        
        // Emit event so frontend knows we're reinitializing
        let _ = app.emit("recording-state-changed", "initializing");
    }
    
    // Update in memory
    {
        let mut config_write = config.write();
        *config_write = new_config.clone();
    }
    
    // Save to disk
    new_config.save(&app).map_err(|e| e.to_string())?;
    
    // Sync preset level to video manager if it changed (no restart needed)
    if preset_or_mode_changed && !device_settings_changed {
        sync_preset_level_to_video_manager(&monitor, &new_config);
    }
    
    // Restart monitor if device-related settings changed
    // This is synchronous to ensure we're in a valid state before returning
    if device_settings_changed {
        let mut monitor = monitor.lock();
        
        // Restart the monitor (this stops existing captures and starts new ones)
        let result = monitor.start();
        
        // Set status back to Idle regardless of success/failure
        {
            let mut state = recording_state.write();
            state.status = RecordingStatus::Idle;
        }
        
        // Emit event so frontend knows we're ready
        let _ = app.emit("recording-state-changed", "idle");
        
        // Return error if restart failed
        result.map_err(|e| format!("Failed to reinitialize devices: {}", e))?;
    }
    
    Ok(())
}

#[tauri::command]
pub fn restart_midi_monitor(
    monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
) -> Result<(), String> {
    let mut monitor = monitor.lock();
    monitor.start().map_err(|e| e.to_string())
}

// ============================================================================
// Similarity Commands
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SimilarityData {
    pub points: Vec<SimilarityPoint>,
    pub clusters: Vec<ClusterInfo>,
}

#[derive(Debug, Serialize)]
pub struct ClusterInfo {
    pub id: i32,
    pub name: String,
    pub count: usize,
}

#[tauri::command]
pub fn get_similarity_data(
    db: State<'_, SessionDatabase>,
) -> Result<SimilarityData, String> {
    let points = db.get_similarity_data()
        .map_err(|e| e.to_string())?;
    
    // Count points per cluster
    let mut cluster_counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    for point in &points {
        if let Some(cluster_id) = point.cluster_id {
            *cluster_counts.entry(cluster_id).or_insert(0) += 1;
        }
    }
    
    let clusters: Vec<ClusterInfo> = cluster_counts.into_iter()
        .map(|(id, count)| ClusterInfo {
            id,
            name: format!("Cluster {}", id + 1),
            count,
        })
        .collect();
    
    Ok(SimilarityData { points, clusters })
}

#[tauri::command]
pub async fn recalculate_similarity(
    config: State<'_, RwLock<Config>>,
    db: State<'_, SessionDatabase>,
) -> Result<usize, String> {
    let storage_path = config.read().storage_path.clone();
    
    // First, clean up sessions that no longer exist on disk
    let existing_sessions = db.query_sessions(&SessionFilter::default())
        .map_err(|e| e.to_string())?;
    
    for session in &existing_sessions {
        let session_path = storage_path.join(&session.id);
        if !session_path.exists() {
            log::info!("Removing deleted session from database: {}", session.id);
            let _ = db.delete_session(&session.id);
        }
    }
    
    // Collect all MIDI files and extract features
    let mut session_features: Vec<(String, crate::session::MidiFeatures)> = Vec::new();
    
    if storage_path.exists() {
        for entry in std::fs::read_dir(&storage_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            
            if !path.is_dir() {
                continue;
            }
            
            // Look for MIDI files
            for file in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
                let file = file.map_err(|e| e.to_string())?;
                let file_path = file.path();
                
                if file_path.extension().map(|e| e == "mid").unwrap_or(false) {
                    if let Ok(features) = similarity::extract_features(&file_path) {
                        // Use folder name as session ID
                        let session_id = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();
                        
                        session_features.push((session_id, features));
                    }
                    break; // Only process first MIDI file per session
                }
            }
        }
    }
    
    if session_features.is_empty() {
        return Ok(0);
    }
    
    // Extract just the features for reduction
    let features: Vec<_> = session_features.iter()
        .map(|(_, f)| f.clone())
        .collect();
    
    // Reduce to 2D
    let coords = similarity::reduce_to_2d(&features, &similarity::UmapParams::default());
    
    // Cluster the points
    let cluster_result = similarity::auto_cluster(&coords);
    
    // Update database
    for (i, (session_id, _)) in session_features.iter().enumerate() {
        if let Some(coord) = coords.get(i) {
            let cluster_id = cluster_result.labels.get(i).and_then(|&l| l);
            let _ = db.update_similarity(session_id, *coord, cluster_id);
        }
    }
    
    Ok(session_features.len())
}

#[tauri::command]
pub fn rescan_sessions(
    config: State<'_, RwLock<Config>>,
    db: State<'_, SessionDatabase>,
) -> Result<usize, String> {
    let storage_path = config.read().storage_path.clone();
    
    if !storage_path.exists() {
        return Ok(0);
    }
    
    // Collect all metadata first, then batch insert
    let mut sessions: Vec<SessionMetadata> = Vec::new();
    
    // Scan all directories in storage path
    for entry in std::fs::read_dir(&storage_path).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        
        if !path.is_dir() {
            continue;
        }
        
        // Check for metadata.json
        let metadata_path = path.join("metadata.json");
        if !metadata_path.exists() {
            continue;
        }
        
        // Read and parse metadata
        if let Ok(contents) = std::fs::read_to_string(&metadata_path) {
            if let Ok(metadata) = serde_json::from_str::<SessionMetadata>(&contents) {
                sessions.push(metadata);
            }
        }
    }
    
    // Batch insert all sessions in a single transaction
    let count = db.batch_upsert_sessions(&sessions).map_err(|e| e.to_string())?;
    
    log::info!("Rescanned {} sessions", count);
    Ok(count)
}

// ============================================================================
// File Access Commands
// ============================================================================

#[tauri::command]
pub fn read_session_file(session_path: String, filename: String) -> Result<Vec<u8>, String> {
    use std::path::Path;
    use std::fs;
    
    let path = Path::new(&session_path).join(&filename);
    fs::read(&path).map_err(|e| format!("Failed to read file {}: {}", filename, e))
}

// ============================================================================
// Video Playback Commands
// ============================================================================

/// Information about a video file for playback
#[derive(Debug, Serialize)]
pub struct VideoPlaybackInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_ms: u64,
    pub frame_count: u64,
    pub codec: String,
}

/// Information about a video file's codec and playability
#[derive(Debug, Serialize)]
pub struct VideoCodecCheck {
    /// The detected codec name
    pub codec: String,
    /// Whether this video can be played
    pub is_playable: bool,
    /// Reason if not playable
    pub reason: Option<String>,
}

/// A single frame for playback
#[derive(Debug, Serialize)]
pub struct VideoFrameData {
    /// Base64-encoded JPEG data
    pub data_base64: String,
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Check if a video file's codec is supported for playback
/// This probes the actual codec from the file, not just the container
#[tauri::command]
pub fn check_video_codec(session_path: String, filename: String) -> Result<VideoCodecCheck, String> {
    use std::path::Path;
    use crate::video;
    
    let path = Path::new(&session_path).join(&filename);
    let codec_info = video::probe_video_codec(&path).map_err(|e| e.to_string())?;
    
    Ok(VideoCodecCheck {
        codec: codec_info.codec,
        is_playable: codec_info.is_supported,
        reason: codec_info.reason,
    })
}

#[tauri::command]
pub fn get_video_info(session_path: String, filename: String) -> Result<VideoPlaybackInfo, String> {
    use std::path::Path;
    use crate::video;
    
    let path = Path::new(&session_path).join(&filename);
    let demuxer = video::open_video(&path).map_err(|e| e.to_string())?;
    let info = demuxer.info();
    
    Ok(VideoPlaybackInfo {
        width: info.width,
        height: info.height,
        fps: info.fps,
        duration_ms: info.duration_ms,
        frame_count: info.frame_count,
        codec: info.codec.clone(),
    })
}

#[tauri::command]
pub fn get_video_frame(
    session_path: String, 
    filename: String, 
    timestamp_ms: u64
) -> Result<VideoFrameData, String> {
    use std::path::Path;
    use crate::video;
    use base64::Engine;
    
    let path = Path::new(&session_path).join(&filename);
    let mut demuxer = video::open_video(&path).map_err(|e| e.to_string())?;
    
    let frame = demuxer.get_frame_at(timestamp_ms).map_err(|e| e.to_string())?;
    
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(&frame.data);
    
    Ok(VideoFrameData {
        data_base64,
        timestamp_ms: frame.timestamp_ms,
        duration_ms: frame.duration_ms,
    })
}

#[tauri::command]
pub fn get_video_frames_batch(
    session_path: String,
    filename: String,
    start_ms: u64,
    end_ms: u64,
    max_frames: Option<usize>,
) -> Result<Vec<VideoFrameData>, String> {
    use std::path::Path;
    use crate::video;
    use base64::Engine;
    
    let path = Path::new(&session_path).join(&filename);
    let mut demuxer = video::open_video(&path).map_err(|e| e.to_string())?;
    
    let frames = demuxer.get_frames_range(start_ms, end_ms).map_err(|e| e.to_string())?;
    
    let max = max_frames.unwrap_or(usize::MAX);
    
    let result: Vec<VideoFrameData> = frames.into_iter()
        .take(max)
        .map(|frame| {
            VideoFrameData {
                data_base64: base64::engine::general_purpose::STANDARD.encode(&frame.data),
                timestamp_ms: frame.timestamp_ms,
                duration_ms: frame.duration_ms,
            }
        })
        .collect();
    
    Ok(result)
}

#[tauri::command]
pub fn get_video_frame_timestamps(
    session_path: String,
    filename: String,
) -> Result<Vec<u64>, String> {
    use std::path::Path;
    use crate::video;
    
    let path = Path::new(&session_path).join(&filename);
    let mut demuxer = video::open_video(&path).map_err(|e| e.to_string())?;
    
    demuxer.get_frame_timestamps().map_err(|e| e.to_string())
}

// ============================================================================
// Encoder Availability Commands
// ============================================================================

/// Information about available video encoders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderAvailability {
    /// Whether AV1 encoding is available (hardware or software)
    pub av1_available: bool,
    /// Whether AV1 hardware encoding is available
    pub av1_hardware: bool,
    /// Whether VP9 encoding is available (hardware or software)
    pub vp9_available: bool,
    /// Whether VP9 hardware encoding is available
    pub vp9_hardware: bool,
    /// Whether VP8 encoding is available (hardware or software)
    pub vp8_available: bool,
    /// Whether VP8 hardware encoding is available
    pub vp8_hardware: bool,
    /// Name of the AV1 encoder if available
    pub av1_encoder_name: Option<String>,
    /// Name of the VP9 encoder if available
    pub vp9_encoder_name: Option<String>,
    /// Name of the VP8 encoder if available
    pub vp8_encoder_name: Option<String>,
    /// Recommended default encoding mode
    pub recommended_default: String,
}

/// Update the encoder preset level on the video manager without restarting.
/// Called from `update_config` when only the preset level changes.
fn sync_preset_level_to_video_manager(
    monitor: &Arc<Mutex<MidiMonitor>>,
    config: &Config,
) {
    let encoding_mode_key = match &config.video_encoding_mode {
        crate::config::VideoEncodingMode::Av1 => "av1",
        crate::config::VideoEncodingMode::Vp9 => "vp9",
        crate::config::VideoEncodingMode::Vp8 => "vp8",
        crate::config::VideoEncodingMode::Raw => "vp8",
    };
    let level = config.encoder_preset_levels
        .get(encoding_mode_key)
        .copied()
        .unwrap_or(crate::encoding::DEFAULT_PRESET);
    
    let monitor = monitor.lock();
    let video_mgr = monitor.video_manager();
    video_mgr.lock().set_preset_level(level);
}

#[tauri::command]
pub fn get_encoder_availability() -> EncoderAvailability {
    use crate::encoding::{
        detect_best_av1_encoder, detect_best_vp8_encoder, detect_best_vp9_encoder,
        has_hardware_av1_encoder, has_hardware_vp9_encoder, has_hardware_vp8_encoder,
        has_av1_encoder, has_vp8_encoder, has_vp9_encoder,
        get_recommended_encoding_mode,
    };
    
    let av1_type = detect_best_av1_encoder();
    let vp9_type = detect_best_vp9_encoder();
    let vp8_type = detect_best_vp8_encoder();
    let recommended = get_recommended_encoding_mode();
    
    EncoderAvailability {
        av1_available: has_av1_encoder(),
        av1_hardware: has_hardware_av1_encoder(),
        vp9_available: has_vp9_encoder(),
        vp9_hardware: has_hardware_vp9_encoder(),
        vp8_available: has_vp8_encoder(),
        vp8_hardware: has_hardware_vp8_encoder(),
        av1_encoder_name: if has_av1_encoder() {
            Some(av1_type.display_name().to_string())
        } else {
            None
        },
        vp9_encoder_name: if has_vp9_encoder() {
            Some(vp9_type.display_name().to_string())
        } else {
            None
        },
        vp8_encoder_name: if has_vp8_encoder() {
            Some(vp8_type.display_name().to_string())
        } else {
            None
        },
        recommended_default: match recommended {
            crate::config::VideoEncodingMode::Av1 => "av1".to_string(),
            crate::config::VideoEncodingMode::Vp9 => "vp9".to_string(),
            crate::config::VideoEncodingMode::Vp8 => "vp8".to_string(),
            crate::config::VideoEncodingMode::Raw => "raw".to_string(),
        },
    }
}

// ============================================================================
// Auto-select Encoder Preset
// ============================================================================

/// Progress update emitted during auto-select
#[derive(Debug, Clone, Serialize)]
pub struct AutoSelectProgress {
    /// The preset level currently being tested (5 down to 1)
    pub testing_level: u8,
    /// Total levels to test
    pub total_levels: u8,
    /// Status message
    pub message: String,
}

/// Run the video encoding pipeline with each preset level to find the best one
/// that doesn't drop frames. Starts at level 5 (maximum quality) and steps down.
///
/// Emits `auto-select-progress` events during the test.
///
/// This command temporarily stops video capture pipelines to gain exclusive
/// access to the camera device, then restarts them when done. MIDI and audio
/// monitoring continue uninterrupted.
#[tauri::command]
pub async fn auto_select_encoder_preset(
    app: tauri::AppHandle,
    config: State<'_, RwLock<Config>>,
    recording_state: State<'_, RwLock<RecordingState>>,
    monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
    device_manager: State<'_, RwLock<DeviceManager>>,
) -> Result<u8, String> {
    use crate::encoding::VideoCodec;
    
    // 1. Check we're not recording
    {
        let state = recording_state.read();
        if state.status == RecordingStatus::Recording {
            return Err("Cannot auto-select while recording".to_string());
        }
        if state.status == RecordingStatus::Stopping {
            return Err("Recording is stopping, please wait".to_string());
        }
    }
    
    // 2. Read config
    let (selected_video_devices, encoding_mode, video_device_configs) = {
        let cfg = config.read();
        (
            cfg.selected_video_devices.clone(),
            cfg.video_encoding_mode.clone(),
            cfg.video_device_configs.clone(),
        )
    };
    
    // Check encoding mode is valid for testing
    if encoding_mode == crate::config::VideoEncodingMode::Raw {
        return Err("Cannot auto-select for raw mode (no encoding)".to_string());
    }
    
    // Find first selected video device that uses raw codec (needs encoding)
    let test_device = {
        let devices = device_manager.read();
        selected_video_devices.iter().find_map(|device_id| {
            let device = devices.video_devices.iter().find(|d| &d.id == device_id)?;
            
            // Check per-device config; fall back to preferred codec
            let codec = if let Some(dev_cfg) = video_device_configs.get(device_id) {
                dev_cfg.source_codec
            } else {
                device.preferred_codec()?
            };
            
            // Only test devices that use raw video (need encoding)
            if codec == VideoCodec::Raw {
                Some((device_id.clone(), device.name.clone()))
            } else {
                None
            }
        })
    };
    
    let (device_id, device_name) = test_device.ok_or_else(|| {
        "No raw video streams selected. Auto-select only works with devices streaming raw video that requires encoding.".to_string()
    })?;
    
    // 3. Set status to initializing to prevent recording attempts
    {
        let mut state = recording_state.write();
        state.status = RecordingStatus::Initializing;
    }
    let _ = app.emit("recording-state-changed", "initializing");
    
    // 4. Get the video manager from the monitor and stop video pipelines only.
    //    This releases the camera without touching audio/MIDI (which use TLS).
    let video_manager = {
        let mon = monitor.lock();
        mon.video_manager()
    };
    
    // Save the info needed to restart pipelines later
    let restart_info = {
        let cfg = config.read();
        let devices = device_manager.read();
        let dev_configs = &cfg.video_device_configs;
        
        let info: Vec<(String, String, crate::config::VideoDeviceConfig)> = cfg.selected_video_devices
            .iter()
            .filter_map(|dev_id| {
                let device = devices.video_devices.iter().find(|d| &d.id == dev_id)?;
                let dev_cfg = if let Some(c) = dev_configs.get(dev_id) {
                    if device.supported_codecs.contains(&c.source_codec) {
                        c.clone()
                    } else {
                        device.default_config()?
                    }
                } else {
                    device.default_config()?
                };
                Some((dev_id.clone(), device.name.clone(), dev_cfg))
            })
            .collect();
        
        let enc_mode = cfg.video_encoding_mode.clone();
        let enc_key = match &enc_mode {
            crate::config::VideoEncodingMode::Av1 => "av1",
            crate::config::VideoEncodingMode::Vp9 => "vp9",
            crate::config::VideoEncodingMode::Vp8 => "vp8",
            crate::config::VideoEncodingMode::Raw => "vp8",
        };
        let preset = cfg.encoder_preset_levels.get(enc_key).copied()
            .unwrap_or(crate::encoding::DEFAULT_PRESET);
        let pre_roll = cfg.pre_roll_secs.min(5);
        
        (info, enc_mode, preset, pre_roll)
    };
    
    // Stop video pipelines (releases camera)
    video_manager.lock().stop();
    
    // 5. Run the auto-select test (this is the long-running part)
    let result = run_auto_select_test(
        &app,
        &device_id,
        &device_name,
        &encoding_mode,
    ).await;
    
    // 6. Restart video pipelines regardless of test result
    {
        let (ref devices_info, ref enc_mode, preset, pre_roll) = restart_info;
        let mut mgr = video_manager.lock();
        mgr.set_preroll_duration(pre_roll);
        mgr.set_encoding_mode(enc_mode.clone());
        // Use the auto-selected preset if successful, otherwise keep the old one
        let final_preset = result.as_ref().copied().unwrap_or(preset);
        mgr.set_preset_level(final_preset);
        if !devices_info.is_empty() {
            if let Err(e) = mgr.start(devices_info) {
                println!("[AutoSelect] Warning: Failed to restart video pipelines: {}", e);
            }
        }
    }
    
    // 7. Set status back to idle
    {
        let mut state = recording_state.write();
        state.status = RecordingStatus::Idle;
    }
    let _ = app.emit("recording-state-changed", "idle");
    
    result
}

/// Core auto-select test logic. Creates a test pipeline and encoder for each
/// preset level, measures frame drops over 10 seconds per level.
async fn run_auto_select_test(
    app: &tauri::AppHandle,
    device_id: &str,
    device_name: &str,
    encoding_mode: &crate::config::VideoEncodingMode,
) -> Result<u8, String> {
    use crate::recording::video::VideoCapturePipeline;
    use crate::encoding::{AsyncVideoEncoder, EncoderConfig, RawVideoFrame, MAX_PRESET, MIN_PRESET};
    use std::time::{Duration, Instant};
    
    let target_codec = match encoding_mode {
        crate::config::VideoEncodingMode::Av1 => crate::encoding::VideoCodec::Av1,
        crate::config::VideoEncodingMode::Vp9 => crate::encoding::VideoCodec::Vp9,
        crate::config::VideoEncodingMode::Vp8 => crate::encoding::VideoCodec::Vp8,
        crate::config::VideoEncodingMode::Raw => {
            return Err("Cannot test raw mode".to_string());
        }
    };
    
    // Extract device index from device_id
    let device_index = device_id
        .strip_prefix("webcam-")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    
    // Create a test capture pipeline with default resolution
    println!("[AutoSelect] Creating test capture pipeline for {} ({})", device_name, device_id);
    let mut capture = VideoCapturePipeline::new_webcam_raw(
        device_index,
        device_name,
        &device_id,
        1920, 1080, 30.0, // Default test resolution
        2, // minimal pre-roll
        encoding_mode.clone(),
        false, // Don't encode during pre-roll for auto-select tests
    ).map_err(|e| format!("Failed to create test pipeline: {}", e))?;
    
    // Start capture
    capture.start().map_err(|e| format!("Failed to start test capture: {}", e))?;
    
    // Wait for frames to start arriving (up to 5 seconds)
    println!("[AutoSelect] Waiting for video frames...");
    let wait_start = Instant::now();
    loop {
        if wait_start.elapsed() > Duration::from_secs(5) {
            let _ = capture.stop();
            return Err("Timeout waiting for video frames from camera".to_string());
        }
        if capture.preroll_duration() > Duration::from_millis(100) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("[AutoSelect] Frames arriving, starting tests");
    
    let test_duration = Duration::from_secs(10);
    let drop_threshold = 2u64;
    let mut best_level = MIN_PRESET; // Fallback to lightest
    
    // Test from most intensive to lightest
    for level in (MIN_PRESET..=MAX_PRESET).rev() {
        // Emit progress
        let _ = app.emit("auto-select-progress", AutoSelectProgress {
            testing_level: level,
            total_levels: MAX_PRESET,
            message: format!("Testing preset {} ({})...", level, crate::encoding::presets::preset_label(level)),
        });
        
        println!("[AutoSelect] Testing preset level {} ({})...", level, crate::encoding::presets::preset_label(level));
        
        // Create a temp file for the test encoder
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("sacho_autoselect_test_{}.mkv", level));
        
        // Create encoder with this preset
        let encoder_config = EncoderConfig {
            keyframe_interval: (capture.fps * 2.0).round() as u32,
            target_codec,
            preset_level: level,
            target_width: None,
            target_height: None,
            target_fps: None,
        };
        
        let encoder = match AsyncVideoEncoder::new(
            temp_file.clone(),
            capture.width,
            capture.height,
            capture.fps,
            encoder_config,
            (capture.fps * 2.0) as usize,
        ) {
            Ok(enc) => enc,
            Err(e) => {
                println!("[AutoSelect] Failed to create encoder for level {}: {}", level, e);
                let _ = std::fs::remove_file(&temp_file);
                continue;
            }
        };
        
        // Feed frames for the test duration
        let test_start = Instant::now();
        let mut total_sent = 0u64;
        let mut total_dropped = 0u64;
        let poll_interval = Duration::from_millis(10);
        let pixel_format = "NV12".to_string();
        
        while test_start.elapsed() < test_duration {
            // Drain frames from the pre-roll buffer
            let frames = capture.drain_preroll_frames();
            
            for frame in &frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: capture.width,
                    height: capture.height,
                    format: frame.pixel_format.clone().unwrap_or_else(|| pixel_format.clone()),
                    capture_time: frame.wall_time,
                };
                
                match encoder.try_send_frame(raw_frame) {
                    Ok(true) => total_sent += 1,
                    Ok(false) => total_dropped += 1,
                    Err(e) => {
                        println!("[AutoSelect] Encoder error at level {}: {}", level, e);
                        total_dropped += 1;
                    }
                }
            }
            
            // Early exit if we've already exceeded the threshold
            if total_dropped >= drop_threshold {
                break;
            }
            
            tokio::time::sleep(poll_interval).await;
        }
        
        // Finish the encoder (gracefully)
        drop(encoder);
        
        // Clean up temp file
        let _ = std::fs::remove_file(&temp_file);
        let temp_tmp = temp_dir.join(format!("sacho_autoselect_test_{}.mkv.tmp", level));
        let _ = std::fs::remove_file(&temp_tmp);
        
        println!("[AutoSelect] Level {}: sent={}, dropped={} (threshold={})", 
            level, total_sent, total_dropped, drop_threshold);
        
        if total_dropped < drop_threshold {
            best_level = level;
            println!("[AutoSelect] Level {} passed! Selecting as best preset.", level);
            break;
        } else {
            println!("[AutoSelect] Level {} had too many drops, trying lower.", level);
        }
    }
    
    // Stop capture
    let _ = capture.stop();
    
    println!("[AutoSelect] Best preset level: {} ({})", best_level, crate::encoding::presets::preset_label(best_level));
    
    Ok(best_level)
}

// ============================================================================
// Autostart Commands
// ============================================================================

#[tauri::command]
pub fn get_autostart_info() -> AutostartInfo {
    AutostartInfo {
        is_per_machine_install: autostart::is_per_machine_install(),
        all_users_autostart: autostart::is_hklm_autostart_enabled(),
    }
}

#[tauri::command]
pub fn set_all_users_autostart(enabled: bool) -> Result<(), String> {
    autostart::request_set_hklm_autostart(enabled)?;
    // Re-check the actual state after the elevated process ran
    let actual_state = autostart::is_hklm_autostart_enabled();
    if actual_state != enabled {
        Err("The autostart setting was not changed. The UAC prompt may have been cancelled.".to_string())
    } else {
        Ok(())
    }
}

/// Dev-only: force a crash to test RegisterApplicationRestart
#[tauri::command]
pub fn simulate_crash() {
    std::process::abort();
}

// ============================================================================
// App Stats Commands
// ============================================================================

#[derive(Serialize)]
pub struct AppStats {
    /// Process CPU usage percentage (0-100+, may exceed 100 on multi-core)
    pub cpu_percent: f32,
    /// Process resident set size (physical memory) in bytes
    pub memory_bytes: u64,
    /// Total size of all files in the recordings folder, in bytes
    pub storage_used_bytes: u64,
    /// Free space on the disk containing the recordings folder, in bytes
    pub disk_free_bytes: u64,
}

/// Get current app resource usage: CPU%, RAM, storage used, and disk free space.
///
/// CPU/RAM are read from sysinfo (per-process). Storage and disk stats run on
/// a blocking thread via `spawn_blocking` to avoid stalling the async runtime.
#[tauri::command]
pub async fn get_app_stats(
    config: State<'_, RwLock<Config>>,
    sys_state: State<'_, Mutex<sysinfo::System>>,
) -> Result<AppStats, String> {
    // --- CPU & RAM (fast, in-process) ---
    let pid = sysinfo::get_current_pid().map_err(|e| e.to_string())?;
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get() as f32)
        .unwrap_or(1.0);
    let (cpu_percent, memory_bytes) = {
        let mut sys = sys_state.lock();
        sys.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            false,
        );
        let process = sys.process(pid);
        match process {
            // sysinfo reports per-core %, so 400% = 4 cores fully used.
            // Normalize to total system capacity (0-100%).
            Some(p) => (p.cpu_usage() / num_cpus, p.memory()),
            None => (0.0, 0),
        }
    };

    // --- Storage walk + disk free (potentially slow, run on blocking thread) ---
    let storage_path = config.read().storage_path.clone();
    let (storage_used_bytes, disk_free_bytes) = tokio::task::spawn_blocking(move || {
        let used = dir_size_recursive(&storage_path);
        let free = disk_free_space(&storage_path);
        (used, free)
    })
    .await
    .map_err(|e| format!("Stats task failed: {}", e))?;

    Ok(AppStats {
        cpu_percent,
        memory_bytes,
        storage_used_bytes,
        disk_free_bytes,
    })
}

/// Recursively walk a directory and sum up all file sizes.
fn dir_size_recursive(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_dir() {
                total += dir_size_recursive(&entry.path());
            } else if ft.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// Find the disk that contains `path` and return its available space.
fn disk_free_space(path: &std::path::Path) -> u64 {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();

    // On Windows, std::fs::canonicalize returns \\?\C:\... (UNC prefix) but
    // sysinfo mount points are plain C:\. Strip the prefix so starts_with works.
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let canonical_str = canonical.to_string_lossy();
    let clean_path = if canonical_str.starts_with(r"\\?\") {
        std::path::PathBuf::from(&canonical_str[4..])
    } else {
        canonical
    };

    // Find the disk whose mount point is the longest prefix of our path
    let mut best_mount: Option<&std::path::Path> = None;
    let mut best_free: u64 = 0;

    for disk in disks.list() {
        let mount = disk.mount_point();
        if clean_path.starts_with(mount) {
            let is_better = match best_mount {
                None => true,
                Some(prev) => mount.as_os_str().len() > prev.as_os_str().len(),
            };
            if is_better {
                best_mount = Some(mount);
                best_free = disk.available_space();
            }
        }
    }
    best_free
}
