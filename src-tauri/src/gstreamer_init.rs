//! GStreamer initialization and environment configuration
//!
//! This module handles setting up GStreamer to use the private deployment
//! that may be bundled with the Sacho installer on Windows.

use std::env;
use std::sync::Once;

static GSTREAMER_INIT: Once = Once::new();

/// Initialize GStreamer environment
/// 
/// On Windows, this checks for a private GStreamer deployment in the app folder
/// and configures the environment to use it. This must be called before any
/// GStreamer functions are used.
pub fn init_gstreamer_env() {
    GSTREAMER_INIT.call_once(|| {
        #[cfg(target_os = "windows")]
        {
            if let Err(e) = setup_private_gstreamer_windows() {
                log::warn!("Failed to set up private GStreamer: {}", e);
                log::info!("Will attempt to use system GStreamer installation");
            }
        }
        
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
fn setup_private_gstreamer_windows() -> Result<(), String> {
    // Get the executable's directory
    let exe_path = env::current_exe()
        .map_err(|e| format!("Failed to get executable path: {}", e))?;
    
    let app_dir = exe_path.parent()
        .ok_or("Failed to get app directory")?;
    
    // Check for private GStreamer deployment
    let gstreamer_dir = app_dir.join("gstreamer");
    let gstreamer_bin = gstreamer_dir.join("1.0").join("msvc_x86_64").join("bin");
    let gstreamer_lib = gstreamer_dir.join("1.0").join("msvc_x86_64").join("lib").join("gstreamer-1.0");
    
    // Also check alternative path structure (direct install)
    let gstreamer_bin_alt = gstreamer_dir.join("bin");
    let gstreamer_lib_alt = gstreamer_dir.join("lib").join("gstreamer-1.0");
    
    let (bin_dir, plugin_dir) = if gstreamer_bin.exists() {
        (gstreamer_bin, gstreamer_lib)
    } else if gstreamer_bin_alt.exists() {
        (gstreamer_bin_alt, gstreamer_lib_alt)
    } else {
        // Check if .sacho-installed marker exists
        let marker = gstreamer_dir.join(".sacho-installed");
        if marker.exists() {
            return Err(format!(
                "GStreamer marker exists but binaries not found. Expected at: {:?} or {:?}",
                gstreamer_bin, gstreamer_bin_alt
            ));
        }
        // No private deployment, will use system GStreamer
        log::debug!("No private GStreamer deployment found at {:?}", gstreamer_dir);
        return Ok(());
    };
    
    log::info!("Found private GStreamer deployment at {:?}", bin_dir.parent().unwrap_or(&gstreamer_dir));
    
    // Add GStreamer bin to PATH
    let path = env::var("PATH").unwrap_or_default();
    let new_path = format!("{};{}", bin_dir.display(), path);
    env::set_var("PATH", &new_path);
    log::debug!("Added to PATH: {}", bin_dir.display());
    
    // Set GST_PLUGIN_PATH to the private plugins
    if plugin_dir.exists() {
        env::set_var("GST_PLUGIN_PATH", plugin_dir.to_str().unwrap_or_default());
        log::debug!("Set GST_PLUGIN_PATH: {}", plugin_dir.display());
    }
    
    // Set GST_PLUGIN_SCANNER if it exists
    let scanner = bin_dir.join("gst-plugin-scanner.exe");
    if scanner.exists() {
        env::set_var("GST_PLUGIN_SCANNER", scanner.to_str().unwrap_or_default());
    }
    
    // Disable plugin registry to avoid conflicts with system GStreamer
    // This forces GStreamer to scan plugins fresh
    env::set_var("GST_REGISTRY_FORK", "no");
    
    // Use a private registry file in the app's data directory
    if let Some(local_app_data) = dirs::data_local_dir() {
        let registry_path = local_app_data
            .join("com.sacho.app")
            .join("gst-registry.bin");
        
        // Ensure parent directory exists
        if let Some(parent) = registry_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        
        env::set_var("GST_REGISTRY", registry_path.to_str().unwrap_or_default());
        log::debug!("Set GST_REGISTRY: {}", registry_path.display());
    }
    
    Ok(())
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

/// Get information about the GStreamer installation for diagnostics
pub fn get_gstreamer_info() -> GStreamerInfo {
    // Try to get version - this will work if GStreamer is initialized
    let version = match gstreamer::init() {
        Ok(_) => {
            let (major, minor, micro, _) = gstreamer::version();
            Some(format!("{}.{}.{}", major, minor, micro))
        }
        Err(_) => None,
    };
    
    let is_available = version.is_some();
    
    #[cfg(target_os = "windows")]
    let deployment_type = {
        let exe_path = env::current_exe().ok();
        let private_dir = exe_path.as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("gstreamer"));
        
        if private_dir.as_ref().map(|p| p.exists()).unwrap_or(false) {
            DeploymentType::Private
        } else {
            DeploymentType::System
        }
    };
    
    #[cfg(not(target_os = "windows"))]
    let deployment_type = DeploymentType::System;
    
    GStreamerInfo {
        is_available,
        version,
        deployment_type,
        plugin_path: env::var("GST_PLUGIN_PATH").ok(),
    }
}

#[derive(Debug, Clone)]
pub struct GStreamerInfo {
    pub is_available: bool,
    pub version: Option<String>,
    pub deployment_type: DeploymentType,
    pub plugin_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeploymentType {
    /// Using a private GStreamer bundled with the app
    Private,
    /// Using the system-wide GStreamer installation
    System,
}
