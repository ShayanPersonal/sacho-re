use serde::Deserialize;
use std::path::Path;

// ── Device config types ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct TestDeviceConfigFile {
    #[serde(default)]
    pub midi: Vec<MidiTestDeviceEntry>,
    #[serde(default)]
    pub audio: Vec<AudioTestDeviceEntry>,
    #[serde(default)]
    pub video: Vec<VideoTestDeviceEntry>,
    #[serde(default)]
    pub settings: TestSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MidiTestDeviceEntry {
    pub name_contains: String,
    pub role: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AudioTestDeviceEntry {
    pub name_contains: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoTestDeviceEntry {
    pub name_contains: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestSettings {
    #[serde(default = "default_warmup")]
    pub pipeline_warmup_secs: u64,
    #[serde(default = "default_finalization")]
    pub file_finalization_secs: u64,
    #[serde(default = "default_tolerance")]
    pub duration_tolerance_secs: f64,
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            pipeline_warmup_secs: default_warmup(),
            file_finalization_secs: default_finalization(),
            duration_tolerance_secs: default_tolerance(),
        }
    }
}

fn default_warmup() -> u64 { 3 }
fn default_finalization() -> u64 { 2 }
fn default_tolerance() -> f64 { 2.0 }

// ── Resolved device config ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MidiRole {
    TriggerAndRecord,
    RecordOnly,
}

#[derive(Debug, Clone)]
pub struct MidiTestDevice {
    pub label: String,
    pub name_contains: String,
    pub role: MidiRole,
    pub resolved_id: Option<String>,
    pub resolved_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioTestDevice {
    pub label: String,
    pub name_contains: String,
    pub resolved_id: Option<String>,
    pub resolved_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoTestDevice {
    pub label: String,
    pub name_contains: String,
    pub resolved_id: Option<String>,
    pub resolved_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestDeviceConfig {
    pub midi: Vec<MidiTestDevice>,
    pub audio: Vec<AudioTestDevice>,
    pub video: Vec<VideoTestDevice>,
    pub settings: TestSettings,
}

impl TestDeviceConfig {
    /// Find a MIDI device by label (only if resolved)
    pub fn midi_by_label(&self, label: &str) -> Option<&MidiTestDevice> {
        self.midi.iter().find(|d| d.label == label && d.resolved_id.is_some())
    }

    /// Find the first resolved audio device
    pub fn first_audio(&self) -> Option<&AudioTestDevice> {
        self.audio.iter().find(|d| d.resolved_id.is_some())
    }

    /// Get all resolved video devices
    pub fn resolved_video_devices(&self) -> Vec<&VideoTestDevice> {
        self.video.iter().filter(|d| d.resolved_id.is_some()).collect()
    }

    /// Find a video device by label (only if resolved)
    pub fn video_by_label(&self, label: &str) -> Option<&VideoTestDevice> {
        self.video.iter().find(|d| d.label == label && d.resolved_id.is_some())
    }
}

// ── Loading & resolution ─────────────────────────────────────────────

pub fn load_device_config(dir: &Path) -> TestDeviceConfig {
    let primary = dir.join("test_devices.toml");
    let fallback = dir.join("test_devices.example.toml");

    let path = if primary.exists() {
        println!("  Loading device config from: {}", primary.display());
        primary
    } else if fallback.exists() {
        println!("  No test_devices.toml found, using example: {}", fallback.display());
        fallback
    } else {
        println!("  WARNING: No device config found. No tests will run.");
        return TestDeviceConfig {
            midi: Vec::new(),
            audio: Vec::new(),
            video: Vec::new(),
            settings: TestSettings::default(),
        };
    };

    let contents = std::fs::read_to_string(&path).expect("Failed to read device config");
    let file: TestDeviceConfigFile = toml::from_str(&contents).expect("Failed to parse device config TOML");

    TestDeviceConfig {
        midi: file.midi.into_iter().map(|e| MidiTestDevice {
            label: e.label,
            name_contains: e.name_contains,
            role: match e.role.as_str() {
                "trigger_and_record" => MidiRole::TriggerAndRecord,
                "record_only" => MidiRole::RecordOnly,
                other => {
                    println!("  WARNING: Unknown MIDI role '{}', defaulting to record_only", other);
                    MidiRole::RecordOnly
                }
            },
            resolved_id: None,
            resolved_name: None,
        }).collect(),
        audio: file.audio.into_iter().map(|e| AudioTestDevice {
            label: e.label,
            name_contains: e.name_contains,
            resolved_id: None,
            resolved_name: None,
        }).collect(),
        video: file.video.into_iter().map(|e| VideoTestDevice {
            label: e.label,
            name_contains: e.name_contains,
            resolved_id: None,
            resolved_name: None,
        }).collect(),
        settings: file.settings,
    }
}

/// Resolve declared devices against actual hardware.
/// Fills resolved_id/resolved_name for each device found on the system.
pub fn resolve_devices(config: &mut TestDeviceConfig) {
    // Resolve MIDI devices via midir
    if let Ok(midi_in) = midir::MidiInput::new("sacho-test-discovery") {
        let ports = midi_in.ports();
        for (idx, port) in ports.iter().enumerate() {
            if let Ok(name) = midi_in.port_name(port) {
                for dev in &mut config.midi {
                    if dev.resolved_id.is_none()
                        && name.to_lowercase().contains(&dev.name_contains.to_lowercase())
                    {
                        dev.resolved_id = Some(format!("midi-{}", idx));
                        dev.resolved_name = Some(name.clone());
                    }
                }
            }
        }
    }

    // Resolve audio devices via cpal
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Ok(input_devices) = host.input_devices() {
        for (idx, device) in input_devices.enumerate() {
            if let Ok(name) = device.name() {
                for dev in &mut config.audio {
                    if dev.resolved_id.is_none()
                        && name.to_lowercase().contains(&dev.name_contains.to_lowercase())
                    {
                        dev.resolved_id = Some(format!("audio-{}", idx));
                        dev.resolved_name = Some(name.clone());
                    }
                }
            }
        }
    }

    // Resolve video devices via GStreamer device monitor
    let video_devices = crate::devices::enumeration::enumerate_video_devices();
    for vdev in &video_devices {
        for dev in &mut config.video {
            if dev.resolved_id.is_none()
                && vdev.name.to_lowercase().contains(&dev.name_contains.to_lowercase())
            {
                dev.resolved_id = Some(vdev.id.clone());
                dev.resolved_name = Some(vdev.name.clone());
            }
        }
    }
}

/// Print a table of discovered vs. declared devices.
pub fn print_inventory(config: &TestDeviceConfig) {
    println!("\n  --- Device Inventory ---");

    println!("  MIDI:");
    for dev in &config.midi {
        let status = match &dev.resolved_name {
            Some(name) => format!("OK  -> {} ({})", name, dev.resolved_id.as_deref().unwrap_or("?")),
            None => "MISSING".to_string(),
        };
        let role_str = match dev.role {
            MidiRole::TriggerAndRecord => "trigger+record",
            MidiRole::RecordOnly => "record_only",
        };
        println!("    [{}] {} ({}): {}", dev.label, dev.name_contains, role_str, status);
    }

    println!("  Audio:");
    for dev in &config.audio {
        let status = match &dev.resolved_name {
            Some(name) => format!("OK  -> {} ({})", name, dev.resolved_id.as_deref().unwrap_or("?")),
            None => "MISSING".to_string(),
        };
        println!("    [{}] {}: {}", dev.label, dev.name_contains, status);
    }

    println!("  Video:");
    for dev in &config.video {
        let status = match &dev.resolved_name {
            Some(name) => format!("OK  -> {} ({})", name, dev.resolved_id.as_deref().unwrap_or("?")),
            None => "MISSING".to_string(),
        };
        println!("    [{}] {}: {}", dev.label, dev.name_contains, status);
    }

    println!();
}
