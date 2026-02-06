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

/// Type of hardware encoder backend
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
    /// Windows Media Foundation
    MediaFoundation,
    /// Software fallback
    Software,
}

impl HardwareEncoderType {
    /// Get the GStreamer element name for AV1 encoding
    /// Supports both hardware and software (libaom) encoders
    pub fn av1_encoder_element(&self) -> Option<&'static str> {
        match self {
            HardwareEncoderType::Nvenc => Some("nvav1enc"),
            HardwareEncoderType::Amf => Some("amfav1enc"),
            HardwareEncoderType::Qsv => Some("qsvav1enc"),
            // VA-API: check for both new 'va' and old 'vaapi' plugins
            HardwareEncoderType::VaApi => {
                if gst::ElementFactory::find("vaav1enc").is_some() {
                    Some("vaav1enc")
                } else if gst::ElementFactory::find("vaapiav1enc").is_some() {
                    Some("vaapiav1enc")
                } else {
                    None
                }
            }
            // Software AV1 encoding via libaom (slower but works everywhere)
            HardwareEncoderType::Software => Some("av1enc"),
            // Media Foundation does not support AV1 encoding
            HardwareEncoderType::MediaFoundation => None,
        }
    }
    
    /// Get the GStreamer element name for VP8 encoding
    /// VP8 is royalty-free, so we can use both hardware and software encoders
    pub fn vp8_encoder_element(&self) -> Option<&'static str> {
        match self {
            HardwareEncoderType::Qsv => Some("qsvvp8enc"),
            // VA-API: check for both new 'va' and old 'vaapi' plugins
            HardwareEncoderType::VaApi => {
                if gst::ElementFactory::find("vavp8enc").is_some() {
                    Some("vavp8enc")
                } else if gst::ElementFactory::find("vaapivp8enc").is_some() {
                    Some("vaapivp8enc")
                } else {
                    None
                }
            }
            // Software fallback - vp8enc from libvpx is royalty-free
            HardwareEncoderType::Software => Some("vp8enc"),
            // These don't support VP8 encoding
            HardwareEncoderType::Nvenc => None,
            HardwareEncoderType::Amf => None,
            HardwareEncoderType::MediaFoundation => None,
        }
    }
    
    /// Get the GStreamer element name for VP9 encoding
    /// VP9 is royalty-free, so we can use both hardware and software encoders
    pub fn vp9_encoder_element(&self) -> Option<&'static str> {
        match self {
            HardwareEncoderType::Qsv => Some("qsvvp9enc"),
            // VA-API: check for both new 'va' and old 'vaapi' plugins
            HardwareEncoderType::VaApi => {
                if gst::ElementFactory::find("vavp9enc").is_some() {
                    Some("vavp9enc")
                } else if gst::ElementFactory::find("vaapivp9enc").is_some() {
                    Some("vaapivp9enc")
                } else {
                    None
                }
            }
            // Windows Media Foundation VP9 encoder
            HardwareEncoderType::MediaFoundation => Some("mfvp9enc"),
            // Software fallback - vp9enc from libvpx is royalty-free
            HardwareEncoderType::Software => Some("vp9enc"),
            // These don't support VP9 encoding
            HardwareEncoderType::Nvenc => None,
            HardwareEncoderType::Amf => None,
        }
    }
    
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            HardwareEncoderType::Nvenc => "NVIDIA NVENC",
            HardwareEncoderType::Amf => "AMD AMF",
            HardwareEncoderType::Qsv => "Intel QuickSync",
            HardwareEncoderType::VaApi => "VA-API",
            HardwareEncoderType::MediaFoundation => "Media Foundation",
            HardwareEncoderType::Software => "Software",
        }
    }
}

/// Detect the best available AV1 encoder
/// 
/// Checks for hardware encoders first, then falls back to software (libaom).
/// 
/// Hardware encoders checked:
/// - NVIDIA NVENC (nvav1enc) - RTX 40 series and newer
/// - AMD AMF (amfav1enc) - RX 7000 series and newer
/// - Intel QuickSync (qsvav1enc) - Arc GPUs and newer Intel iGPUs
/// - VA-API (vaav1enc, vaapiav1enc) - Linux (Intel Arc, AMD, some NVIDIA)
/// 
/// Software fallback:
/// - libaom (av1enc) - slower but works everywhere
/// 
/// Note: Vulkan Video encoding in GStreamer does not yet support AV1.
pub fn detect_best_av1_encoder() -> HardwareEncoderType {
    // Check NVIDIA NVENC first (fastest, best quality)
    if gst::ElementFactory::find("nvav1enc").is_some() {
        return HardwareEncoderType::Nvenc;
    }
    // Check AMD AMF
    if gst::ElementFactory::find("amfav1enc").is_some() {
        return HardwareEncoderType::Amf;
    }
    // Check Intel QuickSync
    if gst::ElementFactory::find("qsvav1enc").is_some() {
        return HardwareEncoderType::Qsv;
    }
    // Check VA-API - newer 'va' plugin (Linux)
    if gst::ElementFactory::find("vaav1enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Check VA-API - older 'gstreamer-vaapi' plugin (Linux, deprecated but still common)
    if gst::ElementFactory::find("vaapiav1enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Fall back to software (libaom) - slower but works everywhere
    HardwareEncoderType::Software
}

/// Check if any AV1 encoder is available (hardware or software)
pub fn has_av1_encoder() -> bool {
    let encoder_type = detect_best_av1_encoder();
    encoder_type.av1_encoder_element().is_some()
}

/// Detect the best available VP8 encoder
/// 
/// VP8 is royalty-free, so we can use any available encoder.
/// Checks for hardware encoders first, then falls back to software (libvpx).
/// 
/// Hardware encoders checked:
/// - Intel QuickSync (qsvvp8enc) - Windows/Linux
/// - VA-API (vavp8enc, vaapivp8enc) - Linux (Intel, AMD)
/// 
/// Note: NVIDIA NVENC and AMD AMF do not support VP8 encoding.
/// Note: Vulkan Video encoding in GStreamer does not yet support VP8.
pub fn detect_best_vp8_encoder() -> HardwareEncoderType {
    // Check Intel QuickSync (Windows and Linux)
    if gst::ElementFactory::find("qsvvp8enc").is_some() {
        return HardwareEncoderType::Qsv;
    }
    // Check VA-API - newer 'va' plugin (Linux - Intel, AMD)
    if gst::ElementFactory::find("vavp8enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Check VA-API - older 'gstreamer-vaapi' plugin (Linux, deprecated but still common)
    if gst::ElementFactory::find("vaapivp8enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Fall back to software (vp8enc from libvpx) - always available with GStreamer
    if gst::ElementFactory::find("vp8enc").is_some() {
        return HardwareEncoderType::Software;
    }
    // No VP8 encoder found
    HardwareEncoderType::Software
}

/// Check if any VP8 encoder is available (hardware or software)
pub fn has_vp8_encoder() -> bool {
    let encoder_type = detect_best_vp8_encoder();
    encoder_type.vp8_encoder_element().is_some()
}

/// Detect the best available VP9 encoder
/// 
/// VP9 is royalty-free, so we can use any available encoder.
/// Checks for hardware encoders first, then falls back to software (libvpx).
/// 
/// Hardware encoders checked:
/// - Intel QuickSync (qsvvp9enc) - Windows/Linux
/// - VA-API (vavp9enc, vaapivp9enc) - Linux (Intel, AMD, some NVIDIA)
/// - Windows Media Foundation (mfvp9enc) - Windows
/// 
/// Note: NVIDIA NVENC and AMD AMF do not support VP9 encoding.
/// Note: Vulkan Video encoding in GStreamer does not yet support VP9.
pub fn detect_best_vp9_encoder() -> HardwareEncoderType {
    // Check Intel QuickSync first (Windows and Linux)
    if gst::ElementFactory::find("qsvvp9enc").is_some() {
        return HardwareEncoderType::Qsv;
    }
    // Check VA-API - newer 'va' plugin (Linux - Intel, AMD, some NVIDIA with nouveau)
    if gst::ElementFactory::find("vavp9enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Check VA-API - older 'gstreamer-vaapi' plugin (Linux, deprecated but still common)
    if gst::ElementFactory::find("vaapivp9enc").is_some() {
        return HardwareEncoderType::VaApi;
    }
    // Check Windows Media Foundation (Windows only)
    if gst::ElementFactory::find("mfvp9enc").is_some() {
        return HardwareEncoderType::MediaFoundation;
    }
    // Fall back to software (vp9enc from libvpx) - royalty-free
    if gst::ElementFactory::find("vp9enc").is_some() {
        return HardwareEncoderType::Software;
    }
    // No VP9 encoder found
    HardwareEncoderType::Software
}

/// Check if any VP9 encoder is available (hardware or software)
pub fn has_vp9_encoder() -> bool {
    let encoder_type = detect_best_vp9_encoder();
    encoder_type.vp9_encoder_element().is_some()
}

/// Detect the best encoder for a given target codec
pub fn detect_best_encoder_for_codec(codec: VideoCodec) -> HardwareEncoderType {
    match codec {
        VideoCodec::Av1 => detect_best_av1_encoder(),
        VideoCodec::Vp8 => detect_best_vp8_encoder(),
        VideoCodec::Vp9 => detect_best_vp9_encoder(),
        _ => HardwareEncoderType::Software,
    }
}

/// Legacy function - detect best AV1 encoder
pub fn detect_best_encoder() -> HardwareEncoderType {
    detect_best_av1_encoder()
}

/// Check if any AV1 hardware encoder is available (not software)
pub fn has_hardware_av1_encoder() -> bool {
    let encoder_type = detect_best_av1_encoder();
    !matches!(encoder_type, HardwareEncoderType::Software) && encoder_type.av1_encoder_element().is_some()
}

/// Check if any VP9 hardware encoder is available (not software)
pub fn has_hardware_vp9_encoder() -> bool {
    let encoder_type = detect_best_vp9_encoder();
    !matches!(encoder_type, HardwareEncoderType::Software) && encoder_type.vp9_encoder_element().is_some()
}

/// Check if any VP8 hardware encoder is available (not software)
pub fn has_hardware_vp8_encoder() -> bool {
    let encoder_type = detect_best_vp8_encoder();
    !matches!(encoder_type, HardwareEncoderType::Software) && encoder_type.vp8_encoder_element().is_some()
}

/// Legacy alias for has_hardware_av1_encoder
pub fn has_hardware_encoder() -> bool {
    has_hardware_av1_encoder()
}

/// Get the recommended default video encoding mode
/// 
/// Priority:
/// 1. AV1 if hardware encoder is available
/// 2. VP9 if hardware encoder is available  
/// 3. VP8 if hardware encoder is available
/// 4. VP8 software (fallback - always available)
pub fn get_recommended_encoding_mode() -> crate::config::VideoEncodingMode {
    use crate::config::VideoEncodingMode;
    
    if has_hardware_av1_encoder() {
        VideoEncodingMode::Av1Hardware
    } else if has_hardware_vp9_encoder() {
        VideoEncodingMode::Vp9
    } else if has_hardware_vp8_encoder() {
        VideoEncodingMode::Vp8
    } else {
        // Fallback to VP8 software
        VideoEncodingMode::Vp8
    }
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
        let hw_type = detect_best_encoder_for_codec(config.target_codec);
        println!("[Encoder] Using {} for {} encoding", hw_type.display_name(), config.target_codec.display_name());
        
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
        
        // Remux the file to add proper duration header
        let bytes_written = match Self::remux_with_duration(&output_path) {
            Ok(size) => {
                println!("[Encoder] Remuxed with duration header, size: {} bytes", size);
                size
            }
            Err(e) => {
                println!("[Encoder] Warning: Failed to remux with duration: {}", e);
                // Fall back to original file size
                std::fs::metadata(&output_path)
                    .map(|m| m.len())
                    .unwrap_or(0)
            }
        };
        
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
    
    /// Remux a video file to add proper duration header
    /// 
    /// Files created in streaming mode may not have duration in the header.
    /// This function remuxes the file to add it.
    /// 
    /// For MP4 files with faststart, this may not be necessary, but we still
    /// attempt it to ensure maximum compatibility.
    fn remux_with_duration(file_path: &PathBuf) -> Result<u64> {
        let extension = file_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("webm");
        
        // MP4 files with faststart/fragmentation typically don't need remuxing
        if extension == "mp4" {
            println!("[Encoder] MP4 file - skipping remux (faststart already applied)");
            return std::fs::metadata(file_path)
                .map(|m| m.len())
                .map_err(|e| EncoderError::Io(e));
        }
        
        println!("[Encoder] Remuxing {} to add duration header...", extension);
        
        // Create temp file path
        let temp_path = file_path.with_extension(format!("{}.tmp", extension));
        
        // Build remux pipeline: filesrc ! matroskademux ! webmmux ! filesink
        let pipeline = gst::Pipeline::new();
        
        let filesrc = gst::ElementFactory::make("filesrc")
            .property("location", file_path.to_string_lossy().to_string())
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create filesrc: {}", e)))?;
        
        let demux = gst::ElementFactory::make("matroskademux")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create matroskademux: {}", e)))?;
        
        let mux = gst::ElementFactory::make("webmmux")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create webmmux: {}", e)))?;
        
        let filesink = gst::ElementFactory::make("filesink")
            .property("location", temp_path.to_string_lossy().to_string())
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create filesink: {}", e)))?;
        
        pipeline.add_many([&filesrc, &demux, &mux, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to add elements: {}", e)))?;
        
        // Link filesrc to demuxer
        filesrc.link(&demux)
            .map_err(|e| EncoderError::Pipeline(format!("Failed to link filesrc to demux: {}", e)))?;
        
        // Link muxer to filesink
        mux.link(&filesink)
            .map_err(|e| EncoderError::Pipeline(format!("Failed to link mux to filesink: {}", e)))?;
        
        // Handle dynamic pads from demuxer
        let mux_weak = mux.downgrade();
        demux.connect_pad_added(move |_demux, src_pad| {
            let Some(mux) = mux_weak.upgrade() else { return };
            
            // Get the pad name to determine the stream type
            let pad_name = src_pad.name();
            println!("[Encoder] Demux pad added: {}", pad_name);
            
            // Request appropriate pad from muxer
            let sink_pad = if pad_name.starts_with("video") {
                mux.request_pad_simple("video_%u")
            } else if pad_name.starts_with("audio") {
                mux.request_pad_simple("audio_%u")
            } else {
                None
            };
            
            if let Some(sink_pad) = sink_pad {
                if let Err(e) = src_pad.link(&sink_pad) {
                    println!("[Encoder] Warning: Failed to link pad {}: {:?}", pad_name, e);
                }
            }
        });
        
        // Run the pipeline
        pipeline.set_state(gst::State::Playing)
            .map_err(|e| EncoderError::Pipeline(format!("Failed to start remux pipeline: {:?}", e)))?;
        
        // Wait for EOS or error
        let bus = pipeline.bus().ok_or_else(|| EncoderError::Pipeline("No bus".into()))?;
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(60)) {
            match msg.view() {
                gst::MessageView::Eos(..) => {
                    println!("[Encoder] Remux complete");
                    break;
                }
                gst::MessageView::Error(err) => {
                    pipeline.set_state(gst::State::Null).ok();
                    return Err(EncoderError::Pipeline(format!(
                        "Remux error: {} ({:?})", err.error(), err.debug()
                    )));
                }
                _ => {}
            }
        }
        
        pipeline.set_state(gst::State::Null).ok();
        
        // Get the new file size
        let new_size = std::fs::metadata(&temp_path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        if new_size > 0 {
            // Replace original with remuxed version
            std::fs::remove_file(file_path)
                .map_err(|e| EncoderError::Io(e))?;
            std::fs::rename(&temp_path, file_path)
                .map_err(|e| EncoderError::Io(e))?;
            Ok(new_size)
        } else {
            // Keep original if remux produced empty file
            let _ = std::fs::remove_file(&temp_path);
            Err(EncoderError::Pipeline("Remux produced empty file".into()))
        }
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
        match config.target_codec {
            VideoCodec::Av1 => Self::create_av1_pipeline(output_path, width, height, fps, config, hw_type),
            VideoCodec::Vp9 => Self::create_vp9_pipeline(output_path, width, height, fps, config, hw_type),
            VideoCodec::Vp8 => Self::create_vp8_pipeline(output_path, width, height, fps, config, hw_type),
            _ => Err(EncoderError::NotAvailable(format!(
                "Encoding not supported for codec: {:?}", config.target_codec
            ))),
        }
    }
    
    /// Create common pipeline elements (appsrc, queue, videoconvert)
    fn create_common_pipeline_start(
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<(gst::Pipeline, gst_app::AppSrc, gst::Element, gst::Element)> {
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
        
        Ok((pipeline, appsrc, queue, videoconvert))
    }
    
    /// Create AV1 encoding pipeline (WebM container)
    fn create_av1_pipeline(
        output_path: &PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        config: &EncoderConfig,
        hw_type: HardwareEncoderType,
    ) -> Result<gst::Pipeline> {
        let (pipeline, appsrc, queue, videoconvert) = Self::create_common_pipeline_start(width, height, fps)?;
        
        // Create AV1 encoder
        let encoder = Self::create_av1_encoder(hw_type, config)?;
        
        // AV1 parser
        let parser = gst::ElementFactory::make("av1parse")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create av1parse: {}", e)))?;
        
        // WebM muxer for AV1
        let muxer = gst::ElementFactory::make("webmmux")
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
    
    /// Create the AV1 encoder element based on hardware type
    fn create_av1_encoder(hw_type: HardwareEncoderType, config: &EncoderConfig) -> Result<gst::Element> {
        let encoder_name = hw_type.av1_encoder_element()
            .ok_or_else(|| EncoderError::NotAvailable(format!(
                "{} does not support AV1 encoding", hw_type.display_name()
            )))?;
        
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
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::Qsv => {
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::VaApi => {
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::Software => {
                // libaom (av1enc) settings - optimize for speed
                // cpu-used: 0 (slowest) to 8 (fastest)
                encoder.set_property("cpu-used", 8u32);
                // Use multiple threads based on CPU cores
                let num_cpus = std::thread::available_parallelism()
                    .map(|p| p.get() as u32)
                    .unwrap_or(4);
                encoder.set_property("threads", num_cpus);
                // row-mt: enable row-based multi-threading
                encoder.set_property("row-mt", true);
                // target-bitrate in kbps for libaom
                if config.bitrate > 0 {
                    encoder.set_property("target-bitrate", config.bitrate / 1000);
                } else {
                    // Default to 8 Mbps
                    encoder.set_property("target-bitrate", 8000u32);
                }
                // Keyframe interval
                if config.keyframe_interval > 0 {
                    encoder.set_property("keyframe-max-dist", config.keyframe_interval);
                }
                // Use VBR (variable bitrate) for better quality
                encoder.set_property_from_str("end-usage", "vbr");
            }
            // Media Foundation does not support AV1 encoding
            HardwareEncoderType::MediaFoundation => {
                return Err(EncoderError::NotAvailable(
                    "Media Foundation does not support AV1 encoding".into()
                ));
            }
        }
        
        Ok(encoder)
    }
    
    /// Create VP8 encoding pipeline (WebM container)
    fn create_vp8_pipeline(
        output_path: &PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        config: &EncoderConfig,
        hw_type: HardwareEncoderType,
    ) -> Result<gst::Pipeline> {
        let (pipeline, appsrc, queue, videoconvert) = Self::create_common_pipeline_start(width, height, fps)?;
        
        // Create VP8 encoder
        let encoder = Self::create_vp8_encoder(hw_type, config)?;
        
        // WebM muxer for VP8
        let muxer = gst::ElementFactory::make("webmmux")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create webmmux: {}", e)))?;
        
        // File sink with sync disabled for better performance
        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_string_lossy().to_string())
            .property("async", false)
            .property("sync", false)
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create filesink: {}", e)))?;
        
        // Add elements to pipeline (no parser needed for VP8)
        pipeline.add_many([appsrc.upcast_ref(), &queue, &videoconvert, &encoder, &muxer, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to add elements: {}", e)))?;
        
        // Link elements
        gst::Element::link_many([appsrc.upcast_ref(), &queue, &videoconvert, &encoder, &muxer, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to link elements: {}", e)))?;
        
        Ok(pipeline)
    }
    
    /// Create the VP8 encoder element based on hardware type
    /// 
    /// VP8 is royalty-free, so we can use both hardware and software encoders.
    /// Hardware encoders (VA-API, QuickSync) are preferred, with libvpx as fallback.
    fn create_vp8_encoder(hw_type: HardwareEncoderType, config: &EncoderConfig) -> Result<gst::Element> {
        let encoder_name = hw_type.vp8_encoder_element()
            .ok_or_else(|| EncoderError::NotAvailable(format!(
                "{} does not support VP8 encoding", hw_type.display_name()
            )))?;
        
        let encoder = gst::ElementFactory::make(encoder_name)
            .build()
            .map_err(|e| EncoderError::NotAvailable(format!("Failed to create {}: {}", encoder_name, e)))?;
        
        // Set encoder properties based on type
        match hw_type {
            HardwareEncoderType::VaApi => {
                // VA-API VP8 settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::Qsv => {
                // Intel QuickSync VP8 settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::Software => {
                // libvpx vp8enc settings - optimize for speed
                // deadline: 0=best, 1=realtime (fastest)
                encoder.set_property_from_str("deadline", "1");
                // cpu-used: 0-16, higher = faster encoding (algorithmic complexity tradeoff)
                // Use 8 as a balance - still fast but better quality than 16
                encoder.set_property("cpu-used", 8i32);
                // threads: use available CPU cores for parallel encoding
                let num_cpus = std::thread::available_parallelism()
                    .map(|p| p.get() as i32)
                    .unwrap_or(4)
                    .min(16); // libvpx max threads is 16
                encoder.set_property("threads", num_cpus);
                // target-bitrate in bits per second (not kbps!)
                if config.bitrate > 0 {
                    encoder.set_property("target-bitrate", config.bitrate as i32);
                } else {
                    // Default to 12 Mbps for good quality
                    encoder.set_property("target-bitrate", 12_000_000i32);
                }
                // Keyframe interval
                if config.keyframe_interval > 0 {
                    encoder.set_property("keyframe-max-dist", config.keyframe_interval as i32);
                }
                // Use VBR for better quality (CBR can be too restrictive)
                encoder.set_property_from_str("end-usage", "vbr");
                // Set buffer sizes for VBR - allows bitrate to fluctuate for quality
                encoder.set_property("buffer-size", 6000i32); // 6 seconds of buffer
                encoder.set_property("buffer-initial-size", 4000i32);
                encoder.set_property("buffer-optimal-size", 5000i32);
            }
            // These encoder types don't support VP8
            _ => {
                return Err(EncoderError::NotAvailable(format!(
                    "VP8 encoding is not available with {}.",
                    hw_type.display_name()
                )));
            }
        }
        
        Ok(encoder)
    }
    
    /// Create VP9 encoding pipeline (WebM container)
    fn create_vp9_pipeline(
        output_path: &PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        config: &EncoderConfig,
        hw_type: HardwareEncoderType,
    ) -> Result<gst::Pipeline> {
        let (pipeline, appsrc, queue, videoconvert) = Self::create_common_pipeline_start(width, height, fps)?;
        
        // Create VP9 encoder
        let encoder = Self::create_vp9_encoder(hw_type, config)?;
        
        // WebM muxer for VP9
        let muxer = gst::ElementFactory::make("webmmux")
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create webmmux: {}", e)))?;
        
        // File sink with sync disabled for better performance
        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_string_lossy().to_string())
            .property("async", false)
            .property("sync", false)
            .build()
            .map_err(|e| EncoderError::Pipeline(format!("Failed to create filesink: {}", e)))?;
        
        // Add elements to pipeline (no parser needed for VP9)
        pipeline.add_many([appsrc.upcast_ref(), &queue, &videoconvert, &encoder, &muxer, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to add elements: {}", e)))?;
        
        // Link elements
        gst::Element::link_many([appsrc.upcast_ref(), &queue, &videoconvert, &encoder, &muxer, &filesink])
            .map_err(|e| EncoderError::Pipeline(format!("Failed to link elements: {}", e)))?;
        
        Ok(pipeline)
    }
    
    /// Create the VP9 encoder element based on hardware type
    /// 
    /// VP9 is royalty-free, so we can use both hardware and software encoders.
    /// Hardware encoders (QuickSync, VA-API) are preferred, with libvpx as fallback.
    fn create_vp9_encoder(hw_type: HardwareEncoderType, config: &EncoderConfig) -> Result<gst::Element> {
        let encoder_name = hw_type.vp9_encoder_element()
            .ok_or_else(|| EncoderError::NotAvailable(format!(
                "{} does not support VP9 encoding", hw_type.display_name()
            )))?;
        
        let encoder = gst::ElementFactory::make(encoder_name)
            .build()
            .map_err(|e| EncoderError::NotAvailable(format!("Failed to create {}: {}", encoder_name, e)))?;
        
        // Set encoder properties based on type
        match hw_type {
            HardwareEncoderType::Qsv => {
                // Intel QuickSync VP9 settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::VaApi => {
                // VA-API VP9 settings
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
            }
            HardwareEncoderType::MediaFoundation => {
                // Windows Media Foundation VP9 settings
                // Bitrate in kbps
                if config.bitrate > 0 {
                    encoder.set_property("bitrate", config.bitrate / 1000);
                }
                // GOP size (keyframe interval) - MF expects i32, not u32
                if config.keyframe_interval > 0 {
                    encoder.set_property("gop-size", config.keyframe_interval as i32);
                }
                // Low latency mode for real-time encoding
                encoder.set_property("low-latency", true);
            }
            HardwareEncoderType::Software => {
                // libvpx vp9enc settings - optimize for speed
                // deadline: 0=best, 1=good, 2=realtime (fastest)
                encoder.set_property_from_str("deadline", "1");
                // cpu-used: 0-8 for VP9, higher = faster encoding
                // Use 6 as a balance between speed and quality
                encoder.set_property("cpu-used", 6i32);
                // threads: use available CPU cores for parallel encoding
                let num_cpus = std::thread::available_parallelism()
                    .map(|p| p.get() as i32)
                    .unwrap_or(4)
                    .min(16);
                encoder.set_property("threads", num_cpus);
                // row-mt: enable row-based multi-threading for better parallelism
                encoder.set_property("row-mt", true);
                // target-bitrate in bits per second
                if config.bitrate > 0 {
                    encoder.set_property("target-bitrate", config.bitrate as i32);
                } else {
                    // Default to 10 Mbps for good quality (VP9 is more efficient than VP8)
                    encoder.set_property("target-bitrate", 10_000_000i32);
                }
                // Keyframe interval
                if config.keyframe_interval > 0 {
                    encoder.set_property("keyframe-max-dist", config.keyframe_interval as i32);
                }
                // Use VBR for better quality
                encoder.set_property_from_str("end-usage", "vbr");
                // Set buffer sizes for VBR
                encoder.set_property("buffer-size", 6000i32);
                encoder.set_property("buffer-initial-size", 4000i32);
                encoder.set_property("buffer-optimal-size", 5000i32);
            }
            // These encoder types don't support VP9
            _ => {
                return Err(EncoderError::NotAvailable(format!(
                    "VP9 encoding is not available with {}.",
                    hw_type.display_name()
                )));
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
