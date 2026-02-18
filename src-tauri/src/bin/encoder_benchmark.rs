//! Sacho Encoder Benchmark
//!
//! Benchmarks all available video encoders using the app's actual encoding
//! infrastructure (same codecs, presets, pipeline construction, hardware detection).
//!
//! Usage:
//!   cargo run --bin encoder_benchmark [-- [OPTIONS]]
//!
//! Options:
//!   --codec <filter>    Only benchmark encoders whose codec name contains <filter>
//!   --duration <secs>   Override benchmark duration per encoder (default: 15s)
//!   --verbose           Extra debug output

use std::time::{Duration, Instant};

use sacho_lib::encoding::{
    available_encoders_for_codec, AsyncVideoEncoder, EncoderConfig, EncoderStats,
    HardwareEncoderType, RawVideoFrame, VideoCodec,
};
use sacho_lib::encoding::presets::{preset_label, DEFAULT_PRESET};
use sacho_lib::gstreamer_init;

/// Codecs that have encode pipelines in the app
const BENCHMARK_CODECS: &[VideoCodec] = &[
    VideoCodec::Av1,
    VideoCodec::Vp9,
    VideoCodec::Vp8,
    VideoCodec::H264,
    VideoCodec::Ffv1,
];

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FPS: f64 = 30.0;
const DEFAULT_DURATION_SECS: u64 = 15;

/// Result for a single encoder benchmark run
struct BenchmarkResult {
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    gst_element: &'static str,
    stats: Option<EncoderStats>,
    file_size: u64,
    error: Option<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let verbose = args.iter().any(|a| a == "--verbose");
    let duration_secs = args
        .iter()
        .position(|a| a == "--duration")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_DURATION_SECS);
    let codec_filter = args
        .iter()
        .position(|a| a == "--codec")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // Init logging
    let log_level = if verbose { "debug" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    // On Windows, attach to parent console for output
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    println!("\n=== Sacho Encoder Benchmark ===\n");
    println!(
        "  Resolution: {}x{} @ {:.0} fps",
        WIDTH, HEIGHT, FPS
    );
    println!("  Duration:   {}s per encoder", duration_secs);
    println!(
        "  Preset:     {} (level {})",
        preset_label(DEFAULT_PRESET),
        DEFAULT_PRESET
    );
    if let Some(ref filter) = codec_filter {
        println!("  Filter:     codec contains '{}'", filter);
    }
    println!();

    // Init GStreamer
    gstreamer_init::init_gstreamer_env();

    // Discover available encoders
    println!("  Discovering encoders...\n");
    let mut encoders: Vec<(VideoCodec, HardwareEncoderType, &'static str)> = Vec::new();

    for &codec in BENCHMARK_CODECS {
        if let Some(ref filter) = codec_filter {
            let name = codec.display_name().to_lowercase();
            if !name.contains(&filter.to_lowercase()) {
                continue;
            }
        }

        let available = available_encoders_for_codec(codec);
        for (hw_type, element) in available {
            println!(
                "    Found: {} / {} ({})",
                codec.display_name(),
                hw_type.display_name(),
                element,
            );
            encoders.push((codec, hw_type, element));
        }
    }

    if encoders.is_empty() {
        println!("  No encoders found. Check GStreamer installation and codec filter.");
        std::process::exit(0);
    }

    println!("\n  Running {} benchmarks...\n", encoders.len());

    // Run benchmarks
    let mut results: Vec<BenchmarkResult> = Vec::new();

    for (i, &(codec, hw_type, element)) in encoders.iter().enumerate() {
        println!(
            "  [{}/{}] {} / {} ({})...",
            i + 1,
            encoders.len(),
            codec.display_name(),
            hw_type.display_name(),
            element,
        );

        let result = run_benchmark(codec, hw_type, element, duration_secs, verbose);

        match &result.error {
            Some(err) => {
                println!(
                    "  [{}/{}] FAILED: {}\n",
                    i + 1,
                    encoders.len(),
                    err,
                );
            }
            None => {
                if let Some(ref stats) = result.stats {
                    let realtime_mult = if stats.encoding_duration.as_secs_f64() > 0.0 {
                        stats.content_duration.as_secs_f64()
                            / stats.encoding_duration.as_secs_f64()
                    } else {
                        0.0
                    };
                    let bitrate_mbps = if stats.content_duration.as_secs_f64() > 0.0 {
                        (result.file_size as f64 * 8.0)
                            / stats.content_duration.as_secs_f64()
                            / 1_000_000.0
                    } else {
                        0.0
                    };
                    println!(
                        "  [{}/{}] {} frames, {:.1} fps, {:.2}x realtime, {:.1} MB, {:.1} Mbps\n",
                        i + 1,
                        encoders.len(),
                        stats.frames_encoded,
                        stats.average_fps,
                        realtime_mult,
                        result.file_size as f64 / 1_048_576.0,
                        bitrate_mbps,
                    );
                }
            }
        }

        results.push(result);
    }

    // Print summary table
    print_summary(&results);
}

/// Generate a single NV12 frame with gradient pattern and per-frame variation.
///
/// NV12 layout: W*H bytes of Y plane, then W*H/2 bytes of interleaved UV plane.
/// The gradient provides spatial correlation (realistic for encoders), and
/// frame_index adds temporal variation so successive frames differ.
fn generate_nv12_frame(width: u32, height: u32, frame_index: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = w * h / 2;
    let mut data = vec![0u8; y_size + uv_size];

    // Y plane: horizontal gradient + vertical gradient + temporal variation
    let phase = (frame_index as f64 * 0.05).sin() * 30.0;
    for row in 0..h {
        for col in 0..w {
            let horiz = (col as f64 / w as f64 * 200.0) as f64;
            let vert = (row as f64 / h as f64 * 55.0) as f64;
            let val = (horiz + vert + phase).clamp(0.0, 255.0) as u8;
            data[row * w + col] = val;
        }
    }

    // UV plane: slower-moving color gradient
    let uv_phase = (frame_index as f64 * 0.02).cos() * 20.0;
    let uv_h = h / 2;
    let uv_offset = y_size;
    for row in 0..uv_h {
        for col in (0..w).step_by(2) {
            let u = (128.0 + (col as f64 / w as f64 * 40.0) + uv_phase).clamp(0.0, 255.0) as u8;
            let v = (128.0 + (row as f64 / uv_h as f64 * 40.0) - uv_phase).clamp(0.0, 255.0) as u8;
            data[uv_offset + row * w + col] = u;
            data[uv_offset + row * w + col + 1] = v;
        }
    }

    data
}

/// Run a single encoder benchmark
fn run_benchmark(
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    gst_element: &'static str,
    duration_secs: u64,
    verbose: bool,
) -> BenchmarkResult {
    let temp_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return BenchmarkResult {
                codec,
                hw_type,
                gst_element,
                stats: None,
                file_size: 0,
                error: Some(format!("Failed to create temp dir: {}", e)),
            };
        }
    };

    let output_path = temp_dir.path().join(format!("bench.{}", codec.container().extension()));

    let config = EncoderConfig {
        target_codec: codec,
        preset_level: DEFAULT_PRESET,
        ..Default::default()
    };

    // Create encoder
    let encoder = match AsyncVideoEncoder::new_with_encoder(
        output_path.clone(),
        WIDTH,
        HEIGHT,
        FPS,
        config,
        60,
        hw_type,
    ) {
        Ok(e) => e,
        Err(e) => {
            return BenchmarkResult {
                codec,
                hw_type,
                gst_element,
                stats: None,
                file_size: 0,
                error: Some(format!("Encoder creation failed: {}", e)),
            };
        }
    };

    // Feed frames for the specified duration
    let frame_duration_ns = (1_000_000_000.0 / FPS) as u64;
    let deadline = Instant::now() + Duration::from_secs(duration_secs);
    let mut frame_index: u32 = 0;

    while Instant::now() < deadline {
        let pts = frame_index as u64 * frame_duration_ns;
        let data = generate_nv12_frame(WIDTH, HEIGHT, frame_index);

        let frame = RawVideoFrame {
            data,
            pts,
            duration: frame_duration_ns,
            width: WIDTH,
            height: HEIGHT,
            format: "NV12".to_string(),
            capture_time: Instant::now(),
        };

        if let Err(e) = encoder.send_frame(frame) {
            return BenchmarkResult {
                codec,
                hw_type,
                gst_element,
                stats: None,
                file_size: 0,
                error: Some(format!("send_frame failed at frame {}: {}", frame_index, e)),
            };
        }

        frame_index += 1;

        if verbose && frame_index % 100 == 0 {
            let now = Instant::now();
            let elapsed = if deadline > now {
                deadline - now
            } else {
                Duration::ZERO
            };
            println!(
                "    {} frames sent, {:.0}s remaining",
                frame_index,
                elapsed.as_secs_f64()
            );
        }
    }

    // Finish encoding
    let stats = match encoder.finish() {
        Ok(s) => s,
        Err(e) => {
            return BenchmarkResult {
                codec,
                hw_type,
                gst_element,
                stats: None,
                file_size: 0,
                error: Some(format!("Encoder finish failed: {}", e)),
            };
        }
    };

    // Read output file size
    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    BenchmarkResult {
        codec,
        hw_type,
        gst_element,
        stats: Some(stats),
        file_size,
        error: None,
    }
}

/// Print a summary table of all benchmark results
fn print_summary(results: &[BenchmarkResult]) {
    println!("\n  ╔══════════╤════════════════════════════╤══════════════════╤════════╤═══════════╤═══════════╤════════════╤═══════════╗");
    println!(
        "  ║ {:<8} │ {:<26} │ {:<16} │ {:>6} │ {:>9} │ {:>9} │ {:>10} │ {:>9} ║",
        "Codec", "Encoder", "GStreamer", "Frames", "FPS", "Realtime", "Size", "Bitrate"
    );
    println!("  ╠══════════╪════════════════════════════╪══════════════════╪════════╪═══════════╪═══════════╪════════════╪═══════════╣");

    for result in results {
        if let Some(ref err) = result.error {
            let err_short = if err.len() > 40 {
                format!("{}...", &err[..37])
            } else {
                err.clone()
            };
            println!(
                "  ║ {:<8} │ {:<26} │ {:<16} │ {:>43} ║",
                result.codec.display_name(),
                result.hw_type.display_name(),
                result.gst_element,
                format!("FAILED: {}", err_short),
            );
        } else if let Some(ref stats) = result.stats {
            let realtime_mult = if stats.encoding_duration.as_secs_f64() > 0.0 {
                stats.content_duration.as_secs_f64() / stats.encoding_duration.as_secs_f64()
            } else {
                0.0
            };
            let bitrate_mbps = if stats.content_duration.as_secs_f64() > 0.0 {
                (result.file_size as f64 * 8.0)
                    / stats.content_duration.as_secs_f64()
                    / 1_000_000.0
            } else {
                0.0
            };
            let size_str = format_size(result.file_size);

            println!(
                "  ║ {:<8} │ {:<26} │ {:<16} │ {:>6} │ {:>7.1}   │ {:>6.2}x   │ {:>10} │ {:>6.1} Mb ║",
                result.codec.display_name(),
                result.hw_type.display_name(),
                result.gst_element,
                stats.frames_encoded,
                stats.average_fps,
                realtime_mult,
                size_str,
                bitrate_mbps,
            );
        }
    }

    println!("  ╚══════════╧════════════════════════════╧══════════════════╧════════╧═══════════╧═══════════╧════════════╧═══════════╝");
    println!();
}

/// Format a file size in human-readable form
fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
