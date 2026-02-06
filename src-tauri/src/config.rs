// Configuration management for Sacho

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::encoding::VideoCodec;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path where recordings are stored
    pub storage_path: PathBuf,
    
    /// Idle timeout in seconds before recording stops
    pub idle_timeout_secs: u32,
    
    /// Pre-roll buffer duration in seconds (0-5)
    /// When recording starts, include this many seconds of prior data
    #[serde(default = "default_pre_roll_secs")]
    pub pre_roll_secs: u32,
    
    /// Audio format for recordings
    pub audio_format: AudioFormat,
    
    /// Video encoding mode for raw video sources
    /// Pre-encoded sources (like MJPEG from webcams) are passed through without re-encoding
    #[serde(default)]
    pub video_encoding_mode: VideoEncodingMode,
    
    /// Whether to use dark color scheme (default is light)
    #[serde(default)]
    pub dark_mode: bool,
    
    /// Whether to start with system
    pub auto_start: bool,
    
    /// Whether to minimize to tray on close
    pub minimize_to_tray: bool,
    
    /// Whether to show notification when recording starts
    #[serde(default = "default_true")]
    pub notify_recording_start: bool,
    
    /// Whether to show notification when recording stops
    #[serde(default = "default_true")]
    pub notify_recording_stop: bool,
    
    /// Selected audio device IDs
    pub selected_audio_devices: Vec<String>,
    
    /// Selected MIDI device IDs for recording
    pub selected_midi_devices: Vec<String>,
    
    /// MIDI device IDs that trigger recording
    pub trigger_midi_devices: Vec<String>,
    
    /// Selected video device IDs
    pub selected_video_devices: Vec<String>,
    
    /// Selected codec per video device (device_id -> codec)
    /// If not set for a device, the preferred codec is used automatically
    #[serde(default)]
    pub video_device_codecs: HashMap<String, VideoCodec>,
    
    /// Device presets
    pub device_presets: Vec<DevicePreset>,
    
    /// Current preset name (if any)
    pub current_preset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Wav,
    Flac,
    Opus,
}

/// Video encoding mode for raw video sources
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VideoEncodingMode {
    /// Use AV1 encoding (royalty-free, best compression, hardware or software)
    Av1,
    /// Use VP9 encoding (royalty-free, excellent compression, hardware or software)
    Vp9,
    /// Use VP8 encoding (royalty-free, widely compatible, hardware or software)
    Vp8,
    /// Keep video raw/uncompressed (largest files, no quality loss)
    Raw,
}

impl Default for VideoEncodingMode {
    fn default() -> Self {
        // Default to VP8 as it always has software fallback
        // The frontend will override with the recommended encoder based on hardware availability
        Self::Vp8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePreset {
    pub name: String,
    pub audio_devices: Vec<String>,
    pub midi_devices: Vec<String>,
    pub trigger_midi_devices: Vec<String>,
    pub video_devices: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage_path: get_default_storage_path(),
            idle_timeout_secs: 5,
            pre_roll_secs: 2, // Default to 2 seconds of pre-roll
            audio_format: AudioFormat::Wav,
            video_encoding_mode: VideoEncodingMode::default(),
            dark_mode: false,
            auto_start: true,
            minimize_to_tray: true,
            notify_recording_start: false,
            notify_recording_stop: true,
            selected_audio_devices: Vec::new(),
            selected_midi_devices: Vec::new(),
            trigger_midi_devices: Vec::new(),
            selected_video_devices: Vec::new(),
            video_device_codecs: HashMap::new(),
            device_presets: Vec::new(),
            current_preset: None,
        }
    }
}

impl Config {
    /// Load config from disk or return default
    pub fn load_or_default(app_handle: &AppHandle) -> Self {
        let config_path = get_config_path(app_handle);
        
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => {
                    match toml::from_str(&contents) {
                        Ok(config) => return config,
                        Err(e) => {
                            log::warn!("Failed to parse config: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to read config file: {}", e);
                }
            }
        }
        
        Self::default()
    }
    
    /// Save config to disk
    pub fn save(&self, app_handle: &AppHandle) -> anyhow::Result<()> {
        let config_path = get_config_path(app_handle);
        
        // Ensure parent directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, contents)?;
        
        Ok(())
    }
}

/// Get the default storage path for recordings
fn get_default_storage_path() -> PathBuf {
    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Sacho")
}

/// Get the config file path
fn get_config_path(app_handle: &AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.toml")
}

/// Default pre-roll duration (for serde)
fn default_pre_roll_secs() -> u32 {
    2
}

/// Default true value (for serde)
fn default_true() -> bool {
    true
}
