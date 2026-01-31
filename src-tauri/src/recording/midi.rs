// MIDI capture using midir

use midir::{MidiInput, MidiInputConnection};
use std::sync::Arc;
use parking_lot::Mutex;
use std::path::PathBuf;
use chrono::{DateTime, Utc};

/// MIDI event with timestamp
#[derive(Debug, Clone)]
pub struct TimestampedMidiEvent {
    pub timestamp_us: u64,
    pub data: Vec<u8>,
}

/// MIDI capture configuration
#[derive(Debug, Clone)]
pub struct MidiCaptureConfig {
    pub device_id: String,
    pub port_index: usize,
    pub output_path: PathBuf,
    pub is_trigger: bool,
}

/// Callback type for MIDI trigger events
pub type MidiTriggerCallback = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;

/// MIDI capture handle for a single device
pub struct MidiCapture {
    config: MidiCaptureConfig,
    connection: Option<MidiInputConnection<()>>,
    events: Arc<Mutex<Vec<TimestampedMidiEvent>>>,
    start_time: Option<DateTime<Utc>>,
    trigger_callback: Option<MidiTriggerCallback>,
}

impl MidiCapture {
    pub fn new(config: MidiCaptureConfig) -> Self {
        Self {
            config,
            connection: None,
            events: Arc::new(Mutex::new(Vec::new())),
            start_time: None,
            trigger_callback: None,
        }
    }
    
    /// Set callback for trigger events
    pub fn set_trigger_callback(&mut self, callback: MidiTriggerCallback) {
        self.trigger_callback = Some(callback);
    }
    
    /// Start capturing MIDI from the configured device
    pub fn start(&mut self) -> anyhow::Result<()> {
        let midi_in = MidiInput::new("sacho-capture")?;
        let ports = midi_in.ports();
        
        let port = ports.get(self.config.port_index)
            .ok_or_else(|| anyhow::anyhow!("MIDI port not found: {}", self.config.port_index))?;
        
        self.start_time = Some(Utc::now());
        let start_time = self.start_time.unwrap();
        let events = self.events.clone();
        let is_trigger = self.config.is_trigger;
        let device_id = self.config.device_id.clone();
        let trigger_callback = self.trigger_callback.clone();
        
        let connection = midi_in.connect(
            port,
            "sacho-input",
            move |_timestamp, data, _| {
                // Calculate timestamp relative to start
                let now = Utc::now();
                let elapsed = (now - start_time).num_microseconds().unwrap_or(0) as u64;
                
                // Store event
                events.lock().push(TimestampedMidiEvent {
                    timestamp_us: elapsed,
                    data: data.to_vec(),
                });
                
                // Trigger callback if this is a trigger device
                if is_trigger {
                    if let Some(ref callback) = trigger_callback {
                        callback(&device_id, data);
                    }
                }
            },
            (),
        ).map_err(|e| anyhow::anyhow!("Failed to connect to MIDI port: {}", e))?;
        
        self.connection = Some(connection);
        
        log::info!("Started MIDI capture for {}", self.config.device_id);
        
        Ok(())
    }
    
    /// Stop capturing and return collected events
    pub fn stop(&mut self) -> Vec<TimestampedMidiEvent> {
        self.connection = None;
        
        let events = std::mem::take(&mut *self.events.lock());
        
        log::info!("Stopped MIDI capture for {}, collected {} events", 
            self.config.device_id, events.len());
        
        events
    }
    
    /// Get the current event count
    pub fn event_count(&self) -> usize {
        self.events.lock().len()
    }
}
