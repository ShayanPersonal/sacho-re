// Sacho - Automatic Recording Studio Companion
// Main library entry point

pub mod config;
pub mod devices;
pub mod encoding;
pub mod gstreamer_init;
pub mod recording;
pub mod session;
pub mod similarity;
pub mod tray;
pub mod notifications;
pub mod commands;
pub mod video;

use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use tauri::Manager;

/// Initialize and run the Tauri application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    
    // Initialize GStreamer environment before anything else
    // This sets up paths for private GStreamer deployment on Windows
    gstreamer_init::init_gstreamer_env();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the existing window when a second instance tries to launch
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .on_window_event(|window, event| {
            // Handle window close - always minimize to tray
            // App can only be quit via tray icon context menu
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide window instead of closing
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .setup(|app| {
            // Initialize application state
            let app_handle = app.handle().clone();
            
            // Initialize config
            let config = config::Config::load_or_default(&app_handle);
            app.manage(RwLock::new(config));
            
            // Initialize recording engine state
            let recording_state = recording::RecordingState::new();
            app.manage(RwLock::new(recording_state));
            
            // Initialize device manager
            let device_manager = devices::DeviceManager::new();
            app.manage(RwLock::new(device_manager));
            
            // Initialize session database
            let session_db = match session::SessionDatabase::open(&app_handle) {
                Ok(db) => db,
                Err(e) => {
                    log::error!("Failed to open session database: {}", e);
                    // Show error to user via dialog
                    let _ = tauri::async_runtime::block_on(async {
                        tauri_plugin_dialog::DialogExt::dialog(app)
                            .message(format!("Failed to initialize database: {}\n\nThe application may not function correctly.", e))
                            .title("Database Error")
                            .blocking_show();
                    });
                    // Create an in-memory fallback so app can still run
                    session::SessionDatabase::open_in_memory()
                        .expect("Failed to create in-memory database fallback")
                }
            };
            app.manage(session_db);
            
            // Initialize and start MIDI monitor
            let mut midi_monitor = recording::MidiMonitor::new(app_handle.clone());
            if let Err(e) = midi_monitor.start() {
                log::error!("Failed to start MIDI monitor: {}", e);
            }
            app.manage(Arc::new(Mutex::new(midi_monitor)));
            
            // Setup system tray
            if let Err(e) = tray::setup_tray(&app_handle) {
                log::error!("Failed to setup tray: {}", e);
            }
            
            log::info!("Sacho initialized successfully");
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_audio_devices,
            commands::get_midi_devices,
            commands::get_video_devices,
            commands::get_recording_state,
            commands::start_recording,
            commands::stop_recording,
            commands::get_sessions,
            commands::get_session_detail,
            commands::delete_session,
            commands::update_session_favorite,
            commands::update_session_notes,
            commands::get_config,
            commands::update_config,
            commands::get_similarity_data,
            commands::recalculate_similarity,
            commands::rescan_sessions,
            commands::restart_midi_monitor,
            commands::read_session_file,
            commands::check_video_codec,
            commands::get_video_info,
            commands::get_video_frame,
            commands::get_video_frames_batch,
            commands::get_video_frame_timestamps,
            commands::get_encoder_availability,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sacho");
}
