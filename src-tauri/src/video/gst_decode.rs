// Generic GStreamer decode demuxer
//
// Uses GStreamer's decodebin to decode any supported codec, then converts
// to JPEG frames for the frontend's custom frame player.
//
// Pipeline: filesrc → decodebin → videoconvert → jpegenc → appsink
//
// This handles codecs like FFV1 that aren't natively supported by HTML5
// but can be decoded by GStreamer. The decodebin element auto-detects
// the codec and selects the appropriate decoder.

use std::path::{Path, PathBuf};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use super::demux::{VideoDemuxer, VideoFrame, VideoInfo};
use super::VideoError;

/// Generic GStreamer decode demuxer — decodes video via GStreamer and outputs JPEG frames
pub struct GstDecodeDemuxer {
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

impl GstDecodeDemuxer {
    /// Open a video file and create a decode pipeline
    ///
    /// The `codec` parameter is used for the VideoInfo codec field — the actual
    /// decoding is handled automatically by decodebin.
    pub fn open<P: AsRef<Path>>(path: P, codec: &str) -> Result<Self, VideoError> {
        let path = path.as_ref().to_path_buf();

        gst::init().map_err(|e| VideoError::Gst(e.to_string()))?;

        // Build pipeline: filesrc → decodebin → videoconvert → jpegenc → appsink
        let pipeline = gst::Pipeline::new();

        let filesrc = gst::ElementFactory::make("filesrc")
            .property("location", path.to_string_lossy().to_string())
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create filesrc: {}", e)))?;

        let decodebin = gst::ElementFactory::make("decodebin")
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create decodebin: {}", e)))?;

        let videoconvert = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create videoconvert: {}", e)))?;

        let jpegenc = gst::ElementFactory::make("jpegenc")
            .property("quality", 95i32)
            .build()
            .map_err(|e| VideoError::Gst(format!("Failed to create jpegenc: {}", e)))?;

        let appsink = gst_app::AppSink::builder()
            .name("sink")
            .sync(false)
            .build();

        pipeline
            .add_many([
                &filesrc,
                &decodebin,
                &videoconvert,
                &jpegenc,
                appsink.upcast_ref(),
            ])
            .map_err(|e| VideoError::Gst(format!("Failed to add elements: {}", e)))?;

        // Link filesrc → decodebin (static link)
        filesrc
            .link(&decodebin)
            .map_err(|e| VideoError::Gst(format!("Failed to link filesrc to decodebin: {}", e)))?;

        // Link videoconvert → jpegenc → appsink (static link)
        gst::Element::link_many([&videoconvert, &jpegenc, appsink.upcast_ref()])
            .map_err(|e| {
                VideoError::Gst(format!(
                    "Failed to link videoconvert → jpegenc → appsink: {}",
                    e
                ))
            })?;

        // Connect decodebin pad-added signal to link video pads to videoconvert
        let videoconvert_weak = videoconvert.downgrade();
        decodebin.connect_pad_added(move |_decodebin, src_pad| {
            let Some(videoconvert) = videoconvert_weak.upgrade() else {
                return;
            };

            // Only link video pads (ignore audio, subtitles, etc.)
            let caps = src_pad
                .current_caps()
                .or_else(|| Some(src_pad.query_caps(None)));
            if let Some(caps) = caps {
                if let Some(structure) = caps.structure(0) {
                    let name = structure.name().as_str();
                    if name.starts_with("video/") {
                        let sink_pad = videoconvert.static_pad("sink").unwrap();
                        if !sink_pad.is_linked() {
                            if let Err(e) = src_pad.link(&sink_pad) {
                                log::warn!("GstDecodeDemuxer: Failed to link video pad: {:?}", e);
                            }
                        }
                    } else {
                        log::debug!(
                            "GstDecodeDemuxer: ignoring non-video pad with caps '{}'",
                            name
                        );
                    }
                }
            }
        });

        // Start pipeline in PAUSED state to preroll
        pipeline.set_state(gst::State::Paused).map_err(|e| {
            VideoError::Gst(format!("Failed to set pipeline to PAUSED: {:?}", e))
        })?;

        // Wait for preroll and get stream info
        let Some(bus) = pipeline.bus() else {
            pipeline.set_state(gst::State::Null).ok();
            return Err(VideoError::Gst("Failed to get pipeline bus".into()));
        };

        let mut width = 0u32;
        let mut height = 0u32;
        let mut duration_ms = 0u64;
        let codec_name = codec.to_string();

        // Wait for async-done or error
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(10)) {
            match msg.view() {
                gst::MessageView::AsyncDone(_) => {
                    // Get duration
                    if let Some(dur) = pipeline.query_duration::<gst::ClockTime>() {
                        duration_ms = dur.mseconds();
                    }

                    // Get video dimensions from appsink preroll sample
                    if let Some(sample) =
                        appsink.try_pull_preroll(gst::ClockTime::from_seconds(5))
                    {
                        if let Some(caps) = sample.caps() {
                            if let Some(structure) = caps.structure(0) {
                                width =
                                    structure.get::<i32>("width").unwrap_or(1280) as u32;
                                height =
                                    structure.get::<i32>("height").unwrap_or(720) as u32;
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

        // Use discoverer to get actual FPS
        let fps = Self::probe_fps(&path).unwrap_or(30.0);

        // Estimate frame count from duration and fps
        let frame_count = (duration_ms as f64 * fps / 1000.0) as u64;

        let info = VideoInfo {
            width,
            height,
            fps,
            duration_ms,
            frame_count,
            codec: codec_name,
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

    /// Probe the video file's FPS using GStreamer's discoverer
    fn probe_fps(path: &Path) -> Option<f64> {
        use gstreamer_pbutils as gst_pbutils;
        use gst_pbutils::prelude::*;

        let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(10)).ok()?;
        let uri = format!("file:///{}", path.to_string_lossy().replace('\\', "/"));
        let info = discoverer.discover_uri(&uri).ok()?;

        let video_streams = info.video_streams();
        let stream = video_streams.first()?;
        let caps = stream.caps()?;
        let structure = caps.structure(0)?;

        // Try to get framerate from caps
        if let Ok(fps) = structure.get::<gst::Fraction>("framerate") {
            let fps_val = fps.numer() as f64 / fps.denom() as f64;
            if fps_val > 0.0 {
                return Some(fps_val);
            }
        }

        None
    }

    /// Pull the next sample from the pipeline
    fn pull_sample(&self, timeout: gst::ClockTime) -> Result<Option<gst::Sample>, VideoError> {
        match self.appsink.try_pull_sample(timeout) {
            Some(sample) => Ok(Some(sample)),
            None => Ok(None),
        }
    }
}

impl VideoDemuxer for GstDecodeDemuxer {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn get_frame_at(&mut self, timestamp_ms: u64) -> Result<VideoFrame, VideoError> {
        self.seek(timestamp_ms)?;
        self.next_frame()?
            .ok_or(VideoError::FrameNotFound(timestamp_ms))
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, VideoError> {
        // Make sure pipeline is playing
        if self.pipeline.current_state() != gst::State::Playing {
            self.pipeline.set_state(gst::State::Playing).map_err(|e| {
                VideoError::Gst(format!("Failed to set PLAYING: {:?}", e))
            })?;

            // Wait for the state change to complete before pulling
            let _ = self.pipeline.state(gst::ClockTime::from_seconds(2));
        }

        // Pull next sample
        let sample = match self.pull_sample(gst::ClockTime::from_seconds(2))? {
            Some(s) => s,
            None => return Ok(None),
        };

        let buffer = sample
            .buffer()
            .ok_or_else(|| VideoError::Parse("No buffer in sample".into()))?;
        let pts = buffer
            .pts()
            .map(|t| t.mseconds())
            .unwrap_or(self.position_ms);
        let duration = buffer.duration().map(|t| t.mseconds()).unwrap_or(33);

        // Check if this is a keyframe via buffer flags
        let flags = buffer.flags();
        let is_keyframe = !flags.contains(gst::BufferFlags::DELTA_UNIT);

        let map = buffer
            .map_readable()
            .map_err(|e| VideoError::Parse(format!("Failed to map buffer: {}", e)))?;

        let frame = VideoFrame {
            data: map.as_slice().to_vec(),
            timestamp_ms: pts,
            duration_ms: duration,
            is_keyframe,
        };

        self.position_ms = pts + duration;

        Ok(Some(frame))
    }

    fn seek(&mut self, timestamp_ms: u64) -> Result<(), VideoError> {
        let seek_pos = gst::ClockTime::from_mseconds(timestamp_ms);

        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                seek_pos,
            )
            .map_err(|e| VideoError::Gst(format!("Seek failed: {:?}", e)))?;

        // Wait for seek to complete
        let Some(bus) = self.pipeline.bus() else {
            return Err(VideoError::Gst(
                "Failed to get pipeline bus for seek".into(),
            ));
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
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| VideoError::Gst(format!("Failed to start playback: {:?}", e)))?;

        while let Some(sample) = self.pull_sample(gst::ClockTime::from_mseconds(100))? {
            if let Some(buffer) = sample.buffer() {
                if let Some(pts) = buffer.pts() {
                    timestamps.push(pts.mseconds());
                }
            }
        }

        self.pipeline
            .set_state(gst::State::Paused)
            .map_err(|e| VideoError::Gst(format!("Failed to pause: {:?}", e)))?;

        self.frame_index = Some(timestamps.clone());
        self.position_ms = 0;

        Ok(timestamps)
    }
}

impl Drop for GstDecodeDemuxer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
