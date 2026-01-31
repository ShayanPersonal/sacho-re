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
    HardwareEncoderType, RawVideoFrame, detect_best_encoder, has_hardware_encoder,
};

use serde::{Deserialize, Serialize};

/// Supported video codecs for recording
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    /// Motion JPEG - each frame is a complete JPEG image
    Mjpeg,
    /// H.264/AVC - widely supported, good compression
    H264,
    /// H.265/HEVC - better compression than H.264
    H265,
    /// AV1 - royalty-free, excellent compression
    Av1,
    /// Raw uncompressed video - requires encoding by the application
    Raw,
}

impl VideoCodec {
    /// All supported codecs (for iteration)
    pub const ALL: &'static [VideoCodec] = &[
        VideoCodec::Mjpeg,
        VideoCodec::H264,
        VideoCodec::H265,
        VideoCodec::Av1,
        VideoCodec::Raw,
    ];
    
    /// Try to parse codec from GStreamer caps structure name
    pub fn from_gst_caps_name(name: &str) -> Option<VideoCodec> {
        match name {
            // MJPEG variants
            "image/jpeg" => Some(VideoCodec::Mjpeg),
            
            // H.264/AVC variants
            "video/x-h264" => Some(VideoCodec::H264),
            "video/h264" => Some(VideoCodec::H264),
            
            // H.265/HEVC variants  
            "video/x-h265" => Some(VideoCodec::H265),
            "video/x-hevc" => Some(VideoCodec::H265),
            "video/h265" => Some(VideoCodec::H265),
            
            // AV1 variants
            "video/x-av1" => Some(VideoCodec::Av1),
            "video/av1" => Some(VideoCodec::Av1),
            
            // Raw uncompressed video
            "video/x-raw" => Some(VideoCodec::Raw),
            
            _ => None,
        }
    }
    
    /// Get the GStreamer caps name for this codec
    pub fn gst_caps_name(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "image/jpeg",
            VideoCodec::H264 => "video/x-h264",
            VideoCodec::H265 => "video/x-h265",
            VideoCodec::Av1 => "video/x-av1",
            VideoCodec::Raw => "video/x-raw",
        }
    }
    
    /// Get the appropriate container format for this codec
    /// Note: Raw codec will be encoded before muxing, so this returns the target container
    pub fn container(&self) -> ContainerFormat {
        match self {
            VideoCodec::Mjpeg => ContainerFormat::Mkv,
            VideoCodec::H264 => ContainerFormat::Mp4,
            VideoCodec::H265 => ContainerFormat::Mp4,
            VideoCodec::Av1 => ContainerFormat::WebM,
            VideoCodec::Raw => ContainerFormat::WebM, // Will be encoded to AV1 -> WebM
        }
    }
    
    /// Get the GStreamer parser element name for this codec
    /// Returns None for Raw since it doesn't have a parser
    pub fn gst_parser(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "jpegparse",
            VideoCodec::H264 => "h264parse",
            VideoCodec::H265 => "h265parse",
            VideoCodec::Av1 => "av1parse",
            VideoCodec::Raw => "identity", // No parsing needed, use identity element
        }
    }
    
    /// Human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "MJPEG",
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265",
            VideoCodec::Av1 => "AV1",
            VideoCodec::Raw => "RAW",
        }
    }
    
    /// Check if native HTML5 video player can handle this codec
    pub fn native_playback_supported(&self) -> bool {
        match self {
            VideoCodec::Mjpeg => false, // Needs custom frame player
            VideoCodec::H264 => true,
            VideoCodec::H265 => true,  // Most browsers support HEVC now
            VideoCodec::Av1 => true,
            VideoCodec::Raw => true, // Will be encoded to AV1, which is supported
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
    /// MP4 (.mp4) - widely compatible, good for H.264/H.265
    Mp4,
    /// WebM (.webm) - web-optimized, good for VP9/AV1
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
