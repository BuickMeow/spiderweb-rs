//! Playback: system MIDI output on Windows/macOS (midir), corresponding to Python files/playback.py.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use midir::{MidiOutput, MidiOutputConnection};

pub const DEFAULT_DEVICE: &str = "Microsoft GS Wavetable Synth";

/// Names of the system's MIDI output devices.
pub fn devices() -> Vec<String> {
    match MidiOutput::new("spiderweb-probe") {
        Ok(out) => out
            .ports()
            .iter()
            .filter_map(|p| out.port_name(p).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Turns note rows into playback events `(tick, is_on, channel, key, velocity)`: only those
/// inside the window, keys above 127 are skipped (256-key mode: the synth only has 128 keys,
/// playback.Player.start), and note-off comes before note-on at the same tick (off first,
/// then on).
pub fn build_events(
    notes: &[[i64; 6]],
    ppq: f64,
    from_beat: f64,
    stop_beat: f64,
) -> Vec<(i64, bool, u8, u8, u8)> {
    let from_tick = (from_beat * ppq) as i64;
    let stop_tick = (stop_beat * ppq) as i64;
    let mut events: Vec<(i64, bool, u8, u8, u8)> = Vec::with_capacity(notes.len() * 2);
    for n in notes {
        if n[2] > 127 {
            continue;
        }
        let (_, ch) = spiderweb_io::midi::slot_track_channel(n[4]);
        let start = n[0].max(from_tick);
        let end = n[1].max(start + 1);
        if end < from_tick || start > stop_tick {
            continue;
        }
        if n[0] >= from_tick {
            events.push((n[0], true, ch, n[2] as u8, n[3] as u8));
        }
        events.push((end.min(stop_tick), false, ch, n[2] as u8, 0));
    }
    events.sort_by_key(|e| (e.0, e.1));
    events
}

/// A player: a background thread sends notes on schedule; supports pause/stop and position queries.
pub struct Player {
    conn: Option<MidiOutputConnection>,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    position: Arc<AtomicI64>, // current tick
    device: String,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            conn: None,
            thread: None,
            running: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            position: Arc::new(AtomicI64::new(0)),
            device: String::new(),
        }
    }
}

impl Player {
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Opens (or switches to) a device; does nothing if the same one is already open. Err is the status-bar message.
    pub fn open(&mut self, name: &str) -> Result<(), String> {
        if self.conn.is_some() && self.device == name {
            return Ok(());
        }
        self.close();
        let out = MidiOutput::new("spiderweb").map_err(|e| e.to_string())?;
        let ports = out.ports();
        let port = ports
            .iter()
            .find(|p| out.port_name(p).map(|n| n == name).unwrap_or(false))
            .or_else(|| ports.first())
            .ok_or_else(|| "No MIDI output device found".to_string())?;
        let conn = out
            .connect(port, "spiderweb-out")
            .map_err(|e| e.to_string())?;
        self.conn = Some(conn);
        self.device = name.to_string();
        Ok(())
    }

    pub fn close(&mut self) {
        self.stop();
        if let Some(c) = self.conn.take() {
            let _ = c.close();
        }
        self.device.clear();
    }

    /// Sends one note immediately (right-drag preview). Keys >127 (256-key mode) cannot be sent: skipped (upstream MidiOut.note).
    pub fn note(&mut self, ch: u8, key: i64, vel: u8) {
        if key > 127 {
            return;
        }
        if let Some(c) = self.conn.as_mut() {
            let _ = c.send(&[0x90 | (ch & 0x0f), key as u8 & 0x7f, vel & 0x7f]);
        }
    }

    /// Plays from from_beat to stop_beat (ticks queued as us; bpm is minutes per beat).
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        notes: &[[i64; 6]],
        ppq: i64,
        bpm: f64,
        from_beat: f64,
        stop_beat: f64,
    ) {
        self.stop();
        if let Err(e) = self.open(&self.device.clone()) {
            let _ = e;
        }
        let Some(mut conn) = self.conn.take() else {
            return;
        };
        let ppq = ppq.max(1) as f64;
        let from_tick = (from_beat * ppq) as i64;
        let us_per_tick = 60_000_000.0 / (bpm.max(1e-6) * ppq);
        let events = build_events(notes, ppq, from_beat, stop_beat);
        let stop = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let position = Arc::new(AtomicI64::new(from_tick));
        self.stop = stop.clone();
        self.running = running.clone();
        self.position = position.clone();
        let handle = std::thread::spawn(move || {
            let t0 = Instant::now();
            for (tick, on, ch, key, vel) in events {
                let target =
                    Duration::from_secs_f64((tick - from_tick).max(0) as f64 * us_per_tick / 1e6);
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let now = t0.elapsed();
                    if now >= target {
                        break;
                    }
                    std::thread::sleep((target - now).min(Duration::from_millis(5)));
                }
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let msg = if on {
                    [0x90 | (ch & 0x0f), key & 0x7f, vel & 0x7f]
                } else {
                    [0x80 | (ch & 0x0f), key & 0x7f, 0]
                };
                if conn.send(&msg).is_err() {
                    break;
                }
                position.store(tick, Ordering::SeqCst);
            }
            // Wrap up: all notes off on every channel
            for ch in 0..16u8 {
                let _ = conn.send(&[0xb0 | ch, 123, 0]);
            }
            running.store(false, Ordering::SeqCst);
        });
        self.thread = Some(handle);
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.stop = Arc::new(AtomicBool::new(false));
    }

    /// Playback position (beats).
    pub fn position(&self, ppq: i64) -> f64 {
        self.position.load(Ordering::SeqCst) as f64 / ppq.max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_skip_keys_above_127() {
        // 256-key mode: keys >127 make no sound (the synth has only 128 keys), <=127 behave normally
        let notes = [
            [0, 960, 200, 100, 0, 0],
            [0, 960, 126, 90, 0, 0],
            [100, 200, 127, 80, 0, 0],
        ];
        let events = build_events(&notes, 960.0, 0.0, 8.0);
        assert!(events.iter().all(|e| e.3 <= 127));
        assert_eq!(
            events,
            vec![
                (0, true, 0, 126, 90),
                (100, true, 0, 127, 80),
                (200, false, 0, 127, 0),
                (960, false, 0, 126, 0),
            ]
        );
    }

    #[test]
    fn events_stay_inside_the_window() {
        // Notes starting inside the window (0.5..1.5 beats) sound; later ones don't
        let notes = [
            [0, 480, 60, 100, 0, 0],
            [960, 1440, 62, 90, 0, 0],
            [1920, 2400, 64, 80, 0, 0],
        ];
        let events = build_events(&notes, 960.0, 0.5, 1.5);
        assert!(events.contains(&(960, true, 0, 62, 90)));
        assert!(events.contains(&(1440, false, 0, 62, 0)));
        assert!(
            events.iter().all(|e| e.3 != 64),
            "notes after the window must not sound"
        );
    }
}
