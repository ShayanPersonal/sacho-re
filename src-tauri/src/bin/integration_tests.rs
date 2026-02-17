//! Sacho Integration Test Runner
//!
//! Standalone binary that builds a headless Tauri app, discovers hardware,
//! runs test permutations sequentially, validates output files, and reports results.
//!
//! Usage:
//!   cargo run --bin integration_tests [-- [OPTIONS]]
//!
//! Options:
//!   --filter <pattern>    Run only tests whose name contains <pattern>
//!   --verbose             Extra debug output
//!   --keep-sessions       Don't clean up temp dirs (for debugging)
//!   --list                List all tests without running them

use sacho_lib::gstreamer_init;
use sacho_lib::test_harness::{discovery, permutations, runner};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let verbose = args.iter().any(|a| a == "--verbose");
    let keep_sessions = args.iter().any(|a| a == "--keep-sessions");
    let list_only = args.iter().any(|a| a == "--list");

    let filter = args.iter()
        .position(|a| a == "--filter")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // Init logging
    let log_level = if verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_level)
    ).init();

    // On Windows, attach to parent console for output
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    println!("\n=== Sacho Integration Tests ===\n");

    // Init GStreamer
    gstreamer_init::init_gstreamer_env();

    // Discover hardware
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut device_config = discovery::load_device_config(&crate_dir);
    discovery::resolve_devices(&mut device_config);
    discovery::print_inventory(&device_config);

    // Build test matrix
    let mut tests = permutations::build_test_matrix(&device_config);

    // Apply filter
    if let Some(ref pattern) = filter {
        tests.retain(|t| t.name.contains(pattern.as_str()));
        println!("  Filter '{}': {} tests match\n", pattern, tests.len());
    }

    if tests.is_empty() {
        println!("  No tests to run. Check device config and hardware.");
        std::process::exit(0);
    }

    // List mode
    if list_only {
        println!("  Tests ({}):", tests.len());
        for (i, test) in tests.iter().enumerate() {
            let trigger = match &test.trigger {
                runner::TriggerMode::Midi { .. } => "midi",
                runner::TriggerMode::Manual => "manual",
            };
            println!(
                "  [{}/{}] {} (trigger={}, pre_roll={}s, idle={}s, play={}s)",
                i + 1,
                tests.len(),
                test.name,
                trigger,
                test.config.pre_roll_secs,
                test.config.idle_timeout_secs,
                test.play_duration_secs,
            );
        }
        std::process::exit(0);
    }

    // Run tests sequentially
    println!("  Running {} tests...\n", tests.len());

    let mut results = Vec::new();

    for (i, test) in tests.iter().enumerate() {
        println!(
            "  [{}/{}] {} ...",
            i + 1,
            tests.len(),
            test.name,
        );

        let result = runner::run_test(test, keep_sessions);

        let status = if result.passed { "PASS" } else { "FAIL" };
        println!(
            "  [{}/{}] {} {} ({:.1}s)",
            i + 1,
            tests.len(),
            test.name,
            status,
            result.duration_ms as f64 / 1000.0,
        );

        if !result.errors.is_empty() {
            for err in &result.errors {
                println!("         -> {}", err);
            }
        }

        results.push(result);
    }

    // Print summary
    runner::print_summary(&results);

    // Exit code
    let any_failed = results.iter().any(|r| !r.passed);
    std::process::exit(if any_failed { 1 } else { 0 });
}
