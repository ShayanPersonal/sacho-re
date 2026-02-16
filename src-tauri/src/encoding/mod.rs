// Video encoding and container format support
//
// This module defines supported video codecs and their container mappings.
// To add a new codec:
// 1. Add variant to VideoCodec enum
// 2. Add GStreamer caps name mapping in from_gst_caps_name()
// 3. Add container mapping in container()
// 4. Add file extension in container's extension()
// 5. Update recording pipeline in recording/video.rs

pub mod encoder;
pub mod presets;

pub use encoder::{
    AsyncVideoEncoder, EncoderConfig, EncoderError, EncoderStats,
    HardwareEncoderType, RawVideoFrame,
    detect_best_encoder, detect_best_encoder_for_codec, detect_best_av1_encoder, detect_best_vp8_encoder, detect_best_vp9_encoder,
    has_hardware_av1_encoder, has_hardware_vp9_encoder, has_hardware_vp8_encoder,
    has_av1_encoder, has_vp8_encoder, has_vp9_encoder,
    has_ffv1_encoder,
    get_recommended_encoding_mode,
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
}

impl VideoCodec {
    /// All supported codecs (for iteration)
    pub const ALL: &'static [VideoCodec] = &[
        VideoCodec::Mjpeg,
        VideoCodec::Av1,
        VideoCodec::Vp8,
        VideoCodec::Vp9,
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
        }
    }
    
    /// Get the appropriate container format for this codec
    /// Note: Raw codec will be encoded before muxing, so this returns the target container
    pub fn container(&self) -> ContainerFormat {
        match self {
            VideoCodec::Mjpeg => ContainerFormat::Mkv,
            VideoCodec::Av1 => ContainerFormat::Mkv,
            VideoCodec::Vp8 => ContainerFormat::Mkv,
            VideoCodec::Vp9 => ContainerFormat::Mkv,
            VideoCodec::Raw => ContainerFormat::Mkv,
            VideoCodec::Ffv1 => ContainerFormat::Mkv,
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
        }
    }
    
    /// Check if this codec requires encoding by the application
    pub fn requires_encoding(&self) -> bool {
        matches!(self, VideoCodec::Raw)
    }
    
    /// Check if this is a pre-encoded codec (passthrough)
    pub fn is_preencoded(&self) -> bool {
        !self.requires_encoding()
    }
}

/// Supported container formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerFormat {
    /// Matroska (.mkv) - flexible, supports any codec
    Mkv,
}

impl ContainerFormat {
    /// Get the file extension for this container
    pub fn extension(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "mkv",
        }
    }
    
    /// Get the GStreamer muxer element name
    pub fn gst_muxer(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "matroskamux",
        }
    }
    
    /// Get the GStreamer demuxer element name
    pub fn gst_demuxer(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "matroskademux",
        }
    }
}

/// Detect codec from file extension
pub fn codec_from_extension(ext: &str) -> Option<ContainerFormat> {
    match ext.to_lowercase().as_str() {
        "mkv" => Some(ContainerFormat::Mkv),
        _ => None,
    }
}
