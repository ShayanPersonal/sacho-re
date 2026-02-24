// Configuration management for Sacho

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::encoding::HardwareEncoderType;

/// Default maximum encoding height when the user hasn't chosen a specific target.
/// This is just the initial selection — users can pick higher values in the UI.
pub const DEFAULT_TARGET_HEIGHT: u32 = 1080;

/// Default maximum encoding FPS when the user hasn't chosen a specific target.
/// This is just the initial selection — users can pick higher values in the UI.
pub const DEFAULT_TARGET_FPS: f64 = 30.0;

/// Tolerance for comparing FPS to [`DEFAULT_TARGET_FPS`] (includes 30000/1001 ≈ 29.97).
pub const DEFAULT_TARGET_FPS_TOLERANCE: f64 = 30.5;

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

    /// Whether to play a sound when recording starts
    #[serde(default)]
    pub sound_recording_start: bool,

    /// Whether to play a sound when recording stops
    #[serde(default)]
    pub sound_recording_stop: bool,

    /// Volume for recording start sound (0.0-1.0)
    #[serde(default = "default_sound_volume")]
    pub sound_volume_start: f64,

    /// Volume for recording stop sound (0.0-1.0)
    #[serde(default = "default_sound_volume")]
    pub sound_volume_stop: f64,

    /// Legacy: single volume for both sounds (migrated to per-sound volumes on load)
    #[serde(default, skip_serializing)]
    sound_volume: Option<f64>,

    /// Path to custom start sound file (relative to app config dir, inside sounds/)
    #[serde(default)]
    pub custom_sound_start: Option<String>,

    /// Path to custom stop sound file (relative to app config dir, inside sounds/)
    #[serde(default)]
    pub custom_sound_stop: Option<String>,

    /// Whether to play a warning sound when a device is disconnected
    #[serde(default)]
    pub sound_device_disconnect: bool,

    /// Volume for device disconnect warning sound (0.0-1.0)
    #[serde(default = "default_sound_volume")]
    pub sound_volume_disconnect: f64,

    /// Path to custom disconnect warning sound file
    #[serde(default)]
    pub custom_sound_disconnect: Option<String>,

    /// Selected audio device IDs
    pub selected_audio_devices: Vec<String>,

    /// Selected MIDI device IDs for recording
    pub selected_midi_devices: Vec<String>,

    /// MIDI device IDs that trigger recording
    pub trigger_midi_devices: Vec<String>,

    /// Audio device IDs that trigger recording
    #[serde(default)]
    pub trigger_audio_devices: Vec<String>,

    /// Per-device audio trigger thresholds (device_name -> threshold 0.0-1.0)
    #[serde(default)]
    pub audio_trigger_thresholds: HashMap<String, f64>,

    /// Selected video device IDs
    pub selected_video_devices: Vec<String>,

    /// Per-device video configuration (device_id -> config)
    /// Stores source codec, source resolution/fps, and target resolution/fps per device
    #[serde(default)]
    pub video_device_configs: HashMap<String, VideoDeviceConfig>,

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

/// Per-device video source configuration.
/// Stores the selected source codec, source resolution/fps, encoding settings,
/// and target encoding resolution/fps.
///
/// A target value of 0 (or 0.0 for fps) means "smart default":
/// - Resolution: match source if ≤1080p, else scale to 1080p at source aspect ratio
/// - FPS: match source if ≤30.5fps, else 30.0
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDeviceConfig {
    /// Source format to capture from the device (e.g. "YUY2", "NV12", "MJPEG", "H264").
    /// This is the actual pixel/codec format string from GStreamer, not an abstract enum.
    pub source_format: String,
    /// Source capture width
    pub source_width: u32,
    /// Source capture height
    pub source_height: u32,
    /// Source capture framerate (f64 to preserve fractional rates like 29.97)
    pub source_fps: f64,

    // ── Encoding settings ──────────────────────────────────────────────
    /// true = record as-is, false = encode. Default: true for pre-encoded, false for raw.
    #[serde(default = "default_true")]
    pub passthrough: bool,
    /// Target codec when encoding (AV1/VP9/VP8/FFV1). None = auto-detect best.
    #[serde(default)]
    pub encoding_codec: Option<crate::encoding::VideoCodec>,
    /// Hardware accelerator. None = auto-detect best for codec.
    #[serde(default)]
    pub encoder_type: Option<HardwareEncoderType>,
    /// Quality preset 1-5. Default: 3.
    #[serde(default = "default_preset_level")]
    pub preset_level: u8,
    /// Compute effort level 1-5. Default: 3.
    /// Only affects software encoders (SVT-AV1, libvpx VP9/VP8).
    #[serde(default = "default_preset_level")]
    pub effort_level: u8,
    /// Encoding bit depth for lossless codecs (FFV1). None = 8-bit default.
    /// Only meaningful when encoding_codec = FFV1 and passthrough = false.
    #[serde(default)]
    pub video_bit_depth: Option<u8>,

    // ── Target resolution/fps ──────────────────────────────────────────
    /// Target encoding width. 0 = smart default (match source if ≤1080p, else 1080p).
    pub target_width: u32,
    /// Target encoding height. 0 = smart default (match source if ≤1080p, else 1080p).
    pub target_height: u32,
    /// Target encoding framerate. 0.0 = smart default (match source if ≤30fps, else 30).
    pub target_fps: f64,
}

impl PartialEq for VideoDeviceConfig {
    fn eq(&self, other: &Self) -> bool {
        self.source_format == other.source_format
            && self.source_width == other.source_width
            && self.source_height == other.source_height
            && (self.source_fps - other.source_fps).abs() < 0.001
            && self.passthrough == other.passthrough
            && self.encoding_codec == other.encoding_codec
            && self.encoder_type == other.encoder_type
            && self.preset_level == other.preset_level
            && self.effort_level == other.effort_level
            && self.video_bit_depth == other.video_bit_depth
            && self.target_width == other.target_width
            && self.target_height == other.target_height
            && (self.target_fps - other.target_fps).abs() < 0.001
    }
}

impl VideoDeviceConfig {
    /// Resolve smart-default sentinel values (0 / 0.0) to concrete values.
    ///
    /// When the user hasn't picked a specific target, the sentinel (0) resolves
    /// to the source value when it's within the default limits, otherwise to the
    /// default limit. See [`DEFAULT_TARGET_HEIGHT`] and [`DEFAULT_TARGET_FPS`].
    pub fn resolved(&self) -> Self {
        let resolved_height = if self.target_height == 0 {
            if self.source_height <= DEFAULT_TARGET_HEIGHT {
                self.source_height
            } else {
                DEFAULT_TARGET_HEIGHT
            }
        } else {
            self.target_height
        };

        let resolved_width = if self.target_width == 0 {
            if self.source_height <= DEFAULT_TARGET_HEIGHT {
                self.source_width
            } else {
                // Scale to default height maintaining aspect ratio
                let ratio = self.source_width as f64 / self.source_height as f64;
                let w = (DEFAULT_TARGET_HEIGHT as f64 * ratio).round() as u32;
                // Ensure even width (required by encoders)
                if w % 2 == 0 { w } else { w - 1 }
            }
        } else {
            self.target_width
        };

        let resolved_fps = if self.target_fps == 0.0 {
            if self.source_fps <= DEFAULT_TARGET_FPS_TOLERANCE {
                self.source_fps
            } else {
                DEFAULT_TARGET_FPS
            }
        } else {
            self.target_fps
        };

        Self {
            target_width: resolved_width,
            target_height: resolved_height,
            target_fps: resolved_fps,
            ..self.clone()
        }
    }

    /// Returns the effective encoding codec, resolving `None` to the recommended codec.
    /// Returns `None` if passthrough mode is active (no encoding needed).
    pub fn effective_codec(&self) -> Option<crate::encoding::VideoCodec> {
        if self.passthrough {
            return None;
        }
        Some(self.encoding_codec.unwrap_or_else(|| crate::encoding::get_recommended_codec()))
    }

    /// Returns true if only preset_level differs (no pipeline restart needed).
    pub fn pipeline_fields_equal(&self, other: &Self) -> bool {
        self.source_format == other.source_format
            && self.source_width == other.source_width
            && self.source_height == other.source_height
            && (self.source_fps - other.source_fps).abs() < 0.001
            && self.passthrough == other.passthrough
            && self.encoding_codec == other.encoding_codec
            && self.encoder_type == other.encoder_type
            && self.video_bit_depth == other.video_bit_depth
            && self.target_width == other.target_width
            && self.target_height == other.target_height
            && (self.target_fps - other.target_fps).abs() < 0.001
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePreset {
    pub name: String,
    pub audio_devices: Vec<String>,
    pub midi_devices: Vec<String>,
    pub trigger_midi_devices: Vec<String>,
    #[serde(default)]
    pub trigger_audio_devices: Vec<String>,
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
            dark_mode: false,
            auto_start: true,
            start_minimized: true,
            minimize_to_tray: true,
            notify_recording_start: false,
            notify_recording_stop: true,
            sound_recording_start: false,
            sound_recording_stop: false,
            sound_volume_start: 0.5,
            sound_volume_stop: 0.5,
            sound_volume: None,
            custom_sound_start: None,
            custom_sound_stop: None,
            sound_device_disconnect: false,
            sound_volume_disconnect: 0.5,
            custom_sound_disconnect: None,
            selected_audio_devices: Vec::new(),
            selected_midi_devices: Vec::new(),
            trigger_midi_devices: Vec::new(),
            trigger_audio_devices: Vec::new(),
            audio_trigger_thresholds: HashMap::new(),
            selected_video_devices: Vec::new(),
            video_device_configs: HashMap::new(),
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

        // Migrate legacy single sound_volume to per-sound volumes
        if let Some(legacy_vol) = self.sound_volume.take() {
            let vol = legacy_vol.clamp(0.0, 1.0);
            self.sound_volume_start = vol;
            self.sound_volume_stop = vol;
        }

        if self.sound_volume_start < 0.0 || self.sound_volume_start > 1.0 {
            let old = self.sound_volume_start;
            self.sound_volume_start = self.sound_volume_start.clamp(0.0, 1.0);
            clamped.push(format!("sound_volume_start: {} -> {}", old, self.sound_volume_start));
        }

        if self.sound_volume_stop < 0.0 || self.sound_volume_stop > 1.0 {
            let old = self.sound_volume_stop;
            self.sound_volume_stop = self.sound_volume_stop.clamp(0.0, 1.0);
            clamped.push(format!("sound_volume_stop: {} -> {}", old, self.sound_volume_stop));
        }

        if self.sound_volume_disconnect < 0.0 || self.sound_volume_disconnect > 1.0 {
            let old = self.sound_volume_disconnect;
            self.sound_volume_disconnect = self.sound_volume_disconnect.clamp(0.0, 1.0);
            clamped.push(format!("sound_volume_disconnect: {} -> {}", old, self.sound_volume_disconnect));
        }

        for (key, value) in self.audio_trigger_thresholds.iter_mut() {
            if *value < 0.0 || *value > 1.0 {
                let old = *value;
                *value = value.clamp(0.0, 1.0);
                clamped.push(format!(
                    "audio_trigger_thresholds[{}]: {} -> {}",
                    key, old, *value
                ));
            }
        }

        // Validate per-device preset levels and effort levels
        for (key, dev_config) in self.video_device_configs.iter_mut() {
            if dev_config.preset_level < 1 || dev_config.preset_level > 5 {
                let old = dev_config.preset_level;
                dev_config.preset_level = dev_config.preset_level.clamp(1, 5);
                clamped.push(format!(
                    "video_device_configs[{}].preset_level: {} -> {}",
                    key, old, dev_config.preset_level
                ));
            }
            if dev_config.effort_level < 1 || dev_config.effort_level > 5 {
                let old = dev_config.effort_level;
                dev_config.effort_level = dev_config.effort_level.clamp(1, 5);
                clamped.push(format!(
                    "video_device_configs[{}].effort_level: {} -> {}",
                    key, old, dev_config.effort_level
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

/// Default preset level (for serde)
fn default_preset_level() -> u8 {
    3
}

/// Default sound volume (for serde)
fn default_sound_volume() -> f64 {
    1.0
}
