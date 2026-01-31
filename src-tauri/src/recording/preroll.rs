// Pre-roll buffer management for MIDI and audio
// Maintains rolling buffers of recent data to include when recording starts

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::midi::TimestampedMidiEvent;

/// Maximum pre-roll duration (5 seconds)
pub const MAX_PRE_ROLL_SECS: u32 = 5;

// ============================================================================
// MIDI Pre-roll Buffer
// ============================================================================

/// A timestamped MIDI event for the pre-roll buffer
#[derive(Debug, Clone)]
pub struct BufferedMidiEvent {
    pub device_name: String,
    pub event: TimestampedMidiEvent,
    pub wall_time: Instant,
}

/// Rolling buffer for MIDI events
pub struct MidiPrerollBuffer {
    events: VecDeque<BufferedMidiEvent>,
    max_duration: Duration,
}

impl MidiPrerollBuffer {
    pub fn new(max_secs: u32) -> Self {
        Self {
            events: VecDeque::new(),
            max_duration: Duration::from_secs(max_secs.min(MAX_PRE_ROLL_SECS) as u64),
        }
    }
    
    pub fn set_duration(&mut self, secs: u32) {
        self.max_duration = Duration::from_secs(secs.min(MAX_PRE_ROLL_SECS) as u64);
        self.trim();
    }
    
    pub fn push(&mut self, device_name: String, event: TimestampedMidiEvent) {
        self.events.push_back(BufferedMidiEvent {
            device_name: device_name.clone(),
            event,
            wall_time: Instant::now(),
        });
        self.trim();
        println!("[Sacho PreRoll] Buffered MIDI event from {}, buffer size: {}, max_duration: {:?}", 
            device_name, self.events.len(), self.max_duration);
    }
    
    fn trim(&mut self) {
        let cutoff = Instant::now() - self.max_duration;
        while let Some(front) = self.events.front() {
            if front.wall_time < cutoff {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }
    
    /// Drain all buffered events, returning them with adjusted timestamps.
    /// 
    /// If `audio_preroll_duration` is provided, timestamps are calculated relative to
    /// the start of the audio pre-roll period, ensuring MIDI and audio stay in sync.
    /// Events are positioned correctly within the pre-roll window based on when they
    /// occurred relative to the trigger moment (now).
    /// 
    /// If `audio_preroll_duration` is None, falls back to making the first event timestamp 0.
    pub fn drain_with_audio_sync(&mut self, audio_preroll_duration: Option<Duration>) -> Vec<(String, TimestampedMidiEvent)> {
        let events: Vec<_> = self.events.drain(..).collect();
        let now = Instant::now();
        
        println!("[Sacho PreRoll] Draining {} pre-roll MIDI events", events.len());
        
        if events.is_empty() {
            return Vec::new();
        }
        
        // Calculate the span of MIDI events
        let first_time = events[0].wall_time;
        let last_time = events.last().map(|e| e.wall_time).unwrap_or(first_time);
        let span_ms = last_time.duration_since(first_time).as_millis();
        
        if let Some(audio_duration) = audio_preroll_duration {
            // Sync with audio: calculate timestamps relative to audio pre-roll start
            // Audio pre-roll starts at (now - audio_duration)
            // Each MIDI event's timestamp = audio_duration - time_before_now
            println!("[Sacho PreRoll] Pre-roll span: {}ms, syncing to audio pre-roll: {}ms", 
                span_ms, audio_duration.as_millis());
            
            events.into_iter()
                .filter_map(|e| {
                    // How long ago did this event occur?
                    let time_before_now = now.duration_since(e.wall_time);
                    
                    // Event timestamp relative to audio pre-roll start
                    // If audio pre-roll is 2s and event was 0.5s ago, timestamp = 2.0 - 0.5 = 1.5s
                    if time_before_now <= audio_duration {
                        let timestamp_us = (audio_duration - time_before_now).as_micros() as u64;
                        let mut adjusted_event = e.event;
                        adjusted_event.timestamp_us = timestamp_us;
                        Some((e.device_name, adjusted_event))
                    } else {
                        // Event is older than audio pre-roll window, skip it
                        // (shouldn't happen if both buffers use same max duration)
                        println!("[Sacho PreRoll] Dropping MIDI event older than audio pre-roll");
                        None
                    }
                })
                .collect()
        } else {
            // No audio sync: use original behavior (first event at timestamp 0)
            println!("[Sacho PreRoll] Pre-roll span: {}ms (no audio sync)", span_ms);
            
            events.into_iter()
                .map(|e| {
                    let relative_us = e.wall_time.duration_since(first_time).as_micros() as u64;
                    let mut adjusted_event = e.event;
                    adjusted_event.timestamp_us = relative_us;
                    (e.device_name, adjusted_event)
                })
                .collect()
        }
    }
    
    /// Drain all buffered events (legacy method, uses first-event-at-zero timing)
    pub fn drain(&mut self) -> Vec<(String, TimestampedMidiEvent)> {
        self.drain_with_audio_sync(None)
    }
    
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

// ============================================================================
// Audio Pre-roll Buffer
// ============================================================================

/// Circular buffer for audio samples
pub struct AudioPrerollBuffer {
    /// Ring buffer of samples (interleaved)
    samples: VecDeque<f32>,
    /// Maximum number of samples to keep
    max_samples: usize,
    /// Sample rate
    sample_rate: u32,
    /// Number of channels
    channels: u16,
    /// Device name
    device_name: String,
}

impl AudioPrerollBuffer {
    pub fn new(device_name: String, sample_rate: u32, channels: u16, max_secs: u32) -> Self {
        let max_samples = (sample_rate as usize) * (channels as usize) * (max_secs.min(MAX_PRE_ROLL_SECS) as usize);
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
            sample_rate,
            channels,
            device_name,
        }
    }
    
    pub fn set_duration(&mut self, secs: u32) {
        self.max_samples = (self.sample_rate as usize) * (self.channels as usize) * (secs.min(MAX_PRE_ROLL_SECS) as usize);
        self.trim();
    }
    
    pub fn push_samples(&mut self, samples: &[f32]) {
        self.samples.extend(samples.iter().cloned());
        self.trim();
    }
    
    fn trim(&mut self) {
        while self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }
    }
    
    /// Drain all buffered samples
    pub fn drain(&mut self) -> Vec<f32> {
        self.samples.drain(..).collect()
    }
    
    pub fn clear(&mut self) {
        self.samples.clear();
    }
    
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
    
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    
    pub fn channels(&self) -> u16 {
        self.channels
    }
}
