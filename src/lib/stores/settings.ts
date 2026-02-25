// Settings store

import { writable, get } from 'svelte/store';
import type { Config } from '$lib/api';
import { getConfig, updateConfig } from '$lib/api';
import { recordingState, refreshRecordingState } from './recording';

export const settings = writable<Config | null>(null);
export const isSaving = writable(false);
// Save status: 'idle' | 'saving' | 'saved' | 'error'
export const saveStatus = writable<'idle' | 'saving' | 'saved' | 'error'>('idle');

let saveStatusTimeout: ReturnType<typeof setTimeout> | null = null;
let debounceTimeout: ReturnType<typeof setTimeout> | null = null;

export async function loadSettings() {
  try {
    const config = await getConfig();
    settings.set(config);
  } catch (error) {
    console.error('Failed to load settings:', error);
  }
}

export async function saveSettings(newSettings: Config) {
  // Clear any pending fade timeout
  if (saveStatusTimeout) {
    clearTimeout(saveStatusTimeout);
    saveStatusTimeout = null;
  }

  isSaving.set(true);
  saveStatus.set('saving');

  // Check if the change affects recording pipelines (requires backend restart)
  const current = get(settings);
  const pipelineAffected = current != null && (
    current.pre_roll_secs !== newSettings.pre_roll_secs ||
    current.encode_during_preroll !== newSettings.encode_during_preroll
  );

  if (pipelineAffected) {
    // Optimistically signal initializing so RecordingIndicator disables during pipeline restart.
    // The backend sets this too, but its synchronous Initializing→Idle cycle completes within
    // the single invoke call, so the frontend event loop never observes the intermediate state.
    recordingState.update(s => ({ ...s, status: 'initializing' }));
  }

  try {
    await updateConfig(newSettings);
    settings.set({ ...newSettings });
    saveStatus.set('saved');

    // Fade back to idle after 2 seconds
    saveStatusTimeout = setTimeout(() => {
      saveStatus.set('idle');
    }, 2000);
  } catch (error) {
    console.error('Failed to save settings:', error);
    saveStatus.set('error');
    throw error;
  } finally {
    isSaving.set(false);
    if (pipelineAffected) {
      // Sync recording state with backend (restores idle, or recording if update was rejected)
      await refreshRecordingState();
    }
  }
}

// Debounced save for text inputs
export function saveSettingsDebounced(newSettings: Config, delay: number = 500) {
  if (debounceTimeout) {
    clearTimeout(debounceTimeout);
  }
  
  // Show saving indicator immediately for feedback
  saveStatus.set('saving');
  
  debounceTimeout = setTimeout(() => {
    saveSettings(newSettings);
  }, delay);
}

export async function updateSetting<K extends keyof Config>(key: K, value: Config[K]) {
  const current = get(settings);
  
  if (!current) return;
  
  const newSettings: Config = { ...current, [key]: value };
  await saveSettings(newSettings);
}

// Initialize
loadSettings();
