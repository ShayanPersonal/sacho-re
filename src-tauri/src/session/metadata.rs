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
    
    /// User-defined tags
    pub tags: Vec<String>,
    
    /// User notes
    pub notes: String,
    
    /// Whether this session is marked as favorite
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFileInfo {
    pub filename: String,
    pub device_name: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_secs: f64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiFileInfo {
    pub filename: String,
    pub device_name: String,
    pub event_count: usize,
    pub size_bytes: u64,
    /// True if the MIDI file has a corrupt header (e.g., from an interrupted recording).
    /// This field is computed at load time, not persisted.
    #[serde(default)]
    pub needs_repair: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFileInfo {
    pub filename: String,
    pub device_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_secs: f64,
    pub size_bytes: u64,
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
    pub audio_count: usize,
    pub midi_count: usize,
    pub video_count: usize,
    pub total_size_bytes: u64,
    pub is_favorite: bool,
    pub tags: Vec<String>,
    pub notes: String,
}

impl From<&SessionMetadata> for SessionSummary {
    fn from(meta: &SessionMetadata) -> Self {
        let total_size = meta.audio_files.iter().map(|f| f.size_bytes).sum::<u64>()
            + meta.midi_files.iter().map(|f| f.size_bytes).sum::<u64>()
            + meta.video_files.iter().map(|f| f.size_bytes).sum::<u64>();
        
        Self {
            id: meta.id.clone(),
            timestamp: meta.timestamp,
            duration_secs: meta.duration_secs,
            has_audio: !meta.audio_files.is_empty()
                || meta.video_files.iter().any(|v| v.has_audio),
            has_midi: !meta.midi_files.is_empty(),
            has_video: !meta.video_files.is_empty(),
            audio_count: meta.audio_files.len(),
            midi_count: meta.midi_files.len(),
            video_count: meta.video_files.len(),
            total_size_bytes: total_size,
            is_favorite: meta.is_favorite,
            tags: meta.tags.clone(),
            notes: meta.notes.clone(),
        }
    }
}
