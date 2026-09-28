//! spiderweb-io 对照测试的公共部分：读向量、数值容差的 JSON 深比较。

#![allow(dead_code)]

use serde_json::{Map, Value};

pub fn cases(module: &str, key: &str) -> Vec<Value> {
    let path = format!("{}/tests/vectors/{module}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("缺少向量文件 {path}（先运行 tools/gen_io_vectors.py）：{e}"));
    let data: Value = serde_json::from_str(&text).expect("向量 JSON 解析失败");
    data[key].as_array().expect("cases 不是数组").clone()
}

pub fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("不是数字: {v}"))
}

pub fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("不是整数: {v}"))
}

pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("不是字符串: {v}"))
}

fn close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() && b.is_nan() {
        return true;
    }
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
}

/// 数值带容差、结构完全一致的深比较（键的顺序无所谓）。
pub fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => a
            .as_f64()
            .is_some_and(|x| b.as_f64().is_some_and(|y| close(x, y))),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_eq(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w)))
        }
        _ => a == b,
    }
}

pub fn assert_json_eq(got: &Value, want: &Value, ctx: &str) {
    if !json_eq(got, want) {
        panic!(
            "{ctx}:\n  got  {}\n  want {}",
            serde_json::to_string(got).unwrap_or_default(),
            serde_json::to_string(want).unwrap_or_default()
        );
    }
}

/// 形状对照：Python `clean_shape` 对 free 形状只有文件里有 "smooth" 时才写 smooth/k，
/// 而 Rust `Shape` 总带着它们；补上默认值再比。
pub fn shape_want(v: &Value) -> Value {
    let Some(d) = v.as_object() else {
        return v.clone();
    };
    if d.get("kind").and_then(Value::as_str) != Some("free") {
        return v.clone();
    }
    let mut out: Map<String, Value> = d.clone();
    out.entry("smooth").or_insert(Value::from(0));
    out.entry("k").or_insert(Value::from(1.0));
    Value::Object(out)
}

/// 工程对照：里面的每个形状也走 [`shape_want`]。
pub fn project_want(v: &Value) -> Value {
    let mut out = v.clone();
    if let Some(shapes) = out.get_mut("shapes").and_then(Value::as_array_mut) {
        for sh in shapes.iter_mut() {
            *sh = shape_want(sh);
        }
    }
    out
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("不是十六进制"))
        .collect()
}
