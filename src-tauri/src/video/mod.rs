// Video file handling module
// 
// This module provides video file operations like demuxing and frame extraction.
// It's designed to be extensible for different container formats and codecs.
//
// Note: For VP8, VP9, and AV1, the native HTML5 video player handles playback.
// This module is primarily used for MJPEG frame extraction for the custom player.

pub mod demux;
pub mod mjpeg;

pub use demux::{VideoDemuxer, VideoFrame, VideoInfo};
pub use mjpeg::MjpegDemuxer;

use std::path::Path;
use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use gst_pbutils::prelude::*;

/// Supported codecs for playback
const SUPPORTED_CODECS: &[&str] = &["mjpeg", "jpeg", "vp8", "vp9", "av1", "raw"];

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
        Some(format!("Codec '{}' is not supported. Supported codecs: MJPEG, VP8, VP9, AV1", codec))
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
        // Unknown codecs
        _ => caps_name.replace("video/x-", "").replace("video/", "").replace("image/", ""),
    }
}

/// Check if a codec is supported for playback
fn is_codec_supported(codec: &str) -> bool {
    let codec_lower = codec.to_lowercase();
    SUPPORTED_CODECS.iter().any(|&c| codec_lower == c || codec_lower.contains(c))
}

/// Check if a video file has a supported codec for playback
pub fn is_video_playable<P: AsRef<Path>>(path: P) -> Result<bool, VideoError> {
    let info = probe_video_codec(path)?;
    Ok(info.is_supported)
}

/// Check if a video file requires the custom frame player
/// Returns true for MJPEG, false for VP8/VP9/AV1 (which use native player)
pub fn needs_custom_player(path: &Path) -> bool {
    // Check by extension - only MKV files with MJPEG need custom player
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    
    // WebM uses VP8/VP9/AV1 which native player handles
    // MKV uses MJPEG which needs custom player
    extension == "mkv"
}

/// Detect the video format and create an appropriate demuxer
/// Only works for formats that need custom playback (MJPEG in MKV)
pub fn open_video<P: AsRef<Path>>(path: P) -> Result<Box<dyn VideoDemuxer>, VideoError> {
    let path = path.as_ref();
    
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    
    match extension.as_str() {
        "mkv" => {
            // MKV contains MJPEG - use frame extractor
            let demuxer = MjpegDemuxer::open(path)?;
            Ok(Box::new(demuxer))
        }
        "webm" => {
            Err(VideoError::UnsupportedFormat(format!(
                "{} files use native player, not custom demuxer", extension
            )))
        }
        _ => Err(VideoError::UnsupportedFormat(extension)),
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
