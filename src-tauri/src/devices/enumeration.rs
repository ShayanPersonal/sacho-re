// Device enumeration implementations

use super::{AudioDevice, MidiDevice, VideoDevice, Resolution};
use crate::encoding::{VideoCodec, has_av1_encoder, has_vp9_encoder, has_vp8_encoder};
use cpal::traits::{DeviceTrait, HostTrait};
use gstreamer as gst;
use gstreamer::prelude::*;

/// Process GStreamer caps to extract codecs and resolutions
fn process_caps(
    caps: &gst::Caps,
    detected_formats: &mut Vec<String>,
    supported_codecs: &mut Vec<VideoCodec>,
    resolutions: &mut Vec<Resolution>,
    can_encode_raw: bool,
) {
    for i in 0..caps.size() {
        if let Some(structure) = caps.structure(i) {
            let format_name = structure.name().as_str();
            
            // Track unique formats (use short names for display)
            let display_name = format_display_name(format_name);
            if !detected_formats.contains(&display_name) {
                detected_formats.push(display_name.clone());
                
                // Try to match to a supported codec
                if let Some(codec) = VideoCodec::from_gst_caps_name(format_name) {
                    // For Raw codec, only add if an encoder is available
                    if codec == VideoCodec::Raw {
                        if can_encode_raw && !supported_codecs.contains(&codec) {
                            supported_codecs.push(codec);
                        }
                    } else if !supported_codecs.contains(&codec) {
                        supported_codecs.push(codec);
                    }
                }
            }
            
            // Extract resolution
            let width = structure.get::<i32>("width").unwrap_or(1280) as u32;
            let height = structure.get::<i32>("height").unwrap_or(720) as u32;
            let fps = structure.get::<gst::Fraction>("framerate")
                .map(|f| {
                    let numer = f.numer() as f64;
                    let denom = (f.denom() as f64).max(1.0);
                    (numer / denom).round() as u32
                })
                .unwrap_or(30);
            
            let resolution = Resolution { width, height, fps };
            if !resolutions.iter().any(|r| r.width == width && r.height == height) {
                resolutions.push(resolution);
            }
        }
    }
}

/// Probe a device for specific compressed formats by trying to negotiate them
/// This is used when DeviceMonitor only shows RAW format
#[cfg(target_os = "windows")]
fn probe_compressed_formats(
    device_name: &str,
    detected_formats: &mut Vec<String>,
    supported_codecs: &mut Vec<VideoCodec>,
) {
    // List of compressed formats to try (in priority order)
    let formats_to_try = [
        ("image/jpeg", VideoCodec::Mjpeg, "MJPEG"),
    ];
    
    let mut found_formats = Vec::new();
    
    for (caps_name, codec, display_name) in formats_to_try {
        let (supported, _) = try_format_with_debug(device_name, caps_name);
        if supported {
            if !detected_formats.contains(&display_name.to_string()) {
                detected_formats.push(display_name.to_string());
            }
            if !supported_codecs.contains(&codec) {
                supported_codecs.push(codec);
                found_formats.push(display_name);
            }
        }
    }
    
    if !found_formats.is_empty() {
        println!("[Sacho]   Probed formats: {:?}", found_formats);
    }
}

/// Try to negotiate a specific format with a device
/// Returns (supported, actual_caps_string) for debugging
#[cfg(target_os = "windows")]
fn try_format_with_debug(device_name: &str, caps_name: &str) -> (bool, Option<String>) {
    let pipeline = gst::Pipeline::new();
    
    let source = match gst::ElementFactory::make("dshowvideosrc")
        .property("device-name", device_name)
        .build() 
    {
        Ok(src) => src,
        Err(_) => return (false, None),
    };
    
    // Create a capsfilter to force the format we're testing
    let capsfilter = match gst::ElementFactory::make("capsfilter")
        .property("caps", gst::Caps::builder(caps_name).build())
        .build()
    {
        Ok(cf) => cf,
        Err(_) => return (false, None),
    };
    
    let fakesink = match gst::ElementFactory::make("fakesink").build() {
        Ok(sink) => sink,
        Err(_) => return (false, None),
    };
    
    // Add and link elements
    if pipeline.add_many([&source, &capsfilter, &fakesink]).is_err() {
        return (false, None);
    }
    
    if gst::Element::link_many([&source, &capsfilter, &fakesink]).is_err() {
        pipeline.set_state(gst::State::Null).ok();
        return (false, None);
    }
    
    // Try to set to PLAYING - PAUSED may not be enough for some devices
    let _ = pipeline.set_state(gst::State::Playing);
    
    // Wait for state change - 1s is usually enough for most devices
    // Slower capture cards may need more time but we err on the side of speed
    let (state_result, _, _) = pipeline.state(Some(gst::ClockTime::from_mseconds(1000)));
    
    // Get the actual caps for debugging
    let actual_caps_str = source.static_pad("src")
        .and_then(|pad| pad.current_caps())
        .map(|caps| caps.to_string());
    
    // Check if negotiation succeeded AND verify the actual caps
    // For live sources, NoPreroll is expected; Success means it reached the target state
    let result = match state_result {
        Ok(gst::StateChangeSuccess::Success) | Ok(gst::StateChangeSuccess::NoPreroll) | Ok(gst::StateChangeSuccess::Async) => {
            // Verify the source's output caps match the requested format
            if let Some(pad) = source.static_pad("src") {
                if let Some(caps) = pad.current_caps() {
                    if let Some(structure) = caps.structure(0) {
                        let actual_name = structure.name().as_str();
                        // Check if format matches (be flexible with naming)
                        actual_name == caps_name ||
                            (caps_name == "image/jpeg" && (actual_name.contains("jpeg") || actual_name == "image/jpeg"))
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    };
    
    // Clean up
    pipeline.set_state(gst::State::Null).ok();
    
    (result, actual_caps_str)
}

/// Convert GStreamer format name to a short display name
fn format_display_name(gst_name: &str) -> String {
    match gst_name {
        "video/x-raw" => "RAW".to_string(),
        "image/jpeg" => "MJPEG".to_string(),
        "video/x-av1" | "video/av1" => "AV1".to_string(),
        "video/x-vp8" => "VP8".to_string(),
        "video/x-vp9" => "VP9".to_string(),
        "video/x-dv" => "DV".to_string(),
        "video/mpeg" => "MPEG".to_string(),
        _ => gst_name.replace("video/x-", "").replace("video/", "").replace("image/", "").to_uppercase(),
    }
}


/// Enumerate all available audio input devices
pub fn enumerate_audio_devices() -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    
    let host = cpal::default_host();
    let default_device_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());
    
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                let config = device
                    .default_input_config()
                    .map(|c| (c.channels(), c.sample_rate().0))
                    .unwrap_or((2, 44100));
                
                let is_default = default_device_name
                    .as_ref()
                    .map(|d| d == &name)
                    .unwrap_or(false);
                
                devices.push(AudioDevice {
                    id: name.clone(),
                    name: name.clone(),
                    channels: config.0,
                    sample_rate: config.1,
                    is_default,
                });
            }
        }
    }
    
    devices
}

/// Enumerate all available MIDI input devices
pub fn enumerate_midi_devices() -> Vec<MidiDevice> {
    let mut devices = Vec::new();
    
    if let Ok(midi_in) = midir::MidiInput::new("sacho-probe") {
        let ports = midi_in.ports();
        for (index, port) in ports.iter().enumerate() {
            if let Ok(name) = midi_in.port_name(port) {
                devices.push(MidiDevice {
                    id: format!("midi-{}", index),
                    name,
                    port_index: index,
                });
            }
        }
    }
    
    devices
}

/// Enumerate all available video capture devices (webcams) using GStreamer
pub fn enumerate_video_devices() -> Vec<VideoDevice> {
    use std::collections::HashMap;
    
    println!("[Sacho] Enumerating video devices with GStreamer...");
    
    // Initialize GStreamer
    if let Err(e) = gstreamer::init() {
        println!("[Sacho] Failed to initialize GStreamer: {}", e);
        return Vec::new();
    }
    
    // Log GStreamer version for diagnostics
    let (major, minor, micro, nano) = gstreamer::version();
    println!("[Sacho] GStreamer version: {}.{}.{}.{}", major, minor, micro, nano);
    
    // Check for required plugins and log their status
    let registry = gstreamer::Registry::get();
    let required_plugins = [
        "coreelements",      // Core elements like fakesink
        "videoconvertscale", // Video conversion
        #[cfg(target_os = "windows")]
        "winks",             // Windows Kernel Streaming (webcams)
        #[cfg(target_os = "windows")]
        "directshow",        // DirectShow video sources  
        #[cfg(target_os = "windows")]
        "mediafoundation",   // Media Foundation (modern Windows API)
        #[cfg(target_os = "macos")]
        "applemedia",        // macOS AVFoundation
        #[cfg(target_os = "linux")]
        "video4linux2",      // V4L2 on Linux
    ];
    
    println!("[Sacho] Checking required GStreamer plugins:");
    let mut missing_plugins = Vec::new();
    for plugin_name in required_plugins {
        if let Some(plugin) = registry.find_plugin(plugin_name) {
            println!("[Sacho]   {} v{} - OK", plugin_name, plugin.version());
        } else {
            println!("[Sacho]   {} - MISSING", plugin_name);
            missing_plugins.push(plugin_name);
        }
    }
    
    if !missing_plugins.is_empty() {
        println!("[Sacho] WARNING: Missing plugins may cause device enumeration to fail: {:?}", missing_plugins);
    }
    
    // Check recording/encoding plugins
    let recording_plugins = [
        // Container & muxing
        ("matroska",         "MKV/WebM container (matroskamux, matroskademux)"),
        ("app",              "App elements (appsrc, appsink)"),
        // Codecs
        ("vpx",              "VP8/VP9 software encoding (libvpx)"),
        ("jpeg",             "MJPEG encoding/decoding"),
        ("videoparsersbad",  "Video parsers (jpegparse, av1parse, etc.)"),
        // GPU-specific encoders
        ("nvcodec",          "NVIDIA NVENC (RTX 40+ for AV1"),
        ("amfcodec",         "AMD AMF (RX 7000+ for AV1)"),
        ("qsv",              "Intel QuickSync (Arc GPUs, recent iGPUs)"),
    ];
    
    println!("[Sacho] Checking recording/encoding plugins:");
    for (plugin_name, description) in recording_plugins {
        if let Some(plugin) = registry.find_plugin(plugin_name) {
            println!("[Sacho]   {} v{} - OK ({})", plugin_name, plugin.version(), description);
        } else {
            println!("[Sacho]   {} - not available ({})", plugin_name, description);
        }
    }
    
    // Also check for device provider elements
    println!("[Sacho] Checking device providers:");
    #[cfg(target_os = "windows")]
    {
        let providers = ["dshowvideosrc", "mfvideosrc", "ksvideosrc"];
        for provider in providers {
            if gstreamer::ElementFactory::find(provider).is_some() {
                println!("[Sacho]   {} - available", provider);
            } else {
                println!("[Sacho]   {} - not available", provider);
            }
        }
    }
    
    // Check if any encoder is available for raw video support (hardware or software)
    let can_encode_raw = has_av1_encoder() || has_vp9_encoder() || has_vp8_encoder();
    if can_encode_raw {
        println!("[Sacho] Video encoder available - Raw video format will be supported");
    } else {
        println!("[Sacho] No video encoder found - Raw video format will not be available");
    }
    
    // Create device monitor for video sources
    let monitor = gstreamer::DeviceMonitor::new();
    
    // Add filters for both MediaFoundation (Video/Source) and DirectShow (Source/Video)
    // DirectShow provides more accurate codec detection on Windows
    monitor.add_filter(Some("Video/Source"), None);
    monitor.add_filter(Some("Source/Video"), None);
    
    // Log the providers that DeviceMonitor will use
    println!("[Sacho] Starting device monitor...");
    
    if let Err(e) = monitor.start() {
        println!("[Sacho] Failed to start device monitor: {}", e);
        println!("[Sacho] This usually means no device provider plugins are loaded.");
        println!("[Sacho] On Windows, ensure GStreamer DLLs are in the same directory as sacho.exe");
        println!("[Sacho] Required: gstreamer-1.0-0.dll and related plugin DLLs");
        
        // Try to provide more context by checking if we can at least create a basic pipeline
        match gstreamer::ElementFactory::make("fakesink").build() {
            Ok(_) => println!("[Sacho] Core GStreamer elements work, but device providers are missing"),
            Err(e) => println!("[Sacho] Even basic GStreamer elements fail: {}", e),
        }
        
        return Vec::new();
    }
    
    // Collect device info, merging duplicates by name
    // Key: device name, Value: (device_class, caps from each source)
    let mut device_map: HashMap<String, DeviceInfo> = HashMap::new();
    
    struct DeviceInfo {
        classes: Vec<String>,
        all_caps: Vec<gst::Caps>,
    }
    
    for device in monitor.devices() {
        let name = device.display_name().to_string();
        let device_class = device.device_class().to_string();
        
        // Only include video source devices
        if !device_class.contains("Video") && !device_class.contains("Source") {
            continue;
        }
        
        println!("[Sacho] Found device: {} (class: {})", name, device_class);
        
        let entry = device_map.entry(name.clone()).or_insert_with(|| DeviceInfo {
            classes: Vec::new(),
            all_caps: Vec::new(),
        });
        
        entry.classes.push(device_class.clone());
        
        if let Some(caps) = device.caps() {
            println!("[Sacho]   {} caps: {} structures", device_class, caps.size());
            entry.all_caps.push(caps);
        }
    }
    
    monitor.stop();
    
    // Process collected devices
    let mut devices = Vec::new();
    
    for (name, info) in device_map {
        println!("[Sacho] Processing device: {} (sources: {:?})", name, info.classes);
        
        // Create stable device ID based on name (not index, which can change)
        let safe_name = name
            .replace(" ", "_")
            .replace("/", "_")
            .replace("\\", "_")
            .replace(":", "_")
            .replace("(", "")
            .replace(")", "")
            .to_lowercase();
        let device_id = format!("video-{}", safe_name);
        
        // Try to get supported resolutions and formats from device caps
        let mut resolutions = Vec::new();
        let mut supported_codecs = Vec::new();
        let mut detected_formats: Vec<String> = Vec::new();
        
        // First, process DeviceMonitor caps (these are accurate for what the device reports)
        for caps in &info.all_caps {
            process_caps(caps, &mut detected_formats, &mut supported_codecs, &mut resolutions, can_encode_raw);
        }
        
        // On Windows, if we only found RAW format, probe more aggressively
        // Some devices (like capture cards) expose video through DirectShow but not DeviceMonitor
        #[cfg(target_os = "windows")]
        {
            // Check if we only have Raw codec (or nothing except Raw/DV formats)
            // We need to probe even if Raw was added, since hardware-encoded formats might be available
            let has_only_raw_codec = supported_codecs.iter().all(|c| *c == VideoCodec::Raw);
            let only_raw_formats = detected_formats.iter().all(|f| f == "RAW" || f == "DV");
            
            if has_only_raw_codec && only_raw_formats {
                println!("[Sacho]   Only RAW detected, probing for compressed formats...");
                probe_compressed_formats(&name, &mut detected_formats, &mut supported_codecs);
            }
        }
            
            // Log all detected formats
            println!("[Sacho]   All formats: {:?}", detected_formats);
            
            if detected_formats.is_empty() {
                println!("[Sacho]   No caps available for device");
            }
            
            // Default resolutions if none detected
            if resolutions.is_empty() {
                resolutions = vec![
                    Resolution { width: 1920, height: 1080, fps: 30 },
                    Resolution { width: 1280, height: 720, fps: 30 },
                    Resolution { width: 640, height: 480, fps: 30 },
                ];
            }
            
            // Sort by resolution (highest first)
            resolutions.sort_by(|a, b| (b.width * b.height).cmp(&(a.width * a.height)));
            
            let codec_names: Vec<_> = supported_codecs.iter().map(|c| c.display_name()).collect();
            println!("[Sacho] Video {}: {} ({} resolutions, codecs: {:?})", 
                device_id, name, resolutions.len(), codec_names);
            
        devices.push(VideoDevice {
            id: device_id,
            name,
            resolutions,
            supported_codecs,
            all_formats: detected_formats,
        });
    }
    
    println!("[Sacho] Found {} video device(s)", devices.len());
    
    devices
}
