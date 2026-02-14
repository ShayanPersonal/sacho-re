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

/** Per-codec resolution capability with available framerates */
export interface CodecCapability {
  width: number;
  height: number;
  /** Available framerates at this resolution, sorted descending */
  framerates: number[];
}

export interface VideoDevice {
  id: string;
  name: string;
  /** Supported video codecs for this device */
  supported_codecs: VideoCodec[];
  /** Per-codec capabilities: codec -> list of resolutions with available framerates */
  capabilities: Record<VideoCodec, CodecCapability[]>;
  /** All formats detected from the device (for display) */
  all_formats: string[];
}

/** Per-device video source configuration.
 * target_width/height = 0 and target_fps = 0 means "Match Source" */
export interface VideoDeviceConfig {
  source_codec: VideoCodec;
  source_width: number;
  source_height: number;
  source_fps: number;
  target_width: number;
  target_height: number;
  target_fps: number;
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
    case 'raw': return 'Raw';
  }
}

/** Get a friendly resolution label like "1080p (1920x1080)" */
export function getResolutionLabel(width: number, height: number): string {
  const labels: Record<string, string> = {
    '3840x2160': '4K',
    '2560x1440': '1440p',
    '1920x1080': '1080p',
    '1280x720': '720p',
    '854x480': '480p',
    '640x480': '480p',
    '640x360': '360p',
  };
  const key = `${width}x${height}`;
  const label = labels[key];
  return label ? `${label} (${width}x${height})` : `${width}x${height}`;
}

/** Generate common target resolutions that match a given aspect ratio and don't exceed source */
export function getTargetResolutions(sourceWidth: number, sourceHeight: number): { width: number; height: number; label: string }[] {
  const gcd = (a: number, b: number): number => b === 0 ? a : gcd(b, a % b);
  const g = gcd(sourceWidth, sourceHeight);
  const ratioW = sourceWidth / g;
  const ratioH = sourceHeight / g;
  
  // Common heights to generate resolutions for
  const commonHeights = [2160, 1440, 1080, 720, 480, 360];
  const results: { width: number; height: number; label: string }[] = [];
  
  // Always include the source resolution itself
  results.push({ 
    width: sourceWidth, 
    height: sourceHeight, 
    label: getResolutionLabel(sourceWidth, sourceHeight) 
  });
  
  for (const h of commonHeights) {
    // Compute width that matches the aspect ratio
    const w = Math.round((h * ratioW) / ratioH);
    // Skip if exceeds source or matches source (already added)
    if (w > sourceWidth || h > sourceHeight) continue;
    if (w === sourceWidth && h === sourceHeight) continue;
    // Ensure even dimensions (required by most encoders)
    const ew = w % 2 === 0 ? w : w - 1;
    const eh = h % 2 === 0 ? h : h - 1;
    if (ew <= 0 || eh <= 0) continue;
    results.push({ width: ew, height: eh, label: getResolutionLabel(ew, eh) });
  }
  
  return results;
}

/** Generate common target framerates that don't exceed the source fps.
 * Uses a small tolerance (0.5) to include NTSC rates like 29.97 for a "30" threshold. */
export function getTargetFramerates(sourceFps: number): number[] {
  const common = [120, 60, 30, 24, 15];
  return common.filter(f => f <= sourceFps + 0.5);
}

/** Format an fps value for display.
 * Integer rates show as "30", fractional rates show as "29.97" */
export function formatFps(fps: number): string {
  if (fps === 0) return 'Match Source';
  const rounded = Math.round(fps);
  if (Math.abs(fps - rounded) < 0.01) return `${rounded}`;
  return fps.toFixed(2);
}

/** Codec preference order for defaults: Raw > AV1 > VP9 > VP8 > MJPEG */
const CODEC_PRIORITY: VideoCodec[] = ['raw', 'av1', 'vp9', 'vp8', 'mjpeg'];

/** Compute a smart default configuration for a device.
 * - Codec: Raw > AV1 > VP9 > VP8 > MJPEG
 * - Resolution: min(highest available, 1080p)
 * - FPS: min(highest available at chosen resolution, ~30)
 * - Target: "Match Source" (0/0/0) */
export function computeDefaultConfig(device: VideoDevice): VideoDeviceConfig | null {
  // Pick preferred codec
  let codec: VideoCodec | null = null;
  for (const c of CODEC_PRIORITY) {
    if (device.supported_codecs.includes(c)) {
      codec = c;
      break;
    }
  }
  if (!codec) return null;
  
  const caps = device.capabilities[codec];
  if (!caps || caps.length === 0) return null;
  
  // Find best resolution: highest that's ≤ 1080p, or smallest available
  // Caps are sorted by resolution descending
  const chosenCap = caps.find(c => c.height <= 1080) ?? caps[caps.length - 1];
  
  const width = chosenCap.width;
  const height = chosenCap.height;
  
  // Find best fps: highest that's ≤ ~30, or lowest available
  const fps = chosenCap.framerates.find(f => f <= 30.5)
    ?? chosenCap.framerates[chosenCap.framerates.length - 1]
    ?? 30;
  
  return {
    source_codec: codec,
    source_width: width,
    source_height: height,
    source_fps: fps,
    target_width: 0,   // Match Source
    target_height: 0,  // Match Source
    target_fps: 0,     // Match Source
  };
}

/** Warning emitted when a video device delivers frames below its negotiated rate */
export interface VideoFpsWarning {
  device_name: string;
  actual_fps: number;
  expected_fps: number;
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
  needs_repair: boolean;
}

export interface VideoFileInfo {
  filename: string;
  device_name: string;
  width: number;
  height: number;
  fps: number;
  duration_secs: number;
  size_bytes: number;
  has_audio?: boolean;
}

export type VideoEncodingMode = 'av1' | 'vp9' | 'vp8' | 'raw';
export type AudioBitDepth = 'int16' | 'int24' | 'float32';
export type AudioSampleRate = 'passthrough' | 'rate44100' | 'rate48000' | 'rate88200' | 'rate96000' | 'rate192000';

export interface Config {
  storage_path: string;
  idle_timeout_secs: number;
  pre_roll_secs: number;
  audio_format: 'wav' | 'flac';
  wav_bit_depth: AudioBitDepth;
  wav_sample_rate: AudioSampleRate;
  flac_bit_depth: AudioBitDepth;
  flac_sample_rate: AudioSampleRate;
  video_encoding_mode: VideoEncodingMode;
  dark_mode: boolean;
  auto_start: boolean;
  start_minimized: boolean;
  notify_recording_start: boolean;
  notify_recording_stop: boolean;
  selected_audio_devices: string[];
  selected_midi_devices: string[];
  trigger_midi_devices: string[];
  selected_video_devices: string[];
  /** Per-device video configuration (device_id -> config) */
  video_device_configs: Record<string, VideoDeviceConfig>;
  /** Encoder quality preset level per encoding mode (e.g. { av1: 3, vp9: 4, vp8: 3 }) */
  encoder_preset_levels: Record<string, number>;
  /** Whether to encode video during pre-roll (trades compute for memory, allows up to 30s pre-roll) */
  encode_during_preroll: boolean;
  /** Whether to combine audio and video into a single MKV file */
  combine_audio_video: boolean;
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

/** Validate that a video device configuration will work at runtime. */
export async function validateVideoDeviceConfig(
  deviceId: string,
  codec: VideoCodec,
  width: number,
  height: number,
  fps: number,
): Promise<boolean> {
  return invoke('validate_video_device_config', {
    deviceId, codec, width, height, fps,
  });
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
// Auto-select Encoder Preset
// ============================================================================

export interface AutoSelectProgress {
  /** The preset level currently being tested (5 down to 1) */
  testing_level: number;
  /** Total levels available */
  total_levels: number;
  /** Human-readable status message */
  message: string;
}

/** Run encoder auto-selection to find the best preset for the current system.
 *  Returns the best preset level (1-5).
 *  Emits 'auto-select-progress' events during the test.
 */
export async function autoSelectEncoderPreset(): Promise<number> {
  return invoke('auto_select_encoder_preset');
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

export async function repairSession(sessionId: string): Promise<SessionMetadata> {
  return invoke('repair_session', { sessionId });
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

  // Ensure OS-level autostart matches config on startup
  try {
    if (config.auto_start) {
      await enableAutostart();
    } else {
      await disableAutostart();
    }
  } catch (e) {
    console.error('Failed to sync autostart on init:', e);
  }

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
// Autostart Commands
// ============================================================================

export interface AutostartInfo {
  is_per_machine_install: boolean;
  all_users_autostart: boolean;
}

export async function getAutostartInfo(): Promise<AutostartInfo> {
  return invoke<AutostartInfo>('get_autostart_info');
}

export async function setAllUsersAutostart(enabled: boolean): Promise<void> {
  return invoke('set_all_users_autostart', { enabled });
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
// App Stats
// ============================================================================

export interface AppStats {
  /** Process CPU usage percentage (0-100+) */
  cpu_percent: number;
  /** Process resident memory in bytes */
  memory_bytes: number;
  /** Total size of recordings folder in bytes */
  storage_used_bytes: number;
  /** Free space on the recordings disk in bytes */
  disk_free_bytes: number;
}

export async function getAppStats(): Promise<AppStats> {
  return invoke<AppStats>('get_app_stats');
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
