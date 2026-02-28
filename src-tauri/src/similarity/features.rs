// MIDI feature extraction for similarity analysis

use serde::{Deserialize, Serialize};
use super::melody::{self, MelodyNote};
use super::midi_parser::{NoteEvent, TempoEvent, tick_to_seconds};

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

/// Extract harmonic features from all note events. Returns None if < 4 notes.
pub fn extract_harmonic(events: &[NoteEvent], ticks_per_beat: u16) -> Option<HarmonicFeatures> {
    if events.len() < 4 {
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

/// Average chunked features from multiple MIDI files into a single set.
/// Used for multi-MIDI sessions (e.g. multiple MIDI devices).
pub fn average_chunked_features(all: &[ChunkedFileFeatures]) -> ChunkedFileFeatures {
    if all.is_empty() {
        return ChunkedFileFeatures { chunks: vec![] };
    }
    if all.len() == 1 {
        return all[0].clone();
    }

    // Group chunks by offset bucket (rounded to 0.1s to avoid float comparison)
    let mut buckets: std::collections::BTreeMap<i32, Vec<&ChunkFeatures>> = std::collections::BTreeMap::new();
    for file_features in all {
        for chunk in &file_features.chunks {
            let key = (chunk.offset_secs * 10.0).round() as i32;
            buckets.entry(key).or_default().push(chunk);
        }
    }

    let chunks = buckets.into_iter().map(|(key, chunk_group)| {
        let offset_secs = key as f32 / 10.0;

        // Average melodic features
        let melodic_refs: Vec<&MelodicFeatures> = chunk_group.iter()
            .filter_map(|c| c.melodic.as_ref())
            .collect();
        let melodic = average_melodic(&melodic_refs);

        // Average harmonic features
        let harmonic_refs: Vec<&HarmonicFeatures> = chunk_group.iter()
            .filter_map(|c| c.harmonic.as_ref())
            .collect();
        let harmonic = average_harmonic(&harmonic_refs);

        ChunkFeatures { offset_secs, melodic, harmonic }
    }).collect();

    ChunkedFileFeatures { chunks }
}

fn average_melodic(features: &[&MelodicFeatures]) -> Option<MelodicFeatures> {
    if features.is_empty() {
        return None;
    }
    let n = features.len() as f32;
    let len_ih = features[0].interval_histogram.len();
    let len_ib = features[0].interval_bigrams.len();
    let len_ct = features[0].contour_trigrams.len();
    let len_pc = features[0].pitch_class_histogram.len();

    let mut interval_histogram = vec![0.0f32; len_ih];
    let mut interval_bigrams = vec![0.0f32; len_ib];
    let mut contour_trigrams = vec![0.0f32; len_ct];
    let mut pitch_class_histogram = vec![0.0f32; len_pc];

    for f in features {
        for (i, v) in f.interval_histogram.iter().enumerate() { interval_histogram[i] += v; }
        for (i, v) in f.interval_bigrams.iter().enumerate() { interval_bigrams[i] += v; }
        for (i, v) in f.contour_trigrams.iter().enumerate() { contour_trigrams[i] += v; }
        for (i, v) in f.pitch_class_histogram.iter().enumerate() { pitch_class_histogram[i] += v; }
    }

    for v in &mut interval_histogram { *v /= n; }
    for v in &mut interval_bigrams { *v /= n; }
    for v in &mut contour_trigrams { *v /= n; }
    for v in &mut pitch_class_histogram { *v /= n; }

    Some(MelodicFeatures {
        interval_histogram,
        interval_bigrams,
        contour_trigrams,
        pitch_class_histogram,
    })
}

fn average_harmonic(features: &[&HarmonicFeatures]) -> Option<HarmonicFeatures> {
    if features.is_empty() {
        return None;
    }
    let n = features.len() as f32;
    let len_ch = features[0].chroma.len();
    let len_pc = features[0].pc_transitions.len();

    let mut chroma = vec![0.0f32; len_ch];
    let mut pc_transitions = vec![0.0f32; len_pc];

    for f in features {
        for (i, v) in f.chroma.iter().enumerate() { chroma[i] += v; }
        for (i, v) in f.pc_transitions.iter().enumerate() { pc_transitions[i] += v; }
    }

    for v in &mut chroma { *v /= n; }
    for v in &mut pc_transitions { *v /= n; }

    Some(HarmonicFeatures { chroma, pc_transitions })
}

fn l1_normalize(arr: &mut [f32]) {
    let sum: f32 = arr.iter().sum();
    if sum > 0.0 {
        for v in arr.iter_mut() {
            *v /= sum;
        }
    }
}

/// Features for a single time-window chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkFeatures {
    pub offset_secs: f32,
    pub melodic: Option<MelodicFeatures>,
    pub harmonic: Option<HarmonicFeatures>,
}

/// All chunks for a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkedFileFeatures {
    pub chunks: Vec<ChunkFeatures>,
}

/// Extract features in 15-second overlapping windows (7.5s stride).
pub fn extract_chunked_features(
    events: &[NoteEvent],
    ticks_per_beat: u16,
    tempo_map: &[TempoEvent],
) -> ChunkedFileFeatures {
    if events.is_empty() {
        return ChunkedFileFeatures { chunks: vec![] };
    }

    // Pre-compute onset seconds for each event
    let onset_secs: Vec<f64> = events.iter()
        .map(|e| tick_to_seconds(e.start_tick, ticks_per_beat, tempo_map))
        .collect();

    let total_duration = onset_secs.last().copied().unwrap_or(0.0);

    const WINDOW_SECS: f64 = 15.0;
    const STRIDE_SECS: f64 = 7.5;

    let mut chunks = Vec::new();

    if total_duration < WINDOW_SECS {
        // Single chunk for short files
        let skyline = melody::extract_skyline(events, ticks_per_beat);
        let melodic = extract_melodic(&skyline);
        let harmonic = extract_harmonic(events, ticks_per_beat);
        chunks.push(ChunkFeatures {
            offset_secs: 0.0,
            melodic,
            harmonic,
        });
    } else {
        let mut start = 0.0;
        while start < total_duration {
            let end = start + WINDOW_SECS;

            // Use partition_point on pre-computed onset times to find slice bounds
            let lo = onset_secs.partition_point(|&t| t < start);
            let hi = onset_secs.partition_point(|&t| t < end);

            if hi > lo {
                let slice = &events[lo..hi];
                let skyline = melody::extract_skyline(slice, ticks_per_beat);
                let melodic = extract_melodic(&skyline);
                let harmonic = extract_harmonic(slice, ticks_per_beat);
                chunks.push(ChunkFeatures {
                    offset_secs: start as f32,
                    melodic,
                    harmonic,
                });
            }

            start += STRIDE_SECS;
            // Stop if the next window would start beyond the last note
            if start >= total_duration {
                break;
            }
        }
    }

    ChunkedFileFeatures { chunks }
}
