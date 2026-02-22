// Tauri IPC commands

use std::sync::Arc;
use crate::config::Config;
use crate::devices::{AudioDevice, MidiDevice, VideoDevice, DeviceManager};
use crate::recording::{RecordingState, RecordingStatus, MidiMonitor};
use crate::session::{SessionDatabase, SessionSummary, SessionMetadata, SessionFilter};
use crate::autostart::{self, AutostartInfo};
use parking_lot::{RwLock, Mutex};
use tauri::{State, Emitter, Manager};
use serde::{Deserialize, Serialize};

// ============================================================================
// Device Commands
// ============================================================================

#[tauri::command]
pub async fn refresh_devices(
    device_manager: State<'_, RwLock<DeviceManager>>
) -> Result<(), String> {
    let (audio, midi, video) = tokio::task::spawn_blocking(|| {
        let audio = crate::devices::enumerate_audio_devices();
        let midi = crate::devices::enumerate_midi_devices();
        let video = crate::devices::enumerate_video_devices();
        (audio, midi, video)
    }).await.map_err(|e| e.to_string())?;

    let mut dm = device_manager.write();
    dm.audio_devices = audio;
    dm.midi_devices = midi;
    dm.video_devices = video;
    Ok(())
}

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
    db: State<'_, SessionDatabase>,
    session_id: String,
) -> Result<Option<SessionMetadata>, String> {
    let config = config.read();

    // Session ID equals folder name, so construct path directly (O(1) instead of O(n))
    let session_path = config.storage_path.join(&session_id);

    if !session_path.exists() {
        return Ok(None);
    }

    // Build metadata from directory scan
    let mut metadata = crate::session::build_session_from_directory(&session_path)
        .map_err(|e| e.to_string())?;

    // Sync notes to DB if notes.txt was modified externally
    let notes_path = session_path.join("notes.txt");
    if notes_path.exists() {
        if let Ok(file_meta) = std::fs::metadata(&notes_path) {
            if let Ok(modified) = file_meta.modified() {
                let dt: chrono::DateTime<chrono::Utc> = modified.into();
                let modified_str = dt.to_rfc3339();
                // Best-effort DB sync — don't fail the detail load on DB error
                let _ = db.update_notes_with_timestamp(&session_id, &metadata.notes, &modified_str);
            }
        }
    }

    // Check file integrity (detect interrupted recordings)
    use crate::recording::monitor;
    let mut has_corrupt_files = false;

    // MIDI needs_repair is already set by build_session_from_directory
    if metadata.midi_files.iter().any(|f| f.needs_repair) {
        has_corrupt_files = true;
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

    // If no media files found, session is empty
    if metadata.midi_files.is_empty() && metadata.audio_files.is_empty() && metadata.video_files.is_empty() {
        return Ok(None);
    }

    // Duration 0 with audio/video files indicates corruption — show repair banner
    if metadata.duration_secs == 0.0
        && (!metadata.audio_files.is_empty() || !metadata.video_files.is_empty())
    {
        has_corrupt_files = true;
    }

    // If any files are corrupt, add a repair flag via a placeholder MIDI entry
    // (the frontend checks midi_files for needs_repair to show the banner)
    if has_corrupt_files && !metadata.midi_files.iter().any(|f| f.needs_repair) {
        metadata.midi_files.push(crate::session::MidiFileInfo {
            filename: String::new(),
            device_name: String::new(),
            event_count: 0,
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

    // Scan directory and repair files
    let entries = std::fs::read_dir(&session_path).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if fname.ends_with(".mid") {
            if crate::recording::monitor::midi_file_needs_repair(&path) {
                if let Err(e) = crate::recording::monitor::repair_midi_file_on_disk(&path) {
                    println!("[Sacho] Failed to repair MIDI {}: {}", fname, e);
                }
            }
        } else if fname.ends_with(".wav") {
            if crate::recording::monitor::wav_file_needs_repair(&path) {
                if let Err(e) = crate::recording::monitor::repair_wav_file(&path) {
                    println!("[Sacho] Failed to repair WAV {}: {}", fname, e);
                }
            }
        } else if fname.ends_with(".flac") {
            if crate::recording::monitor::flac_file_needs_repair(&path) {
                if let Err(e) = crate::recording::monitor::repair_flac_file(&path) {
                    println!("[Sacho] Failed to repair FLAC {}: {}", fname, e);
                }
            }
        } else if fname.ends_with(".mkv") {
            if crate::recording::monitor::video_file_needs_repair(&path) {
                if let Err(e) = crate::recording::monitor::repair_video_file(&path) {
                    println!("[Sacho] Failed to repair video {}: {}", fname, e);
                }
            }
        }
    }

    // Re-scan with build_session_from_directory to get clean metadata
    let metadata = crate::session::build_session_from_directory(&session_path)
        .map_err(|e| e.to_string())?;

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
pub fn update_session_notes(
    db: State<'_, SessionDatabase>,
    config: State<'_, RwLock<Config>>,
    session_id: String,
    notes: String,
) -> Result<(), String> {
    // Write notes.txt to the session folder (or delete if empty)
    let config = config.read();
    let notes_path = config.storage_path.join(&session_id).join("notes.txt");

    if notes.is_empty() {
        // Delete notes.txt if notes are empty
        if notes_path.exists() {
            let _ = std::fs::remove_file(&notes_path);
        }
        // Update database with empty notes and empty timestamp
        db.update_notes_with_timestamp(&session_id, &notes, "")
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::write(&notes_path, &notes)
            .map_err(|e| e.to_string())?;

        // Read back the OS modified time and update DB
        let notes_modified_at = std::fs::metadata(&notes_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            })
            .unwrap_or_default();

        db.update_notes_with_timestamp(&session_id, &notes, &notes_modified_at)
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

    // Detect per-pipeline changes before updating config
    let (midi_changed, audio_changed, video_changed, preroll_changed, preset_only_changed) = {
        let current = config.read();

        let midi = current.selected_midi_devices != new_config.selected_midi_devices
            || current.trigger_midi_devices != new_config.trigger_midi_devices;

        let audio = current.selected_audio_devices != new_config.selected_audio_devices
            || current.trigger_audio_devices != new_config.trigger_audio_devices;

        // Check if video device configs changed in a way that requires pipeline restart
        let video_devices_changed = current.selected_video_devices != new_config.selected_video_devices;
        let video_configs_pipeline_changed = current.video_device_configs.iter().any(|(k, v)| {
            new_config.video_device_configs.get(k).map_or(true, |nv| !v.pipeline_fields_equal(nv))
        }) || new_config.video_device_configs.iter().any(|(k, _)| {
            !current.video_device_configs.contains_key(k)
        });
        let video = video_devices_changed || video_configs_pipeline_changed;

        let preroll = current.pre_roll_secs != new_config.pre_roll_secs
            || current.encode_during_preroll != new_config.encode_during_preroll;

        // Preset-only change: device configs differ only by preset_level (no pipeline restart needed)
        let preset_only = !video && current.video_device_configs.iter().any(|(k, v)| {
            new_config.video_device_configs.get(k).map_or(false, |nv| v.preset_level != nv.preset_level)
        });

        (midi, audio, video, preroll, preset_only)
    };

    let any_pipeline_changed = midi_changed || audio_changed || video_changed || preroll_changed;

    // If any pipeline settings changed, check if we're currently recording
    if any_pipeline_changed {
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
        crate::tray::update_tray_state(&app, crate::tray::TrayState::Initializing);
    }

    // Update in memory
    {
        let mut config_write = config.write();
        *config_write = new_config.clone();
    }

    // Save to disk (best-effort — don't block pipeline restart on save failure)
    if let Err(e) = new_config.save(&app) {
        println!("[Sacho] Warning: Failed to save config to disk: {}. Pipeline restart will still proceed.", e);
    }

    // Sync preset levels to video manager if only presets changed (no restart needed)
    if preset_only_changed && !any_pipeline_changed {
        let video_mgr = monitor.lock().video_manager();
        let mut mgr = video_mgr.lock();
        for (device_id, dev_config) in &new_config.video_device_configs {
            mgr.update_preset_for_device(device_id, dev_config.preset_level);
        }
    }

    // Restart only the pipelines that changed
    if any_pipeline_changed {
        let mut monitor = monitor.lock();

        let result = if preroll_changed {
            // Pre-roll affects all pipelines — full restart
            monitor.start()
        } else {
            // Selective restarts for each changed pipeline
            let mut combined_result: anyhow::Result<()> = Ok(());
            if midi_changed {
                if let Err(e) = monitor.restart_midi() {
                    combined_result = Err(e);
                }
            }
            if audio_changed {
                if let Err(e) = monitor.restart_audio() {
                    combined_result = Err(e);
                }
            }
            if video_changed {
                if let Err(e) = monitor.restart_video() {
                    combined_result = Err(e);
                }
            }
            combined_result
        };

        // Set status back to Idle regardless of success/failure
        {
            let mut state = recording_state.write();
            state.status = RecordingStatus::Idle;
        }

        // Emit event so frontend knows we're ready
        let _ = app.emit("recording-state-changed", "idle");
        crate::tray::update_tray_state(&app, crate::tray::TrayState::Idle);

        // Return error if restart failed
        result.map_err(|e| format!("Failed to reinitialize devices: {}", e))?;
    }

    // After any config change, immediately check device health to detect
    // if newly-activated devices are disconnected (gives instant UI feedback)
    {
        let disconnected_ids = crate::devices::health::check_active_device_health(&app);
        let health = app.state::<RwLock<crate::devices::health::DeviceHealthState>>();
        let dm = app.state::<RwLock<DeviceManager>>();
        let dm_read = dm.read();
        let config_read = config.read();

        let mut health_write = health.write();
        // Rebuild disconnected map from scratch based on current check
        health_write.disconnected.clear();
        for id in &disconnected_ids {
            // Resolve device info
            if let Some(device) = dm_read.midi_devices.iter().find(|d| &d.id == id) {
                health_write.disconnected.insert(
                    id.clone(),
                    crate::devices::health::DisconnectedDeviceInfo {
                        id: id.clone(),
                        name: device.name.clone(),
                        device_type: "midi".to_string(),
                    },
                );
            } else if config_read.selected_audio_devices.contains(id)
                || config_read.trigger_audio_devices.contains(id)
            {
                health_write.disconnected.insert(
                    id.clone(),
                    crate::devices::health::DisconnectedDeviceInfo {
                        id: id.clone(),
                        name: id.clone(),
                        device_type: "audio".to_string(),
                    },
                );
            } else if let Some(device) = dm_read.video_devices.iter().find(|d| &d.id == id) {
                health_write.disconnected.insert(
                    id.clone(),
                    crate::devices::health::DisconnectedDeviceInfo {
                        id: id.clone(),
                        name: device.name.clone(),
                        device_type: "video".to_string(),
                    },
                );
            }
        }

        let all_disconnected: Vec<crate::devices::health::DisconnectedDeviceInfo> =
            health_write.disconnected.values().cloned().collect();
        drop(health_write);
        drop(config_read);
        drop(dm_read);

        // Emit health event so frontend updates immediately
        #[derive(serde::Serialize, Clone)]
        struct HealthPayload {
            disconnected_devices: Vec<crate::devices::health::DisconnectedDeviceInfo>,
        }
        let _ = app.emit(
            "device-health-changed",
            HealthPayload {
                disconnected_devices: all_disconnected,
            },
        );
    }

    Ok(())
}

/// Update audio trigger thresholds without restarting the pipeline.
/// This is safe to call while recording — it just updates the threshold
/// values in-place on the running monitor's capture state.
#[tauri::command]
pub fn update_audio_trigger_thresholds(
    app: tauri::AppHandle,
    config: State<'_, RwLock<Config>>,
    monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
    thresholds: std::collections::HashMap<String, f64>,
) -> Result<(), String> {
    // Update config in memory and save to disk
    {
        let mut config_write = config.write();
        config_write.audio_trigger_thresholds = thresholds.clone();
        config_write.save(&app).map_err(|e| e.to_string())?;
    }

    // Update thresholds in-place on the running monitor
    let monitor = monitor.lock();
    let mut state = monitor.capture_state.lock();
    for trigger_state in state.audio_trigger_states.iter_mut() {
        if let Some(&new_threshold) = thresholds.get(&trigger_state.device_name) {
            trigger_state.threshold = new_threshold;
        }
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
// Device Health Commands
// ============================================================================

#[tauri::command]
pub fn get_disconnected_devices(
    health: State<'_, RwLock<crate::devices::health::DeviceHealthState>>,
) -> Vec<crate::devices::health::DisconnectedDeviceInfo> {
    health.read().disconnected.values().cloned().collect()
}

#[tauri::command]
pub fn restart_device_pipelines(
    device_types: Vec<String>,
    monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
) -> Result<(), String> {
    let mut monitor = monitor.lock();
    for dtype in &device_types {
        match dtype.as_str() {
            "midi" => {
                if let Err(e) = monitor.restart_midi() {
                    println!("[Health] Failed to restart MIDI: {}", e);
                }
            }
            "audio" => {
                if let Err(e) = monitor.restart_audio() {
                    println!("[Health] Failed to restart audio: {}", e);
                }
            }
            "video" => {
                if let Err(e) = monitor.restart_video() {
                    println!("[Health] Failed to restart video: {}", e);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ============================================================================
// Similarity Commands
// ============================================================================

#[derive(Debug, Serialize)]
pub struct MidiImportInfo {
    pub id: String,
    pub file_name: String,
    pub file_path: String,
    pub imported_at: String,
}

#[derive(Debug, Serialize)]
pub struct SimilarityResult {
    pub file: MidiImportInfo,
    pub score: f32,
    pub rank: u32,
}

#[tauri::command]
pub async fn import_midi_folder(
    app: tauri::AppHandle,
    path: String,
    db: State<'_, SessionDatabase>,
) -> Result<Vec<MidiImportInfo>, String> {
    use crate::similarity::{midi_parser, melody, features};
    use std::path::Path;

    let folder = Path::new(&path);
    if !folder.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    // Recursively collect .mid/.midi files
    let mut midi_paths = Vec::new();
    collect_midi_files(folder, &mut midi_paths);

    if midi_paths.is_empty() {
        return Err("No MIDI files found in folder".to_string());
    }

    // Clear old imports
    db.clear_midi_imports().map_err(|e| e.to_string())?;

    let mut imports = Vec::new();
    let now = chrono::Utc::now().to_rfc3339();

    for (idx, midi_path) in midi_paths.iter().enumerate() {
        let file_name = midi_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.mid")
            .to_string();

        let _ = app.emit("midi-import-progress", MidiImportProgress {
            current: idx + 1,
            total: midi_paths.len(),
            file_name: file_name.clone(),
        });
        let file_path_str = midi_path.to_string_lossy().to_string();
        let id = format!("{:x}", md5_hash(&file_path_str));

        // Parse MIDI and extract features
        let (melodic_json, harmonic_json) = match midi_parser::parse_midi(midi_path) {
            Ok((events, tpb)) => {
                let skyline = melody::extract_skyline(&events, tpb);
                let melodic = features::extract_melodic(&skyline);
                let harmonic = features::extract_harmonic(&events, tpb);
                (
                    melodic.and_then(|f| serde_json::to_string(&f).ok()),
                    harmonic.and_then(|f| serde_json::to_string(&f).ok()),
                )
            }
            Err(e) => {
                log::warn!("Failed to parse MIDI {}: {}", file_name, e);
                (None, None)
            }
        };

        imports.push(crate::session::MidiImport {
            id: id.clone(),
            folder_path: path.clone(),
            file_name: file_name.clone(),
            file_path: file_path_str.clone(),
            melodic_features: melodic_json,
            harmonic_features: harmonic_json,
            imported_at: now.clone(),
        });
    }

    db.insert_midi_imports(&imports).map_err(|e| e.to_string())?;

    let result: Vec<MidiImportInfo> = imports.iter().map(|i| MidiImportInfo {
        id: i.id.clone(),
        file_name: i.file_name.clone(),
        file_path: i.file_path.clone(),
        imported_at: i.imported_at.clone(),
    }).collect();

    Ok(result)
}

fn collect_midi_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_midi_files(&path, out);
            } else if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ext == "mid" || ext == "midi" {
                    out.push(path);
                }
            }
        }
    }
}

fn md5_hash(input: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

#[tauri::command]
pub fn get_midi_imports(
    db: State<'_, SessionDatabase>,
) -> Result<Vec<MidiImportInfo>, String> {
    let imports = db.get_all_midi_imports().map_err(|e| e.to_string())?;
    Ok(imports.iter().map(|i| MidiImportInfo {
        id: i.id.clone(),
        file_name: i.file_name.clone(),
        file_path: i.file_path.clone(),
        imported_at: i.imported_at.clone(),
    }).collect())
}

#[tauri::command]
pub fn get_similar_files(
    file_id: String,
    mode: String,
    db: State<'_, SessionDatabase>,
) -> Result<Vec<SimilarityResult>, String> {
    use crate::similarity::{features, scoring};

    let imports = db.get_all_midi_imports().map_err(|e| e.to_string())?;

    let sim_mode = match mode.as_str() {
        "harmonic" => scoring::SimilarityMode::Harmonic,
        _ => scoring::SimilarityMode::Melodic,
    };

    // Build features list
    let mut all_features: Vec<(String, features::MidiFileFeatures)> = Vec::new();
    for import in &imports {
        let melodic = import.melodic_features.as_ref()
            .and_then(|s| serde_json::from_str(s).ok());
        let harmonic = import.harmonic_features.as_ref()
            .and_then(|s| serde_json::from_str(s).ok());
        all_features.push((import.id.clone(), features::MidiFileFeatures { melodic, harmonic }));
    }

    let similar = scoring::find_most_similar(&file_id, &all_features, sim_mode, 12, 0.05);

    let results: Vec<SimilarityResult> = similar.iter().enumerate().map(|(i, (id, score))| {
        let import = imports.iter().find(|imp| imp.id == *id).unwrap();
        SimilarityResult {
            file: MidiImportInfo {
                id: import.id.clone(),
                file_name: import.file_name.clone(),
                file_path: import.file_path.clone(),
                imported_at: import.imported_at.clone(),
            },
            score: *score,
            rank: (i + 1) as u32,
        }
    }).collect();

    Ok(results)
}

#[tauri::command]
pub fn clear_midi_imports(
    db: State<'_, SessionDatabase>,
) -> Result<(), String> {
    db.clear_midi_imports().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rescan_sessions(
    app: tauri::AppHandle,
) -> Result<usize, String> {
    tokio::task::spawn_blocking(move || {
        rescan_sessions_blocking(&app)
    }).await.map_err(|e| e.to_string())?
}

fn rescan_sessions_blocking(app: &tauri::AppHandle) -> Result<usize, String> {
    use std::collections::{HashMap, HashSet};
    use crate::session::{SessionIndexData, UpdatedSessionData, ExistingSessionRow};

    let config = app.state::<RwLock<Config>>();
    let db = app.state::<SessionDatabase>();
    let storage_path = config.read().storage_path.clone();

    if !storage_path.exists() {
        return Ok(0);
    }

    // 1. Collect folder names from disk
    let mut folder_list: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut disk_folders: HashSet<String> = HashSet::new();

    for entry in std::fs::read_dir(&storage_path).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            disk_folders.insert(name.to_string());
            folder_list.push((name.to_string(), path));
        }
    }

    // 2. Get existing sessions from DB
    let existing = db.get_all_existing_sessions().map_err(|e| e.to_string())?;
    let existing_map: HashMap<String, ExistingSessionRow> = existing
        .into_iter()
        .map(|row| (row.id.clone(), row))
        .collect();

    // Only emit progress if there are new sessions to scan (the slow path)
    let new_count = folder_list.iter()
        .filter(|(name, _)| !existing_map.contains_key(name))
        .count();
    let emit_progress = new_count > 0;
    let total = folder_list.len();

    // Create a single shared GStreamer Discoverer for all video duration reads
    let discoverer = if new_count > 0 {
        crate::session::get_or_create_discoverer().ok()
    } else {
        None
    };

    // 3. Diff: new, updated, deleted
    let mut new_sessions: Vec<SessionIndexData> = Vec::new();
    let mut updated_sessions: Vec<UpdatedSessionData> = Vec::new();

    for (i, (folder_name, path)) in folder_list.iter().enumerate() {
        if emit_progress {
            let _ = app.emit("rescan-progress", RescanProgress {
                current: i + 1,
                total,
            });
        }

        if let Some(db_row) = existing_map.get(folder_name) {
            // Existing session - lightweight check only (no header parsing)
            let mut has_audio = false;
            let mut has_midi = false;
            let mut has_video = false;
            let mut notes_modified_at = String::new();

            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let fname = match entry.file_name().to_str() {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    if fname == "notes.txt" {
                        if let Ok(meta) = entry.metadata() {
                            if let Ok(modified) = meta.modified() {
                                let dt: chrono::DateTime<chrono::Utc> = modified.into();
                                notes_modified_at = dt.to_rfc3339();
                            }
                        }
                    } else if fname.ends_with(".wav") || fname.ends_with(".flac") {
                        has_audio = true;
                    } else if fname.ends_with(".mid") {
                        has_midi = true;
                    } else if fname.ends_with(".mkv") {
                        has_video = true;
                    }
                }
            }

            // Check if anything changed
            let tags_changed = has_audio != db_row.has_audio
                || has_midi != db_row.has_midi
                || has_video != db_row.has_video;
            let notes_changed = notes_modified_at != db_row.notes_modified_at;

            if tags_changed || notes_changed {
                let notes_path = path.join("notes.txt");
                let notes = std::fs::read_to_string(&notes_path).unwrap_or_default();

                updated_sessions.push(UpdatedSessionData {
                    id: folder_name.clone(),
                    has_audio,
                    has_midi,
                    has_video,
                    notes,
                    notes_modified_at: if notes_changed {
                        notes_modified_at
                    } else {
                        db_row.notes_modified_at.clone()
                    },
                });
            }
        } else {
            // New session - full scan with header parsing
            match crate::session::scan_session_dir_for_index(path, discoverer.as_ref()) {
                Ok(index_data) => {
                    if index_data.has_audio || index_data.has_midi || index_data.has_video {
                        new_sessions.push(index_data);
                    }
                }
                Err(e) => {
                    log::debug!("Skipping directory {}: {}", path.display(), e);
                }
            }
        }
    }

    // 4. Sessions in DB but not on disk -> deleted
    let deleted_ids: Vec<&String> = existing_map.keys()
        .filter(|id| !disk_folders.contains(id.as_str()))
        .collect();

    // 5. Batch sync in a single transaction
    let _count = db.batch_sync(&new_sessions, &updated_sessions, &deleted_ids)
        .map_err(|e| e.to_string())?;

    let result = new_sessions.len() + updated_sessions.len();
    log::info!("Rescanned sessions: {} new, {} updated, {} deleted",
        new_sessions.len(), updated_sessions.len(), deleted_ids.len());
    Ok(result)
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

/// Information about a single encoder backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderBackendInfo {
    pub id: String,
    pub display_name: String,
    pub is_hardware: bool,
}

/// Per-codec encoder availability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodecEncoderInfo {
    pub available: bool,
    pub has_hardware: bool,
    pub encoders: Vec<EncoderBackendInfo>,
    pub recommended: Option<String>,
}

/// Information about available video encoders (per-codec)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderAvailability {
    pub av1: CodecEncoderInfo,
    pub vp9: CodecEncoderInfo,
    pub vp8: CodecEncoderInfo,
    pub h264: CodecEncoderInfo,
    pub ffv1: CodecEncoderInfo,
    /// Recommended default encoding codec
    pub recommended_codec: String,
}

fn build_codec_encoder_info(codec: crate::encoding::VideoCodec) -> CodecEncoderInfo {
    use crate::encoding::{available_encoders_for_codec, detect_best_encoder_for_codec, HardwareEncoderType};

    let available = available_encoders_for_codec(codec);
    let has_hardware = available.iter().any(|(hw, _)| !matches!(hw, HardwareEncoderType::Software));
    let recommended = if !available.is_empty() {
        let best = detect_best_encoder_for_codec(codec);
        Some(format!("{:?}", best).to_lowercase())
    } else {
        None
    };

    let encoders = available.iter().map(|(hw, _)| {
        EncoderBackendInfo {
            id: format!("{:?}", hw).to_lowercase(),
            display_name: hw.display_name().to_string(),
            is_hardware: !matches!(hw, HardwareEncoderType::Software),
        }
    }).collect();

    CodecEncoderInfo {
        available: !available.is_empty(),
        has_hardware,
        encoders,
        recommended,
    }
}

#[tauri::command]
pub fn get_encoder_availability() -> EncoderAvailability {
    use crate::encoding::{VideoCodec, get_recommended_codec};

    let recommended = get_recommended_codec();

    EncoderAvailability {
        av1: build_codec_encoder_info(VideoCodec::Av1),
        vp9: build_codec_encoder_info(VideoCodec::Vp9),
        vp8: build_codec_encoder_info(VideoCodec::Vp8),
        h264: build_codec_encoder_info(VideoCodec::H264),
        ffv1: build_codec_encoder_info(VideoCodec::Ffv1),
        recommended_codec: match recommended {
            VideoCodec::Av1 => "av1".to_string(),
            VideoCodec::H264 => "h264".to_string(),
            VideoCodec::Vp9 => "vp9".to_string(),
            VideoCodec::Vp8 => "vp8".to_string(),
            VideoCodec::Ffv1 => "ffv1".to_string(),
            _ => "vp8".to_string(),
        },
    }
}

// ============================================================================
// Auto-select Encoder Preset
// ============================================================================

/// Progress update emitted during session rescan
#[derive(Debug, Clone, Serialize)]
pub struct RescanProgress {
    pub current: usize,
    pub total: usize,
}

/// Progress update emitted during MIDI folder import
#[derive(Debug, Clone, Serialize)]
pub struct MidiImportProgress {
    pub current: usize,
    pub total: usize,
    pub file_name: String,
}

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
    device_id: String,
    config: State<'_, RwLock<Config>>,
    recording_state: State<'_, RwLock<RecordingState>>,
    monitor: State<'_, Arc<Mutex<MidiMonitor>>>,
    device_manager: State<'_, RwLock<DeviceManager>>,
) -> Result<u8, String> {
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

    // 2. Read per-device encoding config
    let (device_name, dev_config) = {
        let cfg = config.read();
        let devices = device_manager.read();

        let device = devices.video_devices.iter()
            .find(|d| d.id == device_id)
            .ok_or_else(|| format!("Device {} not found", device_id))?;
        let name = device.name.clone();

        let dev_cfg = cfg.video_device_configs.get(&device_id)
            .cloned()
            .or_else(|| device.default_config())
            .ok_or_else(|| format!("No config available for device {}", device_id))?;

        (name, dev_cfg)
    };

    if dev_config.passthrough {
        return Err("Cannot auto-select for passthrough mode (no encoding)".to_string());
    }

    let target_codec = dev_config.encoding_codec.unwrap_or_else(|| {
        crate::encoding::get_recommended_codec()
    });

    // 3. Set status to initializing to prevent recording attempts
    {
        let mut state = recording_state.write();
        state.status = RecordingStatus::Initializing;
    }
    let _ = app.emit("recording-state-changed", "initializing");
    crate::tray::update_tray_state(&app, crate::tray::TrayState::Initializing);

    // 4. Get the video manager from the monitor and stop video pipelines only.
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

        let pre_roll = cfg.pre_roll_secs.min(5);

        (info, pre_roll)
    };

    // Stop video pipelines (releases camera)
    video_manager.lock().stop();

    // 5. Run the auto-select test (this is the long-running part)
    let result = run_auto_select_test(
        &app,
        &device_id,
        &device_name,
        &dev_config,
        target_codec,
    ).await;

    // 6. Restart video pipelines regardless of test result
    {
        let (ref devices_info, pre_roll) = restart_info;
        let mut mgr = video_manager.lock();
        mgr.set_preroll_duration(pre_roll);
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
    crate::tray::update_tray_state(&app, crate::tray::TrayState::Idle);

    result
}

/// Core auto-select test logic. Creates a test pipeline and encoder for each
/// preset level, measures frame drops over 10 seconds per level.
async fn run_auto_select_test(
    app: &tauri::AppHandle,
    device_id: &str,
    device_name: &str,
    dev_config: &crate::config::VideoDeviceConfig,
    target_codec: crate::encoding::VideoCodec,
) -> Result<u8, String> {
    use crate::recording::video::VideoCapturePipeline;
    use crate::encoding::{AsyncVideoEncoder, EncoderConfig, RawVideoFrame, MAX_PRESET, MIN_PRESET};
    use std::time::{Duration, Instant};

    // Extract device index from device_id
    let device_index = device_id
        .strip_prefix("webcam-")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    // Create a test capture pipeline using the device's source settings
    println!("[AutoSelect] Creating test capture pipeline for {} ({})", device_name, device_id);
    let mut capture = VideoCapturePipeline::new_webcam_raw(
        device_index,
        device_name,
        device_id,
        dev_config.source_codec,
        dev_config.source_width,
        dev_config.source_height,
        dev_config.source_fps,
        2, // minimal pre-roll
        Some(target_codec),
        dev_config.encoder_type,
        dev_config.preset_level,
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
// Custom Sound Commands
// ============================================================================

/// Copy a user-selected audio file into the app config dir (sounds/ subfolder)
/// and store the relative path in config.
#[tauri::command]
pub fn set_custom_sound(
    app: tauri::AppHandle,
    config: State<'_, RwLock<Config>>,
    source_path: String,
    sound_type: String,
) -> Result<String, String> {
    use std::path::Path;

    let source = Path::new(&source_path);
    if !source.exists() {
        return Err("Source file does not exist".to_string());
    }

    let filename = source.file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid filename")?;

    // Build destination: <app_config_dir>/sounds/<sound_type>_<filename>
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let sounds_dir = config_dir.join("sounds");
    std::fs::create_dir_all(&sounds_dir).map_err(|e| e.to_string())?;

    let dest_filename = format!("{}_{}", sound_type, filename);
    let dest_path = sounds_dir.join(&dest_filename);

    std::fs::copy(&source, &dest_path).map_err(|e| e.to_string())?;

    let relative_path = format!("sounds/{}", dest_filename);

    // Update config
    {
        let mut cfg = config.write();
        match sound_type.as_str() {
            "start" => cfg.custom_sound_start = Some(relative_path.clone()),
            "stop" => cfg.custom_sound_stop = Some(relative_path.clone()),
            "disconnect" => cfg.custom_sound_disconnect = Some(relative_path.clone()),
            _ => return Err("Invalid sound_type: must be 'start', 'stop', or 'disconnect'".to_string()),
        }
        cfg.save(&app).map_err(|e| e.to_string())?;
    }

    Ok(relative_path)
}

/// Clear a custom sound: delete the copied file and remove the path from config.
#[tauri::command]
pub fn clear_custom_sound(
    app: tauri::AppHandle,
    config: State<'_, RwLock<Config>>,
    sound_type: String,
) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;

    {
        let mut cfg = config.write();
        let path_opt = match sound_type.as_str() {
            "start" => cfg.custom_sound_start.take(),
            "stop" => cfg.custom_sound_stop.take(),
            "disconnect" => cfg.custom_sound_disconnect.take(),
            _ => return Err("Invalid sound_type: must be 'start', 'stop', or 'disconnect'".to_string()),
        };

        // Delete the file if it exists
        if let Some(ref rel_path) = path_opt {
            let full_path = config_dir.join(rel_path);
            if full_path.exists() {
                let _ = std::fs::remove_file(&full_path);
            }
        }

        cfg.save(&app).map_err(|e| e.to_string())?;
    }

    Ok(())
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
