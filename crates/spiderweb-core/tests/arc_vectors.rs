//! arc.py 对照测试（向量由 tools/gen_arc_vectors.py 生成）。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_core::arc as A;

fn close(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
}

fn one(v: &Value) -> [f64; 2] {
    let a = v.as_array().expect("不是点");
    [f(&a[0]), f(&a[1])]
}

#[test]
fn arc_vectors() {
    let mut checked = 0;
    for (idx, case) in cases("arc").iter().enumerate() {
        let fname = case["fn"].as_str().unwrap();
        let args = case["args"].as_array().unwrap();
        let out = &case["out"];
        let ctx = format!("#{idx} {fname}");
        match fname {
            "circle" => {
                let g = A::circle(one(&args[0]), one(&args[1]), one(&args[2]));
                if out.is_null() {
                    assert!(g.is_none(), "{ctx}: 应无圆");
                } else {
                    let (c, r) = g.expect("应有圆");
                    let wc = one(&out[0]);
                    assert!(
                        close(c[0], wc[0]) && close(c[1], wc[1]),
                        "{ctx}: 圆心 got={c:?} want={wc:?}"
                    );
                    assert!(
                        close(r, f(&out[1])),
                        "{ctx}: 半径 got={r} want={}",
                        f(&out[1])
                    );
                }
            }
            "full_circle" => {
                let g = A::full_circle(one(&args[0]), one(&args[1]), one(&args[2]));
                if out.is_null() {
                    assert!(g.is_none(), "{ctx}: 应为 None");
                } else {
                    let (c, r, t0, span) = g.expect("应有整圆");
                    let wc = one(&out[0]);
                    assert!(close(c[0], wc[0]) && close(c[1], wc[1]), "{ctx}: 圆心");
                    assert!(close(r, f(&out[1])), "{ctx}: 半径");
                    assert!(close(t0, f(&out[2])), "{ctx}: 起始角");
                    assert!(close(span, f(&out[3])), "{ctx}: 扫角");
                }
            }
            "arc_circle" => {
                let g = A::arc_circle(&pts(&args[0]), f(&args[1]));
                if out.is_null() {
                    assert!(g.is_none(), "{ctx}: 应为 None");
                } else {
                    let (c, r, t0, span) = g.expect("应有圆弧");
                    let wc = one(&out[0]);
                    assert!(close(c[0], wc[0]) && close(c[1], wc[1]), "{ctx}: 圆心");
                    assert!(close(r, f(&out[1])), "{ctx}: 半径");
                    assert!(close(t0, f(&out[2])), "{ctx}: 起始角");
                    assert!(close(span, f(&out[3])), "{ctx}: 扫角");
                }
            }
            "arc_points" => {
                let g = A::arc_points(&pts(&args[0]), f(&args[1]), f(&args[2]));
                assert_pts_eq(&g, out, &ctx);
            }
            "arc_bezier" => {
                let g = A::arc_bezier(&pts(&args[0]), f(&args[1]));
                assert_pts_eq(&g, out, &ctx);
            }
            "ellipse_bezier" => {
                let b = floats(&args[0]);
                let g = A::ellipse_bezier([b[0], b[1], b[2], b[3]]);
                assert_pts_eq(&g, out, &ctx);
            }
            "line_bezier" => {
                let g = A::line_bezier(one(&args[0]), one(&args[1]));
                assert_pts_eq(&g, out, &ctx);
            }
            "clean_k" => {
                let k = match &args[0] {
                    Value::Number(n) => n.as_f64().unwrap(),
                    Value::String(s) => s.parse::<f64>().unwrap(),
                    other => panic!("{ctx}: 参数 {other}"),
                };
                assert!(
                    close(A::clean_k(k), f(out)),
                    "{ctx}: got {} want {out}",
                    A::clean_k(k)
                );
            }
            other => panic!("未知用例 {other}"),
        }
        checked += 1;
    }
    assert!(checked >= 55, "用例太少：{checked}");
}
