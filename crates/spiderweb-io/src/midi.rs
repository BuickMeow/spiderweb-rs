//! 标准 MIDI 文件（format 1）写出（Python `files/midi_out.py` 的逐字节移植）。
//!
//! 接收最终的 `(start, end, key, velocity, slot, owner)` 音符行（tick，`engine.render` 的
//! 产物）与 PPQ / BPM / 每小节拍数，每个 slot 一轨、每轨一个通道（跳过鼓通道 10）。
//! 文件布局与 Python 版逐字节一致：MThd + 速度 / 拍号 / 结束的第一轨（轨 0）+ 每个 slot 一轨；
//! 同一 tick 上 note-off 在前，其余按音符顺序，时值用 VLQ 编码。

use std::io;
use std::path::Path;

use spiderweb_core::round_half_even;

use crate::safefile;

/// 这么高的 PPQ 很多 MIDI 程序打不开（仍会写出）。
pub const PPQ_WARN: u16 = 32767;

/// 除鼓通道 10（0 起为 9）外的所有 MIDI 通道（engine.CHANNELS）。
pub const CHANNELS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];

/// slot -> (轨号, MIDI 通道)，每通道一轨，跳过鼓通道（engine.slot_track_channel）。
pub fn slot_track_channel(slot: i64) -> (i64, u8) {
    (
        slot,
        CHANNELS[slot.rem_euclid(CHANNELS.len() as i64) as usize],
    )
}

/// 写出错误。
#[derive(Debug, thiserror::Error)]
pub enum MidiError {
    #[error("tempo out of range")]
    Tempo,
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// 可变长数值（midi_out.vlq）。
pub fn vlq(value: u64) -> Vec<u8> {
    let mut out = vec![(value & 0x7F) as u8];
    let mut value = value >> 7;
    while value != 0 {
        out.push(((value & 0x7F) as u8) | 0x80);
        value >>= 7;
    }
    out.reverse();
    out
}

struct Event {
    tick: i64,
    on: bool,
    idx: usize,
    key: i64,
    vel: i64,
}

/// 一轨的事件（每个音符一个 note-on 一个 note-off）+ 结束标记（midi_out.track_data）。
///
/// 事件按时间排序，同一 tick 上 note-off 在前，其余按音符原本的顺序；note-off 的力度为 0。
pub fn track_data(notes: &[[i64; 6]], ch: u8) -> Vec<u8> {
    let mut events: Vec<Event> = Vec::with_capacity(notes.len() * 2);
    for (i, note) in notes.iter().enumerate() {
        events.push(Event {
            tick: note[0],
            on: true,
            idx: 2 * i,
            key: note[2],
            vel: note[3],
        });
        events.push(Event {
            tick: note[1],
            on: false,
            idx: 2 * i + 1,
            key: note[2],
            vel: 0,
        });
    }
    events.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then(a.on.cmp(&b.on))
            .then(a.idx.cmp(&b.idx))
    });
    if events.is_empty() {
        let mut out = vlq(0);
        out.extend_from_slice(&[0xFF, 0x2F, 0x00]);
        return out;
    }
    let ticks: Vec<i64> = events.iter().map(|e| e.tick).collect();
    let mut deltas: Vec<i64> = Vec::with_capacity(ticks.len());
    let mut prev = 0i64;
    for &t in &ticks {
        deltas.push(t - prev);
        prev = t;
    }
    let sizes: Vec<usize> = deltas
        .iter()
        .map(|&d| 1 + [7, 14, 21, 28].iter().filter(|&&b| d >= 1i64 << b).count())
        .collect();
    let total: usize = sizes.iter().map(|s| s + 3).sum();
    let mut out = vec![0u8; total];
    let mut at = 0usize;
    for (i, (&delta, &size)) in deltas.iter().zip(sizes.iter()).enumerate() {
        for k in 0..size {
            let left = size - 1 - k;
            out[at + k] = ((delta >> (7 * left)) & 0x7F) as u8 | if left > 0 { 0x80 } else { 0 };
        }
        out[at + size] = if events[i].on { 0x90 | ch } else { 0x80 | ch };
        out[at + size + 1] = events[i].key as u8;
        out[at + size + 2] = events[i].vel as u8;
        at += size + 3;
    }
    out.extend_from_slice(&vlq(0));
    out.extend_from_slice(&[0xFF, 0x2F, 0x00]);
    out
}

fn chunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// 整个 MIDI 文件的字节（midi_out.write_midi 的内存版）。
///
/// notes：`[start, end, pitch, velocity, slot, owner]`，每个 slot 一轨。
pub fn midi_bytes(ppq: u16, bpm: f64, beats: u8, notes: &[[i64; 6]]) -> Result<Vec<u8>, MidiError> {
    let micros = round_half_even(60_000_000.0 / bpm) as i64;
    if !(0..=0xFF_FFFF).contains(&micros) {
        return Err(MidiError::Tempo);
    }
    let mut head = Vec::new();
    head.extend_from_slice(&vlq(0));
    head.extend_from_slice(&[0xFF, 0x51, 0x03]);
    head.extend_from_slice(&micros.to_be_bytes()[5..8]);
    head.extend_from_slice(&vlq(0));
    head.extend_from_slice(&[0xFF, 0x58, 4, beats, 2, 24, 8]);
    head.extend_from_slice(&vlq(0));
    head.extend_from_slice(&[0xFF, 0x2F, 0x00]);

    let mut chunks = vec![chunk(&head)];
    let tracks = match notes.iter().map(|n| n[4]).max() {
        Some(max) => (max + 1).max(0),
        None => 0,
    };
    for track in 0..tracks {
        let data: Vec<[i64; 6]> = notes.iter().copied().filter(|n| n[4] == track).collect();
        chunks.push(chunk(&track_data(&data, slot_track_channel(track).1)));
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(chunks.len() as u16).to_be_bytes());
    out.extend_from_slice(&ppq.to_be_bytes());
    for c in &chunks {
        out.extend_from_slice(c);
    }
    Ok(out)
}

/// 写出 MIDI 文件（原子写，midi_out.write_midi）。
pub fn write_midi(
    path: &Path,
    ppq: u16,
    bpm: f64,
    beats: u8,
    notes: &[[i64; 6]],
) -> Result<(), MidiError> {
    safefile::write_bytes(path, &midi_bytes(ppq, bpm, beats, notes)?)?;
    Ok(())
}
