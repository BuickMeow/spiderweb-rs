//! tumour.py 对照测试（向量由 tools/gen_tumour_vectors.py 生成）。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_core::shape::{Tumour, TumourShape, TumourSide, TumourWrap};
use spiderweb_core::tumour as T;

/// 把 JSON 里的数字先包成 `"@<token>"` 字符串。
///
/// serde_json 默认的浮点解析没有正确舍入（`float_roundtrip` 特性未开），会把 Python 原版
/// 算好的坐标改掉 1 ulp；而 tumour 在拐角处逐位敏感（距离与拐点比较 `< 1e-12`），差一点就会
/// 走错分支。所以这里绕开它，最后用 Rust 正确舍入的 `str::parse` 还原数字。
fn mark_numbers(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(text.len() + text.len() / 8);
    let mut i = 0;
    let mut in_str = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
        } else if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if c == b'-' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                i += 1;
            }
            out.extend_from_slice(b"\"@");
            out.extend_from_slice(&bytes[start..i]);
            out.push(b'"');
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).expect("标记后的 JSON 不是 UTF-8")
}

/// 把 `"@<token>"` 还原成精确解析的数字。
fn unmark_numbers(v: &mut Value) {
    match v {
        Value::String(s) if s.starts_with('@') => {
            let t = &s[1..];
            let n = if t.contains(['.', 'e', 'E']) {
                serde_json::Number::from_f64(t.parse::<f64>().expect("坏浮点")).expect("非有限浮点")
            } else if let Ok(i) = t.parse::<i64>() {
                serde_json::Number::from(i)
            } else {
                serde_json::Number::from(t.parse::<u64>().expect("坏整数"))
            };
            *v = Value::Number(n);
        }
        Value::Array(a) => {
            for x in a {
                unmark_numbers(x);
            }
        }
        Value::Object(o) => {
            for (_, x) in o.iter_mut() {
                unmark_numbers(x);
            }
        }
        _ => {}
    }
}

fn cases_exact(module: &str) -> Vec<Value> {
    let path = format!("{}/tests/vectors/{module}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("缺少向量文件 {path}（先运行 tools/gen_{module}_vectors.py）：{e}")
    });
    let mut data: Value = serde_json::from_str(&mark_numbers(&text)).expect("向量 JSON 解析失败");
    unmark_numbers(&mut data);
    data["cases"].as_array().expect("cases 不是数组").clone()
}

fn close(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
}

fn shape_of(name: &str) -> TumourShape {
    match name {
        "triangle" => TumourShape::Triangle,
        "square" => TumourShape::Square,
        "circle" => TumourShape::Circle,
        "parabola" => TumourShape::Parabola,
        other => panic!("未知 shape {other}"),
    }
}

fn shape_name(shape: TumourShape) -> &'static str {
    match shape {
        TumourShape::Triangle => "triangle",
        TumourShape::Square => "square",
        TumourShape::Circle => "circle",
        TumourShape::Parabola => "parabola",
    }
}

fn side_name(side: TumourSide) -> &'static str {
    match side {
        TumourSide::Alt => "alt",
        TumourSide::Left => "left",
        TumourSide::Right => "right",
        TumourSide::Random => "random",
    }
}

fn wrap_name(wrap: TumourWrap) -> &'static str {
    match wrap {
        TumourWrap::Simple => "simple",
        TumourWrap::Wrap => "wrap",
    }
}

fn assert_tumour_eq(got: Option<Tumour>, want: &Value, ctx: &str) {
    if want.is_null() {
        assert!(got.is_none(), "{ctx}: 应为 None");
        return;
    }
    let got = got.unwrap_or_else(|| panic!("{ctx}: 不应为 None"));
    assert_eq!(
        shape_name(got.shape),
        want["shape"].as_str().unwrap(),
        "{ctx} shape"
    );
    assert_eq!(
        side_name(got.side),
        want["side"].as_str().unwrap(),
        "{ctx} side"
    );
    assert_eq!(
        wrap_name(got.wrap),
        want["wrap"].as_str().unwrap(),
        "{ctx} wrap"
    );
    assert!(
        close(got.size, f(&want["size"])),
        "{ctx} size: {} != {}",
        got.size,
        f(&want["size"])
    );
    assert!(
        close(got.length, f(&want["length"])),
        "{ctx} length: {} != {}",
        got.length,
        f(&want["length"])
    );
    assert!(
        close(got.dist, f(&want["dist"])),
        "{ctx} dist: {} != {}",
        got.dist,
        f(&want["dist"])
    );
    assert!(
        close(got.start, f(&want["start"])),
        "{ctx} start: {} != {}",
        got.start,
        f(&want["start"])
    );
    assert!(
        close(got.end, f(&want["end"])),
        "{ctx} end: {} != {}",
        got.end,
        f(&want["end"])
    );
    assert!(
        close(got.ease, f(&want["ease"])),
        "{ctx} ease: {} != {}",
        got.ease,
        f(&want["ease"])
    );
    assert!(
        close(got.k, f(&want["k"])),
        "{ctx} k: {} != {}",
        got.k,
        f(&want["k"])
    );
    assert_eq!(got.seed, i(&want["seed"]), "{ctx} seed");
    assert_eq!(got.on, b(&want["on"]), "{ctx} on");
    assert_eq!(got.mirror, b(&want["mirror"]), "{ctx} mirror");
    assert_eq!(got.fit, b(&want["fit"]), "{ctx} fit");
}

#[test]
fn tumour_vectors() {
    let mut checked = 0;
    for (idx, case) in cases_exact("tumour").iter().enumerate() {
        let fname = case["fn"].as_str().unwrap();
        let args = case["args"].as_array().unwrap();
        let out = &case["out"];
        let ctx = format!("#{idx} {fname}");
        match fname {
            "clean_tumour" => {
                assert_tumour_eq(T::clean_tumour(&args[0]), out, &ctx);
            }
            "template" => {
                let g = T::template(
                    shape_of(args[0].as_str().unwrap()),
                    f(&args[1]),
                    f(&args[2]),
                );
                assert_pts_eq(&g, out, &ctx);
            }
            "cut" => {
                let g = T::cut(&pts(&args[0]), f(&args[1]));
                assert_pts_eq(&g, out, &ctx);
            }
            "subdivide" => {
                let g = T::subdivide(&pts(&args[0]), &floats(&args[1]));
                assert_pts_eq(&g, out, &ctx);
            }
            "tumour_path" => {
                let tm = T::clean_tumour(&args[1]).unwrap_or_else(|| panic!("{ctx}: tm 无效"));
                let g = T::tumour_path(&pts(&args[0]), &tm);
                assert_pts_eq(&g, out, &ctx);
            }
            other => panic!("未知用例 {other}"),
        }
        checked += 1;
    }
    assert!(checked > 60, "用例太少：{checked}");
}
