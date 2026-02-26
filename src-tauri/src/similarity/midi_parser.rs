// MIDI file parser with sustain pedal support

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NoteEvent {
    pub pitch: u8,
    pub velocity: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub channel: u8,
}

#[derive(Debug, Clone)]
pub struct TempoEvent {
    pub tick: u64,
    pub microseconds_per_beat: u32,
}

pub struct MidiParseResult {
    pub events: Vec<NoteEvent>,
    pub ticks_per_beat: u16,
    pub tempo_map: Vec<TempoEvent>,
}

/// Convert a tick position to seconds using the tempo map.
pub fn tick_to_seconds(tick: u64, ticks_per_beat: u16, tempo_map: &[TempoEvent]) -> f64 {
    let tpb = ticks_per_beat as f64;
    let mut seconds = 0.0;
    let mut last_tick = 0u64;
    let mut usec_per_beat = 500_000.0; // default 120 BPM

    for te in tempo_map {
        if te.tick >= tick {
            break;
        }
        let delta_ticks = te.tick - last_tick;
        seconds += (delta_ticks as f64 / tpb) * (usec_per_beat / 1_000_000.0);
        last_tick = te.tick;
        usec_per_beat = te.microseconds_per_beat as f64;
    }

    let delta_ticks = tick - last_tick;
    seconds += (delta_ticks as f64 / tpb) * (usec_per_beat / 1_000_000.0);
    seconds
}

/// Parse a MIDI file into note events with sustain pedal handling.
pub fn parse_midi(path: &Path) -> anyhow::Result<MidiParseResult> {
    let data = std::fs::read(path)?;
    let smf = midly::Smf::parse(&data)?;

    let mut ticks_per_beat: u16 = 480;
    if let midly::Timing::Metrical(tpb) = smf.header.timing {
        ticks_per_beat = tpb.as_int();
    }

    let mut notes: Vec<NoteEvent> = Vec::new();
    let mut tempo_map: Vec<TempoEvent> = Vec::new();

    for track in &smf.tracks {
        let mut current_tick: u64 = 0;
        // Active notes: (pitch, channel) -> (velocity, start_tick)
        let mut active_notes: HashMap<(u8, u8), (u8, u64)> = HashMap::new();
        // Sustain pedal state per channel
        let mut sustain_on: HashMap<u8, bool> = HashMap::new();
        // Notes held by sustain pedal: channel -> { pitch -> (velocity, start_tick) }
        let mut sustained_notes: HashMap<u8, HashMap<u8, (u8, u64)>> = HashMap::new();

        for event in track {
            current_tick += event.delta.as_int() as u64;

            if let midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) = event.kind {
                tempo_map.push(TempoEvent {
                    tick: current_tick,
                    microseconds_per_beat: t.as_int(),
                });
            }

            if let midly::TrackEventKind::Midi { channel, message } = event.kind {
                let ch = channel.as_int();

                match message {
                    midly::MidiMessage::NoteOn { key, vel } => {
                        let pitch = key.as_int();
                        let velocity = vel.as_int();

                        if velocity > 0 {
                            // If this pitch is in the sustained set, finalize it first (re-strike)
                            if let Some(ch_sustained) = sustained_notes.get_mut(&ch) {
                                if let Some((old_vel, old_start)) = ch_sustained.remove(&pitch) {
                                    notes.push(NoteEvent {
                                        pitch,
                                        velocity: old_vel,
                                        start_tick: old_start,
                                        duration_ticks: current_tick.saturating_sub(old_start),
                                        channel: ch,
                                    });
                                }
                            }
                            // Also finalize any active note for this key
                            if let Some((old_vel, old_start)) = active_notes.remove(&(pitch, ch)) {
                                notes.push(NoteEvent {
                                    pitch,
                                    velocity: old_vel,
                                    start_tick: old_start,
                                    duration_ticks: current_tick.saturating_sub(old_start),
                                    channel: ch,
                                });
                            }
                            active_notes.insert((pitch, ch), (velocity, current_tick));
                        } else {
                            // Note off via velocity 0
                            finalize_note_off(
                                &mut active_notes, &mut sustained_notes, &sustain_on,
                                &mut notes, pitch, ch, current_tick,
                            );
                        }
                    }
                    midly::MidiMessage::NoteOff { key, .. } => {
                        let pitch = key.as_int();
                        finalize_note_off(
                            &mut active_notes, &mut sustained_notes, &sustain_on,
                            &mut notes, pitch, ch, current_tick,
                        );
                    }
                    midly::MidiMessage::Controller { controller, value } => {
                        // CC64 = Damper/Sustain pedal
                        if controller.as_int() == 64 {
                            let is_on = value.as_int() >= 32;
                            let was_on = sustain_on.get(&ch).copied().unwrap_or(false);
                            sustain_on.insert(ch, is_on);

                            // Pedal released — finalize all sustained notes on this channel
                            if was_on && !is_on {
                                if let Some(ch_sustained) = sustained_notes.remove(&ch) {
                                    for (pitch, (vel, start)) in ch_sustained {
                                        notes.push(NoteEvent {
                                            pitch,
                                            velocity: vel,
                                            start_tick: start,
                                            duration_ticks: current_tick.saturating_sub(start),
                                            channel: ch,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Finalize any remaining active notes at the last tick
        for ((pitch, ch), (vel, start)) in active_notes.drain() {
            notes.push(NoteEvent {
                pitch,
                velocity: vel,
                start_tick: start,
                duration_ticks: current_tick.saturating_sub(start),
                channel: ch,
            });
        }

        // Finalize any remaining sustained notes
        for (_ch, ch_sustained) in sustained_notes.drain() {
            for (pitch, (vel, start)) in ch_sustained {
                notes.push(NoteEvent {
                    pitch,
                    velocity: vel,
                    start_tick: start,
                    duration_ticks: current_tick.saturating_sub(start),
                    channel: 0,
                });
            }
        }
    }

    notes.sort_by_key(|n| n.start_tick);

    // Sort and dedup tempo map
    tempo_map.sort_by_key(|t| t.tick);
    tempo_map.dedup_by_key(|t| t.tick);
    if tempo_map.is_empty() {
        tempo_map.push(TempoEvent { tick: 0, microseconds_per_beat: 500_000 });
    }

    Ok(MidiParseResult { events: notes, ticks_per_beat, tempo_map })
}

/// Handle a note-off event, respecting sustain pedal state.
fn finalize_note_off(
    active_notes: &mut HashMap<(u8, u8), (u8, u64)>,
    sustained_notes: &mut HashMap<u8, HashMap<u8, (u8, u64)>>,
    sustain_on: &HashMap<u8, bool>,
    notes: &mut Vec<NoteEvent>,
    pitch: u8,
    channel: u8,
    current_tick: u64,
) {
    if let Some((vel, start)) = active_notes.remove(&(pitch, channel)) {
        if sustain_on.get(&channel).copied().unwrap_or(false) {
            // Sustain is on — move to sustained set instead of finalizing
            sustained_notes
                .entry(channel)
                .or_default()
                .insert(pitch, (vel, start));
        } else {
            // Sustain is off — finalize immediately
            notes.push(NoteEvent {
                pitch,
                velocity: vel,
                start_tick: start,
                duration_ticks: current_tick.saturating_sub(start),
                channel,
            });
        }
    }
}
