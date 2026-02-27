//! Sacho Encoder Benchmark
//!
//! Benchmarks all available video encoders using the app's actual encoding
//! infrastructure (same codecs, presets, pipeline construction, hardware detection).
//!
//! Each encoder runs in a subprocess for isolation — if a GStreamer plugin crashes
//! (e.g. buggy driver), the benchmark reports the failure and continues.
//!
//! Usage:
//!   cargo run -p sacho-tools --bin encoder_benchmark [-- [OPTIONS]]
//!
//! Options:
//!   --codec <filter>    Only benchmark encoders whose codec name contains <filter>
//!   --duration <secs>   Duration per benchmark run (default: 15s)
//!   --preset <1-5>      Use a specific preset level (default: 3 = Balanced)
//!   --all-presets        Test every encoder at all preset levels (1-5)
//!   --verbose           Extra debug output
//!   --help              Show this help message

use std::process::Command;
use std::time::{Duration, Instant};

use sacho_lib::encoding::{
    available_encoders_for_codec, AsyncVideoEncoder, EncoderConfig, HardwareEncoderType,
    RawVideoFrame, VideoCodec,
};
use sacho_lib::encoding::presets::{preset_label, DEFAULT_PRESET, MAX_PRESET, MIN_PRESET};
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

/// Marker prefix for structured result output from worker subprocess
const RESULT_OK: &str = "BENCH_OK,";
const RESULT_ERR: &str = "BENCH_ERR,";

// ═══════════════════════════════════════════════════════════════════════════════
// Codec/HwType string conversion for subprocess CLI args
// ═══════════════════════════════════════════════════════════════════════════════

fn codec_to_arg(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::Av1 => "av1",
        VideoCodec::Vp9 => "vp9",
        VideoCodec::Vp8 => "vp8",
        VideoCodec::H264 => "h264",
        VideoCodec::Ffv1 => "ffv1",
        _ => "unknown",
    }
}

fn codec_from_arg(s: &str) -> Option<VideoCodec> {
    match s {
        "av1" => Some(VideoCodec::Av1),
        "vp9" => Some(VideoCodec::Vp9),
        "vp8" => Some(VideoCodec::Vp8),
        "h264" => Some(VideoCodec::H264),
        "ffv1" => Some(VideoCodec::Ffv1),
        _ => None,
    }
}

fn hw_type_to_arg(hw: HardwareEncoderType) -> &'static str {
    match hw {
        HardwareEncoderType::Nvenc => "nvenc",
        HardwareEncoderType::Amf => "amf",
        HardwareEncoderType::Qsv => "qsv",
        HardwareEncoderType::VaApi => "vaapi",
        HardwareEncoderType::MediaFoundation => "mediafoundation",
        HardwareEncoderType::VideoToolbox => "videotoolbox",
        HardwareEncoderType::Software => "software",
    }
}

fn hw_type_from_arg(s: &str) -> Option<HardwareEncoderType> {
    match s {
        "nvenc" => Some(HardwareEncoderType::Nvenc),
        "amf" => Some(HardwareEncoderType::Amf),
        "qsv" => Some(HardwareEncoderType::Qsv),
        "vaapi" => Some(HardwareEncoderType::VaApi),
        "mediafoundation" => Some(HardwareEncoderType::MediaFoundation),
        "videotoolbox" => Some(HardwareEncoderType::VideoToolbox),
        "software" => Some(HardwareEncoderType::Software),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Result type for orchestrator
// ═══════════════════════════════════════════════════════════════════════════════

struct BenchmarkResult {
    codec_name: &'static str,
    hw_name: &'static str,
    gst_element: &'static str,
    preset: u8,
    frames: u64,
    avg_fps: f64,
    realtime_mult: f64,
    file_size: u64,
    bitrate_mbps: f64,
    error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Help
// ═══════════════════════════════════════════════════════════════════════════════

fn print_help() {
    println!(
        "\n\
Sacho Encoder Benchmark

Benchmarks all available video encoders using the app's actual encoding
infrastructure (same codecs, presets, pipeline construction, hardware detection).

USAGE:
    cargo run -p sacho-tools --bin encoder_benchmark [-- [OPTIONS]]

OPTIONS:
    --codec <filter>    Only benchmark encoders whose codec name contains <filter>
    --duration <secs>   Duration per benchmark run (default: {DEFAULT_DURATION_SECS}s)
    --preset <1-5>      Use a specific preset level (default: {DEFAULT_PRESET} = {default_label})
    --all-presets       Test every encoder at all preset levels ({MIN_PRESET}-{MAX_PRESET})
    --verbose           Extra debug output
    --help              Show this help message

PRESET LEVELS:
    1  Lightest   — Minimal CPU/GPU load, lowest quality
    2  Light      — Low resource usage, acceptable quality
    3  Balanced   — Moderate resources, good quality (default)
    4  Quality    — Higher resource usage, very good quality
    5  Maximum    — Highest quality feasible in real-time

EXAMPLES:
    cargo run -p sacho-tools --bin encoder_benchmark
    cargo run -p sacho-tools --bin encoder_benchmark -- --preset 5
    cargo run -p sacho-tools --bin encoder_benchmark -- --all-presets --duration 10
    cargo run -p sacho-tools --bin encoder_benchmark -- --codec h264 --preset 1
",
        default_label = preset_label(DEFAULT_PRESET),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entry point — dispatch to orchestrator or worker
// ═══════════════════════════════════════════════════════════════════════════════

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // On Windows, attach to parent console for output
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    if args.iter().any(|a| a == "--run-single") {
        worker_main(&args);
    } else {
        orchestrator_main(&args);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Argument parsing helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_arg_value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<T>().ok())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Orchestrator — discovers encoders, spawns worker subprocesses, collects results
// ═══════════════════════════════════════════════════════════════════════════════

fn orchestrator_main(args: &[String]) {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        std::process::exit(0);
    }

    let verbose = args.iter().any(|a| a == "--verbose");
    let all_presets = args.iter().any(|a| a == "--all-presets");
    let duration_secs = parse_arg_value::<u64>(args, "--duration").unwrap_or(DEFAULT_DURATION_SECS);
    let codec_filter = parse_arg_value::<String>(args, "--codec");

    let preset_levels: Vec<u8> = if all_presets {
        (MIN_PRESET..=MAX_PRESET).collect()
    } else {
        let level = parse_arg_value::<u8>(args, "--preset")
            .unwrap_or(DEFAULT_PRESET)
            .clamp(MIN_PRESET, MAX_PRESET);
        vec![level]
    };

    // Init logging (suppress for orchestrator unless verbose)
    let log_level = if verbose { "debug" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    println!("\n=== Sacho Encoder Benchmark ===\n");
    println!("  Resolution: {}x{} @ {:.0} fps", WIDTH, HEIGHT, FPS);
    println!("  Duration:   {}s per encoder", duration_secs);
    if all_presets {
        println!("  Presets:    all ({}-{})", MIN_PRESET, MAX_PRESET);
    } else {
        println!(
            "  Preset:     {} (level {})",
            preset_label(preset_levels[0]),
            preset_levels[0],
        );
    }
    if let Some(ref filter) = codec_filter {
        println!("  Filter:     codec contains '{}'", filter);
    }
    println!();

    // Init GStreamer for encoder discovery
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

    let total_runs = encoders.len() * preset_levels.len();
    println!("\n  Running {} benchmarks...\n", total_runs);

    let self_exe = std::env::current_exe().expect("Failed to get current executable path");
    let mut results: Vec<BenchmarkResult> = Vec::new();
    let mut run_idx = 0;

    for &(codec, hw_type, element) in &encoders {
        for &preset in &preset_levels {
            run_idx += 1;
            let preset_info = if all_presets {
                format!(" [preset {}]", preset)
            } else {
                String::new()
            };
            println!(
                "  [{}/{}] {} / {} ({}){}...",
                run_idx,
                total_runs,
                codec.display_name(),
                hw_type.display_name(),
                element,
                preset_info,
            );

            let result = run_in_subprocess(
                &self_exe,
                codec,
                hw_type,
                element,
                duration_secs,
                preset,
                verbose,
            );

            match &result.error {
                Some(err) => {
                    println!(
                        "  [{}/{}] FAILED: {}\n",
                        run_idx, total_runs, err,
                    );
                }
                None => {
                    println!(
                        "  [{}/{}] {} frames, {:.1} fps, {:.2}x realtime, {:.1} MB, {:.1} Mbps\n",
                        run_idx,
                        total_runs,
                        result.frames,
                        result.avg_fps,
                        result.realtime_mult,
                        result.file_size as f64 / 1_048_576.0,
                        result.bitrate_mbps,
                    );
                }
            }

            results.push(result);
        }
    }

    print_summary(&results, all_presets || preset_levels[0] != DEFAULT_PRESET);
}

/// Spawn a worker subprocess for a single encoder benchmark
fn run_in_subprocess(
    self_exe: &std::path::Path,
    codec: VideoCodec,
    hw_type: HardwareEncoderType,
    element: &'static str,
    duration_secs: u64,
    preset: u8,
    verbose: bool,
) -> BenchmarkResult {
    let mut cmd = Command::new(self_exe);
    cmd.args([
        "--run-single",
        codec_to_arg(codec),
        hw_type_to_arg(hw_type),
        element,
        "--duration",
        &duration_secs.to_string(),
        "--preset",
        &preset.to_string(),
    ]);
    if verbose {
        cmd.arg("--verbose");
    }

    let make_error = |msg: String| BenchmarkResult {
        codec_name: codec.display_name(),
        hw_name: hw_type.display_name(),
        gst_element: element,
        preset,
        frames: 0,
        avg_fps: 0.0,
        realtime_mult: 0.0,
        file_size: 0,
        bitrate_mbps: 0.0,
        error: Some(msg),
    };

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return make_error(format!("Failed to spawn worker: {}", e)),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    // In verbose mode, print worker output (excluding result markers)
    if verbose {
        for line in stdout.lines() {
            if !line.starts_with(RESULT_OK) && !line.starts_with(RESULT_ERR) {
                println!("    {}", line);
            }
        }
    }

    // Check for crash (non-zero exit without our error marker)
    if !output.status.success() {
        // Check if worker reported an error before crashing
        if let Some(err_line) = stdout.lines().find(|l| l.starts_with(RESULT_ERR)) {
            return make_error(err_line[RESULT_ERR.len()..].to_string());
        }
        let code = output
            .status
            .code()
            .map(|c| format!("exit code {:#x}", c))
            .unwrap_or_else(|| "killed by signal".to_string());
        return make_error(format!("Encoder crashed ({})", code));
    }

    // Parse BENCH_OK line
    if let Some(ok_line) = stdout.lines().find(|l| l.starts_with(RESULT_OK)) {
        let fields: Vec<&str> = ok_line[RESULT_OK.len()..].split(',').collect();
        if fields.len() == 6 {
            let frames = fields[0].parse::<u64>().unwrap_or(0);
            let avg_fps = fields[1].parse::<f64>().unwrap_or(0.0);
            let realtime_mult = fields[2].parse::<f64>().unwrap_or(0.0);
            let file_size = fields[3].parse::<u64>().unwrap_or(0);
            let bitrate_mbps = fields[4].parse::<f64>().unwrap_or(0.0);
            let _enc_duration_ms = fields[5].parse::<u64>().unwrap_or(0);
            return BenchmarkResult {
                codec_name: codec.display_name(),
                hw_name: hw_type.display_name(),
                gst_element: element,
                preset,
                frames,
                avg_fps,
                realtime_mult,
                file_size,
                bitrate_mbps,
                error: None,
            };
        }
    }

    // Check for explicit error
    if let Some(err_line) = stdout.lines().find(|l| l.starts_with(RESULT_ERR)) {
        return make_error(err_line[RESULT_ERR.len()..].to_string());
    }

    make_error("Worker produced no result".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Worker — runs a single encoder benchmark in an isolated process
// ═══════════════════════════════════════════════════════════════════════════════

fn worker_main(args: &[String]) {
    let run_single_pos = args.iter().position(|a| a == "--run-single").unwrap();
    let codec_str = args.get(run_single_pos + 1).expect("missing codec arg");
    let hw_str = args.get(run_single_pos + 2).expect("missing hw_type arg");
    let _element_str = args.get(run_single_pos + 3).expect("missing element arg");

    let verbose = args.iter().any(|a| a == "--verbose");
    let duration_secs = parse_arg_value::<u64>(args, "--duration").unwrap_or(DEFAULT_DURATION_SECS);
    let preset = parse_arg_value::<u8>(args, "--preset")
        .unwrap_or(DEFAULT_PRESET)
        .clamp(MIN_PRESET, MAX_PRESET);

    let codec = codec_from_arg(codec_str).unwrap_or_else(|| {
        println!("{}Unknown codec: {}", RESULT_ERR, codec_str);
        std::process::exit(1);
    });
    let hw_type = hw_type_from_arg(hw_str).unwrap_or_else(|| {
        println!("{}Unknown hw_type: {}", RESULT_ERR, hw_str);
        std::process::exit(1);
    });

    // Init logging
    let log_level = if verbose { "debug" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    // Init GStreamer
    gstreamer_init::init_gstreamer_env();

    // Run the benchmark
    let temp_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            println!("{}Failed to create temp dir: {}", RESULT_ERR, e);
            std::process::exit(1);
        }
    };

    // Always encode to MKV (encoder always produces MKV)
    let output_path = temp_dir.path().join("bench.mkv");

    let config = EncoderConfig {
        target_codec: codec,
        preset_level: preset,
        ..Default::default()
    };

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
            println!("{}Encoder creation failed: {}", RESULT_ERR, e);
            std::process::exit(1);
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
            println!(
                "{}send_frame failed at frame {}: {}",
                RESULT_ERR, frame_index, e
            );
            std::process::exit(1);
        }

        frame_index += 1;

        if verbose && frame_index % 100 == 0 {
            let now = Instant::now();
            let remaining = if deadline > now {
                deadline - now
            } else {
                Duration::ZERO
            };
            eprintln!(
                "    {} frames sent, {:.0}s remaining",
                frame_index,
                remaining.as_secs_f64()
            );
        }
    }

    // Finish encoding
    let stats = match encoder.finish() {
        Ok(s) => s,
        Err(e) => {
            println!("{}Encoder finish failed: {}", RESULT_ERR, e);
            std::process::exit(1);
        }
    };

    // Read output file size
    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let realtime_mult = if stats.encoding_duration.as_secs_f64() > 0.0 {
        stats.content_duration.as_secs_f64() / stats.encoding_duration.as_secs_f64()
    } else {
        0.0
    };
    let bitrate_mbps = if stats.content_duration.as_secs_f64() > 0.0 {
        (file_size as f64 * 8.0) / stats.content_duration.as_secs_f64() / 1_000_000.0
    } else {
        0.0
    };

    // Output structured result for orchestrator to parse
    println!(
        "{}{},{:.2},{:.4},{},{:.2},{}",
        RESULT_OK,
        stats.frames_encoded,
        stats.average_fps,
        realtime_mult,
        file_size,
        bitrate_mbps,
        stats.encoding_duration.as_millis(),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Frame generation
// ═══════════════════════════════════════════════════════════════════════════════

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
            let horiz = col as f64 / w as f64 * 200.0;
            let vert = row as f64 / h as f64 * 55.0;
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
            let u =
                (128.0 + (col as f64 / w as f64 * 40.0) + uv_phase).clamp(0.0, 255.0) as u8;
            let v =
                (128.0 + (row as f64 / uv_h as f64 * 40.0) - uv_phase).clamp(0.0, 255.0) as u8;
            data[uv_offset + row * w + col] = u;
            data[uv_offset + row * w + col + 1] = v;
        }
    }

    data
}

// ═══════════════════════════════════════════════════════════════════════════════
// Summary table
// ═══════════════════════════════════════════════════════════════════════════════

fn print_summary(results: &[BenchmarkResult], show_preset: bool) {
    if show_preset {
        println!("\n  ╔══════════╤════════════════════════════╤══════════════════╤════════════╤════════╤═══════════╤═══════════╤════════════╤═══════════╗");
        println!(
            "  ║ {:<8} │ {:<26} │ {:<16} │ {:<10} │ {:>6} │ {:>9} │ {:>9} │ {:>10} │ {:>9} ║",
            "Codec", "Encoder", "GStreamer", "Preset", "Frames", "FPS", "Realtime", "Size", "Bitrate"
        );
        println!("  ╠══════════╪════════════════════════════╪══════════════════╪════════════╪════════╪═══════════╪═══════════╪════════════╪═══════════╣");

        for r in results {
            let preset_str = format!("{} ({})", preset_label(r.preset), r.preset);
            if let Some(ref err) = r.error {
                let err_short = if err.len() > 40 {
                    format!("{}...", &err[..37])
                } else {
                    err.clone()
                };
                println!(
                    "  ║ {:<8} │ {:<26} │ {:<16} │ {:<10} │ {:>54} ║",
                    r.codec_name,
                    r.hw_name,
                    r.gst_element,
                    preset_str,
                    format!("FAILED: {}", err_short),
                );
            } else {
                let size_str = format_size(r.file_size);
                println!(
                    "  ║ {:<8} │ {:<26} │ {:<16} │ {:<10} │ {:>6} │ {:>7.1}   │ {:>6.2}x   │ {:>10} │ {:>6.1} Mb ║",
                    r.codec_name,
                    r.hw_name,
                    r.gst_element,
                    preset_str,
                    r.frames,
                    r.avg_fps,
                    r.realtime_mult,
                    size_str,
                    r.bitrate_mbps,
                );
            }
        }

        println!("  ╚══════════╧════════════════════════════╧══════════════════╧════════════╧════════╧═══════════╧═══════════╧════════════╧═══════════╝");
    } else {
        println!("\n  ╔══════════╤════════════════════════════╤══════════════════╤════════╤═══════════╤═══════════╤════════════╤═══════════╗");
        println!(
            "  ║ {:<8} │ {:<26} │ {:<16} │ {:>6} │ {:>9} │ {:>9} │ {:>10} │ {:>9} ║",
            "Codec", "Encoder", "GStreamer", "Frames", "FPS", "Realtime", "Size", "Bitrate"
        );
        println!("  ╠══════════╪════════════════════════════╪══════════════════╪════════╪═══════════╪═══════════╪════════════╪═══════════╣");

        for r in results {
            if let Some(ref err) = r.error {
                let err_short = if err.len() > 40 {
                    format!("{}...", &err[..37])
                } else {
                    err.clone()
                };
                println!(
                    "  ║ {:<8} │ {:<26} │ {:<16} │ {:>43} ║",
                    r.codec_name,
                    r.hw_name,
                    r.gst_element,
                    format!("FAILED: {}", err_short),
                );
            } else {
                let size_str = format_size(r.file_size);
                println!(
                    "  ║ {:<8} │ {:<26} │ {:<16} │ {:>6} │ {:>7.1}   │ {:>6.2}x   │ {:>10} │ {:>6.1} Mb ║",
                    r.codec_name,
                    r.hw_name,
                    r.gst_element,
                    r.frames,
                    r.avg_fps,
                    r.realtime_mult,
                    size_str,
                    r.bitrate_mbps,
                );
            }
        }

        println!("  ╚══════════╧════════════════════════════╧══════════════════╧════════╧═══════════╧═══════════╧════════════╧═══════════╝");
    }
    println!();
}

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
