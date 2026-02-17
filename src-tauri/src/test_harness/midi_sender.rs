use midir::{MidiOutput, MidiOutputConnection};
use std::time::Duration;

/// Sends MIDI messages through a virtual loopback device (e.g. LoopBe1).
pub struct MidiSender {
    connection: MidiOutputConnection,
}

impl MidiSender {
    /// Connect to a MIDI output port whose name contains `name_contains`.
    /// For LoopBe1, the output port appears alongside the input port.
    pub fn connect(name_contains: &str) -> Option<Self> {
        let midi_out = MidiOutput::new("sacho-test-sender").ok()?;
        let ports = midi_out.ports();

        for port in &ports {
            if let Ok(name) = midi_out.port_name(port) {
                if name.to_lowercase().contains(&name_contains.to_lowercase()) {
                    println!("  MidiSender: connecting to output port '{}'", name);
                    match midi_out.connect(port, "sacho-test-out") {
                        Ok(conn) => return Some(Self { connection: conn }),
                        Err(e) => {
                            println!("  MidiSender: failed to connect: {}", e);
                            return None;
                        }
                    }
                }
            }
        }

        println!("  MidiSender: no output port matching '{}'", name_contains);
        None
    }

    /// Send a Note On message.
    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let msg = [0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F];
        let _ = self.connection.send(&msg);
    }

    /// Send a Note Off message.
    pub fn note_off(&mut self, channel: u8, note: u8) {
        let msg = [0x80 | (channel & 0x0F), note & 0x7F, 0];
        let _ = self.connection.send(&msg);
    }

    /// Play a single note for the given duration, then release.
    pub fn play_note(&mut self, note: u8, duration: Duration) {
        self.note_on(0, note, 100);
        std::thread::sleep(duration);
        self.note_off(0, note);
    }

    /// Play a sequence of notes: (note, hold_duration, gap_after).
    pub fn play_sequence(&mut self, notes: &[(u8, Duration, Duration)]) {
        for &(note, hold, gap) in notes {
            self.note_on(0, note, 100);
            std::thread::sleep(hold);
            self.note_off(0, note);
            if !gap.is_zero() {
                std::thread::sleep(gap);
            }
        }
    }

    /// Send periodic notes to keep the recording alive.
    /// Sends a note every `interval` for `total` duration.
    pub fn keep_alive(&mut self, interval: Duration, total: Duration) {
        let start = std::time::Instant::now();
        while start.elapsed() < total {
            self.note_on(0, 60, 80);
            std::thread::sleep(Duration::from_millis(50));
            self.note_off(0, 60);
            let remaining = total.saturating_sub(start.elapsed());
            let sleep = interval.min(remaining);
            if sleep.is_zero() {
                break;
            }
            std::thread::sleep(sleep);
        }
    }
}
