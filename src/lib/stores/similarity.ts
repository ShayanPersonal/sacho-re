// Similarity explorer store

import { writable, derived } from 'svelte/store';
import type { MidiImportInfo, SimilarityResult, SimilarityMode } from '$lib/api';
import { importMidiFolder, getMidiImports, getSimilarFiles, clearMidiImports } from '$lib/api';
import { open } from '@tauri-apps/plugin-dialog';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export const importedFiles = writable<MidiImportInfo[]>([]);
export const selectedFileId = writable<string | null>(null);
export const similarFiles = writable<SimilarityResult[]>([]);
export const similarityMode = writable<SimilarityMode>("melodic");
export const isImporting = writable(false);
export const isComputing = writable(false);

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
    const results = await getSimilarFiles(id, mode);
    similarFiles.set(results);
  } catch (error) {
    console.error('Failed to get similar files:', error);
    similarFiles.set([]);
  } finally {
    isComputing.set(false);
  }
}

export async function switchMode(mode: SimilarityMode) {
  similarityMode.set(mode);
  let currentId: string | null = null;
  selectedFileId.subscribe(id => currentId = id)();
  if (currentId) {
    isComputing.set(true);
    try {
      const results = await getSimilarFiles(currentId, mode);
      similarFiles.set(results);
    } catch (error) {
      console.error('Failed to get similar files:', error);
    } finally {
      isComputing.set(false);
    }
  }
}

export async function clearImports() {
  try {
    await clearMidiImports();
    importedFiles.set([]);
    selectedFileId.set(null);
    similarFiles.set([]);
  } catch (error) {
    console.error('Failed to clear imports:', error);
  }
}

// Load previously imported files on module init
async function init() {
  try {
    const files = await getMidiImports();
    importedFiles.set(files);
  } catch {
    // Silently fail on init — no imports yet
  }
}

init();
