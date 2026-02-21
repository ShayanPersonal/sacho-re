// MIDI feature extraction for similarity analysis

use serde::{Deserialize, Serialize};
use super::melody::MelodyNote;
use super::midi_parser::NoteEvent;

/// Combined features for a MIDI file
#[derive(Debug, Clone)]
pub struct MidiFileFeatures {
    pub melodic: Option<MelodicFeatures>,
    pub harmonic: Option<HarmonicFeatures>,
}

/// Melodic features extracted from the skyline melody
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MelodicFeatures {
    /// Signed interval histogram: -12..+12 (25 bins)
    pub interval_histogram: Vec<f32>,
    /// Interval bigrams: consecutive interval pairs, 25x25 = 625 bins
    pub interval_bigrams: Vec<f32>,
    /// 5-level contour trigrams: 5^3 = 125 bins
    pub contour_trigrams: Vec<f32>,
    /// Onset-counted pitch class histogram (12 bins)
    pub pitch_class_histogram: Vec<f32>,
}

/// Harmonic features extracted from all note events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonicFeatures {
    /// Duration-weighted pitch class profile (12 bins)
    pub chroma: Vec<f32>,
    /// Pitch class transition matrix: 12x12 = 144 bins
    pub pc_transitions: Vec<f32>,
}

/// Extract melodic features from skyline melody. Returns None if < 4 notes.
pub fn extract_melodic(melody: &[MelodyNote]) -> Option<MelodicFeatures> {
    if melody.len() < 4 {
        return None;
    }

    // Pitch class histogram (onset-counted)
    let mut pitch_class_histogram = vec![0.0f32; 12];
    for note in melody {
        pitch_class_histogram[(note.pitch % 12) as usize] += 1.0;
    }
    l1_normalize(&mut pitch_class_histogram);

    // Interval histogram (-12 to +12, index 12 = unison)
    let intervals: Vec<i32> = melody.windows(2)
        .map(|w| w[1].pitch as i32 - w[0].pitch as i32)
        .collect();

    let mut interval_histogram = vec![0.0f32; 25];
    for &interval in &intervals {
        let clamped = interval.clamp(-12, 12);
        interval_histogram[(clamped + 12) as usize] += 1.0;
    }
    l1_normalize(&mut interval_histogram);

    // Interval bigrams (25x25 = 625 bins)
    let mut interval_bigrams = vec![0.0f32; 625];
    if intervals.len() >= 2 {
        for pair in intervals.windows(2) {
            let a = (pair[0].clamp(-12, 12) + 12) as usize;
            let b = (pair[1].clamp(-12, 12) + 12) as usize;
            interval_bigrams[a * 25 + b] += 1.0;
        }
        l1_normalize(&mut interval_bigrams);
    }

    // 5-level contour trigrams (5^3 = 125 bins)
    // Levels: 0=big down (<-3), 1=small down (-3..0), 2=same (0), 3=small up (0..3), 4=big up (>3)
    let contours: Vec<u8> = intervals.iter().map(|&d| {
        if d < -3 { 0 }
        else if d < 0 { 1 }
        else if d == 0 { 2 }
        else if d <= 3 { 3 }
        else { 4 }
    }).collect();

    let mut contour_trigrams = vec![0.0f32; 125];
    if contours.len() >= 3 {
        for tri in contours.windows(3) {
            let idx = tri[0] as usize * 25 + tri[1] as usize * 5 + tri[2] as usize;
            contour_trigrams[idx] += 1.0;
        }
        l1_normalize(&mut contour_trigrams);
    }

    Some(MelodicFeatures {
        interval_histogram,
        interval_bigrams,
        contour_trigrams,
        pitch_class_histogram,
    })
}

/// Extract harmonic features from all note events. Returns None if < 2 notes.
pub fn extract_harmonic(events: &[NoteEvent], ticks_per_beat: u16) -> Option<HarmonicFeatures> {
    if events.len() < 2 {
        return None;
    }

    let tpb = ticks_per_beat as f32;

    // Duration-weighted chroma
    let mut chroma = vec![0.0f32; 12];
    for event in events {
        let duration_beats = event.duration_ticks as f32 / tpb;
        chroma[(event.pitch % 12) as usize] += duration_beats;
    }
    l1_normalize(&mut chroma);

    // Pitch class transition matrix (12x12)
    let mut pc_transitions = vec![0.0f32; 144];
    for pair in events.windows(2) {
        let from_pc = (pair[0].pitch % 12) as usize;
        let to_pc = (pair[1].pitch % 12) as usize;
        pc_transitions[from_pc * 12 + to_pc] += 1.0;
    }
    l1_normalize(&mut pc_transitions);

    Some(HarmonicFeatures {
        chroma,
        pc_transitions,
    })
}

fn l1_normalize(arr: &mut [f32]) {
    let sum: f32 = arr.iter().sum();
    if sum > 0.0 {
        for v in arr.iter_mut() {
            *v /= sum;
        }
    }
}
