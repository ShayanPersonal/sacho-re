// Tauri IPC commands

use std::sync::Arc;
use crate::config::Config;
use crate::devices::{AudioDevice, MidiDevice, VideoDevice, DeviceManager};
use crate::recording::{RecordingState, RecordingStatus, MidiMonitor};
use crate::session::{SessionDatabase, SessionSummary, SessionMetadata, SessionFilter, SimilarityPoint};
use crate::similarity;
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
    let metadata: SessionMetadata = serde_json::from_str(&contents)
        .map_err(|e| e.to_string())?;
    
    Ok(Some(metadata))
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
    new_config: Config,
) -> Result<(), String> {
    // Check if device-related settings changed before updating
    let device_settings_changed = {
        let current = config.read();
        current.selected_midi_devices != new_config.selected_midi_devices
            || current.trigger_midi_devices != new_config.trigger_midi_devices
            || current.selected_video_devices != new_config.selected_video_devices
            || current.video_device_codecs != new_config.video_device_codecs
            || current.selected_audio_devices != new_config.selected_audio_devices
            || current.pre_roll_secs != new_config.pre_roll_secs
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
