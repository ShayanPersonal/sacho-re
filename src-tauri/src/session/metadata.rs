// Session metadata structures

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use chrono::{DateTime, Utc};

/// Sanitize a device name for use in filenames.
/// Replaces spaces, slashes, backslashes, and colons with underscores.
pub fn sanitize_device_name(name: &str) -> String {
    name.replace(' ', "_")
        .replace('/', "_")
        .replace('\\', "_")
        .replace(':', "_")
}

/// Reverse sanitization: replace underscores back to spaces.
/// Used when extracting device names from filenames.
pub fn unsanitize_device_name(sanitized: &str) -> String {
    sanitized.replace('_', " ")
}

/// Complete session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Unique session ID
    pub id: String,
    
    /// When the session was recorded
    pub timestamp: DateTime<Utc>,
    
    /// Duration in seconds
    pub duration_secs: f64,
    
    /// Path to session folder
    pub path: PathBuf,
    
    /// Audio files in this session
    pub audio_files: Vec<AudioFileInfo>,
    
    /// MIDI files in this session
    pub midi_files: Vec<MidiFileInfo>,
    
    /// Video files in this session
    pub video_files: Vec<VideoFileInfo>,

    /// User notes
    pub notes: String,

    /// Session title (extracted from folder name)
    pub title: Option<String>,

    /// True if a .sacho_recording lock file exists in the session folder.
    #[serde(default)]
    pub recording_in_progress: bool,

    /// ISO 8601 timestamp from the lock file (last heartbeat). Null if no lock.
    #[serde(default)]
    pub recording_lock_updated_at: Option<String>,

    /// True if the lock file's hostname matches this machine. Null/false if no lock.
    #[serde(default)]
    pub recording_lock_is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFileInfo {
    pub filename: String,
    pub device_name: String,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiFileInfo {
    pub filename: String,
    pub device_name: String,
    pub event_count: usize,
    /// True if the MIDI file has a corrupt header (e.g., from an interrupted recording).
    /// This field is computed at load time, not persisted.
    #[serde(default)]
    pub needs_repair: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFileInfo {
    pub filename: String,
    pub device_name: String,
    pub duration_secs: f64,
}

/// Session summary for list display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub duration_secs: f64,
    pub has_audio: bool,
    pub has_midi: bool,
    pub has_video: bool,
    pub notes: String,
    pub title: Option<String>,
}

impl From<&SessionMetadata> for SessionSummary {
    fn from(meta: &SessionMetadata) -> Self {
        Self {
            id: meta.id.clone(),
            timestamp: meta.timestamp,
            duration_secs: meta.duration_secs,
            has_audio: !meta.audio_files.is_empty(),
            has_midi: !meta.midi_files.is_empty(),
            has_video: !meta.video_files.is_empty(),
            notes: meta.notes.clone(),
            title: meta.title.clone(),
        }
    }
}
