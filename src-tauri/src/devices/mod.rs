// Device discovery and enumeration

pub mod enumeration;

pub use enumeration::*;

use crate::encoding::VideoCodec;
use serde::{Deserialize, Serialize};

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

/// Represents a video capture device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDevice {
    pub id: String,
    pub name: String,
    pub resolutions: Vec<Resolution>,
    /// Supported video codecs for this device (can be recorded)
    pub supported_codecs: Vec<VideoCodec>,
    /// All formats detected from the device (for display)
    pub all_formats: Vec<String>,
}

impl VideoDevice {
    /// Check if this device supports any recording codec
    pub fn is_supported(&self) -> bool {
        !self.supported_codecs.is_empty()
    }
    
    /// Get the preferred codec for recording
    /// 
    /// Note: Raw is not included in preferred codecs as it requires explicit selection
    pub fn preferred_codec(&self) -> Option<VideoCodec> {
        // Prefer pre-encoded codecs that work with native player
        // Raw is deliberately not in this list - users must explicitly select it
        // We only use free codecs.
        const PRIORITY: &[VideoCodec] = &[
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
        
        // Fall back to first non-raw codec, or Raw if that's all we have
        self.supported_codecs.iter()
            .find(|c| **c != VideoCodec::Raw)
            .copied()
            .or_else(|| self.supported_codecs.first().copied())
    }
    
    /// Check if this device supports raw video (requires encoding)
    pub fn supports_raw(&self) -> bool {
        self.supported_codecs.contains(&VideoCodec::Raw)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VideoDeviceType {
    Webcam,
    Screen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
