// MIDI feature extraction for similarity analysis

use crate::session::MidiFeatures;
use std::path::Path;
use std::collections::HashMap;

/// Extract features from a MIDI file
pub fn extract_features(midi_path: &Path) -> anyhow::Result<MidiFeatures> {
    let data = std::fs::read(midi_path)?;
    let smf = midly::Smf::parse(&data)?;
    
    // Collect all note events
    let mut notes: Vec<NoteEvent> = Vec::new();
    let mut ticks_per_beat = 480u32;
    
    if let midly::Timing::Metrical(tpb) = smf.header.timing {
        ticks_per_beat = tpb.as_int() as u32;
    }
    
    for track in &smf.tracks {
        let mut current_tick: u64 = 0;
        let mut active_notes: HashMap<u8, u64> = HashMap::new();
        
        for event in track {
            current_tick += event.delta.as_int() as u64;
            
            if let midly::TrackEventKind::Midi { message, .. } = event.kind {
                match message {
                    midly::MidiMessage::NoteOn { key, vel } => {
                        if vel.as_int() > 0 {
                            active_notes.insert(key.as_int(), current_tick);
                        } else {
                            // Note off via velocity 0
                            if let Some(start_tick) = active_notes.remove(&key.as_int()) {
                                notes.push(NoteEvent {
                                    pitch: key.as_int(),
                                    velocity: vel.as_int(),
                                    start_tick,
                                    duration_ticks: current_tick - start_tick,
                                });
                            }
                        }
                    }
                    midly::MidiMessage::NoteOff { key, .. } => {
                        if let Some(start_tick) = active_notes.remove(&key.as_int()) {
                            notes.push(NoteEvent {
                                pitch: key.as_int(),
                                velocity: 64, // Default velocity for note-off
                                start_tick,
                                duration_ticks: current_tick - start_tick,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    
    // Sort notes by start time
    notes.sort_by_key(|n| n.start_tick);
    
    // Extract features
    let features = compute_features(&notes, ticks_per_beat);
    
    Ok(features)
}

#[derive(Debug, Clone)]
struct NoteEvent {
    pitch: u8,
    #[allow(dead_code)] // Stored for future velocity-based features
    velocity: u8,
    start_tick: u64,
    duration_ticks: u64,
}

fn compute_features(notes: &[NoteEvent], ticks_per_beat: u32) -> MidiFeatures {
    // Default features for empty MIDI
    if notes.is_empty() {
        return MidiFeatures {
            pitch_class_histogram: [0.0; 12],
            interval_histogram: [0.0; 25],
            contour_ngrams: [0.0; 27],
            notes_per_second: 0.0,
            ioi_mean: 0.0,
            ioi_variance: 0.0,
            tempo_estimate: 120.0,
            pitch_min: 0,
            pitch_max: 0,
            pitch_mean: 0.0,
            pitch_std: 0.0,
            pitch_range: 0,
            tessitura: 0.0,
            duration_histogram: [0.0; 8],
            avg_simultaneous_notes: 0.0,
            chord_frequency: 0.0,
            voice_count_estimate: 0,
            polyphony_ratio: 0.0,
        };
    }
    
    // Pitch class histogram
    let mut pitch_class_histogram = [0.0f32; 12];
    for note in notes {
        pitch_class_histogram[(note.pitch % 12) as usize] += 1.0;
    }
    let pc_sum: f32 = pitch_class_histogram.iter().sum();
    if pc_sum > 0.0 {
        for v in &mut pitch_class_histogram {
            *v /= pc_sum;
        }
    }
    
    // Interval histogram (-12 to +12, index 12 = unison)
    let mut interval_histogram = [0.0f32; 25];
    for i in 1..notes.len() {
        let interval = notes[i].pitch as i32 - notes[i - 1].pitch as i32;
        let clamped = interval.clamp(-12, 12);
        interval_histogram[(clamped + 12) as usize] += 1.0;
    }
    let int_sum: f32 = interval_histogram.iter().sum();
    if int_sum > 0.0 {
        for v in &mut interval_histogram {
            *v /= int_sum;
        }
    }
    
    // Contour n-grams (up=0, same=1, down=2) -> 3^3 = 27 trigrams
    let mut contour_ngrams = [0.0f32; 27];
    if notes.len() >= 4 {
        let contours: Vec<u8> = notes.windows(2)
            .map(|w| {
                if w[1].pitch > w[0].pitch { 0 }
                else if w[1].pitch == w[0].pitch { 1 }
                else { 2 }
            })
            .collect();
        
        for trigram in contours.windows(3) {
            let index = trigram[0] as usize * 9 + trigram[1] as usize * 3 + trigram[2] as usize;
            contour_ngrams[index] += 1.0;
        }
        
        let ngram_sum: f32 = contour_ngrams.iter().sum();
        if ngram_sum > 0.0 {
            for v in &mut contour_ngrams {
                *v /= ngram_sum;
            }
        }
    }
    
    // Rhythm metrics
    let total_ticks = notes.last().map(|n| n.start_tick + n.duration_ticks).unwrap_or(0);
    let total_beats = total_ticks as f32 / ticks_per_beat as f32;
    let total_seconds = total_beats / 2.0; // Assume 120 BPM as default
    let notes_per_second = if total_seconds > 0.0 { notes.len() as f32 / total_seconds } else { 0.0 };
    
    // Inter-onset intervals
    let iois: Vec<f32> = notes.windows(2)
        .map(|w| (w[1].start_tick - w[0].start_tick) as f32)
        .collect();
    let ioi_mean = if !iois.is_empty() { iois.iter().sum::<f32>() / iois.len() as f32 } else { 0.0 };
    let ioi_variance = if !iois.is_empty() {
        iois.iter().map(|&x| (x - ioi_mean).powi(2)).sum::<f32>() / iois.len() as f32
    } else { 0.0 };
    
    // Tempo estimate from median IOI
    let tempo_estimate = if ioi_mean > 0.0 {
        (ticks_per_beat as f32 / ioi_mean) * 60.0
    } else { 120.0 };
    
    // Pitch statistics
    let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    let pitch_min = *pitches.iter().min().unwrap_or(&0);
    let pitch_max = *pitches.iter().max().unwrap_or(&0);
    let pitch_mean = pitches.iter().map(|&p| p as f32).sum::<f32>() / pitches.len() as f32;
    let pitch_std = (pitches.iter()
        .map(|&p| (p as f32 - pitch_mean).powi(2))
        .sum::<f32>() / pitches.len() as f32).sqrt();
    let pitch_range = pitch_max - pitch_min;
    let tessitura = pitch_mean / 127.0;
    
    // Duration histogram (8 buckets: very short to very long)
    let mut duration_histogram = [0.0f32; 8];
    for note in notes {
        let beats = note.duration_ticks as f32 / ticks_per_beat as f32;
        let bucket = match beats {
            b if b < 0.125 => 0,
            b if b < 0.25 => 1,
            b if b < 0.5 => 2,
            b if b < 1.0 => 3,
            b if b < 2.0 => 4,
            b if b < 4.0 => 5,
            b if b < 8.0 => 6,
            _ => 7,
        };
        duration_histogram[bucket] += 1.0;
    }
    let dur_sum: f32 = duration_histogram.iter().sum();
    if dur_sum > 0.0 {
        for v in &mut duration_histogram {
            *v /= dur_sum;
        }
    }
    
    // Polyphony metrics
    let mut time_slots: HashMap<u64, usize> = HashMap::new();
    for note in notes {
        // Quantize to 16th note resolution
        let slot = note.start_tick / (ticks_per_beat as u64 / 4);
        *time_slots.entry(slot).or_insert(0) += 1;
    }
    
    let slot_counts: Vec<usize> = time_slots.values().cloned().collect();
    let avg_simultaneous_notes = if !slot_counts.is_empty() {
        slot_counts.iter().sum::<usize>() as f32 / slot_counts.len() as f32
    } else { 0.0 };
    
    let chords = slot_counts.iter().filter(|&&c| c >= 3).count();
    let chord_frequency = if !slot_counts.is_empty() {
        chords as f32 / slot_counts.len() as f32
    } else { 0.0 };
    
    let voice_count_estimate = slot_counts.iter().max().cloned().unwrap_or(0).min(10) as u8;
    let polyphony_ratio = if !slot_counts.is_empty() {
        slot_counts.iter().filter(|&&c| c > 1).count() as f32 / slot_counts.len() as f32
    } else { 0.0 };
    
    MidiFeatures {
        pitch_class_histogram,
        interval_histogram,
        contour_ngrams,
        notes_per_second,
        ioi_mean,
        ioi_variance,
        tempo_estimate: tempo_estimate.clamp(30.0, 300.0),
        pitch_min,
        pitch_max,
        pitch_mean,
        pitch_std,
        pitch_range,
        tessitura,
        duration_histogram,
        avg_simultaneous_notes,
        chord_frequency,
        voice_count_estimate,
        polyphony_ratio,
    }
}
