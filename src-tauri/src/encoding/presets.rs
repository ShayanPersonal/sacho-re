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
//! The auto-select system ([`crate::commands::auto_select_encoder_preset`])
//! will automatically test your new presets at runtime.

use gstreamer as gst;
use gstreamer::prelude::*;

use super::encoder::HardwareEncoderType;
use super::VideoCodec;

/// Minimum preset level (lightest computational load)
pub const MIN_PRESET: u8 = 1;
/// Maximum preset level (highest quality, most intensive)
pub const MAX_PRESET: u8 = 5;
/// Default preset level (balanced)
pub const DEFAULT_PRESET: u8 = 3;

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

/// Apply encoder-specific parameters for the given preset level.
///
/// This is the **main extension point** for the preset system. When adding a
/// new encoder, add a match arm here that dispatches to your preset function.
///
/// # Arguments
/// * `encoder` — the GStreamer encoder element to configure
/// * `codec` — the target video codec
/// * `hw_type` — the hardware encoder type being used
/// * `level` — preset level (1–5; clamped internally)
/// * `keyframe_interval` — keyframe interval in frames (0 = encoder default)
pub fn apply_preset(
    encoder: &gst::Element,
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    level: u8,
    keyframe_interval: u32,
) {
    let level = level.clamp(MIN_PRESET, MAX_PRESET);

    match (codec, hw_type) {
        // ── AV1 encoders ────────────────────────────────────────────────
        (VideoCodec::Av1, HardwareEncoderType::Nvenc) => {
            apply_nvenc_av1(encoder, level, keyframe_interval);
        }
        (VideoCodec::Av1, HardwareEncoderType::Amf) => {
            apply_amf_av1(encoder, level);
        }
        (VideoCodec::Av1, HardwareEncoderType::Qsv) => {
            apply_qsv_av1(encoder, level);
        }
        (VideoCodec::Av1, HardwareEncoderType::VaApi) => {
            apply_vaapi_av1(encoder, level);
        }
        (VideoCodec::Av1, HardwareEncoderType::Software) => {
            apply_software_av1(encoder, level, keyframe_interval);
        }

        // ── VP9 encoders ────────────────────────────────────────────────
        (VideoCodec::Vp9, HardwareEncoderType::Qsv) => {
            apply_qsv_vp9(encoder, level);
        }
        (VideoCodec::Vp9, HardwareEncoderType::VaApi) => {
            apply_vaapi_vp9(encoder, level);
        }
        (VideoCodec::Vp9, HardwareEncoderType::Software) => {
            apply_software_vp9(encoder, level, keyframe_interval);
        }

        // ── VP8 encoders ────────────────────────────────────────────────
        (VideoCodec::Vp8, HardwareEncoderType::Qsv) => {
            apply_qsv_vp8(encoder, level);
        }
        (VideoCodec::Vp8, HardwareEncoderType::VaApi) => {
            apply_vaapi_vp8(encoder, level);
        }
        (VideoCodec::Vp8, HardwareEncoderType::Software) => {
            apply_software_vp8(encoder, level, keyframe_interval);
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
fn apply_nvenc_av1(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let (preset, bitrate_kbps) = match level {
        1 => ("p1", 2_000u32),
        2 => ("p2", 3_000),
        3 => ("p4", 4_000),
        4 => ("p5", 5_000),
        _ => ("p7", 6_000),
    };

    encoder.set_property_from_str("preset", preset);
    encoder.set_property("bitrate", bitrate_kbps);
    if keyframe_interval > 0 {
        encoder.set_property("gop-size", keyframe_interval as i32);
    }
}

/// AMD AMF AV1 (amfav1enc) — RX 7000 series+
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_amf_av1(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 2_000u32,
        2 => 3_000,
        3 => 4_000,
        4 => 5_000,
        _ => 6_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
}

/// Intel QuickSync AV1 (qsvav1enc)
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_qsv_av1(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 2_000u32,
        2 => 3_000,
        3 => 4_000,
        4 => 5_000,
        _ => 6_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
}

/// VA-API AV1 (vaav1enc / vaapiav1enc) — Linux
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_vaapi_av1(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 2_000u32,
        2 => 3_000,
        3 => 4_000,
        4 => 5_000,
        _ => 6_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
}

/// Software AV1 via libaom (av1enc)
///
/// Properties used:
/// - `cpu-used`: 0 (slowest) to 10 (fastest); 6+ needed for real-time
/// - `threads`: thread count for parallel encoding
/// - `row-mt`: row-based multi-threading
/// - `target-bitrate`: kbps
/// - `keyframe-max-dist`: keyframe interval
/// - `end-usage`: rate control mode (always CBR for low latency)
fn apply_software_av1(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as u32)
        .unwrap_or(4);

    let (cpu_used, threads, bitrate_kbps) = match level {
        1 => (10u32, num_cpus.min(2), 1_500u32),
        2 => (10, num_cpus.min(4), 2_000),
        3 => (9, (num_cpus / 2).max(2), 3_000),
        4 => (8, num_cpus, 4_000),
        _ => (6, num_cpus, 5_000),
    };

    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
    encoder.set_property("row-mt", true);
    encoder.set_property("target-bitrate", bitrate_kbps);
    encoder.set_property_from_str("end-usage", "cbr");

    if keyframe_interval > 0 {
        encoder.set_property("keyframe-max-dist", keyframe_interval);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// VP9 Encoders
// ═════════════════════════════════════════════════════════════════════════════

/// Intel QuickSync VP9 (qsvvp9enc)
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_qsv_vp9(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 1_500u32,
        2 => 2_000,
        3 => 3_000,
        4 => 4_000,
        _ => 5_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
}

/// VA-API VP9 (vavp9enc / vaapivp9enc) — Linux
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_vaapi_vp9(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 1_500u32,
        2 => 2_000,
        3 => 3_000,
        4 => 4_000,
        _ => 5_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
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
fn apply_software_vp9(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4)
        .min(16);

    let (cpu_used, threads, bitrate_bps, static_threshold, row_mt) = match level {
        1 => (8i32, num_cpus.min(2), 1_500_000i32, 200i32, false),
        2 => (8, num_cpus.min(4), 2_000_000, 150, true),
        3 => (7, (num_cpus / 2).max(2), 3_000_000, 100, true),
        4 => (6, num_cpus, 4_000_000, 50, true),
        _ => (4, num_cpus, 5_000_000, 0, true),
    };

    encoder.set_property_from_str("deadline", "1");
    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
    encoder.set_property("row-mt", row_mt);
    encoder.set_property("target-bitrate", bitrate_bps);
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
fn apply_qsv_vp8(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 2_000u32,
        2 => 3_000,
        3 => 4_000,
        4 => 5_000,
        _ => 6_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
}

/// VA-API VP8 (vavp8enc / vaapivp8enc) — Linux
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_vaapi_vp8(encoder: &gst::Element, level: u8) {
    let bitrate_kbps = match level {
        1 => 2_000u32,
        2 => 3_000,
        3 => 4_000,
        4 => 5_000,
        _ => 6_000,
    };

    encoder.set_property("bitrate", bitrate_kbps);
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
fn apply_software_vp8(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4)
        .min(16);

    let (cpu_used, threads, bitrate_bps, static_threshold) = match level {
        1 => (16i32, num_cpus.min(2), 2_000_000i32, 200i32),
        2 => (14, num_cpus.min(4), 3_000_000, 150),
        3 => (12, (num_cpus / 2).max(2), 4_000_000, 100),
        4 => (8, num_cpus, 5_000_000, 50),
        _ => (4, num_cpus, 6_000_000, 0),
    };

    encoder.set_property_from_str("deadline", "1");
    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
    encoder.set_property("target-bitrate", bitrate_bps);
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
