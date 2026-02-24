// Video capture using GStreamer
//
// This module provides video recording with pre-roll buffering using GStreamer pipelines.
// Key features:
// - Continuous capture with ring-buffer pre-roll (configurable duration)
// - Passthrough encoding to MKV container
// - Non-blocking file I/O through GStreamer's async handling
// - Synchronization support with audio/MIDI streams

use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::encoding::{AsyncVideoEncoder, EncoderConfig, HardwareEncoderType, RawVideoFrame};
use crate::session::VideoFileInfo;

use super::preroll::MAX_PRE_ROLL_SECS_ENCODED;

/// Warning emitted when a video device delivers frames at a significantly
/// lower rate than the negotiated/requested framerate.
#[derive(serde::Serialize, Clone, Debug)]
pub struct VideoFpsWarning {
    pub device_name: String,
    pub actual_fps: f64,
    pub expected_fps: f64,
}

/// Error type for video capture operations
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("GStreamer error: {0}")]
    Gst(#[from] gst::glib::Error),

    #[error("GStreamer state error: {0}")]
    StateChange(#[from] gst::StateChangeError),

    #[error("Pipeline error: {0}")]
    Pipeline(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, VideoError>;

/// Buffered video frame with timestamp
#[derive(Clone)]
pub struct BufferedFrame {
    /// Frame data (encoded, e.g., MJPEG or raw)
    pub data: Vec<u8>,
    /// Presentation timestamp in nanoseconds
    pub pts: u64,
    /// Duration in nanoseconds
    pub duration: u64,
    /// Wall-clock time when frame was captured
    pub wall_time: Instant,
    /// Pixel format for raw video (e.g., "NV12", "I420"), None for encoded
    pub pixel_format: Option<String>,
    /// Whether this is a delta/inter frame (not a keyframe).
    /// Preserves the GStreamer DELTA_UNIT flag through the encode-during-preroll
    /// roundtrip so the muxer can correctly mark keyframes in the container.
    pub is_delta_unit: bool,
}

/// Pre-roll buffer for video frames
/// Maintains a rolling window of recent frames
pub struct VideoPrerollBuffer {
    frames: std::collections::VecDeque<BufferedFrame>,
    max_duration: Duration,
    /// Extra retention beyond max_duration to compensate for frame timing jitter
    headroom: Duration,
    /// Estimated bytes per second for memory management
    bytes_per_sec: usize,
    /// Maximum buffer size in bytes (to prevent unbounded memory usage)
    max_bytes: usize,
    current_bytes: usize,
}

impl VideoPrerollBuffer {
    /// Create a new pre-roll buffer for compressed video (MJPEG, H.264, etc.)
    pub fn new(max_duration_secs: u32) -> Self {
        // Estimate ~5MB/sec for compressed video (MJPEG at 720p30)
        Self::with_byte_rate(max_duration_secs, 5 * 1024 * 1024)
    }

    /// Create a new pre-roll buffer with a custom byte rate estimate.
    /// Use this for raw video where frame sizes are much larger than compressed.
    pub fn with_byte_rate(max_duration_secs: u32, bytes_per_sec: usize) -> Self {
        Self::with_headroom(max_duration_secs, bytes_per_sec, 0.0)
    }

    /// Create a new pre-roll buffer with extra headroom beyond `max_duration`.
    /// The buffer retains `max_duration + headroom` worth of frames during `trim()`,
    /// but `drain()` only returns the most recent `max_duration` of frames.
    /// This compensates for frame timing jitter and codec granularity.
    pub fn with_headroom(max_duration_secs: u32, bytes_per_sec: usize, headroom_secs: f64) -> Self {
        let headroom = Duration::from_secs_f64(headroom_secs);
        let total_secs = max_duration_secs as f64 + headroom_secs;
        let max_bytes = (bytes_per_sec as f64 * total_secs) as usize;

        Self {
            frames: std::collections::VecDeque::new(),
            max_duration: Duration::from_secs(max_duration_secs as u64),
            headroom,
            bytes_per_sec,
            max_bytes,
            current_bytes: 0,
        }
    }

    /// Push a new frame, trimming old frames if necessary
    pub fn push(&mut self, frame: BufferedFrame) {
        let frame_size = frame.data.len();
        self.current_bytes += frame_size;
        self.frames.push_back(frame);
        self.trim();
    }

    /// Trim old frames to stay within duration and memory limits.
    /// When max_duration is zero (pre-roll disabled), skip trimming entirely —
    /// the buffer acts purely as a staging area between the appsink callback
    /// and the poll thread, which drains it at ~100Hz.
    fn trim(&mut self) {
        if self.max_duration.is_zero() {
            return;
        }

        let retention = self.max_duration + self.headroom;
        let cutoff = Instant::now() - retention;

        // Trim by time (retaining headroom beyond max_duration)
        while let Some(front) = self.frames.front() {
            if front.wall_time < cutoff || self.current_bytes > self.max_bytes {
                if let Some(removed) = self.frames.pop_front() {
                    self.current_bytes = self.current_bytes.saturating_sub(removed.data.len());
                }
            } else {
                break;
            }
        }
    }

    /// Drain all frames from the buffer, trimmed to at most `max_duration`.
    /// When headroom is configured, the buffer retains extra frames beyond
    /// `max_duration` — this method strips them so the output doesn't exceed
    /// the configured pre-roll length.
    pub fn drain(&mut self) -> Vec<BufferedFrame> {
        self.current_bytes = 0;
        let mut frames: Vec<BufferedFrame> = self.frames.drain(..).collect();

        if !self.headroom.is_zero() && !frames.is_empty() {
            let latest = frames.last().unwrap().wall_time;
            let cutoff = latest - self.max_duration;
            frames.retain(|f| f.wall_time >= cutoff);
        }

        frames
    }

    /// Get the duration of buffered content
    pub fn duration(&self) -> Duration {
        if self.frames.is_empty() {
            return Duration::ZERO;
        }

        let first = self.frames.front().unwrap();
        let last = self.frames.back().unwrap();
        last.wall_time.duration_since(first.wall_time)
    }

    /// Set the maximum duration
    pub fn set_duration(&mut self, secs: u32) {
        self.max_duration = Duration::from_secs(secs as u64);
        let total_secs = secs as f64 + self.headroom.as_secs_f64();
        self.max_bytes = (self.bytes_per_sec as f64 * total_secs) as usize;
        self.trim();
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Clear all buffered frames
    pub fn clear(&mut self) {
        self.frames.clear();
        self.current_bytes = 0;
    }
}

/// Represents a single video capture pipeline for one device
pub struct VideoCapturePipeline {
    /// Device identifier
    pub device_id: String,
    /// Human-readable device name
    pub device_name: String,
    /// Source format string (e.g. "YUY2", "MJPEG", "H264")
    pub source_format: String,
    /// GStreamer pipeline
    pipeline: gst::Pipeline,
    /// App sink for pulling frames (kept alive for pipeline)
    #[allow(dead_code)]
    appsink: gst_app::AppSink,
    /// Pre-roll buffer
    preroll_buffer: Arc<Mutex<VideoPrerollBuffer>>,
    /// Current recording file path (None if not recording)
    recording_path: Option<PathBuf>,
    /// Recording start time
    recording_start: Option<Instant>,
    /// PTS offset for current recording (to normalize timestamps to start at 0).
    /// None until the first frame is seen, then set to that frame's PTS.
    pts_offset: Option<u64>,
    /// Frames written during current recording
    frames_written: u64,
    /// Video dimensions
    pub width: u32,
    pub height: u32,
    /// Frame rate (f64 to preserve fractional rates like 29.97)
    pub fps: f64,
    /// Is currently recording
    is_recording: bool,
    /// File handle for recording (for pre-encoded video)
    file_writer: Option<VideoWriter>,
    /// Async encoder for raw video
    raw_encoder: Option<AsyncVideoEncoder>,
    /// Whether this pipeline is encoding (not passthrough)
    is_encoding: bool,
    /// Target encoding codec (AV1/VP9/VP8/FFV1). None = auto-detect.
    encoding_codec: Option<crate::encoding::VideoCodec>,
    /// Hardware encoder type. None = auto-detect.
    encoder_type: Option<HardwareEncoderType>,
    /// Pixel format for raw video capture
    pixel_format: Option<String>,
    /// Consecutive polls where ALL frames were dropped (encoder stalled detection)
    consecutive_full_drops: u32,
    /// Total frames dropped during this recording
    total_frames_dropped: u64,
    /// Encoder quality preset level (1–5)
    preset_level: u8,
    /// Compute effort level (1–5) for software encoders
    effort_level: u8,
    /// Encoding bit depth for lossless codecs (FFV1). None = 8-bit default.
    video_bit_depth: Option<u8>,
    /// Whether encode-during-preroll is active (raw video only)
    encode_during_preroll: bool,
    /// Configured pre-roll duration in seconds
    pre_roll_secs: u32,
    /// Shared flag: appsink callback skips frame allocation when false.
    /// True when recording or when pre_roll_secs > 0 (frames are needed).
    needs_frames: Arc<AtomicBool>,
    /// Pre-roll encoder (when encode_during_preroll is active)
    preroll_encoder: Option<PrerollVideoEncoder>,
    /// Shared output from pre-roll encoder
    preroll_encoder_output: Option<Arc<Mutex<PrerollEncoderOutput>>>,
    /// Target encoding width (may differ from source width for raw codec)
    target_width: u32,
    /// Target encoding height (may differ from source height for raw codec)
    target_height: u32,
    /// Target encoding fps (may differ from source fps for raw codec)
    target_fps: f64,
    /// Shared frame counter from the appsink callback (for FPS measurement)
    frame_counter: Arc<AtomicU64>,
    /// Timestamp when FPS measurement started
    fps_check_start: Instant,
    /// Frame count snapshot at last FPS check
    frames_at_last_check: u64,
    /// Whether we've already emitted a FPS mismatch warning
    fps_warning_emitted: bool,
}

/// Generic video file writer that handles different codecs and containers
///
/// Pipeline: appsrc -> parser -> muxer -> filesink
struct VideoWriter {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    output_path: PathBuf,
    /// Tracks the end of the last written frame (PTS + duration, in nanoseconds)
    /// for accurate content duration reporting.
    last_pts_end_ns: u64,
}

impl VideoWriter {
    /// Create a new video writer for the specified codec
    fn new(
        path: &PathBuf,
        codec: crate::encoding::VideoCodec,
        width: u32,
        height: u32,
        fps: f64,
    ) -> Result<Self> {
        use crate::encoding::encoder::fps_to_gst_fraction;

        let pipeline = gst::Pipeline::new();
        let container = codec.container();

        println!(
            "[Video] Creating {} writer with {} codec (creating elements...)",
            container.extension(),
            codec.display_name()
        );

        // Create appsrc with appropriate caps for the codec
        let caps = gst::Caps::builder(codec.gst_caps_name())
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", fps_to_gst_fraction(fps))
            .build();

        let appsrc = gst_app::AppSrc::builder()
            .name("src")
            .caps(&caps)
            .format(gst::Format::Time)
            .is_live(true)
            .build();

        // Create muxer for the container
        let muxer = gst::ElementFactory::make(container.gst_muxer())
            .build()
            .map_err(|e| {
                VideoError::Pipeline(format!("Failed to create {}: {}", container.gst_muxer(), e))
            })?;

        // Set muxer-specific properties
        match container {
            crate::encoding::ContainerFormat::Mkv => {
                muxer.set_property("writing-app", "Sacho");
            }
        }

        let filesink = gst::ElementFactory::make("filesink")
            .property("location", path.to_string_lossy().to_string())
            .property("async", false)
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create filesink: {}", e)))?;

        println!("[Video]   Elements created, adding to pipeline...");

        // For MJPEG, skip the parser and link directly to muxer.
        // jpegparse extracts dimensions from JPEG SOF markers, which can override
        // the dimensions we set in appsrc caps. Some capture devices output JPEG
        // frames with swapped width/height in the JPEG headers, causing container
        // metadata to show incorrect dimensions (e.g., 640x720 instead of 720x640).
        // By skipping jpegparse for MJPEG, we let matroskamux use the appsrc caps
        // which contain the correct dimensions from the capture pipeline.
        let use_parser = !matches!(codec, crate::encoding::VideoCodec::Mjpeg);

        if use_parser {
            // Create parser for the codec
            let parser = gst::ElementFactory::make(codec.gst_parser())
                .build()
                .map_err(|e| {
                    VideoError::Pipeline(format!("Failed to create {}: {}", codec.gst_parser(), e))
                })?;

            // Add elements to pipeline
            pipeline
                .add_many([appsrc.upcast_ref(), &parser, &muxer, &filesink])
                .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;

            println!("[Video]   Elements added, linking with parser...");

            // Link elements
            gst::Element::link_many([appsrc.upcast_ref(), &parser, &muxer, &filesink])
                .map_err(|e| VideoError::Pipeline(format!("Failed to link elements: {}", e)))?;
        } else {
            // MJPEG: skip parser, link appsrc directly to muxer
            pipeline
                .add_many([appsrc.upcast_ref(), &muxer, &filesink])
                .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;

            println!("[Video]   Elements added, linking directly (no parser)...");

            // Link elements
            gst::Element::link_many([appsrc.upcast_ref(), &muxer, &filesink])
                .map_err(|e| VideoError::Pipeline(format!("Failed to link elements: {}", e)))?;
        }

        println!("[Video]   Elements linked, starting pipeline...");

        // Start pipeline with async state change (don't block)
        pipeline.set_state(gst::State::Playing)?;

        // Don't wait for state change - appsrc with is_live=true doesn't need preroll
        // The pipeline will transition to PLAYING when we push the first buffer
        println!("[Video] Writer pipeline started");

        Ok(Self {
            pipeline,
            appsrc,
            output_path: path.clone(),
            last_pts_end_ns: 0,
        })
    }

    fn write_frame(&mut self, frame: &BufferedFrame, pts_offset: Option<u64>) -> Result<()> {
        let offset = pts_offset.unwrap_or(frame.pts);
        let normalized_pts = frame.pts.saturating_sub(offset);
        let mut buffer = gst::Buffer::from_slice(frame.data.clone());
        {
            let buffer_ref = buffer.get_mut().expect("BUG: freshly created buffer has refcount > 1");
            buffer_ref.set_pts(gst::ClockTime::from_nseconds(normalized_pts));
            buffer_ref.set_duration(gst::ClockTime::from_nseconds(frame.duration));
            // Preserve the keyframe/delta flag so the muxer marks frames correctly.
            // Without this, all frames are treated as keyframes and VP8/VP9
            // inter-frames get mislabeled, making the file unplayable in browsers.
            if frame.is_delta_unit {
                buffer_ref.set_flags(gst::BufferFlags::DELTA_UNIT);
            }
        }

        // Track content duration
        let pts_end = normalized_pts + frame.duration;
        if pts_end > self.last_pts_end_ns {
            self.last_pts_end_ns = pts_end;
        }

        self.appsrc
            .push_buffer(buffer)
            .map_err(|e| VideoError::Pipeline(format!("Failed to push buffer: {:?}", e)))?;

        Ok(())
    }

    fn finish(self) -> Result<(Duration, u64)> {
        let content_duration = Duration::from_nanos(self.last_pts_end_ns);

        // Send EOS and wait for pipeline to finish
        let eos_result = self.appsrc.end_of_stream();
        if let Err(e) = &eos_result {
            println!("[Video] Warning: Failed to send EOS: {:?}", e);
        }

        // Wait for EOS to propagate
        let mut pipeline_error: Option<String> = None;
        let Some(bus) = self.pipeline.bus() else {
            // No bus available - still try to cleanup and return
            let _ = self.pipeline.set_state(gst::State::Null);
            let file_size = std::fs::metadata(&self.output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            return Ok((content_duration, file_size));
        };
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(5)) {
            match msg.view() {
                gst::MessageView::Eos(..) => break,
                gst::MessageView::Error(err) => {
                    pipeline_error = Some(format!(
                        "Pipeline error: {} ({:?})",
                        err.error(),
                        err.debug()
                    ));
                    break;
                }
                _ => {}
            }
        }

        // Always set pipeline to NULL before dropping to avoid GStreamer warnings
        let _ = self.pipeline.set_state(gst::State::Null);

        // Return error if there was one
        if let Some(err) = pipeline_error {
            return Err(VideoError::Pipeline(err));
        }

        // Get file size from the output path
        let file_size = std::fs::metadata(&self.output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok((content_duration, file_size))
    }
}

impl Drop for VideoWriter {
    fn drop(&mut self) {
        // Ensure pipeline is stopped to avoid GStreamer resource leaks
        // This handles cases where finish() was not called (e.g., error paths)
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

// ============================================================================
// Pre-roll Video Encoder (continuous encoding during pre-roll)
// ============================================================================

/// Shared output state for the pre-roll video encoder.
///
/// The GStreamer appsink callback and main code share this via `Arc<Mutex<>>`.
/// During pre-roll, encoded frames accumulate in a time-bounded ring buffer.
/// When recording starts, the ring buffer is drained to a `VideoWriter` and
/// subsequent encoded frames are routed directly to that writer.
struct PrerollEncoderOutput {
    /// Ring buffer of encoded frames (trimmed by time)
    buffer: std::collections::VecDeque<BufferedFrame>,
    /// Maximum pre-roll duration
    max_duration: Duration,
    /// Keyframe interval duration — extra headroom kept in the time-based trim
    /// so that after stripping to the next keyframe we still meet `max_duration`.
    keyframe_duration: Duration,
    /// Current buffer size in bytes
    current_bytes: usize,
    /// When Some, encoded frames are routed here instead of the ring buffer
    active_writer: Option<VideoWriter>,
    /// PTS offset for normalizing timestamps in the writer
    pts_offset: Option<u64>,
    /// Target codec (needed for VideoWriter creation)
    target_codec: crate::encoding::VideoCodec,
}

impl PrerollEncoderOutput {
    fn new(
        max_duration_secs: u32,
        target_codec: crate::encoding::VideoCodec,
        keyframe_interval_secs: u32,
    ) -> Self {
        Self {
            buffer: std::collections::VecDeque::new(),
            max_duration: Duration::from_secs(max_duration_secs as u64),
            keyframe_duration: Duration::from_secs(keyframe_interval_secs as u64),
            current_bytes: 0,
            active_writer: None,
            pts_offset: None,
            target_codec,
        }
    }

    /// Push an encoded frame. Routes to either the ring buffer or the active writer.
    fn push_encoded_frame(&mut self, frame: BufferedFrame) {
        if let Some(ref mut writer) = self.active_writer {
            // Recording active: write to file
            if let Err(e) = writer.write_frame(&frame, self.pts_offset) {
                println!(
                    "[PrerollEncoder] Warning: Failed to write frame to writer: {}",
                    e
                );
            }
        } else {
            // Pre-roll phase: add to ring buffer
            self.current_bytes += frame.data.len();
            self.buffer.push_back(frame);
            self.trim();
        }
    }

    fn trim(&mut self) {
        // Keep an extra keyframe interval of headroom beyond max_duration so
        // that the keyframe-seeking pass below doesn't eat into the requested
        // pre-roll window. With keyframes every 2 s and max_duration = 5 s,
        // we retain ~7 s of frames by time, then strip to the first keyframe,
        // leaving ≥5 s of usable pre-roll.
        let retention = self.max_duration + self.keyframe_duration;
        let cutoff = Instant::now() - retention;
        while let Some(front) = self.buffer.front() {
            if front.wall_time < cutoff {
                if let Some(removed) = self.buffer.pop_front() {
                    self.current_bytes = self.current_bytes.saturating_sub(removed.data.len());
                }
            } else {
                break;
            }
        }
        // Ensure the buffer starts at a keyframe. After the time-based trim the
        // first remaining frame may be a delta/inter-frame which can't be decoded
        // without its reference keyframe. Drop frames until we hit a keyframe.
        while let Some(front) = self.buffer.front() {
            if front.is_delta_unit {
                if let Some(removed) = self.buffer.pop_front() {
                    self.current_bytes = self.current_bytes.saturating_sub(removed.data.len());
                }
            } else {
                break;
            }
        }
    }

    /// Drain all buffered encoded frames (for recording start)
    fn drain(&mut self) -> Vec<BufferedFrame> {
        self.current_bytes = 0;
        self.buffer.drain(..).collect()
    }

    /// Duration of buffered content
    fn duration(&self) -> Duration {
        if self.buffer.is_empty() {
            return Duration::ZERO;
        }
        let first = self.buffer.front().unwrap();
        let last = self.buffer.back().unwrap();
        last.wall_time.duration_since(first.wall_time)
    }

    /// Clear all buffered encoded frames
    fn clear(&mut self) {
        self.buffer.clear();
        self.current_bytes = 0;
    }
}

/// Continuously encodes raw video frames during pre-roll.
///
/// Runs a GStreamer pipeline (`appsrc -> queue -> videoconvert -> encoder -> appsink`)
/// on its own streaming thread. Encoded frames are stored in a shared ring buffer
/// until recording starts, then seamlessly routed to a file writer.
///
/// This trades CPU/GPU compute for dramatically reduced memory usage, allowing
/// pre-roll durations up to 30 seconds even at high resolutions.
struct PrerollVideoEncoder {
    /// GStreamer encoding pipeline
    pipeline: gst::Pipeline,
    /// AppSrc for pushing raw frames
    appsrc: gst_app::AppSrc,
    /// Shared output state (ring buffer / active writer)
    output: Arc<Mutex<PrerollEncoderOutput>>,
}

impl PrerollVideoEncoder {
    fn new(
        width: u32,
        height: u32,
        fps: f64,
        target_codec: crate::encoding::VideoCodec,
        preset_level: u8,
        effort_level: u8,
        video_bit_depth: Option<u8>,
        max_preroll_secs: u32,
        target_width: Option<u32>,
        target_height: Option<u32>,
        target_fps: Option<f64>,
    ) -> Result<Self> {
        use crate::encoding::encoder::{
            detect_best_encoder_for_codec, AsyncVideoEncoder, EncoderConfig,
        };

        let hw_type = detect_best_encoder_for_codec(target_codec)
            .ok_or_else(|| VideoError::Pipeline(
                format!("No encoder available for {}", target_codec.display_name())
            ))?;
        println!(
            "[PrerollEncoder] Using {} for {} encoding (pre-roll)",
            hw_type.display_name(),
            target_codec.display_name()
        );

        let effective_fps = target_fps.unwrap_or(fps);
        let config = EncoderConfig {
            keyframe_interval: (effective_fps * 2.0).round() as u32,
            target_codec,
            preset_level,
            effort_level,
            video_bit_depth,
            target_width,
            target_height,
            target_fps,
        };

        // Create the common pipeline start (appsrc -> queue -> videoconvert [-> scale] [-> rate])
        let pixel_format = crate::encoding::intermediate_format_for_codec(target_codec, video_bit_depth);
        let (pipeline, appsrc, chain_tail) =
            AsyncVideoEncoder::create_common_pipeline_start_with_target(
                width,
                height,
                fps,
                target_width,
                target_height,
                target_fps,
                pixel_format,
            )
            .map_err(|e| VideoError::Pipeline(format!("PrerollEncoder pipeline: {}", e)))?;

        // Create encoder element based on target codec
        let encoder = match target_codec {
            crate::encoding::VideoCodec::Av1 => {
                AsyncVideoEncoder::create_av1_encoder(hw_type, &config)
            }
            crate::encoding::VideoCodec::Vp8 => {
                AsyncVideoEncoder::create_vp8_encoder(hw_type, &config)
            }
            crate::encoding::VideoCodec::Vp9 => {
                AsyncVideoEncoder::create_vp9_encoder(hw_type, &config)
            }

            crate::encoding::VideoCodec::Ffv1 => {
                AsyncVideoEncoder::create_ffv1_encoder(hw_type, &config)
            }
            crate::encoding::VideoCodec::H264 => {
                AsyncVideoEncoder::create_h264_encoder(hw_type, &config)
            }
            _ => {
                return Err(VideoError::Pipeline(format!(
                    "Unsupported codec for preroll encoding: {:?}",
                    target_codec
                )))
            }
        }
        .map_err(|e| VideoError::Pipeline(format!("PrerollEncoder encoder: {}", e)))?;

        // Create appsink for encoded output
        let appsink = gst_app::AppSink::builder()
            .name("enc_sink")
            .sync(false)
            .build();

        // Add encoder-specific elements and link from the common chain tail
        pipeline
            .add_many([&encoder, appsink.upcast_ref()])
            .map_err(|e| {
                VideoError::Pipeline(format!("Failed to add PrerollEncoder elements: {}", e))
            })?;
        gst::Element::link_many([&chain_tail, &encoder, appsink.upcast_ref()]).map_err(|e| {
            VideoError::Pipeline(format!("Failed to link PrerollEncoder elements: {}", e))
        })?;

        // Create shared output.
        // The keyframe interval is `fps * 2` frames = 2 seconds.
        let keyframe_interval_secs = 2;
        let output = Arc::new(Mutex::new(PrerollEncoderOutput::new(
            max_preroll_secs,
            target_codec,
            keyframe_interval_secs,
        )));

        // Set up appsink callback to route encoded frames
        let output_clone = output.clone();
        // Compute default frame duration from target fps (fallback when buffer lacks duration metadata)
        let enc_default_duration_ns = (1_000_000_000.0 / target_fps.unwrap_or(fps)).round() as u64;
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    match sink.pull_sample() {
                        Ok(sample) => {
                            if let Some(buffer) = sample.buffer() {
                                let pts = buffer.pts().map(|t| t.nseconds()).unwrap_or(0);
                                let duration = buffer
                                    .duration()
                                    .map(|t| t.nseconds())
                                    .unwrap_or(enc_default_duration_ns);
                                let is_delta =
                                    buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);

                                if let Ok(map) = buffer.map_readable() {
                                    let data = map.as_slice().to_vec();
                                    let frame = BufferedFrame {
                                        data,
                                        pts,
                                        duration,
                                        wall_time: Instant::now(),
                                        pixel_format: None, // Encoded, no pixel format
                                        is_delta_unit: is_delta,
                                    };
                                    output_clone.lock().push_encoded_frame(frame);
                                }
                            }
                            Ok(gst::FlowSuccess::Ok)
                        }
                        Err(_) => Err(gst::FlowError::Error),
                    }
                })
                .build(),
        );

        // Start the pipeline
        pipeline.set_state(gst::State::Playing).map_err(|e| {
            VideoError::Pipeline(format!("Failed to start PrerollEncoder: {:?}", e))
        })?;

        println!(
            "[PrerollEncoder] Pipeline started ({}x{} @ {}fps -> {})",
            width,
            height,
            fps,
            target_codec.display_name()
        );

        Ok(Self {
            pipeline,
            appsrc,
            output,
        })
    }

    /// Push a raw frame to be encoded.
    /// Non-blocking: if the pipeline can't accept the frame, it is silently dropped.
    fn push_frame(&self, frame: &BufferedFrame) {
        let mut buffer = gst::Buffer::from_slice(frame.data.clone());
        {
            let buffer_ref = buffer.get_mut().expect("BUG: freshly created buffer has refcount > 1");
            buffer_ref.set_pts(gst::ClockTime::from_nseconds(frame.pts));
            buffer_ref.set_duration(gst::ClockTime::from_nseconds(frame.duration));
        }

        // Push to the encoder pipeline; if the pipeline is full the frame is dropped
        if let Err(e) = self.appsrc.push_buffer(buffer) {
            println!("[PrerollEncoder] Warning: Failed to push frame: {:?}", e);
        }
    }
}

impl Drop for PrerollVideoEncoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl VideoCapturePipeline {
    /// Create the GStreamer source element for a video device.
    ///
    /// When `matched_device` is provided (from `get_device_for_caps`), it uses that
    /// exact provider, ensuring the pipeline source matches the caps that were validated.
    /// Otherwise, falls back to the first stored device or platform-specific defaults.
    fn create_source_element(
        device_id: &str,
        device_index: u32,
        device_name_hint: &str,
        matched_device: Option<gstreamer::Device>,
    ) -> Result<(gst::Element, String)> {
        // Use the matched device (from caps lookup) or fall back to any stored device
        let gst_device =
            matched_device.or_else(|| crate::devices::enumeration::get_gst_device(device_id));

        if let Some(gst_device) = gst_device {
            match gst_device.create_element(Some("source")) {
                Ok(src) => {
                    let factory_name = src
                        .factory()
                        .map(|f| f.name().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let device_name = gst_device.display_name().to_string();
                    println!(
                        "[Video] Using device provider '{}' -> {} for {}",
                        gst_device.device_class(),
                        factory_name,
                        device_name
                    );
                    return Ok((src, device_name));
                }
                Err(e) => {
                    println!(
                        "[Video] Warning: Device::create_element failed for {}: {}",
                        device_id, e
                    );
                    println!("[Video] Falling back to manual source creation");
                }
            }
        } else {
            println!(
                "[Video] No saved GStreamer device for {}, using fallback",
                device_id
            );
        }

        // Fallback: create source element manually based on platform
        println!("[Video] Warning: Using fallback source creation for '{}' (index {})", device_name_hint, device_index);

        #[cfg(target_os = "windows")]
        let (source, device_name) = {
            // Prefer Media Foundation (mfvideosrc) over legacy DirectShow (dshowvideosrc)
            if let Ok(src) = gst::ElementFactory::make("mfvideosrc")
                .property("device-index", device_index as u32)
                .build()
            {
                let name = src
                    .property::<Option<String>>("device-name")
                    .unwrap_or_else(|| device_name_hint.to_string());
                (src, name)
            } else {
                println!("[Video] mfvideosrc unavailable, falling back to dshowvideosrc");
                let src = gst::ElementFactory::make("dshowvideosrc")
                    .property("device-name", device_name_hint)
                    .build()
                    .map_err(|e| {
                        VideoError::Pipeline(format!("Failed to create dshowvideosrc: {}", e))
                    })?;
                (src, device_name_hint.to_string())
            }
        };

        #[cfg(target_os = "linux")]
        let (source, device_name) = {
            println!("[Video] Assuming /dev/video{} for device index {}", device_index, device_index);
            let src = gst::ElementFactory::make("v4l2src")
                .property("device", format!("/dev/video{}", device_index))
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create v4l2src: {}", e)))?;
            let name = src
                .property::<Option<String>>("device-name")
                .unwrap_or_else(|| format!("Webcam {}", device_index));
            (src, name)
        };

        #[cfg(target_os = "macos")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("avfvideosrc")
                .property("device-index", device_index as i32)
                .build()
                .map_err(|e| {
                    VideoError::Pipeline(format!("Failed to create avfvideosrc: {}", e))
                })?;
            let name = src
                .property::<Option<String>>("device-name")
                .unwrap_or_else(|| format!("Webcam {}", device_index));
            (src, name)
        };

        Ok((source, device_name))
    }

    /// Create a new capture pipeline for a webcam device with passthrough
    ///
    /// This pipeline captures video directly from the camera without re-encoding,
    /// which is much more efficient than decode+encode.
    ///
    /// - `device_index`: Device index (used on Linux/macOS)
    /// - `device_name`: Device name (used on Windows with DirectShow)
    /// - `codec`: Video codec to capture
    /// - `source_width`, `source_height`, `source_fps`: Exact source resolution/fps to request
    /// - `pre_roll_secs`: Pre-roll buffer duration
    /// - `device_id`: Our internal device ID (e.g. "video-logi_c270_hd_webcam") used to
    ///    look up the saved GStreamer Device object from enumeration
    pub fn new_webcam(
        device_index: u32,
        device_name_hint: &str,
        device_id: &str,
        source_format: &str,
        source_width: u32,
        source_height: u32,
        source_fps: f64,
        pre_roll_secs: u32,
    ) -> Result<Self> {
        // Initialize GStreamer if not already done
        gst::init().map_err(|e| VideoError::Gst(e))?;

        let pipeline = gst::Pipeline::new();

        // Find the exact caps AND the provider that supports them.
        // The matched device is then used to create the source element, ensuring
        // the pipeline uses the correct provider (KS vs MF vs DirectShow).
        let (caps_name, format_field) = crate::encoding::format_to_gst_caps(source_format);
        let (input_caps, matched_device) = crate::devices::enumeration::get_device_for_format(
            device_id,
            source_format,
            source_width,
            source_height,
            source_fps,
        )
        .map(|(caps, dev)| (caps, Some(dev)))
        .unwrap_or_else(|| {
            println!("[Video] Using fallback partial caps (no exact provider match available)");
            let mut builder = gst::Caps::builder(caps_name)
                .field("width", source_width as i32)
                .field("height", source_height as i32)
                .field(
                    "framerate",
                    crate::encoding::encoder::fps_to_gst_fraction(source_fps),
                );
            if let Some(fmt) = format_field {
                builder = builder.field("format", fmt);
            }
            (builder.build(), None)
        });

        let (source, device_name) =
            Self::create_source_element(device_id, device_index, device_name_hint, matched_device)?;

        println!(
            "[Video] Creating {} passthrough pipeline for {} (device {})",
            source_format,
            device_name,
            device_index
        );

        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", &input_caps)
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create capsfilter: {}", e)))?;

        // Queue for buffering
        let queue = gst::ElementFactory::make("queue")
            .property("max-size-buffers", 60u32)
            .property_from_str("leaky", "downstream")
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create queue: {}", e)))?;

        // App sink to pull frames
        let appsink = gst_app::AppSink::builder()
            .name("sink")
            .max_buffers(2)
            .drop(true)
            .sync(false)
            .build();

        // Skip parsers in the capture pipeline — cameras output well-formed frames.
        // Note: jpegparse IS still used in the MjpegDemuxer for playback (video/mjpeg.rs).
        pipeline
            .add_many([&source, &capsfilter, &queue, appsink.upcast_ref()])
            .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;

        gst::Element::link_many([&source, &capsfilter, &queue, appsink.upcast_ref()])
            .map_err(|e| VideoError::Pipeline(format!("Failed to link pipeline: {}", e)))?;

        // Debug: Print the caps being used
        println!(
            "[Video] {} passthrough pipeline created for {} (device {})",
            source_format,
            device_name,
            device_index
        );
        println!(
            "[Video]   Capsfilter set to: {} {}x{} @ {}fps",
            caps_name,
            source_width,
            source_height,
            source_fps
        );

        // Create pre-roll buffer with 2s headroom for compressed cameras (one full GOP)
        let preroll_buffer = Arc::new(Mutex::new(VideoPrerollBuffer::with_headroom(
            pre_roll_secs,
            5 * 1024 * 1024,
            2.0,
        )));

        // Shared flag: the appsink callback skips frame allocation when false.
        // True when pre_roll_secs > 0 or recording is active.
        let needs_frames = Arc::new(AtomicBool::new(pre_roll_secs > 0));

        // Set up appsink callback to fill pre-roll buffer
        let preroll_clone = preroll_buffer.clone();
        let needs_frames_clone = needs_frames.clone();
        let frame_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let frame_counter_clone = frame_counter.clone();
        // Compute default frame duration from source fps (fallback when buffer lacks duration metadata)
        let default_duration_ns = (1_000_000_000.0 / source_fps).round() as u64;

        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    // Always pull sample and count frames (for health check monitoring)
                    match sink.pull_sample() {
                        Ok(sample) => {
                            frame_counter_clone.fetch_add(1, Ordering::Relaxed);

                            if !needs_frames_clone.load(Ordering::Relaxed) {
                                // Discard: no pre-roll needed and not recording
                                return Ok(gst::FlowSuccess::Ok);
                            }

                            if let Some(buffer) = sample.buffer() {
                                let pts = buffer.pts().map(|t| t.nseconds()).unwrap_or(0);
                                let duration = buffer
                                    .duration()
                                    .map(|t| t.nseconds())
                                    .unwrap_or(default_duration_ns);

                                let is_delta =
                                    buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);

                                if let Ok(map) = buffer.map_readable() {
                                    let data = map.as_slice().to_vec();

                                    let frame = BufferedFrame {
                                        data,
                                        pts,
                                        duration,
                                        wall_time: Instant::now(),
                                        pixel_format: None, // Pre-encoded, no pixel format
                                        is_delta_unit: is_delta,
                                    };
                                    preroll_clone.lock().push(frame);
                                }
                            }
                            Ok(gst::FlowSuccess::Ok)
                        }
                        Err(_) => Err(gst::FlowError::Error),
                    }
                })
                .build(),
        );

        Ok(Self {
            device_id: format!("webcam-{}", device_index),
            device_name,
            source_format: source_format.to_string(),
            pipeline,
            appsink,
            preroll_buffer,
            recording_path: None,
            recording_start: None,
            pts_offset: None,
            frames_written: 0,
            width: source_width,
            height: source_height,
            fps: source_fps,
            is_recording: false,
            file_writer: None,
            raw_encoder: None,
            is_encoding: false,
            encoding_codec: None,
            encoder_type: None,
            pixel_format: None,
            consecutive_full_drops: 0,
            total_frames_dropped: 0,
            preset_level: crate::encoding::DEFAULT_PRESET,
            effort_level: crate::encoding::DEFAULT_PRESET,
            video_bit_depth: None,
            encode_during_preroll: false,
            pre_roll_secs,
            needs_frames,
            preroll_encoder: None,
            preroll_encoder_output: None,
            target_width: source_width,
            target_height: source_height,
            target_fps: source_fps,
            frame_counter,
            fps_check_start: Instant::now(),
            frames_at_last_check: 0,
            fps_warning_emitted: false,
        })
    }

    /// Create a new capture pipeline that decodes source video to raw pixels for encoding.
    ///
    /// Supports any source format: raw pixels (no decoder), MJPEG (jpegdec), VP8/VP9/AV1/FFV1/H264 (appropriate decoder).
    /// The intermediate pixel format is chosen based on the target codec: P010_10LE (10-bit)
    /// for AV1 (always) and FFV1 with video_bit_depth=10, NV12 (8-bit) for everything else.
    ///
    /// - `source_format`: The source format string (e.g. "YUY2", "MJPEG", "H264")
    /// - `encoding_codec`: Target encoding codec (None = auto-detect)
    /// - `encoder_type_hint`: Hardware encoder to use (None = auto-detect)
    pub fn new_webcam_raw(
        device_index: u32,
        device_name_hint: &str,
        device_id: &str,
        source_format: &str,
        source_width: u32,
        source_height: u32,
        source_fps: f64,
        pre_roll_secs: u32,
        encoding_codec: Option<crate::encoding::VideoCodec>,
        encoder_type_hint: Option<HardwareEncoderType>,
        preset_level: u8,
        video_bit_depth: Option<u8>,
        encode_during_preroll: bool,
    ) -> Result<Self> {
        // Initialize GStreamer if not already done
        gst::init().map_err(|e| VideoError::Gst(e))?;

        let pipeline = gst::Pipeline::new();

        // Find exact caps AND the matching provider for the source format.
        let (gst_caps_name, format_field) = crate::encoding::format_to_gst_caps(source_format);
        let (input_caps, matched_device) = crate::devices::enumeration::get_device_for_format(
            device_id,
            source_format,
            source_width,
            source_height,
            source_fps,
        )
        .map(|(caps, dev)| (caps, Some(dev)))
        .unwrap_or_else(|| {
            println!("[Video] Using fallback partial caps (no exact provider match available)");
            let mut builder = gst::Caps::builder(gst_caps_name)
                .field("width", source_width as i32)
                .field("height", source_height as i32)
                .field(
                    "framerate",
                    crate::encoding::encoder::fps_to_gst_fraction(source_fps),
                );
            if let Some(fmt) = format_field {
                builder = builder.field("format", fmt);
            }
            (builder.build(), None)
        });

        let (source, device_name) =
            Self::create_source_element(device_id, device_index, device_name_hint, matched_device)?;

        println!(
            "[Video] Creating encoding capture pipeline for {} (device {}, source: {})",
            device_name,
            device_index,
            source_format
        );

        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", &input_caps)
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create capsfilter: {}", e)))?;

        // Build element chain: source → capsfilter → [decoder] → videoconvert → capsfilter(NV12) → queue → appsink
        let mut elements: Vec<gst::Element> = vec![source.clone(), capsfilter.clone()];

        // Insert decoder if source is not raw
        if let Some(decoder_name) = crate::encoding::decoder_for_format(source_format) {
            // Workaround for GStreamer issue #1118: mfvideosrc (and ksvideosrc) may put
            // non-standard fields like colorimetry and pixel-aspect-ratio on image/jpeg
            // caps. jpegdec tries to preserve these in its output, causing colorimetry
            // mismatch with downstream videoconvert and failing negotiation silently.
            // Strip these fields before they reach the decoder.
            if let Some(src_pad) = capsfilter.static_pad("src") {
                src_pad.add_probe(
                    gst::PadProbeType::EVENT_DOWNSTREAM,
                    move |_pad, info| {
                        if let Some(gst::PadProbeData::Event(ref event)) = info.data {
                            if let gst::EventView::Caps(caps_ev) = event.view() {
                                let caps = caps_ev.caps();
                                if let Some(s) = caps.structure(0) {
                                    if s.name().as_str() == "image/jpeg"
                                        && (s.has_field("colorimetry")
                                            || s.has_field("pixel-aspect-ratio"))
                                    {
                                        let mut builder = gst::Caps::builder("image/jpeg");
                                        for (field_name, value) in s.iter() {
                                            match field_name.as_str() {
                                                "colorimetry" | "pixel-aspect-ratio" => continue,
                                                _ => {
                                                    builder =
                                                        builder.field(field_name, value.to_owned());
                                                }
                                            }
                                        }
                                        let clean_caps = builder.build();
                                        println!(
                                            "[Video]   Stripped non-standard JPEG caps: {} -> {}",
                                            caps, clean_caps
                                        );
                                        let new_event = gst::event::Caps::new(&clean_caps);
                                        info.data = Some(gst::PadProbeData::Event(new_event));
                                    }
                                }
                            }
                        }
                        gst::PadProbeReturn::Ok
                    },
                );
            }

            // H.264-as-raw workaround: some capture cards (e.g. Elgato) advertise
            // H.264 streams as video/x-raw,format=H264 via ksvideosrc. The decoder
            // expects video/x-h264 caps. Insert capssetter to rewrite the media type
            // and h264parse to properly parse the byte stream before the decoder.
            let is_h264_as_raw = source_format == "H264"
                && input_caps
                    .structure(0)
                    .map(|s| s.name().as_str() == "video/x-raw")
                    .unwrap_or(false);

            if is_h264_as_raw {
                let h264_caps = gst::Caps::builder("video/x-h264")
                    .field("stream-format", "byte-stream")
                    .build();
                let capssetter = gst::ElementFactory::make("capssetter")
                    .property("caps", &h264_caps)
                    .property("join", false)
                    .property("replace", true)
                    .build()
                    .map_err(|e| {
                        VideoError::Pipeline(format!("Failed to create capssetter: {}", e))
                    })?;
                let h264parse = gst::ElementFactory::make("h264parse")
                    .build()
                    .map_err(|e| {
                        VideoError::Pipeline(format!("Failed to create h264parse: {}", e))
                    })?;
                println!("[Video]   Inserting capssetter + h264parse for H.264-as-raw source");
                elements.push(capssetter);
                elements.push(h264parse);
            }

            let decoder = gst::ElementFactory::make(decoder_name)
                .build()
                .map_err(|e| {
                    VideoError::Pipeline(format!(
                        "Failed to create decoder {}: {}",
                        decoder_name, e
                    ))
                })?;
            println!("[Video]   Inserting decoder: {}", decoder_name);

            // Diagnostic: count buffers entering and leaving the decoder
            let dec_name = decoder_name.to_string();
            if let Some(sink_pad) = decoder.static_pad("sink") {
                let counter = Arc::new(AtomicU64::new(0));
                let counter_clone = counter.clone();
                let name = dec_name.clone();
                sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, _info| {
                    let n = counter_clone.fetch_add(1, Ordering::Relaxed);
                    if n < 3 {
                        println!("[Video]   {} sink: received buffer #{}", name, n + 1);
                    }
                    gst::PadProbeReturn::Ok
                });
            }
            if let Some(src_pad) = decoder.static_pad("src") {
                let counter = Arc::new(AtomicU64::new(0));
                let counter_clone = counter.clone();
                let name = dec_name.clone();
                src_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, _info| {
                    let n = counter_clone.fetch_add(1, Ordering::Relaxed);
                    if n < 3 {
                        println!("[Video]   {} src: produced buffer #{}", name, n + 1);
                    }
                    gst::PadProbeReturn::Ok
                });
            }

            elements.push(decoder);
        }

        // Video converter to normalize format
        let videoconvert = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create videoconvert: {}", e)))?;
        elements.push(videoconvert);

        // Force output to a format suitable for encoding.
        // AV1 always uses P010_10LE (10-bit); FFV1 uses it when user selects 10-bit;
        // everything else uses NV12 (8-bit).
        let effective_codec = encoding_codec.unwrap_or_else(|| crate::encoding::get_recommended_codec());
        let intermediate_fmt = crate::encoding::intermediate_format_for_codec(effective_codec, video_bit_depth);
        println!(
            "[Video] source_format={}, intermediate_format={}, encoding_codec={:?}",
            source_format, intermediate_fmt, effective_codec
        );
        let output_caps = gst::Caps::builder("video/x-raw")
            .field("format", intermediate_fmt)
            .build();

        let output_capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", output_caps)
            .build()
            .map_err(|e| {
                VideoError::Pipeline(format!("Failed to create output capsfilter: {}", e))
            })?;
        elements.push(output_capsfilter);

        // Queue for buffering with larger size for raw video
        let queue = gst::ElementFactory::make("queue")
            .property("max-size-buffers", 30u32)
            .property("max-size-bytes", 100_000_000u32) // 100MB
            .property_from_str("leaky", "downstream")
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create queue: {}", e)))?;
        elements.push(queue);

        // App sink to pull frames
        let appsink = gst_app::AppSink::builder()
            .name("sink")
            .max_buffers(2)
            .drop(true)
            .sync(false)
            .build();
        elements.push(appsink.clone().upcast());

        // Add all elements to pipeline and link
        let element_refs: Vec<&gst::Element> = elements.iter().collect();
        pipeline
            .add_many(&element_refs)
            .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;
        gst::Element::link_many(&element_refs)
            .map_err(|e| VideoError::Pipeline(format!("Failed to link pipeline: {}", e)))?;

        println!(
            "[Video] Encoding capture pipeline created for {} (device {}, source: {})",
            device_name,
            device_index,
            source_format
        );

        // Create pre-roll buffer for raw frames.
        // When encode_during_preroll is active, this is just a 1-second staging buffer
        // that poll() drains every ~10ms to feed the continuous encoder.
        // When inactive, this is the full pre-roll buffer sized for raw video up to 8K,
        // with 0.5s headroom to compensate for frame timing jitter.
        const RAW_BYTES_PER_SEC: usize = 3840 * 2160 * 3 / 2 * 60;
        let raw_buffer_secs = if encode_during_preroll {
            1
        } else {
            pre_roll_secs
        };
        let headroom_secs = if encode_during_preroll { 0.0 } else { 0.5 };
        let preroll_buffer = Arc::new(Mutex::new(VideoPrerollBuffer::with_headroom(
            raw_buffer_secs,
            RAW_BYTES_PER_SEC,
            headroom_secs,
        )));

        // Shared flag: the appsink callback skips frame allocation when false.
        // True when pre_roll_secs > 0 or recording is active.
        let needs_frames = Arc::new(AtomicBool::new(pre_roll_secs > 0));

        // Set up appsink callback to fill pre-roll buffer
        let preroll_clone = preroll_buffer.clone();
        let needs_frames_clone = needs_frames.clone();
        let frame_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let frame_counter_clone = frame_counter.clone();
        // Compute default frame duration from source fps (fallback when buffer lacks duration metadata)
        let default_duration_ns = (1_000_000_000.0 / source_fps).round() as u64;

        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    // Always pull sample and count frames (for health check monitoring)
                    match sink.pull_sample() {
                        Ok(sample) => {
                            frame_counter_clone.fetch_add(1, Ordering::Relaxed);

                            if !needs_frames_clone.load(Ordering::Relaxed) {
                                // Discard: no pre-roll needed and not recording
                                return Ok(gst::FlowSuccess::Ok);
                            }

                            if let Some(buffer) = sample.buffer() {
                                let pts = buffer.pts().map(|t| t.nseconds()).unwrap_or(0);
                                let duration = buffer
                                    .duration()
                                    .map(|t| t.nseconds())
                                    .unwrap_or(default_duration_ns);

                                // Get pixel format from caps
                                let pixel_format = sample
                                    .caps()
                                    .and_then(|caps| caps.structure(0))
                                    .and_then(|s| s.get::<String>("format").ok());

                                if let Ok(map) = buffer.map_readable() {
                                    let data = map.as_slice().to_vec();

                                    let frame = BufferedFrame {
                                        data,
                                        pts,
                                        duration,
                                        wall_time: Instant::now(),
                                        pixel_format: pixel_format.clone(),
                                        is_delta_unit: false, // Not relevant for raw capture
                                    };
                                    preroll_clone.lock().push(frame);
                                }
                            }
                            Ok(gst::FlowSuccess::Ok)
                        }
                        Err(_) => Err(gst::FlowError::Error),
                    }
                })
                .build(),
        );

        Ok(Self {
            device_id: format!("webcam-{}", device_index),
            device_name,
            source_format: source_format.to_string(),
            pipeline,
            appsink,
            preroll_buffer,
            recording_path: None,
            recording_start: None,
            pts_offset: None,
            frames_written: 0,
            width: source_width,
            height: source_height,
            fps: source_fps,
            is_recording: false,
            file_writer: None,
            raw_encoder: None,
            is_encoding: true,
            encoding_codec,
            encoder_type: encoder_type_hint,
            pixel_format: Some(intermediate_fmt.to_string()),
            consecutive_full_drops: 0,
            total_frames_dropped: 0,
            preset_level,
            effort_level: crate::encoding::DEFAULT_PRESET, // Set by caller via VideoManager
            video_bit_depth,
            encode_during_preroll,
            pre_roll_secs,
            needs_frames,
            preroll_encoder: None, // Created in start() after cap negotiation
            preroll_encoder_output: None, // Created in start() after cap negotiation
            target_width: source_width, // Will be overridden by caller if target differs
            target_height: source_height,
            target_fps: source_fps,
            frame_counter,
            fps_check_start: Instant::now(),
            frames_at_last_check: 0,
            fps_warning_emitted: false,
        })
    }

    /// Start the capture pipeline (begins filling pre-roll buffer)
    pub fn start(&mut self) -> Result<()> {
        self.pipeline.set_state(gst::State::Playing)?;
        println!("[Video] Started capture pipeline for {}", self.device_name);

        // Query the negotiated caps to get actual resolution.
        // USB cameras need time to initialize, especially after a pipeline restart
        // (camera device must be released and reacquired by the OS). Decoders like
        // jpegdec add further latency since they need actual data before negotiating
        // output caps. Allow up to 20 attempts (5 seconds total).
        let mut negotiated = false;
        for attempt in 1..=20 {
            std::thread::sleep(std::time::Duration::from_millis(250));

            if let Some(pad) = self.appsink.static_pad("sink") {
                if let Some(caps) = pad.current_caps() {
                    if let Some(structure) = caps.structure(0) {
                        self.width = structure.get::<i32>("width").unwrap_or(1280) as u32;
                        self.height = structure.get::<i32>("height").unwrap_or(720) as u32;
                        self.fps = structure
                            .get::<gst::Fraction>("framerate")
                            .map(|f| {
                                let numer = f.numer() as f64;
                                let denom = (f.denom() as f64).max(1.0);
                                numer / denom
                            })
                            .unwrap_or(30.0);

                        println!(
                            "[Video]   Negotiated caps: {}x{} @ {:.2}fps (attempt {})",
                            self.width, self.height, self.fps, attempt
                        );

                        negotiated = true;
                        break;
                    }
                }
            }

            // Check bus for errors during negotiation
            if let Some(bus) = self.pipeline.bus() {
                while let Some(msg) = bus.pop_filtered(&[
                    gst::MessageType::Error,
                    gst::MessageType::Warning,
                    gst::MessageType::StateChanged,
                ]) {
                    match msg.view() {
                        gst::MessageView::Error(err) => {
                            let src = err.src().map(|s| s.name().to_string()).unwrap_or_default();
                            println!(
                                "[Video]   BUS ERROR (attempt {}): '{}': {} (debug: {:?})",
                                attempt, src, err.error(), err.debug()
                            );
                        }
                        gst::MessageView::Warning(warn) => {
                            let src = warn.src().map(|s| s.name().to_string()).unwrap_or_default();
                            println!(
                                "[Video]   BUS WARNING (attempt {}): '{}': {}",
                                attempt, src, warn.error()
                            );
                        }
                        _ => {}
                    }
                }
            }

            if attempt < 20 {
                println!(
                    "[Video]   Cap negotiation attempt {}/20 failed for {}, retrying...",
                    attempt, self.device_name
                );
            }
        }

        if !negotiated {
            // Dump per-element state and pad caps BEFORE stopping the pipeline
            println!("[Video] === Pipeline negotiation diagnostics for {} ===", self.device_name);
            for element in self.pipeline.iterate_elements().into_iter().flatten() {
                let name = element.name().to_string();
                let (_, state, _) = element.state(Some(gst::ClockTime::from_mseconds(10)));
                println!("[Video]   Element '{}': state={:?}", name, state);
                for pad in element.pads() {
                    let pad_name = pad.name().to_string();
                    let caps_str = pad.current_caps()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "NOT NEGOTIATED".to_string());
                    println!("[Video]     pad '{}': {}", pad_name, caps_str);
                }
            }
            // Check bus for errors before stopping
            if let Some(bus) = self.pipeline.bus() {
                while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error, gst::MessageType::Warning]) {
                    match msg.view() {
                        gst::MessageView::Error(err) => {
                            let src = err.src().map(|s| s.name().to_string()).unwrap_or_default();
                            println!("[Video]   BUS ERROR from '{}': {} (debug: {:?})", src, err.error(), err.debug());
                        }
                        gst::MessageView::Warning(warn) => {
                            let src = warn.src().map(|s| s.name().to_string()).unwrap_or_default();
                            println!("[Video]   BUS WARNING from '{}': {}", src, warn.error());
                        }
                        _ => {}
                    }
                }
            }
            println!("[Video] === End diagnostics ===");

            // Stop the pipeline since it can't produce valid output
            self.pipeline.set_state(gst::State::Null).ok();

            let mut error_details = String::new();

            // Check if the pipeline is in an error state
            let (state_result, current, pending) = self
                .pipeline
                .state(Some(gst::ClockTime::from_mseconds(100)));
            error_details.push_str(&format!(
                "Pipeline state: {:?}, current: {:?}, pending: {:?}. ",
                state_result, current, pending
            ));

            // Check bus for error messages
            if let Some(bus) = self.pipeline.bus() {
                while let Some(msg) =
                    bus.pop_filtered(&[gst::MessageType::Error, gst::MessageType::Warning])
                {
                    match msg.view() {
                        gst::MessageView::Error(err) => {
                            let src_name =
                                err.src().map(|s| s.name().to_string()).unwrap_or_default();
                            error_details.push_str(&format!(
                                "GStreamer ERROR from {}: {}. ",
                                src_name,
                                err.error()
                            ));
                            if let Some(debug) = err.debug() {
                                error_details.push_str(&format!("Debug: {}. ", debug));
                            }
                        }
                        gst::MessageView::Warning(warn) => {
                            let src_name =
                                warn.src().map(|s| s.name().to_string()).unwrap_or_default();
                            error_details.push_str(&format!(
                                "GStreamer WARNING from {}: {}. ",
                                src_name,
                                warn.error()
                            ));
                        }
                        _ => {}
                    }
                }
            }

            return Err(VideoError::Pipeline(format!(
                "Pipeline for {} did not negotiate caps after 5000ms ({}x{} @ {:.2}fps, codec: {}). {}",
                self.device_name, self.width, self.height, self.fps, self.source_format, error_details
            )));
        }

        // Reset FPS measurement now that capture has actually started
        self.fps_check_start = Instant::now();
        self.frames_at_last_check = 0;
        self.fps_warning_emitted = false;

        // Create the pre-roll encoder if encode_during_preroll is active.
        // This must happen after cap negotiation so we know the actual dimensions.
        // Skip if pre_roll_secs is 0 — no point encoding frames that will be
        // immediately discarded from the ring buffer.
        if self.encode_during_preroll && self.pre_roll_secs > 0 && self.is_encoding {
            // Drop any previous encoder (e.g., from a previous start/stop cycle)
            self.preroll_encoder = None;
            self.preroll_encoder_output = None;

            let target_codec = self
                .encoding_codec
                .unwrap_or_else(|| crate::encoding::get_recommended_codec());

            // Compute target dimensions for preroll encoder
            let pe_tw = if self.target_width != self.width {
                Some(self.target_width)
            } else {
                None
            };
            let pe_th = if self.target_height != self.height {
                Some(self.target_height)
            } else {
                None
            };
            let pe_tf = if (self.target_fps - self.fps).abs() > 0.01 {
                Some(self.target_fps)
            } else {
                None
            };

            match PrerollVideoEncoder::new(
                self.width,
                self.height,
                self.fps,
                target_codec,
                self.preset_level,
                self.effort_level,
                self.video_bit_depth,
                self.pre_roll_secs,
                pe_tw,
                pe_th,
                pe_tf,
            ) {
                Ok(encoder) => {
                    let output = encoder.output.clone();
                    self.preroll_encoder = Some(encoder);
                    self.preroll_encoder_output = Some(output);
                    println!(
                        "[Video] PrerollVideoEncoder started for {} ({}x{} @ {}fps -> {})",
                        self.device_name,
                        self.width,
                        self.height,
                        self.fps,
                        target_codec.display_name()
                    );
                }
                Err(e) => {
                    println!("[Video] Warning: Failed to create PrerollVideoEncoder: {}. Falling back to raw pre-roll.", e);
                    self.encode_during_preroll = false;
                    // Expand the 1-second staging buffer to the full pre-roll duration
                    self.preroll_buffer.lock().set_duration(self.pre_roll_secs);
                }
            }
        }

        Ok(())
    }

    /// Stop the capture pipeline
    pub fn stop(&self) -> Result<()> {
        self.pipeline.set_state(gst::State::Null)?;
        println!("[Video] Stopped capture pipeline for {}", self.device_name);
        Ok(())
    }

    /// Start recording to a file
    /// Returns the pre-roll duration that was captured
    pub fn start_recording(&mut self, mut output_path: PathBuf) -> Result<Duration> {
        if self.is_recording {
            return Err(VideoError::Pipeline("Already recording".to_string()));
        }

        // For encoding pipelines, determine the actual output format
        if self.is_encoding {
            let target_codec = self
                .encoding_codec
                .unwrap_or_else(|| crate::encoding::get_recommended_codec());
            output_path = output_path.with_extension(target_codec.container().extension());
            println!(
                "[Video] Starting recording to {:?} ({} -> {})",
                output_path,
                self.source_format,
                target_codec.display_name()
            );
        } else {
            println!(
                "[Video] Starting recording to {:?} (format: {})",
                output_path,
                self.source_format
            );
        }

        // Drain pre-roll buffer. When pre-roll is disabled, discard any stale
        // frames that may have leaked in (race between appsink and needs_frames flag).
        let mut preroll_frames = if self.pre_roll_secs == 0 {
            let _ = self.preroll_buffer.lock().drain(); // discard stale frames
            Vec::new()
        } else {
            self.preroll_buffer.lock().drain()
        };

        // H.264 uses I/P/B frames — the file must start at a keyframe.
        // Strip leading delta frames so the muxer gets a clean GOP start.
        if self.source_format == "H264" {
            let before = preroll_frames.len();
            while preroll_frames.first().map(|f| f.is_delta_unit).unwrap_or(false) {
                preroll_frames.remove(0);
            }
            if before != preroll_frames.len() {
                println!(
                    "[Video] H.264: stripped {} leading delta frames for keyframe alignment",
                    before - preroll_frames.len()
                );
            }
        }

        println!(
            "[Video] Pre-roll buffer has {} frames",
            preroll_frames.len()
        );

        // Calculate pre-roll duration as time from FIRST frame capture to NOW
        // This is the correct reference for syncing with audio/MIDI
        // (Previously we used last-first span, but that doesn't account for
        // the delay between video processing and audio processing)
        let preroll_duration = preroll_frames
            .first()
            .map(|f| f.wall_time.elapsed())
            .unwrap_or(Duration::ZERO);

        // Set PTS offset from the first pre-roll frame. If there are no pre-roll
        // frames (e.g. recording started before buffer filled), leave as None so
        // the first frame arriving in poll() will set it. This ensures MKV
        // timestamps always start at 0.
        self.pts_offset = preroll_frames.first().map(|f| f.pts);

        // Handle raw vs pre-encoded video differently
        if self.encode_during_preroll && self.preroll_encoder_output.is_some() {
            // ── Encode-during-preroll path ──────────────────────────────────
            // The PrerollVideoEncoder has been continuously encoding. We drain
            // its encoded ring buffer, write those frames to a new VideoWriter,
            // then switch the encoder's output to the writer for live frames.
            let target_codec = self
                .preroll_encoder_output
                .as_ref()
                .unwrap()
                .lock()
                .target_codec;

            // Create the writer OUTSIDE the lock (pipeline creation takes a moment)
            // Use target dimensions since the preroll encoder outputs at target resolution/fps
            let mut writer = VideoWriter::new(
                &output_path,
                target_codec,
                self.target_width,
                self.target_height,
                self.target_fps,
            )?;

            // Lock the output, drain, write pre-roll, and atomically switch to recording
            let mut output = self.preroll_encoder_output.as_ref().unwrap().lock();
            let encoded_frames = output.drain();

            // Calculate pre-roll duration from encoded frames
            let preroll_duration = encoded_frames
                .first()
                .map(|f| f.wall_time.elapsed())
                .unwrap_or(Duration::ZERO);

            println!(
                "[Video] Encode-during-preroll: {} encoded frames in ring buffer ({:?})",
                encoded_frames.len(),
                preroll_duration
            );

            // Set PTS offset from the first encoded pre-roll frame
            let pts_offset = encoded_frames.first().map(|f| f.pts);
            output.pts_offset = pts_offset;

            // Write all pre-roll frames to the writer
            for frame in &encoded_frames {
                if let Err(e) = writer.write_frame(frame, pts_offset) {
                    println!(
                        "[Video] Warning: Failed to write pre-roll encoded frame: {}",
                        e
                    );
                }
            }

            // Switch: new encoded frames from the appsink callback go to the writer
            output.active_writer = Some(writer);
            drop(output); // Explicitly release the lock

            self.raw_encoder = None;
            self.file_writer = None; // Writer is inside PrerollEncoderOutput
            self.recording_path = Some(output_path);
            self.recording_start = Some(Instant::now());
            self.frames_written = encoded_frames.len() as u64;
            self.is_recording = true;
            self.needs_frames.store(true, Ordering::Relaxed);
            self.consecutive_full_drops = 0;
            self.total_frames_dropped = 0;

            println!(
                "[Video] Started recording (encode-during-preroll), pre-roll: {:?}",
                preroll_duration
            );

            return Ok(preroll_duration);
        } else if self.is_encoding {
            let target_codec = self
                .encoding_codec
                .unwrap_or_else(|| crate::encoding::get_recommended_codec());

            // Encoding pipeline - use async encoder
            // Use target dimensions if they differ from source
            let use_target_w = if self.target_width != self.width {
                Some(self.target_width)
            } else {
                None
            };
            let use_target_h = if self.target_height != self.height {
                Some(self.target_height)
            } else {
                None
            };
            let use_target_fps = if (self.target_fps - self.fps).abs() > 0.01 {
                Some(self.target_fps)
            } else {
                None
            };

            let encoder_config = EncoderConfig {
                keyframe_interval: (self.target_fps * 2.0).round() as u32, // Keyframe every 2 seconds at target fps
                target_codec,
                preset_level: self.preset_level,
                effort_level: self.effort_level,
                video_bit_depth: self.video_bit_depth,
                target_width: use_target_w,
                target_height: use_target_h,
                target_fps: use_target_fps,
            };

            // Create encoder with buffer size of ~2 seconds of frames for backpressure
            let buffer_size = (self.fps * 2.0) as usize;
            let encoder = if let Some(hw_type) = self.encoder_type {
                AsyncVideoEncoder::new_with_encoder(
                    output_path.clone(),
                    self.width,
                    self.height,
                    self.fps,
                    encoder_config,
                    buffer_size,
                    hw_type,
                )
            } else {
                AsyncVideoEncoder::new(
                    output_path.clone(),
                    self.width,
                    self.height,
                    self.fps,
                    encoder_config,
                    buffer_size,
                )
            }
            .map_err(|e| VideoError::Pipeline(format!("Failed to create encoder: {}", e)))?;

            // Send pre-roll frames to encoder
            let pixel_format = self
                .pixel_format
                .clone()
                .unwrap_or_else(|| "NV12".to_string());
            for frame in &preroll_frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: self.width,
                    height: self.height,
                    format: frame
                        .pixel_format
                        .clone()
                        .unwrap_or_else(|| pixel_format.clone()),
                    capture_time: frame.wall_time,
                };

                // Use blocking send for pre-roll since we need all frames
                if let Err(e) = encoder.send_frame(raw_frame) {
                    println!("[Video] Warning: Failed to send pre-roll frame: {}", e);
                }
            }

            self.raw_encoder = Some(encoder);
            self.file_writer = None;
        } else {
            // Pre-encoded video - use passthrough writer
            // Map the source format string to the VideoCodec enum for the writer/muxer.
            let (writer_caps_name, _) = crate::encoding::format_to_gst_caps(&self.source_format);
            let writer_codec = crate::encoding::VideoCodec::from_gst_caps_name(writer_caps_name)
                .unwrap_or(crate::encoding::VideoCodec::Mjpeg);
            let mut writer =
                VideoWriter::new(&output_path, writer_codec, self.width, self.height, self.fps)?;

            // Write pre-roll frames
            for frame in &preroll_frames {
                writer.write_frame(frame, self.pts_offset)?;
            }

            self.file_writer = Some(writer);
            self.raw_encoder = None;
        }

        self.recording_path = Some(output_path);
        self.recording_start = Some(Instant::now());
        self.frames_written = preroll_frames.len() as u64;
        self.is_recording = true;
        self.needs_frames.store(true, Ordering::Relaxed);
        self.consecutive_full_drops = 0;
        self.total_frames_dropped = 0;

        println!(
            "[Video] Started recording, pre-roll: {:?}",
            preroll_duration
        );

        Ok(preroll_duration)
    }

    /// Stop recording and finalize the file
    pub fn stop_recording(&mut self) -> Result<VideoFileInfo> {
        if !self.is_recording {
            return Err(VideoError::Pipeline("Not recording".to_string()));
        }

        // Drain any remaining frames from pre-roll buffer
        let remaining_frames = self.preroll_buffer.lock().drain();

        let (duration, file_size) = if self.encode_during_preroll
            && self.preroll_encoder_output.is_some()
        {
            // ── Encode-during-preroll path ──────────────────────────────────
            // Take the writer out of PrerollEncoderOutput (resumes ring buffer mode),
            // then feed remaining raw frames and finalize the writer.

            // First, push remaining raw frames to the preroll encoder so they get encoded
            if let Some(ref encoder) = self.preroll_encoder {
                for frame in &remaining_frames {
                    encoder.push_frame(frame);
                }
            }

            // Brief pause to let the encoder process the last frames
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Take the writer out of the output (atomically switches back to ring buffer)
            let writer = {
                let mut output = self.preroll_encoder_output.as_ref().unwrap().lock();
                output.active_writer.take()
                // pts_offset is left as-is; it will be reset on next recording start
            };

            if let Some(writer) = writer {
                writer.finish()?
            } else {
                return Err(VideoError::Pipeline(
                    "No active writer in PrerollEncoderOutput".to_string(),
                ));
            }
        } else if let Some(encoder) = self.raw_encoder.take() {
            // Raw video with encoding
            let pixel_format = self
                .pixel_format
                .clone()
                .unwrap_or_else(|| "NV12".to_string());

            // Send remaining frames to encoder
            for frame in &remaining_frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: self.width,
                    height: self.height,
                    format: frame
                        .pixel_format
                        .clone()
                        .unwrap_or_else(|| pixel_format.clone()),
                    capture_time: frame.wall_time,
                };

                // Use non-blocking send, drop frames if encoder can't keep up
                if let Ok(false) = encoder.try_send_frame(raw_frame) {
                    println!("[Video] Warning: Dropped frame during stop (encoder backpressure)");
                }
            }
            self.frames_written += remaining_frames.len() as u64;

            // Finish encoding
            let stats = encoder
                .finish()
                .map_err(|e| VideoError::Pipeline(format!("Failed to finish encoding: {}", e)))?;

            (stats.content_duration, stats.bytes_written)
        } else if let Some(mut writer) = self.file_writer.take() {
            // Pre-encoded video
            for frame in &remaining_frames {
                let _ = writer.write_frame(frame, self.pts_offset);
            }
            self.frames_written += remaining_frames.len() as u64;

            writer.finish()?
        } else {
            return Err(VideoError::Pipeline(
                "No active writer or encoder".to_string(),
            ));
        };

        let filename = self
            .recording_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("video.mkv")
            .to_string();

        self.is_recording = false;
        self.needs_frames
            .store(self.pre_roll_secs > 0, Ordering::Relaxed);
        // When pre-roll is disabled, clear any frames that arrived between the
        // drain at the top of stop_recording and needs_frames being set to false.
        // Without this, stale frames linger (trim is a no-op for max_duration=0)
        // and get picked up by the next start_recording, inflating its preroll_duration.
        if self.pre_roll_secs == 0 {
            let _ = self.preroll_buffer.lock().drain();
        }
        self.recording_path = None;
        self.recording_start = None;

        println!(
            "[Video] Stopped recording {}, duration: {:?}, size: {} bytes",
            filename, duration, file_size
        );

        Ok(VideoFileInfo {
            filename,
            device_name: self.device_name.clone(),
            duration_secs: duration.as_secs_f64(),
        })
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.is_recording
    }

    /// Get pre-roll buffer duration
    pub fn preroll_duration(&self) -> Duration {
        if self.encode_during_preroll {
            if let Some(ref output) = self.preroll_encoder_output {
                return output.lock().duration();
            }
        }
        self.preroll_buffer.lock().duration()
    }

    /// Drain all frames from the pre-roll buffer.
    /// Used by the auto-select system to feed frames to a test encoder.
    pub fn drain_preroll_frames(&self) -> Vec<BufferedFrame> {
        self.preroll_buffer.lock().drain()
    }

    /// Set pre-roll duration
    pub fn set_preroll_duration(&mut self, secs: u32) {
        self.pre_roll_secs = secs;
        // Update needs_frames: if not recording, only buffer when pre_roll > 0
        if !self.is_recording {
            self.needs_frames.store(secs > 0, Ordering::Relaxed);
        }
        if self.encode_during_preroll {
            // Raw buffer stays at 1 second (staging only)
            // Update the encoded pre-roll buffer's duration
            if let Some(ref output) = self.preroll_encoder_output {
                let clamped = secs.min(MAX_PRE_ROLL_SECS_ENCODED);
                output.lock().max_duration = Duration::from_secs(clamped as u64);
            }
        } else {
            self.preroll_buffer.lock().set_duration(secs);
        }
    }

    /// Set the target resolution and fps for encoding (may differ from source).
    pub fn set_target_resolution(&mut self, width: u32, height: u32, fps: f64) {
        self.target_width = width;
        self.target_height = height;
        self.target_fps = fps;
    }

    /// Check if the device is delivering frames at a significantly lower rate
    /// than the negotiated framerate. Returns a warning once after 5 seconds of
    /// steady frame delivery (excludes startup latency).
    pub fn check_fps_mismatch(&mut self) -> Option<VideoFpsWarning> {
        if self.fps_warning_emitted {
            return None;
        }

        let total_frames = self.frame_counter.load(Ordering::Relaxed);
        if total_frames == 0 {
            return None;
        }

        // Start the measurement window from the first frame, not pipeline start
        if self.frames_at_last_check == 0 {
            self.fps_check_start = Instant::now();
            self.frames_at_last_check = total_frames;
            return None;
        }

        let elapsed = self.fps_check_start.elapsed();
        if elapsed < Duration::from_secs(5) {
            return None;
        }

        let frames_in_window = total_frames - self.frames_at_last_check;
        let actual_fps = frames_in_window as f64 / elapsed.as_secs_f64();

        // Warn if actual fps is less than 75% of expected
        if actual_fps < self.fps * 0.75 {
            self.fps_warning_emitted = true;
            println!(
                "[Video] FPS mismatch warning for {}: {:.1} actual vs {:.0} expected",
                self.device_name, actual_fps, self.fps
            );
            Some(VideoFpsWarning {
                device_name: self.device_name.clone(),
                actual_fps: (actual_fps * 10.0).round() / 10.0, // Round to 1 decimal
                expected_fps: self.fps,
            })
        } else {
            None
        }
    }

    /// Poll for new frames and write to file if recording
    /// This should be called periodically from a background thread
    pub fn poll(&mut self) -> Result<()> {
        // When encode_during_preroll is active, we always drain the raw staging
        // buffer and feed frames to the PrerollVideoEncoder -- whether recording
        // or not. The encoder's appsink callback handles routing to the ring
        // buffer (pre-roll) or the active VideoWriter (recording).
        if self.encode_during_preroll && self.preroll_encoder.is_some() {
            if let Some(ref encoder) = self.preroll_encoder {
                let frames = self.preroll_buffer.lock().drain();
                for frame in &frames {
                    encoder.push_frame(frame);
                }
            }
            return Ok(());
        }

        if !self.is_recording {
            return Ok(());
        }

        // Drain accumulated frames
        let frames = self.preroll_buffer.lock().drain();

        if let Some(ref encoder) = self.raw_encoder {
            // Raw video - send to encoder (non-blocking)
            let pixel_format = self
                .pixel_format
                .clone()
                .unwrap_or_else(|| "NV12".to_string());
            let mut frames_sent = 0u64;
            let mut frames_dropped = 0u64;

            for frame in &frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: self.width,
                    height: self.height,
                    format: frame
                        .pixel_format
                        .clone()
                        .unwrap_or_else(|| pixel_format.clone()),
                    capture_time: frame.wall_time,
                };

                // Use non-blocking send to avoid blocking capture
                match encoder.try_send_frame(raw_frame) {
                    Ok(true) => frames_sent += 1,
                    Ok(false) => frames_dropped += 1, // Buffer full, frame dropped
                    Err(e) => {
                        println!("[Video] Encoder error: {}", e);
                        return Err(VideoError::Pipeline(format!("Encoder error: {}", e)));
                    }
                }
            }

            self.frames_written += frames_sent;
            self.total_frames_dropped += frames_dropped;

            if frames_dropped > 0 {
                // Track consecutive polls where ALL frames were dropped (encoder stalled)
                if frames_sent == 0 && !frames.is_empty() {
                    self.consecutive_full_drops += 1;
                } else {
                    self.consecutive_full_drops = 0;
                }

                // Rate-limit warnings: log first, then every 30th occurrence
                if self.total_frames_dropped == frames_dropped
                    || self.total_frames_dropped % 30 == 0
                {
                    println!("[Video] Warning: Dropped {} frames this poll ({} total) due to encoder backpressure",
                        frames_dropped, self.total_frames_dropped);
                }

                // If encoder has been completely stalled for ~5 seconds (e.g., 150 polls at ~30ms),
                // it's dead — abort gracefully instead of leaking memory
                if self.consecutive_full_drops > 150 {
                    println!("[Video] ERROR: Encoder stalled for too long ({} consecutive polls with 0 frames accepted, {} total dropped). Aborting.",
                        self.consecutive_full_drops, self.total_frames_dropped);
                    // Drop the encoder to clean up its resources
                    self.raw_encoder = None;
                    self.is_recording = false;
                    self.needs_frames
                        .store(self.pre_roll_secs > 0, Ordering::Relaxed);
                    return Err(VideoError::Pipeline(
                        "Encoder stalled, recording aborted".to_string(),
                    ));
                }
            } else if !frames.is_empty() {
                self.consecutive_full_drops = 0;
            }
        } else if let Some(ref mut writer) = self.file_writer {
            // Pre-encoded video - write directly
            // Set pts_offset from first frame if not yet initialized (no pre-roll case)
            if self.pts_offset.is_none() {
                if let Some(first) = frames.first() {
                    self.pts_offset = Some(first.pts);
                }
            }
            for frame in &frames {
                writer.write_frame(frame, self.pts_offset)?;
            }
            self.frames_written += frames.len() as u64;
        }

        Ok(())
    }
}

impl Drop for VideoCapturePipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// Manages all video capture pipelines
pub struct VideoCaptureManager {
    /// Active pipelines by device ID
    pipelines: HashMap<String, VideoCapturePipeline>,
    /// Pre-roll duration in seconds
    pre_roll_secs: u32,
    /// Is currently recording
    is_recording: bool,
    /// Whether to encode video during pre-roll (encoding pipelines only)
    encode_during_preroll: bool,
}

impl VideoCaptureManager {
    /// Create a new video capture manager
    pub fn new(pre_roll_secs: u32) -> Self {
        // Initialize GStreamer
        if let Err(e) = gst::init() {
            println!("[Video] Warning: Failed to initialize GStreamer: {}", e);
        }

        Self {
            pipelines: HashMap::new(),
            pre_roll_secs,
            is_recording: false,
            encode_during_preroll: false,
        }
    }

    /// Set whether to encode video during pre-roll (encoding pipelines only)
    pub fn set_encode_during_preroll(&mut self, enabled: bool) {
        self.encode_during_preroll = enabled;
    }

    /// Update the encoder preset level and effort level for a specific device (in-place, no pipeline restart).
    pub fn update_preset_for_device(&mut self, device_id: &str, level: u8, effort_level: u8) {
        let clamped = level.clamp(crate::encoding::MIN_PRESET, crate::encoding::MAX_PRESET);
        let effort_clamped = effort_level.clamp(crate::encoding::MIN_PRESET, crate::encoding::MAX_PRESET);
        if let Some(pipeline) = self.pipelines.get_mut(device_id) {
            pipeline.preset_level = clamped;
            pipeline.effort_level = effort_clamped;
        }
    }

    /// Start capturing from specified devices with their per-device configs
    ///
    /// Each tuple is (device_id, device_name, VideoDeviceConfig)
    pub fn start(
        &mut self,
        devices: &[(String, String, crate::config::VideoDeviceConfig)],
    ) -> Result<()> {
        // Stop any existing pipelines
        self.stop();

        for (device_id, device_name, dev_config) in devices {
            // Device index is only used on Linux/macOS; Windows uses device_name
            // For name-based IDs (video-xxx), we don't have an index
            let index = device_id
                .strip_prefix("webcam-")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);

            let source_format = &dev_config.source_format;

            // Create appropriate pipeline based on passthrough setting
            let pipeline_result = if dev_config.passthrough {
                // Passthrough - use direct capture pipeline
                VideoCapturePipeline::new_webcam(
                    index,
                    device_name,
                    device_id,
                    source_format,
                    dev_config.source_width,
                    dev_config.source_height,
                    dev_config.source_fps,
                    self.pre_roll_secs,
                )
            } else {
                // Encoding - decode source and re-encode
                VideoCapturePipeline::new_webcam_raw(
                    index,
                    device_name,
                    device_id,
                    source_format,
                    dev_config.source_width,
                    dev_config.source_height,
                    dev_config.source_fps,
                    self.pre_roll_secs,
                    dev_config.encoding_codec,
                    dev_config.encoder_type,
                    dev_config.preset_level,
                    dev_config.video_bit_depth,
                    self.encode_during_preroll,
                )
            };

            match pipeline_result {
                Ok(mut pipeline) => {
                    // Set target resolution/fps for encoding pipelines
                    // Resolve "Match Source" sentinels (0 / 0.0 → source values)
                    if !dev_config.passthrough {
                        let resolved = dev_config.resolved();
                        pipeline.target_width = resolved.target_width;
                        pipeline.target_height = resolved.target_height;
                        pipeline.target_fps = resolved.target_fps;
                        pipeline.effort_level = dev_config.effort_level;
                    }
                    if let Err(e) = pipeline.start() {
                        println!("[Video] Failed to start pipeline for {}: {}", device_id, e);
                        continue;
                    }
                    self.pipelines.insert(device_id.clone(), pipeline);
                }
                Err(e) => {
                    println!("[Video] Failed to create pipeline for {}: {}", device_id, e);
                }
            }
        }

        println!(
            "[Video] Started {} video capture pipeline(s)",
            self.pipelines.len()
        );
        Ok(())
    }

    /// Stop all capture pipelines
    pub fn stop(&mut self) {
        for (id, pipeline) in self.pipelines.drain() {
            if let Err(e) = pipeline.stop() {
                println!("[Video] Error stopping pipeline {}: {}", id, e);
            }
        }
    }

    /// Start recording on all active pipelines
    pub fn start_recording(&mut self, session_path: &PathBuf) -> Result<Duration> {
        if self.is_recording {
            return Err(VideoError::Pipeline("Already recording".to_string()));
        }

        let mut max_preroll = Duration::ZERO;

        for (device_id, pipeline) in self.pipelines.iter_mut() {
            println!("[Video] Processing recording start for: {}", device_id);

            let safe_name = crate::session::sanitize_device_name(&pipeline.device_name);

            // All output goes to MKV container
            let extension = "mkv";
            let filename = format!("video_{}.{}", safe_name, extension);

            let output_path = session_path.join(&filename);

            match pipeline.start_recording(output_path) {
                Ok(preroll_duration) => {
                    if preroll_duration > max_preroll {
                        max_preroll = preroll_duration;
                    }
                }
                Err(e) => {
                    println!("[Video] Failed to start recording for {}: {}", device_id, e);
                }
            }
        }

        self.is_recording = true;
        Ok(max_preroll)
    }

    /// Stop recording on all active pipelines
    pub fn stop_recording(&mut self) -> Vec<VideoFileInfo> {
        let mut video_files = Vec::new();

        for (device_id, pipeline) in self.pipelines.iter_mut() {
            match pipeline.stop_recording() {
                Ok(info) => {
                    video_files.push(info);
                }
                Err(e) => {
                    println!("[Video] Failed to stop recording for {}: {}", device_id, e);
                }
            }
        }

        self.is_recording = false;
        video_files
    }

    /// Poll all pipelines (call from background thread)
    pub fn poll(&mut self) {
        for (_, pipeline) in self.pipelines.iter_mut() {
            if let Err(e) = pipeline.poll() {
                println!("[Video] Poll error: {}", e);
            }
        }
    }

    /// Collect FPS mismatch warnings from all active pipelines
    pub fn collect_fps_warnings(&mut self) -> Vec<VideoFpsWarning> {
        let mut warnings = Vec::new();
        for (_, pipeline) in self.pipelines.iter_mut() {
            if let Some(warning) = pipeline.check_fps_mismatch() {
                warnings.push(warning);
            }
        }
        warnings
    }

    /// Set pre-roll duration for all pipelines
    pub fn set_preroll_duration(&mut self, secs: u32) {
        self.pre_roll_secs = secs;
        for (_, pipeline) in self.pipelines.iter_mut() {
            pipeline.set_preroll_duration(secs);
        }
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.is_recording
    }

    /// Get number of active pipelines
    pub fn pipeline_count(&self) -> usize {
        self.pipelines.len()
    }

    /// Get frame counts for all active pipelines (for health check monitoring)
    pub fn get_frame_counts(&self) -> HashMap<String, u64> {
        self.pipelines
            .iter()
            .map(|(id, p)| (id.clone(), p.frame_counter.load(Ordering::Relaxed)))
            .collect()
    }

    /// Clear pre-roll buffers for a specific device (on disconnect)
    pub fn clear_preroll_for_device(&mut self, device_id: &str) {
        if let Some(pipeline) = self.pipelines.get_mut(device_id) {
            pipeline.preroll_buffer.lock().clear();
            if let Some(ref output) = pipeline.preroll_encoder_output {
                output.lock().clear();
            }
        }
    }
}

impl Drop for VideoCaptureManager {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Enumerate available video devices using GStreamer's device monitor
pub fn enumerate_video_devices() -> Vec<(String, String)> {
    if gst::init().is_err() {
        return Vec::new();
    }

    let mut devices = Vec::new();

    // Create device monitor for video sources
    let monitor = match gst::DeviceMonitor::new() {
        monitor => monitor,
    };

    // Add filter for video sources
    monitor.add_filter(Some("Video/Source"), None);

    if monitor.start().is_err() {
        return devices;
    }

    for device in monitor.devices() {
        let name = device.display_name().to_string();
        let device_class = device.device_class().to_string();

        // Only include actual video capture devices
        if device_class.contains("Video/Source") {
            // Generate a stable ID based on device properties
            let props = device.properties();
            let device_path = props
                .and_then(|p| p.get::<String>("device.path").ok())
                .unwrap_or_else(|| format!("webcam-{}", devices.len()));

            devices.push((device_path, name));
        }
    }

    monitor.stop();

    devices
}
