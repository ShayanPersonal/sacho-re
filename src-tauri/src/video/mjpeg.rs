// MJPEG frame extractor
//
// Uses GStreamer to extract MJPEG frames from Matroska container files.
// This is a passthrough demuxer - no decoding or re-encoding is performed.
// 
// This is used for the custom video player that displays individual JPEG frames.

use std::path::{Path, PathBuf};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use super::demux::{VideoDemuxer, VideoFrame, VideoInfo};
use super::VideoError;

/// MJPEG frame extractor using GStreamer - extracts JPEG frames without re-encoding
pub struct MjpegDemuxer {
    #[allow(dead_code)]
    path: PathBuf,
    info: VideoInfo,
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    /// Current position in milliseconds
    position_ms: u64,
    /// Cached frame index (timestamp_ms for each frame)
    frame_index: Option<Vec<u64>>,
}

impl MjpegDemuxer {
    /// Open an MKV file containing MJPEG frames
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, VideoError> {
        let path = path.as_ref().to_path_buf();
        
        gst::init().map_err(|e| VideoError::Gst(e.to_string()))?;
        
        // Build pipeline: filesrc -> matroskademux -> jpegparse -> appsink
        // This is pure passthrough - no decoding or encoding needed for MJPEG
        let pipeline = gst::Pipeline::new();
        
        let filesrc = gst::ElementFactory::make("filesrc")
            .property("location", path.to_string_lossy().to_string())
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create filesrc: {}", e)))?;
        
        let matroskademux = gst::ElementFactory::make("matroskademux")
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create matroskademux: {}", e)))?;
        
        // JPEG parser to ensure proper frame boundaries
        let jpegparse = gst::ElementFactory::make("jpegparse")
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create jpegparse: {}", e)))?;
        
        let appsink = gst_app::AppSink::builder()
            .name("sink")
            .sync(false)
            .build();
        
        pipeline.add_many([&filesrc, &matroskademux, &jpegparse, appsink.upcast_ref()])
            .map_err(|e| VideoError::Gst(format!("Failed to add elements: {}", e)))?;
        
        filesrc.link(&matroskademux)
            .map_err(|e| VideoError::Gst(format!("Failed to link filesrc to matroskademux: {}", e)))?;
        
        gst::Element::link_many([&jpegparse, appsink.upcast_ref()])
            .map_err(|e| VideoError::Gst(format!("Failed to link jpegparse to appsink: {}", e)))?;
        
        // Connect matroskademux pad-added signal to link ONLY JPEG streams to jpegparse.
        // We must NOT link VP8/VP9/AV1 pads here - jpegparse can only handle image/jpeg.
        let jpegparse_weak = jpegparse.downgrade();
        matroskademux.connect_pad_added(move |_demux, src_pad| {
            let Some(jpegparse) = jpegparse_weak.upgrade() else {
                return;
            };
            
            let caps = src_pad.current_caps().or_else(|| Some(src_pad.query_caps(None)));
            if let Some(caps) = caps {
                if let Some(structure) = caps.structure(0) {
                    let name = structure.name().as_str();
                    // Only link JPEG pads - reject VP8/VP9/AV1/other codecs
                    if name == "image/jpeg" {
                        let sink_pad = jpegparse.static_pad("sink").unwrap();
                        if !sink_pad.is_linked() {
                            if let Err(e) = src_pad.link(&sink_pad) {
                                log::warn!("Failed to link JPEG pad: {:?}", e);
                            }
                        }
                    } else {
                        log::debug!("MjpegDemuxer: ignoring non-JPEG pad with caps '{}'", name);
                    }
                }
            }
        });
        
        // Start pipeline in PAUSED state to preroll
        pipeline.set_state(gst::State::Paused)
            .map_err(|e| VideoError::Gst(format!("Failed to set pipeline to PAUSED: {:?}", e)))?;
        
        // Wait for preroll and get stream info
        let Some(bus) = pipeline.bus() else {
            pipeline.set_state(gst::State::Null).ok();
            return Err(VideoError::Gst("Failed to get pipeline bus".into()));
        };
        let mut width = 0u32;
        let mut height = 0u32;
        let fps = 30.0f64;
        let codec = String::from("mjpeg");
        let mut duration_ms = 0u64;
        
        // Wait for async-done or error
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(5)) {
            match msg.view() {
                gst::MessageView::AsyncDone(_) => {
                    // Get duration
                    if let Some(dur) = pipeline.query_duration::<gst::ClockTime>() {
                        duration_ms = dur.mseconds();
                    }
                    
                    // Get video dimensions from appsink preroll
                    if let Some(sample) = appsink.try_pull_preroll(gst::ClockTime::from_seconds(1)) {
                        if let Some(caps) = sample.caps() {
                            if let Some(structure) = caps.structure(0) {
                                width = structure.get::<i32>("width").unwrap_or(1280) as u32;
                                height = structure.get::<i32>("height").unwrap_or(720) as u32;
                            }
                        }
                    }
                    break;
                }
                gst::MessageView::Error(err) => {
                    pipeline.set_state(gst::State::Null).ok();
                    return Err(VideoError::Gst(format!(
                        "Pipeline error: {} ({:?})",
                        err.error(),
                        err.debug()
                    )));
                }
                _ => {}
            }
        }
        
        // Estimate frame count from duration and fps
        let frame_count = (duration_ms as f64 * fps / 1000.0) as u64;
        
        let info = VideoInfo {
            width,
            height,
            fps,
            duration_ms,
            frame_count,
            codec,
        };
        
        Ok(Self {
            path,
            info,
            pipeline,
            appsink,
            position_ms: 0,
            frame_index: None,
        })
    }
    
    /// Pull the next sample from the pipeline
    fn pull_sample(&self, timeout: gst::ClockTime) -> Result<Option<gst::Sample>, VideoError> {
        match self.appsink.try_pull_sample(timeout) {
            Some(sample) => Ok(Some(sample)),
            None => Ok(None),
        }
    }
}

impl VideoDemuxer for MjpegDemuxer {
    fn info(&self) -> &VideoInfo {
        &self.info
    }
    
    fn get_frame_at(&mut self, timestamp_ms: u64) -> Result<VideoFrame, VideoError> {
        self.seek(timestamp_ms)?;
        self.next_frame()?.ok_or(VideoError::FrameNotFound(timestamp_ms))
    }
    
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, VideoError> {
        // Make sure pipeline is playing
        if self.pipeline.current_state() != gst::State::Playing {
            self.pipeline.set_state(gst::State::Playing)
                .map_err(|e| VideoError::Gst(format!("Failed to set PLAYING: {:?}", e)))?;
            
            // Wait for the state change to complete before pulling
            let _ = self.pipeline.state(gst::ClockTime::from_seconds(2));
        }
        
        // Pull next sample (generous timeout to handle files with non-zero start PTS
        // where the demuxer may need to scan forward)
        let sample = match self.pull_sample(gst::ClockTime::from_seconds(2))? {
            Some(s) => s,
            None => return Ok(None),
        };
        
        let buffer = sample.buffer().ok_or_else(|| VideoError::Parse("No buffer in sample".into()))?;
        let pts = buffer.pts().map(|t| t.mseconds()).unwrap_or(self.position_ms);
        let duration = buffer.duration().map(|t| t.mseconds()).unwrap_or(33);
        
        let map = buffer.map_readable()
            .map_err(|e| VideoError::Parse(format!("Failed to map buffer: {}", e)))?;
        
        let frame = VideoFrame {
            data: map.as_slice().to_vec(),
            timestamp_ms: pts,
            duration_ms: duration,
            is_keyframe: true, // JPEG frames are always keyframes
        };
        
        self.position_ms = pts + duration;
        
        Ok(Some(frame))
    }
    
    fn seek(&mut self, timestamp_ms: u64) -> Result<(), VideoError> {
        // Seek to the specified position
        let seek_pos = gst::ClockTime::from_mseconds(timestamp_ms);
        
        self.pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            seek_pos
        ).map_err(|e| VideoError::Gst(format!("Seek failed: {:?}", e)))?;
        
        // Wait for seek to complete
        let Some(bus) = self.pipeline.bus() else {
            return Err(VideoError::Gst("Failed to get pipeline bus for seek".into()));
        };
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(2)) {
            match msg.view() {
                gst::MessageView::AsyncDone(_) => break,
                gst::MessageView::Error(err) => {
                    return Err(VideoError::Gst(format!("Seek error: {}", err.error())));
                }
                _ => {}
            }
        }
        
        self.position_ms = timestamp_ms;
        Ok(())
    }
    
    fn get_frame_timestamps(&mut self) -> Result<Vec<u64>, VideoError> {
        // If we have a cached index, return it
        if let Some(ref index) = self.frame_index {
            return Ok(index.clone());
        }
        
        // Build frame index by scanning through the file
        let mut timestamps = Vec::new();
        
        self.seek(0)?;
        self.pipeline.set_state(gst::State::Playing)
            .map_err(|e| VideoError::Gst(format!("Failed to start playback: {:?}", e)))?;
        
        while let Some(sample) = self.pull_sample(gst::ClockTime::from_mseconds(100))? {
            if let Some(buffer) = sample.buffer() {
                if let Some(pts) = buffer.pts() {
                    timestamps.push(pts.mseconds());
                }
            }
        }
        
        self.pipeline.set_state(gst::State::Paused)
            .map_err(|e| VideoError::Gst(format!("Failed to pause: {:?}", e)))?;
        
        self.frame_index = Some(timestamps.clone());
        self.position_ms = 0;
        
        Ok(timestamps)
    }
}

impl Drop for MjpegDemuxer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
