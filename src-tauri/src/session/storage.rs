// Session folder management — directory scan and header parsing

use super::{SessionMetadata, AudioFileInfo, MidiFileInfo, VideoFileInfo};
use super::unsanitize_device_name;
use std::path::Path;
use std::io::{Read, Seek, SeekFrom};
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, NaiveDateTime, Utc, TimeZone};
use gstreamer_pbutils;
use serde::{Serialize, Deserialize};

// ============================================================================
// Recording lock file helpers
// ============================================================================

pub const LOCK_FILE_NAME: &str = ".sacho_recording";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingLockInfo {
    pub hostname: String,
    pub pid: u32,
    pub updated_at: String,
}

pub fn create_recording_lock(session_path: &Path) -> anyhow::Result<()> {
    let lock = RecordingLockInfo {
        hostname: sysinfo::System::host_name().unwrap_or_default(),
        pid: std::process::id(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let json = serde_json::to_string_pretty(&lock)?;
    std::fs::write(session_path.join(LOCK_FILE_NAME), json)?;
    Ok(())
}

pub fn touch_recording_lock(session_path: &Path) {
    let lock_path = session_path.join(LOCK_FILE_NAME);
    if lock_path.exists() {
        let lock = RecordingLockInfo {
            hostname: sysinfo::System::host_name().unwrap_or_default(),
            pid: std::process::id(),
            updated_at: Utc::now().to_rfc3339(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&lock) {
            let _ = std::fs::write(&lock_path, json);
        }
    }
}

pub fn remove_recording_lock(session_path: &Path) {
    let _ = std::fs::remove_file(session_path.join(LOCK_FILE_NAME));
}

pub fn has_recording_lock(session_path: &Path) -> bool {
    session_path.join(LOCK_FILE_NAME).exists()
}

pub fn read_recording_lock(session_path: &Path) -> Option<RecordingLockInfo> {
    let lock_path = session_path.join(LOCK_FILE_NAME);
    let data = std::fs::read_to_string(&lock_path).ok()?;
    serde_json::from_str(&data).ok()
}

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

/// Read MKV/WebM duration by parsing EBML Segment > Info > Duration.
/// Works for both MKV and WebM files (both are EBML-based).
/// Only reads a few KB from the start of the file — no GStreamer needed.
pub fn read_ebml_duration(path: &Path) -> anyhow::Result<f64> {
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    // Read up to 64KB — Segment Info is almost always in the first few KB
    let cap = file_size.min(65536) as usize;
    let mut buf = vec![0u8; cap];
    file.read_exact(&mut buf)?;

    // EBML element IDs we care about
    const SEGMENT: u32        = 0x18538067;
    const INFO: u32           = 0x1549A966;
    const TIMESTAMP_SCALE: u32 = 0x2AD7B1;
    const DURATION: u32       = 0x4489;

    /// Read an EBML variable-length integer (element ID).
    /// Returns (id, bytes_consumed). IDs keep the leading VINT_MARKER bit.
    fn read_ebml_id(data: &[u8]) -> Option<(u32, usize)> {
        if data.is_empty() { return None; }
        let first = data[0];
        if first == 0 { return None; }
        let len = first.leading_zeros() as usize + 1;
        if len > 4 || data.len() < len { return None; }
        let mut val = 0u32;
        for i in 0..len {
            val = (val << 8) | data[i] as u32;
        }
        Some((val, len))
    }

    /// Read an EBML variable-length integer (data size).
    /// Returns (size, bytes_consumed). Strips the VINT_MARKER bit.
    fn read_ebml_size(data: &[u8]) -> Option<(u64, usize)> {
        if data.is_empty() { return None; }
        let first = data[0];
        if first == 0 { return None; }
        let len = first.leading_zeros() as usize + 1;
        if len > 8 || data.len() < len { return None; }
        let mask = if len >= 8 { 0u8 } else { 0xFF >> len };
        let mut val = (first & mask) as u64;
        for i in 1..len {
            val = (val << 8) | data[i] as u64;
        }
        // All-ones = unknown size
        let max = (1u64 << (7 * len)) - 1;
        if val == max { val = u64::MAX; }
        Some((val, len))
    }

    fn read_ebml_float(data: &[u8], size: usize) -> Option<f64> {
        match size {
            4 => {
                let bytes: [u8; 4] = data[..4].try_into().ok()?;
                Some(f32::from_be_bytes(bytes) as f64)
            }
            8 => {
                let bytes: [u8; 8] = data[..8].try_into().ok()?;
                Some(f64::from_be_bytes(bytes))
            }
            _ => None,
        }
    }

    fn read_ebml_uint(data: &[u8], size: usize) -> Option<u64> {
        if size == 0 || size > 8 || data.len() < size { return None; }
        let mut val = 0u64;
        for i in 0..size {
            val = (val << 8) | data[i] as u64;
        }
        Some(val)
    }

    // Parse: skip EBML header, enter Segment, find Info, read Duration
    let mut pos = 0usize;

    // Skip EBML header element
    let (id, id_len) = read_ebml_id(&buf[pos..]).ok_or_else(|| anyhow::anyhow!("No EBML header"))?;
    pos += id_len;
    if id != 0x1A45DFA3 {
        return Err(anyhow::anyhow!("Not an EBML file"));
    }
    let (size, size_len) = read_ebml_size(&buf[pos..]).ok_or_else(|| anyhow::anyhow!("Bad EBML header size"))?;
    pos += size_len;
    if size != u64::MAX { pos += size as usize; }

    // Expect Segment element
    if pos >= buf.len() { return Err(anyhow::anyhow!("Truncated before Segment")); }
    let (id, id_len) = read_ebml_id(&buf[pos..]).ok_or_else(|| anyhow::anyhow!("No Segment"))?;
    pos += id_len;
    if id != SEGMENT {
        return Err(anyhow::anyhow!("Expected Segment, got 0x{:X}", id));
    }
    let (_seg_size, size_len) = read_ebml_size(&buf[pos..]).ok_or_else(|| anyhow::anyhow!("Bad Segment size"))?;
    pos += size_len;
    // Now inside Segment — scan top-level children for Info

    let mut timestamp_scale: u64 = 1_000_000; // default: 1ms
    let mut duration_raw: Option<f64> = None;

    // Scan Segment children until we find Info
    while pos + 2 < buf.len() {
        let (child_id, id_len) = match read_ebml_id(&buf[pos..]) {
            Some(v) => v,
            None => break,
        };
        pos += id_len;
        let (child_size, size_len) = match read_ebml_size(&buf[pos..]) {
            Some(v) => v,
            None => break,
        };
        pos += size_len;

        if child_id == INFO {
            // Parse Info children
            let info_end = if child_size == u64::MAX { buf.len() } else { (pos + child_size as usize).min(buf.len()) };
            let mut ipos = pos;
            while ipos + 2 < info_end {
                let (info_child_id, iid_len) = match read_ebml_id(&buf[ipos..]) {
                    Some(v) => v,
                    None => break,
                };
                ipos += iid_len;
                let (info_child_size, isz_len) = match read_ebml_size(&buf[ipos..]) {
                    Some(v) => v,
                    None => break,
                };
                ipos += isz_len;
                let sz = info_child_size as usize;

                if info_child_id == TIMESTAMP_SCALE && ipos + sz <= info_end {
                    if let Some(v) = read_ebml_uint(&buf[ipos..], sz) {
                        timestamp_scale = v;
                    }
                } else if info_child_id == DURATION && ipos + sz <= info_end {
                    duration_raw = read_ebml_float(&buf[ipos..], sz);
                }

                if info_child_size == u64::MAX { break; }
                ipos += sz;
            }
            break; // Done with Info
        }

        // Skip non-Info children
        if child_size == u64::MAX { break; } // unknown-size non-Info element, can't skip
        pos += child_size as usize;
    }

    match duration_raw {
        Some(dur) => {
            let secs = dur * (timestamp_scale as f64) / 1_000_000_000.0;
            if secs > 0.0 && secs.is_finite() {
                Ok(secs)
            } else {
                Err(anyhow::anyhow!("Invalid MKV duration: {}", secs))
            }
        }
        None => Err(anyhow::anyhow!("Duration element not found in MKV header")),
    }
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
    discoverer_fallback_count: Option<&std::sync::atomic::AtomicUsize>,
) -> anyhow::Result<SessionIndexData> {
    let folder_name = session_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid session path"))?
        .to_string();

    let timestamp = parse_session_timestamp(&folder_name)
        .unwrap_or_else(|| fallback_timestamp_from_dir(session_path));

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
        } else if crate::encoding::is_video_extension(&fname) {
            has_video = true;
            // Fast path: parse container header directly
            let result = if fname.ends_with(".mkv") || fname.ends_with(".webm") {
                // EBML-based: parse MKV/WebM header
                read_ebml_duration(&path)
            } else {
                // MP4: try fast parser, fall back to GStreamer Discoverer
                Err(anyhow::anyhow!("Use discoverer for MP4"))
            };
            let result = result.or_else(|_| {
                // Fallback: GStreamer Discoverer
                if let Some(counter) = discoverer_fallback_count {
                    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let disc = discoverer.or_else(|| {
                    if fallback_discoverer.is_none() {
                        fallback_discoverer = get_or_create_discoverer().ok();
                    }
                    fallback_discoverer.as_ref()
                });
                match disc {
                    Some(d) => read_video_duration_with_discoverer(&path, d),
                    None => Err(anyhow::anyhow!("No discoverer available")),
                }
            });
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

    // If folder name doesn't match the expected timestamp format, use the full
    // folder name as the title (non-standard folder — title is not editable).
    let title = if parse_session_timestamp(&folder_name).is_some() {
        extract_title_from_folder_name(&folder_name)
    } else {
        Some(folder_name.clone())
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
        title,
    })
}

// ============================================================================
// Directory scan → SessionMetadata
// ============================================================================

/// Parse a session timestamp from a folder name.
/// Supports formats:
///   "2026-02-25_17-46-00 PST"           → local time with timezone abbreviation
///   "2026-02-25_17-46-00 PST - My Song" → with title
///   "2026-02-25_01-46-00"               → no timezone → assume UTC (legacy)
pub fn parse_session_timestamp(folder_name: &str) -> Option<DateTime<Utc>> {
    let timestamp_part = folder_name.split(" - ").next().unwrap_or(folder_name);

    // Try splitting off a timezone suffix: "2026-02-25_17-46-00 PST" → ("2026-02-25_17-46-00", "PST")
    if let Some((datetime_str, tz_str)) = timestamp_part.rsplit_once(' ') {
        let naive = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d_%H-%M-%S").ok()?;
        if let Some(offset) = tz_abbr_to_offset(tz_str) {
            // Known timezone — interpret as local time in that timezone
            return Some(offset.from_local_datetime(&naive).unwrap().with_timezone(&Utc));
        }
    }

    // No timezone suffix (or unknown abbreviation) — parse as UTC for backwards compat
    NaiveDateTime::parse_from_str(timestamp_part, "%Y-%m-%d_%H-%M-%S")
        .ok()
        .map(|dt| Utc.from_utc_datetime(&dt))
}

/// Map common timezone abbreviations to fixed UTC offsets.
fn tz_abbr_to_offset(abbr: &str) -> Option<FixedOffset> {
    let secs = match abbr {
        "UTC" | "GMT" => 0,
        // North America
        "EST" => -5 * 3600,
        "EDT" => -4 * 3600,
        "CST" => -6 * 3600,
        "CDT" => -5 * 3600,
        "MST" => -7 * 3600,
        "MDT" => -6 * 3600,
        "PST" => -8 * 3600,
        "PDT" => -7 * 3600,
        "AKST" => -9 * 3600,
        "AKDT" => -8 * 3600,
        "HST" => -10 * 3600,
        "HAST" => -10 * 3600,
        "HADT" => -9 * 3600,
        // Europe
        "WET" => 0,
        "WEST" => 1 * 3600,
        "CET" => 1 * 3600,
        "CEST" => 2 * 3600,
        "EET" => 2 * 3600,
        "EEST" => 3 * 3600,
        "GMT+1" => 1 * 3600,
        // Asia
        "IST" => 5 * 3600 + 30 * 60,
        "JST" => 9 * 3600,
        "KST" => 9 * 3600,
        "HKT" => 8 * 3600,
        "SGT" => 8 * 3600,
        // Australia
        "AEST" => 10 * 3600,
        "AEDT" => 11 * 3600,
        "ACST" => 9 * 3600 + 30 * 60,
        "ACDT" => 10 * 3600 + 30 * 60,
        "AWST" => 8 * 3600,
        // Try numeric offset: "+0530", "-0800"
        _ => return parse_numeric_tz_offset(abbr),
    };
    FixedOffset::east_opt(secs)
}

/// Parse a numeric timezone offset like "+0530" or "-0800" into a FixedOffset.
fn parse_numeric_tz_offset(s: &str) -> Option<FixedOffset> {
    if s.len() == 5 && (s.starts_with('+') || s.starts_with('-')) {
        let sign: i32 = if s.starts_with('-') { -1 } else { 1 };
        let hours: i32 = s[1..3].parse().ok()?;
        let minutes: i32 = s[3..5].parse().ok()?;
        FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
    } else {
        None
    }
}

/// Get a timezone abbreviation for a local datetime.
/// Tries the system timezone name first (%Z → "Pacific Standard Time" → "PST").
/// Falls back to determining standard/DST from UTC offset and mapping to a known
/// abbreviation (e.g. PST, PDT, EST, EDT). Uses numeric offset ("-0800") as last resort.
pub fn local_timezone_abbreviation(dt: &DateTime<Local>) -> String {
    // Try %Z first (works on Unix/macOS, sometimes Windows)
    let name = dt.format("%Z").to_string();
    if !name.trim().is_empty() {
        if name.len() <= 5 && !name.contains(' ') {
            return name;
        }
        let abbr: String = name.split_whitespace()
            .filter_map(|w| w.chars().next())
            .filter(|c| c.is_alphabetic())
            .collect::<String>()
            .to_uppercase();
        if !abbr.is_empty() {
            return abbr;
        }
    }

    // Determine standard vs DST by comparing January and July offsets.
    // Standard time always has the smaller UTC offset (DST = standard + 3600).
    let current_secs = dt.offset().local_minus_utc();

    let jan = NaiveDate::from_ymd_opt(dt.year(), 1, 15).unwrap()
        .and_hms_opt(12, 0, 0).unwrap();
    let jul = NaiveDate::from_ymd_opt(dt.year(), 7, 15).unwrap()
        .and_hms_opt(12, 0, 0).unwrap();

    let jan_secs = Local.from_local_datetime(&jan)
        .earliest()
        .map(|d| d.offset().local_minus_utc())
        .unwrap_or(current_secs);
    let jul_secs = Local.from_local_datetime(&jul)
        .earliest()
        .map(|d| d.offset().local_minus_utc())
        .unwrap_or(current_secs);

    let (standard_secs, is_dst) = if jan_secs == jul_secs {
        (jan_secs, false)
    } else {
        let std_secs = jan_secs.min(jul_secs);
        (std_secs, current_secs != std_secs)
    };

    if let Some(abbr) = offset_to_tz_abbreviation(standard_secs, is_dst) {
        return abbr.to_string();
    }

    // Last resort: numeric offset "-0800", "+0530", etc.
    dt.format("%z").to_string()
}

/// Map a standard-time UTC offset (seconds) + DST flag to a timezone abbreviation.
fn offset_to_tz_abbreviation(standard_offset_secs: i32, is_dst: bool) -> Option<&'static str> {
    match (standard_offset_secs, is_dst) {
        // North America
        (-36000, false) => Some("HST"),
        (-36000, true)  => Some("HDT"),
        (-32400, false) => Some("AKST"),
        (-32400, true)  => Some("AKDT"),
        (-28800, false) => Some("PST"),
        (-28800, true)  => Some("PDT"),
        (-25200, false) => Some("MST"),
        (-25200, true)  => Some("MDT"),
        (-21600, false) => Some("CST"),
        (-21600, true)  => Some("CDT"),
        (-18000, false) => Some("EST"),
        (-18000, true)  => Some("EDT"),
        // Europe
        (0, false)      => Some("GMT"),
        (0, true)       => Some("BST"),
        (3600, false)   => Some("CET"),
        (3600, true)    => Some("CEST"),
        (7200, false)   => Some("EET"),
        (7200, true)    => Some("EEST"),
        // Asia (no DST)
        (19800, _)      => Some("IST"),
        (28800, _)      => Some("SGT"),
        (32400, _)      => Some("JST"),
        // Australia
        (34200, false)  => Some("ACST"),
        (34200, true)   => Some("ACDT"),
        (36000, false)  => Some("AEST"),
        (36000, true)   => Some("AEDT"),
        _ => None,
    }
}

/// Fallback timestamp for sessions whose folder name doesn't contain a parseable timestamp.
/// Uses the most recent modification time among files in the directory (or the directory itself).
fn fallback_timestamp_from_dir(session_path: &Path) -> DateTime<Utc> {
    let mut latest: Option<std::time::SystemTime> = None;

    if let Ok(entries) = std::fs::read_dir(session_path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    latest = Some(match latest {
                        Some(prev) if modified > prev => modified,
                        Some(prev) => prev,
                        None => modified,
                    });
                }
            }
        }
    }

    // Fall back to directory mtime if no files found
    if latest.is_none() {
        if let Ok(meta) = std::fs::metadata(session_path) {
            latest = meta.modified().ok();
        }
    }

    latest
        .map(|t| -> DateTime<Utc> { t.into() })
        .unwrap_or_else(Utc::now)
}

/// Extract title from a folder name like "2026-01-22_20-19-05 - sad Song".
/// Returns None if no " - " separator or folder is just a timestamp.
pub fn extract_title_from_folder_name(folder_name: &str) -> Option<String> {
    folder_name.split_once(" - ").map(|(_, title)| title.to_string())
}

/// Build a folder name from a timestamp string and optional title.
/// "2026-01-22_20-19-05" + Some("sad Song") => "2026-01-22_20-19-05 - sad Song"
pub fn build_folder_name(timestamp_prefix: &str, title: Option<&str>) -> String {
    match title {
        Some(t) if !t.is_empty() => format!("{} - {}", timestamp_prefix, t),
        _ => timestamp_prefix.to_string(),
    }
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
        .unwrap_or_else(|| fallback_timestamp_from_dir(session_path));

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

        if fname == LOCK_FILE_NAME {
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
        } else if crate::encoding::is_video_extension(&fname) {
            let sanitized = crate::encoding::strip_video_extension(
                fname.trim_start_matches("video_")
            );
            let device_name = unsanitize_device_name(sanitized);
            let duration_secs = if fname.ends_with(".mkv") || fname.ends_with(".webm") {
                read_ebml_duration(&path)
                    .or_else(|_| read_video_duration(&path))
                    .unwrap_or(0.0)
            } else {
                read_video_duration(&path).unwrap_or(0.0)
            };

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

    // If folder name doesn't match the expected timestamp format, use the full
    // folder name as the title (non-standard folder — title is not editable).
    let title = if parse_session_timestamp(&folder_name).is_some() {
        extract_title_from_folder_name(&folder_name)
    } else {
        Some(folder_name.clone())
    };

    // Read recording lock info
    let lock_info = read_recording_lock(session_path);
    let recording_in_progress = lock_info.is_some();
    let recording_lock_updated_at = lock_info.as_ref().map(|l| l.updated_at.clone());
    let recording_lock_is_local = lock_info
        .as_ref()
        .map(|l| l.hostname == sysinfo::System::host_name().unwrap_or_default())
        .unwrap_or(false);

    Ok(SessionMetadata {
        id: folder_name,
        timestamp,
        duration_secs,
        path: session_path.to_path_buf(),
        audio_files,
        midi_files,
        video_files,
        notes,
        title,
        recording_in_progress,
        recording_lock_updated_at,
        recording_lock_is_local,
    })
}
