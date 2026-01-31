// Recording modules

pub mod midi;
pub mod monitor;
pub mod preroll;
pub mod video;

pub use monitor::MidiMonitor;
pub use preroll::{MidiPrerollBuffer, AudioPrerollBuffer};
pub use video::{VideoCaptureManager, VideoError};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use chrono::{DateTime, Utc};

/// Current recording state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RecordingStatus {
    /// Ready to record
    Idle,
    /// Currently recording
    Recording,
    /// Stopping a recording (saving files)
    Stopping,
    /// Reinitializing devices (cannot record during this time)
    Initializing,
}

/// Recording state managed by the application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingState {
    pub status: RecordingStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub current_session_path: Option<PathBuf>,
    pub elapsed_seconds: u64,
    pub active_audio_devices: Vec<String>,
    pub active_midi_devices: Vec<String>,
    pub active_video_devices: Vec<String>,
}

impl RecordingState {
    pub fn new() -> Self {
        Self {
            status: RecordingStatus::Idle,
            started_at: None,
            current_session_path: None,
            elapsed_seconds: 0,
            active_audio_devices: Vec::new(),
            active_midi_devices: Vec::new(),
            active_video_devices: Vec::new(),
        }
    }
    
    pub fn is_recording(&self) -> bool {
        self.status == RecordingStatus::Recording
    }
    
    /// Check if the system is ready to start recording
    pub fn can_start_recording(&self) -> bool {
        self.status == RecordingStatus::Idle
    }
}

impl Default for RecordingState {
    fn default() -> Self {
        Self::new()
    }
}
