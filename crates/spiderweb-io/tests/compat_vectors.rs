//! engine.clean_shape / project.short_shape 对照测试（向量由 tools/gen_io_vectors.py 生成）。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_io::compat::{shape_from_json, shape_to_json};

/// 漏斗第一版（两点 + sides）：线 + 墙 + 一个起点。原版会用最小二乘拟合 bends 曲线，
/// 这里按模块说明用默认曲线（差异只在这条曲线的点上）。
#[test]
fn legacy_funnel() {
    for extra in [serde_json::json!({}), serde_json::json!({"starts": null})] {
        let mut input = serde_json::json!({
            "kind": "funnel",
            "pts": [[0, 60], [4, 60]],
            "bend": [0.75, 0.2],
        });
        if extra.get("starts").is_some() {
            input["starts"] = serde_json::Value::Null;
        }
        let shape = shape_from_json(&input)
            .expect("旧漏斗应当能读")
            .expect("旧漏斗应当有效");
        assert_eq!(shape.pts.len(), 4);
        assert_eq!(shape.starts.len(), 1);
        assert_eq!(shape.starts[0].line, 0);
        assert!(shape.starts[0].ends[1].is_some());
        assert_eq!(
            shape.starts[0].ends[0].as_ref().map(|c| c.pts.len()),
            Some(4)
        );
    }
}

#[test]
fn shape_from_json_vectors() {
    let cases = cases("compat", "cases");
    for case in &cases {
        let name = s(&case["name"]);
        let input = &case["input"];
        let got = shape_from_json(input);
        if let Some(err_name) = case.get("error").and_then(Value::as_str) {
            assert!(
                got.is_err(),
                "{name}: Python 抛 {err_name}，Rust 却解析成功: {got:?}"
            );
            continue;
        }
        if case.get("non_numeric_vel").is_some() {
            // Python 把坏 vel0/vel1 原样留下（之后用的时候才炸）；Rust 的 Shape 是 f64，直接报错
            assert!(got.is_err(), "{name}: 坏速度应当报错");
            continue;
        }
        let clean = &case["clean"];
        match (got, clean.is_null()) {
            (Ok(None), true) => {}
            (Ok(None), false) => panic!("{name}: Python 有结果，Rust 返回 None"),
            (Ok(Some(_)), true) => panic!("{name}: Python 返回 None，Rust 却有结果"),
            (Err(e), _) => panic!("{name}: Rust 报错 {e}"),
            (Ok(Some(shape)), false) => {
                let json = shape_to_json(&shape);
                assert_json_eq(&json, &shape_want(clean), name);
                // 再解析回来必须是同一个形状
                let again = shape_from_json(&json)
                    .unwrap_or_else(|e| panic!("{name}: 自己写出的 JSON 读不回: {e}"))
                    .unwrap_or_else(|| panic!("{name}: 自己写出的 JSON 读回是 None"));
                assert_json_eq(&shape_to_json(&again), &json, &format!("{name}（二次）"));
            }
        }
    }
}
