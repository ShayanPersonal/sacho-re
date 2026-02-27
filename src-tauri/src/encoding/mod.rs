// Video encoding and container format support
//
// This module defines supported video codecs and their container mappings.
// To add a new codec:
// 1. Add variant to VideoCodec enum
// 2. Add GStreamer caps name mapping in from_gst_caps_name()
// 3. Update recording pipeline in recording/video.rs

pub mod encoder;
pub mod presets;

pub use encoder::{
    AsyncVideoEncoder, EncoderConfig, EncoderError, EncoderStats,
    HardwareEncoderType, RawVideoFrame,
    detect_best_encoder, detect_best_encoder_for_codec, detect_best_av1_encoder, detect_best_vp8_encoder, detect_best_vp9_encoder,
    detect_best_h264_encoder, has_h264_encoder, has_hardware_h264_encoder,
    has_hardware_av1_encoder, has_hardware_vp9_encoder, has_hardware_vp8_encoder,
    has_av1_encoder, has_vp8_encoder, has_vp9_encoder,
    has_ffv1_encoder,
    get_recommended_codec,
    available_encoders_for_codec,
};
pub use presets::{DEFAULT_PRESET, MIN_PRESET, MAX_PRESET};

use serde::{Deserialize, Serialize};

/// Supported video codecs for recording
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    /// Motion JPEG - each frame is a complete JPEG image
    Mjpeg,
    /// AV1 - royalty-free, excellent compression
    Av1,
    /// VP8 - royalty-free, good compression, widely supported
    Vp8,
    /// VP9 - royalty-free, excellent compression, widely supported
    Vp9,
    /// Raw uncompressed video - requires encoding by the application
    Raw,
    /// FFV1 - lossless intra-frame compression (avenc_ffv1)
    Ffv1,
    /// H.264 - passthrough or platform-native encoding (Media Foundation / VideoToolbox)
    H264,
}

impl VideoCodec {
    /// All supported codecs (for iteration)
    pub const ALL: &'static [VideoCodec] = &[
        VideoCodec::Mjpeg,
        VideoCodec::Av1,
        VideoCodec::Vp8,
        VideoCodec::Vp9,
        VideoCodec::H264,
        VideoCodec::Raw,
    ];
    
    /// Try to parse codec from GStreamer caps structure name
    pub fn from_gst_caps_name(name: &str) -> Option<VideoCodec> {
        match name {
            // MJPEG variants
            "image/jpeg" => Some(VideoCodec::Mjpeg),
            
            // AV1 variants
            "video/x-av1" => Some(VideoCodec::Av1),
            "video/av1" => Some(VideoCodec::Av1),
            
            // VP8 variants
            "video/x-vp8" => Some(VideoCodec::Vp8),
            
            // VP9 variants
            "video/x-vp9" => Some(VideoCodec::Vp9),
            
            // Raw uncompressed video
            "video/x-raw" => Some(VideoCodec::Raw),

            // FFV1
            "video/x-ffv" => Some(VideoCodec::Ffv1),

            // H.264 - passthrough only
            "video/x-h264" => Some(VideoCodec::H264),

            _ => None,
        }
    }
    
    /// Get the GStreamer caps name for this codec
    pub fn gst_caps_name(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "image/jpeg",
            VideoCodec::Av1 => "video/x-av1",
            VideoCodec::Vp8 => "video/x-vp8",
            VideoCodec::Vp9 => "video/x-vp9",
            VideoCodec::Raw => "video/x-raw",
            VideoCodec::Ffv1 => "video/x-ffv",
            VideoCodec::H264 => "video/x-h264",
        }
    }
    
    /// Get the GStreamer parser element name for this codec
    /// 
    /// Note: This is only used by the VideoWriter (recording/video.rs) for muxing.
    /// The capture pipeline no longer uses a parser for any codec.
    /// For MJPEG playback, jpegparse is used directly in MjpegDemuxer (video/mjpeg.rs).
    pub fn gst_parser(&self) -> &'static str {
        match self {
            // MJPEG writer skips parser (line ~249 in video.rs) to avoid dimension issues
            VideoCodec::Mjpeg => "jpegparse",
            VideoCodec::Av1 => "av1parse",
            VideoCodec::Vp8 => "identity", // VP8 doesn't need parsing before muxing
            VideoCodec::Vp9 => "identity", // VP9 doesn't need parsing before muxing
            VideoCodec::Raw => "identity", // No parsing needed, use identity element
            VideoCodec::Ffv1 => "identity", // No parser needed
            VideoCodec::H264 => "h264parse", // NAL unit framing before muxing (gst-plugins-good, LGPL)
        }
    }
    
    /// Human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "MJPEG",
            VideoCodec::Av1 => "AV1",
            VideoCodec::Vp8 => "VP8",
            VideoCodec::Vp9 => "VP9",
            VideoCodec::Raw => "RAW",
            VideoCodec::Ffv1 => "FFV1",
            VideoCodec::H264 => "H.264",
        }
    }

    /// Check if native HTML5 video player can handle this codec
    pub fn native_playback_supported(&self) -> bool {
        match self {
            VideoCodec::Mjpeg => false, // Needs custom frame player
            VideoCodec::Av1 => true,
            VideoCodec::Vp8 => true,
            VideoCodec::Vp9 => true,
            VideoCodec::Raw => true, // Will be encoded, which is supported
            VideoCodec::Ffv1 => false, // Uses custom frame player (GstDecodeDemuxer), not HTML5 native
            VideoCodec::H264 => true, // WebView2 (Windows) and WKWebView (macOS) handle H264 natively
        }
    }
    
    /// Check if this is a pre-encoded codec (passthrough by default)
    pub fn is_preencoded(&self) -> bool {
        !matches!(self, VideoCodec::Raw)
    }

    /// Get the GStreamer decoder element for this codec (used when re-encoding).
    /// Returns None for Raw since it's already uncompressed.
    pub fn gst_decoder(&self) -> Option<&'static str> {
        match self {
            VideoCodec::Mjpeg => Some("jpegdec"),
            VideoCodec::Vp8 => Some("vp8dec"),
            VideoCodec::Vp9 => Some("vp9dec"),
            VideoCodec::Av1 => Some("av1dec"),
            VideoCodec::Ffv1 => Some("avdec_ffv1"),
            VideoCodec::Raw => None,
            VideoCodec::H264 => {
                // Platform-native decoders only (no bundled software decoder)
                #[cfg(target_os = "windows")]
                { Some("d3d11h264dec") }  // DXVA via Direct3D11 (system H.264 decoder)
                #[cfg(target_os = "macos")]
                { Some("vtdec") }         // Apple VideoToolbox
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                { None }                  // No H.264 decoder on Linux
            }
        }
    }
}

/// Supported container formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerFormat {
    /// Matroska (.mkv) - flexible, supports any codec
    Mkv,
    /// WebM (.webm) - VP8/VP9/AV1 in Matroska subset
    WebM,
    /// MPEG-4 Part 14 (.mp4) - widely compatible
    Mp4,
}

impl ContainerFormat {
    /// All supported container formats (for iteration)
    pub const ALL: &'static [ContainerFormat] = &[
        ContainerFormat::Mp4,
        ContainerFormat::Mkv,
        ContainerFormat::WebM,
    ];

    /// Get the file extension for this container
    pub fn extension(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "mkv",
            ContainerFormat::WebM => "webm",
            ContainerFormat::Mp4 => "mp4",
        }
    }

    /// Human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "MKV",
            ContainerFormat::WebM => "WebM",
            ContainerFormat::Mp4 => "MP4",
        }
    }

    /// Get the GStreamer muxer element name
    pub fn gst_muxer(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "matroskamux",
            ContainerFormat::WebM => "webmmux",
            ContainerFormat::Mp4 => "mp4mux",
        }
    }

    /// Get the GStreamer demuxer element name
    pub fn gst_demuxer(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "matroskademux",
            ContainerFormat::WebM => "matroskademux", // WebM is a subset of Matroska
            ContainerFormat::Mp4 => "qtdemux",
        }
    }

    /// Whether this container's muxer supports the "writing-app" property.
    /// matroskamux and webmmux support it; mp4mux does not.
    pub fn has_writing_app_property(&self) -> bool {
        match self {
            ContainerFormat::Mkv | ContainerFormat::WebM => true,
            ContainerFormat::Mp4 => false,
        }
    }

    /// Returns the default container for a given codec.
    pub fn default_container_for_codec(codec: VideoCodec) -> ContainerFormat {
        match codec {
            VideoCodec::Av1 => ContainerFormat::Mp4,
            VideoCodec::Vp9 => ContainerFormat::Mp4,
            VideoCodec::Vp8 => ContainerFormat::WebM,
            VideoCodec::H264 => ContainerFormat::Mp4,
            VideoCodec::Ffv1 => ContainerFormat::Mkv,
            VideoCodec::Mjpeg => ContainerFormat::Mkv,
            VideoCodec::Raw => ContainerFormat::Mkv,
        }
    }
}

/// Detect container format from file extension
pub fn codec_from_extension(ext: &str) -> Option<ContainerFormat> {
    match ext.to_lowercase().as_str() {
        "mkv" => Some(ContainerFormat::Mkv),
        "webm" => Some(ContainerFormat::WebM),
        "mp4" => Some(ContainerFormat::Mp4),
        _ => None,
    }
}

/// Known video file extensions
pub const VIDEO_EXTENSIONS: &[&str] = &[".mkv", ".webm", ".mp4"];

/// Check if a filename ends with a known video extension
pub fn is_video_extension(fname: &str) -> bool {
    VIDEO_EXTENSIONS.iter().any(|ext| fname.ends_with(ext))
}

/// Strip a video extension from a filename, returning the stem
pub fn strip_video_extension(fname: &str) -> &str {
    for ext in VIDEO_EXTENSIONS {
        if let Some(stem) = fname.strip_suffix(ext) {
            return stem;
        }
    }
    fname
}

/// Detect container format from a filename
pub fn container_from_filename(fname: &str) -> Option<ContainerFormat> {
    if fname.ends_with(".mkv") {
        Some(ContainerFormat::Mkv)
    } else if fname.ends_with(".webm") {
        Some(ContainerFormat::WebM)
    } else if fname.ends_with(".mp4") {
        Some(ContainerFormat::Mp4)
    } else {
        None
    }
}

/// Returns the optimal intermediate pixel format for the given encoding codec and bit depth.
/// - AV1: always P010_10LE — AV1 internally uses 10-bit, so feeding it 10-bit
///   avoids a lossy 8→10→8 round-trip. Upconverting 8-bit source is lossless.
/// - FFV1 with video_bit_depth=10: P010_10LE — user explicitly chose 10-bit lossless.
/// - Everything else: NV12 (8-bit 4:2:0).
pub fn intermediate_format_for_codec(codec: VideoCodec, video_bit_depth: Option<u8>) -> &'static str {
    match codec {
        VideoCodec::Av1 => "P010_10LE",
        VideoCodec::Ffv1 if video_bit_depth == Some(10) => "P010_10LE",
        _ => "NV12",
    }
}

/// Returns true if the GStreamer pixel format string represents 10-bit or higher.
pub fn is_10bit_format(format: &str) -> bool {
    format.contains("10")
}

// ============================================================================
// Format-string helpers (source format → GStreamer pipeline elements)
// ============================================================================

/// Returns true for raw pixel formats (YUY2, NV12, BGR, etc.).
/// Returns false for known pre-encoded formats (MJPEG, H264, AV1, VP8, VP9).
/// Unknown formats are assumed raw.
pub fn is_raw_format(format: &str) -> bool {
    !matches!(
        format,
        "MJPEG" | "H264" | "AV1" | "VP8" | "VP9"
    )
}

/// Build GStreamer caps media-type and optional format field from a format string.
///
/// Returns `(media_type, format_field)`:
/// - "MJPEG" → ("image/jpeg", None)
/// - "H264"  → ("video/x-h264", None)
/// - "AV1"   → ("video/x-av1", None)
/// - "VP8"   → ("video/x-vp8", None)
/// - "VP9"   → ("video/x-vp9", None)
/// - anything else → ("video/x-raw", Some(format))
pub fn format_to_gst_caps(format: &str) -> (&'static str, Option<&str>) {
    match format {
        "MJPEG" => ("image/jpeg", None),
        "H264"  => ("video/x-h264", None),
        "AV1"   => ("video/x-av1", None),
        "VP8"   => ("video/x-vp8", None),
        "VP9"   => ("video/x-vp9", None),
        _       => ("video/x-raw", Some(format)),
    }
}

/// Returns the GStreamer decoder element name for a pre-encoded format.
/// Raw pixel formats return None (no decoding needed).
pub fn decoder_for_format(format: &str) -> Option<&'static str> {
    match format {
        "MJPEG" => Some("jpegdec"),
        "VP8"   => Some("vp8dec"),
        "VP9"   => Some("vp9dec"),
        "AV1"   => Some("av1dec"),
        "H264"  => {
            #[cfg(target_os = "windows")]
            { Some("d3d11h264dec") }
            #[cfg(target_os = "macos")]
            { Some("vtdec") }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            { None }
        }
        _ => None, // Raw pixel formats
    }
}

/// Returns the GStreamer parser element name for a format.
/// Only H264 and AV1 need a real parser; everything else uses identity.
pub fn parser_for_format(format: &str) -> &'static str {
    match format {
        "H264"  => "h264parse",
        "AV1"   => "av1parse",
        _       => "identity",
    }
}

/// Whether HTML5 `<video>` can natively play content in this source format.
pub fn native_playback_for_format(format: &str) -> bool {
    match format {
        "MJPEG" => false,
        "AV1" | "VP8" | "VP9" | "H264" => true,
        _ => true, // Raw will be encoded to a supported codec
    }
}
