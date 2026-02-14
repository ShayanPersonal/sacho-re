// Device list and selection store

import { writable, derived, get } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import type { AudioDevice, MidiDevice, VideoDevice, VideoDeviceConfig, VideoFpsWarning, Config } from '$lib/api';
import { getAudioDevices, getMidiDevices, getVideoDevices, getConfig, updateConfig } from '$lib/api';
import { settings } from './settings';

// Save status for device changes: 'idle' | 'saving' | 'saved' | 'error'
export const deviceSaveStatus = writable<'idle' | 'saving' | 'saved' | 'error'>('idle');
let deviceSaveTimeout: ReturnType<typeof setTimeout> | null = null;

// Device lists
export const audioDevices = writable<AudioDevice[]>([]);
export const midiDevices = writable<MidiDevice[]>([]);
export const videoDevices = writable<VideoDevice[]>([]);

// Selection state (synced with config)
export const selectedAudioDevices = writable<Set<string>>(new Set());
export const selectedMidiDevices = writable<Set<string>>(new Set());
export const triggerMidiDevices = writable<Set<string>>(new Set());
export const selectedVideoDevices = writable<Set<string>>(new Set());

// Per-device video configuration (device_id -> config)
export const videoDeviceConfigs = writable<Record<string, VideoDeviceConfig>>({});

// FPS mismatch warnings from video capture devices
export const videoFpsWarnings = writable<VideoFpsWarning[]>([]);

// Listen for FPS warning events from backend
listen<VideoFpsWarning>('video-fps-warning', (event) => {
  videoFpsWarnings.update(warnings => {
    // Replace any existing warning for the same device
    const filtered = warnings.filter(w => w.device_name !== event.payload.device_name);
    return [...filtered, event.payload];
  });
});

// Clear FPS warnings when monitoring restarts (recording-state-changed to initializing)
listen<string>('recording-state-changed', (event) => {
  if (event.payload === 'initializing') {
    videoFpsWarnings.set([]);
  }
});

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
  const [audioResult, midiResult, videoResult] = await Promise.allSettled([
    getAudioDevices(),
    getMidiDevices(),
    getVideoDevices()
  ]);

  if (audioResult.status === 'fulfilled') {
    audioDevices.set(audioResult.value);
  } else {
    console.error('Failed to refresh audio devices:', audioResult.reason);
  }

  if (midiResult.status === 'fulfilled') {
    midiDevices.set(midiResult.value);
  } else {
    console.error('Failed to refresh MIDI devices:', midiResult.reason);
  }

  if (videoResult.status === 'fulfilled') {
    videoDevices.set(videoResult.value);
  } else {
    console.error('Failed to refresh video devices:', videoResult.reason);
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
    videoDeviceConfigs.set(cfg.video_device_configs ?? {});
  } catch (error) {
    console.error('Failed to load config:', error);
  }
}

/** Clean up stale device IDs from config that no longer match any existing devices */
export async function cleanupStaleDeviceIds() {
  const currentAudio = get(audioDevices);
  const currentMidi = get(midiDevices);
  const currentVideo = get(videoDevices);
  const currentConfig = get(config);
  
  if (!currentConfig) return;
  
  const audioIds = new Set(currentAudio.map(d => d.id));
  const midiIds = new Set(currentMidi.map(d => d.id));
  const videoIds = new Set(currentVideo.map(d => d.id));
  
  // Filter out IDs that don't match any existing device
  const cleanedAudio = currentConfig.selected_audio_devices.filter(id => audioIds.has(id));
  const cleanedMidi = currentConfig.selected_midi_devices.filter(id => midiIds.has(id));
  const cleanedTriggers = currentConfig.trigger_midi_devices.filter(id => midiIds.has(id));
  const cleanedVideo = currentConfig.selected_video_devices.filter(id => videoIds.has(id));
  const cleanedConfigs: Record<string, VideoDeviceConfig> = {};
  for (const [id, cfg] of Object.entries(currentConfig.video_device_configs)) {
    if (videoIds.has(id)) {
      cleanedConfigs[id] = cfg;
    }
  }
  
  // Check if anything changed
  const hasChanges = 
    cleanedAudio.length !== currentConfig.selected_audio_devices.length ||
    cleanedMidi.length !== currentConfig.selected_midi_devices.length ||
    cleanedTriggers.length !== currentConfig.trigger_midi_devices.length ||
    cleanedVideo.length !== currentConfig.selected_video_devices.length ||
    Object.keys(cleanedConfigs).length !== Object.keys(currentConfig.video_device_configs).length;
  
  if (hasChanges) {
    console.log('[Sacho] Cleaning up stale device IDs from config');
    selectedAudioDevices.set(new Set(cleanedAudio));
    selectedMidiDevices.set(new Set(cleanedMidi));
    triggerMidiDevices.set(new Set(cleanedTriggers));
    selectedVideoDevices.set(new Set(cleanedVideo));
    videoDeviceConfigs.set(cleanedConfigs);
    await saveDeviceSelection();
  }
}

export async function saveDeviceSelection() {
  // Clear any pending fade timeout
  if (deviceSaveTimeout) {
    clearTimeout(deviceSaveTimeout);
    deviceSaveTimeout = null;
  }
  
  deviceSaveStatus.set('saving');
  
  // Use the settings store as the source of truth for current config
  // This ensures we don't overwrite settings changed elsewhere (like video_encoding_mode)
  let currentConfig: Config | null = get(settings);
  
  if (!currentConfig) {
    // Fallback to local config if settings not available
    currentConfig = get(config);
  }
  
  if (!currentConfig) return;
  
  const audioSelected = get(selectedAudioDevices);
  const midiSelected = get(selectedMidiDevices);
  const midiTriggers = get(triggerMidiDevices);
  const videoSelected = get(selectedVideoDevices);
  const deviceConfigs = get(videoDeviceConfigs);
  
  const newConfig: Config = {
    ...(currentConfig as Config),
    selected_audio_devices: Array.from(audioSelected),
    selected_midi_devices: Array.from(midiSelected),
    trigger_midi_devices: Array.from(midiTriggers),
    selected_video_devices: Array.from(videoSelected),
    video_device_configs: deviceConfigs
  };
  
  try {
    // updateConfig is synchronous on the backend - it waits for monitor.start()
    // to complete before returning, so when this resolves the backend is ready
    await updateConfig(newConfig);
    config.set(newConfig);
    // Also update the settings store so RecordingIndicator reflects the changes
    settings.set(newConfig);
    
    deviceSaveStatus.set('saved');
    
    // Fade back to idle after 2 seconds
    deviceSaveTimeout = setTimeout(() => {
      deviceSaveStatus.set('idle');
    }, 2000);
  } catch (error) {
    console.error('Failed to save device selection:', error);
    deviceSaveStatus.set('error');
    throw error;
  }
}

/** Auto-save device selection (call after any toggle/change) */
async function autoSaveDevices() {
  try {
    await saveDeviceSelection();
  } catch (error) {
    // Error is already logged in saveDeviceSelection
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
  autoSaveDevices();
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
  autoSaveDevices();
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
  autoSaveDevices();
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
  autoSaveDevices();
}

/** Set the full configuration for a video device */
export function setVideoDeviceConfig(deviceId: string, deviceConfig: VideoDeviceConfig) {
  videoDeviceConfigs.update(configs => ({
    ...configs,
    [deviceId]: deviceConfig
  }));
  autoSaveDevices();
}

// Initialize
async function initialize() {
  await refreshDevices();
  await loadConfig();
  // Clean up stale device IDs after both devices and config are loaded
  await cleanupStaleDeviceIds();
}
initialize();
