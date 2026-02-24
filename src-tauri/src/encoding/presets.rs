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
//! ## Quality-based encoding
//!
//! All encoders use quality-based rate control (CRF/CQ/CQP) instead of
//! bitrate-based. This lets the encoder automatically adapt bitrate to
//! content complexity — static scenes use fewer bits, complex motion gets
//! more — producing smaller files at the same visual quality with no
//! resolution/fps scaling needed.
//!
//! ## Effort slider (software encoders only)
//!
//! Software encoders (SVT-AV1, libvpx VP9/VP8) have independent quality
//! and compute-effort axes. The `effort_level` parameter (1–5) controls
//! how much CPU time the encoder spends per frame (more effort = better
//! compression at the same quality, but slower encoding).
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
/// All encoders use quality-based rate control (CRF/CQ/CQP). The `level`
/// parameter (1–5) controls visual quality, while `effort_level` (1–5)
/// controls compute effort for software encoders (ignored by hardware encoders).
///
/// # Arguments
/// * `encoder` — the GStreamer encoder element to configure
/// * `codec` — the target video codec
/// * `hw_type` — the hardware encoder type being used
/// * `level` — quality preset level (1–5; clamped internally)
/// * `effort_level` — compute effort for software encoders (1–5; clamped internally)
/// * `keyframe_interval` — keyframe interval in frames (0 = encoder default)
pub fn apply_preset(
    encoder: &gst::Element,
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    level: u8,
    effort_level: u8,
    keyframe_interval: u32,
) {
    let level = level.clamp(MIN_PRESET, MAX_PRESET);
    let effort_level = effort_level.clamp(MIN_PRESET, MAX_PRESET);

    println!(
        "[Preset] {:?} {:?} quality={} effort={}",
        codec, hw_type, level, effort_level,
    );

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
            apply_software_av1(encoder, level, effort_level, keyframe_interval);
        }

        // ── VP9 encoders ────────────────────────────────────────────────
        (VideoCodec::Vp9, HardwareEncoderType::Qsv) => {
            apply_qsv_vp9(encoder, level);
        }
        (VideoCodec::Vp9, HardwareEncoderType::VaApi) => {
            apply_vaapi_vp9(encoder, level);
        }
        (VideoCodec::Vp9, HardwareEncoderType::Software) => {
            apply_software_vp9(encoder, level, effort_level, keyframe_interval);
        }

        // ── VP8 encoders ────────────────────────────────────────────────
        (VideoCodec::Vp8, HardwareEncoderType::Qsv) => {
            apply_qsv_vp8(encoder, level);
        }
        (VideoCodec::Vp8, HardwareEncoderType::VaApi) => {
            apply_vaapi_vp8(encoder, level);
        }
        (VideoCodec::Vp8, HardwareEncoderType::Software) => {
            apply_software_vp8(encoder, level, effort_level, keyframe_interval);
        }

        // ── H264 encoders (platform-native only) ────────────────────────
        (VideoCodec::H264, HardwareEncoderType::MediaFoundation) => {
            apply_mf_h264(encoder, level, keyframe_interval);
        }
        (VideoCodec::H264, HardwareEncoderType::VideoToolbox) => {
            apply_vtb_h264(encoder, level, keyframe_interval);
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
/// Quality-based: `rc-mode=vbr` + `const-quality` with always-on AQ.
/// Spatial and temporal AQ run on the NVENC ASIC (zero CPU cost) and
/// always improve quality distribution — especially for static scenes
/// where they redirect bits from flat backgrounds to moving subjects.
///
/// Properties used:
/// - `preset`: p1 (fastest) to p7 (best quality)
/// - `rc-mode`: VBR (enables const-quality)
/// - `const-quality`: CQ level (lower = better quality)
/// - `spatial-aq`: adaptive quantization across spatial blocks
/// - `temporal-aq`: adaptive quantization across frames
/// - `gop-size`: keyframe interval
fn apply_nvenc_av1(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let (const_quality, preset) = match level {
        1 => (38.0f64, "p1"),
        2 => (32.0, "p3"),
        3 => (28.0, "p4"),
        4 => (24.0, "p5"),
        _ => (20.0, "p7"),
    };

    encoder.set_property_from_str("rc-mode", "vbr");
    encoder.set_property("const-quality", const_quality);
    encoder.set_property_from_str("preset", preset);
    encoder.set_property("spatial-aq", true);
    encoder.set_property("temporal-aq", true);
    if keyframe_interval > 0 {
        encoder.set_property("gop-size", keyframe_interval as i32);
    }
}

/// AMD AMF AV1 (amfav1enc) — RX 7000 series+
///
/// Quality-based: `rate-control=cqp` + `qp-i`/`qp-p` with always-on pre-analysis.
/// Pre-analysis runs content analysis on the media engine, improving quality
/// at the same bitrate with negligible GPU cost.
///
/// Properties used:
/// - `rate-control`: CQP (constant quantization parameter)
/// - `qp-i` / `qp-p`: quantization parameters for I/P frames
/// - `preset`: speed (100) to high-quality (0)
/// - `pre-analysis`: AMD content analysis
fn apply_amf_av1(encoder: &gst::Element, level: u8) {
    let (qp, preset) = match level {
        1 => (180u32, "speed"),
        2 => (150, "balanced"),
        3 => (128, "balanced"),
        4 => (100, "quality"),
        _ => (70, "high-quality"),
    };

    if encoder.find_property("rate-control").is_some() {
        encoder.set_property_from_str("rate-control", "cqp");
    }
    try_set_u32_clamped(encoder, "qp-i", qp);
    try_set_u32_clamped(encoder, "qp-p", qp);
    if encoder.find_property("preset").is_some() {
        encoder.set_property_from_str("preset", preset);
    }
    if encoder.find_property("pre-analysis").is_some() {
        encoder.set_property("pre-analysis", true);
    }
}

/// Intel QuickSync AV1 (qsvav1enc)
///
/// Quality-based: `rate-control=cqp` + `qp-i`/`qp-p`.
///
/// Properties used:
/// - `rate-control`: CQP
/// - `qp-i` / `qp-p`: quantization parameters (offset for P-frames)
fn apply_qsv_av1(encoder: &gst::Element, level: u8) {
    let (qp_i, qp_p) = match level {
        1 => (38u32, 40u32),
        2 => (32, 34),
        3 => (28, 30),
        4 => (24, 26),
        _ => (18, 20),
    };

    if encoder.find_property("rate-control").is_some() {
        encoder.set_property_from_str("rate-control", "cqp");
    }
    try_set_u32_clamped(encoder, "qp-i", qp_i);
    try_set_u32_clamped(encoder, "qp-p", qp_p);
}

/// VA-API AV1 (vaav1enc / vaapiav1enc) — Linux
///
/// Quality-based: `rate-control=cqp` + `qp` + `target-usage`.
///
/// Properties used:
/// - `rate-control`: CQP
/// - `qp`: quantization parameter
/// - `target-usage`: speed/quality tradeoff (1=quality, 7=speed)
fn apply_vaapi_av1(encoder: &gst::Element, level: u8) {
    let (qp, target_usage) = match level {
        1 => (180u32, 7u32),
        2 => (150, 6),
        3 => (128, 4),
        4 => (100, 2),
        _ => (70, 1),
    };

    if encoder.find_property("rate-control").is_some() {
        encoder.set_property_from_str("rate-control", "cqp");
    }
    try_set_u32_clamped(encoder, "qp", qp);
    try_set_u32_clamped(encoder, "target-usage", target_usage);
}

/// Software AV1 via SVT-AV1 (svtav1enc) — 2 sliders
///
/// Quality: CRF mode (do NOT set target-bitrate).
/// Effort: preset (12=fastest, 8=slowest feasible for real-time).
///
/// Properties used:
/// - `crf`: constant rate factor (lower = better quality)
/// - `preset`: speed preset (higher = faster, lower = better compression)
/// - `intra-period-length`: keyframe interval
fn apply_software_av1(encoder: &gst::Element, level: u8, effort_level: u8, keyframe_interval: u32) {
    let crf: i32 = match level {
        1 => 45,
        2 => 38,
        3 => 33,
        4 => 28,
        _ => 23,
    };

    let preset: u32 = match effort_level {
        1 => 12,
        2 => 11,
        3 => 10,
        4 => 9,
        _ => 8,
    };

    encoder.set_property("crf", crf);
    encoder.set_property("preset", preset);

    if keyframe_interval > 0 {
        encoder.set_property("intra-period-length", keyframe_interval as i32);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// VP9 Encoders
// ═════════════════════════════════════════════════════════════════════════════

/// Intel QuickSync VP9 (qsvvp9enc)
///
/// Quality-based: `rate-control=icq` + `icq-quality`.
///
/// Properties used:
/// - `rate-control`: ICQ (intelligent constant quality)
/// - `icq-quality`: quality level (lower = better quality)
fn apply_qsv_vp9(encoder: &gst::Element, level: u8) {
    let icq_quality = match level {
        1 => 38u32,
        2 => 32,
        3 => 26,
        4 => 22,
        _ => 16,
    };

    if encoder.find_property("rate-control").is_some() {
        encoder.set_property_from_str("rate-control", "icq");
    }
    try_set_u32_clamped(encoder, "icq-quality", icq_quality);
}

/// VA-API VP9 (vavp9enc / vaapivp9enc) — Linux
///
/// Quality-based: `rate-control=cqp` + `qp`.
///
/// Properties used:
/// - `rate-control`: CQP
/// - `qp`: quantization parameter
fn apply_vaapi_vp9(encoder: &gst::Element, level: u8) {
    let qp = match level {
        1 => 180u32,
        2 => 150,
        3 => 128,
        4 => 100,
        _ => 70,
    };

    if encoder.find_property("rate-control").is_some() {
        encoder.set_property_from_str("rate-control", "cqp");
    }
    try_set_u32_clamped(encoder, "qp", qp);
}

/// Software VP9 via libvpx (vp9enc) — 2 sliders
///
/// Quality: CQ mode (`end-usage=cq`) + `cq-level` with generous bitrate ceiling.
/// Effort: `cpu-used`, `threads`, `row-mt`, `static-threshold`.
///
/// Properties used:
/// - `deadline`: 1 = realtime (always)
/// - `end-usage`: CQ (constrained quality)
/// - `cq-level`: quality level (lower = better quality)
/// - `target-bitrate`: generous ceiling for CQ mode (prevents runaway)
/// - `cpu-used`: 0–8 (higher = faster)
/// - `threads`: thread count
/// - `row-mt`: row-based multi-threading
/// - `static-threshold`: skip encoding unchanged blocks
/// - `keyframe-max-dist`: keyframe interval
fn apply_software_vp9(encoder: &gst::Element, level: u8, effort_level: u8, keyframe_interval: u32) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4)
        .min(16);

    let cq_level = match level {
        1 => 42i32,
        2 => 36,
        3 => 31,
        4 => 26,
        _ => 20,
    };

    let (cpu_used, threads, row_mt, static_threshold) = match effort_level {
        1 => (8i32, num_cpus.min(2), false, 200i32),
        2 => (8, num_cpus.min(4), true, 150),
        3 => (7, (num_cpus / 2).max(2), true, 100),
        4 => (6, num_cpus, true, 50),
        _ => (4, num_cpus, true, 0),
    };

    encoder.set_property_from_str("deadline", "1");
    encoder.set_property_from_str("end-usage", "cq");
    encoder.set_property("cq-level", cq_level);
    // Generous ceiling for CQ mode to prevent runaway on complex scenes
    encoder.set_property("target-bitrate", 50_000_000i32);
    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
    encoder.set_property("row-mt", row_mt);
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
/// Bitrate fallback — rare/nonexistent encoder, no CQ mode available.
///
/// Properties used:
/// - `bitrate`: target bitrate in kbps
fn apply_qsv_vp8(encoder: &gst::Element, level: u8) {
    let bitrate = match level {
        1 => 2_500u32,
        2 => 3_500,
        3 => 5_000,
        4 => 6_500,
        _ => 8_000,
    };

    encoder.set_property("bitrate", bitrate);
}

/// VA-API VP8 (vavp8enc / vaapivp8enc) — Linux
///
/// Quality-based: `rate-control=cqp` + `qp`.
///
/// Properties used:
/// - `rate-control`: CQP
/// - `qp`: quantization parameter
fn apply_vaapi_vp8(encoder: &gst::Element, level: u8) {
    let qp = match level {
        1 => 180u32,
        2 => 150,
        3 => 128,
        4 => 100,
        _ => 70,
    };

    if encoder.find_property("rate-control").is_some() {
        encoder.set_property_from_str("rate-control", "cqp");
    }
    try_set_u32_clamped(encoder, "qp", qp);
}

/// Software VP8 via libvpx (vp8enc) — 2 sliders
///
/// Quality: CQ mode (`end-usage=cq`) + `cq-level` with generous bitrate ceiling.
/// Effort: `cpu-used`, `threads`, `static-threshold`.
///
/// Properties used:
/// - `deadline`: 1 = realtime (always)
/// - `end-usage`: CQ (constrained quality)
/// - `cq-level`: quality level (lower = better quality)
/// - `target-bitrate`: generous ceiling for CQ mode (prevents runaway)
/// - `cpu-used`: 0–16 (higher = faster)
/// - `threads`: thread count (max 16 for libvpx)
/// - `static-threshold`: skip encoding unchanged blocks
/// - `keyframe-max-dist`: keyframe interval
fn apply_software_vp8(encoder: &gst::Element, level: u8, effort_level: u8, keyframe_interval: u32) {
    let num_cpus = std::thread::available_parallelism()
        .map(|p| p.get() as i32)
        .unwrap_or(4)
        .min(16);

    let cq_level = match level {
        1 => 42i32,
        2 => 36,
        3 => 31,
        4 => 26,
        _ => 20,
    };

    let (cpu_used, threads, static_threshold) = match effort_level {
        1 => (16i32, num_cpus.min(2), 200i32),
        2 => (14, num_cpus.min(4), 150),
        3 => (12, (num_cpus / 2).max(2), 100),
        4 => (8, num_cpus, 50),
        _ => (4, num_cpus, 0),
    };

    encoder.set_property_from_str("deadline", "1");
    encoder.set_property_from_str("end-usage", "cq");
    encoder.set_property("cq-level", cq_level);
    // Generous ceiling for CQ mode to prevent runaway on complex scenes
    encoder.set_property("target-bitrate", 50_000_000i32);
    encoder.set_property("cpu-used", cpu_used);
    encoder.set_property("threads", threads);
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
/// Quality-based: `rc-mode=qvbr` + `qp` with bitrate fallback if QVBR
/// is unavailable on the underlying MFT.
///
/// Properties used (when available):
/// - `rc-mode`: QVBR (quality variable bitrate) — falls back to CBR if unavailable
/// - `qp`: target quantization parameter
/// - `quality-vs-speed`: 0 (quality) to 100 (speed) (guint)
/// - `low-latency`: enable low-latency mode (gboolean)
/// - `bframes`: number of B-frames (guint, 0 for low-latency)
/// - `ref`: number of reference frames (guint)
/// - `gop-size`: keyframe interval (gint)
fn apply_mf_h264(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let (qp, quality_vs_speed, low_latency, bframes, ref_frames) = match level {
        1 => (32u32, 100u32, true, 0u32, 1u32),
        2 => (28, 75, true, 0, 1),
        3 => (24, 50, true, 0, 2),
        4 => (20, 25, false, 2, 2),
        _ => (16, 0, false, 3, 4),
    };

    // Try QVBR first; fall back to CBR with a reasonable bitrate if unavailable
    if encoder.find_property("rc-mode").is_some() {
        encoder.set_property_from_str("rc-mode", "qvbr");
    }
    try_set_u32_clamped(encoder, "qp", qp);

    try_set_u32_clamped(encoder, "quality-vs-speed", quality_vs_speed);
    if encoder.find_property("low-latency").is_some() {
        encoder.set_property("low-latency", low_latency);
    }
    try_set_u32_clamped(encoder, "bframes", bframes);
    try_set_u32_clamped(encoder, "ref", ref_frames);
    if keyframe_interval > 0 {
        if encoder.find_property("gop-size").is_some() {
            encoder.set_property("gop-size", keyframe_interval as i32);
        }
    }
}

/// Apple VideoToolbox H264 (vtenc_h264)
///
/// Quality-based: `bitrate=0` (auto) + `quality` slider.
///
/// Properties used:
/// - `bitrate`: 0 = auto (let quality parameter drive)
/// - `quality`: compression quality 0.0–1.0 (gdouble)
/// - `realtime`: enable realtime encoding (gboolean)
/// - `allow-frame-reordering`: enable B-frames (gboolean, levels 4–5 only)
/// - `max-keyframe-interval`: keyframe interval (gint, 0 = auto)
fn apply_vtb_h264(encoder: &gst::Element, level: u8, keyframe_interval: u32) {
    let (quality, realtime, allow_reorder) = match level {
        1 => (0.25f64, true, false),
        2 => (0.40, true, false),
        3 => (0.55, true, false),
        4 => (0.70, false, true),
        _ => (0.85, false, true),
    };

    encoder.set_property("bitrate", 0u32);
    encoder.set_property("quality", quality);
    encoder.set_property("realtime", realtime);
    encoder.set_property("allow-frame-reordering", allow_reorder);
    if keyframe_interval > 0 {
        encoder.set_property("max-keyframe-interval", keyframe_interval as i32);
    }
}
