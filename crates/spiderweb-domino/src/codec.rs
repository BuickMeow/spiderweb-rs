//! MidiPortalSequence 编解码（对应原版 `item` / `clip_data` / `read_notes` 及其辅助函数）。

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::fmt;
use std::io::{Read, Write};

/// 剪贴板数据开头的魔数。
pub const MAGIC: &[u8] = b"PortalSequenceData";

// 原版里从真实 Domino 复制结果抄下来的固定片段，必须逐字节一致。
const SONG_START: &[u8] = &[
    // 1000（空）、1001 = 版权文本（空）
    0xe8, 0x03, 0x00, 0x00, 0x00, 0x00, 0xe9, 0x03, 0x00, 0x00, 0x00, 0x00,
];
const SONG_REST: &[u8] = &[
    // 1002 = PPQ 之后的部分
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

/// 一条 NOTE 记录的字节数（原版 `NOTE.itemsize`）。
const NOTE_REC: usize = 40;

/// 一条输入音符：`[起点 tick, 结束 tick, 音高, 力度, 通道 slot, ...]`，只有前 5 列参与编码
/// （多余的列是给调用方用的，原版同样忽略）。
pub type Note6 = [i64; 6];

/// `clip_data` / `read_notes` 的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// 没有音符可复制（原版对空数组取 min 会报错）。
    Empty,
    /// `bar <= 0`（原版会除零）。
    BadBar,
    /// 长度 / 数据大小超出 u32（原版 `struct.pack` 会抛错）。
    TooLarge,
    /// 不是 Domino 的剪贴板数据（魔数不对或太短）。
    NotDomino,
    /// zlib 数据损坏。
    Damaged,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "没有音符"),
            Error::BadBar => write!(f, "bar 必须大于 0"),
            Error::TooLarge => write!(f, "数据太大，放不进 u32"),
            Error::NotDomino => write!(f, "不是 Domino 的数据"),
            Error::Damaged => write!(f, "Domino 的数据损坏了"),
        }
    }
}

impl std::error::Error for Error {}

/// `[tag u16][length u32][body]`，小端（原版 `item`）。
pub fn item(tag: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + body.len());
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// 原版 `clip_data`：音符行 -> 剪贴板字节。
///
/// 每个有音符的 slot 一轨（按 slot 升序，轨内按 (tick, key) 稳定排序），复制范围从首个
/// 音符之前的小节线到最后一个音符结束之后的小节线（至少 1 小节）。`ppq` 以 u16 写入，
/// 原版 `struct.pack("<H", ppq)` 在超出 `0..=65535` 时会抛错，这里由参数类型保证。
/// `notes` 为空或 `bar <= 0` 时返回错误（原版分别报 numpy / 除零错误）。
pub fn clip_data(notes: &[Note6], ppq: u16, bar: i64) -> Result<Vec<u8>, Error> {
    if notes.is_empty() {
        return Err(Error::Empty);
    }
    if bar <= 0 {
        return Err(Error::BadBar);
    }
    let first_start = notes.iter().map(|n| n[0]).min().ok_or(Error::Empty)?;
    let max_end = notes.iter().map(|n| n[1]).max().ok_or(Error::Empty)?;

    // Python 的 // 是向下取整：first = min(start) // bar * bar
    let first = (first_start as i128).div_euclid(bar as i128) * bar as i128;
    // length = max(ceil((max_end - first) / bar) * bar, bar)
    let span = max_end as i128 - first;
    let ceil_div = -(-span).div_euclid(bar as i128);
    let length = ceil_div.saturating_mul(bar as i128).max(bar as i128);
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
        // 原版 np.lexsort((key, tick))：先按起点，再按音高，且是稳定排序。
        mine.sort_by_key(|n| (n[0], n[2]));
        let mut body = Vec::with_capacity(
            TRACK_HEAD.len() + mine.len() * NOTE_REC + end.len() + TRACK_TAIL.len(),
        );
        body.extend_from_slice(TRACK_HEAD);
        for n in mine {
            // numpy 的赋值会按 C 语义截断（模 2^32 / 2^8），这里用 as 复刻。
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

/// 原版 `read_notes`：剪贴板字节 -> `(行, ppq)`（只保留 0..=127 的键，同原版）。
///
/// 行的布局是 `(tick, gate, key, velocity, track)`，tick 从复制起点算，track 从 0 数；
/// 只取音符（控制器等其它项跳过），gate 至少 1，velocity 夹到 `1..=127`。没有音符时
/// 返回空行。`raw` 不是 Domino 数据或 zlib 损坏时返回错误。
pub fn read_notes(raw: &[u8]) -> Result<(Vec<[i64; 5]>, Option<u16>), Error> {
    read_notes_max_key(raw, 127)
}

/// 同 [`read_notes`]，但键的上限可调：Domino 256k 版用 `max_key = 255`，
/// 音符 item 里的 key 本来就是 u8，格式本身不限制。
pub fn read_notes_max_key(raw: &[u8], max_key: i64) -> Result<(Vec<[i64; 5]>, Option<u16>), Error> {
    if !raw.starts_with(MAGIC) || raw.len() < MAGIC.len() + 4 {
        return Err(Error::NotDomino);
    }
    let data = zlib_decompress(&raw[MAGIC.len() + 4..])?;

    // 原版的 runs 按解析顺序记（轨道顺序，轨内分块顺序），odd 单独攒着最后拼在所有
    // 常规布局音符之后；这里保持一致。
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
                // 常规布局：连续多条一起收。
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
                // 其它布局的音符；不是音符的项（控制器、设置）跳过。
                if let Some(row) = parse_odd_note(&body[i + 6..i + 6 + n], track) {
                    odd.push(row);
                }
            }
            i += 6 + n;
        }
    }

    let mut rows: Vec<[i64; 5]> = Vec::new();
    for (run, tr) in &runs {
        for rec in run.chunks_exact(NOTE_REC) {
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

/// 原版 `items`：从 data 的 i 处逐个取 `(tag, body)`，项放不下就停。
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

/// 原版 `note_run`：从 body 的 i 处起，连续按 NOTE 布局写下的音符（至少第一条是）。
/// 返回每段音符的字节切片和新的位置。
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
        n *= 2; // 一块块地长大：音符之间夹了东西会提前停下
    }
    (got, i)
}

/// NOTE 记录各字段的标志位是否都符合常规布局（原版 note_run 里那串比较）。
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

/// 一条“其它布局”的 2001 音符项：内部按 tag 找字段，缺 2002 时力度按 100 算
/// （原版 `f.get(2002, b"")[:1] or bytes([100])`）。三个必需字段大小不对就不算音符。
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

/// 一条 NOTE 记录（40 字节）写入按常规布局编码的音符项。
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
