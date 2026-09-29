//! engine.py 对照测试（向量由 tools/gen_engine_vectors.py 生成）。
//!
//! 向量里的形状是 Python `clean_shape` 之后的形状字典；测试里有一小段把这种字典
//! 读成 [`Shape`] 的解析（只覆盖向量用到的字段，core 里不重复实现 clean_shape）。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_core::Pt;
use spiderweb_core::engine as E;
use spiderweb_core::shape::{
    Align, Fill, FunnelCurve, FunnelFill, FunnelStart, GateChange, GateFollow, Kind, Shape, Stroke,
    Sym, Tumour, TumourShape, TumourSide, TumourWrap, WallMode,
};

fn close(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
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

fn rows4(v: &Value) -> Vec<[i64; 4]> {
    v.as_array()
        .expect("不是数组")
        .iter()
        .map(|r| {
            let a = r.as_array().expect("行不是数组");
            [i(&a[0]), i(&a[1]), i(&a[2]), i(&a[3])]
        })
        .collect()
}

fn rows6(v: &Value) -> Vec<[i64; 6]> {
    v.as_array()
        .expect("不是数组")
        .iter()
        .map(|r| {
            let a = r.as_array().expect("行不是数组");
            [i(&a[0]), i(&a[1]), i(&a[2]), i(&a[3]), i(&a[4]), i(&a[5])]
        })
        .collect()
}

fn assert_rows4_eq(got: &[[i64; 4]], want: &Value, ctx: &str) {
    assert_eq!(got, rows4(want).as_slice(), "{ctx}");
}

fn assert_rows6_eq(got: &[[i64; 6]], want: &Value, ctx: &str) {
    assert_eq!(got, rows6(want).as_slice(), "{ctx}");
}

// ---------------------------------------------------------------- 形状字典 -> Shape

fn kind_of(s: &str) -> Kind {
    match s {
        "line" => Kind::Line,
        "poly" => Kind::Poly,
        "free" => Kind::Free,
        "curve" => Kind::Curve,
        "arc" => Kind::Arc,
        "custom" => Kind::Custom,
        "funnel" => Kind::Funnel,
        other => panic!("未知 kind {other}"),
    }
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

fn sym_of(v: &Value) -> Option<Sym> {
    match v.as_str() {
        Some("mirror") => Some(Sym::Mirror),
        Some("turn") => Some(Sym::Turn),
        _ => None,
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
            sym: v.get("sym").map(sym_of).unwrap_or(None),
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

fn curve_of(v: &Value) -> FunnelCurve {
    FunnelCurve {
        pts: pts(&v["pts"]),
        sharp: v
            .get("sharp")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| i(x) as usize).collect())
            .unwrap_or_default(),
        link: v.get("link").and_then(Value::as_i64),
        flip: v.get("flip").and_then(Value::as_bool).unwrap_or(false),
    }
}

fn start_of(v: &Value) -> FunnelStart {
    let line = v.get("line").map(|x| i(x) as usize).unwrap_or(0);
    let at = v.get("at").map(f).unwrap_or(0.0);
    let mut ends = [None, None];
    if let Some(a) = v.get("ends").and_then(Value::as_array) {
        for (j, e) in a.iter().take(2).enumerate() {
            if !e.is_null() {
                ends[j] = Some(curve_of(e));
            }
        }
    }
    FunnelStart { line, at, ends }
}

fn tumour_of(v: &Value) -> Tumour {
    let mut tm = Tumour::default();
    if let Some(x) = v.get("on") {
        tm.on = b(x);
    }
    if let Some(s) = v.get("shape").and_then(Value::as_str) {
        tm.shape = match s {
            "triangle" => TumourShape::Triangle,
            "square" => TumourShape::Square,
            "circle" => TumourShape::Circle,
            "parabola" => TumourShape::Parabola,
            other => panic!("未知肿瘤形状 {other}"),
        };
    }
    if let Some(s) = v.get("side").and_then(Value::as_str) {
        tm.side = match s {
            "alt" => TumourSide::Alt,
            "left" => TumourSide::Left,
            "right" => TumourSide::Right,
            "random" => TumourSide::Random,
            other => panic!("未知肿瘤方向 {other}"),
        };
    }
    if let Some(s) = v.get("wrap").and_then(Value::as_str) {
        tm.wrap = match s {
            "simple" => TumourWrap::Simple,
            "wrap" => TumourWrap::Wrap,
            other => panic!("未知肿瘤跟随 {other}"),
        };
    }
    if let Some(x) = v.get("size") {
        tm.size = f(x);
    }
    if let Some(x) = v.get("length") {
        tm.length = f(x);
    }
    if let Some(x) = v.get("dist") {
        tm.dist = f(x);
    }
    if let Some(x) = v.get("start") {
        tm.start = f(x);
    }
    if let Some(x) = v.get("end") {
        tm.end = f(x);
    }
    if let Some(x) = v.get("ease") {
        tm.ease = f(x);
    }
    if let Some(x) = v.get("k") {
        tm.k = f(x);
    }
    if let Some(x) = v.get("seed") {
        tm.seed = i(x);
    }
    if let Some(x) = v.get("fit") {
        tm.fit = b(x);
    }
    if let Some(x) = v.get("mirror") {
        tm.mirror = b(x);
    }
    tm
}

fn shape_of(v: &Value) -> Shape {
    let kind = kind_of(v["kind"].as_str().expect("形状没有 kind"));
    let mut sh = Shape {
        kind,
        pts: pts(&v["pts"]),
        ..Shape::default()
    };
    if let Some(x) = v.get("vel0") {
        sh.vel0 = f(x);
    }
    if let Some(x) = v.get("vel1") {
        sh.vel1 = f(x);
    }
    if let Some(x) = v.get("end_dot") {
        sh.end_dot = b(x);
    }
    if let Some(env) = v.get("vel_env").filter(|x| !x.is_null()) {
        sh.vel_env = pts(env);
    }
    if let Some(tm) = v.get("tumour").filter(|x| !x.is_null()) {
        sh.tumour = Some(tumour_of(tm));
    }
    match kind {
        Kind::Free => {
            if let Some(x) = v.get("smooth") {
                sh.smooth = i(x);
            }
            if let Some(x) = v.get("k") {
                sh.k = f(x);
            }
        }
        Kind::Arc => {
            if let Some(x) = v.get("k") {
                sh.k = f(x);
            }
        }
        Kind::Curve => {
            if let Some(a) = v.get("sharp").and_then(Value::as_array) {
                sh.sharp = a.iter().map(|x| i(x) as usize).collect();
            }
            if let Some(x) = v.get("sym") {
                sh.sym = sym_of(x);
            }
        }
        Kind::Custom => {
            if let Some(x) = v.get("name").and_then(Value::as_str) {
                sh.name = x.to_string();
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
        }
        Kind::Funnel => {
            if let Some(x) = v.get("fill").and_then(Value::as_str) {
                sh.funnel_fill = FunnelFill::from_name(x).unwrap_or_default();
            }
            if let Some(x) = v.get("change").and_then(Value::as_str) {
                sh.change = GateChange::from_name(x).unwrap_or_default();
            }
            if let Some(x) = v.get("follow").and_then(Value::as_str) {
                sh.follow = GateFollow::from_name(x).unwrap_or_default();
            }
            if let Some(x) = v.get("wall").and_then(Value::as_str) {
                sh.wall = WallMode::from_name(x).unwrap_or_default();
            }
            if let Some(x) = v.get("gate0") {
                sh.gate0 = f(x);
            }
            if let Some(x) = v.get("gate1") {
                sh.gate1 = f(x);
            }
            if let Some(x) = v.get("vary") {
                sh.vary = b(x);
            }
            if let Some(items) = v.get("starts").and_then(Value::as_array) {
                sh.starts = items.iter().map(start_of).collect();
            }
        }
        Kind::Line | Kind::Poly => {}
    }
    sh
}

// ---------------------------------------------------------------- 用例

fn split_of(s: &str) -> E::Split {
    match s {
        "key" => E::Split::Key,
        "time" => E::Split::Time,
        other => panic!("未知 split {other}"),
    }
}

fn mode_of(s: &str) -> E::Mode {
    match s {
        "raw" => E::Mode::Raw,
        "single" => E::Mode::Single,
        "auto" => E::Mode::Auto,
        other => panic!("未知 mode {other}"),
    }
}

fn note_lists_of(v: &Value) -> Vec<Vec<[i64; 4]>> {
    v.as_array()
        .expect("note_lists 不是数组")
        .iter()
        .map(rows4)
        .collect()
}

#[test]
fn engine_vectors() {
    let mut checked = 0;
    for (idx, case) in cases("engine").iter().enumerate() {
        let fname = case["fn"].as_str().unwrap();
        let args = case["args"].as_array().unwrap();
        let out = &case["out"];
        let ctx = format!("#{idx} {fname}");
        match fname {
            "make_shape" => {
                let defaults = Shape {
                    vel0: f(&args[2]["vel0"]),
                    vel1: f(&args[2]["vel1"]),
                    end_dot: b(&args[2]["end_dot"]),
                    ..Shape::default()
                };
                let got = E::make_shape(
                    kind_of(args[0].as_str().unwrap()),
                    &pts(&args[1]),
                    &defaults,
                );
                assert_eq!(
                    got.kind.as_str(),
                    out["kind"].as_str().unwrap(),
                    "{ctx}.kind"
                );
                assert_pts_eq(&got.pts, &out["pts"], &format!("{ctx}.pts"));
                assert_eq!(got.vel0, f(&out["vel0"]), "{ctx}.vel0");
                assert_eq!(got.vel1, f(&out["vel1"]), "{ctx}.vel1");
                assert_eq!(got.end_dot, b(&out["end_dot"]), "{ctx}.end_dot");
                if let Some(starts) = out.get("starts") {
                    assert!(starts.as_array().unwrap().is_empty(), "{ctx}.starts");
                    assert!(got.starts.is_empty(), "{ctx}.starts");
                }
            }
            "point_names" => {
                let got = E::point_names(&shape_of(&args[0]));
                match (got, out.is_null()) {
                    (None, true) => {}
                    (Some(names), false) => {
                        let want: Vec<&str> = out
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap())
                            .collect();
                        assert_eq!(names, want, "{ctx}");
                    }
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "shape_path" => {
                assert_pts_eq(&E::shape_path(&shape_of(&args[0])), out, &ctx);
            }
            "shape_strokes" => {
                assert_paths_eq(&E::shape_strokes(&shape_of(&args[0])), out, &ctx);
            }
            "shape_notes" => {
                let keys = args.get(2).map_or(128, i);
                let got = E::shape_notes(&shape_of(&args[0]), f(&args[1]), keys);
                assert_rows4_eq(&got, out, &ctx);
            }
            "shape_notes_tracks" => {
                let keys = args.get(2).map_or(128, i);
                let (notes, tracks) = E::shape_notes_tracks(&shape_of(&args[0]), f(&args[1]), keys);
                assert_rows4_eq(&notes, &out[0], &ctx);
                let want: Option<Vec<i64>> = out[1].as_array().map(|a| a.iter().map(i).collect());
                assert_eq!(tracks, want, "{ctx}.tracks");
            }
            "assign_slots" => {
                let lists = note_lists_of(&args[0]);
                let got = E::assign_slots(&lists, split_of(args[1].as_str().unwrap()));
                let want: Vec<usize> = out
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| i(v) as usize)
                    .collect();
                assert_eq!(got, want, "{ctx}");
            }
            "resolve_overlaps" => {
                assert_rows6_eq(&E::resolve_overlaps(&rows6(&args[0])), out, &ctx);
            }
            "render" => {
                let lists = note_lists_of(&args[0]);
                let tracks: Option<Vec<Option<Vec<i64>>>> = args[3].as_array().map(|a| {
                    a.iter()
                        .map(|t| t.as_array().map(|r| r.iter().map(i).collect()))
                        .collect()
                });
                let (notes, count) = E::render(
                    &lists,
                    mode_of(args[1].as_str().unwrap()),
                    split_of(args[2].as_str().unwrap()),
                    tracks.as_deref(),
                );
                assert_rows6_eq(&notes, &out[0], &ctx);
                assert_eq!(count as i64, i(&out[1]), "{ctx}.count");
            }
            "slot_track_channel" => {
                let (track, channel) = E::slot_track_channel(i(&args[0]) as usize);
                let want = out.as_array().unwrap();
                assert_eq!(track as i64, i(&want[0]), "{ctx}.track");
                assert_eq!(channel as i64, i(&want[1]), "{ctx}.channel");
            }
            other => panic!("未知用例 {other}"),
        }
        checked += 1;
    }
    assert!(checked >= 120, "用例太少：{checked}");

    // ------------------------------------------------------------ 常量
    assert_eq!(E::KINDS.len(), 7);
    assert_eq!(E::KINDS[0], (Kind::Line, "Line"));
    assert_eq!(E::KINDS[6], (Kind::Funnel, "Funnel"));
    assert_eq!(E::POINT_NAMES[0].1, ["A", "B"]);
    assert_eq!(E::POINT_NAMES[1].1, ["Start", "Through", "End"]);
    assert_eq!(
        E::POINT_NAMES[2].1,
        ["Line start", "Line end", "Wall 1", "Wall 2"]
    );
    assert_eq!(E::SHAPE_DEFAULTS.vel0, 127.0);
    assert_eq!(E::SHAPE_DEFAULTS.vel1, 127.0);
    const { assert!(!E::SHAPE_DEFAULTS.end_dot) };
    assert_eq!(
        E::CHANNELS,
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15]
    );
}
