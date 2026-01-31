// Video encoding and container format support
//
// This module defines supported video codecs and their container mappings.
// To add a new codec:
// 1. Add variant to VideoCodec enum
// 2. Add GStreamer caps name mapping in from_gst_caps_name()
// 3. Add container mapping in container()
// 4. Add file extension in container's extension()
// 5. Update recording pipeline in recording/video.rs

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
}

impl VideoCodec {
    /// All supported codecs (for iteration)
    pub const ALL: &'static [VideoCodec] = &[
        VideoCodec::Mjpeg,
        VideoCodec::H264,
        VideoCodec::H265,
        VideoCodec::Av1,
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
        }
    }
    
    /// Get the appropriate container format for this codec
    pub fn container(&self) -> ContainerFormat {
        match self {
            VideoCodec::Mjpeg => ContainerFormat::Mkv,
            VideoCodec::H264 => ContainerFormat::Mp4,
            VideoCodec::H265 => ContainerFormat::Mp4,
            VideoCodec::Av1 => ContainerFormat::WebM,
        }
    }
    
    /// Get the GStreamer parser element name for this codec
    pub fn gst_parser(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "jpegparse",
            VideoCodec::H264 => "h264parse",
            VideoCodec::H265 => "h265parse",
            VideoCodec::Av1 => "av1parse",
        }
    }
    
    /// Human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            VideoCodec::Mjpeg => "MJPEG",
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265",
            VideoCodec::Av1 => "AV1",
        }
    }
    
    /// Check if native HTML5 video player can handle this codec
    pub fn native_playback_supported(&self) -> bool {
        match self {
            VideoCodec::Mjpeg => false, // Needs custom frame player
            VideoCodec::H264 => true,
            VideoCodec::H265 => true,  // Most browsers support HEVC now
            VideoCodec::Av1 => true,
        }
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
