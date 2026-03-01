// Session list store

import { writable, derived, get } from 'svelte/store';
import type { SessionSummary, SessionMetadata, SessionFilter, RescanProgress } from '$lib/api';
import { getSessions, getSessionDetail, deleteSession as apiDeleteSession, updateSessionNotes as apiUpdateNotes, rescanSessions as apiRescanSessions, renameSession as apiRenameSession } from '$lib/api';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// Store for session list
export const sessions = writable<SessionSummary[]>([]);

// Store for currently selected session
export const selectedSessionId = writable<string | null>(null);

// Store for selected session details
export const selectedSession = writable<SessionMetadata | null>(null);

// Store for current filter
export const sessionFilter = writable<SessionFilter>({
  has_audio: undefined,
  has_midi: undefined,
  has_video: undefined,
  has_notes: undefined,
});

// Loading state
export const isLoading = writable(false);

// Pending seek offset — set before navigating to a session, consumed after MIDI loads
export const pendingSeekOffset = writable<number | null>(null);

// Scan progress (non-null only during first-time scan of new sessions)
export const scanProgress = writable<RescanProgress | null>(null);

// Derived store for grouped sessions by date
export const groupedSessions = derived(sessions, $sessions => {
  const groups: Record<string, SessionSummary[]> = {};
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const weekAgo = new Date(today);
  weekAgo.setDate(weekAgo.getDate() - 7);
  
  for (const session of $sessions) {
    const sessionDate = new Date(session.timestamp);
    sessionDate.setHours(0, 0, 0, 0);
    
    let groupKey: string;
    if (sessionDate.getTime() === today.getTime()) {
      groupKey = 'Today';
    } else if (sessionDate.getTime() === yesterday.getTime()) {
      groupKey = 'Yesterday';
    } else if (sessionDate >= weekAgo) {
      groupKey = 'This Week';
    } else {
      // Group by month
      groupKey = sessionDate.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
    }
    
    if (!groups[groupKey]) {
      groups[groupKey] = [];
    }
    groups[groupKey].push(session);
  }
  
  return groups;
});

// Actions
export async function refreshSessions(autoSelectLatest = false) {
  isLoading.set(true);
  scanProgress.set(null);
  try {
    // Listen for progress events during rescan
    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listen<RescanProgress>('rescan-progress', (event) => {
        scanProgress.set(event.payload);
      });
      await apiRescanSessions();
    } catch (e) {
      console.error('Failed to rescan sessions:', e);
    } finally {
      if (unlisten) unlisten();
      scanProgress.set(null);
    }

    let filter: SessionFilter = {};
    sessionFilter.subscribe(f => filter = f)();

    const sessionList = await getSessions(filter);
    sessions.set(sessionList);

    // Auto-select the latest session if requested and no session is currently selected
    if (autoSelectLatest && sessionList.length > 0) {
      let currentSelection: string | null = null;
      selectedSessionId.subscribe(id => currentSelection = id)();

      if (!currentSelection) {
        // Sessions are sorted by timestamp descending, so first one is the latest
        await selectSession(sessionList[0].id);
      }
    }
  } catch (error) {
    console.error('Failed to fetch sessions:', error);
  } finally {
    isLoading.set(false);
  }
}

/** 
 * Add a newly recorded session to the list without full refresh.
 * Converts SessionMetadata to SessionSummary and prepends to the list.
 */
export function addNewSession(metadata: SessionMetadata) {
  // Convert metadata to summary format
  const summary: SessionSummary = {
    id: metadata.id,
    timestamp: metadata.timestamp,
    duration_secs: metadata.duration_secs,
    has_audio: (metadata.audio_files?.length ?? 0) > 0,
    has_midi: (metadata.midi_files?.length ?? 0) > 0,
    has_video: (metadata.video_files?.length ?? 0) > 0,
    notes: '',
    title: metadata.title ?? null,
  };
  
  // Prepend to list (newest first)
  sessions.update(list => [summary, ...list]);
  
  // Auto-select the new session
  selectSession(metadata.id);
}

let selectSessionSeq = 0;

export async function selectSession(sessionId: string | null) {
  const seq = ++selectSessionSeq;
  selectedSessionId.set(sessionId);

  if (sessionId) {
    try {
      const detail = await getSessionDetail(sessionId);
      if (seq !== selectSessionSeq) return; // stale
      selectedSession.set(detail);
    } catch (error) {
      if (seq !== selectSessionSeq) return;
      console.error('Failed to fetch session detail:', error);
      selectedSession.set(null);
    }
  } else {
    selectedSession.set(null);
  }
}

export async function deleteSessionById(sessionId: string) {
  try {
    await apiDeleteSession(sessionId);
    
    // Remove from local state
    sessions.update(list => list.filter(s => s.id !== sessionId));
    
    // Clear selection if deleted session was selected
    selectedSessionId.update(id => id === sessionId ? null : id);
    selectedSession.update(s => s?.id === sessionId ? null : s);
  } catch (error) {
    console.error('Failed to delete session:', error);
    throw error;
  }
}

export function updateFilter(partial: Partial<SessionFilter>) {
  sessionFilter.update(f => ({ ...f, ...partial }));
  refreshSessions();
}

export async function updateNotes(sessionId: string, notes: string) {
  try {
    await apiUpdateNotes(sessionId, notes);
    
    // Update local state
    sessions.update(list => list.map(s => 
      s.id === sessionId ? { ...s, notes } : s
    ));
    
    selectedSession.update(s => 
      s?.id === sessionId ? { ...s, notes } : s
    );
  } catch (error) {
    console.error('Failed to update notes:', error);
    throw error;
  }
}

export async function renameCurrentSession(oldId: string, newTitle: string) {
  try {
    const newSummary = await apiRenameSession(oldId, newTitle);

    // Replace old entry with new summary in the list
    sessions.update(list => list.map(s =>
      s.id === oldId ? newSummary : s
    ));

    // Update selected session ID to the new ID
    selectedSessionId.set(newSummary.id);

    // Reload detail with the new path
    await selectSession(newSummary.id);

    return newSummary;
  } catch (error) {
    console.error('Failed to rename session:', error);
    throw error;
  }
}

// Initialize - auto-select latest session on app start
refreshSessions(true);
