use std::time::{Duration, Instant};

use crate::config::{AudioFormat, Config};
use crate::recording::RecordingStatus;

use super::app::TestApp;
use super::discovery::TestSettings;
use super::midi_sender::MidiSender;
use super::validators;

// ── Test case types ──────────────────────────────────────────────────

/// How the test triggers recording.
#[derive(Debug, Clone)]
pub enum TriggerMode {
    /// Send MIDI via loopback device with given name_contains.
    Midi { loopback_name_contains: String },
    /// Use manual_start_recording / manual_stop_recording.
    Manual,
}

/// What outputs we expect from a test.
#[derive(Debug, Clone)]
pub struct Expected {
    pub has_midi: bool,
    pub has_audio: bool,
    pub has_video: bool,
    pub duration_secs: f64,
    pub duration_tolerance: f64,
    pub audio_format: Option<AudioFormat>,
    pub video_codec: Option<String>,
    pub video_resolution: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub config: Config,
    pub trigger: TriggerMode,
    /// How long to play MIDI / keep recording active (seconds).
    pub play_duration_secs: u32,
    /// Specific MIDI notes to send for content validation.
    pub notes_to_send: Vec<u8>,
    pub expected: Expected,
    /// Pipeline warmup + file finalization settings.
    pub settings: TestSettings,
}

/// Result of a single test run.
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub errors: Vec<String>,
}

// ── Test execution ───────────────────────────────────────────────────

pub fn run_test(case: &TestCase, keep_sessions: bool) -> TestResult {
    let start = Instant::now();
    let mut errors: Vec<String> = Vec::new();

    // 1. Build headless app
    let app = TestApp::new(case.config.clone(), keep_sessions);

    // 2. Start monitor (connects devices, starts pipelines)
    if let Err(e) = app.start_monitor() {
        return TestResult {
            name: case.name.clone(),
            passed: false,
            duration_ms: start.elapsed().as_millis() as u64,
            errors: vec![format!("Failed to start monitor: {}", e)],
        };
    }

    // 3. Pipeline warmup
    std::thread::sleep(Duration::from_secs(case.settings.pipeline_warmup_secs));

    // 4. Trigger recording
    match &case.trigger {
        TriggerMode::Midi { loopback_name_contains } => {
            let mut sender = match MidiSender::connect(loopback_name_contains) {
                Some(s) => s,
                None => {
                    app.stop_monitor();
                    return TestResult {
                        name: case.name.clone(),
                        passed: false,
                        duration_ms: start.elapsed().as_millis() as u64,
                        errors: vec![format!(
                            "Failed to connect MidiSender to '{}'",
                            loopback_name_contains
                        )],
                    };
                }
            };

            // Send initial trigger note
            sender.note_on(0, 60, 100);
            std::thread::sleep(Duration::from_millis(50));
            sender.note_off(0, 60);

            // Wait for recording to start
            if !app.wait_for_status(RecordingStatus::Recording, Duration::from_secs(10)) {
                errors.push("Recording did not start within 10s after MIDI trigger".into());
                app.stop_monitor();
                return TestResult {
                    name: case.name.clone(),
                    passed: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                    errors,
                };
            }

            // Play the test notes sequence (keeps recording alive via MIDI activity)
            if !case.notes_to_send.is_empty() {
                let hold = Duration::from_millis(200);
                let gap = Duration::from_millis(100);
                let seq: Vec<(u8, Duration, Duration)> = case
                    .notes_to_send
                    .iter()
                    .map(|&n| (n, hold, gap))
                    .collect();
                sender.play_sequence(&seq);
            }

            // Keep alive for the remaining play duration
            let notes_time_ms =
                case.notes_to_send.len() as u64 * 300; // 200ms hold + 100ms gap per note
            let play_ms = case.play_duration_secs as u64 * 1000;
            if play_ms > notes_time_ms {
                let remaining = Duration::from_millis(play_ms - notes_time_ms);
                sender.keep_alive(Duration::from_millis(500), remaining);
            }

            // Stop sending MIDI — idle timeout will stop recording
            let idle_timeout = case.config.idle_timeout_secs as u64;
            let wait_for_stop = Duration::from_secs(idle_timeout + 10);
            if !app.wait_for_status(RecordingStatus::Idle, wait_for_stop) {
                errors.push(format!(
                    "Recording did not stop within {}s after last MIDI event (idle_timeout={}s)",
                    idle_timeout + 10,
                    idle_timeout
                ));
            }
        }
        TriggerMode::Manual => {
            if let Err(e) = app.manual_start_recording() {
                app.stop_monitor();
                return TestResult {
                    name: case.name.clone(),
                    passed: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                    errors: vec![format!("Manual start failed: {}", e)],
                };
            }

            // Wait for recording to start
            if !app.wait_for_status(RecordingStatus::Recording, Duration::from_secs(10)) {
                errors.push("Recording did not start within 10s after manual start".into());
                app.stop_monitor();
                return TestResult {
                    name: case.name.clone(),
                    passed: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                    errors,
                };
            }

            // Wait for play duration
            std::thread::sleep(Duration::from_secs(case.play_duration_secs as u64));

            // Stop manually
            if let Err(e) = app.manual_stop_recording() {
                errors.push(format!("Manual stop failed: {}", e));
            }

            // Wait for idle
            if !app.wait_for_status(RecordingStatus::Idle, Duration::from_secs(15)) {
                errors.push("Recording did not reach Idle within 15s after manual stop".into());
            }
        }
    }

    // 5. File finalization delay
    std::thread::sleep(Duration::from_secs(case.settings.file_finalization_secs));

    // 6. Find session dir and validate
    let session_dirs = app.session_dirs();
    if session_dirs.is_empty() {
        errors.push("No session directory created".into());
    } else {
        let session_dir = &session_dirs[session_dirs.len() - 1];

        // Parse metadata.json
        let meta_path = session_dir.join("metadata.json");
        let metadata: Option<crate::session::SessionMetadata> = std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok());

        if metadata.is_none() {
            errors.push(format!(
                "metadata.json not found or invalid at {}",
                meta_path.display()
            ));
        }

        // Validate MIDI files
        if case.expected.has_midi {
            let midi_files: Vec<_> = std::fs::read_dir(session_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    e.path().extension().map(|x| x == "mid").unwrap_or(false)
                })
                .collect();

            if midi_files.is_empty() {
                errors.push("Expected MIDI file but none found".into());
            } else {
                for entry in &midi_files {
                    match validators::validate_midi(&entry.path()) {
                        Ok(v) => {
                            if v.event_count == 0 {
                                errors.push(format!(
                                    "MIDI file {} has 0 events",
                                    entry.file_name().to_string_lossy()
                                ));
                            }
                            // Check that our sent notes appear in the file
                            for note in &case.notes_to_send {
                                if !v.notes_found.contains(note) {
                                    errors.push(format!(
                                        "MIDI file missing expected note {} (found: {:?})",
                                        note, v.notes_found
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            errors.push(format!(
                                "MIDI validation failed for {}: {}",
                                entry.file_name().to_string_lossy(),
                                e
                            ));
                        }
                    }
                }
            }
        }

        // Validate audio files
        if case.expected.has_audio {
            let expected_ext = match &case.expected.audio_format {
                Some(AudioFormat::Flac) => "flac",
                _ => "wav",
            };

            let audio_files: Vec<_> = std::fs::read_dir(session_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x == expected_ext)
                        .unwrap_or(false)
                })
                .collect();

            if audio_files.is_empty() {
                errors.push(format!("Expected {} audio file but none found", expected_ext));
            } else {
                for entry in &audio_files {
                    let path = entry.path();
                    if expected_ext == "wav" {
                        match validators::validate_wav(&path) {
                            Ok(v) => {
                                validate_duration(
                                    &mut errors,
                                    &format!("WAV {}", entry.file_name().to_string_lossy()),
                                    v.duration_secs,
                                    case.expected.duration_secs,
                                    case.expected.duration_tolerance,
                                );
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "WAV validation failed for {}: {}",
                                    entry.file_name().to_string_lossy(),
                                    e
                                ));
                            }
                        }
                    } else {
                        match validators::validate_flac(&path) {
                            Ok(v) => {
                                validate_duration(
                                    &mut errors,
                                    &format!("FLAC {}", entry.file_name().to_string_lossy()),
                                    v.duration_secs,
                                    case.expected.duration_secs,
                                    case.expected.duration_tolerance,
                                );
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "FLAC validation failed for {}: {}",
                                    entry.file_name().to_string_lossy(),
                                    e
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Validate video files
        if case.expected.has_video {
            let video_files: Vec<_> = std::fs::read_dir(session_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x == "mkv")
                        .unwrap_or(false)
                })
                .collect();

            if video_files.is_empty() {
                errors.push("Expected MKV video file but none found".into());
            } else {
                for entry in &video_files {
                    match validators::validate_mkv(&entry.path()) {
                        Ok(v) => {
                            validate_duration(
                                &mut errors,
                                &format!("MKV {}", entry.file_name().to_string_lossy()),
                                v.duration_secs,
                                case.expected.duration_secs,
                                case.expected.duration_tolerance,
                            );

                            if let Some((ew, eh)) = case.expected.video_resolution {
                                if v.width != ew || v.height != eh {
                                    errors.push(format!(
                                        "MKV resolution {}x{} != expected {}x{}",
                                        v.width, v.height, ew, eh
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            errors.push(format!(
                                "MKV validation failed for {}: {}",
                                entry.file_name().to_string_lossy(),
                                e
                            ));
                        }
                    }
                }
            }
        }
    }

    // 7. Cleanup
    app.stop_monitor();

    TestResult {
        name: case.name.clone(),
        passed: errors.is_empty(),
        duration_ms: start.elapsed().as_millis() as u64,
        errors,
    }
}

fn validate_duration(
    errors: &mut Vec<String>,
    label: &str,
    actual: f64,
    expected: f64,
    tolerance: f64,
) {
    if (actual - expected).abs() > tolerance {
        errors.push(format!(
            "{} duration {:.1}s outside expected {:.1}s +/- {:.1}s",
            label, actual, expected, tolerance
        ));
    }
}

/// Print a formatted summary of all test results.
pub fn print_summary(results: &[TestResult]) {
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;

    println!("\n  === Test Results ===\n");

    for (i, result) in results.iter().enumerate() {
        let status = if result.passed { "PASS" } else { "FAIL" };
        let duration = format!("{:.1}s", result.duration_ms as f64 / 1000.0);
        println!(
            "  [{}/{}] {} {} {} ({})",
            i + 1,
            results.len(),
            result.name,
            ".".repeat(50_usize.saturating_sub(result.name.len())),
            status,
            duration
        );
        for err in &result.errors {
            println!("         -> {}", err);
        }
    }

    println!();
    if failed == 0 {
        println!("  Results: {} passed, 0 failed", passed);
    } else {
        println!("  Results: {} passed, {} FAILED", passed, failed);
    }
    println!();
}
