// Video file handling module
// 
// This module provides video file operations like demuxing and frame extraction.
// It's designed to be extensible for different container formats and codecs.
//
// Note: For VP8, VP9, and AV1, the native HTML5 video player handles playback.
// This module is primarily used for MJPEG frame extraction for the custom player.

pub mod demux;
pub mod gst_decode;
pub mod mjpeg;

pub use demux::{VideoDemuxer, VideoFrame, VideoInfo};
pub use gst_decode::GstDecodeDemuxer;
pub use mjpeg::MjpegDemuxer;

use std::path::Path;
use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use gst_pbutils::prelude::*;

/// Supported codecs for playback
const SUPPORTED_CODECS: &[&str] = &["mjpeg", "vp8", "vp9", "av1", "raw", "ffv1"];

/// Information about a video file's codec
#[derive(Debug, Clone)]
pub struct VideoCodecInfo {
    /// The detected codec name (e.g., "vp9", "mjpeg", "av1")
    pub codec: String,
    /// Whether this codec is supported for playback
    pub is_supported: bool,
    /// Human-readable reason if not supported
    pub reason: Option<String>,
}

/// Probe a video file to detect its actual video codec
/// Uses GStreamer's discoverer to analyze the file's video stream
pub fn probe_video_codec<P: AsRef<Path>>(path: P) -> Result<VideoCodecInfo, VideoError> {
    let path = path.as_ref();
    
    gst::init().map_err(|e| VideoError::Gst(e.to_string()))?;
    
    // Use GStreamer's Discoverer to analyze the file
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(10))
        .map_err(|e| VideoError::Gst(format!("Failed to create discoverer: {}", e)))?;
    
    let uri = format!("file:///{}", path.to_string_lossy().replace('\\', "/"));
    let info = discoverer.discover_uri(&uri)
        .map_err(|e| VideoError::Gst(format!("Failed to discover video: {}", e)))?;
    
    // Get video streams
    let video_streams = info.video_streams();
    if video_streams.is_empty() {
        return Err(VideoError::NoVideoTrack);
    }
    
    // Get the codec from the first video stream
    let video_stream = &video_streams[0];
    let caps = video_stream.caps()
        .ok_or_else(|| VideoError::Gst("No caps on video stream".into()))?;
    
    let structure = caps.structure(0)
        .ok_or_else(|| VideoError::Gst("No structure in caps".into()))?;
    
    let caps_name = structure.name().as_str();
    
    // Extract codec name from caps
    let codec = normalize_codec_name(caps_name);
    let is_supported = is_codec_supported(&codec);
    
    let reason = if !is_supported {
        Some(format!("Codec '{}' is not supported. Supported codecs: MJPEG, VP8, VP9, AV1, FFV1", codec))
    } else {
        None
    };
    
    Ok(VideoCodecInfo {
        codec,
        is_supported,
        reason,
    })
}

/// Normalize GStreamer caps name to a simple codec name
/// Returns "unsupported" for codecs we don't support
fn normalize_codec_name(caps_name: &str) -> String {
    match caps_name {
        // Supported codecs
        "image/jpeg" => "mjpeg".to_string(),
        "video/x-vp8" => "vp8".to_string(),
        "video/x-vp9" => "vp9".to_string(),
        "video/x-av1" | "video/av1" => "av1".to_string(),
        "video/x-raw" => "raw".to_string(),
        "video/x-ffv" => "ffv1".to_string(),
        "video/x-h264" => "h264".to_string(),
        // Unknown codecs
        _ => caps_name.replace("video/x-", "").replace("video/", "").replace("image/", ""),
    }
}

/// Check if a codec is supported for playback
fn is_codec_supported(codec: &str) -> bool {
    let codec_lower = codec.to_lowercase();
    SUPPORTED_CODECS.iter().any(|&c| codec_lower == c)
}

/// Detect the video format and create an appropriate demuxer
/// Only works for MJPEG codec which needs custom frame-by-frame playback.
/// VP8/VP9/AV1 should use the native HTML5 video player instead.
pub fn open_video<P: AsRef<Path>>(path: P) -> Result<Box<dyn VideoDemuxer>, VideoError> {
    let path = path.as_ref();
    
    // Probe the actual codec - don't rely on file extension since both MJPEG
    // and encoded codecs (VP8/VP9/AV1) may use the .mkv extension
    let codec_info = probe_video_codec(path)?;
    
    match codec_info.codec.as_str() {
        "mjpeg" => {
            let demuxer = MjpegDemuxer::open(path)?;
            Ok(Box::new(demuxer))
        }
        "ffv1" => {
            let demuxer = GstDecodeDemuxer::open(path, "ffv1")?;
            Ok(Box::new(demuxer))
        }
        codec @ ("vp8" | "vp9" | "av1") => {
            Err(VideoError::UnsupportedFormat(format!(
                "{} videos use the native player, not the custom demuxer", codec.to_uppercase()
            )))
        }
        other => Err(VideoError::UnsupportedCodec(other.to_string())),
    }
}

/// Error type for video operations
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Unsupported video format: {0}")]
    UnsupportedFormat(String),
    
    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("No video track found")]
    NoVideoTrack,
    
    #[error("Frame not found at timestamp {0}ms")]
    FrameNotFound(u64),
    
    #[error("GStreamer error: {0}")]
    Gst(String),
}
