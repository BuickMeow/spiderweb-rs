//! domino_clip.py 对照测试（向量由 tools/gen_domino_vectors.py 生成）。

use flate2::read::ZlibDecoder;
use serde_json::Value;
use spiderweb_domino::{Error, MAGIC, clip_data, read_notes, read_notes_max_key};
use std::io::Read;

fn vectors() -> Value {
    let path = format!("{}/tests/vectors/domino.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("缺少向量文件 {path}（先运行 tools/gen_domino_vectors.py）：{e}")
    });
    serde_json::from_str(&text).expect("向量 JSON 解析失败")
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("不是十六进制"))
        .collect()
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("不是整数: {v}"))
}

fn notes6(v: &Value) -> Vec<[i64; 6]> {
    v.as_array()
        .expect("notes 不是数组")
        .iter()
        .map(|row| {
            let r = row.as_array().expect("notes 行不是数组");
            let mut n = [0i64; 6];
            for (k, x) in r.iter().enumerate() {
                n[k] = i(x);
            }
            n
        })
        .collect()
}

fn rows5(v: &Value) -> Vec<[i64; 5]> {
    v.as_array()
        .expect("rows 不是数组")
        .iter()
        .map(|row| {
            let r = row.as_array().expect("rows 行不是数组");
            [i(&r[0]), i(&r[1]), i(&r[2]), i(&r[3]), i(&r[4])]
        })
        .collect()
}

fn size_field(raw: &[u8]) -> usize {
    u32::from_le_bytes(
        raw[MAGIC.len()..MAGIC.len() + 4]
            .try_into()
            .expect("大小字段"),
    ) as usize
}

fn decompress(raw: &[u8]) -> Vec<u8> {
    let mut dec = ZlibDecoder::new(&raw[MAGIC.len() + 4..]);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).expect("解压失败");
    out
}

fn case_name(case: &Value) -> &str {
    case["name"].as_str().expect("用例没有 name")
}

/// clip_data：Rust 的解压负载必须和 Python 逐字节相同，且 Rust 自己读得回来。
#[test]
fn clip_vectors() {
    let data = vectors();
    let cases = data["clip"].as_array().expect("clip 不是数组");
    for case in cases {
        let name = case_name(case);
        let notes = notes6(&case["notes"]);
        let ppq = case["ppq"].as_u64().expect("ppq") as u16;
        let bar = i(&case["bar"]);
        let py_raw = unhex(case["raw"].as_str().expect("raw"));
        let py_payload = unhex(case["payload"].as_str().expect("payload"));

        let got = clip_data(&notes, ppq, bar).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(got.starts_with(MAGIC), "{name}: 魔数不对");
        assert_eq!(size_field(&got), py_payload.len(), "{name}: 大小字段不对");
        assert_eq!(
            decompress(&got),
            py_payload,
            "{name}: 解压负载与 Python 不同"
        );
        assert_eq!(
            decompress(&py_raw),
            py_payload,
            "{name}: Python 向量自检失败"
        );

        let rust = read_notes(&got).unwrap_or_else(|e| panic!("{name}: 读回失败 {e}"));
        let py = read_notes(&py_raw).unwrap_or_else(|e| panic!("{name}: Python raw 读失败 {e}"));
        assert_eq!(rust, py, "{name}: Rust raw 读回的结果和 Python raw 不同");
    }
    assert!(cases.len() >= 9, "clips 用例太少：{}", cases.len());
}

/// clip_data 的错误语义：空、bar 0、长度超出 u32。
#[test]
fn clip_error_vectors() {
    let data = vectors();
    let cases = data["clip_errors"]
        .as_array()
        .expect("clip_errors 不是数组");
    for case in cases {
        let name = case_name(case);
        let notes = notes6(&case["notes"]);
        let ppq = case["ppq"].as_u64().expect("ppq") as u16;
        let bar = i(&case["bar"]);
        let want = match case["error"].as_str().expect("error") {
            "empty" => Error::Empty,
            "bad_bar" => Error::BadBar,
            "too_large" => Error::TooLarge,
            other => panic!("未知错误种类 {other}"),
        };
        assert_eq!(clip_data(&notes, ppq, bar), Err(want), "{name}");
    }
    assert!(cases.len() >= 3, "clip_errors 用例太少：{}", cases.len());
}

/// read_notes：直接读 Python 生成的 raw（含真实剪贴板布局、坏数据）。
#[test]
fn read_vectors() {
    let data = vectors();
    let cases = data["read"].as_array().expect("read 不是数组");
    for case in cases {
        let name = case_name(case);
        let raw = unhex(case["raw"].as_str().expect("raw"));
        if let Some(kind) = case["error"].as_str() {
            let want = match kind {
                "not_domino" => Error::NotDomino,
                "damaged" => Error::Damaged,
                other => panic!("未知错误种类 {other}"),
            };
            assert_eq!(read_notes(&raw), Err(want), "{name}");
        } else {
            let (rows, ppq) = read_notes(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(rows, rows5(&case["rows"]), "{name}");
            let want_ppq = case["ppq"].as_u64().map(|p| p as u16);
            assert_eq!(ppq, want_ppq, "{name}");
        }
    }
    assert!(cases.len() >= 30, "read 用例太少：{}", cases.len());
}

/// 非 Windows 平台的剪贴板空实现。
#[cfg(not(windows))]
#[test]
fn clipboard_stub() {
    assert!(!spiderweb_domino::put_on_clipboard(b"x"));
    assert_eq!(
        spiderweb_domino::get_from_clipboard(),
        spiderweb_domino::ClipboardGet::NoData
    );
}

#[test]
fn high_keys_round_trip_with_max_key() {
    // 音符 item 的 key 是 u8：key 200 编码得下；默认读取按原版丢 >127，
    // 256k 档（max_key = 255）能原样读回。
    let notes = vec![[0i64, 100, 200, 80, 0, 0]];
    let raw = clip_data(&notes, 960, 3840).expect("编码");
    let (rows, _) = read_notes(&raw).expect("读取");
    assert!(rows.iter().all(|r| r[2] != 200), "默认应丢弃 >127 的键");
    let (rows, _) = read_notes_max_key(&raw, 255).expect("读取");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][2], 200);
}
