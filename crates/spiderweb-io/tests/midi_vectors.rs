//! files/midi_out.py 对照测试（向量由 tools/gen_io_vectors.py 生成）。
//!
//! 首要断言：Rust 写出的文件与 Python 逐字节一致（hex 对照）。另外用 midly 解析一遍，
//! 确认是合法的 SMF format 1。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_io::midi::{CHANNELS, PPQ_WARN, midi_bytes, slot_track_channel, track_data, vlq};

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

        // midly 能解析：format 1，第一轨是速度 / 拍号
        let smf = midly::Smf::parse(&got).unwrap_or_else(|e| panic!("{name}: midly 解析失败: {e}"));
        assert_eq!(smf.header.format, midly::Format::Parallel, "{name}: format");
        assert_eq!(
            smf.header.timing,
            midly::Timing::Metrical(ppq.into()),
            "{name}: ppq"
        );
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
