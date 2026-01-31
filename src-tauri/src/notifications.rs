// Desktop notifications

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Send a notification when recording starts
pub fn notify_recording_started(app: &AppHandle, devices: &[String]) {
    let device_list = if devices.is_empty() {
        "No devices".to_string()
    } else if devices.len() <= 3 {
        devices.join(", ")
    } else {
        format!("{} and {} more", devices[..2].join(", "), devices.len() - 2)
    };
    
    let _ = app.notification()
        .builder()
        .title("Recording Started")
        .body(format!("Recording on: {}", device_list))
        .show();
}

/// Send a notification when recording stops
pub fn notify_recording_stopped(app: &AppHandle, duration_secs: f64, folder_name: &str) {
    let duration_str = format_duration(duration_secs);
    
    let _ = app.notification()
        .builder()
        .title("Recording Saved")
        .body(format!("Duration: {} • Saved to: {}", duration_str, folder_name))
        .show();
}

/// Send a notification for errors
pub fn notify_error(app: &AppHandle, message: &str) {
    let _ = app.notification()
        .builder()
        .title("Sacho Error")
        .body(message)
        .show();
}

/// Format duration as human-readable string
fn format_duration(secs: f64) -> String {
    let total_secs = secs as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}
