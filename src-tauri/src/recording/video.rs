// Video capture using GStreamer
//
// This module provides video recording with pre-roll buffering using GStreamer pipelines.
// Key features:
// - Continuous capture with ring-buffer pre-roll (configurable duration)
// - Passthrough encoding to MKV container (no re-encoding)
// - Non-blocking file I/O through GStreamer's async handling
// - Synchronization support with audio/MIDI streams

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::config::VideoEncodingMode;
use crate::encoding::{AsyncVideoEncoder, EncoderConfig, RawVideoFrame};
use crate::session::VideoFileInfo;

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
}

/// Pre-roll buffer for video frames
/// Maintains a rolling window of recent frames
pub struct VideoPrerollBuffer {
    frames: std::collections::VecDeque<BufferedFrame>,
    max_duration: Duration,
    /// Estimated bytes per second for memory management
    bytes_per_sec: usize,
    /// Maximum buffer size in bytes (to prevent unbounded memory usage)
    max_bytes: usize,
    current_bytes: usize,
}

impl VideoPrerollBuffer {
    pub fn new(max_duration_secs: u32) -> Self {
        // Estimate ~5MB/sec for compressed video (MJPEG at 720p30)
        let bytes_per_sec = 5 * 1024 * 1024;
        let max_bytes = bytes_per_sec * max_duration_secs as usize;
        
        Self {
            frames: std::collections::VecDeque::new(),
            max_duration: Duration::from_secs(max_duration_secs as u64),
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
    
    /// Trim old frames to stay within duration and memory limits
    fn trim(&mut self) {
        let cutoff = Instant::now() - self.max_duration;
        
        // Trim by time
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
    
    /// Drain all frames from the buffer
    pub fn drain(&mut self) -> Vec<BufferedFrame> {
        self.current_bytes = 0;
        self.frames.drain(..).collect()
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
        self.max_bytes = self.bytes_per_sec * secs as usize;
        self.trim();
    }
    
    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// Represents a single video capture pipeline for one device
pub struct VideoCapturePipeline {
    /// Device identifier
    pub device_id: String,
    /// Human-readable device name
    pub device_name: String,
    /// Video codec being captured (Raw means we need to encode)
    pub codec: crate::encoding::VideoCodec,
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
    /// PTS offset for current recording (to normalize timestamps)
    pts_offset: u64,
    /// Frames written during current recording
    frames_written: u64,
    /// Video dimensions
    pub width: u32,
    pub height: u32,
    /// Frame rate
    pub fps: u32,
    /// Is currently recording
    is_recording: bool,
    /// File handle for recording (for pre-encoded video)
    file_writer: Option<VideoWriter>,
    /// Async encoder for raw video
    raw_encoder: Option<AsyncVideoEncoder>,
    /// Encoding mode for raw video
    encoding_mode: VideoEncodingMode,
    /// Pixel format for raw video capture
    pixel_format: Option<String>,
}

/// Generic video file writer that handles different codecs and containers
/// 
/// Pipeline: appsrc -> parser -> muxer -> filesink
struct VideoWriter {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    start_time: Instant,
    codec: crate::encoding::VideoCodec,
    output_path: PathBuf,
}

impl VideoWriter {
    /// Create a new video writer for the specified codec
    fn new(path: &PathBuf, codec: crate::encoding::VideoCodec, width: u32, height: u32, fps: u32) -> Result<Self> {
        let pipeline = gst::Pipeline::new();
        let container = codec.container();
        
        println!("[Video] Creating {} writer with {} codec (creating elements...)", 
            container.extension(), codec.display_name());
        
        // Create appsrc with appropriate caps for the codec
        let caps = gst::Caps::builder(codec.gst_caps_name())
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", gst::Fraction::new(fps as i32, 1))
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
            .map_err(|e| VideoError::Pipeline(format!("Failed to create {}: {}", container.gst_muxer(), e)))?;
        
        // Set muxer-specific properties
        match container {
            crate::encoding::ContainerFormat::Mkv => {
                muxer.set_property("writing-app", "Sacho");
            }
            crate::encoding::ContainerFormat::Mp4 => {
                // mp4mux needs faststart for streaming/seeking
                muxer.set_property("faststart", true);
            }
            crate::encoding::ContainerFormat::WebM => {
                // webmmux doesn't need special properties
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
                .map_err(|e| VideoError::Pipeline(format!("Failed to create {}: {}", codec.gst_parser(), e)))?;
            
            // Add elements to pipeline
            pipeline.add_many([appsrc.upcast_ref(), &parser, &muxer, &filesink])
                .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;
            
            println!("[Video]   Elements added, linking with parser...");
            
            // Link elements
            gst::Element::link_many([appsrc.upcast_ref(), &parser, &muxer, &filesink])
                .map_err(|e| VideoError::Pipeline(format!("Failed to link elements: {}", e)))?;
        } else {
            // MJPEG: skip parser, link appsrc directly to muxer
            pipeline.add_many([appsrc.upcast_ref(), &muxer, &filesink])
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
            start_time: Instant::now(),
            codec,
            output_path: path.clone(),
        })
    }
    
    fn write_frame(&self, frame: &BufferedFrame, pts_offset: u64) -> Result<()> {
        let mut buffer = gst::Buffer::from_slice(frame.data.clone());
        {
            let buffer_ref = buffer.get_mut().unwrap();
            buffer_ref.set_pts(gst::ClockTime::from_nseconds(frame.pts.saturating_sub(pts_offset)));
            buffer_ref.set_duration(gst::ClockTime::from_nseconds(frame.duration));
        }
        
        self.appsrc.push_buffer(buffer)
            .map_err(|e| VideoError::Pipeline(format!("Failed to push buffer: {:?}", e)))?;
        
        Ok(())
    }
    
    fn finish(self) -> Result<(Duration, u64)> {
        let duration = self.start_time.elapsed();
        
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
            let file_size = std::fs::metadata(&self.output_path).map(|m| m.len()).unwrap_or(0);
            return Ok((duration, file_size));
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
        
        Ok((duration, file_size))
    }
}

impl Drop for VideoWriter {
    fn drop(&mut self) {
        // Ensure pipeline is stopped to avoid GStreamer resource leaks
        // This handles cases where finish() was not called (e.g., error paths)
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl VideoCapturePipeline {
    /// Create a new capture pipeline for a webcam device with passthrough
    /// 
    /// This pipeline captures video directly from the camera without re-encoding,
    /// which is much more efficient than decode+encode.
    /// 
    /// - `device_index`: Device index (used on Linux/macOS)
    /// - `device_name`: Device name (used on Windows with DirectShow)
    /// - `codec`: Video codec to capture
    /// - `pre_roll_secs`: Pre-roll buffer duration
    pub fn new_webcam(device_index: u32, device_name_hint: &str, codec: crate::encoding::VideoCodec, pre_roll_secs: u32) -> Result<Self> {
        // Initialize GStreamer if not already done
        gst::init().map_err(|e| VideoError::Gst(e))?;
        
        let pipeline = gst::Pipeline::new();
        
        // Create source element based on platform
        // Windows: Use DirectShow (dshowvideosrc) to access video from capture cards
        // Linux: Use v4l2src
        // macOS: Use avfvideosrc
        #[cfg(target_os = "windows")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("dshowvideosrc")
                .property("device-name", device_name_hint)
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create dshowvideosrc: {}", e)))?;
            (src, device_name_hint.to_string())
        };
        
        #[cfg(target_os = "linux")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("v4l2src")
                .property("device", format!("/dev/video{}", device_index))
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create v4l2src: {}", e)))?;
            let name = src.property::<Option<String>>("device-name")
                .unwrap_or_else(|| format!("Webcam {}", device_index));
            (src, name)
        };
        
        #[cfg(target_os = "macos")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("avfvideosrc")
                .property("device-index", device_index as i32)
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create avfvideosrc: {}", e)))?;
            let name = src.property::<Option<String>>("device-name")
                .unwrap_or_else(|| format!("Webcam {}", device_index));
            (src, name)
        };
        
        println!("[Video] Creating {} passthrough pipeline for {} (device {})", 
            codec.display_name(), device_name, device_index);
        
        // Passthrough pipeline: source -> capsfilter(codec) -> parser -> capsfilter(byte-stream) -> queue -> appsink
        // We force byte-stream output so the writer's parser can properly convert to AVC for muxing
        
        // Capsfilter to force the specified codec output from camera
        // Use width/height ranges to prefer higher resolutions while allowing negotiation
        let input_caps = gst::Caps::builder(codec.gst_caps_name())
            .field("width", gst::IntRange::new(640, 1920))
            .field("height", gst::IntRange::new(480, 1080))
            .field("framerate", gst::FractionRange::new(
                gst::Fraction::new(15, 1),
                gst::Fraction::new(60, 1)
            ))
            .build();
        
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", input_caps)
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
        
        // For MJPEG, skip the parser. jpegparse extracts dimensions from JPEG SOF markers,
        // which some capture devices report with swapped width/height. By skipping it,
        // we use the camera's advertised dimensions directly.
        // For MJPEG, use jpegparse. For other codecs (AV1, VP8, VP9), use identity (passthrough)
        let parser = gst::ElementFactory::make(codec.gst_parser())
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create {}: {}", codec.gst_parser(), e)))?;
        
        pipeline.add_many([&source, &capsfilter, &parser, &queue, appsink.upcast_ref()])
            .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;
        
        gst::Element::link_many([&source, &capsfilter, &parser, &queue, appsink.upcast_ref()])
            .map_err(|e| VideoError::Pipeline(format!("Failed to link pipeline: {}", e)))?;
        
        // Debug: Print the caps being used
        println!("[Video] {} passthrough pipeline created for {} (device {})", 
            codec.display_name(), device_name, device_index);
        println!("[Video]   Capsfilter set to: {}", codec.gst_caps_name());
        
        // Create pre-roll buffer
        let preroll_buffer = Arc::new(Mutex::new(VideoPrerollBuffer::new(pre_roll_secs)));
        
        // Set up appsink callback to fill pre-roll buffer
        let preroll_clone = preroll_buffer.clone();
        let frame_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let frame_counter_clone = frame_counter.clone();
        
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    match sink.pull_sample() {
                        Ok(sample) => {
                            if let Some(buffer) = sample.buffer() {
                                let pts = buffer.pts().map(|t| t.nseconds()).unwrap_or(0);
                                let duration = buffer.duration().map(|t| t.nseconds()).unwrap_or(33_333_333); // ~30fps default
                                
                                if let Ok(map) = buffer.map_readable() {
                                    let data = map.as_slice().to_vec();
                                    let frame_num = frame_counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    
                                    // Minimal logging - only first frame
                                    if frame_num == 0 {
                                        println!("[Video] First frame: {} bytes, pts={}", data.len(), pts);
                                    }
                                    
                                    let frame = BufferedFrame {
                                        data,
                                        pts,
                                        duration,
                                        wall_time: Instant::now(),
                                        pixel_format: None, // Pre-encoded, no pixel format
                                    };
                                    preroll_clone.lock().push(frame);
                                }
                            }
                            Ok(gst::FlowSuccess::Ok)
                        }
                        Err(_) => Err(gst::FlowError::Error),
                    }
                })
                .build()
        );
        
        // Store frame counter for later reference (unused for now but useful for debugging)
        let _ = frame_counter;
        
        Ok(Self {
            device_id: format!("webcam-{}", device_index),
            device_name,
            codec,
            pipeline,
            appsink,
            preroll_buffer,
            recording_path: None,
            recording_start: None,
            pts_offset: 0,
            frames_written: 0,
            width: 1280,
            height: 720,
            fps: 30,
            is_recording: false,
            file_writer: None,
            raw_encoder: None,
            encoding_mode: VideoEncodingMode::Av1Hardware,
            pixel_format: None,
        })
    }
    
    /// Create a new capture pipeline for raw video from a webcam device
    /// 
    /// This pipeline captures raw video and encodes it using hardware acceleration.
    /// 
    /// - `device_index`: Device index (used on Linux/macOS)
    /// - `device_name`: Device name (used on Windows with DirectShow)
    /// - `pre_roll_secs`: Pre-roll buffer duration
    /// - `encoding_mode`: How to encode the raw video
    pub fn new_webcam_raw(
        device_index: u32, 
        device_name_hint: &str, 
        pre_roll_secs: u32,
        encoding_mode: VideoEncodingMode,
    ) -> Result<Self> {
        // Initialize GStreamer if not already done
        gst::init().map_err(|e| VideoError::Gst(e))?;
        
        let pipeline = gst::Pipeline::new();
        
        // Create source element based on platform
        #[cfg(target_os = "windows")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("dshowvideosrc")
                .property("device-name", device_name_hint)
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create dshowvideosrc: {}", e)))?;
            (src, device_name_hint.to_string())
        };
        
        #[cfg(target_os = "linux")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("v4l2src")
                .property("device", format!("/dev/video{}", device_index))
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create v4l2src: {}", e)))?;
            let name = src.property::<Option<String>>("device-name")
                .unwrap_or_else(|| format!("Webcam {}", device_index));
            (src, name)
        };
        
        #[cfg(target_os = "macos")]
        let (source, device_name) = {
            let src = gst::ElementFactory::make("avfvideosrc")
                .property("device-index", device_index as i32)
                .build()
                .map_err(|e| VideoError::Pipeline(format!("Failed to create avfvideosrc: {}", e)))?;
            let name = src.property::<Option<String>>("device-name")
                .unwrap_or_else(|| format!("Webcam {}", device_index));
            (src, name)
        };
        
        println!("[Video] Creating RAW capture pipeline for {} (device {})", device_name, device_index);
        println!("[Video]   Encoding mode: {:?}", encoding_mode);
        
        // Raw video pipeline: source -> capsfilter(raw) -> videoconvert -> queue -> appsink
        // We prefer NV12 format as it's efficient for hardware encoders
        let input_caps = gst::Caps::builder("video/x-raw")
            .field("width", gst::IntRange::new(640, 1920))
            .field("height", gst::IntRange::new(480, 1080))
            .field("framerate", gst::FractionRange::new(
                gst::Fraction::new(15, 1),
                gst::Fraction::new(60, 1)
            ))
            .build();
        
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", input_caps)
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create capsfilter: {}", e)))?;
        
        // Video converter to normalize format
        let videoconvert = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create videoconvert: {}", e)))?;
        
        // Force output to a format suitable for encoding
        let output_caps = gst::Caps::builder("video/x-raw")
            .field("format", "NV12") // NV12 is efficient for most hardware encoders
            .build();
        
        let output_capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", output_caps)
            .build()
            .map_err(|e| VideoError::Pipeline(format!("Failed to create output capsfilter: {}", e)))?;
        
        // Queue for buffering with larger size for raw video
        let queue = gst::ElementFactory::make("queue")
            .property("max-size-buffers", 30u32)
            .property("max-size-bytes", 100_000_000u32) // 100MB
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
        
        // Add elements to pipeline
        pipeline.add_many([&source, &capsfilter, &videoconvert, &output_capsfilter, &queue, appsink.upcast_ref()])
            .map_err(|e| VideoError::Pipeline(format!("Failed to add elements: {}", e)))?;
        
        // Link all elements
        gst::Element::link_many([&source, &capsfilter, &videoconvert, &output_capsfilter, &queue, appsink.upcast_ref()])
            .map_err(|e| VideoError::Pipeline(format!("Failed to link pipeline: {}", e)))?;
        
        println!("[Video] RAW capture pipeline created for {} (device {})", device_name, device_index);
        
        // Create pre-roll buffer (larger for raw video)
        let preroll_buffer = Arc::new(Mutex::new(VideoPrerollBuffer::new(pre_roll_secs)));
        
        // Set up appsink callback to fill pre-roll buffer
        let preroll_clone = preroll_buffer.clone();
        let frame_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let frame_counter_clone = frame_counter.clone();
        
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    match sink.pull_sample() {
                        Ok(sample) => {
                            if let Some(buffer) = sample.buffer() {
                                let pts = buffer.pts().map(|t| t.nseconds()).unwrap_or(0);
                                let duration = buffer.duration().map(|t| t.nseconds()).unwrap_or(33_333_333);
                                
                                // Get pixel format from caps
                                let pixel_format = sample.caps()
                                    .and_then(|caps| caps.structure(0))
                                    .and_then(|s| s.get::<String>("format").ok());
                                
                                if let Ok(map) = buffer.map_readable() {
                                    let data = map.as_slice().to_vec();
                                    let frame_num = frame_counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    
                                    if frame_num == 0 {
                                        println!("[Video] First RAW frame: {} bytes, pts={}, format={:?}", 
                                            data.len(), pts, pixel_format);
                                    }
                                    
                                    let frame = BufferedFrame {
                                        data,
                                        pts,
                                        duration,
                                        wall_time: Instant::now(),
                                        pixel_format: pixel_format.clone(),
                                    };
                                    preroll_clone.lock().push(frame);
                                }
                            }
                            Ok(gst::FlowSuccess::Ok)
                        }
                        Err(_) => Err(gst::FlowError::Error),
                    }
                })
                .build()
        );
        
        let _ = frame_counter;
        
        Ok(Self {
            device_id: format!("webcam-{}", device_index),
            device_name,
            codec: crate::encoding::VideoCodec::Raw,
            pipeline,
            appsink,
            preroll_buffer,
            recording_path: None,
            recording_start: None,
            pts_offset: 0,
            frames_written: 0,
            width: 1280,
            height: 720,
            fps: 30,
            is_recording: false,
            file_writer: None,
            raw_encoder: None,
            encoding_mode,
            pixel_format: Some("NV12".to_string()),
        })
    }
    
    /// Start the capture pipeline (begins filling pre-roll buffer)
    pub fn start(&mut self) -> Result<()> {
        self.pipeline.set_state(gst::State::Playing)?;
        println!("[Video] Started capture pipeline for {}", self.device_name);
        
        // Query the negotiated caps to get actual resolution
        // Give the pipeline a moment to negotiate
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        if let Some(pad) = self.appsink.static_pad("sink") {
            if let Some(caps) = pad.current_caps() {
                if let Some(structure) = caps.structure(0) {
                    self.width = structure.get::<i32>("width").unwrap_or(1280) as u32;
                    self.height = structure.get::<i32>("height").unwrap_or(720) as u32;
                    self.fps = structure.get::<gst::Fraction>("framerate")
                        .map(|f| {
                            let numer = f.numer() as f64;
                            let denom = (f.denom() as f64).max(1.0);
                            (numer / denom).round() as u32
                        })
                        .unwrap_or(30);
                    
                    println!("[Video]   Negotiated: {}x{} @ {}fps", self.width, self.height, self.fps);
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
        
        // For raw video, determine the actual output format based on encoding mode
        if self.codec == crate::encoding::VideoCodec::Raw {
            let target_codec = match self.encoding_mode {
                VideoEncodingMode::Av1Hardware => crate::encoding::VideoCodec::Av1,
                VideoEncodingMode::Vp9 => crate::encoding::VideoCodec::Vp9,
                VideoEncodingMode::Vp8 => crate::encoding::VideoCodec::Vp8,
                VideoEncodingMode::Raw => crate::encoding::VideoCodec::Av1, // Fallback
            };
            output_path = output_path.with_extension(target_codec.container().extension());
            println!("[Video] Starting recording to {:?} (raw -> {})", output_path, target_codec.display_name());
        } else {
            println!("[Video] Starting recording to {:?} (codec: {})", output_path, self.codec.display_name());
        }
        
        // Drain pre-roll buffer
        let preroll_frames = self.preroll_buffer.lock().drain();
        println!("[Video] Pre-roll buffer has {} frames", preroll_frames.len());
        
        // Calculate pre-roll duration as time from FIRST frame capture to NOW
        // This is the correct reference for syncing with audio/MIDI
        // (Previously we used last-first span, but that doesn't account for 
        // the delay between video processing and audio processing)
        let preroll_duration = preroll_frames.first()
            .map(|f| f.wall_time.elapsed())
            .unwrap_or(Duration::ZERO);
        
        self.pts_offset = preroll_frames.first().map(|f| f.pts).unwrap_or(0);
        
        // Handle raw vs pre-encoded video differently
        if self.codec == crate::encoding::VideoCodec::Raw {
            // Determine target codec based on encoding mode
            let target_codec = match self.encoding_mode {
                VideoEncodingMode::Av1Hardware => crate::encoding::VideoCodec::Av1,
                VideoEncodingMode::Vp9 => crate::encoding::VideoCodec::Vp9,
                VideoEncodingMode::Vp8 => crate::encoding::VideoCodec::Vp8,
                VideoEncodingMode::Raw => crate::encoding::VideoCodec::Av1, // Fallback
            };
            
            // Raw video - use async encoder
            let encoder_config = EncoderConfig {
                bitrate: 0, // Auto
                keyframe_interval: self.fps * 2, // Keyframe every 2 seconds
                preset: "p4".to_string(), // Balanced preset
                target_codec,
            };
            
            // Create encoder with buffer size of ~2 seconds of frames for backpressure
            let buffer_size = (self.fps * 2) as usize;
            let encoder = AsyncVideoEncoder::new(
                output_path.clone(),
                self.width,
                self.height,
                self.fps,
                encoder_config,
                buffer_size,
            ).map_err(|e| VideoError::Pipeline(format!("Failed to create encoder: {}", e)))?;
            
            // Send pre-roll frames to encoder
            let pixel_format = self.pixel_format.clone().unwrap_or_else(|| "NV12".to_string());
            for frame in &preroll_frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: self.width,
                    height: self.height,
                    format: frame.pixel_format.clone().unwrap_or_else(|| pixel_format.clone()),
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
            let writer = VideoWriter::new(&output_path, self.codec, self.width, self.height, self.fps)?;
            
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
        
        println!("[Video] Started recording, pre-roll: {:?}", preroll_duration);
        
        Ok(preroll_duration)
    }
    
    /// Stop recording and finalize the file
    pub fn stop_recording(&mut self) -> Result<VideoFileInfo> {
        if !self.is_recording {
            return Err(VideoError::Pipeline("Not recording".to_string()));
        }
        
        // Drain any remaining frames from pre-roll buffer
        let remaining_frames = self.preroll_buffer.lock().drain();
        
        let (duration, file_size) = if let Some(encoder) = self.raw_encoder.take() {
            // Raw video with encoding
            let pixel_format = self.pixel_format.clone().unwrap_or_else(|| "NV12".to_string());
            
            // Send remaining frames to encoder
            for frame in &remaining_frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: self.width,
                    height: self.height,
                    format: frame.pixel_format.clone().unwrap_or_else(|| pixel_format.clone()),
                    capture_time: frame.wall_time,
                };
                
                // Use non-blocking send, drop frames if encoder can't keep up
                if let Ok(false) = encoder.try_send_frame(raw_frame) {
                    println!("[Video] Warning: Dropped frame during stop (encoder backpressure)");
                }
            }
            self.frames_written += remaining_frames.len() as u64;
            
            // Finish encoding
            let stats = encoder.finish()
                .map_err(|e| VideoError::Pipeline(format!("Failed to finish encoding: {}", e)))?;
            
            (stats.encoding_duration, stats.bytes_written)
        } else if let Some(writer) = self.file_writer.take() {
            // Pre-encoded video
            for frame in &remaining_frames {
                let _ = writer.write_frame(frame, self.pts_offset);
            }
            self.frames_written += remaining_frames.len() as u64;
            
            writer.finish()?
        } else {
            return Err(VideoError::Pipeline("No active writer or encoder".to_string()));
        };
        
        let filename = self.recording_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("video.webm")
            .to_string();
        
        self.is_recording = false;
        self.recording_path = None;
        self.recording_start = None;
        
        println!("[Video] Stopped recording {}, duration: {:?}, size: {} bytes", 
            filename, duration, file_size);
        
        Ok(VideoFileInfo {
            filename,
            device_name: self.device_name.clone(),
            width: self.width,
            height: self.height,
            fps: self.fps,
            duration_secs: duration.as_secs_f64(),
            size_bytes: file_size,
        })
    }
    
    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.is_recording
    }
    
    /// Get pre-roll buffer duration
    pub fn preroll_duration(&self) -> Duration {
        self.preroll_buffer.lock().duration()
    }
    
    /// Set pre-roll duration
    pub fn set_preroll_duration(&self, secs: u32) {
        self.preroll_buffer.lock().set_duration(secs);
    }
    
    /// Poll for new frames and write to file if recording
    /// This should be called periodically from a background thread
    pub fn poll(&mut self) -> Result<()> {
        if !self.is_recording {
            return Ok(());
        }
        
        // Drain accumulated frames
        let frames = self.preroll_buffer.lock().drain();
        
        if let Some(ref encoder) = self.raw_encoder {
            // Raw video - send to encoder (non-blocking)
            let pixel_format = self.pixel_format.clone().unwrap_or_else(|| "NV12".to_string());
            let mut frames_sent = 0u64;
            let mut frames_dropped = 0u64;
            
            for frame in &frames {
                let raw_frame = RawVideoFrame {
                    data: frame.data.clone(),
                    pts: frame.pts,
                    duration: frame.duration,
                    width: self.width,
                    height: self.height,
                    format: frame.pixel_format.clone().unwrap_or_else(|| pixel_format.clone()),
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
            
            if frames_dropped > 0 {
                println!("[Video] Warning: Dropped {} frames due to encoder backpressure", frames_dropped);
            }
        } else if let Some(ref writer) = self.file_writer {
            // Pre-encoded video - write directly
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
    /// Encoding mode for raw video
    encoding_mode: VideoEncodingMode,
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
            encoding_mode: VideoEncodingMode::Av1Hardware,
        }
    }
    
    /// Set the encoding mode for raw video
    pub fn set_encoding_mode(&mut self, mode: VideoEncodingMode) {
        self.encoding_mode = mode;
    }
    
    /// Start capturing from specified devices with their codecs
    /// 
    /// Each tuple is (device_id, device_name, codec)
    pub fn start(&mut self, devices: &[(String, String, crate::encoding::VideoCodec)]) -> Result<()> {
        // Stop any existing pipelines
        self.stop();
        
        for (device_id, device_name, codec) in devices {
            // Device index is only used on Linux/macOS; Windows uses device_name
            // For name-based IDs (video-xxx), we don't have an index
            let index = device_id
                .strip_prefix("webcam-")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            
            // Create appropriate pipeline based on codec
            let pipeline_result = if codec.requires_encoding() {
                // Raw video - use encoding pipeline
                VideoCapturePipeline::new_webcam_raw(
                    index, 
                    device_name, 
                    self.pre_roll_secs,
                    self.encoding_mode.clone(),
                )
            } else {
                // Pre-encoded video - use passthrough pipeline
                VideoCapturePipeline::new_webcam(index, device_name, *codec, self.pre_roll_secs)
            };
            
            match pipeline_result {
                Ok(mut pipeline) => {
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
        
        println!("[Video] Started {} video capture pipeline(s)", self.pipelines.len());
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
        let pipeline_count = self.pipelines.len();
        
        for (device_id, pipeline) in self.pipelines.iter_mut() {
            println!("[Video] Processing recording start for: {}", device_id);
            
            let safe_id = device_id
                .replace(" ", "_")
                .replace("/", "_")
                .replace("\\", "_")
                .replace(":", "_");
            
            // Use the correct file extension for the codec's container
            let extension = pipeline.codec.container().extension();
            let filename = if pipeline_count == 1 {
                format!("video.{}", extension)
            } else {
                format!("video_{}.{}", safe_id, extension)
            };
            
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
    
    /// Set pre-roll duration for all pipelines
    pub fn set_preroll_duration(&mut self, secs: u32) {
        self.pre_roll_secs = secs;
        for (_, pipeline) in self.pipelines.iter() {
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
