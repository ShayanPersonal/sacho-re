use std::path::Path;

// ── WAV validation ───────────────────────────────────────────────────

#[derive(Debug)]
pub struct WavValidation {
    pub channels: u16,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub duration_secs: f64,
    pub rms: f64,
}

/// Parse and validate a WAV file by reading RIFF headers directly.
pub fn validate_wav(path: &Path) -> Result<WavValidation, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("Failed to read WAV file: {}", e))?;

    if data.len() < 44 {
        return Err("WAV file too small (< 44 bytes)".into());
    }

    // RIFF header
    if &data[0..4] != b"RIFF" {
        return Err("Missing RIFF header".into());
    }
    if &data[8..12] != b"WAVE" {
        return Err("Missing WAVE format".into());
    }

    // Walk chunks to find fmt and data
    let mut offset = 12;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut bit_depth: u16 = 0;
    let mut block_align: u16 = 0;
    let mut data_start: usize = 0;
    let mut data_size: u32 = 0;
    let mut found_fmt = false;
    let mut found_data = false;

    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]);

        if chunk_id == b"fmt " {
            if chunk_size < 16 || offset + 8 + 16 > data.len() {
                return Err("fmt chunk too small".into());
            }
            let base = offset + 8;
            channels = u16::from_le_bytes([data[base + 2], data[base + 3]]);
            sample_rate = u32::from_le_bytes([
                data[base + 4], data[base + 5], data[base + 6], data[base + 7],
            ]);
            block_align = u16::from_le_bytes([data[base + 12], data[base + 13]]);
            bit_depth = u16::from_le_bytes([data[base + 14], data[base + 15]]);
            found_fmt = true;
        } else if chunk_id == b"data" {
            data_start = offset + 8;
            data_size = chunk_size;
            found_data = true;
        }

        // Move to next chunk (chunks are word-aligned)
        let advance = 8 + chunk_size as usize;
        let advance = if advance % 2 != 0 { advance + 1 } else { advance };
        offset += advance;

        if found_fmt && found_data {
            break;
        }
    }

    if !found_fmt {
        return Err("No fmt chunk found".into());
    }
    if !found_data {
        return Err("No data chunk found".into());
    }
    if channels == 0 || sample_rate == 0 || block_align == 0 {
        return Err(format!(
            "Invalid WAV params: ch={}, sr={}, ba={}",
            channels, sample_rate, block_align
        ));
    }

    let total_frames = data_size as u64 / block_align as u64;
    let duration_secs = total_frames as f64 / sample_rate as f64;

    // Compute RMS of first 1000 samples to verify non-silence
    let rms = compute_wav_rms(&data, data_start, data_size as usize, bit_depth, channels);

    Ok(WavValidation {
        channels,
        sample_rate,
        bit_depth,
        duration_secs,
        rms,
    })
}

fn compute_wav_rms(data: &[u8], data_start: usize, data_size: usize, bit_depth: u16, channels: u16) -> f64 {
    let num_samples = 1000.min(data_size / (bit_depth as usize / 8));
    if num_samples == 0 {
        return 0.0;
    }

    let mut sum_sq: f64 = 0.0;
    let bytes_per_sample = bit_depth as usize / 8;

    for i in 0..num_samples {
        let offset = data_start + i * bytes_per_sample;
        if offset + bytes_per_sample > data.len() {
            break;
        }

        let sample_f64 = match bit_depth {
            16 => {
                let val = i16::from_le_bytes([data[offset], data[offset + 1]]);
                val as f64 / i16::MAX as f64
            }
            24 => {
                let val = i32::from_le_bytes([0, data[offset], data[offset + 1], data[offset + 2]]);
                val as f64 / (1 << 23) as f64
            }
            32 => {
                let val = f32::from_le_bytes([
                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                ]);
                val as f64
            }
            _ => 0.0,
        };
        sum_sq += sample_f64 * sample_f64;
    }

    let _ = channels; // RMS is per-sample, channels don't matter here
    (sum_sq / num_samples as f64).sqrt()
}

// ── FLAC validation ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct FlacValidation {
    pub channels: u8,
    pub sample_rate: u32,
    pub duration_secs: f64,
}

/// Validate a FLAC file by checking the magic header and STREAMINFO block.
pub fn validate_flac(path: &Path) -> Result<FlacValidation, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("Failed to read FLAC file: {}", e))?;

    if data.len() < 42 {
        return Err("FLAC file too small".into());
    }

    // Check fLaC magic
    if &data[0..4] != b"fLaC" {
        return Err("Missing fLaC magic header".into());
    }

    // STREAMINFO is always the first metadata block
    // Byte 4: block type (0 = STREAMINFO) + is-last flag in high bit
    let block_type = data[4] & 0x7F;
    if block_type != 0 {
        return Err(format!("First metadata block is not STREAMINFO (type={})", block_type));
    }

    // Block length (3 bytes, big-endian)
    let block_len = ((data[5] as u32) << 16) | ((data[6] as u32) << 8) | (data[7] as u32);
    if block_len < 34 || data.len() < 8 + block_len as usize {
        return Err("STREAMINFO block too small".into());
    }

    let si = &data[8..8 + block_len as usize];

    // STREAMINFO layout (34 bytes):
    // 0-1: min block size
    // 2-3: max block size
    // 4-6: min frame size
    // 7-9: max frame size
    // 10-13: sample rate (20 bits) | channels-1 (3 bits) | bps-1 (5 bits) | total samples high (4 bits)
    // 14-17: total samples low (32 bits)
    // 18-33: MD5 signature

    let sample_rate = ((si[10] as u32) << 12) | ((si[11] as u32) << 4) | ((si[12] as u32) >> 4);
    let channels = ((si[12] >> 1) & 0x07) + 1;
    let total_samples_high = ((si[12] & 0x01) as u64) << 32;
    let total_samples_low = ((si[13] as u64) << 24)
        | ((si[14] as u64) << 16)
        | ((si[15] as u64) << 8)
        | (si[16] as u64);
    let total_samples = total_samples_high | total_samples_low;

    if sample_rate == 0 {
        return Err("FLAC sample rate is 0".into());
    }

    let duration_secs = total_samples as f64 / sample_rate as f64;

    Ok(FlacValidation {
        channels,
        sample_rate,
        duration_secs,
    })
}

// ── MIDI validation ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct MidiValidation {
    pub event_count: usize,
    pub notes_found: Vec<u8>,
    pub duration_ticks: u64,
}

/// Parse and validate a MIDI file by walking MThd + MTrk headers.
pub fn validate_midi(path: &Path) -> Result<MidiValidation, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("Failed to read MIDI file: {}", e))?;

    if data.len() < 14 {
        return Err("MIDI file too small".into());
    }

    // MThd header
    if &data[0..4] != b"MThd" {
        return Err("Missing MThd header".into());
    }

    let header_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if header_len < 6 {
        return Err(format!("MThd header length too small: {}", header_len));
    }

    let _format = u16::from_be_bytes([data[8], data[9]]);
    let num_tracks = u16::from_be_bytes([data[10], data[11]]);

    // Walk tracks and collect note events
    let mut offset = 8 + header_len as usize;
    let mut event_count: usize = 0;
    let mut notes_found: Vec<u8> = Vec::new();
    let mut total_ticks: u64 = 0;

    for _ in 0..num_tracks {
        if offset + 8 > data.len() {
            break;
        }
        if &data[offset..offset + 4] != b"MTrk" {
            return Err("Missing MTrk header".into());
        }
        let track_len = u32::from_be_bytes([
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]);
        let track_end = offset + 8 + track_len as usize;
        let mut pos = offset + 8;
        let mut running_status: u8 = 0;
        let mut track_ticks: u64 = 0;

        while pos < track_end && pos < data.len() {
            // Read variable-length delta time
            let (delta, bytes_read) = read_vlq(&data, pos);
            pos += bytes_read;
            track_ticks += delta as u64;

            if pos >= data.len() {
                break;
            }

            let status_byte = data[pos];

            if status_byte == 0xFF {
                // Meta event
                if pos + 2 >= data.len() { break; }
                let _meta_type = data[pos + 1];
                let (len, vbytes) = read_vlq(&data, pos + 2);
                pos += 2 + vbytes + len as usize;
                event_count += 1;
            } else if status_byte == 0xF0 || status_byte == 0xF7 {
                // SysEx
                let (len, vbytes) = read_vlq(&data, pos + 1);
                pos += 1 + vbytes + len as usize;
                event_count += 1;
            } else {
                // Channel event
                let (status, data_start) = if status_byte & 0x80 != 0 {
                    running_status = status_byte;
                    (status_byte, pos + 1)
                } else {
                    (running_status, pos)
                };

                let msg_type = status & 0xF0;
                let data_len = match msg_type {
                    0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
                    0xC0 | 0xD0 => 1,
                    _ => 0,
                };

                if data_start + data_len <= data.len() {
                    // Track note-on events
                    if msg_type == 0x90 && data_len >= 2 {
                        let note = data[data_start];
                        let velocity = data[data_start + 1];
                        if velocity > 0 && !notes_found.contains(&note) {
                            notes_found.push(note);
                        }
                    }
                }

                pos = data_start + data_len;
                event_count += 1;
            }
        }

        total_ticks = total_ticks.max(track_ticks);
        offset = track_end;
    }

    notes_found.sort();

    Ok(MidiValidation {
        event_count,
        notes_found,
        duration_ticks: total_ticks,
    })
}

/// Read a MIDI variable-length quantity. Returns (value, bytes_consumed).
fn read_vlq(data: &[u8], start: usize) -> (u32, usize) {
    let mut value: u32 = 0;
    let mut bytes = 0;
    loop {
        if start + bytes >= data.len() {
            break;
        }
        let b = data[start + bytes];
        value = (value << 7) | (b & 0x7F) as u32;
        bytes += 1;
        if b & 0x80 == 0 || bytes >= 4 {
            break;
        }
    }
    (value, bytes)
}

// ── OGG validation ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct OggValidation {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_secs: f64,
}

/// Validate an OGG/Vorbis file using GStreamer's Discoverer.
pub fn validate_ogg(path: &Path) -> Result<OggValidation, String> {
    // Check OggS magic header
    let data = std::fs::read(path)
        .map_err(|e| format!("Failed to read OGG file: {}", e))?;

    if data.len() < 4 {
        return Err("OGG file too small".into());
    }
    if &data[0..4] != b"OggS" {
        return Err("Missing OggS header".into());
    }

    let uri = format!("file:///{}", path.display().to_string().replace('\\', "/"));

    let discoverer = gstreamer_pbutils::Discoverer::new(gstreamer::ClockTime::from_seconds(10))
        .map_err(|e| format!("Failed to create Discoverer: {}", e))?;

    let info = discoverer.discover_uri(&uri)
        .map_err(|e| format!("Discoverer failed for {}: {}", uri, e))?;

    let duration_secs = info.duration()
        .map(|d| d.nseconds() as f64 / 1_000_000_000.0)
        .unwrap_or(0.0);

    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;

    for stream in info.audio_streams() {
        channels = stream.channels() as u16;
        sample_rate = stream.sample_rate();
    }

    if channels == 0 {
        return Err("No audio stream found in OGG".into());
    }

    Ok(OggValidation {
        channels,
        sample_rate,
        duration_secs,
    })
}

// ── MKV validation (via GStreamer Discoverer) ─────────────────────────

#[derive(Debug)]
pub struct MkvValidation {
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: String,
    pub has_audio: bool,
}

/// Validate an MKV file using GStreamer's Discoverer.
pub fn validate_mkv(path: &Path) -> Result<MkvValidation, String> {
    use gstreamer_pbutils::prelude::*;

    let uri = format!("file:///{}", path.display().to_string().replace('\\', "/"));

    let discoverer = gstreamer_pbutils::Discoverer::new(gstreamer::ClockTime::from_seconds(10))
        .map_err(|e| format!("Failed to create Discoverer: {}", e))?;

    let info = discoverer.discover_uri(&uri)
        .map_err(|e| format!("Discoverer failed for {}: {}", uri, e))?;

    let duration_secs = info.duration()
        .map(|d| d.nseconds() as f64 / 1_000_000_000.0)
        .unwrap_or(0.0);

    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut fps: f64 = 0.0;
    let mut codec = String::new();
    let mut has_audio = false;

    for stream in info.video_streams() {
        width = stream.width();
        height = stream.height();
        let fps_n = stream.framerate().numer() as f64;
        let fps_d = stream.framerate().denom() as f64;
        if fps_d > 0.0 {
            fps = fps_n / fps_d;
        }
        // Try to get codec name from caps
        if let Some(caps) = stream.caps() {
            if let Some(structure) = caps.structure(0) {
                codec = structure.name().to_string();
            }
        }
    }

    for _stream in info.audio_streams() {
        has_audio = true;
    }

    if width == 0 || height == 0 {
        return Err("No video stream found in MKV".into());
    }

    Ok(MkvValidation {
        duration_secs,
        width,
        height,
        fps,
        codec,
        has_audio,
    })
}
