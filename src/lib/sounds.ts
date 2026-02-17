// Sound notification utilities using Tone.js
// Uses perfect fifths (harmonically ambiguous — no major/minor implication)
// with a bell-like timbre so it sits outside the musical context.

import * as Tone from "tone";

let synth: Tone.PolySynth | null = null;

function ensureSynth(): Tone.PolySynth {
  if (!synth) {
    synth = new Tone.PolySynth(Tone.Synth, {
      oscillator: { type: "sine" },
      envelope: {
        attack: 0.01,
        decay: 0.35,
        sustain: 0,
        release: 0.2,
      },
    }).toDestination();
  }
  return synth;
}

/** Play a single high note for recording start */
export function playStartSound(volume: number): void {
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  s.triggerAttackRelease("A5", "8n", Tone.now());
}

/** Play a single lower note for recording stop */
export function playStopSound(volume: number): void {
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  s.triggerAttackRelease("D5", "8n", Tone.now());
}

/** Convert a 0.0-1.0 volume to decibels */
function volumeToDb(volume: number): number {
  if (volume <= 0) return -Infinity;
  // Map 0-1 to roughly -30dB to 0dB
  return -30 * (1 - volume);
}
