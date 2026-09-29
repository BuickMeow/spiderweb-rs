//! Standard MIDI file (format 1) writing (byte-for-byte port of Python `files/midi_out.py`).
//!
//! Takes the final `(start, end, key, velocity, slot, owner)` note rows (ticks, produced by
//! `engine.render`) plus PPQ / BPM / beats per bar, one track per slot and one channel per
//! track (skipping drum channel 10). The file layout matches the Python version byte for
//! byte: MThd + tempo / time signature / end-of-track first track (track 0) + one track per
//! slot; at the same tick note-offs come first, the rest keeps note order, and deltas are
//! VLQ-encoded.

use std::io;
use std::path::Path;

use spiderweb_core::round_half_even;

use crate::safefile;

/// Many MIDI programs cannot open a PPQ this high (it is still written).
pub const PPQ_WARN: u16 = 32767;

/// All MIDI channels except drum channel 10 (9 zero-based) (engine.CHANNELS).
pub const CHANNELS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];

/// slot -> (track number, MIDI channel), one track per channel, skipping the drum channel (engine.slot_track_channel).
pub fn slot_track_channel(slot: i64) -> (i64, u8) {
    (
        slot,
        CHANNELS[slot.rem_euclid(CHANNELS.len() as i64) as usize],
    )
}

/// Write error.
#[derive(Debug, thiserror::Error)]
pub enum MidiError {
    #[error("tempo out of range")]
    Tempo,
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Variable-length quantity (midi_out.vlq).
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

/// Events for one track (one note-on and one note-off per note) + end-of-track marker (midi_out.track_data).
///
/// Events are sorted by time, note-offs come first at the same tick, the rest keeps the
/// original note order; note-off velocity is 0.
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

/// Bytes of the whole MIDI file (in-memory version of midi_out.write_midi).
///
/// notes: `[start, end, pitch, velocity, slot, owner]`, one track per slot.
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

/// Write a MIDI file (atomic write, midi_out.write_midi).
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
