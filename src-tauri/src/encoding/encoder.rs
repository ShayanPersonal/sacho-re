//! Video encoder abstraction for raw video encoding
//!
//! This module provides a modular encoding system that supports various hardware
//! and software encoders. The primary use case is encoding raw video from cameras
//! that don't provide hardware-compressed output.
//!
//! ## Design Goals
//! - Non-blocking encoding to avoid blocking video capture
//! - Regular flushing to disk to prevent excessive RAM usage
//! - Thread-safe operation with proper synchronization
//! - Modular architecture for adding new encoder backends

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use crossbeam_channel::{Sender, Receiver, bounded, TrySendError};
use parking_lot::Mutex;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use super::VideoCodec;

/// Error type for encoder operations
#[derive(Debug, thiserror::Error)]
pub enum EncoderError {
    #[error("GStreamer error: {0}")]
    Gst(String),
    
    #[error("Encoder not available: {0}")]
    NotAvailable(String),
    
    #[error("Pipeline error: {0}")]
    Pipeline(String),
    
    #[error("Channel error: {0}")]
    Channel(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, EncoderError>;

/// Represents a raw video frame to be encoded
#[derive(Clone)]
pub struct RawVideoFrame {
    /// Raw pixel data (typically NV12, I420, or similar)
    pub data: Vec<u8>,
    /// Presentation timestamp in nanoseconds
    pub pts: u64,
    /// Duration in nanoseconds
    pub duration: u64,
    /// Frame width
    pub width: u32,
    /// Frame height
    pub height: u32,
    /// Pixel format (GStreamer format string, e.g., "NV12", "I420", "BGRA")
    pub format: String,
    /// Wall clock time when frame was captured
    pub capture_time: Instant,
}

/// Represents an encoded video frame
#[derive(Clone)]
pub struct EncodedFrame {
    /// Encoded frame data
    pub data: Vec<u8>,
    /// Presentation timestamp in nanoseconds
    pub pts: u64,
    /// Duration in nanoseconds
    pub duration: u64,
    /// Is this a keyframe
    pub is_keyframe: bool,
}

/// Configuration for video encoding
#[derive(Clone, Debug)]
pub struct EncoderConfig {
    /// Target bitrate in bits per second (0 = automatic)
    pub bitrate: u32,
    /// Keyframe interval in frames (0 = automatic)
    pub keyframe_interval: u32,
    /// Preset (encoder-specific, e.g., "p1" to "p7" for NVENC)
    pub preset: String,
    /// Target codec for encoding
    pub target_codec: VideoCodec,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            bitrate: 0, // Automatic
            keyframe_interval: 60, // Every 2 seconds at 30fps
            preset: "p4".to_string(), // NVENC default (balanced)
            target_codec: VideoCodec::Av1,
        }
    }
}

/// Type of hardware encoder available
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HardwareEncoderType {
    /// NVIDIA NVENC
    Nvenc,
    /// AMD AMF
    Amf,
    /// Intel QuickSync
    Qsv,
    /// VA-API (Linux)
    VaApi,
    /// Software fallback
    Software,
}

impl HardwareEncoderType {
    /// Get the GStreamer element name for AV1 encoding
    pub fn av1_encoder_element(&self) -> &'static str {
        match self {
            HardwareEncoderType::Nvenc => "nvav1enc",
            HardwareEncoderType::Amf => "amfav1enc",
            HardwareEncoderType::Qsv => "qsvav1enc",
            HardwareEncoderType::VaApi => "vaav1enc",
            HardwareEncoderType::Software => "av1enc", // libaom
        }
    }
    
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            HardwareEncoderType::Nvenc => "NVIDIA NVENC",
            HardwareEncoderType::Amf => "AMD AMF",
            HardwareEncoderType::Qsv => "Intel QuickSync",
            HardwareEncoderType::VaApi => "VA-API",
            HardwareEncoderType::Software => "Software (libaom)",
        }
    }
}

/// Detect the best available hardware encoder
pub fn detect_best_encoder() -> HardwareEncoderType {
    // Check in order of preference
    if gst::ElementFactory::find("nvav1enc").is_some() {
        return HardwareEncoderType::Nvenc;
    }
    if gst::ElementFactory::find("amfav1enc").is_some() {
        return HardwareEncoderType::Amf;
    }
    if gst::ElementFactory::find("qsvav1enc").is_some() {
        return HardwareEncoderType::Qsv;
    }
    if gst::ElementFactory::find("vaav1enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Fall back to software
    HardwareEncoderType::Software
}

/// Check if any hardware encoder is available
pub fn has_hardware_encoder() -> bool {
    detect_best_encoder() != HardwareEncoderType::Software
}

/// Asynchronous video encoder that runs encoding in a background thread
/// 
/// This encoder uses a producer-consumer pattern:
/// - Producer: Video capture thread pushes raw frames via `send_frame()`
/// - Consumer: Background encoding thread encodes and writes to file
/// 
/// The encoder maintains backpressure through bounded channels to prevent
/// memory exhaustion if encoding can't keep up with capture.
pub struct AsyncVideoEncoder {
    /// Channel to send frames to the encoder thread
    frame_sender: Sender<EncoderMessage>,
    /// Handle to the encoder thread
    encoder_thread: Option<std::thread::JoinHandle<Result<EncoderStats>>>,
    /// Encoder configuration (stored for potential diagnostics)
    #[allow(dead_code)]
    config: EncoderConfig,
    /// Hardware encoder type being used (stored for potential diagnostics)
    #[allow(dead_code)]
    hw_type: HardwareEncoderType,
    /// Shared state for checking encoder status
    state: Arc<Mutex<EncoderState>>,
}

/// Messages sent to the encoder thread
enum EncoderMessage {
    /// A frame to encode
    Frame(RawVideoFrame),
    /// Flush and finalize the output
    Finish,
}

/// Encoder state shared between threads
struct EncoderState {
    frames_encoded: u64,
    bytes_written: u64,
    is_finished: bool,
    last_error: Option<String>,
}

/// Statistics from encoding session
#[derive(Debug, Clone)]
pub struct EncoderStats {
    pub frames_encoded: u64,
    pub bytes_written: u64,
    pub encoding_duration: Duration,
    pub average_fps: f64,
}

impl AsyncVideoEncoder {
    /// Create a new async video encoder
    /// 
    /// # Arguments
    /// * `output_path` - Path to the output file
    /// * `width` - Video width
    /// * `height` - Video height
    /// * `fps` - Frame rate
    /// * `config` - Encoder configuration
    /// * `buffer_size` - Maximum frames to buffer (backpressure limit)
    pub fn new(
        output_path: PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        config: EncoderConfig,
        buffer_size: usize,
    ) -> Result<Self> {
        let hw_type = detect_best_encoder();
        println!("[Encoder] Using {} for AV1 encoding", hw_type.display_name());
        
        // Create bounded channel for frames (provides backpressure)
        let (frame_sender, frame_receiver) = bounded::<EncoderMessage>(buffer_size);
        
        let state = Arc::new(Mutex::new(EncoderState {
            frames_encoded: 0,
            bytes_written: 0,
            is_finished: false,
            last_error: None,
        }));
        
        let state_clone = state.clone();
        let config_clone = config.clone();
        
        // Spawn encoder thread
        let encoder_thread = std::thread::Builder::new()
            .name("sacho-video-encoder".into())
            .spawn(move || {
                Self::encoder_thread_main(
                    frame_receiver,
                    output_path,
                    width,
                    height,
                    fps,
                    config_clone,
                    hw_type,
                    state_clone,
                )
            })
            .map_err(|e| EncoderError::Pipeline(format!("Failed to spawn encoder thread: {}", e)))?;
        
        Ok(Self {
            frame_sender,
            encoder_thread: Some(encoder_thread),
            config,
            hw_type,
            state,
        })
    }
    
    /// Send a frame to be encoded (non-blocking)
    /// 
    /// Returns `Ok(true)` if frame was accepted, `Ok(false)` if buffer is full
    /// (frame was dropped), or `Err` if encoder has failed.
    pub fn try_send_frame(&self, frame: RawVideoFrame) -> Result<bool> {
        // Check for encoder error first
        {
            let state = self.state.lock();
            if let Some(ref err) = state.last_error {
                return Err(EncoderError::Pipeline(err.clone()));
            }
        }
        
        match self.frame_sender.try_send(EncoderMessage::Frame(frame)) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => {
                // Buffer full, frame will be dropped
                Ok(false)
            }
            Err(TrySendError::Disconnected(_)) => {
                Err(EncoderError::Channel("Encoder thread disconnected".into()))
            }
        }
    }
    
    /// Send a frame to be encoded (blocking if buffer is full)
    pub fn send_frame(&self, frame: RawVideoFrame) -> Result<()> {
        // Check for encoder error first
        {
            let state = self.state.lock();
            if let Some(ref err) = state.last_error {
                return Err(EncoderError::Pipeline(err.clone()));
            }
        }
        
        self.frame_sender.send(EncoderMessage::Frame(frame))
            .map_err(|_| EncoderError::Channel("Encoder thread disconnected".into()))
    }
    
    /// Finish encoding and wait for completion
    pub fn finish(mut self) -> Result<EncoderStats> {
        // Send finish message
        let _ = self.frame_sender.send(EncoderMessage::Finish);
        
        // Wait for encoder thread to complete
        if let Some(handle) = self.encoder_thread.take() {
            handle.join()
                .map_err(|_| EncoderError::Pipeline("Encoder thread panicked".into()))?
        } else {
            Err(EncoderError::Pipeline("Encoder already finished".into()))
        }
    }
    
    /// Get current encoding statistics
    pub fn stats(&self) -> (u64, u64) {
        let state = self.state.lock();
        (state.frames_encoded, state.bytes_written)
    }
    
    /// Check if the encoder has encountered an error
    pub fn has_error(&self) -> Option<String> {
        self.state.lock().last_error.clone()
    }
    
    /// Main function for the encoder thread
    fn encoder_thread_main(
        receiver: Receiver<EncoderMessage>,
        output_path: PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        config: EncoderConfig,
        hw_type: HardwareEncoderType,
        state: Arc<Mutex<EncoderState>>,
    ) -> Result<EncoderStats> {
        let start_time = Instant::now();
        
        // Create GStreamer encoding pipeline
        let pipeline = Self::create_pipeline(&output_path, width, height, fps, &config, hw_type)?;
        
        // Get appsrc element
        let appsrc = pipeline.by_name("src")
            .ok_or_else(|| EncoderError::Pipeline("Could not find appsrc".into()))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| EncoderError::Pipeline("Could not downcast to AppSrc".into()))?;
        
        // Start pipeline and wait for it to reach PLAYING state
        pipeline.set_state(gst::State::Playing)
            .map_err(|e| EncoderError::Pipeline(format!("Failed to start pipeline: {:?}", e)))?;
        
        // Wait for pipeline to be ready (up to 5 seconds)
        let (state_result, _, _) = pipeline.state(Some(gst::ClockTime::from_seconds(5)));
        match state_result {
            Ok(gst::StateChangeSuccess::Success) | Ok(gst::StateChangeSuccess::NoPreroll) => {
                println!("[Encoder] Pipeline ready");
            }
            Ok(gst::StateChangeSuccess::Async) => {
                println!("[Encoder] Pipeline starting asynchronously");
            }
            Err(e) => {
                return Err(EncoderError::Pipeline(format!("Failed to reach PLAYING state: {:?}", e)));
            }
        }
        
        let mut frames_encoded = 0u64;
        let mut first_pts: Option<u64> = None;
        
        // Process frames from channel
        loop {
            match receiver.recv() {
                Ok(EncoderMessage::Frame(frame)) => {
                    // Normalize PTS relative to first frame
                    let pts = if let Some(base) = first_pts {
                        frame.pts.saturating_sub(base)
                    } else {
                        first_pts = Some(frame.pts);
                        0
                    };
                    
                    // Create GStreamer buffer
                    let mut buffer = gst::Buffer::from_slice(frame.data);
                    {
                        let buffer_ref = buffer.get_mut().unwrap();
                        buffer_ref.set_pts(gst::ClockTime::from_nseconds(pts));
                        buffer_ref.set_duration(gst::ClockTime::from_nseconds(frame.duration));
                    }
                    
                    // Push to encoder
                    if let Err(e) = appsrc.push_buffer(buffer) {
                        let err_msg = format!("Failed to push buffer: {:?}", e);
                        state.lock().last_error = Some(err_msg.clone());
                        return Err(EncoderError::Pipeline(err_msg));
                    }
                    
                    frames_encoded += 1;
                    state.lock().frames_encoded = frames_encoded;
                    
                    // Log progress periodically
                    if frames_encoded % 100 == 0 {
                        println!("[Encoder] Encoded {} frames", frames_encoded);
                    }
                }
                Ok(EncoderMessage::Finish) => {
                    println!("[Encoder] Finishing encoding...");
                    break;
                }
                Err(_) => {
                    // Channel closed, finish up
                    break;
                }
            }
        }
        
        // Send EOS and wait for pipeline to finish
        println!("[Encoder] Sending EOS...");
        if let Err(e) = appsrc.end_of_stream() {
            println!("[Encoder] Warning: EOS send failed: {:?}", e);
        }
        
        // Wait for EOS on bus with longer timeout to allow muxer to finalize
        let mut got_eos = false;
        if let Some(bus) = pipeline.bus() {
            for msg in bus.iter_timed(gst::ClockTime::from_seconds(30)) {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        println!("[Encoder] EOS received");
                        got_eos = true;
                        break;
                    }
                    gst::MessageView::Error(err) => {
                        let err_msg = format!("Pipeline error: {} ({:?})", err.error(), err.debug());
                        println!("[Encoder] Error during finalization: {}", err_msg);
                        // Don't return error - try to save what we have
                        break;
                    }
                    gst::MessageView::StateChanged(sc) => {
                        if sc.src().map(|s| s == pipeline.upcast_ref::<gst::Object>()).unwrap_or(false) {
                            println!("[Encoder] Pipeline state: {:?} -> {:?}", sc.old(), sc.current());
                        }
                    }
                    _ => {}
                }
            }
        }
        
        if !got_eos {
            println!("[Encoder] Warning: Did not receive EOS, forcing stop");
        }
        
        // Stop pipeline gracefully
        pipeline.set_state(gst::State::Null).ok();
        
        // Give filesystem time to sync
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        // Get final file size
        let bytes_written = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        let encoding_duration = start_time.elapsed();
        let average_fps = if encoding_duration.as_secs_f64() > 0.0 {
            frames_encoded as f64 / encoding_duration.as_secs_f64()
        } else {
            0.0
        };
        
        // Update final state
        {
            let mut s = state.lock();
            s.frames_encoded = frames_encoded;
            s.bytes_written = bytes_written;
            s.is_finished = true;
        }
        
        println!("[Encoder] Finished: {} frames, {} bytes, {:.1} fps", 
            frames_encoded, bytes_written, average_fps);
        
        Ok(EncoderStats {
            frames_encoded,
            bytes_written,
            encoding_duration,
            average_fps,
        })
    }
    
    /// Create the GStreamer encoding pipeline
    fn create_pipeline(
        output_path: &PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        config: &EncoderConfig,
        hw_type: HardwareEncoderType,
    ) -> Result<gst::Pipeline> {
        let pipeline = gst::Pipeline::new();
        
        // Create appsrc with raw video caps - must specify format for proper negotiation
        // NV12 is the standard format we use for raw capture
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "NV12")
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", gst::Fraction::new(fps as i32, 1))
            .build();
        
        let appsrc = gst_app::AppSrc::builder()
            .name("src")
            .caps(&caps)
            .format(gst::Format::Time)
            .is_live(true)
            .stream_type(gst_app::AppStreamType::Stream)
            .build();
        
        // Queue to decouple appsrc from encoder and provide buffering
        let queue = gst::ElementFactory::make("queue")
            .property("max-size-buffers", 30u32)
            .property("max-size-time", 0u64) // No time limit
            .property("max-size-bytes", 0u32) // No byte limit
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create queue: {}", e)))?;
        
        // Video converter to handle any needed format conversion for encoder
        let videoconvert = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create videoconvert: {}", e)))?;
        
        // Create encoder based on hardware type
        let encoder = Self::create_encoder(hw_type, config)?;
        
        // AV1 parser
        let parser = gst::ElementFactory::make("av1parse")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create av1parse: {}", e)))?;
        
        // WebM muxer for AV1
        let muxer = gst::ElementFactory::make("webmmux")
            .property("streamable", true) // Allow streaming output
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create webmmux: {}", e)))?;
        
        // File sink with sync disabled for better performance
        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_string_lossy().to_string())
            .property("async", false)
            .property("sync", false)
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create filesink: {}", e)))?;
        
        // Add elements to pipeline
        pipeline.add_many([appsrc.upcast_ref(), &queue, &videoconvert, &encoder, &parser, &muxer, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to add elements: {}", e)))?;
        
        // Link elements
        gst::Element::link_many([appsrc.upcast_ref(), &queue, &videoconvert, &encoder, &parser, &muxer, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to link elements: {}", e)))?;
        
        Ok(pipeline)
    }
    
    /// Create the encoder element based on hardware type
    fn create_encoder(hw_type: HardwareEncoderType, config: &EncoderConfig) -> Result<gst::Element> {
        let encoder_name = hw_type.av1_encoder_element();
        
        let encoder = gst::ElementFactory::make(encoder_name)
            .build()
            .map_err(|e| EncoderError::NotAvailable(format!("Failed to create {}: {}", encoder_name, e)))?;
        
        // Set encoder properties based on type
        match hw_type {
            HardwareEncoderType::Nvenc => {
                // NVENC-specific settings
                // Preset: p1 (fastest) to p7 (best quality)
                if !config.preset.is_empty() {
                    if let Ok(preset_num) = config.preset.trim_start_matches('p').parse::<i32>() {
                        encoder.set_property_from_str("preset", &format!("p{}", preset_num.clamp(1, 7)));
                    }
                }
                // Rate control - NVENC uses kbps for bitrate
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
                // GOP size - must be i32, not u32
                if config.keyframe_interval > 0 {
                    encoder.set_property("gop-size", config.keyframe_interval as i32);
                }
            }
            HardwareEncoderType::Amf => {
                // AMD AMF settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000); // AMF uses kbps
                }
            }
            HardwareEncoderType::Qsv => {
                // Intel QuickSync settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::VaApi => {
                // VA-API settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::Software => {
                // libaom settings - software is slow, use fast settings
                encoder.set_property_from_str("cpu-used", "8"); // Fastest
            }
        }
        
        Ok(encoder)
    }
}

impl Drop for AsyncVideoEncoder {
    fn drop(&mut self) {
        // Ensure we clean up the encoder thread
        if self.encoder_thread.is_some() {
            // Send finish message to gracefully stop
            let _ = self.frame_sender.send(EncoderMessage::Finish);
            // Don't wait in drop - the thread will clean up on its own
        }
    }
}
