use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::{Mutex, RwLock};
use tauri::{AppHandle, Manager};
use tempfile::TempDir;

use crate::config::Config;
use crate::devices::DeviceManager;
use crate::recording::{MidiMonitor, RecordingState, RecordingStatus};
use crate::session::{SessionDatabase, SessionMetadata};

/// A headless Tauri app wired up identically to the production app,
/// but using a temp directory for session storage and an in-memory DB.
pub struct TestApp {
    /// Keeps the Wry runtime alive.
    _app: tauri::App,
    handle: AppHandle,
    /// Auto-cleaned on drop (unless --keep-sessions is used).
    _storage_dir: Option<TempDir>,
    /// If --keep-sessions, we store the path but don't auto-clean.
    storage_path: PathBuf,
}

impl TestApp {
    /// Build a headless Tauri app with the given config overrides.
    /// The window is created but never shown.
    pub fn new(mut config: Config, keep_sessions: bool) -> Self {
        // Create temp dir for session storage
        let temp_dir = TempDir::new().expect("Failed to create temp dir for test sessions");
        let storage_path = temp_dir.path().to_path_buf();
        config.storage_path = storage_path.clone();

        let config_for_setup = config.clone();

        let app = tauri::Builder::default()
            .plugin(tauri_plugin_notification::init())
            .plugin(tauri_plugin_shell::init())
            .plugin(tauri_plugin_dialog::init())
            .setup(move |app| {
                // Manage config
                app.manage(RwLock::new(config_for_setup));

                // Recording state
                app.manage(RwLock::new(RecordingState::new()));

                // Device manager
                app.manage(RwLock::new(DeviceManager::new()));

                // In-memory session database
                let session_db = SessionDatabase::open_in_memory()
                    .expect("Failed to create in-memory session database");
                app.manage(session_db);

                // Sysinfo (needed by some commands)
                let mut sys = sysinfo::System::new();
                sys.refresh_processes(
                    sysinfo::ProcessesToUpdate::Some(&[sysinfo::get_current_pid().unwrap()]),
                    true,
                );
                app.manage(Mutex::new(sys));

                // Create MidiMonitor but don't start it yet — the test runner
                // will call start() after pipeline warmup delay.
                let app_handle = app.handle().clone();
                let midi_monitor = MidiMonitor::new(app_handle);
                app.manage(Arc::new(Mutex::new(midi_monitor)));

                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                crate::commands::get_audio_devices,
                crate::commands::get_midi_devices,
                crate::commands::get_video_devices,
                crate::commands::validate_video_device_config,
                crate::commands::get_recording_state,
                crate::commands::start_recording,
                crate::commands::stop_recording,
                crate::commands::get_sessions,
                crate::commands::get_session_detail,
                crate::commands::repair_session,
                crate::commands::delete_session,
                crate::commands::update_session_favorite,
                crate::commands::update_session_notes,
                crate::commands::get_config,
                crate::commands::update_config,
                crate::commands::update_audio_trigger_thresholds,
                crate::commands::get_similarity_data,
                crate::commands::recalculate_similarity,
                crate::commands::rescan_sessions,
                crate::commands::restart_midi_monitor,
                crate::commands::read_session_file,
                crate::commands::check_video_codec,
                crate::commands::get_video_info,
                crate::commands::get_video_frame,
                crate::commands::get_video_frames_batch,
                crate::commands::get_video_frame_timestamps,
                crate::commands::get_encoder_availability,
                crate::commands::auto_select_encoder_preset,
                crate::commands::get_autostart_info,
                crate::commands::set_all_users_autostart,
                crate::commands::simulate_crash,
                crate::commands::get_app_stats,
            ])
            .build(tauri::generate_context!())
            .expect("Failed to build headless Tauri app");

        let handle = app.handle().clone();

        let storage_dir = if keep_sessions {
            // Leak the TempDir so it doesn't auto-clean
            let path = temp_dir.keep();
            println!("  Sessions will be kept at: {}", path.display());
            None
        } else {
            Some(temp_dir)
        };

        Self {
            _app: app,
            handle,
            _storage_dir: storage_dir,
            storage_path,
        }
    }

    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    pub fn storage_path(&self) -> &PathBuf {
        &self.storage_path
    }

    /// Start the MidiMonitor (connects devices, starts pipelines).
    pub fn start_monitor(&self) -> anyhow::Result<()> {
        let monitor = self.handle.state::<Arc<Mutex<MidiMonitor>>>();
        let mut monitor = monitor.lock();
        monitor.start()
    }

    /// Stop the MidiMonitor.
    pub fn stop_monitor(&self) {
        let monitor = self.handle.state::<Arc<Mutex<MidiMonitor>>>();
        let mut monitor = monitor.lock();
        monitor.stop();
    }

    /// Get the current recording status.
    pub fn recording_status(&self) -> RecordingStatus {
        let state = self.handle.state::<RwLock<RecordingState>>();
        let status = state.read().status.clone();
        status
    }

    /// Poll until the recording status matches `target`, or timeout.
    /// Returns true if the target status was reached.
    pub fn wait_for_status(&self, target: RecordingStatus, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            if self.recording_status() == target {
                return true;
            }
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(poll_interval);
        }
    }

    /// List session directories in the storage path.
    pub fn session_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.storage_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                }
            }
        }
        dirs.sort();
        dirs
    }

    /// Parse metadata.json from the latest (or only) session directory.
    pub fn latest_metadata(&self) -> Option<SessionMetadata> {
        let dirs = self.session_dirs();
        let dir = dirs.last()?;
        let meta_path = dir.join("metadata.json");
        let contents = std::fs::read_to_string(&meta_path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    /// Manually start recording (no MIDI trigger needed).
    pub fn manual_start_recording(&self) -> Result<(), String> {
        let monitor = self.handle.state::<Arc<Mutex<MidiMonitor>>>();
        let monitor = monitor.lock();
        monitor.manual_start_recording()
    }

    /// Manually stop recording.
    pub fn manual_stop_recording(&self) -> Result<(), String> {
        let monitor = self.handle.state::<Arc<Mutex<MidiMonitor>>>();
        let monitor = monitor.lock();
        monitor.manual_stop_recording()
    }
}
