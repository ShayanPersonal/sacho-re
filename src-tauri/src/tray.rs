// System tray management

use crate::recording::{RecordingStatus, MidiMonitor};
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::{
    AppHandle, 
    Manager,
    tray::{TrayIconBuilder, MouseButton, MouseButtonState},
    menu::{Menu, MenuItem},
};

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
    let stop_item = MenuItem::with_id(app, "stop", "Stop Recording", false, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    
    // Build menu
    let menu = Menu::with_items(app, &[
        &open_item,
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
                "stop" => {
                    log::info!("Stop recording requested from tray");
                    let midi_monitor = app.state::<Arc<Mutex<MidiMonitor>>>();
                    let monitor = midi_monitor.lock();
                    if let Err(e) = monitor.manual_stop_recording() {
                        log::warn!("Could not stop recording from tray: {}", e);
                    }
                }
                "quit" => {
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
        
        // TODO: Update icon based on state
        // let icon_path = match state {
        //     TrayState::Idle => "icons/tray-idle.png",
        //     TrayState::Recording => "icons/tray-recording.png",
        //     TrayState::Stopping => "icons/tray-stopping.png",
        // };
        // let _ = tray.set_icon(Some(icon_path));
    }
}
