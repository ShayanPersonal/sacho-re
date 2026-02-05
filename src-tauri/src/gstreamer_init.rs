//! GStreamer initialization and environment configuration
//!
//! On Windows, GStreamer runtime and plugin DLLs are bundled in the same
//! directory as the executable. This module sets up the environment so
//! GStreamer can find its plugins.

use std::env;
use std::sync::Once;

static GSTREAMER_INIT: Once = Once::new();

/// Initialize GStreamer environment
/// 
/// On Windows, this sets GST_PLUGIN_PATH to the executable's directory
/// where the plugin DLLs are located alongside the runtime DLLs.
pub fn init_gstreamer_env() {
    GSTREAMER_INIT.call_once(|| {
        #[cfg(target_os = "windows")]
        setup_gstreamer_windows();
        
        // Initialize GStreamer
        match gstreamer::init() {
            Ok(_) => {
                log::info!("GStreamer initialized successfully");
                log_gstreamer_version();
            }
            Err(e) => {
                log::error!("Failed to initialize GStreamer: {}", e);
                log::error!("Video capture and playback will not be available");
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn setup_gstreamer_windows() {
    // Get the executable's directory - this is where all DLLs (runtime + plugins) are located
    let exe_dir = match env::current_exe() {
        Ok(path) => path.parent().map(|p| p.to_path_buf()),
        Err(e) => {
            log::warn!("Failed to get executable path: {}", e);
            None
        }
    };
    
    if let Some(app_dir) = exe_dir {
        // Tell GStreamer to look for plugins in the exe directory
        let plugin_path = app_dir.to_str().unwrap_or_default();
        env::set_var("GST_PLUGIN_PATH", plugin_path);
        log::debug!("Set GST_PLUGIN_PATH: {}", plugin_path);
        
        // Use a private, versioned registry file to avoid conflicts with system GStreamer.
        // The registry caches plugin load results (including failures). When bundled DLLs
        // change between versions (e.g., new dependencies added), a stale registry will
        // still report plugins as failed. Using a version-specific registry path ensures
        // a fresh scan after every update.
        if let Some(local_app_data) = dirs::data_local_dir() {
            let app_version = env!("CARGO_PKG_VERSION");
            let registry_path = local_app_data
                .join("com.sacho.app")
                .join(format!("gst-registry-v{}.bin", app_version));
            
            if let Some(parent) = registry_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            
            // Clean up old registry files from previous versions
            if let Some(parent) = registry_path.parent() {
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("gst-registry") && name_str.ends_with(".bin") 
                            && name_str != registry_path.file_name().unwrap_or_default().to_string_lossy() 
                        {
                            log::debug!("Removing old registry: {}", name_str);
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
            
            env::set_var("GST_REGISTRY", registry_path.to_str().unwrap_or_default());
            log::debug!("Set GST_REGISTRY: {}", registry_path.display());
        }
    }
}

fn log_gstreamer_version() {
    let (major, minor, micro, nano) = gstreamer::version();
    let nano_str = match nano {
        0 => String::new(),
        1 => " (CVS)".to_string(),
        2 => " (prerelease)".to_string(),
        _ => format!(" (nano: {})", nano),
    };
    log::info!("GStreamer version: {}.{}.{}{}", major, minor, micro, nano_str);
}

/// Check if GStreamer is available and properly configured
pub fn is_gstreamer_available() -> bool {
    gstreamer::init().is_ok()
}
