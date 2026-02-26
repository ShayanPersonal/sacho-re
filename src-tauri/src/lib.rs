// Sacho - Automatic Recording Studio Companion
// Main library entry point

pub mod autostart;
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
#[cfg(feature = "test-harness")]
pub mod test_harness;
pub mod video;

use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use tauri::Manager;
use sysinfo::System;

/// Initialize and run the Tauri application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Check for --console flag to enable console logging
    let enable_console = std::env::args().any(|arg| arg == "--console");
    
    if enable_console {
        // On Windows, attach to parent console (if launched from cmd/powershell)
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
        
        // Initialize logger with a sensible default level if RUST_LOG isn't set
        env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("info")
        ).init();
    }
    
    // Register with Windows Error Reporting for automatic restart on crash/hang
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Recovery::RegisterApplicationRestart;
        let restart_args: Vec<u16> = "--autostarted\0".encode_utf16().collect();
        unsafe { RegisterApplicationRestart(restart_args.as_ptr(), 0); }
    }

    // Initialize GStreamer environment before anything else
    // This sets up paths for private GStreamer deployment on Windows
    gstreamer_init::init_gstreamer_env();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostarted"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // The NSIS uninstaller (PREUNINSTALL hook) launches a second
            // instance with --quit to ask us to shut down gracefully.  This
            // lets midir close WinMM MIDI handles before the process exits;
            // without it, some USB MIDI drivers leave the device marked
            // "in use" system-wide after a force-kill.
            if args.iter().any(|a| a == "--quit") {
                log::info!("Received --quit from uninstaller, shutting down gracefully");
                let midi_monitor = app.state::<Arc<Mutex<recording::MidiMonitor>>>();
                midi_monitor.lock().stop();
                app.exit(0);
                return;
            }

            // Focus the existing window when a second instance tries to launch,
            // but not if the second instance was also an autostart (e.g. both
            // HKCU and HKLM Run entries exist from a legacy install).
            let is_autostart = args.iter().any(|a| a == "--autostarted");
            if !is_autostart {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
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
            // If --quit was passed but we reached setup, it means no other
            // instance was running (the single-instance plugin would have
            // intercepted and exited).  Exit immediately without initializing
            // any resources — no MIDI ports to open, nothing to clean up.
            if std::env::args().any(|arg| arg == "--quit") {
                log::info!("--quit: no running instance found, exiting");
                std::process::exit(0);
            }

            // Initialize application state
            let app_handle = app.handle().clone();
            
            // Initialize config
            let config = config::Config::load_or_default(&app_handle);
            
            // Window starts hidden (visible: false in tauri.conf.json) to prevent
            // a flash on screen when auto-starting. Show it now unless the app
            // was auto-started and the user wants to start hidden.
            let was_autostarted = std::env::args().any(|arg| arg == "--autostarted");
            let should_hide = was_autostarted && config.start_minimized;
            if !should_hide {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            
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

            // Initialize similarity cache and warm it in the background
            app.manage(commands::SimilarityCache::new());
            let handle = app_handle.clone();
            std::thread::spawn(move || {
                let db = handle.state::<session::SessionDatabase>();
                let cache = handle.state::<commands::SimilarityCache>();
                commands::warm_similarity_cache(&db, &cache);
            });

            // Initialize device health state (before MIDI monitor so it's available)
            app.manage(RwLock::new(devices::health::DeviceHealthState::new()));

            // Initialize and start MIDI monitor
            let mut midi_monitor = recording::MidiMonitor::new(app_handle.clone());
            if let Err(e) = midi_monitor.start() {
                log::error!("Failed to start MIDI monitor: {}", e);
            }
            app.manage(Arc::new(Mutex::new(midi_monitor)));
            
            // Initialize sysinfo for process stats (CPU/RAM monitoring)
            let mut sys = System::new();
            sys.refresh_processes(
                sysinfo::ProcessesToUpdate::Some(&[sysinfo::get_current_pid().unwrap()]),
                true,
            );
            app.manage(Mutex::new(sys));
            
            // Setup system tray
            if let Err(e) = tray::setup_tray(&app_handle) {
                log::error!("Failed to setup tray: {}", e);
            }
            
            log::info!("Sacho initialized successfully");
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::refresh_devices,
            commands::get_audio_devices,
            commands::get_midi_devices,
            commands::get_video_devices,
            commands::validate_video_device_config,
            commands::get_recording_state,
            commands::start_recording,
            commands::stop_recording,
            commands::get_sessions,
            commands::get_session_detail,
            commands::repair_session,
            commands::delete_session,
            commands::rename_session,
            commands::update_session_notes,
            commands::get_config,
            commands::update_config,
            commands::update_audio_trigger_thresholds,
            commands::import_midi_folder,
            commands::get_midi_imports,
            commands::get_similar_files,
            commands::clear_midi_imports,
            commands::rescan_sessions,
            commands::reset_cache,
            commands::reset_settings,
            commands::restart_midi_monitor,
            commands::read_session_file,
            commands::check_video_codec,
            commands::get_video_info,
            commands::get_video_frame,
            commands::get_video_frames_batch,
            commands::get_video_frame_timestamps,
            commands::get_encoder_availability,
            commands::test_encoder_preset,
            commands::auto_select_encoder_preset,
            commands::set_custom_sound,
            commands::clear_custom_sound,
            commands::get_autostart_info,
            commands::set_all_users_autostart,
            commands::simulate_crash,
            commands::get_app_stats,
            commands::get_disconnected_devices,
            commands::restart_device_pipelines,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Sacho")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Explicitly stop the MIDI monitor so midir closes all WinMM
                // handles before the process exits.  Without this,
                // std::process::exit() skips Drop impls and some USB MIDI
                // drivers leave the device marked "in use" system-wide.
                let midi_monitor = app.state::<Arc<Mutex<recording::MidiMonitor>>>();
                midi_monitor.lock().stop();
            }
        });
}
