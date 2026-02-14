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

    /// WAV bit depth
    #[serde(default)]
    pub wav_bit_depth: AudioBitDepth,

    /// WAV sample rate
    #[serde(default)]
    pub wav_sample_rate: AudioSampleRate,

    /// FLAC bit depth (Int16, Int24, or 32-bit integer via GStreamer)
    #[serde(default)]
    pub flac_bit_depth: AudioBitDepth,

    /// FLAC sample rate
    #[serde(default)]
    pub flac_sample_rate: AudioSampleRate,

    /// Video encoding mode for raw video sources
    /// Pre-encoded sources (like MJPEG from webcams) are passed through without re-encoding
    #[serde(default)]
    pub video_encoding_mode: VideoEncodingMode,

    /// Whether to use dark color scheme (default is light)
    #[serde(default)]
    pub dark_mode: bool,

    /// Whether to start with system
    pub auto_start: bool,

    /// Whether to hide the window when launched via autostart or crash recovery
    #[serde(default = "default_true")]
    pub start_minimized: bool,

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

    /// Per-device video configuration (device_id -> config)
    /// Stores source codec, source resolution/fps, and target resolution/fps per device
    #[serde(default)]
    pub video_device_configs: HashMap<String, VideoDeviceConfig>,

    /// Encoder quality preset level per encoding mode (1=lightest, 5=highest quality)
    /// Keys are the encoding mode names: "av1", "vp9", "vp8"
    /// See [`crate::encoding::presets`] for per-encoder parameter details.
    #[serde(default)]
    pub encoder_preset_levels: HashMap<String, u8>,

    /// Whether to encode video during pre-roll (trades CPU/GPU compute for memory).
    /// When enabled, the pre-roll limit increases from 5 to 30 seconds.
    /// Only affects raw video sources; passthrough (MJPEG etc.) is already encoded.
    #[serde(default)]
    pub encode_during_preroll: bool,

    /// Whether to combine audio and video into a single MKV file.
    /// When enabled (and exactly 1 video + 1 audio device are selected),
    /// the separate audio file is muxed into the video MKV after recording stops.
    #[serde(default)]
    pub combine_audio_video: bool,

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
}

/// Audio bit depth for recorded files
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudioBitDepth {
    /// 16-bit integer
    Int16,
    /// 24-bit integer
    Int24,
    /// 32-bit float (WAV only)
    Float32,
}

impl Default for AudioBitDepth {
    fn default() -> Self {
        Self::Int24
    }
}

/// Audio sample rate for recorded files
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudioSampleRate {
    /// Use the device's native sample rate
    Passthrough,
    /// 44.1 kHz
    Rate44100,
    /// 48 kHz
    Rate48000,
    /// 88.2 kHz
    Rate88200,
    /// 96 kHz
    Rate96000,
    /// 192 kHz
    Rate192000,
}

impl Default for AudioSampleRate {
    fn default() -> Self {
        Self::Passthrough
    }
}

impl AudioSampleRate {
    /// Get the target sample rate in Hz, or None for passthrough
    pub fn target_rate(&self) -> Option<u32> {
        match self {
            AudioSampleRate::Passthrough => None,
            AudioSampleRate::Rate44100 => Some(44100),
            AudioSampleRate::Rate48000 => Some(48000),
            AudioSampleRate::Rate88200 => Some(88200),
            AudioSampleRate::Rate96000 => Some(96000),
            AudioSampleRate::Rate192000 => Some(192000),
        }
    }
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

/// Per-device video source configuration.
/// Stores the selected source codec, source resolution/fps, and target encoding resolution/fps.
/// When source_codec is not Raw, the video is recorded as passthrough and target fields are ignored.
///
/// A target value of 0 (or 0.0 for fps) means "Match Source" — the encoding will use the
/// source resolution/fps directly without scaling or rate conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDeviceConfig {
    /// Source codec to capture from the device
    pub source_codec: VideoCodec,
    /// Source capture width
    pub source_width: u32,
    /// Source capture height
    pub source_height: u32,
    /// Source capture framerate (f64 to preserve fractional rates like 29.97)
    pub source_fps: f64,
    /// Target encoding width (0 = match source). Only used when source_codec is Raw.
    pub target_width: u32,
    /// Target encoding height (0 = match source). Only used when source_codec is Raw.
    pub target_height: u32,
    /// Target encoding framerate (0.0 = match source). Only used when source_codec is Raw.
    pub target_fps: f64,
}

impl PartialEq for VideoDeviceConfig {
    fn eq(&self, other: &Self) -> bool {
        self.source_codec == other.source_codec
            && self.source_width == other.source_width
            && self.source_height == other.source_height
            && (self.source_fps - other.source_fps).abs() < 0.001
            && self.target_width == other.target_width
            && self.target_height == other.target_height
            && (self.target_fps - other.target_fps).abs() < 0.001
    }
}

impl VideoDeviceConfig {
    /// Resolve "Match Source" sentinel values (0 / 0.0) to actual source values.
    /// Returns a config with concrete target dimensions/fps.
    pub fn resolved(&self) -> Self {
        Self {
            target_width: if self.target_width == 0 {
                self.source_width
            } else {
                self.target_width
            },
            target_height: if self.target_height == 0 {
                self.source_height
            } else {
                self.target_height
            },
            target_fps: if self.target_fps == 0.0 {
                self.source_fps
            } else {
                self.target_fps
            },
            ..self.clone()
        }
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
            wav_bit_depth: AudioBitDepth::default(),
            wav_sample_rate: AudioSampleRate::default(),
            flac_bit_depth: AudioBitDepth::default(),
            flac_sample_rate: AudioSampleRate::default(),
            video_encoding_mode: VideoEncodingMode::default(),
            dark_mode: false,
            auto_start: true,
            start_minimized: true,
            minimize_to_tray: true,
            notify_recording_start: false,
            notify_recording_stop: true,
            selected_audio_devices: Vec::new(),
            selected_midi_devices: Vec::new(),
            trigger_midi_devices: Vec::new(),
            selected_video_devices: Vec::new(),
            video_device_configs: HashMap::new(),
            encoder_preset_levels: HashMap::new(),
            encode_during_preroll: false,
            combine_audio_video: false,
            device_presets: Vec::new(),
            current_preset: None,
        }
    }
}

impl Config {
    /// Validate and clamp config values to safe ranges.
    /// Returns a list of fields that were clamped (empty if all valid).
    pub fn validate(&mut self) -> Vec<String> {
        let mut clamped = Vec::new();

        if self.idle_timeout_secs < 1 || self.idle_timeout_secs > 30 {
            let old = self.idle_timeout_secs;
            self.idle_timeout_secs = self.idle_timeout_secs.clamp(1, 30);
            clamped.push(format!(
                "idle_timeout_secs: {} -> {}",
                old, self.idle_timeout_secs
            ));
        }

        if self.pre_roll_secs > 30 {
            let old = self.pre_roll_secs;
            self.pre_roll_secs = self.pre_roll_secs.clamp(0, 30);
            clamped.push(format!("pre_roll_secs: {} -> {}", old, self.pre_roll_secs));
        }

        for (key, value) in self.encoder_preset_levels.iter_mut() {
            if *value < 1 || *value > 5 {
                let old = *value;
                *value = (*value).clamp(1, 5);
                clamped.push(format!(
                    "encoder_preset_levels[{}]: {} -> {}",
                    key, old, *value
                ));
            }
        }

        if !clamped.is_empty() {
            println!("[Sacho] Config validation clamped: {:?}", clamped);
        }

        clamped
    }

    /// Load config from disk or return default
    pub fn load_or_default(app_handle: &AppHandle) -> Self {
        let config_path = get_config_path(app_handle);

        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => match toml::from_str::<Config>(&contents) {
                    Ok(mut config) => {
                        config.validate();
                        return config;
                    }
                    Err(e) => {
                        log::warn!("Failed to parse config: {}", e);
                    }
                },
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
