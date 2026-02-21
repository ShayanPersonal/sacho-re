// Session metadata structures

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use chrono::{DateTime, Utc};

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
    
    /// Whether this session is marked as favorite
    pub is_favorite: bool,
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
    /// Whether this MKV file contains an embedded audio track
    #[serde(default)]
    pub has_audio: bool,
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
    pub is_favorite: bool,
    pub notes: String,
}

impl From<&SessionMetadata> for SessionSummary {
    fn from(meta: &SessionMetadata) -> Self {
        Self {
            id: meta.id.clone(),
            timestamp: meta.timestamp,
            duration_secs: meta.duration_secs,
            has_audio: !meta.audio_files.is_empty()
                || meta.video_files.iter().any(|v| v.has_audio),
            has_midi: !meta.midi_files.is_empty(),
            has_video: !meta.video_files.is_empty(),
            is_favorite: meta.is_favorite,
            notes: meta.notes.clone(),
        }
    }
}
