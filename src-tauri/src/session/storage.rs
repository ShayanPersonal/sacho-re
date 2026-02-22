// Session folder management — directory scan and header parsing

use super::{SessionMetadata, AudioFileInfo, MidiFileInfo, VideoFileInfo};
use super::unsanitize_device_name;
use std::path::Path;
use std::io::{Read, Seek, SeekFrom};
use chrono::{DateTime, NaiveDateTime, Utc, TimeZone};
use gstreamer_pbutils;

// ============================================================================
// Read-only header parsing functions
// ============================================================================

/// Read WAV duration by parsing RIFF fmt+data chunks (read-only, no patching).
pub fn read_wav_duration(path: &Path) -> anyhow::Result<f64> {
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();

    if file_size < 44 {
        return Err(anyhow::anyhow!("File too small to be a valid WAV file"));
    }

    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(anyhow::anyhow!("Not a valid WAV file"));
    }

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

        pos += 8 + chunk_size as u64;
        if chunk_size % 2 != 0 { pos += 1; }
    }

    if data_chunk_offset == 0 || channels == 0 || sample_rate == 0 || bits_per_sample == 0 {
        return Err(anyhow::anyhow!("Could not find fmt/data chunks"));
    }

    let data_size = (file_size - data_chunk_offset - 8) as f64;
    let bytes_per_frame = (bits_per_sample as f64 / 8.0) * channels as f64;
    Ok(data_size / (sample_rate as f64 * bytes_per_frame))
}

/// Read FLAC duration by parsing STREAMINFO block.
/// Falls back to GStreamer Discoverer if total_samples is 0.
pub fn read_flac_duration(path: &Path) -> anyhow::Result<f64> {
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();

    if file_size < 42 {
        return Err(anyhow::anyhow!("File too small to be a valid FLAC file"));
    }

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

    let sample_rate = ((streaminfo[10] as u32) << 12)
        | ((streaminfo[11] as u32) << 4)
        | ((streaminfo[12] as u32) >> 4);

    let total_samples_hi = (streaminfo[13] & 0x0F) as u64;
    let total_samples_lo = u32::from_be_bytes([streaminfo[14], streaminfo[15], streaminfo[16], streaminfo[17]]) as u64;
    let total_samples = (total_samples_hi << 32) | total_samples_lo;

    if total_samples > 0 && sample_rate > 0 {
        return Ok(total_samples as f64 / sample_rate as f64);
    }

    // Fall back to GStreamer Discoverer
    read_video_duration(path)
}

/// Read video (or any media) duration using GStreamer Discoverer.
/// Creates a one-off Discoverer. For batch operations, use
/// `read_video_duration_with_discoverer` with a shared instance.
pub fn read_video_duration(path: &Path) -> anyhow::Result<f64> {
    let discoverer = get_or_create_discoverer()?;
    read_video_duration_with_discoverer(path, &discoverer)
}

/// Read video duration using a pre-created GStreamer Discoverer (avoids
/// repeated init + teardown when scanning many files).
pub fn read_video_duration_with_discoverer(
    path: &Path,
    discoverer: &gstreamer_pbutils::Discoverer,
) -> anyhow::Result<f64> {
    let uri = format!("file:///{}", path.to_string_lossy().replace('\\', "/"));

    let info = discoverer.discover_uri(&uri)
        .map_err(|e| anyhow::anyhow!("Discovery failed: {}", e))?;

    let duration = info.duration()
        .ok_or_else(|| anyhow::anyhow!("No duration found"))?;

    Ok(duration.nseconds() as f64 / 1_000_000_000.0)
}

/// Create a GStreamer Discoverer, ensuring gstreamer::init() has been called.
pub fn get_or_create_discoverer() -> anyhow::Result<gstreamer_pbutils::Discoverer> {
    gstreamer::init().map_err(|e| anyhow::anyhow!("GStreamer init failed: {}", e))?;

    gstreamer_pbutils::Discoverer::new(gstreamer::ClockTime::from_seconds(10))
        .map_err(|e| anyhow::anyhow!("Failed to create discoverer: {}", e))
}

/// Count NoteOn events with velocity > 0 in a MIDI file using midly.
pub fn count_midi_events(path: &Path) -> anyhow::Result<usize> {
    let data = std::fs::read(path)?;
    let smf = midly::Smf::parse(&data)
        .map_err(|e| anyhow::anyhow!("Failed to parse MIDI: {}", e))?;

    let mut count = 0;
    for track in &smf.tracks {
        for event in track {
            if let midly::TrackEventKind::Midi { message, .. } = event.kind {
                match message {
                    midly::MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0 => {
                        count += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(count)
}

// ============================================================================
// Lightweight scan for session index (rescan_sessions)
// ============================================================================

use super::database::SessionIndexData;

/// Lightweight scan of a session directory for the session index.
/// Reads file extensions, parses audio/video durations from headers, reads notes.txt.
/// Does NOT count MIDI events or check MIDI header corruption.
/// If ANY audio/video file fails to return a valid duration, session duration is 0.0.
///
/// Pass `discoverer` to reuse a GStreamer Discoverer across multiple calls
/// (avoids repeated init/teardown). Pass `None` to create one on demand.
pub fn scan_session_dir_for_index(
    session_path: &Path,
    discoverer: Option<&gstreamer_pbutils::Discoverer>,
) -> anyhow::Result<SessionIndexData> {
    let folder_name = session_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid session path"))?
        .to_string();

    let timestamp = parse_session_timestamp(&folder_name)
        .unwrap_or_else(Utc::now);

    let entries = std::fs::read_dir(session_path)?;

    let mut has_audio = false;
    let mut has_midi = false;
    let mut has_video = false;
    let mut durations: Vec<f64> = Vec::new();
    let mut any_duration_failed = false;
    let mut notes = String::new();
    let mut notes_modified_at = String::new();

    // Lazy-init a fallback discoverer only if needed and none was provided
    let mut fallback_discoverer: Option<gstreamer_pbutils::Discoverer> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if fname == "notes.txt" {
            notes = std::fs::read_to_string(&path).unwrap_or_default();
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    let dt: DateTime<Utc> = modified.into();
                    notes_modified_at = dt.to_rfc3339();
                }
            }
            continue;
        }

        if fname.ends_with(".mid") {
            has_midi = true;
        } else if fname.ends_with(".wav") {
            has_audio = true;
            match read_wav_duration(&path) {
                Ok(d) => durations.push(d),
                Err(_) => any_duration_failed = true,
            }
        } else if fname.ends_with(".flac") {
            has_audio = true;
            match read_flac_duration(&path) {
                Ok(d) => durations.push(d),
                Err(_) => any_duration_failed = true,
            }
        } else if fname.ends_with(".mkv") {
            has_video = true;
            let disc = discoverer.or_else(|| {
                if fallback_discoverer.is_none() {
                    fallback_discoverer = get_or_create_discoverer().ok();
                }
                fallback_discoverer.as_ref()
            });
            let result = match disc {
                Some(d) => read_video_duration_with_discoverer(&path, d),
                None => Err(anyhow::anyhow!("Failed to create GStreamer discoverer")),
            };
            match result {
                Ok(d) => durations.push(d),
                Err(_) => any_duration_failed = true,
            }
        }
    }

    let duration_secs = if any_duration_failed {
        0.0
    } else {
        durations.into_iter().fold(0.0f64, f64::max)
    };

    Ok(SessionIndexData {
        id: folder_name,
        timestamp,
        path: session_path.to_string_lossy().to_string(),
        duration_secs,
        has_audio,
        has_midi,
        has_video,
        notes,
        notes_modified_at,
    })
}

// ============================================================================
// Directory scan → SessionMetadata
// ============================================================================

/// Parse a session timestamp from a folder name like "2026-02-21_14-32-45".
pub fn parse_session_timestamp(folder_name: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(folder_name, "%Y-%m-%d_%H-%M-%S")
        .ok()
        .map(|dt| Utc.from_utc_datetime(&dt))
}

/// Build a `SessionMetadata` by scanning a session directory's files.
/// Does NOT auto-repair anything. Detects MIDI corruption via `needs_repair` flag.
pub fn build_session_from_directory(session_path: &Path) -> anyhow::Result<SessionMetadata> {
    let folder_name = session_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid session path"))?
        .to_string();

    let timestamp = parse_session_timestamp(&folder_name)
        .unwrap_or_else(Utc::now);

    let entries = std::fs::read_dir(session_path)?;

    let mut audio_files = Vec::new();
    let mut midi_files = Vec::new();
    let mut video_files = Vec::new();
    let mut notes = String::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if fname == "notes.txt" {
            notes = std::fs::read_to_string(&path).unwrap_or_default();
            continue;
        }

        if fname.ends_with(".mid") {
            // Extract device name: "midi_Device_Name.mid" → "Device Name"
            let sanitized = fname.trim_start_matches("midi_").trim_end_matches(".mid");
            let device_name = unsanitize_device_name(sanitized);

            let needs_repair = crate::recording::monitor::midi_file_needs_repair(&path.to_path_buf());

            let event_count = if !needs_repair {
                count_midi_events(&path).unwrap_or(0)
            } else {
                0
            };

            midi_files.push(MidiFileInfo {
                filename: fname,
                device_name,
                event_count,
                needs_repair,
            });
        } else if fname.ends_with(".wav") {
            let sanitized = fname.trim_start_matches("audio_").trim_end_matches(".wav");
            let device_name = unsanitize_device_name(sanitized);
            let duration_secs = read_wav_duration(&path).unwrap_or(0.0);

            audio_files.push(AudioFileInfo {
                filename: fname,
                device_name,
                duration_secs,
            });
        } else if fname.ends_with(".flac") {
            let sanitized = fname.trim_start_matches("audio_").trim_end_matches(".flac");
            let device_name = unsanitize_device_name(sanitized);
            let duration_secs = read_flac_duration(&path).unwrap_or(0.0);

            audio_files.push(AudioFileInfo {
                filename: fname,
                device_name,
                duration_secs,
            });
        } else if fname.ends_with(".mkv") {
            let sanitized = fname.trim_start_matches("video_").trim_end_matches(".mkv");
            let device_name = unsanitize_device_name(sanitized);
            let duration_secs = read_video_duration(&path).unwrap_or(0.0);

            video_files.push(VideoFileInfo {
                filename: fname,
                device_name,
                duration_secs,
            });
        }
    }

    // Compute session duration = max of all file durations
    let max_audio = audio_files.iter().map(|f| f.duration_secs).fold(0.0f64, f64::max);
    let max_video = video_files.iter().map(|f| f.duration_secs).fold(0.0f64, f64::max);
    let duration_secs = max_audio.max(max_video);

    Ok(SessionMetadata {
        id: folder_name,
        timestamp,
        duration_secs,
        path: session_path.to_path_buf(),
        audio_files,
        midi_files,
        video_files,
        notes,
    })
}
