// Recording state store

import { writable, derived } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import type { RecordingState, SessionMetadata } from '$lib/api';
import { getRecordingState, startRecording, stopRecording } from '$lib/api';
import { addNewSession } from './sessions';

// Create the store with initial state
const initialState: RecordingState = {
  status: 'idle',
  started_at: null,
  current_session_path: null,
  elapsed_seconds: 0,
  active_audio_devices: [],
  active_midi_devices: [],
  active_video_devices: []
};

export const recordingState = writable<RecordingState>(initialState);

// Listen for backend recording events
listen('recording-started', (event) => {
  console.log('Recording started from backend:', event.payload);
  refreshRecordingState();
});

listen('recording-stopped', async (event) => {
    console.log('Recording stopped from backend:', event.payload);
    await refreshRecordingState();
    
    // Add the new session to the list without full refresh
    try {
      const metadata = JSON.parse(event.payload as string) as SessionMetadata;
      addNewSession(metadata);
    } catch (e) {
      console.error('Failed to parse session metadata:', e);
    }
  });

// Listen for recording state changes (e.g., when devices are being reinitialized)
listen('recording-state-changed', async (event) => {
  console.log('Recording state changed:', event.payload);
  await refreshRecordingState();
});

// Derived stores for convenience
export const isRecording = derived(recordingState, $state => $state.status === 'recording');
export const isStopping = derived(recordingState, $state => $state.status === 'stopping');
export const isIdle = derived(recordingState, $state => $state.status === 'idle');
export const isInitializing = derived(recordingState, $state => $state.status === 'initializing');

// Can we start recording right now?
export const canRecord = derived(recordingState, $state => $state.status === 'idle');

// Timer for updating elapsed time
let elapsedTimer: ReturnType<typeof setInterval> | null = null;

// Calculate elapsed seconds from started_at timestamp
function calculateElapsed(startedAt: string | null): number {
  if (!startedAt) return 0;
  const start = new Date(startedAt).getTime();
  const now = Date.now();
  return Math.floor((now - start) / 1000);
}

// Actions
export async function refreshRecordingState() {
  try {
    const state = await getRecordingState();
    // Calculate elapsed time from started_at
    state.elapsed_seconds = calculateElapsed(state.started_at);
    recordingState.set(state);
    
    // Start or stop elapsed timer based on state
    if (state.status === 'recording' && !elapsedTimer) {
      elapsedTimer = setInterval(() => {
        recordingState.update(s => ({
          ...s,
          elapsed_seconds: calculateElapsed(s.started_at)
        }));
      }, 1000);
    } else if (state.status !== 'recording' && elapsedTimer) {
      clearInterval(elapsedTimer);
      elapsedTimer = null;
    }
  } catch (error) {
    console.error('Failed to refresh recording state:', error);
  }
}

export async function doStartRecording() {
  try {
    await startRecording();
    await refreshRecordingState();
  } catch (error) {
    console.error('Failed to start recording:', error);
    throw error;
  }
}

export async function doStopRecording() {
  try {
    await stopRecording();
  } catch (error) {
    console.error('Failed to stop recording:', error);
    // Still refresh state even on error to sync UI with backend
  }
  // Always refresh state after stop attempt
  await refreshRecordingState();
  // Note: Session list is updated via the 'recording-stopped' event listener
}

// Initialize on import
refreshRecordingState();
