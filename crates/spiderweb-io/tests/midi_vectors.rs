//! files/midi_out.py 对照测试（向量由 tools/gen_io_vectors.py 生成）。
//!
//! 首要断言：Rust 写出的文件与 Python 逐字节一致（hex 对照）。另外手写解析 SMF 头，
//! 确认是合法的 format 1（不依赖任何 MIDI 库）。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_io::midi::{CHANNELS, PPQ_WARN, midi_bytes, slot_track_channel, track_data, vlq};

/// 手写解析 SMF 头："MThd"、长度 6、format 1、division = ppq；后面紧跟 "MTrk"。
fn assert_smf_format1(bytes: &[u8], ppq: u16, name: &str) {
    assert!(bytes.len() > 14, "{name}: 文件太短");
    assert_eq!(&bytes[0..4], b"MThd", "{name}: 没有 MThd");
    let hlen = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(hlen, 6, "{name}: MThd 长度");
    let format = u16::from_be_bytes([bytes[8], bytes[9]]);
    assert_eq!(format, 1, "{name}: 不是 format 1");
    let ntrks = u16::from_be_bytes([bytes[10], bytes[11]]);
    assert!(ntrks >= 1, "{name}: 没有轨道");
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    assert_eq!(division, ppq, "{name}: division 不是 PPQ");
    assert_eq!(&bytes[14..18], b"MTrk", "{name}: 第一个块不是 MTrk");
}

fn notes6(v: &Value) -> Vec<[i64; 6]> {
    v.as_array()
        .expect("notes 不是数组")
        .iter()
        .map(|row| {
            let r = row.as_array().expect("音符不是数组");
            let mut n = [0i64; 6];
            for (k, x) in r.iter().enumerate() {
                n[k] = i(x);
            }
            n
        })
        .collect()
}

#[test]
fn midi_byte_vectors() {
    let cases = cases("midi", "cases");
    for case in &cases {
        let name = s(&case["name"]);
        let ppq = case["ppq"].as_u64().expect("ppq") as u16;
        let bpm = f(&case["bpm"]);
        let beats = case["beats"].as_u64().expect("beats") as u8;
        let notes = notes6(&case["notes"]);
        let got = midi_bytes(ppq, bpm, beats, &notes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want = unhex(s(&case["hex"]));
        assert_eq!(got, want, "{name}: MIDI 不是逐字节一致");

        assert_smf_format1(&got, ppq, name);
    }
}

#[test]
fn midi_helpers() {
    assert_eq!(vlq(0), vec![0x00]);
    assert_eq!(vlq(0x7F), vec![0x7F]);
    assert_eq!(vlq(0x80), vec![0x81, 0x00]);
    assert_eq!(vlq(0x3FFF), vec![0xFF, 0x7F]);
    assert_eq!(vlq(0x4000), vec![0x81, 0x80, 0x00]);
    assert_eq!(vlq(0x1FFFFF), vec![0xFF, 0xFF, 0x7F]);
    assert_eq!(vlq(0x200000), vec![0x81, 0x80, 0x80, 0x00]);
    // 每个 slot 一个通道，跳过鼓通道 10（0 起的 9）
    assert_eq!(slot_track_channel(0), (0, CHANNELS[0]));
    assert_eq!(slot_track_channel(9), (9, 10));
    assert_eq!(slot_track_channel(14), (14, 15));
    assert_eq!(slot_track_channel(15), (15, CHANNELS[0]));
    assert_eq!(slot_track_channel(-1), (-1, CHANNELS[14]));
    assert_eq!(PPQ_WARN, 32767);
    // 空轨：只有结束标记
    assert_eq!(track_data(&[], 0), vec![0x00, 0xFF, 0x2F, 0x00]);
    // 同一 tick 上 note-off 在前
    let notes = [[0i64, 0, 60, 100, 0, 0]];
    assert_eq!(
        track_data(&notes, 0),
        vec![
            0x00, 0x80, 60, 0x00, 0x00, 0x90, 60, 100, 0x00, 0xFF, 0x2F, 0x00
        ]
    );
}
