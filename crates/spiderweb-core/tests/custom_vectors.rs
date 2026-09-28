//! custom.py 对照测试（向量由 tools/gen_custom_vectors.py 生成）。
//!
//! pack_notes 与 Python 的压缩实现不同（zlib 逐位不同是允许的），所以向量给出 Python 的
//! 文本：测试验证 Python 文本能在 Rust 解开、Rust 打包能被 Rust 解开且与 Python 的行完全一致
//! （两边都是标准 zlib 流，Python 侧用 zlib.decompress 必然能读）。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_core::Pt;
use spiderweb_core::custom as C;
use spiderweb_core::shape::{Align, Fill, Kind, Shape, Stroke, Sym, TextSettings};

fn close(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
}

fn close_f(got: f64, want: f64, ctx: &str) {
    assert!(close(got, want), "{ctx}: got={got} want={want}");
}

fn assert_pt_eq(got: Pt, want: &Value, ctx: &str) {
    close_f(got[0], f(&want[0]), &format!("{ctx}.x"));
    close_f(got[1], f(&want[1]), &format!("{ctx}.y"));
}

fn assert_pts_eq(got: &[Pt], want: &Value, ctx: &str) {
    let w = pts(want);
    assert_eq!(got.len(), w.len(), "{ctx}: 点数不同");
    for (k, (g, x)) in got.iter().zip(w.iter()).enumerate() {
        assert!(
            close(g[0], x[0]) && close(g[1], x[1]),
            "{ctx}: 第 {k} 点 {g:?} != {x:?}"
        );
    }
}

fn assert_paths_eq(got: &[Vec<Pt>], want: &Value, ctx: &str) {
    let w = want.as_array().expect("paths 不是数组");
    assert_eq!(got.len(), w.len(), "{ctx}: 路径数不同");
    for (i, (g, x)) in got.iter().zip(w).enumerate() {
        assert_pts_eq(g, x, &format!("{ctx}[{i}]"));
    }
}

fn assert_polys_eq(got: &[Vec<Pt>], want: &Value, ctx: &str) {
    assert_paths_eq(got, want, ctx);
}

fn assert_spans_eq(got: &[[f64; 2]], want: &Value, ctx: &str) {
    let w = want.as_array().expect("spans 不是数组");
    assert_eq!(got.len(), w.len(), "{ctx}: 段数不同");
    for (i, (g, x)) in got.iter().zip(w).enumerate() {
        let a = x.as_array().expect("段不是数组");
        close_f(g[0], f(&a[0]), &format!("{ctx}[{i}].a"));
        close_f(g[1], f(&a[1]), &format!("{ctx}[{i}].b"));
    }
}

fn rows5(v: &Value) -> Vec<[i64; 5]> {
    v.as_array()
        .expect("不是数组")
        .iter()
        .map(|r| {
            let a = r.as_array().expect("行不是数组");
            [i(&a[0]), i(&a[1]), i(&a[2]), i(&a[3]), i(&a[4])]
        })
        .collect()
}

fn fill_of(s: &str) -> Fill {
    match s {
        "empty" => Fill::Empty,
        "fill" => Fill::Fill,
        "spam" => Fill::Spam,
        "outline_spam" => Fill::OutlineSpam,
        other => panic!("未知 fill {other}"),
    }
}

fn align_of(s: &str) -> Align {
    match s {
        "auto" => Align::Auto,
        "aligned" => Align::Aligned,
        other => panic!("未知 align {other}"),
    }
}

fn stroke_of(v: &Value) -> Stroke {
    let kind = v["kind"]
        .as_str()
        .unwrap_or_else(|| panic!("笔画没有 kind: {v}"));
    match kind {
        "poly" => Stroke::Poly {
            pts: pts(&v["pts"]),
            free: v.get("free").is_some_and(b),
            smooth: v.get("smooth").map_or(0, i),
            k: v.get("k").map_or(1.0, f),
        },
        "curve" => Stroke::Curve {
            pts: pts(&v["pts"]),
            sharp: v
                .get("sharp")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|x| i(x) as usize).collect())
                .unwrap_or_default(),
            sym: match v.get("sym").and_then(Value::as_str) {
                Some("mirror") => Some(Sym::Mirror),
                Some("turn") => Some(Sym::Turn),
                _ => None,
            },
        },
        "arc" => Stroke::Arc {
            pts: pts(&v["pts"]),
            k: v.get("k").map_or(1.0, f),
        },
        "ellipse" => {
            let b = floats(&v["box"]);
            Stroke::Ellipse {
                box_: [b[0], b[1], b[2], b[3]],
            }
        }
        other => panic!("未知笔画 {other}"),
    }
}

fn strokes_of(v: &Value) -> Vec<Stroke> {
    v.as_array()
        .expect("strokes 不是数组")
        .iter()
        .map(stroke_of)
        .collect()
}

fn shape_of(v: &Value) -> Shape {
    let mut sh = Shape {
        kind: Kind::Custom,
        ..Shape::default()
    };
    if let Some(p) = v.get("pts") {
        sh.pts = pts(p);
    }
    if let Some(s) = v.get("strokes") {
        sh.strokes = strokes_of(s);
    }
    if let Some(x) = v.get("fill").and_then(Value::as_str) {
        sh.fill = fill_of(x);
    }
    if let Some(x) = v.get("gate") {
        sh.gate = f(x);
    }
    if let Some(x) = v.get("align").and_then(Value::as_str) {
        sh.align = align_of(x);
    }
    if let Some(x) = v.get("notes").and_then(Value::as_str) {
        sh.notes = Some(x.to_string());
    }
    if let Some(x) = v.get("own_vel") {
        sh.own_vel = b(x);
    }
    if let Some(x) = v.get("vel0") {
        sh.vel0 = f(x);
    }
    if let Some(x) = v.get("vel1") {
        sh.vel1 = f(x);
    }
    if let Some(x) = v.get("end_dot") {
        sh.end_dot = b(x);
    }
    if let Some(tx) = v.get("text") {
        let mut t = TextSettings::default();
        if let Some(x) = tx.get("threshold") {
            t.threshold = f(x);
        }
        if let Some(x) = tx.get("grow") {
            t.grow = f(x);
        }
        if let Some(x) = tx.get("k") {
            t.k = f(x);
        }
        if let Some(h) = tx.get("holes").and_then(Value::as_array) {
            t.holes = h.iter().map(|x| i(x) as usize).collect();
        }
        sh.text = Some(t);
    }
    sh
}

fn assert_stroke_eq(got: &Stroke, want: &Value, ctx: &str) {
    let w = stroke_of(want);
    match (got, &w) {
        (
            Stroke::Poly {
                pts: gp,
                free: gf,
                smooth: gs,
                k: gk,
            },
            Stroke::Poly {
                free: wf,
                smooth: ws,
                k: wk,
                ..
            },
        ) => {
            assert_pts_eq(gp, &want["pts"], &format!("{ctx}.pts"));
            assert_eq!(gf, wf, "{ctx}.free");
            assert_eq!(gs, ws, "{ctx}.smooth");
            close_f(*gk, *wk, &format!("{ctx}.k"));
        }
        (
            Stroke::Curve {
                pts: gp,
                sharp: gsh,
                sym: gsy,
            },
            Stroke::Curve {
                sharp: wsh,
                sym: wsy,
                ..
            },
        ) => {
            assert_pts_eq(gp, &want["pts"], &format!("{ctx}.pts"));
            assert_eq!(gsh, wsh, "{ctx}.sharp");
            assert_eq!(gsy, wsy, "{ctx}.sym");
        }
        (Stroke::Arc { pts: gp, k: gk }, Stroke::Arc { k: wk, .. }) => {
            assert_pts_eq(gp, &want["pts"], &format!("{ctx}.pts"));
            close_f(*gk, *wk, &format!("{ctx}.k"));
        }
        (Stroke::Ellipse { box_: gb }, Stroke::Ellipse { box_: wb }) => {
            for j in 0..4 {
                close_f(gb[j], wb[j], &format!("{ctx}.box[{j}]"));
            }
        }
        _ => panic!("{ctx}: 笔画种类不同 got={got:?} want={w:?}"),
    }
}

fn assert_strokes_eq(got: &[Stroke], want: &Value, ctx: &str) {
    let w = want.as_array().expect("strokes 不是数组");
    assert_eq!(got.len(), w.len(), "{ctx}: 笔画数不同");
    for (k, (g, x)) in got.iter().zip(w).enumerate() {
        assert_stroke_eq(g, x, &format!("{ctx}[{k}]"));
    }
}

fn assert_shape_eq(got: &Shape, want: &Value, ctx: &str) {
    let obj = want.as_object().expect("形状不是对象");
    for (key, v) in obj {
        match key.as_str() {
            "kind" => assert_eq!(got.kind, Kind::Custom, "{ctx}.kind"),
            "pts" => assert_pts_eq(&got.pts, v, &format!("{ctx}.pts")),
            "strokes" => assert_strokes_eq(&got.strokes, v, &format!("{ctx}.strokes")),
            "fill" => assert_eq!(got.fill, fill_of(v.as_str().unwrap()), "{ctx}.fill"),
            "gate" => close_f(got.gate, f(v), &format!("{ctx}.gate")),
            "align" => assert_eq!(got.align, align_of(v.as_str().unwrap()), "{ctx}.align"),
            "name" => assert_eq!(got.name, v.as_str().unwrap(), "{ctx}.name"),
            "own_vel" => assert_eq!(got.own_vel, b(v), "{ctx}.own_vel"),
            "vel0" => close_f(got.vel0, f(v), &format!("{ctx}.vel0")),
            "vel1" => close_f(got.vel1, f(v), &format!("{ctx}.vel1")),
            "end_dot" => assert_eq!(got.end_dot, b(v), "{ctx}.end_dot"),
            "notes" => {
                let want_rows = C::unpack_notes(v.as_str().unwrap()).expect("want notes 解不开");
                let got_rows = C::unpack_notes(got.notes.as_deref().expect("got 没有 notes"))
                    .expect("got notes 解不开");
                assert_eq!(got_rows, want_rows, "{ctx}.notes");
            }
            other => panic!("{ctx}: 未知形状字段 {other}"),
        }
    }
}

#[test]
fn custom_vectors() {
    let mut checked = 0;
    for (idx, case) in cases("custom").iter().enumerate() {
        let fname = case["fn"].as_str().unwrap();
        let args = case["args"].as_array().unwrap();
        let out = &case["out"];
        let ctx = format!("#{idx} {fname}");
        match fname {
            // ------------------------------------------------------------ 笔画 / 点
            "stroke_points" => {
                let g = C::stroke_points(&stroke_of(&args[0]));
                assert_pts_eq(&g, out, &ctx);
            }
            "clean_strokes" => {
                let g = C::clean_strokes(&args[0]);
                assert_strokes_eq(&g, out, &ctx);
            }
            "clean_curve" => {
                let g = C::clean_curve(&args[0], &pts(&args[1]));
                assert_stroke_eq(&g, out, &ctx);
            }
            "path_closed" => {
                assert_eq!(C::path_closed(&pts(&args[0])), b(out), "{ctx}");
            }
            "stroke_closed" => {
                assert_eq!(C::stroke_closed(&stroke_of(&args[0])), b(out), "{ctx}");
            }
            "join_paths" => {
                let paths: Vec<Vec<Pt>> = args[0].as_array().unwrap().iter().map(pts).collect();
                let g = C::join_paths(&paths);
                assert_paths_eq(&g, out, &ctx);
            }
            "stroke_span" => {
                let g = C::stroke_span(&stroke_of(&args[0]));
                assert_eq!(g.is_some(), !out.is_null(), "{ctx}");
                if let Some(s) = g {
                    assert_pts_eq(&s, out, &ctx);
                }
            }
            "open_paths" => {
                let g = C::open_paths(&strokes_of(&args[0]));
                assert_paths_eq(&g, out, &ctx);
            }
            "open_ends" => {
                let g = C::open_ends(&strokes_of(&args[0]));
                assert_pts_eq(&g, out, &ctx);
            }
            "strokes_closed" => {
                assert_eq!(C::strokes_closed(&strokes_of(&args[0])), b(out), "{ctx}");
            }
            "fillable" => {
                assert_eq!(C::fillable(&strokes_of(&args[0])), b(out), "{ctx}");
            }
            "gap_line" => {
                let g = C::gap_line(&shape_of(&args[0]));
                assert_eq!(g.is_some(), !out.is_null(), "{ctx}");
                if let Some(s) = g {
                    assert_pts_eq(&s, out, &ctx);
                }
            }
            "join_strokes" => {
                let g = C::join_strokes(&strokes_of(&args[0]));
                assert_strokes_eq(&g, out, &ctx);
            }
            "custom_strokes" => {
                let g = C::custom_strokes(&shape_of(&args[0]));
                assert_polys_eq(&g, out, &ctx);
            }
            "box_frame" => {
                let g = C::box_frame(f(&args[0]), f(&args[1]), f(&args[2]), f(&args[3]));
                assert_pts_eq(&g, out, &ctx);
            }
            "normalize_strokes" => {
                let (g, ratio) = C::normalize_strokes(&strokes_of(&args[0]));
                assert_strokes_eq(&g, &out[0], &format!("{ctx}.strokes"));
                assert_eq!(ratio.is_some(), !out[1].is_null(), "{ctx}.ratio");
                if let Some(r) = ratio {
                    close_f(r, f(&out[1]), &format!("{ctx}.ratio"));
                }
            }

            // ------------------------------------------------------------ 框
            "frame_to_bp" => {
                let p = pts(&args[0]);
                let to = C::frame_to_bp(&p).expect("frame_to_bp");
                let g = to(f(&args[1]), f(&args[2]));
                assert_pt_eq(g, out, &ctx);
            }
            "frame_to_uv" => {
                let p = pts(&args[0]);
                let g = C::frame_to_uv(&p).map(|to| to(f(&args[1]), f(&args[2])));
                assert_eq!(g.is_some(), !out.is_null(), "{ctx}");
                if let Some(g) = g {
                    assert_pt_eq(g, out, &ctx);
                }
            }
            "uv_k" => {
                close_f(C::uv_k(&pts(&args[0]), f(&args[1])), f(out), &ctx);
            }
            "frame_upright" => {
                assert_eq!(C::frame_upright(&pts(&args[0])), b(out), "{ctx}");
            }
            "map_stroke" => {
                let name = args[0].as_str().unwrap();
                let params = floats(&args[1]);
                let st = stroke_of(&args[2]);
                let su = f(&args[3]);
                let sv = f(&args[4]);
                let map: Box<dyn Fn(f64, f64) -> Pt> = match name {
                    "shift" => Box::new(move |u, v| [u + params[0], v + params[1]]),
                    "scale" => Box::new(move |u, v| [u * params[0], v * params[1]]),
                    "shear" => Box::new(move |u, v| [u + params[0] * v, v + params[1] * u]),
                    "mix" => Box::new(|u, v| [0.5 * u + 0.2 * v + 0.1, -0.3 * u + 0.7 * v - 0.05]),
                    other => panic!("未知映射 {other}"),
                };
                let g = C::map_stroke(&st, map, su, sv);
                assert_stroke_eq(&g, out, &ctx);
            }
            "refit" => {
                let mut sh = shape_of(&args[0]);
                C::refit(&mut sh);
                assert_shape_eq(&sh, out, &ctx);
            }
            "stroke_ends" => {
                let g = C::stroke_ends(&strokes_of(&args[0]));
                assert_pts_eq(&g, out, &ctx);
            }
            "add_stroke" => {
                let mut sh = shape_of(&args[0]);
                let k = C::add_stroke(&mut sh, &stroke_of(&args[1])).expect("add_stroke");
                assert_eq!(k as i64, i(&out[0]), "{ctx}.k");
                assert_shape_eq(&sh, &out[1], &ctx);
            }
            "new_live_shape" => {
                let defaults = shape_of(&args[0]);
                let cd = C::CustomDefaults {
                    fill: fill_of(args[1]["fill"].as_str().unwrap()),
                    gate: f(&args[1]["gate"]),
                    align: align_of(args[1]["align"].as_str().unwrap()),
                };
                let g = C::new_live_shape(&defaults, &cd);
                assert_shape_eq(&g, out, &ctx);
            }

            // ------------------------------------------------------------ 音符
            "outline_notes" => {
                let g = C::outline_notes(&shape_of(&args[0]), f(&args[1]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "row_spans" => {
                let polys: Vec<Vec<Pt>> = args[0].as_array().unwrap().iter().map(pts).collect();
                let g = C::row_spans(&polys, f(&args[1]));
                assert_spans_eq(&g, out, &ctx);
            }
            "inside_spans" => {
                let g = C::inside_spans(&shape_of(&args[0]), f(&args[1]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "spam_gate" => {
                assert_eq!(
                    C::spam_gate(&shape_of(&args[0]), f(&args[1])),
                    i(out),
                    "{ctx}"
                );
            }
            "spam_starts" => {
                let sh = shape_of(&args[0]);
                let (s, n) = C::spam_starts(&sh, i(&args[1]), i(&args[2]), i(&args[3]));
                assert_eq!(s, i(&out[0]), "{ctx}.s");
                assert_eq!(n, i(&out[1]), "{ctx}.n");
            }
            "chop" => {
                let sh = shape_of(&args[0]);
                let stretches = rows3(&args[1]);
                let g = C::chop(&sh, &stretches, i(&args[2]), b(&args[3]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "outline_spam" => {
                let g = C::outline_spam(&shape_of(&args[0]), f(&args[1]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "custom_note_count" => {
                let g = C::custom_note_count(&shape_of(&args[0]), f(&args[1]));
                assert_eq!(g.is_some(), !out.is_null(), "{ctx}");
                if let Some(n) = g {
                    assert_eq!(n, i(out), "{ctx}");
                }
            }
            "custom_notes" => {
                let g = C::custom_notes(&shape_of(&args[0]), f(&args[1]));
                assert_rows3_eq(&g, out, &ctx);
            }

            // ------------------------------------------------------------ 粘贴音符
            "pack_notes" => {
                let rows = rows5(&args[0]);
                let py_text = out.as_str().unwrap();
                let py_rows = C::unpack_notes(py_text).expect("Python 的文本应能解");
                assert_eq!(py_rows, rows, "{ctx}: Python 文本");
                let got = C::pack_notes(&rows);
                let got_rows = C::unpack_notes(&got).expect("Rust 的文本应能解");
                assert_eq!(got_rows, rows, "{ctx}: Rust 往返");
                if !rows.is_empty() {
                    assert!(C::check_notes(&got), "{ctx}: Rust 文本应通过 check_notes");
                }
            }
            "unpack_notes" => {
                let text = args[0].as_str().unwrap();
                let g = C::unpack_notes(text).expect("unpack_notes");
                assert_eq!(g, rows5(out), "{ctx}");
            }
            "check_notes" => {
                let text = args[0].as_str().unwrap();
                assert_eq!(C::check_notes(text), b(out), "{ctx}");
            }
            "notes_shape" => {
                let rows = rows5(&args[0]);
                let g = C::notes_shape(&rows, f(&args[1]), args[2].as_str().unwrap())
                    .expect("notes_shape");
                assert_shape_eq(&g, out, &ctx);
            }
            "block_notes" => {
                let sh = shape_of(&args[0]);
                let g = C::block_notes(&sh, f(&args[1])).expect("block_notes");
                assert_eq!(g, rows5(out), "{ctx}");
            }
            other => panic!("未知用例 {other}"),
        }
        checked += 1;
    }
    assert!(checked >= 120, "用例太少：{checked}");

    // ------------------------------------------------------------ 常量
    assert_eq!(
        C::FILLS,
        [Fill::Empty, Fill::Fill, Fill::Spam, Fill::OutlineSpam]
    );
    assert_eq!(C::SPAM_FILLS, [Fill::Spam, Fill::OutlineSpam]);
    assert_eq!(C::ALIGNS, [Align::Auto, Align::Aligned]);
    assert_eq!(C::CUSTOM_DEFAULTS, C::CustomDefaults::default());
    assert_eq!(C::CUSTOM_DEFAULTS.fill, Fill::Empty);
    assert_eq!(C::CUSTOM_DEFAULTS.gate, 0.0625);
    assert_eq!(C::CUSTOM_DEFAULTS.align, Align::Auto);
    assert_eq!(C::ELLIPSE_STEPS, 360);
    assert_eq!(C::CURVE_STEPS, 240);
    assert_eq!(C::TRACKS, "t:");
    if let Stroke::Poly { pts, .. } = &*C::BOX_STROKE {
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[0], pts[4]);
    } else {
        panic!("BOX_STROKE 应是折线");
    }
}
