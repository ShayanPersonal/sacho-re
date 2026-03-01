// Similarity explorer store

import { writable, derived, get } from 'svelte/store';
import type { MidiImportInfo, SimilarityResult, SimilarityMode, SimilaritySourceMode, RecordingSimFile, SessionSimilarityResult } from '$lib/api';
import { importMidiFolder, getMidiImports, getSimilarFiles, clearMidiImports, readSessionFile, getRecordingSimilarityFiles, getSimilarSessions } from '$lib/api';
import { open } from '@tauri-apps/plugin-dialog';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { Midi } from '@tonejs/midi';

// --- Source Mode ---
export const sourceMode = writable<SimilaritySourceMode>("recordings");

// --- Import Mode Stores ---
export const importedFiles = writable<MidiImportInfo[]>([]);
export const selectedFileId = writable<string | null>(null);
export const similarFiles = writable<SimilarityResult[]>([]);
export const similarityMode = writable<SimilarityMode>("melodic");
export const isImporting = writable(false);
export const isComputing = writable(false);
export const resultCount = writable(20);

export interface ImportProgress {
  current: number;
  total: number;
  file_name: string;
}

export const importProgress = writable<ImportProgress | null>(null);

export const selectedFile = derived(
  [importedFiles, selectedFileId],
  ([$files, $id]) => $id ? $files.find(f => f.id === $id) ?? null : null
);

// --- Recording Mode Stores ---
export const recordingFiles = writable<RecordingSimFile[]>([]);
export const selectedRecordingId = writable<string | null>(null);
export const similarSessions = writable<SessionSimilarityResult[]>([]);

export const selectedRecording = derived(
  [recordingFiles, selectedRecordingId],
  ([$files, $id]) => $id ? $files.find(f => f.session_id === $id) ?? null : null
);

// --- Import Mode Functions ---

export async function importFolder() {
  const selected = await open({ directory: true, title: "Select MIDI Folder" });
  if (!selected) return;

  isImporting.set(true);
  let unlisten: UnlistenFn | undefined;
  try {
    unlisten = await listen<ImportProgress>('midi-import-progress', (e) => {
      importProgress.set(e.payload);
    });
    const files = await importMidiFolder(selected as string);
    importedFiles.set(files);
    selectedFileId.set(null);
    similarFiles.set([]);
  } catch (error) {
    console.error('Failed to import MIDI folder:', error);
  } finally {
    unlisten?.();
    importProgress.set(null);
    isImporting.set(false);
  }
}

export async function selectFile(id: string) {
  selectedFileId.set(id);
  isComputing.set(true);
  try {
    let mode: SimilarityMode = "melodic";
    similarityMode.subscribe(m => mode = m)();
    const n = get(resultCount);
    const results = await getSimilarFiles(id, mode, n);
    similarFiles.set(results);
    // Fetch durations for visible nodes (center + satellites)
    const selected = get(importedFiles).find(f => f.id === id);
    const allVisible = [...results.map(r => r.file), ...(selected ? [selected] : [])];
    fetchDurations(allVisible);
  } catch (error) {
    console.error('Failed to get similar files:', error);
    similarFiles.set([]);
  } finally {
    isComputing.set(false);
  }
}

export async function switchMode(mode: SimilarityMode) {
  similarityMode.set(mode);

  const currentSourceMode = get(sourceMode);

  const n = get(resultCount);

  if (currentSourceMode === "recordings") {
    const currentId = get(selectedRecordingId);
    if (currentId) {
      isComputing.set(true);
      try {
        const results = await getSimilarSessions(currentId, mode, n);
        similarSessions.set(results);
      } catch (error) {
        console.error('Failed to get similar sessions:', error);
      } finally {
        isComputing.set(false);
      }
    }
  } else {
    const currentId = get(selectedFileId);
    if (currentId) {
      isComputing.set(true);
      try {
        const results = await getSimilarFiles(currentId, mode, n);
        similarFiles.set(results);
        const selected = get(importedFiles).find(f => f.id === currentId);
        const allVisible = [...results.map(r => r.file), ...(selected ? [selected] : [])];
        fetchDurations(allVisible);
      } catch (error) {
        console.error('Failed to get similar files:', error);
      } finally {
        isComputing.set(false);
      }
    }
  }
}

export async function clearImports() {
  try {
    await clearMidiImports();
    importedFiles.set([]);
    selectedFileId.set(null);
    similarFiles.set([]);
    fileDurations.set(new Map());
  } catch (error) {
    console.error('Failed to clear imports:', error);
  }
}

// Cache of file ID -> duration in seconds, loaded lazily from MIDI files
export const fileDurations = writable<Map<string, number>>(new Map());

function splitPath(filePath: string): { dir: string; name: string } {
  const sep = filePath.includes('\\') ? '\\' : '/';
  const lastSep = filePath.lastIndexOf(sep);
  if (lastSep === -1) return { dir: '.', name: filePath };
  return { dir: filePath.substring(0, lastSep), name: filePath.substring(lastSep + 1) };
}

async function loadDuration(file: MidiImportInfo): Promise<number | null> {
  try {
    const { dir, name } = splitPath(file.file_path);
    const bytes = await readSessionFile(dir, name);
    const midi = new Midi(bytes);
    return midi.duration || 0;
  } catch {
    return null;
  }
}

export async function fetchDurations(files: MidiImportInfo[]) {
  const current = get(fileDurations);
  const missing = files.filter(f => !current.has(f.id));
  if (missing.length === 0) return;

  const results = await Promise.all(missing.map(f => loadDuration(f).then(d => [f.id, d] as const)));
  fileDurations.update(map => {
    const next = new Map(map);
    for (const [id, dur] of results) {
      if (dur !== null) next.set(id, dur);
    }
    return next;
  });
}

// --- Recording Mode Functions ---

export async function loadRecordingFiles() {
  try {
    const files = await getRecordingSimilarityFiles();
    recordingFiles.set(files);
  } catch (error) {
    console.error('Failed to load recording files:', error);
  }
}

export async function selectRecording(sessionId: string) {
  selectedRecordingId.set(sessionId);
  isComputing.set(true);
  try {
    let mode: SimilarityMode = "melodic";
    similarityMode.subscribe(m => mode = m)();
    const n = get(resultCount);
    const results = await getSimilarSessions(sessionId, mode, n);
    similarSessions.set(results);
  } catch (error) {
    console.error('Failed to get similar sessions:', error);
    similarSessions.set([]);
  } finally {
    isComputing.set(false);
  }
}

// --- Event Listeners ---

let featuresUnlisten: UnlistenFn | undefined;
let syncUnlisten: UnlistenFn | undefined;

async function setupEventListeners() {
  featuresUnlisten = await listen('session-features-computed', () => {
    loadRecordingFiles();
  });
  syncUnlisten = await listen('recording-features-synced', () => {
    loadRecordingFiles();
  });
}

setupEventListeners();

// --- Init ---

async function init() {
  try {
    const files = await getMidiImports();
    importedFiles.set(files);
  } catch {
    // Silently fail on init — no imports yet
  }

  // Also load recording files
  loadRecordingFiles();
}

init();
