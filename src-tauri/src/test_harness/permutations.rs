use crate::config::{AudioFormat, Config};
use crate::devices::enumeration::enumerate_video_devices;

use super::discovery::TestDeviceConfig;
use super::runner::{Expected, TestCase, TriggerMode};

/// Build the full test matrix from discovered devices.
/// Tests referencing unresolved devices are automatically skipped.
pub fn build_test_matrix(devices: &TestDeviceConfig) -> Vec<TestCase> {
    let mut tests = Vec::new();

    let loopbe = devices.midi_by_label("loopbe1");
    let first_audio = devices.first_audio();
    let video_devices = devices.resolved_video_devices();

    // ── MIDI-only tests ──────────────────────────────────────────────

    if let Some(midi) = loopbe {
        let midi_name = midi.name_contains.clone();
        let midi_id = midi.resolved_id.clone().unwrap();

        // midi_only_2s_3s: minimal MIDI test
        tests.push(make_midi_only(
            "midi_only_2s_3s",
            &midi_name,
            &midi_id,
            2, 3,
            3,
            vec![60, 64, 67],
            &devices.settings,
        ));

        // midi_only_8s_5s: long pre-roll
        tests.push(make_midi_only(
            "midi_only_8s_5s",
            &midi_name,
            &midi_id,
            8, 5,
            3,
            vec![48, 55, 62, 69],
            &devices.settings,
        ));

        // ── MIDI + Audio tests ───────────────────────────────────────

        if let Some(audio) = first_audio {
            let audio_id = audio.resolved_id.clone().unwrap();

            // WAV format
            tests.push(make_midi_audio(
                "midi_audio_wav_2s_5s",
                &midi_name, &midi_id,
                &audio_id,
                2, 5,
                3,
                AudioFormat::Wav,
                vec![60, 64, 67],
                &devices.settings,
            ));

            // FLAC format
            tests.push(make_midi_audio(
                "midi_audio_flac_2s_5s",
                &midi_name, &midi_id,
                &audio_id,
                2, 5,
                3,
                AudioFormat::Flac,
                vec![60, 64, 67],
                &devices.settings,
            ));

            // double_trigger: note during recording
            tests.push(make_midi_audio(
                "double_trigger",
                &midi_name, &midi_id,
                &audio_id,
                2, 5,
                3,
                AudioFormat::Wav,
                vec![60, 64, 67, 72],
                &devices.settings,
            ));

            // rapid_stop_restart: quick re-trigger (short idle timeout)
            tests.push(make_midi_audio(
                "rapid_stop_restart",
                &midi_name, &midi_id,
                &audio_id,
                2, 3,
                3,
                AudioFormat::Wav,
                vec![60, 64],
                &devices.settings,
            ));
        }

        // ── MIDI + Video tests (one per video device) ────────────────

        for vdev in &video_devices {
            let vdev_id = vdev.resolved_id.clone().unwrap();
            let vlabel = &vdev.label;

            tests.push(make_midi_video(
                &format!("midi_video_{}_2s_5s", vlabel),
                &midi_name, &midi_id,
                &vdev_id,
                2, 5,
                3,
                vec![60, 64, 67],
                &devices.settings,
            ));
        }

        // ── Full pipeline tests (MIDI + Audio + Video) ────────────────

        if let Some(audio) = first_audio {
            let audio_id = audio.resolved_id.clone().unwrap();

            for vdev in &video_devices {
                let vdev_id = vdev.resolved_id.clone().unwrap();
                let vlabel = &vdev.label;

                // Short full test
                tests.push(make_full(
                    &format!("full_{}_wav_2s_3s", vlabel),
                    &midi_name, &midi_id,
                    &audio_id, &vdev_id,
                    2, 3,
                    3,
                    AudioFormat::Wav,
                    vec![60, 64, 67],
                    &devices.settings,
                ));

                // User's example case: 8s pre-roll, 5s idle
                tests.push(make_full(
                    &format!("full_{}_wav_8s_5s", vlabel),
                    &midi_name, &midi_id,
                    &audio_id, &vdev_id,
                    8, 5,
                    3,
                    AudioFormat::Wav,
                    vec![60, 64, 67, 72],
                    &devices.settings,
                ));

                // FLAC + video
                tests.push(make_full(
                    &format!("full_{}_flac_2s_5s", vlabel),
                    &midi_name, &midi_id,
                    &audio_id, &vdev_id,
                    2, 5,
                    3,
                    AudioFormat::Flac,
                    vec![60, 64, 67],
                    &devices.settings,
                ));

                // Min pre-roll
                tests.push(make_full(
                    &format!("full_{}_wav_1s_5s", vlabel),
                    &midi_name, &midi_id,
                    &audio_id, &vdev_id,
                    1, 5,
                    3,
                    AudioFormat::Wav,
                    vec![60, 64, 67],
                    &devices.settings,
                ));
            }
        }
    }

    // ── Manual tests (no MIDI trigger) ────────────────────────────────

    if let Some(audio) = first_audio {
        let audio_id = audio.resolved_id.clone().unwrap();

        tests.push(make_manual_audio(
            "manual_audio_only",
            &audio_id,
            5,
            AudioFormat::Wav,
            &devices.settings,
        ));

        for vdev in &video_devices {
            let vdev_id = vdev.resolved_id.clone().unwrap();
            let vlabel = &vdev.label;

            tests.push(make_manual_full(
                &format!("manual_full_{}", vlabel),
                &audio_id, &vdev_id,
                5,
                AudioFormat::Wav,
                &devices.settings,
            ));
        }
    }

    tests
}

// ── Helper builders ──────────────────────────────────────────────────

fn base_config(pre_roll: u32, idle_timeout: u32) -> Config {
    let mut config = Config::default();
    config.pre_roll_secs = pre_roll;
    config.idle_timeout_secs = idle_timeout;
    config
}

fn make_midi_only(
    name: &str,
    midi_name_contains: &str,
    midi_id: &str,
    pre_roll: u32,
    idle_timeout: u32,
    play_secs: u32,
    notes: Vec<u8>,
    settings: &super::discovery::TestSettings,
) -> TestCase {
    let mut config = base_config(pre_roll, idle_timeout);
    config.trigger_midi_devices = vec![midi_id.to_string()];
    config.selected_midi_devices = vec![midi_id.to_string()];

    let expected_duration = pre_roll as f64 + play_secs as f64 + idle_timeout as f64;

    TestCase {
        name: name.to_string(),
        config,
        trigger: TriggerMode::Midi {
            loopback_name_contains: midi_name_contains.to_string(),
        },
        play_duration_secs: play_secs,
        notes_to_send: notes,
        expected: Expected {
            has_midi: true,
            has_audio: false,
            has_video: false,
            duration_secs: expected_duration,
            duration_tolerance: settings.duration_tolerance_secs,
            audio_format: None,
            video_codec: None,
            video_resolution: None,
        },
        settings: settings.clone(),
    }
}

fn make_midi_audio(
    name: &str,
    midi_name_contains: &str,
    midi_id: &str,
    audio_id: &str,
    pre_roll: u32,
    idle_timeout: u32,
    play_secs: u32,
    audio_format: AudioFormat,
    notes: Vec<u8>,
    settings: &super::discovery::TestSettings,
) -> TestCase {
    let mut config = base_config(pre_roll, idle_timeout);
    config.trigger_midi_devices = vec![midi_id.to_string()];
    config.selected_midi_devices = vec![midi_id.to_string()];
    config.selected_audio_devices = vec![audio_id.to_string()];
    config.audio_format = audio_format.clone();

    let expected_duration = pre_roll as f64 + play_secs as f64 + idle_timeout as f64;

    TestCase {
        name: name.to_string(),
        config,
        trigger: TriggerMode::Midi {
            loopback_name_contains: midi_name_contains.to_string(),
        },
        play_duration_secs: play_secs,
        notes_to_send: notes,
        expected: Expected {
            has_midi: true,
            has_audio: true,
            has_video: false,
            duration_secs: expected_duration,
            duration_tolerance: settings.duration_tolerance_secs,
            audio_format: Some(audio_format),
            video_codec: None,
            video_resolution: None,
        },
        settings: settings.clone(),
    }
}

fn make_midi_video(
    name: &str,
    midi_name_contains: &str,
    midi_id: &str,
    video_id: &str,
    pre_roll: u32,
    idle_timeout: u32,
    play_secs: u32,
    notes: Vec<u8>,
    settings: &super::discovery::TestSettings,
) -> TestCase {
    let mut config = base_config(pre_roll, idle_timeout);
    config.trigger_midi_devices = vec![midi_id.to_string()];
    config.selected_midi_devices = vec![midi_id.to_string()];
    config.selected_video_devices = vec![video_id.to_string()];

    // Populate video device config from discovered device defaults
    populate_video_config(&mut config, video_id);

    let expected_duration = pre_roll as f64 + play_secs as f64 + idle_timeout as f64;

    TestCase {
        name: name.to_string(),
        config,
        trigger: TriggerMode::Midi {
            loopback_name_contains: midi_name_contains.to_string(),
        },
        play_duration_secs: play_secs,
        notes_to_send: notes,
        expected: Expected {
            has_midi: true,
            has_audio: false,
            has_video: true,
            duration_secs: expected_duration,
            duration_tolerance: settings.duration_tolerance_secs,
            audio_format: None,
            video_codec: None,
            video_resolution: None,
        },
        settings: settings.clone(),
    }
}

fn make_full(
    name: &str,
    midi_name_contains: &str,
    midi_id: &str,
    audio_id: &str,
    video_id: &str,
    pre_roll: u32,
    idle_timeout: u32,
    play_secs: u32,
    audio_format: AudioFormat,
    notes: Vec<u8>,
    settings: &super::discovery::TestSettings,
) -> TestCase {
    let mut config = base_config(pre_roll, idle_timeout);
    config.trigger_midi_devices = vec![midi_id.to_string()];
    config.selected_midi_devices = vec![midi_id.to_string()];
    config.selected_audio_devices = vec![audio_id.to_string()];
    config.selected_video_devices = vec![video_id.to_string()];
    config.audio_format = audio_format.clone();

    populate_video_config(&mut config, video_id);

    let expected_duration = pre_roll as f64 + play_secs as f64 + idle_timeout as f64;

    TestCase {
        name: name.to_string(),
        config,
        trigger: TriggerMode::Midi {
            loopback_name_contains: midi_name_contains.to_string(),
        },
        play_duration_secs: play_secs,
        notes_to_send: notes,
        expected: Expected {
            has_midi: true,
            has_audio: true,
            has_video: true,
            duration_secs: expected_duration,
            duration_tolerance: settings.duration_tolerance_secs,
            audio_format: Some(audio_format),
            video_codec: None,
            video_resolution: None,
        },
        settings: settings.clone(),
    }
}

fn make_manual_audio(
    name: &str,
    audio_id: &str,
    play_secs: u32,
    audio_format: AudioFormat,
    settings: &super::discovery::TestSettings,
) -> TestCase {
    let mut config = base_config(2, 5);
    config.selected_audio_devices = vec![audio_id.to_string()];
    config.audio_format = audio_format.clone();

    // Manual tests: duration = play time only (no idle timeout wait)
    let expected_duration = play_secs as f64;

    TestCase {
        name: name.to_string(),
        config,
        trigger: TriggerMode::Manual,
        play_duration_secs: play_secs,
        notes_to_send: Vec::new(),
        expected: Expected {
            has_midi: false,
            has_audio: true,
            has_video: false,
            duration_secs: expected_duration,
            duration_tolerance: settings.duration_tolerance_secs,
            audio_format: Some(audio_format),
            video_codec: None,
            video_resolution: None,
        },
        settings: settings.clone(),
    }
}

fn make_manual_full(
    name: &str,
    audio_id: &str,
    video_id: &str,
    play_secs: u32,
    audio_format: AudioFormat,
    settings: &super::discovery::TestSettings,
) -> TestCase {
    let mut config = base_config(2, 5);
    config.selected_audio_devices = vec![audio_id.to_string()];
    config.selected_video_devices = vec![video_id.to_string()];
    config.audio_format = audio_format.clone();

    populate_video_config(&mut config, video_id);

    let expected_duration = play_secs as f64;

    TestCase {
        name: name.to_string(),
        config,
        trigger: TriggerMode::Manual,
        play_duration_secs: play_secs,
        notes_to_send: Vec::new(),
        expected: Expected {
            has_midi: false,
            has_audio: true,
            has_video: true,
            duration_secs: expected_duration,
            duration_tolerance: settings.duration_tolerance_secs,
            audio_format: Some(audio_format),
            video_codec: None,
            video_resolution: None,
        },
        settings: settings.clone(),
    }
}

/// Look up a video device by ID from GStreamer enumeration and populate
/// the config's video_device_configs with its default_config().
fn populate_video_config(config: &mut Config, video_id: &str) {
    let video_devices = enumerate_video_devices();
    if let Some(vdev) = video_devices.iter().find(|d| d.id == video_id) {
        if let Some(default_cfg) = vdev.default_config() {
            config.video_device_configs.insert(video_id.to_string(), default_cfg);
        }
    }
}
