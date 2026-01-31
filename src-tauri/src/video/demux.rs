// Video demuxer trait
//
// This defines the interface for video demuxers that can extract frames
// from different container formats.

use super::VideoError;

/// Information about a video stream
#[derive(Debug, Clone)]
pub struct VideoInfo {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Frame rate (frames per second)
    pub fps: f64,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Total number of frames (approximate)
    pub frame_count: u64,
    /// Codec name (e.g., "mjpeg", "vp9", "h264")
    pub codec: String,
}

/// A single video frame
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Frame data (format depends on codec - for MJPEG this is raw JPEG)
    pub data: Vec<u8>,
    /// Presentation timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Frame duration in milliseconds
    pub duration_ms: u64,
    /// Whether this is a keyframe
    pub is_keyframe: bool,
}

/// Trait for video demuxers
/// 
/// A demuxer extracts video frames from a container format.
/// Implementations should be stateful and support seeking.
pub trait VideoDemuxer: Send + Sync {
    /// Get information about the video stream
    fn info(&self) -> &VideoInfo;
    
    /// Get a frame at or near the specified timestamp
    /// 
    /// Returns the frame closest to the requested timestamp.
    /// For non-keyframe codecs, this may need to decode from the previous keyframe.
    fn get_frame_at(&mut self, timestamp_ms: u64) -> Result<VideoFrame, VideoError>;
    
    /// Get the next frame after the current position
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, VideoError>;
    
    /// Seek to a specific timestamp
    fn seek(&mut self, timestamp_ms: u64) -> Result<(), VideoError>;
    
    /// Get all frames in a time range
    /// 
    /// Returns frames from start_ms (inclusive) to end_ms (exclusive).
    fn get_frames_range(&mut self, start_ms: u64, end_ms: u64) -> Result<Vec<VideoFrame>, VideoError> {
        let mut frames = Vec::new();
        self.seek(start_ms)?;
        
        while let Some(frame) = self.next_frame()? {
            if frame.timestamp_ms >= end_ms {
                break;
            }
            if frame.timestamp_ms >= start_ms {
                frames.push(frame);
            }
        }
        
        Ok(frames)
    }
    
    /// Get frame timestamps for the entire video
    /// 
    /// This is useful for building a frame index without loading all frame data.
    fn get_frame_timestamps(&mut self) -> Result<Vec<u64>, VideoError>;
}
