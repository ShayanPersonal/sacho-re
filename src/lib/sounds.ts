// Sound notification utilities using Tone.js

import * as Tone from "tone";

let synth: Tone.PolySynth | null = null;

function ensureSynth(): Tone.PolySynth {
  if (!synth) {
    synth = new Tone.PolySynth(Tone.Synth, {
      oscillator: { type: "sine" },
      envelope: {
        attack: 0.01,
        decay: 0.15,
        sustain: 0.05,
        release: 0.2,
      },
    }).toDestination();
  }
  return synth;
}

/** Play a short ascending chime (C5→E5→G5) for recording start */
export function playStartSound(volume: number): void {
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  const now = Tone.now();
  s.triggerAttackRelease("C5", "16n", now);
  s.triggerAttackRelease("E5", "16n", now + 0.1);
  s.triggerAttackRelease("G5", "16n", now + 0.2);
}

/** Play the same chime reversed (G5→E5→C5) for recording stop */
export function playStopSound(volume: number): void {
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  const now = Tone.now();
  s.triggerAttackRelease("G5", "16n", now);
  s.triggerAttackRelease("E5", "16n", now + 0.1);
  s.triggerAttackRelease("C5", "16n", now + 0.2);
}

/** Convert a 0.0-1.0 volume to decibels */
function volumeToDb(volume: number): number {
  if (volume <= 0) return -Infinity;
  // Map 0-1 to roughly -30dB to 0dB
  return -30 * (1 - volume);
}
