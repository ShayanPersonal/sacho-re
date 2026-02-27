// Tauri command bindings

import { invoke } from "@tauri-apps/api/core";
import {
  enable as enableAutostart,
  disable as disableAutostart,
} from "@tauri-apps/plugin-autostart";

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
export type VideoCodec =
  | "mjpeg"
  | "av1"
  | "vp8"
  | "vp9"
  | "raw"
  | "ffv1"
  | "h264";

/** Supported container formats */
export type ContainerFormat = "mkv" | "webm" | "mp4";

/** Hardware encoder backend types */
export type HardwareEncoderType =
  | "nvenc"
  | "amf"
  | "qsv"
  | "vaapi"
  | "mediafoundation"
  | "videotoolbox"
  | "software";

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
  /** Per-format capabilities: format string -> list of resolutions with available framerates.
   * Format strings are actual pixel/codec names from GStreamer (e.g. "YUY2", "NV12", "MJPEG", "H264"). */
  capabilities: Record<string, CodecCapability[]>;
}

/** Per-device video source configuration. */
export interface VideoDeviceConfig {
  /** Source format string (e.g. "YUY2", "NV12", "MJPEG", "H264") */
  source_format: string;
  source_width: number;
  source_height: number;
  source_fps: number;
  /** true = record as-is, false = decode and re-encode */
  passthrough: boolean;
  /** Target encoding codec (null = auto-detect best) */
  encoding_codec: VideoCodec | null;
  /** Hardware encoder backend (null = auto-detect best) */
  encoder_type: HardwareEncoderType | null;
  /** Quality preset 1-5 */
  preset_level: number;
  /** Compute effort level 1-5 (only affects software encoders) */
  effort_level: number;
  /** Encoding bit depth for lossless codecs like FFV1. null = 8-bit default. */
  video_bit_depth: number | null;
  target_width: number;
  target_height: number;
  target_fps: number;
}

/** Check if a video device supports any recording format */
export function isVideoDeviceSupported(device: VideoDevice): boolean {
  return Object.keys(device.capabilities).length > 0;
}

/** Get human-readable codec name for encoding codecs */
export function getCodecDisplayName(codec: VideoCodec): string {
  switch (codec) {
    case "mjpeg":
      return "MJPEG";
    case "vp8":
      return "VP8";
    case "vp9":
      return "VP9";
    case "av1":
      return "AV1";
    case "raw":
      return "RAW";
    case "ffv1":
      return "FFV1";
    case "h264":
      return "H.264";
  }
}

/** Returns a UI-friendly display name for a format string.
 * E.g. "H264" → "H.264". All other formats are returned as-is. */
export function formatDisplayName(format: string): string {
  if (format === "H264") return "H.264";
  return format;
}

/** Returns true for raw pixel formats that require encoding.
 * Returns false for known pre-encoded formats (MJPEG, H264, AV1, VP8, VP9). */
export function isRawFormat(format: string): boolean {
  return !["MJPEG", "H264", "AV1", "VP8", "VP9"].includes(format);
}

/** Returns true if a GStreamer source format name is 10-bit or higher */
export function is10BitFormat(format: string): boolean {
  return format.includes("10");
}

/** Get a friendly resolution label like "1080p (1920x1080)" */
export function getResolutionLabel(width: number, height: number): string {
  const labels: Record<string, string> = {
    "3840x2160": "4K",
    "2560x1440": "1440p",
    "1920x1080": "1080p",
    "1280x720": "720p",
    "854x480": "480p",
    "640x480": "480p",
    "640x360": "360p",
  };
  const key = `${width}x${height}`;
  const label = labels[key];
  return label ? `${label} (${width}x${height})` : `${width}x${height}`;
}

/** Generate common target resolutions that match a given aspect ratio and don't exceed source */
export function getTargetResolutions(
  sourceWidth: number,
  sourceHeight: number,
): { width: number; height: number; label: string }[] {
  const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
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
    label: getResolutionLabel(sourceWidth, sourceHeight),
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

/** Generate target framerates: source fps first (default), then common values below source.
 * Deduplicates common values within 0.5 of the source fps to avoid near-duplicates. */
export function getTargetFramerates(sourceFps: number): number[] {
  const common = [120, 60, 30, 24, 15];
  const filtered = common.filter((f) => f <= sourceFps + 0.5 && Math.abs(f - sourceFps) > 0.5);
  return [sourceFps, ...filtered];
}

/** Format an fps value for display.
 * Integer rates show as "30", fractional rates show as "29.97" */
export function formatFps(fps: number): string {
  const rounded = Math.round(fps);
  if (Math.abs(fps - rounded) < 0.01) return `${rounded}`;
  return fps.toFixed(2);
}

/** Default maximum encoding height when the user hasn't chosen a specific target.
 * This is just the initial selection — users can pick higher values in the UI. */
export const DEFAULT_TARGET_HEIGHT = 1080;

/** Default maximum encoding FPS when the user hasn't chosen a specific target.
 * This is just the initial selection — users can pick higher values in the UI. */
export const DEFAULT_TARGET_FPS = 30;

/** Tolerance for comparing FPS to DEFAULT_TARGET_FPS (includes 30000/1001 ≈ 29.97). */
export const DEFAULT_TARGET_FPS_TOLERANCE = 30.5;

/** Whether a format supports passthrough (can be stored without re-encoding). */
export function supportsPassthrough(format: string): boolean {
  return !isRawFormat(format);
}

/** Whether a format should default to passthrough (no re-encoding).
 * Used by both computeDefaultConfig and the Configure modal to stay in sync. */
export function defaultPassthrough(format: string): boolean {
  if (!supportsPassthrough(format)) return false;
  if (format === "MJPEG") return false; // Large file sizes, re-encode by default
  return true;
}

/** Format preference order: modern compressed first, then raw by ecosystem compatibility.
 * Used by both computeDefaultConfig and the Configure modal to stay in sync. */
export const FORMAT_PRIORITY: string[] = [
  "MJPEG",
  "NV12", "I420", "YV12", "YUY2", "BGR", "BGRx",
  "AV1", "VP9", "H264", "VP8",
];

/** Sort an array of format strings by FORMAT_PRIORITY order. */
export function sortFormatsByPriority(formats: string[]): string[] {
  return [...formats].sort((a, b) => {
    const ai = FORMAT_PRIORITY.indexOf(a);
    const bi = FORMAT_PRIORITY.indexOf(b);
    const pa = ai !== -1 ? ai : FORMAT_PRIORITY.length;
    const pb = bi !== -1 ? bi : FORMAT_PRIORITY.length;
    return pa - pb;
  });
}

/** Compute a smart default configuration for a device.
 * - Format: AV1 > VP9 > H264 > VP8 > MJPEG > NV12 > I420 > YV12 > YUY2 > BGR > BGRx
 * - Resolution: min(highest available, 1080p)
 * - FPS: min(highest available at chosen resolution, ~30)
 * - Target: "Match Source" (0/0/0) */
export function computeDefaultConfig(
  device: VideoDevice,
): VideoDeviceConfig | null {
  // Pick preferred format
  let format: string | null = null;
  for (const f of FORMAT_PRIORITY) {
    if (device.capabilities[f]) {
      format = f;
      break;
    }
  }
  // Fall back to first available format
  if (!format) {
    const keys = Object.keys(device.capabilities);
    if (keys.length === 0) return null;
    format = keys[0];
  }

  const caps = device.capabilities[format];
  if (!caps || caps.length === 0) return null;

  // Find best resolution: highest that's ≤ default target height, or smallest available
  // Caps are sorted by resolution descending
  const chosenCap =
    caps.find((c) => c.height <= DEFAULT_TARGET_HEIGHT) ?? caps[caps.length - 1];

  const width = chosenCap.width;
  const height = chosenCap.height;

  // Find best fps: highest that's ≤ default target fps, or lowest available
  const fps =
    chosenCap.framerates.find((f) => f <= DEFAULT_TARGET_FPS_TOLERANCE) ??
    chosenCap.framerates[chosenCap.framerates.length - 1] ??
    DEFAULT_TARGET_FPS;

  return {
    source_format: format,
    source_width: width,
    source_height: height,
    source_fps: fps,
    passthrough: defaultPassthrough(format),
    encoding_codec: null,
    encoder_type: null,
    preset_level: 3,
    effort_level: 3,
    video_bit_depth: null,
    target_width: width,
    target_height: height,
    target_fps: fps,
  };
}

/** Warning emitted when a video device delivers frames below its negotiated rate */
export interface VideoFpsWarning {
  device_name: string;
  actual_fps: number;
  expected_fps: number;
}

export interface RecordingState {
  status: "idle" | "recording" | "stopping" | "initializing";
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
  notes: string;
  title: string | null;
}

export interface SessionMetadata {
  id: string;
  timestamp: string;
  duration_secs: number;
  path: string;
  audio_files: AudioFileInfo[];
  midi_files: MidiFileInfo[];
  video_files: VideoFileInfo[];
  notes: string;
  title: string | null;
}

export interface AudioFileInfo {
  filename: string;
  device_name: string;
  duration_secs: number;
}

export interface MidiFileInfo {
  filename: string;
  device_name: string;
  event_count: number;
  needs_repair: boolean;
}

export interface VideoFileInfo {
  filename: string;
  device_name: string;
  duration_secs: number;
}

export type AudioBitDepth = "int16" | "int24" | "float32";
export type AudioSampleRate =
  | "passthrough"
  | "rate44100"
  | "rate48000"
  | "rate88200"
  | "rate96000"
  | "rate192000";

export interface Config {
  storage_path: string;
  idle_timeout_secs: number;
  pre_roll_secs: number;
  audio_format: "wav" | "flac";
  wav_bit_depth: AudioBitDepth;
  wav_sample_rate: AudioSampleRate;
  flac_bit_depth: AudioBitDepth;
  flac_sample_rate: AudioSampleRate;
  dark_mode: boolean;
  auto_start: boolean;
  start_minimized: boolean;
  notify_recording_start: boolean;
  notify_recording_stop: boolean;
  sound_recording_start: boolean;
  sound_recording_stop: boolean;
  sound_volume_start: number;
  sound_volume_stop: number;
  custom_sound_start: string | null;
  custom_sound_stop: string | null;
  sound_device_disconnect: boolean;
  sound_volume_disconnect: number;
  custom_sound_disconnect: string | null;
  selected_audio_devices: string[];
  selected_midi_devices: string[];
  trigger_midi_devices: string[];
  trigger_audio_devices: string[];
  audio_trigger_thresholds: Record<string, number>;
  selected_video_devices: string[];
  /** Per-device video configuration (device_id -> config) */
  video_device_configs: Record<string, VideoDeviceConfig>;
  /** Whether to encode video during pre-roll (trades compute for memory, allows up to 30s pre-roll) */
  encode_during_preroll: boolean;
  /** Whether to combine audio and video into a single container file */
  combine_audio_video: boolean;
  /** Preferred video container format. AV1/VP9/H.264 remux to this; FFV1 stays MKV, VP8 stays WebM. */
  preferred_video_container: ContainerFormat;
  device_presets: DevicePreset[];
  current_preset: string | null;
}

export interface DevicePreset {
  name: string;
  audio_devices: string[];
  midi_devices: string[];
  trigger_midi_devices: string[];
  trigger_audio_devices: string[];
  video_devices: string[];
}

export interface AudioTriggerLevel {
  device_id: string;
  current_rms: number;
  peak_level: number;
}

export interface SessionFilter {
  search?: string;
  has_audio?: boolean;
  has_midi?: boolean;
  has_video?: boolean;
  has_notes?: boolean;
  limit?: number;
  offset?: number;
}

export interface MidiImportInfo {
  id: string;
  file_name: string;
  file_path: string;
  has_features: boolean;
  imported_at: string;
}

export interface SimilarityResult {
  file: MidiImportInfo;
  score: number;
  rank: number;
  match_offset_secs: number;
}

export type SimilarityMode = "melodic" | "harmonic";

// ============================================================================
// Device Commands
// ============================================================================

export async function refreshAllDevices(): Promise<void> {
  return invoke("refresh_devices");
}

export async function getAudioDevices(): Promise<AudioDevice[]> {
  return invoke("get_audio_devices");
}

export async function getMidiDevices(): Promise<MidiDevice[]> {
  return invoke("get_midi_devices");
}

export async function getVideoDevices(): Promise<VideoDevice[]> {
  return invoke("get_video_devices");
}

/** Validate that a video device configuration will work at runtime. */
export async function validateVideoDeviceConfig(
  deviceId: string,
  format: string,
  width: number,
  height: number,
  fps: number,
): Promise<boolean> {
  return invoke("validate_video_device_config", {
    deviceId,
    format,
    width,
    height,
    fps,
  });
}

// ============================================================================
// Encoder Availability
// ============================================================================

export interface EncoderBackendInfo {
  id: string;
  display_name: string;
  is_hardware: boolean;
}

export interface CodecEncoderInfo {
  available: boolean;
  has_hardware: boolean;
  encoders: EncoderBackendInfo[];
  recommended: string | null;
}

export interface EncoderAvailability {
  av1: CodecEncoderInfo;
  vp9: CodecEncoderInfo;
  vp8: CodecEncoderInfo;
  h264: CodecEncoderInfo;
  ffv1: CodecEncoderInfo;
  recommended_codec: string;
}

export async function getEncoderAvailability(): Promise<EncoderAvailability> {
  return invoke("get_encoder_availability");
}

/** Look up the CodecEncoderInfo for a given codec from the availability object. */
export function getCodecInfo(availability: EncoderAvailability, codec: VideoCodec): CodecEncoderInfo | null {
  return availability[codec as keyof Pick<EncoderAvailability, "av1" | "vp9" | "vp8" | "h264" | "ffv1">] ?? null;
}

/** Resolve null codec/encoder fields in a config to the backend's recommended values.
 * Returns a new config object (does not mutate the input). */
export function resolveEncoderDefaults(
  config: VideoDeviceConfig,
  availability: EncoderAvailability,
): VideoDeviceConfig {
  const resolved = { ...config };
  if (!resolved.encoding_codec) {
    resolved.encoding_codec = availability.recommended_codec as VideoCodec;
  }
  if (!resolved.encoder_type && resolved.encoding_codec) {
    const info = getCodecInfo(availability, resolved.encoding_codec);
    if (info?.recommended) {
      resolved.encoder_type = info.recommended as HardwareEncoderType;
    }
  }
  return resolved;
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

/** Result of an encoder preset test */
export interface EncoderTestResult {
  success: boolean;
  warning: boolean;
  frames_sent: number;
  frames_dropped: number;
  message: string;
}

/** Test the current encoder preset for a specific video device.
 *  Runs a ~3 second encoding test and returns whether the encoder can keep up. */
export async function testEncoderPreset(
  deviceId: string,
): Promise<EncoderTestResult> {
  return invoke("test_encoder_preset", { deviceId });
}

/** Run encoder auto-selection for a specific device.
 *  Returns the best preset level (1-5).
 *  Emits 'auto-select-progress' events during the test.
 */
export async function autoSelectEncoderPreset(
  deviceId: string,
): Promise<number> {
  return invoke("auto_select_encoder_preset", { deviceId });
}

// ============================================================================
// Recording Commands
// ============================================================================

export async function getRecordingState(): Promise<RecordingState> {
  return invoke("get_recording_state");
}

export async function startRecording(): Promise<string> {
  return invoke("start_recording");
}

export async function stopRecording(): Promise<void> {
  return invoke("stop_recording");
}

// ============================================================================
// Session Commands
// ============================================================================

export async function getSessions(
  filter: SessionFilter = {},
): Promise<SessionSummary[]> {
  return invoke("get_sessions", { filter });
}

export async function getSessionDetail(
  sessionId: string,
): Promise<SessionMetadata | null> {
  return invoke("get_session_detail", { sessionId });
}

export async function deleteSession(sessionId: string): Promise<void> {
  return invoke("delete_session", { sessionId });
}

export async function repairSession(
  sessionId: string,
): Promise<SessionMetadata> {
  return invoke("repair_session", { sessionId });
}

export async function readSessionFile(
  sessionPath: string,
  filename: string,
): Promise<Uint8Array> {
  const data = await invoke<number[]>("read_session_file", {
    sessionPath,
    filename,
  });
  return new Uint8Array(data);
}

export async function updateSessionNotes(
  sessionId: string,
  notes: string,
): Promise<void> {
  return invoke("update_session_notes", { sessionId, notes });
}

export async function renameSession(
  sessionId: string,
  newTitle: string,
): Promise<SessionSummary> {
  return invoke("rename_session", { sessionId, newTitle });
}

// ============================================================================
// Config Commands
// ============================================================================

// Track previous auto_start value to avoid unnecessary system calls
let previousAutoStart: boolean | null = null;

export async function getConfig(): Promise<Config> {
  const config = await invoke<Config>("get_config");
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
    console.error("Failed to sync autostart on init:", e);
  }

  return config;
}

export async function updateConfig(newConfig: Config): Promise<void> {
  await invoke("update_config", { newConfig });

  // Only sync autostart if the setting actually changed
  const autoStartChanged =
    previousAutoStart !== null && previousAutoStart !== newConfig.auto_start;
  previousAutoStart = newConfig.auto_start;

  if (autoStartChanged) {
    try {
      if (newConfig.auto_start) {
        await enableAutostart();
      } else {
        await disableAutostart();
      }
    } catch (e) {
      console.error("Failed to sync autostart setting:", e);
    }
  }
}

export async function updateAudioTriggerThresholds(
  thresholds: Record<string, number>,
): Promise<void> {
  await invoke("update_audio_trigger_thresholds", { thresholds });
}

// ============================================================================
// Device Health Commands
// ============================================================================

export interface DisconnectedDeviceInfo {
  id: string;
  name: string;
  device_type: string;
}

export async function getDisconnectedDevices(): Promise<
  DisconnectedDeviceInfo[]
> {
  return invoke("get_disconnected_devices");
}

export async function restartDevicePipelines(
  deviceTypes: string[],
): Promise<void> {
  await invoke("restart_device_pipelines", { deviceTypes });
}

/** Copy a custom sound file into the app config dir. Returns the relative path. */
export async function setCustomSound(
  sourcePath: string,
  soundType: "start" | "stop" | "disconnect",
): Promise<string> {
  return invoke("set_custom_sound", { sourcePath, soundType });
}

/** Clear a custom sound: delete the copied file and remove from config. */
export async function clearCustomSound(
  soundType: "start" | "stop" | "disconnect",
): Promise<void> {
  return invoke("clear_custom_sound", { soundType });
}

// ============================================================================
// Autostart Commands
// ============================================================================

export interface AutostartInfo {
  is_per_machine_install: boolean;
  all_users_autostart: boolean;
}

export async function getAutostartInfo(): Promise<AutostartInfo> {
  return invoke<AutostartInfo>("get_autostart_info");
}

export async function setAllUsersAutostart(enabled: boolean): Promise<void> {
  return invoke("set_all_users_autostart", { enabled });
}

// ============================================================================
// Similarity Commands
// ============================================================================

export async function importMidiFolder(path: string): Promise<MidiImportInfo[]> {
  return invoke("import_midi_folder", { path });
}

export async function getMidiImports(): Promise<MidiImportInfo[]> {
  return invoke("get_midi_imports");
}

export async function getSimilarFiles(fileId: string, mode: SimilarityMode): Promise<SimilarityResult[]> {
  return invoke("get_similar_files", { fileId, mode });
}

export async function clearMidiImports(): Promise<void> {
  return invoke("clear_midi_imports");
}

export interface RescanProgress {
  current: number;
  total: number;
}

export async function rescanSessions(): Promise<number> {
  return invoke("rescan_sessions");
}

export async function resetCache(): Promise<number> {
  return invoke("reset_cache");
}

export async function resetSettings(): Promise<void> {
  return invoke("reset_settings");
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
export async function checkVideoCodec(
  sessionPath: string,
  filename: string,
): Promise<VideoCodecCheck> {
  return invoke("check_video_codec", { sessionPath, filename });
}

export async function getVideoInfo(
  sessionPath: string,
  filename: string,
): Promise<VideoPlaybackInfo> {
  return invoke("get_video_info", { sessionPath, filename });
}

export async function getVideoFrame(
  sessionPath: string,
  filename: string,
  timestampMs: number,
): Promise<VideoFrameData> {
  return invoke("get_video_frame", { sessionPath, filename, timestampMs });
}

export async function getVideoFramesBatch(
  sessionPath: string,
  filename: string,
  startMs: number,
  endMs: number,
  maxFrames?: number,
): Promise<VideoFrameData[]> {
  return invoke("get_video_frames_batch", {
    sessionPath,
    filename,
    startMs,
    endMs,
    maxFrames,
  });
}

export async function getVideoFrameTimestamps(
  sessionPath: string,
  filename: string,
): Promise<number[]> {
  return invoke("get_video_frame_timestamps", { sessionPath, filename });
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
  return invoke<AppStats>("get_app_stats");
}

// ============================================================================
// Utility Functions
// ============================================================================

export function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return "0:00";
  const totalSecs = Math.floor(secs);
  const hours = Math.floor(totalSecs / 3600);
  const mins = Math.floor((totalSecs % 3600) / 60);
  const seconds = totalSecs % 60;

  if (hours > 0) {
    return `${hours}:${mins.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`;
  }
  return `${mins}:${seconds.toString().padStart(2, "0")}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDate(dateString: string): string {
  const date = new Date(dateString);
  const now = new Date();

  // Use calendar-day comparison (midnight-based) to match session list grouping
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const weekAgo = new Date(today);
  weekAgo.setDate(weekAgo.getDate() - 7);
  const sessionDay = new Date(date.getFullYear(), date.getMonth(), date.getDate());

  const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

  if (sessionDay.getTime() >= today.getTime()) {
    return `Today ${time}`;
  } else if (sessionDay.getTime() >= yesterday.getTime()) {
    return `Yesterday ${time}`;
  } else if (sessionDay >= weekAgo) {
    return date.toLocaleDateString([], {
      weekday: "long",
      hour: "2-digit",
      minute: "2-digit",
    });
  } else {
    return date.toLocaleDateString([], {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  }
}
