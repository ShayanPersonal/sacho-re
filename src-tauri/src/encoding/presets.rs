//! Encoder preset system for real-time video encoding
//!
//! This module provides 5 quality preset levels (1–5) for every supported
//! encoder variant (codec + hardware type). All presets are designed for
//! real-time encoding:
//!
//! - **Level 1** — Lightest: minimal CPU/GPU load, lowest quality
//! - **Level 2** — Light: low resource usage, acceptable quality
//! - **Level 3** — Balanced: moderate resources, good quality (default)
//! - **Level 4** — Quality: higher resource usage, very good quality
//! - **Level 5** — Maximum: highest quality feasible in real-time
//!
//! ## Adding presets for a new encoder
//!
//! When a new encoder backend is added (e.g., a new hardware vendor):
//!
//! 1. Add a new `apply_<vendor>_<codec>()` function in this file, following
//!    the pattern of existing ones (match on `level`, set encoder properties).
//! 2. Add a match arm in [`apply_preset()`] for your `(VideoCodec, HardwareEncoderType)`.
//! 3. Each level must produce output suitable for real-time encoding at
//!    common resolutions (720p–1080p, 30 fps).
//! 4. Document which GStreamer element properties you set and why.
//!
//! ## Bitrate scaling
//!
//! Base bitrates are calibrated for **1080p @ 30 fps** (the reference point).
//! At runtime, [`apply_preset()`] scales them by the actual source resolution
//! and framerate using per-codec spatial exponents and a shared temporal
//! exponent. See the constants and helpers below `DEFAULT_PRESET` for details.
//!
//! The auto-select system ([`crate::commands::auto_select_encoder_preset`])
//! will automatically test your new presets at runtime.

use gstreamer as gst;
use gstreamer::prelude::*;

use super::encoder::HardwareEncoderType;
use super::VideoCodec;

/// Try to set a u32 property on an element, clamping to the element's valid range.
///
/// Some MFTs expose a property but accept only a subset of values (e.g., a
/// bframes property that allows 0–2 but not 3). This helper reads the
/// `ParamSpecUInt` bounds and clamps accordingly, preventing a panic from
/// `set_property` when the value is out of range.
///
/// Returns `true` if the property was found and set, `false` if missing.
fn try_set_u32_clamped(element: &gst::Element, name: &str, value: u32) -> bool {
    let Some(pspec) = element.find_property(name) else {
        return false;
    };
    if let Some(uint_spec) = pspec.downcast_ref::<gst::glib::ParamSpecUInt>() {
        let clamped = value.clamp(uint_spec.minimum(), uint_spec.maximum());
        if clamped != value {
            let element_name = element
                .factory()
                .map(|f| f.name().to_string())
                .unwrap_or_default();
            log::warn!(
                "[Preset] {}={} out of range [{}, {}] for {}, using {}",
                name,
                value,
                uint_spec.minimum(),
                uint_spec.maximum(),
                element_name,
                clamped,
            );
        }
        element.set_property(name, clamped);
    } else {
        element.set_property(name, value);
    }
    true
}

/// Minimum preset level (lightest computational load)
pub const MIN_PRESET: u8 = 1;
/// Maximum preset level (highest quality, most intensive)
pub const MAX_PRESET: u8 = 5;
/// Default preset level (balanced)
pub const DEFAULT_PRESET: u8 = 3;

// ── Base bitrate constants (kbps @ 1080p30) ──────────────────────────────
//
// Single source of truth for per-codec, per-backend bitrates.
// Indexed by `(level - 1)`, so level 1 → index 0, level 5 → index 4.

const AV1_HW_BITRATES_KBPS: [u32; 5] = [1_500, 2_000, 3_000, 4_000, 5_000];
const AV1_SW_BITRATES_KBPS: [u32; 5] = [1_200, 1_800, 2_500, 3_500, 4_500];
const VP9_BITRATES_KBPS:    [u32; 5] = [2_000, 2_500, 3_500, 4_500, 5_500];
const VP8_BITRATES_KBPS:    [u32; 5] = [2_500, 3_500, 5_000, 6_500, 8_000];
const H264_BITRATES_KBPS:   [u32; 5] = [2_500, 3_500, 5_000, 7_000, 9_000];

// ── Resolution/FPS-aware bitrate scaling ─────────────────────────────────
//
// Base bitrates are calibrated for 1080p@30fps (the reference point).
// At runtime they are scaled by the actual source resolution and framerate
// so that a 480p15 webcam gets a proportionally lower bitrate while a
// 4K60 camera gets proportionally more.

/// Reference resolution: 1920 × 1080 = 2 073 600 pixels
const REFERENCE_PIXELS: f64 = 2_073_600.0;
/// Reference framerate: 30 fps
const REFERENCE_FPS: f64 = 30.0;
/// Minimum scale factor (floor) — prevents absurdly low bitrates
const MIN_SCALE: f64 = 0.25;
/// Maximum scale factor (ceiling) — prevents absurdly high bitrates
const MAX_SCALE: f64 = 6.0;
/// Temporal exponent (β): doubling fps → ~41% more bitrate
const TEMPORAL_EXPONENT: f64 = 0.5;

/// Per-codec spatial exponent (α).
///
/// More efficient codecs exploit spatial redundancy better and therefore
/// need a smaller bitrate increase per additional pixel.
fn spatial_exponent(codec: VideoCodec) -> f64 {
    match codec {
        VideoCodec::Av1 => 0.70,
        VideoCodec::Vp9 => 0.75,
        VideoCodec::H264 => 0.80,
        VideoCodec::Vp8 => 0.85,
        _ => 0.80, // sensible default
    }
}

/// Compute the bitrate scale factor for the given codec, resolution, and fps.
///
/// ```text
/// pixel_ratio = (width * height) / (1920 * 1080)
/// fps_ratio   = fps / 30.0
/// scale       = pixel_ratio^α * fps_ratio^β
/// ```
///
/// The result is clamped to [`MIN_SCALE`]..=[`MAX_SCALE`].
pub(super) fn bitrate_scale(codec: VideoCodec, width: u32, height: u32, fps: f64) -> f64 {
    let pixels = (width as f64) * (height as f64);
    let pixel_ratio = pixels / REFERENCE_PIXELS;
    let fps_ratio = fps / REFERENCE_FPS;

    let alpha = spatial_exponent(codec);
    let scale = pixel_ratio.powf(alpha) * fps_ratio.powf(TEMPORAL_EXPONENT);

    scale.clamp(MIN_SCALE, MAX_SCALE)
}

/// Scale a u32 base bitrate (kbps) by the given factor, rounding to nearest.
fn scale_bitrate_u32(base: u32, scale: f64) -> u32 {
    ((base as f64) * scale).round() as u32
}

/// Scale an i32 base bitrate (bps) by the given factor, rounding to nearest.
fn scale_bitrate_i32(base: i32, scale: f64) -> i32 {
    ((base as f64) * scale).round() as i32
}

/// Get a human-readable label for a preset level.
pub fn preset_label(level: u8) -> &'static str {
    match level.clamp(MIN_PRESET, MAX_PRESET) {
        1 => "Lightest",
        2 => "Light",
        3 => "Balanced",
        4 => "Quality",
        5 => "Maximum",
        _ => "Balanced",
    }
}

/// Get the base bitrate (kbps, calibrated for 1080p@30fps) for a given
/// codec + hardware backend + preset level. Returns `None` for FFV1 or
/// unsupported codec/backend combinations.
pub fn base_bitrate_kbps(
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    level: u8,
) -> Option<u32> {
    let level = level.clamp(MIN_PRESET, MAX_PRESET);
    let idx = (level - 1) as usize;

    let arr: &[u32; 5] = match (codec, hw_type) {
        // AV1
        (VideoCodec::Av1, HardwareEncoderType::Software) => &AV1_SW_BITRATES_KBPS,
        (VideoCodec::Av1, _) => &AV1_HW_BITRATES_KBPS,
        // VP9
        (VideoCodec::Vp9, _) => &VP9_BITRATES_KBPS,
        // VP8
        (VideoCodec::Vp8, _) => &VP8_BITRATES_KBPS,
        // H264
        (VideoCodec::H264, _) => &H264_BITRATES_KBPS,
        // FFV1 is lossless — no meaningful bitrate
        _ => return None,
    };

    Some(arr[idx])
}

/// Get the bitrate (kbps) scaled for the actual encoding resolution and fps.
/// Returns `None` for FFV1 or unsupported codec/backend combinations.
pub fn scaled_bitrate_kbps(
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    level: u8,
    width: u32,
    height: u32,
    fps: f64,
) -> Option<u32> {
    let base = base_bitrate_kbps(codec, hw_type, level)?;
    let scale = bitrate_scale(codec, width, height, fps);
    Some(((base as f64) * scale).round() as u32)
}

/// Apply encoder-specific parameters for the given preset level.
///
/// This is the **main extension point** for the preset system. When adding a
/// new encoder, add a match arm here that dispatches to your preset function.
///
/// Base bitrates are calibrated for 1080p@30fps. The `width`, `height`, and
/// `fps` parameters describe the *effective* encoding resolution/framerate
/// (after any target scaling) and are used to scale bitrates accordingly.
///
/// When `bitrate_override_kbps` is `Some`, the effective bitrate scale is
/// adjusted so that `base_kbps * scale ≈ override_kbps`. All `apply_*`
/// functions continue to use `scale_bitrate_*()` unchanged.
///
/// # Arguments
/// * `encoder` — the GStreamer encoder element to configure
/// * `codec` — the target video codec
/// * `hw_type` — the hardware encoder type being used
/// * `level` — preset level (1–5; clamped internally)
/// * `keyframe_interval` — keyframe interval in frames (0 = encoder default)
/// * `width` — effective encoding width in pixels
/// * `height` — effective encoding height in pixels
/// * `fps` — effective encoding framerate
/// * `bitrate_override_kbps` — custom bitrate override; `None` = use preset default
pub fn apply_preset(
    encoder: &gst::Element,
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    level: u8,
    keyframe_interval: u32,
    width: u32,
    height: u32,
    fps: f64,
    bitrate_override_kbps: Option<u32>,
) {
    let level = level.clamp(MIN_PRESET, MAX_PRESET);

    // Compute resolution/fps scale factor (skip for lossless FFV1)
    let scale = if codec == VideoCodec::Ffv1 {
        1.0
    } else {
        let resolution_scale = bitrate_scale(codec, width, height, fps);

        // If the user specified a custom bitrate, compute a synthetic scale
        // factor so that `base_kbps * scale ≈ override_kbps`.
        let s = if let Some(override_kbps) = bitrate_override_kbps {
            if let Some(base) = base_bitrate_kbps(codec, hw_type, level) {
                if base > 0 {
                    (override_kbps as f64) / (base as f64)
                } else {
                    resolution_scale
                }
            } else {
                resolution_scale
            }
        } else {
            resolution_scale
        };

        println!(
            "[Preset] {:?} {:?} level={} {}x{}@{:.1}fps → scale={:.3}{}",
            codec, hw_type, level, width, height, fps, s,
            if bitrate_override_kbps.is_some() { " (custom bitrate)" } else { "" }
        );
        s
    };

    match (codec, hw_type) {
        // ── AV1 encoders ────────────────────────────────────────────────
        (VideoCodec::Av1, HardwareEncoderType::Nvenc) => {
            apply_nvenc_av1(encoder, level, keyframe_interval, scale);
        }
        (VideoCodec::Av1, HardwareEncoderType::Amf) => {
            apply_amf_av1(encoder, level, scale);
        }
        (VideoCodec::Av1, HardwareEncoderType::Qsv) => {
            apply_qsv_av1(encoder, level, scale);
        }
        (VideoCodec::Av1, HardwareEncoderType::VaApi) => {
            apply_vaapi_av1(encoder, level, scale);
        }
        (VideoCodec::Av1, HardwareEncoderType::Software) => {
            apply_software_av1(encoder, level, keyframe_interval, scale);
        }

        // ── VP9 encoders ────────────────────────────────────────────────
        (VideoCodec::Vp9, HardwareEncoderType::Qsv) => {
            apply_qsv_vp9(encoder, level, scale);
        }
        (VideoCodec::Vp9, HardwareEncoderType::VaApi) => {
            apply_vaapi_vp9(encoder, level, scale);
        }
        (VideoCodec::Vp9, HardwareEncoderType::Software) => {
            apply_software_vp9(encoder, level, keyframe_interval, scale);
        }

        // ── VP8 encoders ────────────────────────────────────────────────
        (VideoCodec::Vp8, HardwareEncoderType::Qsv) => {
            apply_qsv_vp8(encoder, level, scale);
        }
        (VideoCodec::Vp8, HardwareEncoderType::VaApi) => {
            apply_vaapi_vp8(encoder, level, scale);
        }
        (VideoCodec::Vp8, HardwareEncoderType::Software) => {
            apply_software_vp8(encoder, level, keyframe_interval, scale);
        }

        // ── H264 encoders (platform-native only) ────────────────────────
        (VideoCodec::H264, HardwareEncoderType::MediaFoundation) => {
            apply_mf_h264(encoder, level, keyframe_interval, scale);
        }
        (VideoCodec::H264, HardwareEncoderType::VideoToolbox) => {
            apply_vtb_h264(encoder, level, keyframe_interval, scale);
        }

        // ── FFV1 encoder ────────────────────────────────────────────────
        (VideoCodec::Ffv1, HardwareEncoderType::Software) => {
            apply_software_ffv1(encoder, level);
        }

        // ── Unsupported combinations ────────────────────────────────────
        // NVENC and AMF don't support VP8/VP9 encoding.
        _ => {
            println!(
                "[Preset] No presets for {:?} + {:?}, using encoder defaults",
                codec, hw_type
            );
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// AV1 Encoders
// ═════════════════════════════════════════════════════════════════════════════

/// NVIDIA NVENC AV1 (nvav1enc) — RTX 40 series+
///
/// Properties used:
/// - `preset`: p1 (fastest) to p7 (best quality)
/// - `bitrate`: target bitrate in kbps
/// - `gop-size`: keyframe interval
fn apply_nvenc_av1(encoder: &gst::Element, level: u8, keyframe_interval: u32, scale: f64) {
    let idx = (level - 1) as usize;
    let preset = match level {
        1 => "p1",
        2 => "p2",
        3 => "p4",
        4 => "p5",
        _ => "p7",
    };

    encoder.set_property_from_str("preset", preset);
    encoder.set_property("bitrate", scale_bitrate_u32(AV1_HW_BITRATES_KBPS[idx], scale));
    if keyframe_interval > 0 {
        encoder.set_property("gop-size", keyframe_interval as i32);
    }
}

/// AMD AMF AV1 (amfav1enc) — RX 7000 series+
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_amf_av1(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(AV1_HW_BITRATES_KBPS[idx], scale));
}

/// Intel QuickSync AV1 (qsvav1enc)
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_qsv_av1(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(AV1_HW_BITRATES_KBPS[idx], scale));
}

/// VA-API AV1 (vaav1enc / vaapiav1enc) — Linux
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_vaapi_av1(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(AV1_HW_BITRATES_KBPS[idx], scale));
}

/// Software AV1 via SVT-AV1 (svtav1enc)
///
/// Properties used:
/// - `preset`: 0 (best quality) to 13 (fastest); 8+ needed for real-time
/// - `target-bitrate`: kbps (enables CBR mode)
/// - `intra-period-length`: keyframe interval (-2 = auto, -1 = no updates)
fn apply_software_av1(encoder: &gst::Element, level: u8, keyframe_interval: u32, scale: f64) {
    let idx = (level - 1) as usize;
    let preset = match level {
        1 => 12u32,
        2 => 11,
        3 => 10,
        4 => 8,
        _ => 8,
    };

    encoder.set_property("preset", preset);
    encoder.set_property("target-bitrate", scale_bitrate_u32(AV1_SW_BITRATES_KBPS[idx], scale));

    if keyframe_interval > 0 {
        encoder.set_property("intra-period-length", keyframe_interval as i32);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// VP9 Encoders
// ═════════════════════════════════════════════════════════════════════════════

/// Intel QuickSync VP9 (qsvvp9enc)
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_qsv_vp9(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(VP9_BITRATES_KBPS[idx], scale));
}

/// VA-API VP9 (vavp9enc / vaapivp9enc) — Linux
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_vaapi_vp9(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(VP9_BITRATES_KBPS[idx], scale));
}

/// Software VP9 via libvpx (vp9enc)
///
/// Properties used:
/// - `deadline`: 1 = realtime (always)
/// - `cpu-used`: 0–8 (higher = faster; VP9 max is 8)
/// - `threads`: thread count
/// - `row-mt`: row-based multi-threading
/// - `target-bitrate`: bits per second
/// - `keyframe-max-dist`: keyframe interval
/// - `end-usage`: rate control (CBR for low latency)
/// - `buffer-size`, `buffer-initial-size`, `buffer-optimal-size`: latency
/// - `static-threshold`: skip encoding unchanged blocks
fn apply_software_vp9(encoder: &gst::Element, level: u8, keyframe_interval: u32, scale: f64) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4)
        .min(16);

    let idx = (level - 1) as usize;
    let (cpu_used, threads, static_threshold, row_mt) = match level {
        1 => (8i32, num_cpus.min(2), 200i32, false),
        2 => (8, num_cpus.min(4), 150, true),
        3 => (7, (num_cpus / 2).max(2), 100, true),
        4 => (6, num_cpus, 50, true),
        _ => (4, num_cpus, 0, true),
    };
    let bitrate_bps = (VP9_BITRATES_KBPS[idx] as i32) * 1000;

    encoder.set_property_from_str("deadline", "1");
    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
    encoder.set_property("row-mt", row_mt);
    encoder.set_property("target-bitrate", scale_bitrate_i32(bitrate_bps, scale));
    encoder.set_property_from_str("end-usage", "cbr");
    encoder.set_property("buffer-size", 500i32);
    encoder.set_property("buffer-initial-size", 300i32);
    encoder.set_property("buffer-optimal-size", 400i32);
    encoder.set_property("static-threshold", static_threshold);

    if keyframe_interval > 0 {
        encoder.set_property("keyframe-max-dist", keyframe_interval as i32);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// VP8 Encoders
// ═════════════════════════════════════════════════════════════════════════════

/// Intel QuickSync VP8 (qsvvp8enc)
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_qsv_vp8(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(VP8_BITRATES_KBPS[idx], scale));
}

/// VA-API VP8 (vavp8enc / vaapivp8enc) — Linux
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_vaapi_vp8(encoder: &gst::Element, level: u8, scale: f64) {
    let idx = (level - 1) as usize;
    encoder.set_property("bitrate", scale_bitrate_u32(VP8_BITRATES_KBPS[idx], scale));
}

/// Software VP8 via libvpx (vp8enc)
///
/// Properties used:
/// - `deadline`: 1 = realtime (always)
/// - `cpu-used`: 0–16 (higher = faster)
/// - `threads`: thread count (max 16 for libvpx)
/// - `target-bitrate`: bits per second
/// - `keyframe-max-dist`: keyframe interval
/// - `end-usage`: rate control (CBR for low latency)
/// - `buffer-size`, `buffer-initial-size`, `buffer-optimal-size`: latency
/// - `static-threshold`: skip encoding unchanged blocks
fn apply_software_vp8(encoder: &gst::Element, level: u8, keyframe_interval: u32, scale: f64) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4)
        .min(16);

    let idx = (level - 1) as usize;
    let (cpu_used, threads, static_threshold) = match level {
        1 => (16i32, num_cpus.min(2), 200i32),
        2 => (14, num_cpus.min(4), 150),
        3 => (12, (num_cpus / 2).max(2), 100),
        4 => (8, num_cpus, 50),
        _ => (4, num_cpus, 0),
    };
    let bitrate_bps = (VP8_BITRATES_KBPS[idx] as i32) * 1000;

    encoder.set_property_from_str("deadline", "1");
    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
    encoder.set_property("target-bitrate", scale_bitrate_i32(bitrate_bps, scale));
    encoder.set_property_from_str("end-usage", "cbr");
    encoder.set_property("buffer-size", 500i32);
    encoder.set_property("buffer-initial-size", 300i32);
    encoder.set_property("buffer-optimal-size", 400i32);
    encoder.set_property("static-threshold", static_threshold);

    if keyframe_interval > 0 {
        encoder.set_property("keyframe-max-dist", keyframe_interval as i32);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// FFV1 Encoder (avenc_ffv1)
// ═════════════════════════════════════════════════════════════════════════════

/// Software FFV1 via libav/ffmpeg (avenc_ffv1)
///
/// FFV1 is a lossless intra-frame codec. The slider controls compression
/// compute vs file size (all presets are lossless).
///
/// Properties used:
/// - `context`: 0 = small context model (fast), 1 = large context model (better compression)
/// - `coder`: 0 = Golomb-Rice (fast), 1 = Range coder (better compression)
/// - `slices`: more slices = more parallelism but slightly worse compression
/// - `slicecrc`: per-slice CRC for error detection
fn apply_software_ffv1(encoder: &gst::Element, level: u8) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4);

    let (context, coder_name, slices) = match level {
        1 => (0i32, "rice", (num_cpus * 4).min(24)),    // Fast: rice coder, many slices
        2 => (0, "rice", (num_cpus * 2).min(16)),         // Rice coder, more slices
        3 => (1, "ac", num_cpus.min(12)),                  // Large context, range coder
        4 => (1, "ac", (num_cpus / 2).max(4)),             // Fewer slices, better context
        _ => (1, "ac", 4),                                  // Best compression, fewer slices
    };

    encoder.set_property("context", context);
    encoder.set_property_from_str("coder", coder_name);
    encoder.set_property("slices", slices);
    encoder.set_property_from_str("slicecrc", "on");
}

// ═════════════════════════════════════════════════════════════════════════════
// H264 Encoders (platform-native only)
// ═════════════════════════════════════════════════════════════════════════════

/// Windows Media Foundation H264 (mfh264enc)
///
/// Most properties are "conditionally available" — their presence depends on
/// the underlying MFT (e.g., NVIDIA MF vs Intel MF vs software MF). We guard
/// each with `find_property()` to avoid panics on hardware that doesn't expose them.
///
/// Properties used (when available):
/// - `bitrate`: target bitrate in kbps (guint, always available)
/// - `quality-vs-speed`: 0 (quality) to 100 (speed) (guint)
/// - `low-latency`: enable low-latency mode (gboolean)
/// - `bframes`: number of B-frames (guint, 0 for low-latency; not all MFTs expose this)
/// - `ref`: number of reference frames (guint)
/// - `rc-mode`: rate control mode (enum, CBR for predictable output)
/// - `gop-size`: keyframe interval (gint)
fn apply_mf_h264(encoder: &gst::Element, level: u8, keyframe_interval: u32, scale: f64) {
    let idx = (level - 1) as usize;
    let (quality_vs_speed, low_latency, bframes, ref_frames) = match level {
        1 => (100u32, true, 0u32, 1u32),
        2 => (75, true, 0, 1),
        3 => (50, true, 0, 2),
        4 => (25, false, 2, 2),
        _ => (0, false, 3, 4),
    };

    // bitrate is always available
    encoder.set_property("bitrate", scale_bitrate_u32(H264_BITRATES_KBPS[idx], scale));

    // Conditionally-available properties — guard each to avoid panics
    // on MFTs that don't expose them (e.g., NVIDIA MF lacks bframes).
    // Use try_set_u32_clamped for u32 props since some MFTs accept the
    // property name but restrict the range (e.g., bframes max=2).
    try_set_u32_clamped(encoder, "quality-vs-speed", quality_vs_speed);
    if encoder.find_property("low-latency").is_some() {
        encoder.set_property("low-latency", low_latency);
    }
    try_set_u32_clamped(encoder, "bframes", bframes);
    try_set_u32_clamped(encoder, "ref", ref_frames);
    if encoder.find_property("rc-mode").is_some() {
        encoder.set_property_from_str("rc-mode", "cbr");
    }
    if keyframe_interval > 0 {
        if encoder.find_property("gop-size").is_some() {
            encoder.set_property("gop-size", keyframe_interval as i32);
        }
    }
}

/// Apple VideoToolbox H264 (vtenc_h264)
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps (guint, 0 = auto)
/// - `quality`: compression quality 0.0–1.0 (gdouble)
/// - `realtime`: enable realtime encoding (gboolean)
/// - `allow-frame-reordering`: enable B-frames (gboolean, levels 4–5 only)
/// - `max-keyframe-interval`: keyframe interval (gint, 0 = auto)
fn apply_vtb_h264(encoder: &gst::Element, level: u8, keyframe_interval: u32, scale: f64) {
    let idx = (level - 1) as usize;
    let (quality, realtime, allow_reorder) = match level {
        1 => (0.25f64, true, false),
        2 => (0.40, true, false),
        3 => (0.55, true, false),
        4 => (0.70, false, true),
        _ => (0.85, false, true),
    };

    encoder.set_property("bitrate", scale_bitrate_u32(H264_BITRATES_KBPS[idx], scale));
    encoder.set_property("quality", quality);
    encoder.set_property("realtime", realtime);
    encoder.set_property("allow-frame-reordering", allow_reorder);
    if keyframe_interval > 0 {
        encoder.set_property("max-keyframe-interval", keyframe_interval as i32);
    }
}
