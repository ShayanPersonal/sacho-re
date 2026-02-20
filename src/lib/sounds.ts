// Sound notification utilities using Tone.js

import * as Tone from "tone";
import { convertFileSrc } from "@tauri-apps/api/core";
import { appConfigDir, join } from "@tauri-apps/api/path";

let synth: Tone.PolySynth | null = null;
let cachedConfigDir: string | null = null;

// Playback tracking for preview toggle
let currentAudio: HTMLAudioElement | null = null;
let playingType: "start" | "stop" | "disconnect" | null = null;

async function getConfigDir(): Promise<string> {
  if (!cachedConfigDir) {
    cachedConfigDir = await appConfigDir();
  }
  return cachedConfigDir;
}

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

/** Stop any currently playing preview sound */
export function stopPlayback(): void {
  if (currentAudio) {
    currentAudio.pause();
    currentAudio.currentTime = 0;
    currentAudio = null;
  }
  if (synth) {
    synth.releaseAll();
  }
  playingType = null;
}

/** Play a custom audio file via HTMLAudioElement. Returns true if playback started. */
async function playCustomFile(
  relativePath: string,
  volume: number,
): Promise<boolean> {
  try {
    const configDir = await getConfigDir();
    const fullPath = await join(configDir, relativePath);
    const url = convertFileSrc(fullPath);
    const audio = new Audio(url);
    audio.volume = Math.max(0, Math.min(1, volume));
    currentAudio = audio;
    audio.addEventListener("ended", () => {
      if (currentAudio === audio) {
        currentAudio = null;
        playingType = null;
      }
    });
    await audio.play();
    return true;
  } catch (e) {
    console.warn("Failed to play custom sound, falling back to default:", e);
    currentAudio = null;
    return false;
  }
}

/** Play a double C5 note for recording start.
 *  When called as a preview (from Settings), toggles playback on repeat press. */
export async function playStartSound(
  volume: number,
  customPath?: string | null,
): Promise<void> {
  // Toggle: if already playing start sound, stop it
  if (playingType === "start") {
    stopPlayback();
    return;
  }
  stopPlayback();
  playingType = "start";

  if (customPath) {
    const played = await playCustomFile(customPath, volume);
    if (played) return;
  }
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  const now = Tone.now();
  s.triggerAttackRelease("G5", "16n", now);
  s.triggerAttackRelease("G5", "16n", now + 0.125);
  setTimeout(() => {
    if (playingType === "start") playingType = null;
  }, 400);
}

/** Play a single C5 note for recording stop.
 *  When called as a preview (from Settings), toggles playback on repeat press. */
export async function playStopSound(
  volume: number,
  customPath?: string | null,
): Promise<void> {
  // Toggle: if already playing stop sound, stop it
  if (playingType === "stop") {
    stopPlayback();
    return;
  }
  stopPlayback();
  playingType = "stop";

  if (customPath) {
    const played = await playCustomFile(customPath, volume);
    if (played) return;
  }
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  const now = Tone.now();
  s.triggerAttackRelease("G5", "16n", now);
  setTimeout(() => {
    if (playingType === "stop") playingType = null;
  }, 300);
}

/** Play three long D4 notes as a disconnect warning.
 *  Lower and more attention-getting than the G5 used for start/stop. */
export async function playDisconnectWarningSound(
  volume: number,
  customPath?: string | null,
): Promise<void> {
  // Toggle: if already playing disconnect sound, stop it
  if (playingType === "disconnect") {
    stopPlayback();
    return;
  }
  stopPlayback();
  playingType = "disconnect";

  if (customPath) {
    const played = await playCustomFile(customPath, volume);
    if (played) return;
  }
  const s = ensureSynth();
  s.volume.value = volumeToDb(volume);
  const now = Tone.now();
  s.triggerAttackRelease("D4", 0.3, now);
  s.triggerAttackRelease("D4", 0.3, now + 0.5);
  s.triggerAttackRelease("D4", 0.3, now + 1.0);
  setTimeout(() => {
    if (playingType === "disconnect") playingType = null;
  }, 1500);
}

/** Preview a custom sound file by its relative path in the config dir */
export async function previewCustomSound(
  relativePath: string,
  volume: number,
): Promise<void> {
  stopPlayback();
  await playCustomFile(relativePath, volume);
}

/** Convert a 0.0-1.0 volume to decibels */
function volumeToDb(volume: number): number {
  if (volume <= 0) return -Infinity;
  // Map 0-1 to roughly -30dB to 0dB
  return -30 * (1 - volume);
}
