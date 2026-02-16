// MIDI monitoring service that triggers automatic recording

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::io::{Write, Seek, SeekFrom};
use std::collections::HashMap;
use parking_lot::{RwLock, Mutex};
use midir::{MidiInput, MidiInputConnection};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Manager, Emitter};

use crate::config::Config;
use crate::devices::DeviceManager;
use crate::recording::RecordingState;
use crate::recording::midi::TimestampedMidiEvent;
use crate::recording::preroll::{MidiPrerollBuffer, AudioPrerollBuffer, MAX_PRE_ROLL_SECS, MAX_PRE_ROLL_SECS_ENCODED};
use crate::recording::video::VideoCaptureManager;
use crate::session::{SessionMetadata, SessionDatabase, MidiFileInfo, AudioFileInfo};
use crate::notifications;

/// Streaming audio writer that pipes samples to disk via GStreamer.
/// Pipeline: appsrc(F32LE) ! audioconvert ! audioresample ! capsfilter ! encoder(flacenc/wavenc) ! filesink
pub struct AudioStreamWriter {
    pipeline: gstreamer::Pipeline,
    appsrc: gstreamer_app::AppSrc,
    file_path: PathBuf,
    filename: String,
    device_name: String,
    channels: u16,
    /// Native input sample rate from cpal
    native_rate: u32,
    /// Output sample rate (after resampling, or native if passthrough)
    output_rate: u32,
    /// Total frames pushed (for PTS / duration calculation)
    frames_pushed: u64,
}

impl AudioStreamWriter {
    /// Create and start a new streaming audio writer.
    pub fn new(
        session_path: &PathBuf,
        filename: &str,
        device_name: &str,
        channels: u16,
        native_rate: u32,
        audio_format: &crate::config::AudioFormat,
        bit_depth: &crate::config::AudioBitDepth,
        sample_rate_setting: &crate::config::AudioSampleRate,
    ) -> anyhow::Result<Self> {
        use gstreamer as gst;
        use gstreamer::prelude::*;
        use gstreamer_app as gst_app;
        use gstreamer_audio as gst_audio;
        
        let file_path = session_path.join(filename);
        let output_rate = sample_rate_setting.target_rate().unwrap_or(native_rate);
        
        // Input caps: F32LE at the device's native rate
        let input_info = gst_audio::AudioInfo::builder(gst_audio::AudioFormat::F32le, native_rate, channels as u32)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create input audio info: {}", e))?;
        
        // Target format for the capsfilter (depends on format + bit_depth)
        let target_format = match (audio_format, bit_depth) {
            (crate::config::AudioFormat::Wav, crate::config::AudioBitDepth::Int16) => gst_audio::AudioFormat::S16le,
            (crate::config::AudioFormat::Wav, crate::config::AudioBitDepth::Int24) => gst_audio::AudioFormat::S24le,
            (crate::config::AudioFormat::Wav, crate::config::AudioBitDepth::Float32) => gst_audio::AudioFormat::F32le,
            (crate::config::AudioFormat::Flac, crate::config::AudioBitDepth::Int16) => gst_audio::AudioFormat::S16le,
            (crate::config::AudioFormat::Flac, crate::config::AudioBitDepth::Int24) => gst_audio::AudioFormat::S2432le,
            (crate::config::AudioFormat::Flac, crate::config::AudioBitDepth::Float32) => gst_audio::AudioFormat::S32le,
        };
        
        // Target caps for the capsfilter (format + rate + channel-mask)
        let target_info = gst_audio::AudioInfo::builder(target_format, output_rate, channels as u32)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create target audio info: {}", e))?;
        
        // Build pipeline elements
        let pipeline = gst::Pipeline::new();
        
        let appsrc = gst_app::AppSrc::builder()
            .name("src")
            .caps(&input_info.to_caps().map_err(|e| anyhow::anyhow!("Failed to create input caps: {}", e))?)
            .format(gst::Format::Time)
            .build();
        
        let audioconvert = gst::ElementFactory::make("audioconvert")
            .name("convert")
            .build()
            .map_err(|_| anyhow::anyhow!("Failed to create audioconvert element"))?;
        
        let audioresample = gst::ElementFactory::make("audioresample")
            .name("resample")
            .build()
            .map_err(|_| anyhow::anyhow!("Failed to create audioresample element"))?;
        
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name("filter")
            .property("caps", target_info.to_caps().map_err(|e| anyhow::anyhow!("Failed to create target caps: {}", e))?)
            .build()
            .map_err(|_| anyhow::anyhow!("Failed to create capsfilter element"))?;
        
        // Encoder: flacenc or wavenc
        let encoder_name = match audio_format {
            crate::config::AudioFormat::Flac => "flacenc",
            crate::config::AudioFormat::Wav => "wavenc",
        };
        let encoder = gst::ElementFactory::make(encoder_name)
            .name("encoder")
            .build()
            .map_err(|_| anyhow::anyhow!("Failed to create {} element", encoder_name))?;
        
        // For 32-bit FLAC, disable the Subset restriction (Subset limits to 24-bit max)
        if matches!(audio_format, crate::config::AudioFormat::Flac)
            && matches!(bit_depth, crate::config::AudioBitDepth::Float32)
        {
            encoder.set_property("streamable-subset", false);
        }
        
        let filesink = gst::ElementFactory::make("filesink")
            .name("sink")
            .property("location", file_path.to_str().unwrap_or("output"))
            .build()
            .map_err(|_| anyhow::anyhow!("Failed to create filesink element"))?;
        
        // Assemble and link
        pipeline.add_many([appsrc.upcast_ref(), &audioconvert, &audioresample, &capsfilter, &encoder, &filesink])
            .map_err(|e| anyhow::anyhow!("Failed to add elements to pipeline: {}", e))?;
        
        gst::Element::link_many([appsrc.upcast_ref(), &audioconvert, &audioresample, &capsfilter, &encoder, &filesink])
            .map_err(|e| anyhow::anyhow!("Failed to link pipeline elements: {}", e))?;
        
        // Start the pipeline
        pipeline.set_state(gst::State::Playing)
            .map_err(|e| anyhow::anyhow!("Failed to start audio pipeline: {}", e))?;
        
        println!("[Sacho] Audio streaming started: {} -> {} ({}Hz {}ch -> {}Hz {})",
            device_name, filename, native_rate, channels, output_rate, encoder_name);
        
        Ok(Self {
            pipeline,
            appsrc,
            file_path,
            filename: filename.to_string(),
            device_name: device_name.to_string(),
            channels,
            native_rate,
            output_rate,
            frames_pushed: 0,
        })
    }
    
    /// Push interleaved f32 samples to the pipeline.
    pub fn push_samples(&mut self, data: &[f32]) {
        use gstreamer as gst;
        
        if data.is_empty() {
            return;
        }
        
        let num_frames = data.len() / self.channels as usize;
        
        // Calculate PTS and duration based on frames pushed so far
        let pts_ns = self.frames_pushed * 1_000_000_000 / self.native_rate as u64;
        let duration_ns = num_frames as u64 * 1_000_000_000 / self.native_rate as u64;
        
        // Convert f32 samples to F32LE bytes
        let bytes: Vec<u8> = data.iter().copied().flat_map(f32::to_le_bytes).collect();
        
        let mut buffer = gst::Buffer::from_slice(bytes);
        {
            let buf_ref = buffer.get_mut().unwrap();
            buf_ref.set_pts(gst::ClockTime::from_nseconds(pts_ns));
            buf_ref.set_duration(gst::ClockTime::from_nseconds(duration_ns));
        }
        
        if let Err(e) = self.appsrc.push_buffer(buffer) {
            println!("[Sacho] Audio push error for {}: {}", self.device_name, e);
        }
        
        self.frames_pushed += num_frames as u64;
    }
    
    /// Push silence for padding (e.g., to match video duration).
    pub fn push_silence(&mut self, duration_secs: f64) {
        let num_frames = (duration_secs * self.native_rate as f64) as usize;
        let total_samples = num_frames * self.channels as usize;
        let silence = vec![0.0f32; total_samples];
        self.push_samples(&silence);
    }
    
    /// Finalize the stream: send EOS, wait for completion, return file info.
    pub fn finish(self) -> anyhow::Result<AudioFileInfo> {
        use gstreamer as gst;
        use gstreamer::prelude::*;
        
        // Signal end of stream
        self.appsrc.end_of_stream()
            .map_err(|e| anyhow::anyhow!("Failed to send EOS: {}", e))?;
        
        // Wait for the pipeline to finish processing
        let bus = self.pipeline.bus().ok_or_else(|| anyhow::anyhow!("No pipeline bus for audio finalization"))?;
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(30)) {
            match msg.view() {
                gst::MessageView::Eos(..) => break,
                gst::MessageView::Error(err) => {
                    self.pipeline.set_state(gst::State::Null).ok();
                    return Err(anyhow::anyhow!(
                        "Audio encoding error for {}: {} ({})",
                        self.device_name,
                        err.error(),
                        err.debug().unwrap_or_default()
                    ));
                }
                _ => {}
            }
        }
        
        self.pipeline.set_state(gst::State::Null).ok();
        
        let size = std::fs::metadata(&self.file_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let duration_secs = self.frames_pushed as f64 / self.native_rate as f64;
        
        println!("[Sacho] Audio stream finished: {} ({:.1}s, {} bytes)", self.filename, duration_secs, size);
        
        Ok(AudioFileInfo {
            filename: self.filename,
            device_name: self.device_name,
            channels: self.channels,
            sample_rate: self.output_rate,
            duration_secs,
            size_bytes: size,
        })
    }
}

/// Streaming MIDI file writer that writes events to disk incrementally.
/// Writes SMF (Standard MIDI File) format 0 with one track.
/// The MTrk length is a placeholder until finish() patches it.
/// If the app crashes, repair_midi_file() can fix the header.
pub struct MidiStreamWriter {
    file: std::fs::File,
    filename: String,
    device_name: String,
    last_tick: u64,
    event_count: usize,
    /// Number of track data bytes written (after the MTrk header)
    track_data_bytes: u32,
    ticks_per_us: f64,
    /// Last time the file was flushed to disk
    last_flush: Instant,
    /// Count of write errors (logged on first occurrence, summarized in finish())
    write_errors: u32,
}

impl MidiStreamWriter {
    /// MIDI timing: 480 ticks per quarter note at 120 BPM (500000 us per beat)
    const TICKS_PER_QUARTER: u16 = 480;
    const US_PER_QUARTER: f64 = 500_000.0;
    
    /// Create a new MIDI stream writer and write the file header.
    pub fn new(session_path: &PathBuf, filename: &str, device_name: &str) -> anyhow::Result<Self> {
        let file_path = session_path.join(filename);
        let mut file = std::fs::File::create(&file_path)?;
        
        // MThd header
        file.write_all(b"MThd")?;
        file.write_all(&[0, 0, 0, 6])?;           // Header length
        file.write_all(&[0, 0])?;                   // Format 0
        file.write_all(&[0, 1])?;                   // 1 track
        file.write_all(&Self::TICKS_PER_QUARTER.to_be_bytes())?;
        
        // MTrk header with placeholder length
        file.write_all(b"MTrk")?;
        file.write_all(&[0, 0, 0, 0])?;             // Length placeholder (patched at finish)
        
        file.flush()?;
        
        println!("[Sacho] MIDI streaming started: {} -> {}", device_name, filename);
        
        Ok(Self {
            file,
            filename: filename.to_string(),
            device_name: device_name.to_string(),
            last_tick: 0,
            event_count: 0,
            track_data_bytes: 0,
            ticks_per_us: Self::TICKS_PER_QUARTER as f64 / Self::US_PER_QUARTER,
            last_flush: Instant::now(),
            write_errors: 0,
        })
    }

    /// Push a single MIDI event to the file.
    pub fn push_event(&mut self, event: &TimestampedMidiEvent) {
        let tick = (event.timestamp_us as f64 * self.ticks_per_us) as u64;
        let delta = tick.saturating_sub(self.last_tick);
        self.last_tick = tick;

        // Write variable-length delta time
        let delta_bytes = Self::encode_variable_length(delta as u32);
        if let Err(e) = self.file.write_all(&delta_bytes) {
            self.write_errors += 1;
            if self.write_errors == 1 {
                println!("[Sacho] MIDI write error for {}: {}", self.device_name, e);
            }
            return;
        }

        // Write event data
        if let Err(e) = self.file.write_all(&event.data) {
            self.write_errors += 1;
            if self.write_errors == 1 {
                println!("[Sacho] MIDI write error for {}: {}", self.device_name, e);
            }
            return;
        }
        
        self.track_data_bytes += delta_bytes.len() as u32 + event.data.len() as u32;
        self.event_count += 1;
        
        // Flush periodically (every 100ms) to balance crash safety and I/O overhead
        if self.last_flush.elapsed() >= Duration::from_millis(100) {
            let _ = self.file.flush();
            self.last_flush = Instant::now();
        }
    }
    
    /// Finalize: write end-of-track marker and patch the MTrk length.
    pub fn finish(mut self) -> anyhow::Result<MidiFileInfo> {
        // Write end-of-track: delta=0, meta event FF 2F 00
        self.file.write_all(&[0x00, 0xFF, 0x2F, 0x00])?;
        self.track_data_bytes += 4;
        
        // Patch MTrk length at byte offset 18
        self.file.seek(SeekFrom::Start(18))?;
        self.file.write_all(&self.track_data_bytes.to_be_bytes())?;
        self.file.flush()?;
        
        let size = self.file.metadata().map(|m| m.len()).unwrap_or(0);

        if self.write_errors > 0 {
            println!("[Sacho] MIDI stream for {} had {} write errors", self.device_name, self.write_errors);
        }

        println!("[Sacho] MIDI stream finished: {} ({} events, {} bytes)",
            self.filename, self.event_count, size);
        
        Ok(MidiFileInfo {
            filename: self.filename,
            device_name: self.device_name,
            event_count: self.event_count,
            size_bytes: size,
            needs_repair: false,
        })
    }
    
    /// Encode a value as MIDI variable-length quantity.
    fn encode_variable_length(mut value: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4);
        bytes.push((value & 0x7F) as u8);
        value >>= 7;
        while value > 0 {
            bytes.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        bytes.reverse();
        bytes
    }
}

// ============================================================================
// File integrity checking and repair
// ============================================================================

/// Check if a MIDI file has a valid MTrk header length.
/// Returns true if the file needs repair (header length doesn't match actual data).
pub fn midi_file_needs_repair(file_path: &PathBuf) -> bool {
    use std::io::Read;
    
    let Ok(mut file) = std::fs::File::open(file_path) else { return false; };
    let Ok(metadata) = file.metadata() else { return false; };
    let file_size = metadata.len();
    
    // Minimum valid MIDI: 14 (MThd) + 8 (MTrk header) = 22
    if file_size < 22 { return false; }
    
    // Read MThd header
    let mut header = [0u8; 14];
    if file.read_exact(&mut header).is_err() { return false; }
    if &header[0..4] != b"MThd" { return false; }
    
    // Read MTrk header
    let mut mtrk = [0u8; 8];
    if file.read_exact(&mut mtrk).is_err() { return false; }
    if &mtrk[0..4] != b"MTrk" { return false; }
    
    let stored_length = u32::from_be_bytes([mtrk[4], mtrk[5], mtrk[6], mtrk[7]]);
    let actual_data_length = file_size - 22;
    
    stored_length as u64 != actual_data_length
}

/// Repair a MIDI file by fixing the MTrk header length and ensuring end-of-track marker.
/// Returns the updated event count estimate.
pub fn repair_midi_file_on_disk(file_path: &PathBuf) -> anyhow::Result<usize> {
    use std::io::Read;
    
    let mut file = std::fs::OpenOptions::new()
        .read(true).write(true).open(file_path)?;
    let file_size = file.metadata()?.len();
    
    if file_size < 22 {
        return Err(anyhow::anyhow!("File too small to be a valid MIDI file"));
    }
    
    // Verify MThd and MTrk headers
    let mut header = [0u8; 22];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"MThd" || &header[14..18] != b"MTrk" {
        return Err(anyhow::anyhow!("Not a valid MIDI file"));
    }
    
    // Check if end-of-track marker (FF 2F 00) exists at end of file
    let has_eot = if file_size >= 25 {
        file.seek(SeekFrom::End(-3))?;
        let mut tail = [0u8; 3];
        file.read_exact(&mut tail)?;
        tail == [0xFF, 0x2F, 0x00]
    } else {
        false
    };
    
    if !has_eot {
        // Append end-of-track: delta=0 + FF 2F 00
        file.seek(SeekFrom::End(0))?;
        file.write_all(&[0x00, 0xFF, 0x2F, 0x00])?;
    }
    
    // Calculate and patch MTrk length
    let new_file_size = file.metadata()?.len();
    let track_data_length = (new_file_size - 22) as u32;
    
    file.seek(SeekFrom::Start(18))?;
    file.write_all(&track_data_length.to_be_bytes())?;
    file.flush()?;
    
    // Estimate event count from track data (each event is ~4 bytes on average)
    let event_count = track_data_length.saturating_sub(4) as usize / 4;
    
    println!("[Sacho] Repaired MIDI file: {} ({} bytes, ~{} events)",
        file_path.display(), new_file_size, event_count);
    
    Ok(event_count)
}

/// Check if a WAV file has a valid RIFF header (chunk sizes match file size).
/// WAV structure: RIFF[4] size[4] WAVE[4] ... fmt [4] ... data[4] size[4] ...
pub fn wav_file_needs_repair(file_path: &PathBuf) -> bool {
    use std::io::Read;
    
    let Ok(mut file) = std::fs::File::open(file_path) else { return false; };
    let Ok(meta) = file.metadata() else { return false; };
    let file_size = meta.len();
    
    if file_size < 44 { return false; } // Minimum WAV header
    
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() { return false; }
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" { return false; }
    
    let stored_riff_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let expected_riff_size = (file_size - 8) as u32;
    
    stored_riff_size != expected_riff_size
}

/// Repair a WAV file by fixing the RIFF and data chunk sizes.
/// Returns (channels, sample_rate, duration_secs, size_bytes).
pub fn repair_wav_file(file_path: &PathBuf) -> anyhow::Result<(u16, u32, f64, u64)> {
    use std::io::Read;
    
    let mut file = std::fs::OpenOptions::new()
        .read(true).write(true).open(file_path)?;
    let file_size = file.metadata()?.len();
    
    if file_size < 44 {
        return Err(anyhow::anyhow!("File too small to be a valid WAV file"));
    }
    
    // Read and verify RIFF header
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(anyhow::anyhow!("Not a valid WAV file"));
    }
    
    // Scan chunks to find fmt and data
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut bits_per_sample: u16 = 0;
    let mut data_chunk_offset: u64 = 0;
    
    let mut pos: u64 = 12;
    loop {
        file.seek(SeekFrom::Start(pos))?;
        let mut chunk_header = [0u8; 8];
        if file.read_exact(&mut chunk_header).is_err() { break; }
        
        let chunk_id = &chunk_header[0..4];
        let chunk_size = u32::from_le_bytes([chunk_header[4], chunk_header[5], chunk_header[6], chunk_header[7]]);
        
        if chunk_id == b"fmt " {
            let mut fmt = [0u8; 16];
            file.read_exact(&mut fmt)?;
            channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);
        } else if chunk_id == b"data" {
            data_chunk_offset = pos;
            break;
        }
        
        // Move to next chunk (chunk size + 8 for header, padded to even)
        pos += 8 + chunk_size as u64;
        if chunk_size % 2 != 0 { pos += 1; } // WAV chunks are 2-byte aligned
    }
    
    if data_chunk_offset == 0 || channels == 0 {
        return Err(anyhow::anyhow!("Could not find fmt/data chunks"));
    }
    
    // Calculate correct sizes
    let data_size = (file_size - data_chunk_offset - 8) as u32;
    let riff_size = (file_size - 8) as u32;
    
    // Patch RIFF size (bytes 4-7)
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&riff_size.to_le_bytes())?;
    
    // Patch data chunk size (4 bytes after "data" tag)
    file.seek(SeekFrom::Start(data_chunk_offset + 4))?;
    file.write_all(&data_size.to_le_bytes())?;
    file.flush()?;
    
    // Calculate duration
    let bytes_per_sample = bits_per_sample as u32 / 8;
    let bytes_per_frame = bytes_per_sample * channels as u32;
    let duration_secs = if bytes_per_frame > 0 && sample_rate > 0 {
        data_size as f64 / (sample_rate as f64 * bytes_per_frame as f64)
    } else {
        0.0
    };
    
    println!("[Sacho] Repaired WAV file: {} ({}Hz, {}ch, {:.1}s)",
        file_path.display(), sample_rate, channels, duration_secs);
    
    Ok((channels, sample_rate, duration_secs, file_size))
}

/// Check if a FLAC file has an unfinalized STREAMINFO block (total_samples == 0).
pub fn flac_file_needs_repair(file_path: &PathBuf) -> bool {
    use std::io::Read;
    
    let Ok(mut file) = std::fs::File::open(file_path) else { return false; };
    let Ok(meta) = file.metadata() else { return false; };
    if meta.len() < 42 { return false; } // fLaC marker + STREAMINFO block
    
    // Read fLaC marker
    let mut marker = [0u8; 4];
    if file.read_exact(&mut marker).is_err() { return false; }
    if &marker != b"fLaC" { return false; }
    
    // Read STREAMINFO block header (1 byte type + 3 bytes length)
    let mut block_header = [0u8; 4];
    if file.read_exact(&mut block_header).is_err() { return false; }
    
    // STREAMINFO is always block type 0
    let block_type = block_header[0] & 0x7F;
    if block_type != 0 { return false; }
    
    // Read STREAMINFO data (34 bytes)
    let mut streaminfo = [0u8; 34];
    if file.read_exact(&mut streaminfo).is_err() { return false; }
    
    // Total samples is stored in bytes 14-17 (upper 4 bits of byte 13 + bytes 14-17)
    // Layout: byte 13 has [4 bits sample_size_minus1 | 4 bits total_samples_hi]
    // bytes 14-17 have total_samples_lo (32 bits)
    let total_samples_hi = (streaminfo[13] & 0x0F) as u64;
    let total_samples_lo = u32::from_be_bytes([streaminfo[14], streaminfo[15], streaminfo[16], streaminfo[17]]) as u64;
    let total_samples = (total_samples_hi << 32) | total_samples_lo;
    
    total_samples == 0
}

/// Repair a FLAC file by using GStreamer to determine the accurate duration,
/// then patching total_samples in the STREAMINFO block.
/// Returns (channels, sample_rate, duration_secs, size_bytes).
pub fn repair_flac_file(file_path: &PathBuf) -> anyhow::Result<(u16, u32, f64, u64)> {
    use std::io::Read;
    use gstreamer as gst;
    use gstreamer::prelude::*;
    
    let file_size = std::fs::metadata(file_path)?.len();
    
    if file_size < 42 {
        return Err(anyhow::anyhow!("File too small to be a valid FLAC file"));
    }
    
    // Step 1: Read STREAMINFO to get sample_rate and channels
    let (sample_rate, channels) = {
        let mut file = std::fs::File::open(file_path)?;
        
        let mut marker = [0u8; 4];
        file.read_exact(&mut marker)?;
        if &marker != b"fLaC" {
            return Err(anyhow::anyhow!("Not a valid FLAC file"));
        }
        
        let mut block_header = [0u8; 4];
        file.read_exact(&mut block_header)?;
        if (block_header[0] & 0x7F) != 0 {
            return Err(anyhow::anyhow!("First metadata block is not STREAMINFO"));
        }
        
        let mut streaminfo = [0u8; 34];
        file.read_exact(&mut streaminfo)?;
        
        let sr = ((streaminfo[10] as u32) << 12)
            | ((streaminfo[11] as u32) << 4)
            | ((streaminfo[12] as u32) >> 4);
        let ch = ((streaminfo[12] >> 1) & 0x07) as u16 + 1;
        
        (sr, ch)
    };
    
    // Step 2: Use GStreamer flacparse to get accurate duration by parsing all frames
    let pipeline_str = format!(
        "filesrc location=\"{}\" ! flacparse ! fakesink",
        file_path.to_string_lossy().replace('\\', "/")
    );
    
    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| anyhow::anyhow!("Failed to create FLAC parse pipeline: {}", e))?;
    let pipeline = pipeline.dynamic_cast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to pipeline"))?;
    
    pipeline.set_state(gst::State::Playing)
        .map_err(|e| anyhow::anyhow!("Failed to start FLAC parse: {}", e))?;
    
    let bus = pipeline.bus().ok_or_else(|| anyhow::anyhow!("No pipeline bus for FLAC repair"))?;
    let mut duration_secs = 0.0;

    for msg in bus.iter_timed(gst::ClockTime::from_seconds(60)) {
        match msg.view() {
            gst::MessageView::Eos(..) => {
                // Query duration after all frames have been parsed
                if let Some(dur) = pipeline.query_duration::<gst::ClockTime>() {
                    duration_secs = dur.nseconds() as f64 / 1_000_000_000.0;
                }
                break;
            }
            gst::MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).ok();
                return Err(anyhow::anyhow!(
                    "FLAC parse error: {} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
            }
            _ => {}
        }
    }
    
    pipeline.set_state(gst::State::Null).ok();
    
    // Step 3: Calculate total_samples and patch STREAMINFO
    let total_samples = if sample_rate > 0 {
        (duration_secs * sample_rate as f64).round() as u64
    } else {
        0
    };
    
    {
        let mut file = std::fs::OpenOptions::new()
            .read(true).write(true).open(file_path)?;
        
        // Patch total_samples in STREAMINFO
        // Byte 13 (offset 4+4+13=21): lower 4 bits = total_samples upper 4 bits
        // Bytes 14-17 (offset 22-25) = total_samples lower 32 bits
        let ts_hi = ((total_samples >> 32) & 0x0F) as u8;
        let ts_lo = (total_samples & 0xFFFFFFFF) as u32;
        
        file.seek(SeekFrom::Start(4 + 4 + 13))?; // offset to byte 13 of streaminfo
        let mut byte13 = [0u8; 1];
        file.read_exact(&mut byte13)?;
        byte13[0] = (byte13[0] & 0xF0) | ts_hi;
        
        file.seek(SeekFrom::Start(4 + 4 + 13))?;
        file.write_all(&byte13)?;
        file.write_all(&ts_lo.to_be_bytes())?;
        file.flush()?;
    }
    
    println!("[Sacho] Repaired FLAC file: {} ({}Hz, {}ch, {:.1}s, {} total samples)",
        file_path.display(), sample_rate, channels, duration_secs, total_samples);
    
    Ok((channels, sample_rate, duration_secs, file_size))
}

/// Check if a Matroska file is unfinalized (missing duration or has zero segment size).
pub fn video_file_needs_repair(file_path: &PathBuf) -> bool {
    use std::io::Read;
    
    let Ok(mut file) = std::fs::File::open(file_path) else { return false; };
    let Ok(meta) = file.metadata() else { return false; };
    if meta.len() < 32 { return false; }
    
    // EBML header starts with 0x1A45DFA3
    let mut header = [0u8; 4];
    if file.read_exact(&mut header).is_err() { return false; }
    if header != [0x1A, 0x45, 0xDF, 0xA3] { return false; }
    
    // Read the file looking for Segment Duration element (0x4489)
    // A quick heuristic: scan the first 1KB for the Duration element
    file.seek(SeekFrom::Start(0)).ok();
    let scan_size = meta.len().min(4096) as usize;
    let mut buf = vec![0u8; scan_size];
    if file.read_exact(&mut buf).is_err() { return false; }
    
    // Look for Duration element ID (0x44 0x89) followed by a float
    for i in 0..buf.len().saturating_sub(12) {
        if buf[i] == 0x44 && buf[i + 1] == 0x89 {
            // Found Duration element. Check if the float value is 0.0
            // The size byte follows, then the float data
            if i + 2 < buf.len() {
                let size = buf[i + 2];
                if size == 0x88 && i + 11 < buf.len() {
                    // 8-byte float (most common)
                    let val = f64::from_be_bytes([
                        buf[i+3], buf[i+4], buf[i+5], buf[i+6],
                        buf[i+7], buf[i+8], buf[i+9], buf[i+10]
                    ]);
                    return val == 0.0;
                } else if size == 0x84 && i + 7 < buf.len() {
                    // 4-byte float
                    let val = f32::from_be_bytes([buf[i+3], buf[i+4], buf[i+5], buf[i+6]]);
                    return val == 0.0;
                }
            }
            return false; // Found duration element, value is non-zero
        }
    }
    
    // No Duration element found at all - needs repair
    true
}

/// Repair a video file (MKV) by remuxing through GStreamer to fix container metadata.
/// The remuxed file replaces the original.
/// Returns (duration_secs, size_bytes).
pub fn repair_video_file(file_path: &PathBuf) -> anyhow::Result<(f64, u64)> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    
    let extension = file_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mkv");
    
    // Create a temp file for the remuxed output
    let temp_path = file_path.with_extension(format!("{}.repair.tmp", extension));
    
    // Build pipeline: filesrc ! matroskademux ! matroskamux ! filesink
    let pipeline_str = format!(
        "filesrc location=\"{}\" ! matroskademux name=demux ! queue ! matroskamux name=mux ! filesink location=\"{}\"",
        file_path.to_string_lossy().replace('\\', "/"),
        temp_path.to_string_lossy().replace('\\', "/"),
    );
    
    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| anyhow::anyhow!("Failed to create remux pipeline: {}", e))?;
    
    let pipeline = pipeline.dynamic_cast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to pipeline"))?;
    
    // Start and wait for completion
    pipeline.set_state(gst::State::Playing)
        .map_err(|e| anyhow::anyhow!("Failed to start remux: {}", e))?;
    
    let bus = pipeline.bus().ok_or_else(|| anyhow::anyhow!("No pipeline bus for video remux repair"))?;
    let mut duration_secs = 0.0;

    for msg in bus.iter_timed(gst::ClockTime::from_seconds(120)) {
        match msg.view() {
            gst::MessageView::Eos(..) => {
                // Query duration before stopping
                if let Some(dur) = pipeline.query_duration::<gst::ClockTime>() {
                    duration_secs = dur.nseconds() as f64 / 1_000_000_000.0;
                }
                break;
            }
            gst::MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).ok();
                // Clean up temp file
                let _ = std::fs::remove_file(&temp_path);
                return Err(anyhow::anyhow!(
                    "Video remux error: {} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
            }
            _ => {}
        }
    }
    
    pipeline.set_state(gst::State::Null).ok();
    
    // Replace original with remuxed file
    if temp_path.exists() {
        std::fs::rename(&temp_path, file_path)
            .map_err(|e| anyhow::anyhow!("Failed to replace original video: {}", e))?;
    }
    
    let size = std::fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);
    
    println!("[Sacho] Repaired video file: {} ({:.1}s, {} bytes)",
        file_path.display(), duration_secs, size);
    
    Ok((duration_secs, size))
}

/// Combine a video MKV and an audio file into a single MKV with both tracks.
/// The combined file replaces the original video file. Returns the new file size.
pub fn combine_audio_video_mkv(
    video_path: &PathBuf,
    audio_path: &PathBuf,
    audio_format: &crate::config::AudioFormat,
) -> anyhow::Result<u64> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    
    println!("[Sacho] Combining audio+video into single MKV: {:?} + {:?}",
        video_path.file_name().unwrap_or_default(),
        audio_path.file_name().unwrap_or_default());
    
    let temp_path = video_path.with_extension("mkv.combine.tmp");
    
    // Build pipeline manually so we can handle dynamic pads from matroskademux
    let pipeline = gst::Pipeline::new();
    
    // ── Video source: filesrc ! matroskademux (dynamic pads) ──
    let video_filesrc = gst::ElementFactory::make("filesrc")
        .property("location", video_path.to_string_lossy().to_string())
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create video filesrc: {}", e))?;
    
    let demux = gst::ElementFactory::make("matroskademux")
        .name("demux")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create matroskademux: {}", e))?;
    
    let video_queue = gst::ElementFactory::make("queue")
        .name("vqueue")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create video queue: {}", e))?;
    
    // ── Audio source: filesrc ! parser ──
    let audio_filesrc = gst::ElementFactory::make("filesrc")
        .property("location", audio_path.to_string_lossy().to_string())
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create audio filesrc: {}", e))?;
    
    let audio_parser_name = match audio_format {
        crate::config::AudioFormat::Flac => "flacparse",
        crate::config::AudioFormat::Wav => "wavparse",
    };
    let audio_parser = gst::ElementFactory::make(audio_parser_name)
        .name("aparser")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", audio_parser_name, e))?;
    
    let audio_queue = gst::ElementFactory::make("queue")
        .name("aqueue")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create audio queue: {}", e))?;
    
    // ── Muxer and sink ──
    let mux = gst::ElementFactory::make("matroskamux")
        .name("mux")
        .property("writing-app", "Sacho")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create matroskamux: {}", e))?;
    
    let filesink = gst::ElementFactory::make("filesink")
        .property("location", temp_path.to_string_lossy().to_string())
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create filesink: {}", e))?;
    
    // Add all elements to the pipeline
    pipeline.add_many([
        &video_filesrc, &demux, &video_queue,
        &audio_filesrc, &audio_parser, &audio_queue,
        &mux, &filesink,
    ]).map_err(|e| anyhow::anyhow!("Failed to add elements: {}", e))?;
    
    // Static links
    video_filesrc.link(&demux)
        .map_err(|e| anyhow::anyhow!("Failed to link video filesrc -> demux: {}", e))?;
    video_queue.link(&mux)
        .map_err(|e| anyhow::anyhow!("Failed to link video queue -> mux: {}", e))?;
    audio_filesrc.link(&audio_parser)
        .map_err(|e| anyhow::anyhow!("Failed to link audio filesrc -> parser: {}", e))?;
    audio_parser.link(&audio_queue)
        .map_err(|e| anyhow::anyhow!("Failed to link audio parser -> queue: {}", e))?;
    audio_queue.link(&mux)
        .map_err(|e| anyhow::anyhow!("Failed to link audio queue -> mux: {}", e))?;
    mux.link(&filesink)
        .map_err(|e| anyhow::anyhow!("Failed to link mux -> filesink: {}", e))?;
    
    // Handle dynamic pads from matroskademux (video stream)
    let vqueue_weak = video_queue.downgrade();
    demux.connect_pad_added(move |_demux, src_pad| {
        let pad_name = src_pad.name();
        // Only link video pads; the original MKV should only have video
        if pad_name.starts_with("video") {
            if let Some(queue) = vqueue_weak.upgrade() {
                if let Some(sink_pad) = queue.static_pad("sink") {
                    if !sink_pad.is_linked() {
                        if let Err(e) = src_pad.link(&sink_pad) {
                            println!("[Sacho] Warning: Failed to link demux video pad: {:?}", e);
                        }
                    }
                }
            }
        } else {
            println!("[Sacho] Ignoring demux pad: {} (only taking video)", pad_name);
        }
    });
    
    // Run the pipeline
    pipeline.set_state(gst::State::Playing)
        .map_err(|e| anyhow::anyhow!("Failed to start combine pipeline: {:?}", e))?;
    
    let bus = pipeline.bus().ok_or_else(|| anyhow::anyhow!("No pipeline bus for audio+video combine"))?;
    for msg in bus.iter_timed(gst::ClockTime::from_seconds(300)) {
        match msg.view() {
            gst::MessageView::Eos(..) => {
                println!("[Sacho] Audio+video combine complete");
                break;
            }
            gst::MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).ok();
                let _ = std::fs::remove_file(&temp_path);
                return Err(anyhow::anyhow!(
                    "Combine error: {} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
            }
            _ => {}
        }
    }
    
    pipeline.set_state(gst::State::Null).ok();
    
    // Replace original video file with the combined file
    let new_size = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
    if new_size > 0 {
        std::fs::remove_file(video_path)
            .map_err(|e| anyhow::anyhow!("Failed to remove original video: {}", e))?;
        std::fs::rename(&temp_path, video_path)
            .map_err(|e| anyhow::anyhow!("Failed to rename combined file: {}", e))?;
        
        println!("[Sacho] Combined audio+video: {} ({} bytes)",
            video_path.file_name().unwrap_or_default().to_string_lossy(), new_size);
        
        Ok(new_size)
    } else {
        let _ = std::fs::remove_file(&temp_path);
        Err(anyhow::anyhow!("Combine produced empty file"))
    }
}

/// Per-device audio trigger amplitude tracking state
pub struct AudioTriggerState {
    pub device_name: String,
    pub threshold: f64,
    /// Running sum of squared samples for current 50ms window
    window_sum_sq: f64,
    /// Number of samples accumulated in current window
    window_sample_count: usize,
    /// Total samples per 50ms window (sample_rate * channels / 20)
    samples_per_window: usize,
    /// Recent RMS values for 3-second peak hold (timestamp, rms)
    recent_rms: std::collections::VecDeque<(Instant, f32)>,
    /// Latest 50ms window RMS, read by level poller
    pub current_rms: f32,
    /// Max of recent_rms (3s peak hold), read by level poller
    pub current_peak_level: f32,
}

impl AudioTriggerState {
    pub fn new(device_name: String, threshold: f64, sample_rate: u32, channels: u16) -> Self {
        Self {
            device_name,
            threshold,
            window_sum_sq: 0.0,
            window_sample_count: 0,
            samples_per_window: (sample_rate as usize * channels as usize) / 20, // 50ms
            recent_rms: std::collections::VecDeque::new(),
            current_rms: 0.0,
            current_peak_level: 0.0,
        }
    }

    /// Process incoming audio samples. Returns true if RMS exceeds threshold
    /// at a 50ms window boundary.
    pub fn process_samples(&mut self, data: &[f32]) -> bool {
        let mut triggered = false;
        for &sample in data {
            self.window_sum_sq += (sample as f64) * (sample as f64);
            self.window_sample_count += 1;

            if self.window_sample_count >= self.samples_per_window {
                let rms = (self.window_sum_sq / self.window_sample_count as f64).sqrt() as f32;
                let now = Instant::now();

                self.recent_rms.push_back((now, rms));
                // Trim entries older than 3 seconds
                while let Some(&(t, _)) = self.recent_rms.front() {
                    if now.duration_since(t) > Duration::from_secs(3) {
                        self.recent_rms.pop_front();
                    } else {
                        break;
                    }
                }

                self.current_rms = rms;
                self.current_peak_level = self.recent_rms.iter()
                    .map(|(_, v)| *v)
                    .fold(0.0f32, f32::max);

                // Reset accumulator
                self.window_sum_sq = 0.0;
                self.window_sample_count = 0;

                if rms > self.threshold as f32 {
                    triggered = true;
                }
            }
        }
        triggered
    }
}

/// Shared state for recording capture
pub struct CaptureState {
    pub is_recording: bool,
    /// True while starting (prevents duplicate triggers, keeps pre-roll active)
    pub is_starting: bool,
    pub session_path: Option<PathBuf>,
    pub start_time: Option<Instant>,
    /// When recording transitioned to active (for idle checker grace period)
    pub recording_started_at: Option<Instant>,
    /// Streaming MIDI writers (one per recording device, keyed by port name)
    pub midi_writers: HashMap<String, MidiStreamWriter>,
    /// Streaming audio writers (one per device, Some when recording)
    pub audio_writers: Vec<Option<AudioStreamWriter>>,
    /// Pre-roll buffer for MIDI events (used when not recording)
    pub midi_preroll: MidiPrerollBuffer,
    /// Pre-roll buffers for audio (one per device, used when not recording)
    pub audio_prerolls: Vec<AudioPrerollBuffer>,
    /// Audio trigger amplitude states (one per trigger device)
    pub audio_trigger_states: Vec<AudioTriggerState>,
    /// Pre-roll duration in seconds
    pub pre_roll_secs: u32,
    /// MIDI timestamp offset in microseconds (equals sync_preroll_duration)
    /// This is added to real-time MIDI timestamps to align with pre-roll content
    pub midi_timestamp_offset_us: u64,
}

impl CaptureState {
    pub fn new(pre_roll_secs: u32) -> Self {
        Self {
            is_recording: false,
            is_starting: false,
            session_path: None,
            start_time: None,
            recording_started_at: None,
            midi_writers: HashMap::new(),
            audio_writers: Vec::new(),
            midi_preroll: MidiPrerollBuffer::new(pre_roll_secs),
            audio_prerolls: Vec::new(),
            audio_trigger_states: Vec::new(),
            pre_roll_secs,
            midi_timestamp_offset_us: 0,
        }
    }
    
    /// Check if we should capture to pre-roll (not recording, or starting)
    pub fn should_use_preroll(&self) -> bool {
        !self.is_recording || self.is_starting
    }
    
    /// Push a MIDI event to the appropriate writer, creating one lazily if needed.
    pub fn push_midi_event(&mut self, device_name: &str, event: TimestampedMidiEvent) {
        if !self.midi_writers.contains_key(device_name) {
            if let Some(session_path) = self.session_path.clone() {
                let safe_name = device_name
                    .replace(' ', "_")
                    .replace('/', "_")
                    .replace('\\', "_")
                    .replace(':', "_");
                let filename = format!("midi_{}.mid", safe_name);
                match MidiStreamWriter::new(&session_path, &filename, device_name) {
                    Ok(writer) => { self.midi_writers.insert(device_name.to_string(), writer); }
                    Err(e) => { println!("[Sacho] Failed to create MIDI writer for {}: {}", device_name, e); }
                }
            }
        }
        if let Some(writer) = self.midi_writers.get_mut(device_name) {
            writer.push_event(&event);
        }
    }
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            is_recording: false,
            is_starting: false,
            session_path: None,
            start_time: None,
            recording_started_at: None,
            midi_writers: HashMap::new(),
            audio_writers: Vec::new(),
            midi_preroll: MidiPrerollBuffer::new(2),
            audio_prerolls: Vec::new(),
            audio_trigger_states: Vec::new(),
            pre_roll_secs: 2,
            midi_timestamp_offset_us: 0,
        }
    }
}

// We can't store cpal::Stream in the struct because it's not Send
// Use a thread-local approach instead
// 
// IMPORTANT: This means start() and stop() MUST be called from the same thread
// for audio streams to be properly cleaned up. Since MidiMonitor is behind an
// Arc<Mutex<>>, the Tauri command handlers should always call from the same thread.
use std::cell::RefCell;
thread_local! {
    static AUDIO_STREAMS: RefCell<Vec<cpal::Stream>> = RefCell::new(Vec::new());
}

/// Manages background MIDI monitoring and automatic recording
pub struct MidiMonitor {
    trigger_connections: Vec<MidiInputConnection<()>>,
    capture_connections: Vec<MidiInputConnection<()>>,
    app_handle: AppHandle,
    last_event_time: Arc<RwLock<Option<Instant>>>,
    is_monitoring: Arc<RwLock<bool>>,
    pub(crate) capture_state: Arc<Mutex<CaptureState>>,
    video_manager: Arc<Mutex<VideoCaptureManager>>,
    /// Handle for the video poller background thread
    video_poller_handle: Option<std::thread::JoinHandle<()>>,
    /// Handle for the idle checker background thread
    idle_checker_handle: Option<std::thread::JoinHandle<()>>,
    /// Handle for the audio level poller background thread
    audio_level_poller_handle: Option<std::thread::JoinHandle<()>>,
    /// Per-thread stop flags for selective pipeline restart
    video_poller_stop: Arc<AtomicBool>,
    idle_checker_stop: Arc<AtomicBool>,
    audio_poller_stop: Arc<AtomicBool>,
}

impl MidiMonitor {
    /// Create a new MIDI monitor
    pub fn new(app_handle: AppHandle) -> Self {
        // Get pre-roll duration from config
        let pre_roll_secs = {
            let config_state = app_handle.state::<RwLock<Config>>();
            let config = config_state.read();
            let limit = if config.encode_during_preroll { MAX_PRE_ROLL_SECS_ENCODED } else { MAX_PRE_ROLL_SECS };
            config.pre_roll_secs.min(limit)
        };
        
        Self {
            trigger_connections: Vec::new(),
            capture_connections: Vec::new(),
            app_handle,
            last_event_time: Arc::new(RwLock::new(None)),
            is_monitoring: Arc::new(RwLock::new(false)),
            capture_state: Arc::new(Mutex::new(CaptureState::default())),
            video_manager: Arc::new(Mutex::new(VideoCaptureManager::new(pre_roll_secs))),
            video_poller_handle: None,
            idle_checker_handle: None,
            audio_level_poller_handle: None,
            video_poller_stop: Arc::new(AtomicBool::new(false)),
            idle_checker_stop: Arc::new(AtomicBool::new(false)),
            audio_poller_stop: Arc::new(AtomicBool::new(false)),
        }
    }
    
    /// Get a reference to the video manager
    pub fn video_manager(&self) -> Arc<Mutex<VideoCaptureManager>> {
        self.video_manager.clone()
    }
    
    /// Start monitoring MIDI ports based on config
    pub fn start(&mut self) -> anyhow::Result<()> {
        // Stop any existing monitoring
        self.stop();

        let config = self.app_handle.state::<RwLock<Config>>();
        let config = config.read().clone();

        // Determine pre-roll limit based on encode_during_preroll setting
        let pre_roll_limit = if config.encode_during_preroll { MAX_PRE_ROLL_SECS_ENCODED } else { MAX_PRE_ROLL_SECS };

        // Update pre-roll duration from config
        {
            let pre_roll = config.pre_roll_secs.min(pre_roll_limit);
            let mut state = self.capture_state.lock();
            state.pre_roll_secs = pre_roll;
            state.midi_preroll.set_duration_with_limit(pre_roll, pre_roll_limit);
        }

        self.start_midi(&config)?;
        let (_audio_count, has_audio_triggers) = self.start_audio(&config)?;
        let video_count = self.start_video_pipeline(&config)?;

        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        let has_any_device = midi_count > 0 || audio_count > 0 || video_count > 0;

        if has_any_device {
            *self.is_monitoring.write() = true;

            // Start idle checker if we have any triggers (MIDI or audio) for auto-stop on idle
            if !self.trigger_connections.is_empty() || has_audio_triggers {
                self.start_idle_checker();
            }

            // Start video polling thread
            if video_count > 0 {
                self.start_video_poller();
            }

            // Start audio level poller for trigger devices
            if has_audio_triggers {
                self.start_audio_level_poller();
            }

            println!("[Sacho] Monitoring active ({} MIDI, {} audio, {} video)",
                midi_count, audio_count, video_count);
        } else {
            println!("[Sacho] No devices configured");
        }

        Ok(())
    }

    /// Start MIDI connections (trigger + record devices)
    fn start_midi(&mut self, config: &Config) -> anyhow::Result<()> {
        println!("[Sacho] Trigger MIDI devices: {:?}", config.trigger_midi_devices);
        println!("[Sacho] Record MIDI devices: {:?}", config.selected_midi_devices);
        println!("[Sacho] Pre-roll: {} seconds", config.pre_roll_secs);

        let midi_in = MidiInput::new("sacho-enum")?;
        let ports = midi_in.ports();

        // Build port info map
        let mut port_info: Vec<(usize, String)> = Vec::new();
        for (idx, port) in ports.iter().enumerate() {
            if let Ok(name) = midi_in.port_name(port) {
                port_info.push((idx, name));
            }
        }

        println!("[Sacho] Available MIDI ports: {:?}", port_info);

        // Connect to trigger devices
        for (port_index, port_name) in &port_info {
            let device_id = format!("midi-{}", port_index);

            if config.trigger_midi_devices.contains(&device_id) {
                println!("[Sacho] Connecting trigger: {} ({})", port_name, device_id);

                let midi_in = MidiInput::new("sacho-trigger")?;
                let ports = midi_in.ports();

                if let Some(port) = ports.get(*port_index) {
                    let app_handle = self.app_handle.clone();
                    let last_event_time = self.last_event_time.clone();
                    let capture_state = self.capture_state.clone();
                    let video_manager = self.video_manager.clone();
                    let port_name_clone = port_name.clone();
                    // Only store MIDI events if this trigger device is also selected for recording
                    let also_record = config.selected_midi_devices.contains(&device_id);

                    match midi_in.connect(
                        port,
                        "sacho-trigger",
                        move |timestamp_us, message, _| {
                            // Only store events if this device is also marked for recording
                            if also_record {
                                let mut state = capture_state.lock();

                                // Use pre-roll if not recording OR if recording is starting (video init)
                                if state.should_use_preroll() {
                                    // Store in pre-roll buffer with driver timestamp for accurate timing
                                    let event = TimestampedMidiEvent {
                                        timestamp_us: 0,
                                        data: message.to_vec(),
                                    };
                                    state.midi_preroll.push(port_name_clone.clone(), event, timestamp_us);
                                } else {
                                    // Recording is active, stream to disk
                                    let rel_time = state.start_time
                                        .map(|st| st.elapsed().as_micros() as u64 + state.midi_timestamp_offset_us)
                                        .unwrap_or(state.midi_timestamp_offset_us);
                                    state.push_midi_event(
                                        &port_name_clone,
                                        TimestampedMidiEvent {
                                            timestamp_us: rel_time,
                                            data: message.to_vec(),
                                        },
                                    );
                                }
                            }

                            // Check for note-on to trigger recording
                            if message.len() >= 3 {
                                let status = message[0] & 0xF0;
                                let velocity = message[2];

                                if status == 0x90 && velocity > 0 {
                                    handle_trigger(&app_handle, &last_event_time, &capture_state, &video_manager);
                                }
                            }
                        },
                        (),
                    ) {
                        Ok(conn) => {
                            self.trigger_connections.push(conn);
                            println!("[Sacho] Connected to trigger: {}", port_name);
                        }
                        Err(e) => {
                            println!("[Sacho] Failed to connect trigger {}: {}", port_name, e);
                        }
                    }
                }
            }
        }

        // Connect to record devices (that aren't already triggers)
        for (port_index, port_name) in &port_info {
            let device_id = format!("midi-{}", port_index);

            // Skip if already connected as trigger
            if config.trigger_midi_devices.contains(&device_id) {
                continue;
            }

            if config.selected_midi_devices.contains(&device_id) {
                println!("[Sacho] Connecting record device: {} ({})", port_name, device_id);

                let midi_in = MidiInput::new("sacho-record")?;
                let ports = midi_in.ports();

                if let Some(port) = ports.get(*port_index) {
                    let capture_state = self.capture_state.clone();
                    let last_event_time = self.last_event_time.clone();
                    let port_name_clone = port_name.clone();

                    match midi_in.connect(
                        port,
                        "sacho-record",
                        move |timestamp_us, message, _| {
                            let mut state = capture_state.lock();

                            // Update last event time for idle detection (even during pre-roll)
                            if message.len() >= 3 {
                                let status = message[0] & 0xF0;
                                if status == 0x90 || status == 0x80 {
                                    *last_event_time.write() = Some(Instant::now());
                                }
                            }

                            // Use pre-roll if not recording OR if recording is starting (video init)
                            if state.should_use_preroll() {
                                // Store in pre-roll buffer with driver timestamp for accurate timing
                                state.midi_preroll.push(
                                    port_name_clone.clone(),
                                    TimestampedMidiEvent {
                                        timestamp_us: 0,
                                        data: message.to_vec(),
                                    },
                                    timestamp_us,
                                );
                            } else {
                                // Recording is active, stream to disk
                                let rel_time = state.start_time
                                    .map(|st| st.elapsed().as_micros() as u64 + state.midi_timestamp_offset_us)
                                    .unwrap_or(state.midi_timestamp_offset_us);
                                state.push_midi_event(
                                    &port_name_clone,
                                    TimestampedMidiEvent {
                                        timestamp_us: rel_time,
                                        data: message.to_vec(),
                                    },
                                );
                            }
                        },
                        (),
                    ) {
                        Ok(conn) => {
                            self.capture_connections.push(conn);
                            println!("[Sacho] Connected to record device: {}", port_name);
                        }
                        Err(e) => {
                            println!("[Sacho] Failed to connect record {}: {}", port_name, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Start audio capture streams. Returns (audio_count, has_audio_triggers).
    fn start_audio(&mut self, config: &Config) -> anyhow::Result<(usize, bool)> {
        println!("[Sacho] Audio record devices: {:?}", config.selected_audio_devices);
        println!("[Sacho] Audio trigger devices: {:?}", config.trigger_audio_devices);

        let pre_roll_limit = if config.encode_during_preroll { MAX_PRE_ROLL_SECS_ENCODED } else { MAX_PRE_ROLL_SECS };
        let host = cpal::default_host();
        let pre_roll_secs = config.pre_roll_secs.min(pre_roll_limit);

        // Build union of audio devices that need a cpal stream
        let mut audio_device_roles: HashMap<String, (bool, bool)> = HashMap::new(); // (is_record, is_trigger)
        for name in &config.selected_audio_devices {
            audio_device_roles.entry(name.clone()).or_insert((false, false)).0 = true;
        }
        for name in &config.trigger_audio_devices {
            audio_device_roles.entry(name.clone()).or_insert((false, false)).1 = true;
        }
        let audio_trigger_thresholds = config.audio_trigger_thresholds.clone();
        let has_audio_triggers = !config.trigger_audio_devices.is_empty();

        if let Ok(audio_devices) = host.input_devices() {
            for device in audio_devices {
                if let Ok(device_name) = device.name() {
                    // Check if this device needs a stream (record, trigger, or both)
                    let Some(&(is_record, is_trigger)) = audio_device_roles.get(&device_name) else {
                        continue;
                    };

                    let role_str = match (is_record, is_trigger) {
                        (true, true) => "record+trigger",
                        (true, false) => "record",
                        (false, true) => "trigger-only",
                        (false, false) => continue,
                    };
                    println!("[Sacho] Setting up audio {}: {}", role_str, device_name);

                    if let Ok(supported_config) = device.default_input_config() {
                        let sample_rate = supported_config.sample_rate().0;
                        let channels = supported_config.channels();

                        // Create pre-roll buffer and writer slot only for record devices
                        let buffer_index = if is_record {
                            let mut state = self.capture_state.lock();

                            state.audio_prerolls.push(AudioPrerollBuffer::with_limit(
                                device_name.clone(),
                                sample_rate,
                                channels,
                                pre_roll_secs,
                                pre_roll_limit,
                            ));
                            state.audio_writers.push(None);

                            Some(state.audio_prerolls.len() - 1)
                        } else {
                            None
                        };

                        // Create trigger state for trigger devices
                        let trigger_index = if is_trigger {
                            let threshold = audio_trigger_thresholds
                                .get(&device_name)
                                .copied()
                                .unwrap_or(0.1); // Default threshold
                            let mut state = self.capture_state.lock();
                            state.audio_trigger_states.push(AudioTriggerState::new(
                                device_name.clone(),
                                threshold,
                                sample_rate,
                                channels,
                            ));
                            Some(state.audio_trigger_states.len() - 1)
                        } else {
                            None
                        };

                        let capture_state = self.capture_state.clone();
                        let app_handle = self.app_handle.clone();
                        let last_event_time = self.last_event_time.clone();
                        let video_manager = self.video_manager.clone();

                        match device.build_input_stream(
                            &supported_config.into(),
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                let should_trigger = {
                                    let mut state = capture_state.lock();

                                    // Route audio to preroll/writer if this is a record device
                                    if let Some(idx) = buffer_index {
                                        if state.should_use_preroll() {
                                            if let Some(preroll) = state.audio_prerolls.get_mut(idx) {
                                                preroll.push_samples(data);
                                            }
                                        } else if let Some(Some(writer)) = state.audio_writers.get_mut(idx) {
                                            writer.push_samples(data);
                                        }
                                    }

                                    // Compute amplitude if this is a trigger device
                                    if let Some(idx) = trigger_index {
                                        state.audio_trigger_states[idx].process_samples(data)
                                    } else {
                                        false
                                    }
                                }; // lock released

                                if should_trigger {
                                    handle_trigger(&app_handle, &last_event_time, &capture_state, &video_manager);
                                }
                            },
                            |err| {
                                println!("[Sacho] Audio error: {}", err);
                            },
                            None,
                        ) {
                            Ok(stream) => {
                                if stream.play().is_ok() {
                                    AUDIO_STREAMS.with(|streams| {
                                        streams.borrow_mut().push(stream);
                                    });
                                    println!("[Sacho] Audio {} ready: {} ({}Hz, {}ch, {}s pre-roll)",
                                        role_str, device_name, sample_rate, channels, pre_roll_secs);
                                }
                            }
                            Err(e) => {
                                println!("[Sacho] Failed to create audio stream for {}: {}", device_name, e);
                            }
                        }
                    }
                }
            }
        }

        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        Ok((audio_count, has_audio_triggers))
    }

    /// Start video capture pipelines. Returns the number of active video pipelines.
    fn start_video_pipeline(&mut self, config: &Config) -> anyhow::Result<usize> {
        let pre_roll_limit = if config.encode_during_preroll { MAX_PRE_ROLL_SECS_ENCODED } else { MAX_PRE_ROLL_SECS };
        let encode_during_preroll = config.encode_during_preroll;
        let selected_video = config.selected_video_devices.clone();
        let pre_roll = config.pre_roll_secs.min(pre_roll_limit);

        // Look up per-device config and name for each selected video device
        let device_manager = self.app_handle.state::<RwLock<DeviceManager>>();
        let devices = device_manager.read();

        let device_configs = &config.video_device_configs;

        let video_with_info: Vec<(String, String, crate::config::VideoDeviceConfig)> = selected_video
            .iter()
            .filter_map(|device_id| {
                // Find the device
                let device = devices.video_devices.iter().find(|d| &d.id == device_id)?;

                // Use user-saved config if available, otherwise compute smart defaults
                let dev_config = if let Some(cfg) = device_configs.get(device_id) {
                    // Verify the saved codec is still supported
                    if device.supported_codecs.contains(&cfg.source_codec) {
                        println!("[Sacho] Video device {}: using saved config ({:?} {}x{} @ {:.2}fps)",
                            device_id, cfg.source_codec, cfg.source_width, cfg.source_height, cfg.source_fps);
                        cfg.clone()
                    } else {
                        // Saved codec no longer available, fall back to defaults
                        let default = device.default_config()?;
                        println!("[Sacho] Video device {}: saved codec {:?} unavailable, falling back to {:?} {}x{} @ {:.2}fps",
                            device_id, cfg.source_codec, default.source_codec, default.source_width, default.source_height, default.source_fps);
                        default
                    }
                } else {
                    // No saved config - compute smart defaults
                    let default = device.default_config()?;
                    println!("[Sacho] Video device {}: no config saved, defaulting to {:?} {}x{} @ {:.2}fps",
                        device_id, default.source_codec, default.source_width, default.source_height, default.source_fps);
                    default
                };

                Some((device_id.clone(), device.name.clone(), dev_config))
            })
            .collect();

        drop(devices); // Release device manager lock

        let mut video_mgr = self.video_manager.lock();
        video_mgr.set_preroll_duration(pre_roll);
        video_mgr.set_encode_during_preroll(encode_during_preroll);

        if !video_with_info.is_empty() {
            if let Err(e) = video_mgr.start(&video_with_info) {
                println!("[Sacho] Failed to start video capture: {}", e);
            }
        }
        Ok(video_mgr.pipeline_count())
    }
    
    /// Start background thread to poll video frames
    fn start_video_poller(&mut self) {
        self.video_poller_stop.store(false, Ordering::SeqCst);
        let stop_flag = self.video_poller_stop.clone();
        let video_manager = self.video_manager.clone();
        let app_handle = self.app_handle.clone();

        let handle = std::thread::Builder::new()
            .name("sacho-video-poller".into())
            .spawn(move || {
                while !stop_flag.load(Ordering::SeqCst) {
                    {
                        let mut mgr = video_manager.lock();
                        mgr.poll();

                        // Check for FPS mismatch warnings
                        let warnings = mgr.collect_fps_warnings();
                        for warning in warnings {
                            let _ = app_handle.emit("video-fps-warning", warning);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10)); // Poll at ~100Hz
                }
            })
            .expect("Failed to spawn video poller thread");

        self.video_poller_handle = Some(handle);
    }

    /// Start background thread to emit audio trigger levels to the frontend
    fn start_audio_level_poller(&mut self) {
        self.audio_poller_stop.store(false, Ordering::SeqCst);
        let stop_flag = self.audio_poller_stop.clone();
        let capture_state = self.capture_state.clone();
        let app_handle = self.app_handle.clone();

        let handle = std::thread::Builder::new()
            .name("sacho-audio-levels".into())
            .spawn(move || {
                while !stop_flag.load(Ordering::SeqCst) {
                    {
                        let state = capture_state.lock();
                        if !state.audio_trigger_states.is_empty() {
                            let levels: Vec<serde_json::Value> = state.audio_trigger_states.iter()
                                .map(|ts| serde_json::json!({
                                    "device_id": ts.device_name,
                                    "current_rms": ts.current_rms,
                                    "peak_level": ts.current_peak_level,
                                }))
                                .collect();
                            let _ = app_handle.emit("audio-trigger-levels", levels);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .expect("Failed to spawn audio level poller thread");

        self.audio_level_poller_handle = Some(handle);
    }

    /// Stop monitoring (all pipelines)
    pub fn stop(&mut self) {
        self.stop_idle_checker();
        self.stop_midi();
        self.stop_audio();
        self.stop_video();
        *self.is_monitoring.write() = false;
    }

    /// Stop only the MIDI connections and clear MIDI capture state
    fn stop_midi(&mut self) {
        self.trigger_connections.clear();
        self.capture_connections.clear();

        let mut state = self.capture_state.lock();
        state.midi_writers.clear();
        state.midi_preroll.clear();
    }

    /// Stop only the audio streams and clear audio capture state
    fn stop_audio(&mut self) {
        // Stop the audio level poller thread
        self.audio_poller_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.audio_level_poller_handle.take() {
            let _ = handle.join();
        }

        // Clear audio streams (stops cpal callbacks)
        AUDIO_STREAMS.with(|streams| {
            streams.borrow_mut().clear();
        });

        // Clear audio capture state
        let mut state = self.capture_state.lock();
        state.audio_writers.clear();
        state.audio_prerolls.clear();
        state.audio_trigger_states.clear();
    }

    /// Stop only the video pipeline
    fn stop_video(&mut self) {
        // Stop the video poller thread
        self.video_poller_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.video_poller_handle.take() {
            let _ = handle.join();
        }

        self.video_manager.lock().stop();
    }

    /// Stop only the idle checker thread
    fn stop_idle_checker(&mut self) {
        self.idle_checker_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.idle_checker_handle.take() {
            let _ = handle.join();
        }
    }

    /// Restart only MIDI connections without touching audio or video
    pub fn restart_midi(&mut self) -> anyhow::Result<()> {
        self.stop_idle_checker();
        self.stop_midi();

        let config = self.app_handle.state::<RwLock<Config>>();
        let config = config.read().clone();

        self.start_midi(&config)?;

        // Restart idle checker if we have any triggers (MIDI or audio)
        let has_audio_triggers = !self.capture_state.lock().audio_trigger_states.is_empty();
        if !self.trigger_connections.is_empty() || has_audio_triggers {
            self.start_idle_checker();
        }

        // Ensure is_monitoring is set if we have any active device
        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        let video_count = self.video_manager.lock().pipeline_count();
        if midi_count > 0 || audio_count > 0 || video_count > 0 {
            *self.is_monitoring.write() = true;
        }

        println!("[Sacho] MIDI pipeline restarted ({} connections)", midi_count);
        Ok(())
    }

    /// Restart only audio streams without touching MIDI or video
    pub fn restart_audio(&mut self) -> anyhow::Result<()> {
        self.stop_idle_checker();
        self.stop_audio();

        let config = self.app_handle.state::<RwLock<Config>>();
        let config = config.read().clone();

        let (_audio_count, has_audio_triggers) = self.start_audio(&config)?;

        // Restart idle checker if we have any triggers (MIDI or audio)
        if !self.trigger_connections.is_empty() || has_audio_triggers {
            self.start_idle_checker();
        }

        // Restart audio level poller if we have audio triggers
        if has_audio_triggers {
            self.start_audio_level_poller();
        }

        // Ensure is_monitoring is set if we have any active device
        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        let video_count = self.video_manager.lock().pipeline_count();
        if midi_count > 0 || audio_count > 0 || video_count > 0 {
            *self.is_monitoring.write() = true;
        }

        println!("[Sacho] Audio pipeline restarted ({} streams)", audio_count);
        Ok(())
    }

    /// Restart only video pipeline without touching MIDI or audio
    pub fn restart_video(&mut self) -> anyhow::Result<()> {
        self.stop_video();

        let config = self.app_handle.state::<RwLock<Config>>();
        let config = config.read().clone();

        let video_count = self.start_video_pipeline(&config)?;

        // Restart video poller if pipelines are active
        if video_count > 0 {
            self.start_video_poller();
        }

        // Ensure is_monitoring is set if we have any active device
        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        if midi_count > 0 || audio_count > 0 || video_count > 0 {
            *self.is_monitoring.write() = true;
        }

        println!("[Sacho] Video pipeline restarted ({} pipelines)", video_count);
        Ok(())
    }
    
    /// Manually start recording (same as MIDI trigger but without waiting for MIDI)
    pub fn manual_start_recording(&self) -> Result<(), String> {
        // Check that at least one device is active
        let midi_count = self.trigger_connections.len() + self.capture_connections.len();
        let audio_count = AUDIO_STREAMS.with(|streams| streams.borrow().len());
        let video_count = self.video_manager.lock().pipeline_count();
        
        if midi_count == 0 && audio_count == 0 && video_count == 0 {
            return Err("No devices selected. Configure at least one MIDI, audio, or video device before recording.".to_string());
        }
        
        // Atomically check and set is_starting to prevent race conditions
        {
            let mut state = self.capture_state.lock();
            if state.is_recording || state.is_starting {
                return Err("Already recording".to_string());
            }
            state.is_starting = true;
        }
        
        println!("[Sacho] Manual recording start requested");
        
        // Clear any stale idle timer so the idle checker doesn't immediately stop us.
        // Without this, a stale last_event_time from a previous MIDI event
        // can cause the idle checker to see "idle for > N seconds" and stop
        // the recording within 1 second of starting.
        // Setting to None means manual recordings run until explicitly stopped
        // (idle timeout only applies when MIDI events set last_event_time).
        *self.last_event_time.write() = None;
        
        // Start recording (synchronous for manual start so caller knows when it's ready)
        start_recording(&self.app_handle, &self.capture_state, &self.video_manager);
        
        Ok(())
    }
    
    /// Manually stop recording
    pub fn manual_stop_recording(&self) -> Result<(), String> {
        let is_recording = {
            let state = self.capture_state.lock();
            state.is_recording
        };
        
        if !is_recording {
            return Err("Not currently recording".to_string());
        }
        
        println!("[Sacho] Manual recording stop requested");
        stop_recording(&self.app_handle, &self.capture_state, &self.video_manager);
        
        Ok(())
    }
    
    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.capture_state.lock().is_recording
    }
    
    /// Start idle timeout checker thread
    fn start_idle_checker(&mut self) {
        self.idle_checker_stop.store(false, Ordering::SeqCst);
        let app_handle = self.app_handle.clone();
        let last_event_time = self.last_event_time.clone();
        let stop_flag = self.idle_checker_stop.clone();
        let capture_state = self.capture_state.clone();
        let video_manager = self.video_manager.clone();

        let handle = std::thread::Builder::new()
            .name("sacho-idle-checker".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));

                    if stop_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    
                    let config = app_handle.state::<RwLock<Config>>();
                    let idle_timeout = config.read().idle_timeout_secs;
                    
                    let (is_recording, recording_started_at) = {
                        let state = capture_state.lock();
                        (state.is_recording, state.recording_started_at)
                    };

                    if is_recording {
                        // Skip idle check if recording just started (grace period)
                        // This prevents a stale last_event_time from immediately stopping
                        // a recording that took a while to initialize (e.g., slow camera)
                        if let Some(started_at) = recording_started_at {
                            if started_at.elapsed() < Duration::from_secs(idle_timeout as u64) {
                                continue;
                            }
                        }

                        if let Some(last_time) = *last_event_time.read() {
                            if last_time.elapsed() >= Duration::from_secs(idle_timeout as u64) {
                                println!("[Sacho] Idle timeout ({} sec), stopping recording", idle_timeout);
                                stop_recording(&app_handle, &capture_state, &video_manager);
                            }
                        }
                    }
                }
            })
            .expect("Failed to spawn idle checker thread");
        
        self.idle_checker_handle = Some(handle);
    }
}

impl Drop for MidiMonitor {
    fn drop(&mut self) {
        // Ensure monitoring is stopped and resources are cleaned up
        self.stop();
    }
}

/// Handle trigger event (MIDI note-on or audio threshold exceeded)
fn handle_trigger(
    app_handle: &AppHandle, 
    last_event_time: &Arc<RwLock<Option<Instant>>>,
    capture_state: &Arc<Mutex<CaptureState>>,
    video_manager: &Arc<Mutex<VideoCaptureManager>>,
) {
    // Update last event time
    *last_event_time.write() = Some(Instant::now());
    
    // Check if the global recording state allows starting
    // (e.g., we're not in Initializing mode from a device config change)
    {
        let recording_state = app_handle.state::<RwLock<crate::recording::RecordingState>>();
        let state = recording_state.read();
        if state.status == crate::recording::RecordingStatus::Initializing {
            // Silently ignore MIDI triggers during device reinitialization
            return;
        }
    }
    
    // Atomically check and set is_starting to prevent race conditions
    let should_start = {
        let mut state = capture_state.lock();
        if state.is_recording || state.is_starting {
            false
        } else {
            state.is_starting = true;
            true
        }
    };
    
    if should_start {
        println!("[Sacho] Trigger -> starting recording (async)");
        
        // Spawn recording start on a separate thread so MIDI callback isn't blocked
        // This allows pre-roll to continue capturing during video initialization
        let app_handle = app_handle.clone();
        let capture_state = capture_state.clone();
        let video_manager = video_manager.clone();
        std::thread::spawn(move || {
            start_recording(&app_handle, &capture_state, &video_manager);
        });
    }
}

/// Start recording
fn start_recording(
    app_handle: &AppHandle, 
    capture_state: &Arc<Mutex<CaptureState>>,
    video_manager: &Arc<Mutex<VideoCaptureManager>>,
) {
    let config = app_handle.state::<RwLock<Config>>();
    let config_read = config.read().clone();
    
    let timestamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let session_path = config_read.storage_path.join(&timestamp);
    
    if let Err(e) = std::fs::create_dir_all(&session_path) {
        println!("[Sacho] Failed to create session folder: {}", e);
        // Reset is_starting flag so future recording attempts can work
        capture_state.lock().is_starting = false;
        return;
    }
    
    // Capture the instant BEFORE video starts - this is our sync reference point
    // The video pre-roll duration is relative to this instant
    let video_start_instant = Instant::now();
    
    // Start video recording (this captures pre-roll and begins file writing)
    let video_preroll_duration = {
        let mut mgr = video_manager.lock();
        match mgr.start_recording(&session_path) {
            Ok(duration) => {
                println!("[Sacho] Video recording started with {:?} pre-roll", duration);
                Some(duration)
            }
            Err(e) => {
                println!("[Sacho] Failed to start video recording: {}", e);
                None
            }
        }
    };
    
    // Capture a single trigger instant for consistent timing across all streams
    let trigger_instant = Instant::now();
    
    // Initialize capture state and drain pre-roll buffers
    {
        let mut state = capture_state.lock();
        
        // Calculate the actual audio pre-roll duration from the first audio buffer
        // This tells us how much audio we captured before the trigger
        let configured_preroll = Duration::from_secs(state.pre_roll_secs as u64);
        let audio_preroll_duration = state.audio_prerolls.first().map(|_preroll| {
            configured_preroll
        });
        
        // SYNC FIX: Calculate the correct audio pre-roll to align with video
        // 
        // video_preroll_duration = time from first video frame capture to when video.rs STARTED
        // (measured using first_frame.wall_time.elapsed() at the moment video processing began)
        // 
        // delay_since_video_start = time elapsed from when video started to NOW
        // This includes the time video took to process AND any time to reach this point
        //
        // Total audio pre-roll = video_preroll + delay_since_video_start
        // This ensures the first video frame and first audio sample represent the same moment
        let delay_since_video_start = video_start_instant.elapsed();
        
        let sync_preroll_duration = match (audio_preroll_duration, video_preroll_duration) {
            (Some(audio_dur), Some(video_dur)) => {
                // Add the delay since video STARTED to get the correct audio pre-roll
                // This accounts for the ~340ms that video processing takes
                let adjusted_video_dur = video_dur + delay_since_video_start;
                // Use the minimum to avoid requesting more audio than we have
                let sync_dur = audio_dur.min(adjusted_video_dur);
                
                println!("[Sacho] SYNC: video_preroll={:?}, delay={:?}, adjusted={:?}, audio={:?}, using={:?}", 
                    video_dur, delay_since_video_start, adjusted_video_dur, audio_dur, sync_dur);
                Some(sync_dur)
            }
            (Some(audio_dur), None) => Some(audio_dur), // No video, use audio
            (None, Some(video_dur)) => Some(video_dur + delay_since_video_start), // No audio, use adjusted video
            (None, None) => None,
        };
        
        // Drain pre-roll MIDI buffer with sync duration
        // This ensures MIDI timestamps align with the synchronized pre-roll start
        let preroll_events = state.midi_preroll.drain_with_audio_sync(sync_preroll_duration);
        let midi_preroll_count = preroll_events.len();
        
        // Create MIDI writers and flush pre-roll events through them
        state.midi_writers.clear();
        for (device_name, _event) in &preroll_events {
            if !state.midi_writers.contains_key(device_name.as_str()) {
                let safe_name = device_name
                    .replace(' ', "_")
                    .replace('/', "_")
                    .replace('\\', "_")
                    .replace(':', "_");
                let filename = format!("midi_{}.mid", safe_name);
                match MidiStreamWriter::new(&session_path, &filename, device_name) {
                    Ok(writer) => { state.midi_writers.insert(device_name.clone(), writer); }
                    Err(e) => { println!("[Sacho] Failed to create MIDI writer for {}: {}", device_name, e); }
                }
            }
        }
        for (device_name, event) in preroll_events {
            if let Some(writer) = state.midi_writers.get_mut(&device_name) {
                writer.push_event(&event);
            }
        }
        
        // Create streaming audio writers and drain pre-roll into them
        // Read audio format config
        let audio_format = config_read.audio_format.clone();
        let (bit_depth, sample_rate_setting) = match audio_format {
            crate::config::AudioFormat::Wav => (config_read.wav_bit_depth.clone(), config_read.wav_sample_rate.clone()),
            crate::config::AudioFormat::Flac => (config_read.flac_bit_depth.clone(), config_read.flac_sample_rate.clone()),
        };
        
        let extension = match audio_format {
            crate::config::AudioFormat::Wav => "wav",
            crate::config::AudioFormat::Flac => "flac",
        };
        
        let num_audio_devices = state.audio_prerolls.len();
        let mut audio_preroll_samples = 0;
        
        for i in 0..num_audio_devices {
            // Drain pre-roll samples
            let preroll_samples = if let Some(sync_dur) = sync_preroll_duration {
                state.audio_prerolls[i].drain_duration(sync_dur)
            } else {
                state.audio_prerolls[i].drain()
            };
            audio_preroll_samples += preroll_samples.len();
            
            // Build filename
            let filename = if num_audio_devices == 1 {
                format!("recording.{}", extension)
            } else {
                format!("recording_{}.{}", i + 1, extension)
            };
            
            // Create streaming writer using device info from preroll buffer
            let dev_name = state.audio_prerolls[i].device_name().to_string();
            let native_rate = state.audio_prerolls[i].sample_rate();
            let channels = state.audio_prerolls[i].channels();
            
            match AudioStreamWriter::new(
                &session_path, &filename, &dev_name, channels, native_rate,
                &audio_format, &bit_depth, &sample_rate_setting,
            ) {
                Ok(mut writer) => {
                    // Push drained pre-roll samples into the streaming writer
                    if !preroll_samples.is_empty() {
                        writer.push_samples(&preroll_samples);
                    }
                    state.audio_writers[i] = Some(writer);
                }
                Err(e) => {
                    println!("[Sacho] Failed to create audio writer for {}: {}", dev_name, e);
                }
            }
        }
        
        // Set the session path and start time to the same trigger instant
        state.session_path = Some(session_path.clone());
        state.start_time = Some(trigger_instant);
        
        // Set MIDI timestamp offset to sync_preroll_duration
        // Real-time MIDI events need this offset added to align with pre-roll content
        state.midi_timestamp_offset_us = sync_preroll_duration
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        
        // Switch from "starting" to "recording" - now new events go directly to midi_events
        state.is_starting = false;
        state.is_recording = true;
        state.recording_started_at = Some(Instant::now());
        
        println!("[Sacho] Recording started with {} pre-roll MIDI events, {} pre-roll audio samples (sync pre-roll: {:?})", 
            midi_preroll_count, audio_preroll_samples, sync_preroll_duration);
    }
    
    // Update recording state
    let active_devices = {
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.status = crate::recording::RecordingStatus::Recording;
        state.started_at = Some(chrono::Utc::now());
        state.current_session_path = Some(session_path.clone());
        state.active_midi_devices = config_read.selected_midi_devices.clone();
        state.active_audio_devices = config_read.selected_audio_devices.clone();
        state.active_video_devices = config_read.selected_video_devices.clone();
        
        // Collect device names for notification
        let mut devices = state.active_midi_devices.clone();
        devices.extend(state.active_audio_devices.clone());
        devices.extend(state.active_video_devices.clone());
        devices
    };
    
    // Write initial metadata so the session is discoverable even if the app crashes
    let session_id = session_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    
    let initial_metadata = SessionMetadata {
        id: session_id,
        timestamp: chrono::Utc::now(),
        duration_secs: 0.0,
        path: session_path.clone(),
        audio_files: Vec::new(),
        midi_files: Vec::new(),
        video_files: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        is_favorite: false,
        midi_features: None,
        similarity_coords: None,
        cluster_id: None,
    };
    
    if let Err(e) = crate::session::save_metadata(&initial_metadata) {
        println!("[Sacho] Failed to write initial metadata: {}", e);
    }
    
    // Send desktop notification
    if config_read.notify_recording_start {
        notifications::notify_recording_started(app_handle, &active_devices);
    }
    
    let _ = app_handle.emit("recording-started", session_path.to_string_lossy().to_string());
    println!("[Sacho] Recording started: {:?}", session_path);
}

/// Stop recording and save files
fn stop_recording(
    app_handle: &AppHandle, 
    capture_state: &Arc<Mutex<CaptureState>>,
    video_manager: &Arc<Mutex<VideoCaptureManager>>,
) {
    // First, extract what we need from capture_state
    let (session_path, midi_writers, audio_writers, duration_secs) = {
        let mut state = capture_state.lock();
        if !state.is_recording {
            return;
        }
        
        let duration = state.start_time
            .map(|st| st.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        
        let path = state.session_path.take();
        
        // Take MIDI writers out of the state
        let midi_ws: HashMap<String, MidiStreamWriter> = std::mem::take(&mut state.midi_writers);
        
        // Take audio writers out of the state (replace with None)
        let audio_ws: Vec<Option<AudioStreamWriter>> = state.audio_writers.iter_mut()
            .map(|w| w.take())
            .collect();
        
        state.is_recording = false;
        state.is_starting = false;
        state.start_time = None;
        state.recording_started_at = None;
        state.midi_timestamp_offset_us = 0;
        
        (path, midi_ws, audio_ws, duration)
    };
    
    let Some(session_path) = session_path else {
        // Even if no session path, update recording state to idle
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.status = crate::recording::RecordingStatus::Idle;
        state.started_at = None;
        state.current_session_path = None;
        state.elapsed_seconds = 0;
        state.active_midi_devices.clear();
        state.active_audio_devices.clear();
        state.active_video_devices.clear();
        return;
    };
    
    // Update recording state to idle immediately (before slow file operations)
    {
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.status = crate::recording::RecordingStatus::Idle;
        state.started_at = None;
        state.current_session_path = Some(session_path.clone());
        state.elapsed_seconds = 0;
        // Keep device info for now, will be cleared after save
    }
    
    // Stop video recording and get video files
    let mut video_files = {
        let mut mgr = video_manager.lock();
        mgr.stop_recording()
    };
    
    let midi_writer_count = midi_writers.len();
    let audio_writer_count = audio_writers.iter().filter(|w| w.is_some()).count();
    println!("[Sacho] Stopping recording, {} MIDI streams, {} audio streams, {} video files", 
        midi_writer_count, audio_writer_count, video_files.len());
    
    // Finalize MIDI writers (patch headers and close files)
    let mut midi_files = Vec::new();
    for (_, writer) in midi_writers.into_iter() {
        match writer.finish() {
            Ok(info) => midi_files.push(info),
            Err(e) => println!("[Sacho] Failed to finalize MIDI: {}", e),
        }
    }
    
    // Calculate max video duration for potential audio padding
    let video_max_duration = video_files.iter()
        .map(|f| f.duration_secs)
        .fold(0.0f64, |a, b| a.max(b));
    
    let target_duration = duration_secs.max(video_max_duration);
    
    // Finalize audio writers: pad if needed, then finish (EOS + flush to disk)
    let mut audio_files = Vec::new();
    for writer_opt in audio_writers.into_iter() {
        if let Some(mut writer) = writer_opt {
            // Pad with silence if video is longer
            let writer_duration = writer.frames_pushed as f64 / writer.native_rate as f64;
            if writer_duration < target_duration - 0.1 {
                let padding_secs = target_duration - writer_duration;
                writer.push_silence(padding_secs);
                println!("[Sacho] Padded audio {} with {:.2}s of silence", writer.filename, padding_secs);
            }
            
            match writer.finish() {
                Ok(info) => audio_files.push(info),
                Err(e) => println!("[Sacho] Failed to finalize audio: {}", e),
            }
        }
    }
    
    // Update overall duration to include audio
    let audio_max_duration = audio_files.iter()
        .map(|f| f.duration_secs)
        .fold(0.0f64, |a, b| a.max(b));
    let duration_secs = target_duration.max(audio_max_duration);
    
    // Combine audio+video into a single MKV if configured (exactly 1 of each)
    {
        let config = app_handle.state::<RwLock<Config>>();
        let config_read = config.read();
        if config_read.combine_audio_video
            && video_files.len() == 1
            && audio_files.len() == 1
        {
            let video_path = session_path.join(&video_files[0].filename);
            let audio_path = session_path.join(&audio_files[0].filename);
            match combine_audio_video_mkv(&video_path, &audio_path, &config_read.audio_format) {
                Ok(new_size) => {
                    // Delete the separate audio file
                    let _ = std::fs::remove_file(&audio_path);
                    // Update metadata: video now has audio, remove separate audio entry
                    video_files[0].has_audio = true;
                    video_files[0].size_bytes = new_size;
                    audio_files.clear();
                    println!("[Sacho] Combined audio+video into single MKV");
                }
                Err(e) => {
                    println!("[Sacho] Failed to combine audio+video: {}. Keeping separate files.", e);
                    // Graceful fallback: separate files are still valid
                }
            }
        }
    }
    
    // Clear remaining recording state (session path and devices)
    {
        let recording_state = app_handle.state::<RwLock<RecordingState>>();
        let mut state = recording_state.write();
        state.current_session_path = None;
        state.active_midi_devices.clear();
        state.active_audio_devices.clear();
        state.active_video_devices.clear();
    }
    
    // Create and save metadata
    // Use folder name as session ID (for consistency with similarity calculation)
    let session_id = session_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    
    let metadata = SessionMetadata {
        id: session_id,
        timestamp: chrono::Utc::now(),
        duration_secs,
        path: session_path.clone(),
        audio_files,
        midi_files,
        video_files,
        tags: Vec::new(),
        notes: String::new(),
        is_favorite: false,
        midi_features: None,
        similarity_coords: None,
        cluster_id: None,
    };
    
    if let Err(e) = crate::session::save_metadata(&metadata) {
        println!("[Sacho] Failed to save metadata: {}", e);
    }
    
    let db = app_handle.state::<SessionDatabase>();
    if let Err(e) = db.upsert_session(&metadata) {
        println!("[Sacho] Failed to index session: {}", e);
    }
    
    // Send desktop notification
    let config = app_handle.state::<RwLock<Config>>();
    if config.read().notify_recording_stop {
        let folder_name = session_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("session");
        notifications::notify_recording_stopped(app_handle, duration_secs, folder_name);
    }
    
    let _ = app_handle.emit("recording-stopped", serde_json::to_string(&metadata).unwrap_or_default());
    println!("[Sacho] Recording stopped, duration: {} sec", duration_secs);
}



