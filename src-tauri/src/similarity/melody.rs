// Skyline melody extraction

use super::midi_parser::NoteEvent;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct MelodyNote {
    pub pitch: u8,
    pub onset_beat: f32,
    pub duration_beats: f32,
}

/// Extract the skyline melody: at each quantized onset, keep only the highest pitch.
/// Quantization: 1/32-note buckets (ticks_per_beat / 8).
pub fn extract_skyline(events: &[NoteEvent], ticks_per_beat: u16) -> Vec<MelodyNote> {
    if events.is_empty() {
        return Vec::new();
    }

    let bucket_size = (ticks_per_beat as u64).max(1) / 8;
    let bucket_size = bucket_size.max(1); // avoid division by zero

    // Group by quantized onset bucket, keeping highest pitch per bucket
    let mut buckets: BTreeMap<u64, &NoteEvent> = BTreeMap::new();

    for event in events {
        let bucket = event.start_tick / bucket_size;
        let entry = buckets.entry(bucket).or_insert(event);
        if event.pitch > entry.pitch {
            *entry = event;
        }
    }

    let tpb = ticks_per_beat as f32;
    buckets.values().map(|e| {
        MelodyNote {
            pitch: e.pitch,
            onset_beat: e.start_tick as f32 / tpb,
            duration_beats: e.duration_ticks as f32 / tpb,
        }
    }).collect()
}
