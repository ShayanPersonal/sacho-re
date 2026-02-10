// Pre-roll buffer management for MIDI and audio
// Maintains rolling buffers of recent data to include when recording starts

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::midi::TimestampedMidiEvent;

/// Maximum pre-roll duration when encoding during pre-roll is OFF
pub const MAX_PRE_ROLL_SECS: u32 = 5;

/// Maximum pre-roll duration when encoding during pre-roll is ON
/// Encoded frames are much smaller than raw, so we can afford a longer window.
pub const MAX_PRE_ROLL_SECS_ENCODED: u32 = 30;

// ============================================================================
// MIDI Pre-roll Buffer
// ============================================================================

/// A timestamped MIDI event for the pre-roll buffer
#[derive(Debug, Clone)]
pub struct BufferedMidiEvent {
    pub device_name: String,
    pub event: TimestampedMidiEvent,
    /// Wall clock time when event was processed (used for buffer trimming)
    pub wall_time: Instant,
    /// Driver timestamp in microseconds (preserves accurate timing between events)
    pub driver_timestamp_us: u64,
}

/// Rolling buffer for MIDI events
pub struct MidiPrerollBuffer {
    events: VecDeque<BufferedMidiEvent>,
    max_duration: Duration,
}

impl MidiPrerollBuffer {
    pub fn new(max_secs: u32) -> Self {
        Self::with_limit(max_secs, MAX_PRE_ROLL_SECS)
    }
    
    pub fn with_limit(max_secs: u32, limit: u32) -> Self {
        Self {
            events: VecDeque::new(),
            max_duration: Duration::from_secs(max_secs.min(limit) as u64),
        }
    }
    
    pub fn set_duration(&mut self, secs: u32) {
        self.set_duration_with_limit(secs, MAX_PRE_ROLL_SECS);
    }
    
    pub fn set_duration_with_limit(&mut self, secs: u32, limit: u32) {
        self.max_duration = Duration::from_secs(secs.min(limit) as u64);
        self.trim();
    }
    
    pub fn push(&mut self, device_name: String, event: TimestampedMidiEvent, driver_timestamp_us: u64) {
        self.events.push_back(BufferedMidiEvent {
            device_name: device_name.clone(),
            event,
            wall_time: Instant::now(),
            driver_timestamp_us,
        });
        self.trim();
        println!("[Sacho PreRoll] Buffered MIDI event from {}, buffer size: {}, driver_ts: {}us", 
            device_name, self.events.len(), driver_timestamp_us);
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
            // Sync with audio using DRIVER timestamps for accurate relative timing
            // 
            // The problem: wall_time is set when the callback acquires the lock, not when
            // the MIDI event actually arrived. If the lock is held (e.g., during start_recording),
            // multiple events can have nearly identical wall_times, destroying their relative timing.
            //
            // The solution: Use driver_timestamp_us which preserves accurate timing from the hardware.
            // We anchor the LAST event to the current moment, then calculate all other events'
            // timestamps relative to it using their driver timestamp differences.
            
            println!("[Sacho PreRoll] Pre-roll span: {}ms, syncing to audio pre-roll: {}ms", 
                span_ms, audio_duration.as_millis());
            
            // First, filter to events within the pre-roll window (using wall_time for this check)
            let filtered_events: Vec<_> = events.into_iter()
                .filter(|e| now.duration_since(e.wall_time) <= audio_duration)
                .collect();
            
            if filtered_events.is_empty() {
                return Vec::new();
            }
            
            // Get the last event's driver timestamp as our anchor point
            let last_event = filtered_events.last().unwrap();
            let last_driver_ts = last_event.driver_timestamp_us;
            let last_wall_ago = now.duration_since(last_event.wall_time);
            
            // The last event's timestamp in the output = audio_duration - time_since_last_event
            let last_output_ts_us = (audio_duration - last_wall_ago).as_micros() as u64;
            
            // Calculate each event's timestamp relative to the last event using driver timestamps
            filtered_events.into_iter()
                .map(|e| {
                    // How many microseconds before the last event did this event occur?
                    let driver_delta_us = last_driver_ts.saturating_sub(e.driver_timestamp_us);
                    
                    // This event's output timestamp = last_output_ts - driver_delta
                    let timestamp_us = last_output_ts_us.saturating_sub(driver_delta_us);
                    
                    let mut adjusted_event = e.event;
                    adjusted_event.timestamp_us = timestamp_us;
                    (e.device_name, adjusted_event)
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
        Self::with_limit(device_name, sample_rate, channels, max_secs, MAX_PRE_ROLL_SECS)
    }
    
    pub fn with_limit(device_name: String, sample_rate: u32, channels: u16, max_secs: u32, limit: u32) -> Self {
        let max_samples = (sample_rate as usize) * (channels as usize) * (max_secs.min(limit) as usize);
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
            sample_rate,
            channels,
            device_name,
        }
    }
    
    pub fn set_duration(&mut self, secs: u32) {
        self.set_duration_with_limit(secs, MAX_PRE_ROLL_SECS);
    }
    
    pub fn set_duration_with_limit(&mut self, secs: u32, limit: u32) {
        self.max_samples = (self.sample_rate as usize) * (self.channels as usize) * (secs.min(limit) as usize);
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
    
    /// Drain all buffered samples (aligned to frame boundary)
    pub fn drain(&mut self) -> Vec<f32> {
        // Ensure we return a multiple of channels to prevent WAV write errors
        let total = self.samples.len();
        let aligned = (total / self.channels as usize) * self.channels as usize;
        if aligned < total {
            // Drop the last partial frame
            self.samples.truncate(aligned);
        }
        self.samples.drain(..).collect()
    }
    
    /// Drain samples for a specific duration (from the end of the buffer)
    /// This allows syncing audio pre-roll to a shorter video pre-roll duration
    pub fn drain_duration(&mut self, duration: Duration) -> Vec<f32> {
        // Calculate samples and round DOWN to nearest frame boundary (multiple of channels)
        // This prevents partial frames which cause WAV write errors
        let raw_samples = (duration.as_secs_f64() * self.sample_rate as f64 * self.channels as f64) as usize;
        let samples_for_duration = (raw_samples / self.channels as usize) * self.channels as usize;
        
        if samples_for_duration >= self.samples.len() {
            // Requested duration covers all samples, drain everything
            // But ensure we return a multiple of channels
            let total = self.samples.len();
            let aligned = (total / self.channels as usize) * self.channels as usize;
            if aligned < total {
                // Drop the last partial frame
                self.samples.truncate(aligned);
            }
            self.samples.drain(..).collect()
        } else {
            // Only take the last N samples (most recent audio)
            let skip_count = self.samples.len() - samples_for_duration;
            let result: Vec<f32> = self.samples.iter().skip(skip_count).cloned().collect();
            self.samples.clear();
            result
        }
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
