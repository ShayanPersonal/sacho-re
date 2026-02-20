// Device list and selection store

import { writable, derived, get } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import type { AudioDevice, MidiDevice, VideoDevice, VideoDeviceConfig, VideoFpsWarning, AudioTriggerLevel, Config, DisconnectedDeviceInfo } from '$lib/api';
import { refreshAllDevices, getAudioDevices, getMidiDevices, getVideoDevices, getConfig, updateConfig, updateAudioTriggerThresholds, getDisconnectedDevices, restartDevicePipelines } from '$lib/api';
import { settings } from './settings';
import { playDisconnectWarningSound } from '$lib/sounds';

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

// Audio trigger state
export const triggerAudioDevices = writable<Set<string>>(new Set());
export const audioTriggerThresholds = writable<Record<string, number>>({});
export const audioTriggerLevels = writable<Record<string, { current_rms: number; peak_level: number }>>({});

// Per-device video configuration (device_id -> config)
export const videoDeviceConfigs = writable<Record<string, VideoDeviceConfig>>({});

// Active video devices whose effective output codec is FFV1 (large disk usage warning)
export const ffv1WarningDevices = derived(
  [videoDevices, selectedVideoDevices, videoDeviceConfigs],
  ([$devices, $selected, $configs]) => {
    const warnings: string[] = [];
    for (const device of $devices) {
      if (!$selected.has(device.id)) continue;
      const cfg = $configs[device.id];
      if (!cfg) continue;
      const outputCodec = cfg.passthrough ? cfg.source_codec : cfg.encoding_codec;
      if (outputCodec === 'ffv1') warnings.push(device.name);
    }
    return warnings;
  }
);

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

// Listen for audio trigger level events from backend
listen<AudioTriggerLevel[]>('audio-trigger-levels', (event) => {
  audioTriggerLevels.update(levels => {
    const updated = { ...levels };
    for (const entry of event.payload) {
      updated[entry.device_id] = { current_rms: entry.current_rms, peak_level: entry.peak_level };
    }
    return updated;
  });
});

// Clear FPS warnings and audio levels when monitoring restarts (recording-state-changed to initializing)
listen<string>('recording-state-changed', (event) => {
  if (event.payload === 'initializing') {
    videoFpsWarnings.set([]);
    audioTriggerLevels.set({});
  }
});

// Disconnected device IDs (from health checker)
export const disconnectedDevices = writable<Set<string>>(new Set());

// Listen for device health change events from backend
listen<{ disconnected_devices: DisconnectedDeviceInfo[] }>('device-health-changed', (event) => {
  const ids = new Set(event.payload.disconnected_devices.map(d => d.id));
  disconnectedDevices.set(ids);
});

// Repeating warning sound when devices are disconnected
let disconnectWarningInterval: ReturnType<typeof setInterval> | null = null;

disconnectedDevices.subscribe((ids) => {
  if (ids.size > 0) {
    // Start repeating sound if not already running
    if (!disconnectWarningInterval) {
      disconnectWarningInterval = setInterval(() => {
        const cfg = get(settings);
        if (cfg?.sound_device_disconnect) {
          playDisconnectWarningSound(cfg.sound_volume_disconnect, cfg.custom_sound_disconnect);
        }
      }, 3000);
    }
  } else {
    // All devices reconnected — stop the warning
    if (disconnectWarningInterval) {
      clearInterval(disconnectWarningInterval);
      disconnectWarningInterval = null;
    }
  }
});

// Listen for device reconnection — trigger pipeline restart via frontend round-trip
listen<{ device_types: string[] }>('_device-needs-restart', async (event) => {
  try {
    await restartDevicePipelines(event.payload.device_types);
    // Refresh device lists after restart so UI is up to date
    await refreshDevices();
  } catch (e) {
    console.error('Failed to restart device pipelines:', e);
  }
});

// Config reference
export const config = writable<Config | null>(null);

// Derived counts
export const audioDeviceCount = derived(
  [audioDevices, selectedAudioDevices, triggerAudioDevices],
  ([$devices, $selected, $triggers]) => ({
    total: $devices.length,
    selected: $devices.filter(d => $selected.has(d.id)).length,
    triggers: $devices.filter(d => $triggers.has(d.id)).length
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
async function loadDevices() {
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

export async function refreshDevices() {
  try {
    await refreshAllDevices();
  } catch (e) {
    console.error('Failed to re-enumerate devices:', e);
  }
  await loadDevices();
}

export async function loadConfig() {
  try {
    const cfg = await getConfig();
    config.set(cfg);
    
    // Update selections from config
    selectedAudioDevices.set(new Set(cfg.selected_audio_devices));
    selectedMidiDevices.set(new Set(cfg.selected_midi_devices));
    triggerMidiDevices.set(new Set(cfg.trigger_midi_devices));
    triggerAudioDevices.set(new Set(cfg.trigger_audio_devices ?? []));
    audioTriggerThresholds.set(cfg.audio_trigger_thresholds ?? {});
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
  
  // Preserve IDs that are temporarily disconnected (health checker knows about them)
  const currentDisconnected = get(disconnectedDevices);

  // Filter out IDs that don't match any existing device AND aren't disconnected
  const cleanedAudio = currentConfig.selected_audio_devices.filter(id => audioIds.has(id) || currentDisconnected.has(id));
  const cleanedMidi = currentConfig.selected_midi_devices.filter(id => midiIds.has(id) || currentDisconnected.has(id));
  const cleanedTriggers = currentConfig.trigger_midi_devices.filter(id => midiIds.has(id) || currentDisconnected.has(id));
  const cleanedAudioTriggers = (currentConfig.trigger_audio_devices ?? []).filter(id => audioIds.has(id) || currentDisconnected.has(id));
  const cleanedVideo = currentConfig.selected_video_devices.filter(id => videoIds.has(id) || currentDisconnected.has(id));
  const cleanedConfigs: Record<string, VideoDeviceConfig> = {};
  for (const [id, cfg] of Object.entries(currentConfig.video_device_configs)) {
    if (videoIds.has(id) || currentDisconnected.has(id)) {
      cleanedConfigs[id] = cfg;
    }
  }
  
  // Check if anything changed
  const hasChanges = 
    cleanedAudio.length !== currentConfig.selected_audio_devices.length ||
    cleanedMidi.length !== currentConfig.selected_midi_devices.length ||
    cleanedTriggers.length !== currentConfig.trigger_midi_devices.length ||
    cleanedAudioTriggers.length !== (currentConfig.trigger_audio_devices ?? []).length ||
    cleanedVideo.length !== currentConfig.selected_video_devices.length ||
    Object.keys(cleanedConfigs).length !== Object.keys(currentConfig.video_device_configs).length;
  
  if (hasChanges) {
    console.log('[Sacho] Cleaning up stale device IDs from config');
    selectedAudioDevices.set(new Set(cleanedAudio));
    selectedMidiDevices.set(new Set(cleanedMidi));
    triggerMidiDevices.set(new Set(cleanedTriggers));
    triggerAudioDevices.set(new Set(cleanedAudioTriggers));
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
  // This ensures we don't overwrite settings changed elsewhere
  let currentConfig: Config | null = get(settings);
  
  if (!currentConfig) {
    // Fallback to local config if settings not available
    currentConfig = get(config);
  }
  
  if (!currentConfig) return;
  
  const audioSelected = get(selectedAudioDevices);
  const midiSelected = get(selectedMidiDevices);
  const midiTriggers = get(triggerMidiDevices);
  const audioTriggers = get(triggerAudioDevices);
  const audioThresholds = get(audioTriggerThresholds);
  const videoSelected = get(selectedVideoDevices);
  const deviceConfigs = get(videoDeviceConfigs);

  const newConfig: Config = {
    ...(currentConfig as Config),
    selected_audio_devices: Array.from(audioSelected),
    selected_midi_devices: Array.from(midiSelected),
    trigger_midi_devices: Array.from(midiTriggers),
    trigger_audio_devices: Array.from(audioTriggers),
    audio_trigger_thresholds: audioThresholds,
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

export function toggleAudioTrigger(deviceId: string) {
  triggerAudioDevices.update(set => {
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

export function setAudioTriggerThreshold(deviceId: string, threshold: number) {
  audioTriggerThresholds.update(thresholds => ({
    ...thresholds,
    [deviceId]: threshold
  }));
  // Use lightweight threshold update — no pipeline restart needed
  saveThresholds();
}

let thresholdSaveTimeout: ReturnType<typeof setTimeout> | null = null;

/** Save thresholds via the dedicated command (no pipeline restart). Debounced. */
async function saveThresholds() {
  if (thresholdSaveTimeout) clearTimeout(thresholdSaveTimeout);
  thresholdSaveTimeout = setTimeout(async () => {
    try {
      const thresholds = get(audioTriggerThresholds);
      await updateAudioTriggerThresholds(thresholds);
    } catch (error) {
      console.error('Failed to save audio trigger thresholds:', error);
    }
  }, 300);
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
  await loadDevices();
  await loadConfig();
  // Populate disconnected devices before cleanup (so cleanup preserves them)
  try {
    const disconnected = await getDisconnectedDevices();
    disconnectedDevices.set(new Set(disconnected.map(d => d.id)));
  } catch (e) {
    console.error('Failed to load disconnected devices:', e);
  }
  // Clean up stale device IDs after both devices and config are loaded
  await cleanupStaleDeviceIds();
}
initialize();
