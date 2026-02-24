// Device enumeration implementations

use super::{AudioDevice, MidiDevice, VideoDevice, CodecCapability};
use crate::encoding::{has_av1_encoder, has_vp9_encoder, has_vp8_encoder};
use cpal::traits::{DeviceTrait, HostTrait};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// Global storage for GStreamer Device objects, keyed by our device ID.
///
/// Each physical device may have multiple GStreamer providers (e.g. Kernel Streaming,
/// MediaFoundation, DirectShow on Windows). We store ALL provider `gst::Device` objects
/// so that at pipeline creation time we can pick the provider whose caps actually match
/// the requested mode. This avoids phantom framerates (e.g. 60fps reported by MF but
/// only 59.94 supported by KS) and ensures we always use the correct provider.
static GST_DEVICE_STORE: Mutex<Option<HashMap<String, Vec<gst::Device>>>> = Mutex::new(None);

/// Retrieve the first (fallback) GStreamer Device object by device ID.
/// Prefer `get_device_for_caps` when you have a specific mode to match.
pub fn get_gst_device(device_id: &str) -> Option<gst::Device> {
    let store = GST_DEVICE_STORE.lock().ok()?;
    store.as_ref()?.get(device_id)?.first().cloned()
}

/// Validate that a video device configuration will produce a working pipeline.
///
/// Checks whether ANY stored GStreamer provider for this device has exact caps
/// matching the requested format, resolution, and framerate. Returns true if at
/// least one provider can handle the configuration.
pub fn validate_video_config(device_id: &str, format: &str, width: u32, height: u32, fps: f64) -> bool {
    get_device_for_format(device_id, format, width, height, fps).is_some()
}

/// Find the best GStreamer Device + exact caps for a given source format string.
///
/// Converts the format string (e.g. "YUY2", "MJPEG", "H264") to GStreamer caps
/// and delegates to `get_device_for_caps`. For raw pixel formats, the caps include
/// the format field (e.g. `video/x-raw,format=YUY2`).
pub fn get_device_for_format(
    device_id: &str,
    format: &str,
    width: u32,
    height: u32,
    fps: f64,
) -> Option<(gst::Caps, gst::Device)> {
    let (caps_name, format_field) = crate::encoding::format_to_gst_caps(format);

    // For raw formats with a specific pixel format field, try the exact caps first
    if let Some(fmt) = format_field {
        let store = GST_DEVICE_STORE.lock().ok()?;
        let devices = store.as_ref()?.get(device_id)?;
        let target_fps = crate::encoding::encoder::fps_to_gst_fraction(fps);

        let filter = gst::Caps::builder(caps_name)
            .field("format", fmt)
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", target_fps)
            .build();

        for gst_dev in devices {
            if let Some(device_caps) = gst_dev.caps() {
                let matched = device_caps.intersect_with_mode(&filter, gst::CapsIntersectMode::First);
                if !matched.is_empty() {
                    println!("[Video] Found exact caps for format '{}' via provider '{}': {}",
                        format, gst_dev.device_class(), matched);
                    return Some((matched, gst_dev.clone()));
                }
            }
        }

        println!("[Video] No provider has exact caps for format '{}' {}x{} @ {:.2}fps in device {}",
            format, width, height, fps, device_id);
        None
    } else {
        // Encoded format (MJPEG, H264, AV1, VP8, VP9) — delegate to existing caps-based lookup
        get_device_for_caps(device_id, caps_name, width, height, fps)
    }
}

/// Find the best GStreamer Device + exact caps that match the desired mode.
///
/// Loops through ALL stored providers for the given device ID and returns the
/// first provider whose caps intersect with the requested codec/resolution/fps.
/// The returned caps preserve ALL fields (format, pixel-aspect-ratio, colorimetry,
/// etc.) from the device — critical for Windows KS/MF sources where partial caps
/// cause negotiation failures.
///
/// Returns `(exact_caps, matching_gst_device)`, or `None` if no provider matches.
pub fn get_device_for_caps(
    device_id: &str,
    caps_name: &str,
    width: u32,
    height: u32,
    fps: f64,
) -> Option<(gst::Caps, gst::Device)> {
    let store = GST_DEVICE_STORE.lock().ok()?;
    let devices = store.as_ref()?.get(device_id)?;
    let target_fps = crate::encoding::encoder::fps_to_gst_fraction(fps);

    let filter = gst::Caps::builder(caps_name)
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", target_fps)
        .build();

    for gst_dev in devices {
        if let Some(device_caps) = gst_dev.caps() {
            let matched = device_caps.intersect_with_mode(&filter, gst::CapsIntersectMode::First);
            if !matched.is_empty() {
                println!("[Video] Found exact caps via provider '{}': {}",
                    gst_dev.device_class(), matched);
                return Some((matched, gst_dev.clone()));
            }
        }
    }

    // Some capture cards advertise H.264 as video/x-raw with format=H264 or X264.
    // Elgato cards use H264 at native resolution and X264 for scaled outputs.
    // If the standard caps didn't match, try both raw-format variants.
    if caps_name == "video/x-h264" {
        for raw_fmt in &["H264", "X264"] {
            let raw_h264_filter = gst::Caps::builder("video/x-raw")
                .field("format", *raw_fmt)
                .field("width", width as i32)
                .field("height", height as i32)
                .field("framerate", target_fps)
                .build();

            for gst_dev in devices {
                if let Some(device_caps) = gst_dev.caps() {
                    let matched = device_caps.intersect_with_mode(&raw_h264_filter, gst::CapsIntersectMode::First);
                    if !matched.is_empty() {
                        println!("[Video] Found H.264-as-raw caps via provider '{}': {}",
                            gst_dev.device_class(), matched);
                        return Some((matched, gst_dev.clone()));
                    }
                }
            }
        }
    }

    println!("[Video] No provider has exact caps for {} {}x{} @ {:.2}fps in device {}",
        caps_name, width, height, fps, device_id);
    None
}

/// Convenience wrapper that returns only the caps (without the device).
/// Used when the caller doesn't need the matching device object.
pub fn get_device_exact_caps(
    device_id: &str,
    caps_name: &str,
    width: u32,
    height: u32,
    fps: f64,
) -> Option<gst::Caps> {
    get_device_for_caps(device_id, caps_name, width, height, fps).map(|(caps, _)| caps)
}

/// Intermediate structure for collecting per-format capabilities during enumeration
struct CapabilityCollector {
    /// format_string -> (width, height) -> set of framerates (f64 to preserve fractions like 29.97)
    data: HashMap<String, HashMap<(u32, u32), Vec<f64>>>,
}

impl CapabilityCollector {
    fn new() -> Self {
        Self { data: HashMap::new() }
    }

    fn add(&mut self, format: String, width: u32, height: u32, fps: f64) {
        let res_map = self.data.entry(format).or_default();
        let fps_list = res_map.entry((width, height)).or_default();
        // Deduplicate: consider fps values within 0.01 as the same
        if !fps_list.iter().any(|&existing| (existing - fps).abs() < 0.01) {
            fps_list.push(fps);
        }
    }

    /// Finalize into sorted CodecCapability lists per format
    fn finalize(self) -> HashMap<String, Vec<CodecCapability>> {
        let mut result = HashMap::new();
        for (format, res_map) in self.data {
            let mut caps: Vec<CodecCapability> = res_map.into_iter()
                .map(|((w, h), mut fps_list)| {
                    fps_list.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)); // Descending
                    CodecCapability { width: w, height: h, framerates: fps_list }
                })
                .collect();
            // Sort by resolution descending (highest pixel count first)
            caps.sort_by(|a, b| (b.width * b.height).cmp(&(a.width * a.height)));
            result.insert(format, caps);
        }
        result
    }
}

/// Process GStreamer caps to extract per-format capabilities.
///
/// For `video/x-raw`, the format field (e.g. "YUY2", "NV12") becomes the key.
/// For encoded formats, the codec display name (e.g. "MJPEG", "H264") is the key.
/// Some capture cards advertise H.264 as `video/x-raw,format=H264` — these are
/// detected and keyed as "H264".
fn process_caps(
    caps: &gst::Caps,
    collector: &mut CapabilityCollector,
    can_encode_raw: bool,
) {
    for i in 0..caps.size() {
        if let Some(structure) = caps.structure(i) {
            let caps_name = structure.name().as_str();

            // Determine the format key string for this caps structure
            let format_key: String = match caps_name {
                "video/x-raw" => {
                    if let Ok(fmt) = structure.get::<&str>("format") {
                        if fmt == "H264" || fmt == "X264" {
                            // H.264-as-raw: capture cards advertise H.264 as video/x-raw,format=H264
                            "H264".to_string()
                        } else if !can_encode_raw {
                            continue; // Skip raw pixel formats when no encoder is available
                        } else {
                            fmt.to_string() // Use actual pixel format name: "YUY2", "NV12", etc.
                        }
                    } else if !can_encode_raw {
                        continue
                    } else {
                        "RAW".to_string() // Fallback if no format field
                    }
                }
                "image/jpeg" => "MJPEG".to_string(),
                "video/x-h264" => "H264".to_string(),
                "video/x-av1" | "video/av1" => "AV1".to_string(),
                "video/x-vp8" => "VP8".to_string(),
                "video/x-vp9" => "VP9".to_string(),
                _ => continue, // Skip unsupported caps types
            };

            // Extract width (may be fixed int or IntRange)
            let widths = extract_int_values(&structure, "width");
            let heights = extract_int_values(&structure, "height");
            let framerates = extract_framerate_values(&structure);

            // If we got nothing useful, use defaults
            let widths = if widths.is_empty() { vec![1280] } else { widths };
            let heights = if heights.is_empty() { vec![720] } else { heights };
            let framerates = if framerates.is_empty() { vec![30.0] } else { framerates };

            // Add every combination
            for &w in &widths {
                for &h in &heights {
                    for &fps in &framerates {
                        if fps > 0.0 {
                            collector.add(format_key.clone(), w, h, fps);
                        }
                    }
                }
            }
        }
    }
}

/// Extract integer values from a GStreamer structure field.
/// Handles fixed values, IntRange (samples representative values), and lists.
fn extract_int_values(structure: &gst::StructureRef, field: &str) -> Vec<u32> {
    // Try as a fixed int first (most common case)
    if let Ok(val) = structure.get::<i32>(field) {
        return vec![val as u32];
    }
    
    // Try to get the raw glib::Value and inspect it
    // GStreamer caps can contain IntRange or lists
    if let Ok(value) = structure.value(field) {
        // Check if it's a list of values
        if let Ok(list) = value.get::<gst::List>() {
            let mut result = Vec::new();
            for v in list.iter() {
                if let Ok(int_val) = v.get::<i32>() {
                    result.push(int_val as u32);
                }
            }
            if !result.is_empty() {
                return result;
            }
        }
        
        // Handle IntRange: extract min/max and return common values within the range
        if let Ok(range) = value.get::<gst::IntRange<i32>>() {
            let min = range.min() as u32;
            let max = range.max() as u32;
            let step = range.step() as u32;
            
            println!("[Sacho]   IntRange for {}: {} .. {} (step {})", field, min, max, step);
            
            // For width/height, sample common video resolutions within the range
            let common_values: Vec<u32> = if field == "width" {
                vec![7680, 3840, 2560, 1920, 1280, 1024, 960, 800, 640, 480, 352, 320, 176, 160]
            } else if field == "height" {
                vec![4320, 2160, 1440, 1080, 960, 720, 600, 576, 480, 400, 360, 288, 240, 144, 120]
            } else {
                vec![]
            };
            
            let result: Vec<u32> = common_values.into_iter()
                .filter(|&v| v >= min && v <= max && (step <= 1 || (v - min) % step == 0))
                .collect();
            
            if !result.is_empty() {
                return result;
            }
            
            // If no common values fit, return at least the max
            return vec![max];
        }
    }
    
    Vec::new()
}

/// Extract framerate values from a GStreamer structure.
/// Handles fixed fractions, fraction ranges, and fraction lists.
/// Returns f64 values preserving fractional rates (e.g. 29.97 for 30000/1001).
fn extract_framerate_values(structure: &gst::StructureRef) -> Vec<f64> {
    // Try as a fixed fraction first (most common in negotiated caps)
    if let Ok(frac) = structure.get::<gst::Fraction>("framerate") {
        let numer = frac.numer() as f64;
        let denom = (frac.denom() as f64).max(1.0);
        let fps = numer / denom;
        if fps > 0.0 {
            return vec![fps];
        }
    }
    
    // Try to get the raw value for list/range handling
    if let Ok(value) = structure.value("framerate") {
        // Check if it's a list of fractions
        if let Ok(list) = value.get::<gst::List>() {
            let mut result = Vec::new();
            for v in list.iter() {
                if let Ok(frac) = v.get::<gst::Fraction>() {
                    let numer = frac.numer() as f64;
                    let denom = (frac.denom() as f64).max(1.0);
                    let fps = numer / denom;
                    if fps > 0.0 && !result.iter().any(|&existing: &f64| (existing - fps).abs() < 0.01) {
                        result.push(fps);
                    }
                }
            }
            if !result.is_empty() {
                return result;
            }
        }
        
        // For fraction ranges, extract min/max and filter common values
        if let Ok(range) = value.get::<gst::FractionRange>() {
            let min_frac = range.min();
            let max_frac = range.max();
            let min_fps = min_frac.numer() as f64 / (min_frac.denom() as f64).max(1.0);
            let max_fps = max_frac.numer() as f64 / (max_frac.denom() as f64).max(1.0);
            
            println!("[Sacho]   FractionRange: {}/{} .. {}/{} ({:.2} .. {:.2} fps)",
                min_frac.numer(), min_frac.denom(),
                max_frac.numer(), max_frac.denom(),
                min_fps, max_fps);
            
            // Only list common framerates that fall within the device's actual range.
            // Using a small tolerance (0.5) to include NTSC rates like 29.97 when max is 30.
            let common = vec![120.0, 60.0, 30.0, 24.0, 15.0, 10.0, 5.0];
            let result: Vec<f64> = common.into_iter()
                .filter(|&f| f >= min_fps - 0.5 && f <= max_fps + 0.5)
                .collect();
            
            if !result.is_empty() {
                return result;
            }
            
            // If no common values fit, return the max as a single option
            if max_fps > 0.0 {
                return vec![max_fps];
            }
        }
        
        // Final fallback: check serialized form for any other fraction-like type
        let val_str = format!("{:?}", value);
        if val_str.contains("Fraction") {
            // Unknown fraction type — conservative default
            println!("[Sacho]   Unknown fraction value: {}", val_str);
            return vec![30.0, 15.0];
        }
    }
    
    Vec::new()
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
        ("matroska",         "MKV container (matroskamux, matroskademux)"),
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
        /// GStreamer Device objects from each provider (for create_element later)
        gst_devices: Vec<gst::Device>,
    }
    
    for device in monitor.devices() {
        let name = device.display_name().to_string();
        let device_class = device.device_class().to_string();
        
        // Only include video source devices
        if !device_class.contains("Video") && !device_class.contains("Source") {
            continue;
        }
        
        let caps_count = device.caps().map(|c| c.size()).unwrap_or(0);
        println!("[Sacho] Found device: {} (class: {}, caps: {})", name, device_class, caps_count);
        
        let entry = device_map.entry(name.clone()).or_insert_with(|| DeviceInfo {
            classes: Vec::new(),
            gst_devices: Vec::new(),
        });
        
        entry.classes.push(device_class);
        entry.gst_devices.push(device);
    }
    
    monitor.stop();
    
    // Clear previous device store and prepare to save new ones
    if let Ok(mut store) = GST_DEVICE_STORE.lock() {
        *store = Some(HashMap::new());
    }
    
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
        
        let mut collector = CapabilityCollector::new();

        // Store ALL GStreamer Device objects for this physical device so we can
        // pick the right provider at pipeline creation time. Process capabilities
        // from every provider — `get_device_for_caps` will match the exact provider
        // when building the pipeline, so only truly supported modes succeed.
        for gst_dev in &info.gst_devices {
            println!("[Sacho]   Processing provider '{}' for {} (caps: {})",
                gst_dev.device_class(), device_id, gst_dev.caps().map(|c| c.size()).unwrap_or(0));

            if let Some(caps) = gst_dev.caps() {
                process_caps(&caps, &mut collector, can_encode_raw);
            }
        }

        // Save all providers for this device, sorted so that Media Foundation
        // ("Source/Video" → mfvideosrc) comes before Kernel Streaming
        // ("Video/Source" → ksvideosrc). MF is the modern Windows capture API and
        // produces data that GStreamer decoders (e.g. jpegdec) handle correctly,
        // whereas KS is deprecated and its JPEG output can fail negotiation.
        // get_device_for_caps() picks the first matching provider, so order matters.
        let mut sorted_devices = info.gst_devices;
        sorted_devices.sort_by_key(|d| {
            let class = d.device_class().to_string();
            if class == "Source/Video" { 0 }   // MF first
            else if class == "Video/Source" { 2 } // KS last
            else { 1 }                           // anything else in between
        });
        if let Ok(mut store) = GST_DEVICE_STORE.lock() {
            if let Some(map) = store.as_mut() {
                map.insert(device_id.clone(), sorted_devices);
            }
        }

        // Finalize capabilities
        let capabilities = collector.finalize();

        let format_names: Vec<_> = capabilities.keys().collect();
        let total_modes: usize = capabilities.values().map(|v| v.iter().map(|c| c.framerates.len()).sum::<usize>()).sum();
        println!("[Sacho] Video {}: {} ({} modes, formats: {:?})",
            device_id, name, total_modes, format_names);

        if capabilities.is_empty() {
            println!("[Sacho]   No supported formats for device");
        }

        devices.push(VideoDevice {
            id: device_id,
            name,
            capabilities,
        });
    }
    
    println!("[Sacho] Found {} video device(s)", devices.len());
    
    devices
}
