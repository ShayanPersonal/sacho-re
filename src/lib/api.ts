// Tauri command bindings

import { invoke } from '@tauri-apps/api/core';
import { enable as enableAutostart, disable as disableAutostart } from '@tauri-apps/plugin-autostart';

// ============================================================================
// Types
// ============================================================================

export interface AudioDevice {
  id: string;
  name: string;
  channels: number;
  sample_rate: number;
  is_default: boolean;
}

export interface MidiDevice {
  id: string;
  name: string;
  port_index: number;
}

/** Supported video codecs */
export type VideoCodec = 'mjpeg' | 'av1' | 'vp8' | 'vp9' | 'raw';

export interface VideoDevice {
  id: string;
  name: string;
  resolutions: Resolution[];
  /** Supported video codecs for this device (can be recorded) */
  supported_codecs: VideoCodec[];
  /** All formats detected from the device (for display) */
  all_formats: string[];
}

/** Check if a video device supports any recording codec */
export function isVideoDeviceSupported(device: VideoDevice): boolean {
  return device.supported_codecs.length > 0;
}

/** Get human-readable codec name */
export function getCodecDisplayName(codec: VideoCodec): string {
  switch (codec) {
    case 'mjpeg': return 'MJPEG';
    case 'vp8': return 'VP8';
    case 'vp9': return 'VP9';
    case 'av1': return 'AV1';
    case 'raw': return 'RAW';
  }
}

export interface Resolution {
  width: number;
  height: number;
  fps: number;
}

export interface RecordingState {
  status: 'idle' | 'recording' | 'stopping' | 'initializing';
  started_at: string | null;
  current_session_path: string | null;
  elapsed_seconds: number;
  active_audio_devices: string[];
  active_midi_devices: string[];
  active_video_devices: string[];
}

export interface SessionSummary {
  id: string;
  timestamp: string;
  duration_secs: number;
  has_audio: boolean;
  has_midi: boolean;
  has_video: boolean;
  audio_count: number;
  midi_count: number;
  video_count: number;
  total_size_bytes: number;
  is_favorite: boolean;
  tags: string[];
  notes: string;
  similarity_coords: { x: number; y: number } | null;
  cluster_id: number | null;
}

export interface SessionMetadata {
  id: string;
  timestamp: string;
  duration_secs: number;
  path: string;
  audio_files: AudioFileInfo[];
  midi_files: MidiFileInfo[];
  video_files: VideoFileInfo[];
  tags: string[];
  notes: string;
  is_favorite: boolean;
  similarity_coords: { x: number; y: number } | null;
  cluster_id: number | null;
}

export interface AudioFileInfo {
  filename: string;
  device_name: string;
  channels: number;
  sample_rate: number;
  duration_secs: number;
  size_bytes: number;
}

export interface MidiFileInfo {
  filename: string;
  device_name: string;
  event_count: number;
  size_bytes: number;
}

export interface VideoFileInfo {
  filename: string;
  device_name: string;
  width: number;
  height: number;
  fps: number;
  duration_secs: number;
  size_bytes: number;
}

export type VideoEncodingMode = 'av1_hardware' | 'vp9' | 'vp8' | 'raw';

export interface Config {
  storage_path: string;
  idle_timeout_secs: number;
  pre_roll_secs: number;
  audio_format: 'wav' | 'flac';
  video_encoding_mode: VideoEncodingMode;
  dark_mode: boolean;
  auto_start: boolean;
  notify_recording_start: boolean;
  notify_recording_stop: boolean;
  selected_audio_devices: string[];
  selected_midi_devices: string[];
  trigger_midi_devices: string[];
  selected_video_devices: string[];
  /** Selected codec per video device (device_id -> codec) */
  video_device_codecs: Record<string, VideoCodec>;
  device_presets: DevicePreset[];
  current_preset: string | null;
}

export interface DevicePreset {
  name: string;
  audio_devices: string[];
  midi_devices: string[];
  trigger_midi_devices: string[];
  video_devices: string[];
}

export interface SessionFilter {
  search?: string;
  favorites_only?: boolean;
  has_audio?: boolean;
  has_midi?: boolean;
  has_video?: boolean;
  has_notes?: boolean;
  limit?: number;
  offset?: number;
}

export interface SimilarityPoint {
  id: string;
  x: number;
  y: number;
  cluster_id: number | null;
  timestamp: string;
}

export interface SimilarityData {
  points: SimilarityPoint[];
  clusters: ClusterInfo[];
}

export interface ClusterInfo {
  id: number;
  name: string;
  count: number;
}

// ============================================================================
// Device Commands
// ============================================================================

export async function getAudioDevices(): Promise<AudioDevice[]> {
  return invoke('get_audio_devices');
}

export async function getMidiDevices(): Promise<MidiDevice[]> {
  return invoke('get_midi_devices');
}

export async function getVideoDevices(): Promise<VideoDevice[]> {
  return invoke('get_video_devices');
}

// ============================================================================
// Encoder Availability
// ============================================================================

export interface EncoderAvailability {
  /** Whether AV1 encoding is available (hardware or software) */
  av1_available: boolean;
  /** Whether AV1 hardware encoding is available */
  av1_hardware: boolean;
  /** Whether VP9 encoding is available (hardware or software) */
  vp9_available: boolean;
  /** Whether VP9 hardware encoding is available */
  vp9_hardware: boolean;
  /** Whether VP8 encoding is available (hardware or software) */
  vp8_available: boolean;
  /** Whether VP8 hardware encoding is available */
  vp8_hardware: boolean;
  /** Name of the AV1 encoder if available */
  av1_encoder_name: string | null;
  /** Name of the VP9 encoder if available */
  vp9_encoder_name: string | null;
  /** Name of the VP8 encoder if available */
  vp8_encoder_name: string | null;
  /** Recommended default encoding mode */
  recommended_default: VideoEncodingMode;
}

export async function getEncoderAvailability(): Promise<EncoderAvailability> {
  return invoke('get_encoder_availability');
}

// ============================================================================
// Recording Commands
// ============================================================================

export async function getRecordingState(): Promise<RecordingState> {
  return invoke('get_recording_state');
}

export async function startRecording(): Promise<string> {
  return invoke('start_recording');
}

export async function stopRecording(): Promise<void> {
  return invoke('stop_recording');
}

// ============================================================================
// Session Commands
// ============================================================================

export async function getSessions(filter: SessionFilter = {}): Promise<SessionSummary[]> {
  return invoke('get_sessions', { filter });
}

export async function getSessionDetail(sessionId: string): Promise<SessionMetadata | null> {
  return invoke('get_session_detail', { sessionId });
}

export async function deleteSession(sessionId: string): Promise<void> {
  return invoke('delete_session', { sessionId });
}

export async function readSessionFile(sessionPath: string, filename: string): Promise<Uint8Array> {
  const data = await invoke<number[]>('read_session_file', { sessionPath, filename });
  return new Uint8Array(data);
}

export async function updateSessionFavorite(sessionId: string, isFavorite: boolean): Promise<void> {
  return invoke('update_session_favorite', { sessionId, isFavorite });
}

export async function updateSessionNotes(sessionId: string, notes: string): Promise<void> {
  return invoke('update_session_notes', { sessionId, notes });
}

// ============================================================================
// Config Commands
// ============================================================================

// Track previous auto_start value to avoid unnecessary system calls
let previousAutoStart: boolean | null = null;

export async function getConfig(): Promise<Config> {
  const config = await invoke<Config>('get_config');
  // Initialize autostart tracking
  previousAutoStart = config.auto_start;
  return config;
}

export async function updateConfig(newConfig: Config): Promise<void> {
  await invoke('update_config', { newConfig });
  
  // Only sync autostart if the setting actually changed
  const autoStartChanged = previousAutoStart !== null && previousAutoStart !== newConfig.auto_start;
  previousAutoStart = newConfig.auto_start;
  
  if (autoStartChanged) {
    try {
      if (newConfig.auto_start) {
        await enableAutostart();
      } else {
        await disableAutostart();
      }
    } catch (e) {
      console.error('Failed to sync autostart setting:', e);
    }
  }
}

// ============================================================================
// Similarity Commands
// ============================================================================

export async function getSimilarityData(): Promise<SimilarityData> {
  return invoke('get_similarity_data');
}

export async function recalculateSimilarity(): Promise<number> {
  return invoke('recalculate_similarity');
}

export async function rescanSessions(): Promise<number> {
  return invoke('rescan_sessions');
}

// ============================================================================
// Video Playback Commands
// ============================================================================

export interface VideoPlaybackInfo {
  width: number;
  height: number;
  fps: number;
  duration_ms: number;
  frame_count: number;
  codec: string;
}

export interface VideoCodecCheck {
  /** The detected codec name */
  codec: string;
  /** Whether this video can be played */
  is_playable: boolean;
  /** Reason if not playable */
  reason: string | null;
}

export interface VideoFrameData {
  data_base64: string;
  timestamp_ms: number;
  duration_ms: number;
}

/** Check if a video file's codec is supported for playback */
export async function checkVideoCodec(sessionPath: string, filename: string): Promise<VideoCodecCheck> {
  return invoke('check_video_codec', { sessionPath, filename });
}

export async function getVideoInfo(sessionPath: string, filename: string): Promise<VideoPlaybackInfo> {
  return invoke('get_video_info', { sessionPath, filename });
}

export async function getVideoFrame(sessionPath: string, filename: string, timestampMs: number): Promise<VideoFrameData> {
  return invoke('get_video_frame', { sessionPath, filename, timestampMs });
}

export async function getVideoFramesBatch(
  sessionPath: string, 
  filename: string, 
  startMs: number, 
  endMs: number,
  maxFrames?: number
): Promise<VideoFrameData[]> {
  return invoke('get_video_frames_batch', { sessionPath, filename, startMs, endMs, maxFrames });
}

export async function getVideoFrameTimestamps(sessionPath: string, filename: string): Promise<number[]> {
  return invoke('get_video_frame_timestamps', { sessionPath, filename });
}

// ============================================================================
// Utility Functions
// ============================================================================

export function formatDuration(secs: number): string {
  const totalSecs = Math.floor(secs);
  const hours = Math.floor(totalSecs / 3600);
  const mins = Math.floor((totalSecs % 3600) / 60);
  const seconds = totalSecs % 60;
  
  if (hours > 0) {
    return `${hours}:${mins.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
  }
  return `${mins}:${seconds.toString().padStart(2, '0')}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDate(dateString: string): string {
  const date = new Date(dateString);
  const now = new Date();
  const diffDays = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60 * 24));
  
  if (diffDays === 0) {
    return `Today ${date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
  } else if (diffDays === 1) {
    return `Yesterday ${date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
  } else if (diffDays < 7) {
    return date.toLocaleDateString([], { weekday: 'long', hour: '2-digit', minute: '2-digit' });
  } else {
    return date.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
  }
}
