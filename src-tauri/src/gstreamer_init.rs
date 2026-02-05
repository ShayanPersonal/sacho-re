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
        
        // Use a private registry file to avoid conflicts with system GStreamer
        if let Some(local_app_data) = dirs::data_local_dir() {
            let registry_path = local_app_data
                .join("com.sacho.app")
                .join("gst-registry.bin");
            
            if let Some(parent) = registry_path.parent() {
                let _ = std::fs::create_dir_all(parent);
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
