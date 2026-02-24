// Device discovery and enumeration

pub mod enumeration;
pub mod health;

pub use enumeration::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Manages all device discovery and monitoring
pub struct DeviceManager {
    /// Cached audio devices
    pub audio_devices: Vec<AudioDevice>,
    /// Cached MIDI devices
    pub midi_devices: Vec<MidiDevice>,
    /// Cached video devices
    pub video_devices: Vec<VideoDevice>,
}

impl DeviceManager {
    pub fn new() -> Self {
        let mut manager = Self {
            audio_devices: Vec::new(),
            midi_devices: Vec::new(),
            video_devices: Vec::new(),
        };
        manager.refresh_all();
        manager
    }
    
    /// Refresh all device lists
    pub fn refresh_all(&mut self) {
        self.audio_devices = enumeration::enumerate_audio_devices();
        self.midi_devices = enumeration::enumerate_midi_devices();
        self.video_devices = enumeration::enumerate_video_devices();
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents an audio input device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub is_default: bool,
}

/// Represents a MIDI input device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiDevice {
    pub id: String,
    pub name: String,
    pub port_index: usize,
}

/// Per-codec resolution capability: a resolution and its available framerates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodecCapability {
    pub width: u32,
    pub height: u32,
    /// Available framerates at this resolution, sorted descending (highest first).
    /// Values are stored as f64 to preserve fractional rates (e.g. 29.97 for 30000/1001).
    pub framerates: Vec<f64>,
}

/// Represents a video capture device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDevice {
    pub id: String,
    pub name: String,
    /// Per-format capabilities: format string -> list of resolutions with available framerates.
    /// Format strings are the actual pixel/codec names from GStreamer (e.g. "YUY2", "NV12", "MJPEG", "H264").
    pub capabilities: HashMap<String, Vec<CodecCapability>>,
}

impl VideoDevice {
    /// Check if this device supports any recording format
    pub fn is_supported(&self) -> bool {
        !self.capabilities.is_empty()
    }

    /// Get the preferred source format for recording.
    ///
    /// Priority: YUY2 > NV12 > I420 > YV12 > BGR > MJPEG > H264 > AV1 > VP9 > VP8
    /// Raw pixel formats first (highest quality, we encode ourselves),
    /// then pre-encoded formats for passthrough.
    pub fn preferred_format(&self) -> Option<&str> {
        const PRIORITY: &[&str] = &[
            "YUY2", "NV12", "I420", "YV12", "BGR", "BGRx",
            "MJPEG", "H264", "AV1", "VP9", "VP8",
        ];

        for fmt in PRIORITY {
            if self.capabilities.contains_key(*fmt) {
                return Some(fmt);
            }
        }

        // Fall back to first available format
        self.capabilities.keys().next().map(|s| s.as_str())
    }

    /// Get the best default resolution and fps for a given format.
    /// Returns (width, height, fps) picking the highest resolution then highest fps.
    pub fn best_mode(&self, format: &str) -> Option<(u32, u32, f64)> {
        let caps = self.capabilities.get(format)?;
        // Capabilities are sorted highest resolution first
        let best = caps.first()?;
        let fps = best.framerates.first().copied().unwrap_or(30.0);
        Some((best.width, best.height, fps))
    }

    /// Compute the smart default configuration for this device.
    ///
    /// Defaults:
    /// - Format: YUY2 > NV12 > I420 > YV12 > BGR > MJPEG > H264 > AV1 > VP9 > VP8
    /// - Resolution: min(highest available, 1080p)
    /// - FPS: min(highest available at chosen resolution, ~30)
    /// - Target: "Match Source" (0/0/0.0 sentinel)
    pub fn default_config(&self) -> Option<crate::config::VideoDeviceConfig> {
        let format = self.preferred_format()?.to_string();
        let caps = self.capabilities.get(&format)?;
        if caps.is_empty() { return None; }

        use crate::config::{DEFAULT_TARGET_HEIGHT, DEFAULT_TARGET_FPS, DEFAULT_TARGET_FPS_TOLERANCE};

        // Find best resolution: highest that's ≤ default target height, or smallest available
        let chosen_cap = caps.iter()
            .find(|c| c.height <= DEFAULT_TARGET_HEIGHT)
            .or_else(|| caps.last())?;

        let width = chosen_cap.width;
        let height = chosen_cap.height;

        // Find best fps at this resolution: highest that's ≤ default target fps, or lowest available
        let fps = chosen_cap.framerates.iter()
            .copied()
            .find(|&f| f <= DEFAULT_TARGET_FPS_TOLERANCE)
            .or_else(|| chosen_cap.framerates.last().copied())
            .unwrap_or(DEFAULT_TARGET_FPS);

        let is_raw = crate::encoding::is_raw_format(&format);

        Some(crate::config::VideoDeviceConfig {
            source_format: format,
            source_width: width,
            source_height: height,
            source_fps: fps,
            passthrough: !is_raw, // Default: encode for raw, passthrough for pre-encoded
            encoding_codec: None,   // Auto-detect
            encoder_type: None,     // Auto-detect
            preset_level: crate::encoding::DEFAULT_PRESET,
            effort_level: crate::encoding::DEFAULT_PRESET,
            video_bit_depth: None,
            target_width: 0,   // "Match Source"
            target_height: 0,  // "Match Source"
            target_fps: 0.0,   // "Match Source"
        })
    }
}
