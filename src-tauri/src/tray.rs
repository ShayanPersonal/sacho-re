// System tray management

use crate::recording::{RecordingStatus, MidiMonitor};
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::{
    AppHandle,
    Manager, Runtime,
    tray::{TrayIconBuilder, MouseButton, MouseButtonState},
    menu::{Menu, MenuItem},
};

/// Holds references to tray menu items that need dynamic enable/disable
pub struct TrayMenuItems<R: Runtime> {
    pub start: MenuItem<R>,
    pub stop: MenuItem<R>,
}

/// Tray icon state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayState {
    Idle,
    Recording,
    Stopping,
    Initializing,
}

impl From<RecordingStatus> for TrayState {
    fn from(status: RecordingStatus) -> Self {
        match status {
            RecordingStatus::Idle => TrayState::Idle,
            RecordingStatus::Recording => TrayState::Recording,
            RecordingStatus::Stopping => TrayState::Stopping,
            RecordingStatus::Initializing => TrayState::Initializing,
        }
    }
}

/// Create and configure the system tray
pub fn setup_tray(app: &AppHandle) -> anyhow::Result<()> {
    // Create menu items
    let open_item = MenuItem::with_id(app, "open", "Open Sacho", true, None::<&str>)?;
    let start_item = MenuItem::with_id(app, "start", "Start Recording", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop Recording", false, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    // Store references for dynamic enable/disable in update_tray_state
    app.manage(TrayMenuItems {
        start: start_item.clone(),
        stop: stop_item.clone(),
    });

    // Build menu
    let menu = Menu::with_items(app, &[
        &open_item,
        &start_item,
        &stop_item,
        &quit_item,
    ])?;
    
    // Build tray icon with a unique ID for later lookup
    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().expect("Failed to load tray icon"))
        .tooltip("Sacho - Idle")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "open" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "start" => {
                    log::info!("Start recording requested from tray");
                    let midi_monitor = app.state::<Arc<Mutex<MidiMonitor>>>();
                    let monitor = midi_monitor.lock();
                    if let Err(e) = monitor.manual_start_recording() {
                        log::warn!("Could not start recording from tray: {}", e);
                    }
                }
                "stop" => {
                    log::info!("Stop recording requested from tray");
                    let midi_monitor = app.state::<Arc<Mutex<MidiMonitor>>>();
                    let monitor = midi_monitor.lock();
                    if let Err(e) = monitor.manual_stop_recording() {
                        log::warn!("Could not stop recording from tray: {}", e);
                    }
                }
                "quit" => {
                    // Explicitly stop MIDI monitor before exiting so that
                    // midir's MidiInputConnection::close() runs and releases
                    // WinMM handles.  Without this, std::process::exit()
                    // skips Drop impls and some USB MIDI drivers leave the
                    // port marked "in use" system-wide.
                    let midi_monitor = app.state::<Arc<Mutex<MidiMonitor>>>();
                    midi_monitor.lock().stop();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { 
                button: MouseButton::Left, 
                button_state: MouseButtonState::Up,
                .. 
            } = event {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;
    
    log::info!("System tray initialized");
    
    Ok(())
}

/// Update tray icon and tooltip based on recording state
pub fn update_tray_state(app: &AppHandle, state: TrayState) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let tooltip = match state {
            TrayState::Idle => "Sacho - Idle",
            TrayState::Recording => "Sacho - Recording",
            TrayState::Stopping => "Sacho - Stopping...",
            TrayState::Initializing => "Sacho - Initializing...",
        };

        let _ = tray.set_tooltip(Some(tooltip));

        // Toggle start/stop enabled state based on recording status
        let is_idle = state == TrayState::Idle;
        let items = app.state::<TrayMenuItems<tauri::Wry>>();
        let _ = items.start.set_enabled(is_idle);
        let _ = items.stop.set_enabled(!is_idle);
    }
}
