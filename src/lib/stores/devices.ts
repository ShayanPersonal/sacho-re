// Device list and selection store

import { writable, derived } from 'svelte/store';
import type { AudioDevice, MidiDevice, VideoDevice, Config } from '$lib/api';
import { getAudioDevices, getMidiDevices, getVideoDevices, getConfig, updateConfig } from '$lib/api';
import { settings } from './settings';

import type { VideoCodec } from '$lib/api';

// Device lists
export const audioDevices = writable<AudioDevice[]>([]);
export const midiDevices = writable<MidiDevice[]>([]);
export const videoDevices = writable<VideoDevice[]>([]);

// Selection state (synced with config)
export const selectedAudioDevices = writable<Set<string>>(new Set());
export const selectedMidiDevices = writable<Set<string>>(new Set());
export const triggerMidiDevices = writable<Set<string>>(new Set());
export const selectedVideoDevices = writable<Set<string>>(new Set());

// Video device codec selection (device_id -> codec)
export const videoDeviceCodecs = writable<Record<string, VideoCodec>>({});

// Config reference
export const config = writable<Config | null>(null);

// Derived counts
export const audioDeviceCount = derived(
  [audioDevices, selectedAudioDevices],
  ([$devices, $selected]) => ({
    total: $devices.length,
    selected: $devices.filter(d => $selected.has(d.id)).length
  })
);

export const midiDeviceCount = derived(
  [midiDevices, selectedMidiDevices, triggerMidiDevices],
  ([$devices, $selected, $triggers]) => ({
    total: $devices.length,
    selected: $devices.filter(d => $selected.has(d.id)).length,
    triggers: $devices.filter(d => $triggers.has(d.id)).length
  })
);

export const videoDeviceCount = derived(
  [videoDevices, selectedVideoDevices],
  ([$devices, $selected]) => ({
    total: $devices.length,
    selected: $devices.filter(d => $selected.has(d.id)).length
  })
);

// Actions
export async function refreshDevices() {
  try {
    const [audio, midi, video] = await Promise.all([
      getAudioDevices(),
      getMidiDevices(),
      getVideoDevices()
    ]);
    
    audioDevices.set(audio);
    midiDevices.set(midi);
    videoDevices.set(video);
  } catch (error) {
    console.error('Failed to refresh devices:', error);
  }
}

export async function loadConfig() {
  try {
    const cfg = await getConfig();
    config.set(cfg);
    
    // Update selections from config
    selectedAudioDevices.set(new Set(cfg.selected_audio_devices));
    selectedMidiDevices.set(new Set(cfg.selected_midi_devices));
    triggerMidiDevices.set(new Set(cfg.trigger_midi_devices));
    selectedVideoDevices.set(new Set(cfg.selected_video_devices));
    videoDeviceCodecs.set(cfg.video_device_codecs ?? {});
  } catch (error) {
    console.error('Failed to load config:', error);
  }
}

/** Clean up stale device IDs from config that no longer match any existing devices */
export async function cleanupStaleDeviceIds() {
  let currentAudio: AudioDevice[] = [];
  let currentMidi: MidiDevice[] = [];
  let currentVideo: VideoDevice[] = [];
  let currentConfig: Config | null = null;
  
  const unsub1 = audioDevices.subscribe(d => currentAudio = d);
  const unsub2 = midiDevices.subscribe(d => currentMidi = d);
  const unsub3 = videoDevices.subscribe(d => currentVideo = d);
  const unsub4 = config.subscribe(c => currentConfig = c);
  unsub1(); unsub2(); unsub3(); unsub4();
  
  if (!currentConfig) return;
  
  const audioIds = new Set(currentAudio.map(d => d.id));
  const midiIds = new Set(currentMidi.map(d => d.id));
  const videoIds = new Set(currentVideo.map(d => d.id));
  
  // Filter out IDs that don't match any existing device
  const cleanedAudio = currentConfig.selected_audio_devices.filter(id => audioIds.has(id));
  const cleanedMidi = currentConfig.selected_midi_devices.filter(id => midiIds.has(id));
  const cleanedTriggers = currentConfig.trigger_midi_devices.filter(id => midiIds.has(id));
  const cleanedVideo = currentConfig.selected_video_devices.filter(id => videoIds.has(id));
  const cleanedCodecs: Record<string, VideoCodec> = {};
  for (const [id, codec] of Object.entries(currentConfig.video_device_codecs)) {
    if (videoIds.has(id)) {
      cleanedCodecs[id] = codec;
    }
  }
  
  // Check if anything changed
  const hasChanges = 
    cleanedAudio.length !== currentConfig.selected_audio_devices.length ||
    cleanedMidi.length !== currentConfig.selected_midi_devices.length ||
    cleanedTriggers.length !== currentConfig.trigger_midi_devices.length ||
    cleanedVideo.length !== currentConfig.selected_video_devices.length ||
    Object.keys(cleanedCodecs).length !== Object.keys(currentConfig.video_device_codecs).length;
  
  if (hasChanges) {
    console.log('[Sacho] Cleaning up stale device IDs from config');
    selectedAudioDevices.set(new Set(cleanedAudio));
    selectedMidiDevices.set(new Set(cleanedMidi));
    triggerMidiDevices.set(new Set(cleanedTriggers));
    selectedVideoDevices.set(new Set(cleanedVideo));
    videoDeviceCodecs.set(cleanedCodecs);
    await saveDeviceSelection();
  }
}

export async function saveDeviceSelection() {
  // Use the settings store as the source of truth for current config
  // This ensures we don't overwrite settings changed elsewhere (like video_encoding_mode)
  let currentConfig: Config | null = null;
  const unsubConfig = settings.subscribe(c => currentConfig = c);
  unsubConfig();
  
  if (!currentConfig) {
    // Fallback to local config if settings not available
    const unsubLocal = config.subscribe(c => currentConfig = c);
    unsubLocal();
  }
  
  if (!currentConfig) return;
  
  let audioSelected: Set<string> = new Set();
  let midiSelected: Set<string> = new Set();
  let midiTriggers: Set<string> = new Set();
  let videoSelected: Set<string> = new Set();
  let codecSelections: Record<string, VideoCodec> = {};
  
  const unsub1 = selectedAudioDevices.subscribe(s => audioSelected = s);
  const unsub2 = selectedMidiDevices.subscribe(s => midiSelected = s);
  const unsub3 = triggerMidiDevices.subscribe(s => midiTriggers = s);
  const unsub4 = selectedVideoDevices.subscribe(s => videoSelected = s);
  const unsub5 = videoDeviceCodecs.subscribe(c => codecSelections = c);
  unsub1(); unsub2(); unsub3(); unsub4(); unsub5();
  
  const newConfig: Config = {
    ...(currentConfig as Config),
    selected_audio_devices: Array.from(audioSelected),
    selected_midi_devices: Array.from(midiSelected),
    trigger_midi_devices: Array.from(midiTriggers),
    selected_video_devices: Array.from(videoSelected),
    video_device_codecs: codecSelections
  };
  
  try {
    await updateConfig(newConfig);
    config.set(newConfig);
    // Also update the settings store so RecordingIndicator reflects the changes
    settings.set(newConfig);
  } catch (error) {
    console.error('Failed to save device selection:', error);
    throw error;
  }
}

export function toggleAudioDevice(deviceId: string) {
  selectedAudioDevices.update(set => {
    const newSet = new Set(set);
    if (newSet.has(deviceId)) {
      newSet.delete(deviceId);
    } else {
      newSet.add(deviceId);
    }
    return newSet;
  });
}

export function toggleMidiDevice(deviceId: string) {
  selectedMidiDevices.update(set => {
    const newSet = new Set(set);
    if (newSet.has(deviceId)) {
      newSet.delete(deviceId);
    } else {
      newSet.add(deviceId);
    }
    return newSet;
  });
}

export function toggleMidiTrigger(deviceId: string) {
  triggerMidiDevices.update(set => {
    const newSet = new Set(set);
    if (newSet.has(deviceId)) {
      newSet.delete(deviceId);
    } else {
      newSet.add(deviceId);
    }
    return newSet;
  });
}

export function toggleVideoDevice(deviceId: string) {
  selectedVideoDevices.update(set => {
    const newSet = new Set(set);
    if (newSet.has(deviceId)) {
      newSet.delete(deviceId);
    } else {
      newSet.add(deviceId);
    }
    return newSet;
  });
}

/** Set the codec to use for a video device */
export function setVideoDeviceCodec(deviceId: string, codec: VideoCodec) {
  videoDeviceCodecs.update(codecs => ({
    ...codecs,
    [deviceId]: codec
  }));
}

/** Get the selected codec for a video device, or undefined if using default */
export function getVideoDeviceCodec(deviceId: string): VideoCodec | undefined {
  let codecs: Record<string, VideoCodec> = {};
  const unsub = videoDeviceCodecs.subscribe(c => codecs = c);
  unsub();
  return codecs[deviceId];
}

// Initialize
async function initialize() {
  await refreshDevices();
  await loadConfig();
  // Clean up stale device IDs after both devices and config are loaded
  await cleanupStaleDeviceIds();
}
initialize();
