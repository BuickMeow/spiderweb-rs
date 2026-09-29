//! MidiPortalSequence encode/decode (corresponds to the original `item` / `clip_data` /
//! `read_notes` and their helpers).

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::fmt;
use std::io::{Read, Write};

/// Magic number at the start of clipboard data.
pub const MAGIC: &[u8] = b"PortalSequenceData";

// Fixed fragments transcribed from real Domino copy results in the original; must match byte for byte.
const SONG_START: &[u8] = &[
    // 1000 (empty), 1001 = copyright text (empty)
    0xe8, 0x03, 0x00, 0x00, 0x00, 0x00, 0xe9, 0x03, 0x00, 0x00, 0x00, 0x00,
];
const SONG_REST: &[u8] = &[
    // 1002 = the part after PPQ
    0xef, 0x03, 0x04, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00, 0xf1, 0x03, 0x04, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xf4, 0x03, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0xf5, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6, 0x03, 0x04, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfb, 0x03, 0x00, 0x00, 0x00, 0x00, 0xfc, 0x03, 0x04, 0x00,
    0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xfd, 0x03, 0x01, 0x00, 0x00, 0x00, 0x01, 0xfe, 0x03, 0x01,
    0x00, 0x00, 0x00, 0x01, 0xff, 0x03, 0x04, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x00, 0x04,
    0x01, 0x00, 0x00, 0x00, 0x01,
];
const TRACK_HEAD: &[u8] = &[
    0xe8, 0x03, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xe9, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0xea,
    0x03, 0x00, 0x00, 0x00, 0x00, 0xeb, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0xec, 0x03, 0x01, 0x00,
    0x00, 0x00, 0x00, 0xf0, 0x03, 0x01, 0x00, 0x00, 0x00, 0x3c, 0xf1, 0x03, 0x11, 0x00, 0x00, 0x00,
    0x47, 0x65, 0x6e, 0x65, 0x72, 0x61, 0x6c, 0x20, 0x4d, 0x49, 0x44, 0x49, 0x20, 0x44, 0x72, 0x75,
    0x6d, 0xf3, 0x03, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf4, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0xf8, 0x03, 0x04, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0xf9, 0x03, 0x04,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf5, 0x03, 0x01, 0x00, 0x00, 0x00, 0x01, 0xf6, 0x03,
    0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0xf7, 0x03, 0x01, 0x00, 0x00, 0x00, 0x01, 0xfa, 0x03, 0x01,
    0x00, 0x00, 0x00, 0xff, 0xfb, 0x03, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xfc, 0x03,
    0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xfd, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0xfe, 0x03, 0x01, 0x00, 0x00,
    0x00, 0x7f,
];
const TRACK_TAIL: &[u8] = &[
    0xed, 0x03, 0x04, 0x00, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00, 0xee, 0x03, 0x01, 0x00, 0x00, 0x00,
    0x64, 0xef, 0x03, 0x04, 0x00, 0x00, 0x00, 0xe0, 0x01, 0x00, 0x00, 0xf2, 0x03, 0x0e, 0x00, 0x00,
    0x00, 0xe8, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0xe9, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00,
];
const SONG_TAIL: &[u8] = &[
    0xee, 0x03, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x03, 0x1a, 0x00, 0x00, 0x00, 0xe8, 0x03, 0x00, 0x00,
    0x00, 0x00, 0xe9, 0x03, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xea, 0x03, 0x04, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xf9, 0x03, 0x42, 0x00, 0x00, 0x00, 0x64, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x65, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x66, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x67, 0x00, 0x04, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x68, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x69, 0x00, 0x04, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x6a, 0x00, 0x0c, 0x00,
    0x00, 0x00, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
];

/// Size of one NOTE record in bytes (the original's `NOTE.itemsize`).
const NOTE_REC: usize = 40;

/// One input note: `[start tick, end tick, pitch, velocity, channel slot, ...]`; only the
/// first 5 columns take part in encoding (extra columns are for the caller; the original
/// ignores them too).
pub type Note6 = [i64; 6];

/// Where copying / pasting starts (the saved values of upstream `DOMINO_STARTS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DominoStart {
    /// `"note"`: the first note is at tick 0 (copying drops the empty lead, pasting puts the
    /// first note on the play line).
    Note,
    /// `"bar"`: start at the bar line before the first note, keeping the empty lead (the
    /// default of upstream `clip_data`).
    #[default]
    Bar,
}

impl DominoStart {
    /// The saved value used by project files / the dropdown (`"note"` / `"bar"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DominoStart::Note => "note",
            DominoStart::Bar => "bar",
        }
    }

    /// Saved value -> start; None for anything unknown.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "note" => Some(DominoStart::Note),
            "bar" => Some(DominoStart::Bar),
            _ => None,
        }
    }
}

/// Errors of `clip_data` / `read_notes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No notes to copy (the original's min on an empty array raises).
    Empty,
    /// `bar <= 0` (the original divides by zero).
    BadBar,
    /// Length / data size exceeds u32 (the original's `struct.pack` raises).
    TooLarge,
    /// Not Domino clipboard data (bad magic or too short).
    NotDomino,
    /// Corrupted zlib data.
    Damaged,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "no notes"),
            Error::BadBar => write!(f, "bar must be greater than 0"),
            Error::TooLarge => write!(f, "data too large for u32"),
            Error::NotDomino => write!(f, "not Domino data"),
            Error::Damaged => write!(f, "Domino data is damaged"),
        }
    }
}

impl std::error::Error for Error {}

/// `[tag u16][length u32][body]`, little-endian (the original's `item`).
pub fn item(tag: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + body.len());
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Upstream `clip_data`: note rows -> clipboard bytes.
///
/// One track per slot that has notes (slot order, notes sorted by (tick, key) like the
/// original). `start`: [`DominoStart::Bar`] runs from the bar line before the first note to
/// the bar line after the last (at least one bar, the original default);
/// [`DominoStart::Note`] runs from the first note to the last, that stretch being the length
/// (first note at tick 0). `ppq` goes in as a u16 (`struct.pack("<H", ppq)` raises when it
/// doesn't fit `0..=65535`; the parameter type guarantees that here). Empty `notes` is an
/// error; with the `bar` start `bar <= 0` is one too (numpy / division by zero upstream).
/// The `note` start never uses `bar`, so it isn't checked there, like the original.
pub fn clip_data(
    notes: &[Note6],
    ppq: u16,
    bar: i64,
    start: DominoStart,
) -> Result<Vec<u8>, Error> {
    if notes.is_empty() {
        return Err(Error::Empty);
    }
    let first_start = notes.iter().map(|n| n[0]).min().ok_or(Error::Empty)?;
    let max_end = notes.iter().map(|n| n[1]).max().ok_or(Error::Empty)?;

    let (first, length) = match start {
        DominoStart::Note => {
            // first = min(start); length = max(max_end - first, 1)
            let first = first_start as i128;
            (first, (max_end as i128 - first).max(1))
        }
        DominoStart::Bar => {
            if bar <= 0 {
                return Err(Error::BadBar);
            }
            // Python's // rounds down: first = min(start) // bar * bar
            let first = (first_start as i128).div_euclid(bar as i128) * bar as i128;
            // length = max(ceil((max_end - first) / bar) * bar, bar)
            let span = max_end as i128 - first;
            let ceil_div = -(-span).div_euclid(bar as i128);
            (first, ceil_div.saturating_mul(bar as i128).max(bar as i128))
        }
    };
    if length > u32::MAX as i128 {
        return Err(Error::TooLarge);
    }
    let length = length as u32;

    let end = item(2009, &item(1001, &length.to_le_bytes()));

    let mut slots: Vec<i64> = notes.iter().map(|n| n[4]).collect();
    slots.sort_unstable();
    slots.dedup();

    let mut data = Vec::new();
    data.extend_from_slice(SONG_START);
    data.extend_from_slice(&item(1002, &ppq.to_le_bytes()));
    data.extend_from_slice(SONG_REST);
    for slot in slots {
        let mut mine: Vec<&Note6> = notes.iter().filter(|n| n[4] == slot).collect();
        // The original's np.lexsort((key, tick)): by start first, then pitch, and stable.
        mine.sort_by_key(|n| (n[0], n[2]));
        let mut body = Vec::with_capacity(
            TRACK_HEAD.len() + mine.len() * NOTE_REC + end.len() + TRACK_TAIL.len(),
        );
        body.extend_from_slice(TRACK_HEAD);
        for n in mine {
            // numpy assignment truncates with C semantics (mod 2^32 / 2^8); `as` replicates that here.
            let tick = ((n[0] as i128) - first) as u32;
            let key = n[2] as u8;
            let vel = n[3] as u8;
            let gate = ((n[1] as i128) - (n[0] as i128)) as u32;
            push_note(&mut body, tick, key, vel, gate);
        }
        body.extend_from_slice(&end);
        body.extend_from_slice(TRACK_TAIL);
        data.extend_from_slice(&item(1003, &body));
    }
    data.extend_from_slice(SONG_TAIL);

    let size = u32::try_from(data.len()).map_err(|_| Error::TooLarge)?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&zlib_compress(&data)?);
    Ok(out)
}

/// The original `read_notes`: clipboard bytes -> `(rows, ppq)` (only keys 0..=127 are kept,
/// as in the original).
///
/// A row is `(tick, gate, key, velocity, track)`, tick measured from the copy start and track
/// counted from 0; only notes are taken (other items such as controllers are skipped), gate is
/// at least 1 and velocity is clamped to `1..=127`. No notes gives empty rows. When `raw` is
/// not Domino data or the zlib data is corrupt, an error is returned.
pub fn read_notes(raw: &[u8]) -> Result<(Vec<[i64; 5]>, Option<u16>), Error> {
    read_notes_max_key(raw, 127)
}

/// Like [`read_notes`], but with an adjustable key ceiling: the Domino 256k version uses
/// `max_key = 255`; the key in a note item is already a u8, so the format itself has no limit.
pub fn read_notes_max_key(raw: &[u8], max_key: i64) -> Result<(Vec<[i64; 5]>, Option<u16>), Error> {
    if !raw.starts_with(MAGIC) || raw.len() < MAGIC.len() + 4 {
        return Err(Error::NotDomino);
    }
    let data = zlib_decompress(&raw[MAGIC.len() + 4..])?;

    // The original records runs in parse order (track order, chunk order within a track) and
    // collects odd separately, appending it after all regular-layout notes; this matches that.
    let mut runs: Vec<(&[u8], i64)> = Vec::new();
    let mut odd: Vec<[i64; 5]> = Vec::new();
    let mut ppq = None;
    let mut track: i64 = -1;

    for (tag, body) in items(&data) {
        if tag == 1002 && body.len() == 2 {
            ppq = Some(u16::from_le_bytes([body[0], body[1]]));
        }
        if tag != 1003 {
            continue;
        }
        track += 1;
        let mut i = 0usize;
        while i + 6 <= body.len() {
            let t = le16(body, i);
            let n = le32(body, i + 2) as usize;
            if t == 2001 && n == NOTE_REC - 6 {
                // Regular layout: gather consecutive records together.
                let (got, next) = note_run(body, i);
                i = next;
                let any = got.iter().any(|r| !r.is_empty());
                for r in got {
                    runs.push((r, track));
                }
                if any {
                    continue;
                }
            }
            if i + 6 + n > body.len() {
                break;
            }
            if t == 2001 {
                // Notes in another layout; non-note items (controllers, settings) are skipped.
                if let Some(row) = parse_odd_note(&body[i + 6..i + 6 + n], track) {
                    odd.push(row);
                }
            }
            i += 6 + n;
        }
    }

    let mut rows: Vec<[i64; 5]> = Vec::new();
    for (run, tr) in &runs {
        for rec in run.as_chunks::<NOTE_REC>().0 {
            rows.push([
                le32(rec, 12) as i64,
                le32(rec, 36) as i64,
                rec[22] as i64,
                rec[29] as i64,
                *tr,
            ]);
        }
    }
    rows.extend(odd);
    rows.retain(|r| r[2] <= max_key);
    for r in &mut rows {
        r[1] = r[1].max(1);
        r[3] = r[3].clamp(1, 127);
    }
    Ok((rows, ppq))
}

/// The original `items`: take `(tag, body)` one by one from position i in data, stopping when an item doesn't fit.
fn items(data: &[u8]) -> Items<'_> {
    Items { data, i: 0 }
}

struct Items<'a> {
    data: &'a [u8],
    i: usize,
}

impl<'a> Iterator for Items<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i + 6 > self.data.len() {
            return None;
        }
        let tag = le16(self.data, self.i);
        let n = le32(self.data, self.i + 2) as usize;
        if self.i + 6 + n > self.data.len() {
            return None;
        }
        let body = &self.data[self.i + 6..self.i + 6 + n];
        self.i += 6 + n;
        Some((tag, body))
    }
}

/// The original `note_run`: consecutive notes written in the NOTE layout starting at position
/// i in body (at least the first one is). Returns the byte slices of each run and the new position.
fn note_run(body: &[u8], mut i: usize) -> (Vec<&[u8]>, usize) {
    let mut got: Vec<&[u8]> = Vec::new();
    let mut n = 64usize;
    while i + NOTE_REC <= body.len() {
        let count = n.min((body.len() - i) / NOTE_REC);
        let run = (0..count)
            .position(|k| !is_usual_note(&body[i + k * NOTE_REC..i + (k + 1) * NOTE_REC]))
            .unwrap_or(count);
        got.push(&body[i..i + run * NOTE_REC]);
        i += run * NOTE_REC;
        if run < count {
            break;
        }
        n *= 2; // grow in chunks: something between notes makes it stop early
    }
    (got, i)
}

/// Whether the flags of every NOTE record field match the regular layout (the comparisons in the original's note_run).
fn is_usual_note(r: &[u8]) -> bool {
    le16(r, 0) == 2001
        && le32(r, 2) == NOTE_REC as u32 - 6
        && le16(r, 6) == 1001
        && le32(r, 8) == 4
        && le16(r, 16) == 2001
        && le32(r, 18) == 1
        && le16(r, 23) == 2002
        && le32(r, 25) == 1
        && le16(r, 30) == 2003
        && le32(r, 32) == 4
}

/// One "other layout" 2001 note item: fields are found by tag inside; a missing 2002 means
/// velocity 100 (the original's `f.get(2002, b"")[:1] or bytes([100])`). If the three required
/// fields have the wrong sizes, it is not a note.
fn parse_odd_note(body: &[u8], track: i64) -> Option<[i64; 5]> {
    let mut tick = None;
    let mut key = None;
    let mut vel = None;
    let mut gate = None;
    for (t, b) in items(body) {
        match t {
            1001 => tick = Some(b),
            2001 => key = Some(b),
            2002 => vel = Some(b),
            2003 => gate = Some(b),
            _ => {}
        }
    }
    let tick = tick?;
    let key = key?;
    let gate = gate?;
    if tick.len() != 4 || key.len() != 1 || gate.len() != 4 {
        return None;
    }
    let vel = vel.and_then(|v| v.first().copied()).unwrap_or(100);
    Some([
        le32(tick, 0) as i64,
        le32(gate, 0) as i64,
        key[0] as i64,
        vel as i64,
        track,
    ])
}

/// Write one NOTE record (40 bytes) as a note item encoded in the regular layout.
fn push_note(out: &mut Vec<u8>, tick: u32, key: u8, vel: u8, gate: u32) {
    out.extend_from_slice(&2001u16.to_le_bytes());
    out.extend_from_slice(&(NOTE_REC as u32 - 6).to_le_bytes());
    out.extend_from_slice(&1001u16.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&tick.to_le_bytes());
    out.extend_from_slice(&2001u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(key);
    out.extend_from_slice(&2002u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(vel);
    out.extend_from_slice(&2003u16.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&gate.to_le_bytes());
}

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn zlib_compress(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).map_err(|_| Error::Damaged)?;
    enc.finish().map_err(|_| Error::Damaged)
}

fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut dec = ZlibDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).map_err(|_| Error::Damaged)?;
    Ok(out)
}
