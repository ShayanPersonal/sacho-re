// Device discovery and enumeration

pub mod enumeration;

pub use enumeration::*;

use crate::encoding::VideoCodec;
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
    /// Supported video codecs for this device (convenience list, derived from capabilities keys)
    pub supported_codecs: Vec<VideoCodec>,
    /// Per-codec capabilities: codec -> list of resolutions with available framerates
    pub capabilities: HashMap<VideoCodec, Vec<CodecCapability>>,
    /// All formats detected from the device (for display)
    pub all_formats: Vec<String>,
}

impl VideoDevice {
    /// Check if this device supports any recording codec
    pub fn is_supported(&self) -> bool {
        !self.supported_codecs.is_empty()
    }
    
    /// Get the preferred codec for recording.
    /// 
    /// Prefers Raw (highest quality, we encode ourselves) first,
    /// then AV1 > VP9 > VP8 > MJPEG for pre-encoded passthrough.
    pub fn preferred_codec(&self) -> Option<VideoCodec> {
        const PRIORITY: &[VideoCodec] = &[
            VideoCodec::Raw,
            VideoCodec::Av1,
            VideoCodec::Vp9,
            VideoCodec::Vp8,
            VideoCodec::Mjpeg,
        ];
        
        for codec in PRIORITY {
            if self.supported_codecs.contains(codec) {
                return Some(*codec);
            }
        }
        
        // Fall back to first available codec
        self.supported_codecs.first().copied()
    }
    
    /// Check if this device supports raw video (requires encoding)
    pub fn supports_raw(&self) -> bool {
        self.supported_codecs.contains(&VideoCodec::Raw)
    }
    
    /// Get the best default resolution and fps for a given codec.
    /// Returns (width, height, fps) picking the highest resolution then highest fps.
    pub fn best_mode(&self, codec: &VideoCodec) -> Option<(u32, u32, f64)> {
        let caps = self.capabilities.get(codec)?;
        // Capabilities are sorted highest resolution first
        let best = caps.first()?;
        let fps = best.framerates.first().copied().unwrap_or(30.0);
        Some((best.width, best.height, fps))
    }
    
    /// Compute the smart default configuration for this device.
    ///
    /// Defaults:
    /// - Codec: Raw > AV1 > VP9 > VP8 > MJPEG
    /// - Resolution: min(highest available, 1080p)
    /// - FPS: min(highest available at chosen resolution, 30)
    /// - Target: "Match Source" (0/0/0.0 sentinel)
    pub fn default_config(&self) -> Option<crate::config::VideoDeviceConfig> {
        let codec = self.preferred_codec()?;
        let caps = self.capabilities.get(&codec)?;
        if caps.is_empty() { return None; }
        
        // Find best resolution: highest that's ≤ 1080p, or smallest available
        let chosen_cap = caps.iter()
            .find(|c| c.height <= 1080)
            .or_else(|| caps.last())?;
        
        let width = chosen_cap.width;
        let height = chosen_cap.height;
        
        // Find best fps at this resolution: highest that's ≤ ~30, or lowest available
        // Use 30.5 tolerance to include 30000/1001 ≈ 29.97
        let fps = chosen_cap.framerates.iter()
            .copied()
            .find(|&f| f <= 30.5)
            .or_else(|| chosen_cap.framerates.last().copied())
            .unwrap_or(30.0);
        
        Some(crate::config::VideoDeviceConfig {
            source_codec: codec,
            source_width: width,
            source_height: height,
            source_fps: fps,
            passthrough: codec.is_preencoded(), // Default: passthrough for pre-encoded, encode for raw
            encoding_codec: None,   // Auto-detect
            encoder_type: None,     // Auto-detect
            preset_level: crate::encoding::DEFAULT_PRESET,
            target_width: 0,   // "Match Source"
            target_height: 0,  // "Match Source"
            target_fps: 0.0,   // "Match Source"
        })
    }
}
