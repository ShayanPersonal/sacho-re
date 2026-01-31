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
    
    /// Extracted MIDI features for similarity analysis
    pub midi_features: Option<MidiFeatures>,
    
    /// 2D coordinates from similarity analysis
    pub similarity_coords: Option<SimilarityCoords>,
    
    /// Cluster ID from similarity analysis
    pub cluster_id: Option<i32>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFileInfo {
    pub filename: String,
    pub device_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_secs: f64,
    pub size_bytes: u64,
}

/// Extracted MIDI features for similarity analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiFeatures {
    /// Pitch class histogram (12 bins, normalized)
    pub pitch_class_histogram: [f32; 12],
    
    /// Interval histogram (-12 to +12 semitones = 25 bins)
    pub interval_histogram: [f32; 25],
    
    /// Contour n-grams (3^3 = 27 possible 3-grams)
    pub contour_ngrams: [f32; 27],
    
    /// Rhythm metrics
    pub notes_per_second: f32,
    pub ioi_mean: f32,
    pub ioi_variance: f32,
    pub tempo_estimate: f32,
    
    /// Pitch statistics
    pub pitch_min: u8,
    pub pitch_max: u8,
    pub pitch_mean: f32,
    pub pitch_std: f32,
    pub pitch_range: u8,
    pub tessitura: f32,
    
    /// Duration histogram (8 buckets)
    pub duration_histogram: [f32; 8],
    
    /// Polyphony metrics
    pub avg_simultaneous_notes: f32,
    pub chord_frequency: f32,
    pub voice_count_estimate: u8,
    pub polyphony_ratio: f32,
}

impl MidiFeatures {
    /// Convert features to a flat vector for dimensionality reduction
    pub fn to_vector(&self) -> Vec<f32> {
        let mut vec = Vec::with_capacity(86);
        
        // Pitch class histogram (12)
        vec.extend_from_slice(&self.pitch_class_histogram);
        
        // Interval histogram (25)
        vec.extend_from_slice(&self.interval_histogram);
        
        // Contour n-grams (27)
        vec.extend_from_slice(&self.contour_ngrams);
        
        // Rhythm metrics (4)
        vec.push(self.notes_per_second);
        vec.push(self.ioi_mean);
        vec.push(self.ioi_variance);
        vec.push(self.tempo_estimate);
        
        // Pitch statistics (6)
        vec.push(self.pitch_min as f32 / 127.0);
        vec.push(self.pitch_max as f32 / 127.0);
        vec.push(self.pitch_mean / 127.0);
        vec.push(self.pitch_std / 127.0);
        vec.push(self.pitch_range as f32 / 127.0);
        vec.push(self.tessitura);
        
        // Duration histogram (8)
        vec.extend_from_slice(&self.duration_histogram);
        
        // Polyphony metrics (4)
        vec.push(self.avg_simultaneous_notes / 10.0);
        vec.push(self.chord_frequency);
        vec.push(self.voice_count_estimate as f32 / 10.0);
        vec.push(self.polyphony_ratio);
        
        vec
    }
}

/// 2D coordinates for similarity visualization
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SimilarityCoords {
    pub x: f32,
    pub y: f32,
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
    pub similarity_coords: Option<SimilarityCoords>,
    pub cluster_id: Option<i32>,
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
            has_audio: !meta.audio_files.is_empty(),
            has_midi: !meta.midi_files.is_empty(),
            has_video: !meta.video_files.is_empty(),
            audio_count: meta.audio_files.len(),
            midi_count: meta.midi_files.len(),
            video_count: meta.video_files.len(),
            total_size_bytes: total_size,
            is_favorite: meta.is_favorite,
            tags: meta.tags.clone(),
            notes: meta.notes.clone(),
            similarity_coords: meta.similarity_coords,
            cluster_id: meta.cluster_id,
        }
    }
}
