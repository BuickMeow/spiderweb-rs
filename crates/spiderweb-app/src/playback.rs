//! 播放：Windows/macOS 系统 MIDI 输出（midir），对应 Python files/playback.py。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use midir::{MidiOutput, MidiOutputConnection};

pub const DEFAULT_DEVICE: &str = "Microsoft GS Wavetable Synth";

/// 系统上的 MIDI 输出设备名。
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

/// 一个播放器：后台线程按时间发送音符，支持暂停/停止与位置查询。
pub struct Player {
    conn: Option<MidiOutputConnection>,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    position: Arc<AtomicI64>, // 当前 tick
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

    /// 打开（或换到）设备；已经打开同一个就什么都不做。Err 是给状态栏的消息。
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

    /// 立刻发一个音符（右拖试听）。
    #[allow(dead_code)] // 右拖试听待移植
    pub fn note(&mut self, ch: u8, key: u8, vel: u8) {
        if let Some(c) = self.conn.as_mut() {
            let _ = c.send(&[0x90 | (ch & 0x0f), key & 0x7f, vel & 0x7f]);
        }
    }

    /// 从 from_beat 开始播到 stop_beat（ticks 转 us 排队；bpm 为每拍的分钟数）。
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
        let stop_tick = (stop_beat * ppq) as i64;
        let us_per_tick = 60_000_000.0 / (bpm.max(1e-6) * ppq);
        // (tick, is_on, channel, key, vel)
        let mut events: Vec<(i64, bool, u8, u8, u8)> = Vec::with_capacity(notes.len() * 2);
        for n in notes {
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
        events.sort_by_key(|e| (e.0, e.1)); // 同一 tick：先关后开
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
            // 收尾：所有通道 all notes off
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

    /// 播放位置（beat）。
    pub fn position(&self, ppq: i64) -> f64 {
        self.position.load(Ordering::SeqCst) as f64 / ppq.max(1) as f64
    }
}
