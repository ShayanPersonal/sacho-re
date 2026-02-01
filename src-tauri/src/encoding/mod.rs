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

pub use encoder::{
    AsyncVideoEncoder, EncoderConfig, EncoderError, EncoderStats,
    HardwareEncoderType, RawVideoFrame, 
    detect_best_encoder, detect_best_encoder_for_codec, detect_best_av1_encoder, detect_best_vp8_encoder, detect_best_vp9_encoder,
    has_hardware_encoder, has_hardware_av1_encoder, has_hardware_vp9_encoder, has_hardware_vp8_encoder,
    has_av1_encoder, has_vp8_encoder, has_vp9_encoder,
    get_recommended_encoding_mode,
};

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
        }
    }
    
    /// Get the appropriate container format for this codec
    /// Note: Raw codec will be encoded before muxing, so this returns the target container
    pub fn container(&self) -> ContainerFormat {
        match self {
            VideoCodec::Mjpeg => ContainerFormat::Mkv,
            VideoCodec::Av1 => ContainerFormat::WebM,
            VideoCodec::Vp8 => ContainerFormat::WebM,
            VideoCodec::Vp9 => ContainerFormat::WebM,
            VideoCodec::Raw => ContainerFormat::WebM,
        }
    }
    
    /// Get the GStreamer parser element name for this codec
    /// Returns None for Raw since it doesn't have a parser
    pub fn gst_parser(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "jpegparse",
            VideoCodec::Av1 => "av1parse",
            VideoCodec::Vp8 => "identity", // VP8 doesn't need parsing before muxing
            VideoCodec::Vp9 => "identity", // VP9 doesn't need parsing before muxing
            VideoCodec::Raw => "identity", // No parsing needed, use identity element
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
    /// MP4 (.mp4) - widely compatible
    Mp4,
    /// WebM (.webm) - web-optimized
    WebM,
}

impl ContainerFormat {
    /// Get the file extension for this container
    pub fn extension(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "mkv",
            ContainerFormat::Mp4 => "mp4",
            ContainerFormat::WebM => "webm",
        }
    }
    
    /// Get the GStreamer muxer element name
    pub fn gst_muxer(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "matroskamux",
            ContainerFormat::Mp4 => "mp4mux",
            ContainerFormat::WebM => "webmmux",
        }
    }
    
    /// Get the GStreamer demuxer element name
    pub fn gst_demuxer(&self) -> &'static str {
        match self {
            ContainerFormat::Mkv => "matroskademux",
            ContainerFormat::Mp4 => "qtdemux",
            ContainerFormat::WebM => "matroskademux", // WebM uses matroska demuxer
        }
    }
}

/// Detect codec from file extension
pub fn codec_from_extension(ext: &str) -> Option<ContainerFormat> {
    match ext.to_lowercase().as_str() {
        "mkv" => Some(ContainerFormat::Mkv),
        "mp4" | "m4v" => Some(ContainerFormat::Mp4),
        "webm" => Some(ContainerFormat::WebM),
        _ => None,
    }
}
