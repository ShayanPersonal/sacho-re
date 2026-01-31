// Video file handling module
// 
// This module provides video file operations like demuxing and frame extraction.
// It's designed to be extensible for different container formats and codecs.
//
// Note: For H.264, H.265, and AV1, the native HTML5 video player handles playback.
// This module is primarily used for MJPEG frame extraction for the custom player.

pub mod demux;
pub mod mjpeg;

pub use demux::{VideoDemuxer, VideoFrame, VideoInfo};
pub use mjpeg::MjpegDemuxer;

use std::path::Path;

/// Check if a video file requires the custom frame player
/// Returns true for MJPEG, false for H.264/H.265/AV1 (which use native player)
pub fn needs_custom_player(path: &Path) -> bool {
    // Check by extension - only MKV files with MJPEG need custom player
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    
    // MP4 and WebM use H.264/H.265/AV1 which native player handles
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
        "mp4" | "webm" => {
            // These formats use H.264/H.265/AV1 - native player handles them
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
