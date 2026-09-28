//! funnel.py 对照测试（向量由 tools/gen_funnel_vectors.py 生成）。

mod common;

use std::collections::BTreeMap;

use common::*;
use serde_json::{Value, json};

use spiderweb_core::Pt;
use spiderweb_core::funnel as F;
use spiderweb_core::shape::{
    FunnelCurve, FunnelFill, FunnelStart, GateChange, GateFollow, Kind, Shape, WallMode,
};

fn close(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
}

fn pt_of(v: &Value) -> Pt {
    let a = v.as_array().expect("点不是数组");
    [f(&a[0]), f(&a[1])]
}

fn bend_of(v: &Value) -> Pt {
    pt_of(v)
}

fn box_of(v: &Value) -> (Pt, Pt, Pt) {
    let a = v.as_array().expect("方盒不是数组");
    (pt_of(&a[0]), pt_of(&a[1]), pt_of(&a[2]))
}

fn curve_of(v: &Value) -> FunnelCurve {
    FunnelCurve {
        pts: pts(&v["pts"]),
        sharp: v
            .get("sharp")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| i(x) as usize).collect())
            .unwrap_or_default(),
        link: v
            .get("link")
            .and_then(|x| if x.is_null() { None } else { Some(i(x)) }),
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

fn shape_of(v: &Value) -> Shape {
    let mut sh = Shape {
        kind: Kind::Funnel,
        pts: pts(&v["pts"]),
        ..Shape::default()
    };
    if let Some(items) = v.get("starts").and_then(Value::as_array) {
        for st in items {
            sh.starts.push(start_of(st));
        }
    }
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
    if let Some(x) = v.get("vary").and_then(Value::as_bool) {
        sh.vary = x;
    }
    sh
}

fn map_spans(v: &Value) -> BTreeMap<i64, Vec<[f64; 2]>> {
    let mut spans = BTreeMap::new();
    for (k, vv) in v.as_object().expect("spans 不是对象") {
        let list: Vec<[f64; 2]> = vv
            .as_array()
            .expect("spans 值不是数组")
            .iter()
            .map(|p| {
                let a = p.as_array().expect("区间不是数组");
                [f(&a[0]), f(&a[1])]
            })
            .collect();
        spans.insert(k.parse().expect("key 不是整数"), list);
    }
    spans
}

fn spans_from(v: &Value) -> F::Spans {
    let mut spans = F::Spans::new();
    for (k, list) in map_spans(v) {
        spans.insert(k, list);
    }
    spans
}

fn got_spans(g: &F::Spans) -> BTreeMap<i64, Vec<[f64; 2]>> {
    g.iter()
        .map(|(k, v)| (k, v.iter().map(|s| [s[0], s[1]]).collect()))
        .collect()
}

fn assert_pt(g: &[f64; 2], w: &Value, ctx: &str) {
    let a = w.as_array().unwrap_or_else(|| panic!("{ctx}: 不是点 {w}"));
    assert!(
        close(g[0], f(&a[0])) && close(g[1], f(&a[1])),
        "{ctx}: got={g:?} want={w}"
    );
}

fn assert_poly(g: &[Pt], w: &Value, ctx: &str) {
    let wp = pts(w);
    assert_eq!(
        g.len(),
        wp.len(),
        "{ctx}: 点数 got={} want={}",
        g.len(),
        wp.len()
    );
    for (k, (a, b)) in g.iter().zip(wp.iter()).enumerate() {
        assert!(
            close(a[0], b[0]) && close(a[1], b[1]),
            "{ctx}: 第 {k} 点 got={a:?} want={b:?}"
        );
    }
}

fn assert_polys(g: &[Vec<Pt>], w: &Value, ctx: &str) {
    let a = w.as_array().unwrap();
    assert_eq!(g.len(), a.len(), "{ctx}: 折线数");
    for (k, (p, wv)) in g.iter().zip(a).enumerate() {
        assert_poly(p, wv, &format!("{ctx}: poly {k}"));
    }
}

fn assert_segs(g: &[[Pt; 2]], w: &Value, ctx: &str) {
    let a = w.as_array().unwrap();
    assert_eq!(g.len(), a.len(), "{ctx}: 段数");
    for (k, (p, wv)) in g.iter().zip(a).enumerate() {
        assert_poly(p, wv, &format!("{ctx}: seg {k}"));
    }
}

fn assert_curve(g: &FunnelCurve, w: &Value, ctx: &str) {
    assert_poly(&g.pts, &w["pts"], &format!("{ctx}: pts"));
    let sharp: Vec<usize> = w
        .get("sharp")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|x| i(x) as usize).collect())
        .unwrap_or_default();
    assert_eq!(g.sharp, sharp, "{ctx}: sharp");
    let link = w
        .get("link")
        .and_then(|x| if x.is_null() { None } else { Some(i(x)) });
    assert_eq!(g.link, link, "{ctx}: link");
    let flip = w.get("flip").and_then(Value::as_bool).unwrap_or(false);
    assert_eq!(g.flip, flip, "{ctx}: flip");
}

fn assert_start(g: &FunnelStart, w: &Value, ctx: &str) {
    assert_eq!(g.line, i(&w["line"]) as usize, "{ctx}: line");
    assert!(
        close(g.at, f(&w["at"])),
        "{ctx}: at got={} want={}",
        g.at,
        w["at"]
    );
    let ends = w["ends"].as_array().unwrap();
    for e in 0..2 {
        match (&g.ends[e], ends.get(e).unwrap_or(&Value::Null)) {
            (None, Value::Null) => {}
            (Some(c), x) if !x.is_null() => assert_curve(c, x, &format!("{ctx}: end {e}")),
            (g, x) => panic!("{ctx}: end {e} got={g:?} want={x}"),
        }
    }
}

fn assert_spans(g: &F::Spans, w: &Value, ctx: &str) {
    let want = map_spans(w);
    let got = got_spans(g);
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>(),
        "{ctx}: keys got={:?} want={:?}",
        got.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>()
    );
    for (k, wv) in &want {
        let gv = &got[k];
        assert_eq!(gv.len(), wv.len(), "{ctx}: key {k}");
        for (a, b) in gv.iter().zip(wv) {
            assert!(
                close(a[0], b[0]) && close(a[1], b[1]),
                "{ctx}: key {k} got={a:?} want={b:?}"
            );
        }
    }
}

fn assert_layout(g: &F::FunnelLayout, w: &Value, ctx: &str) {
    let a = w.as_array().unwrap();
    assert_spans(&g.dspans, &a[0], &format!("{ctx}: dspans"));
    let walls: BTreeMap<i64, [f64; 2]> = a[1]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            let p = v.as_array().unwrap();
            (k.parse().unwrap(), [f(&p[0]), f(&p[1])])
        })
        .collect();
    assert_eq!(g.walls.len(), walls.len(), "{ctx}: walls 数");
    for (k, wv) in &walls {
        let gv = g
            .walls
            .get(k)
            .unwrap_or_else(|| panic!("{ctx}: 缺 wall key {k}"));
        assert!(
            close(gv[0], wv[0]) && close(gv[1], wv[1]),
            "{ctx}: wall {k} got={gv:?} want={wv:?}"
        );
    }
    assert!(close(g.length, f(&a[2])), "{ctx}: length");
    assert!(close(g.t0, f(&a[3])), "{ctx}: t0");
    assert_eq!(g.sign, i(&a[4]), "{ctx}: sign");
}

fn assert_floats_eq(g: &[f64], w: &Value, ctx: &str) {
    let wf = floats(w);
    assert_eq!(
        g.len(),
        wf.len(),
        "{ctx}: 个数 got={} want={}",
        g.len(),
        wf.len()
    );
    for (k, (a, b)) in g.iter().zip(wf.iter()).enumerate() {
        assert!(close(*a, *b), "{ctx}: 第 {k} 个 got={a} want={b}");
    }
}

fn json_close(g: &Value, w: &Value, ctx: &str) {
    match (g, w) {
        (Value::Null, Value::Null) => {}
        (Value::Bool(a), Value::Bool(b)) => assert_eq!(a, b, "{ctx}"),
        (Value::String(a), Value::String(b)) => assert_eq!(a, b, "{ctx}"),
        (Value::Number(_), Value::Number(_)) => {
            let (x, y) = (g.as_f64().unwrap(), w.as_f64().unwrap());
            assert!(close(x, y), "{ctx}: got={g} want={w}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{ctx}: 长度 got={g} want={w}");
            for (k, (x, y)) in a.iter().zip(b).enumerate() {
                json_close(x, y, &format!("{ctx}[{k}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{ctx}: 键数 got={g} want={w}");
            for (k, x) in a {
                let y = b.get(k).unwrap_or_else(|| panic!("{ctx}: 缺键 {k}"));
                json_close(x, y, &format!("{ctx}.{k}"));
            }
        }
        _ => panic!("{ctx}: got={g} want={w}"),
    }
}

fn handle_id_of(v: &Value) -> F::HandleId {
    let a = v.as_array().expect("id 不是数组");
    match a[0].as_str().expect("id 名字不是字符串") {
        "start" => F::HandleId::Start(i(&a[1]) as usize),
        "ctrl" => F::HandleId::Ctrl(i(&a[1]) as usize, i(&a[2]) as usize, i(&a[3]) as usize),
        "anchor" => F::HandleId::Anchor(i(&a[1]) as usize, i(&a[2]) as usize, i(&a[3]) as usize),
        other => panic!("未知手柄 id {other}"),
    }
}

fn formula_of(name: &str) -> Box<dyn Fn(f64) -> Result<f64, String>> {
    match name {
        "x" => Box::new(Ok),
        "sq" => Box::new(|x| Ok(x * x)),
        "flip_sq" => Box::new(|x| Ok(1.0 - (1.0 - x) * (1.0 - x))),
        "scurve" => Box::new(|x| Ok(x * x * (3.0 - 2.0 * x))),
        "steep_s" => Box::new(|x| Ok(x * x * x * (x * (6.0 * x - 15.0) + 10.0))),
        "quarter" => Box::new(|x| Ok(1.0 - (1.0 - x * x).sqrt())),
        "reverse_s" => Box::new(|x| Ok(0.5 - ((1.0 - 2.0 * x).asin() / 3.0).sin())),
        "exp5" => Box::new(|x| Ok((5.0 * x).exp())),
        "log" => Box::new(|x| Ok((1.0 + 20.0 * x).ln())),
        "const" => Box::new(|_| Ok(1.0)),
        "nan" => Box::new(|_| Ok(f64::NAN)),
        "domain" => Box::new(|x| Ok((x - 2.0).sqrt())),
        "raise" => Box::new(|_| Err("no".to_string())),
        other => panic!("未知公式 {other}"),
    }
}

#[test]
fn funnel_vectors() {
    let mut checked = 0;
    for (idx, case) in cases("funnel").iter().enumerate() {
        let fname = case["fn"].as_str().unwrap();
        let args = case["args"].as_array().unwrap();
        let out = &case["out"];
        let is_err = case.get("err").and_then(Value::as_bool).unwrap_or(false);
        let ctx = format!("#{idx} {fname}");
        match fname {
            "line_index" => {
                assert_eq!(
                    F::line_index(i(&args[0]) as usize),
                    i(out) as usize,
                    "{ctx}"
                );
            }
            "start_point" => {
                let sh = shape_of(&args[0]);
                let g = F::start_point(&sh, f(&args[1]), i(&args[2]) as usize);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(p), false) => assert_pt(&p, out, &ctx),
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "curve_box" => {
                let sh = shape_of(&args[0]);
                let g = F::curve_box(&sh, f(&args[1]), i(&args[2]) as usize, i(&args[3]) as usize);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(b), false) => {
                        let wb = box_of(out);
                        assert_pt(&b.0, &json!(wb.0), &format!("{ctx}: S"));
                        assert_pt(&b.1, &json!(wb.1), &format!("{ctx}: U"));
                        assert_pt(&b.2, &json!(wb.2), &format!("{ctx}: V"));
                    }
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "box_point" => {
                let b = box_of(&args[0]);
                let g = F::box_point(&b, f(&args[1]), f(&args[2]));
                assert_pt(&g, out, &ctx);
            }
            "box_uf" => {
                let b = box_of(&args[0]);
                let g = F::box_uf(&b, f(&args[1]), f(&args[2]));
                assert_pt(&g, out, &ctx);
            }
            "funnel_lines" => {
                let sh = shape_of(&args[0]);
                assert_segs(&F::funnel_lines(&sh), out, &ctx);
            }
            "funnel_segments" => {
                let sh = shape_of(&args[0]);
                assert_segs(&F::funnel_segments(&sh), out, &ctx);
            }
            "funnel_strokes" => {
                let sh = shape_of(&args[0]);
                assert_polys(&F::funnel_strokes(&sh), out, &ctx);
            }
            "funnel_curves" => {
                let sh = shape_of(&args[0]);
                let g = F::funnel_curves(&sh, b(&args[1]));
                let a = out.as_array().unwrap();
                assert_eq!(g.len(), a.len(), "{ctx}: 曲线数");
                for (k, (item, wv)) in g.iter().zip(a).enumerate() {
                    let arr = wv.as_array().unwrap();
                    assert_eq!(item.0, i(&arr[0]) as usize, "{ctx}: curve {k} start");
                    assert_eq!(item.1, i(&arr[1]) as usize, "{ctx}: curve {k} end");
                    assert_poly(&item.2, &arr[2], &format!("{ctx}: curve {k} pts"));
                    assert_pt(&item.3, &arr[3], &format!("{ctx}: curve {k} corner"));
                }
            }
            "funnel_polys" => {
                let sh = shape_of(&args[0]);
                assert_polys(&F::funnel_polys(&sh, b(&args[1])), out, &ctx);
            }
            "funnel_contains" => {
                let sh = shape_of(&args[0]);
                assert_eq!(
                    F::funnel_contains(&sh, f(&args[1]), f(&args[2])),
                    b(out),
                    "{ctx}"
                );
            }
            "funnel_reversed" => {
                assert_eq!(F::funnel_reversed(&shape_of(&args[0])), b(out), "{ctx}");
            }
            "line_band" => {
                let g = F::line_band(pt_of(&args[0]), pt_of(&args[1]), i(&args[2]));
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some((a, bb)), false) => {
                        assert!(
                            close(a, f(&out[0])) && close(bb, f(&out[1])),
                            "{ctx}: got=({a},{bb}) want={out}"
                        );
                    }
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "funnel_key_spans" => {
                let sh = shape_of(&args[0]);
                assert_spans(&F::funnel_key_spans(&sh), out, &ctx);
            }
            "funnel_axis" => {
                let sh = shape_of(&args[0]);
                let g = F::funnel_axis(&sh, &F::funnel_key_spans(&sh));
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some((t0, sign, length)), false) => {
                        assert!(close(t0, f(&out[0])), "{ctx}: t0");
                        assert_eq!(sign, i(&out[1]), "{ctx}: sign");
                        assert!(close(length, f(&out[2])), "{ctx}: length");
                    }
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "funnel_layout" => {
                let sh = shape_of(&args[0]);
                let g = F::funnel_layout(&sh, f(&args[1]));
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(lay), false) => assert_layout(&lay, out, &ctx),
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "funnel_grid" => {
                let sh = shape_of(&args[0]);
                let ppq = f(&args[1]);
                let lay = F::funnel_layout(&sh, ppq).expect("layout");
                assert_floats_eq(
                    &F::funnel_grid(&sh, ppq, &lay.dspans, lay.length),
                    out,
                    &ctx,
                );
            }
            "funnel_grid_spans" => {
                let sh = shape_of(&args[0]);
                let spans = spans_from(&args[2]);
                let g = F::funnel_grid(&sh, f(&args[1]), &spans, f(&args[3]));
                assert_floats_eq(&g, out, &ctx);
            }
            "funnel_openness" => {
                let sh = shape_of(&args[0]);
                let ppq = f(&args[1]);
                let g = F::funnel_layout(&sh, ppq).map(|lay| F::funnel_openness(&lay.dspans));
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(o), false) => {
                        let ds = floats(&args[2]);
                        let ws: Vec<f64> = ds.iter().map(|d| o.w(*d)).collect();
                        assert_floats_eq(&ws, out, &ctx);
                    }
                    _ => panic!("{ctx}: got 状态与 want={out} 不一致"),
                }
            }
            "funnel_openness_spans" => {
                let spans = spans_from(&args[0]);
                let o = F::funnel_openness(&spans);
                let ds = floats(&args[1]);
                let ws: Vec<f64> = ds.iter().map(|d| o.w(*d)).collect();
                assert_floats_eq(&ws, out, &ctx);
            }
            "funnel_gate" => {
                let sh = shape_of(&args[0]);
                let g = F::funnel_gate(&sh, f(&args[1]), f(&args[2]), f(&args[3]));
                assert!(close(g, f(out)), "{ctx}: got={g} want={out}");
            }
            "funnel_cells" => {
                let sh = shape_of(&args[0]);
                let (ticks, cells) = F::funnel_cells(&sh, f(&args[1]));
                let a = out.as_array().unwrap();
                if a[0].is_null() {
                    assert!(ticks.is_none(), "{ctx}: ticks got={ticks:?}");
                } else {
                    let wt: Vec<i64> = a[0].as_array().unwrap().iter().map(i).collect();
                    assert_eq!(ticks.as_deref(), Some(wt.as_slice()), "{ctx}: ticks");
                }
                assert_rows3_eq(&cells, &a[1], &ctx);
            }
            "funnel_note_count" => {
                let sh = shape_of(&args[0]);
                assert_eq!(F::funnel_note_count(&sh, f(&args[1])), i(out), "{ctx}");
            }
            "funnel_notes" => {
                let sh = shape_of(&args[0]);
                assert_rows3_eq(&F::funnel_notes(&sh, f(&args[1])), out, &ctx);
            }
            "new_start" => {
                let sh = shape_of(&args[0]);
                let g = F::new_start(&sh, f(&args[1]), i(&args[2]) as usize);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(st), false) => assert_start(&st, out, &ctx),
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "next_link" => {
                assert_eq!(F::next_link(&shape_of(&args[0])), i(out), "{ctx}");
            }
            "partners" => {
                let sh = shape_of(&args[0]);
                let g = F::partners(&sh, i(&args[1]) as usize, i(&args[2]) as usize);
                if is_err {
                    assert!(g.is_empty(), "{ctx}: 越界应返回空 got={g:?}");
                } else {
                    let a = out.as_array().unwrap();
                    assert_eq!(g.len(), a.len(), "{ctx}");
                    for (item, wv) in g.iter().zip(a) {
                        let arr = wv.as_array().unwrap();
                        assert_eq!(item.0, i(&arr[0]) as usize, "{ctx}");
                        assert_eq!(item.1, i(&arr[1]) as usize, "{ctx}");
                        assert_eq!(item.2, b(&arr[2]), "{ctx}");
                    }
                }
            }
            "turned" => {
                assert_poly(&F::turned(&pts(&args[0])), out, &ctx);
            }
            "turned_curve" => {
                assert_curve(
                    &F::turned_curve(&curve_of(&args[0]), b(&args[1])),
                    out,
                    &ctx,
                );
            }
            "inside_out" => {
                assert_curve(&F::inside_out(&curve_of(&args[0])), out, &ctx);
            }
            "set_shape" => {
                let mut c = curve_of(&args[0]);
                F::set_shape(&mut c, &curve_of(&args[1]));
                assert_curve(&c, out, &ctx);
            }
            "funnel_handles" => {
                let sh = shape_of(&args[0]);
                let g = F::funnel_handles(&sh);
                let a = out.as_array().unwrap();
                assert_eq!(g.len(), a.len(), "{ctx}: 手柄数");
                for (item, wv) in g.iter().zip(a) {
                    let arr = wv.as_array().unwrap();
                    assert!(
                        close(item.0[0], f(&arr[0])) && close(item.0[1], f(&arr[1])),
                        "{ctx}: 点 got={:?} want={wv}",
                        item.0
                    );
                    assert_eq!(item.1, handle_id_of(&arr[2]), "{ctx}: id");
                }
            }
            "funnel_handle_lines" => {
                let sh = shape_of(&args[0]);
                let g = F::funnel_handle_lines(&sh);
                let a = out.as_array().unwrap();
                assert_eq!(g.len(), a.len(), "{ctx}: 手柄线数");
                for (item, wv) in g.iter().zip(a) {
                    let arr = wv.as_array().unwrap();
                    assert_pt(&item.0, &arr[0], &format!("{ctx}: anchor"));
                    assert_pt(&item.1, &arr[1], &format!("{ctx}: handle"));
                    let pair = arr[2].as_array().unwrap();
                    assert_eq!(item.2.0, i(&pair[0]) as usize, "{ctx}");
                    assert_eq!(item.2.1, i(&pair[1]) as usize, "{ctx}");
                }
            }
            "remove_funnel_parts" => {
                let mut sh = shape_of(&args[0]);
                let lines: Vec<usize> = args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| i(x) as usize)
                    .collect();
                let curves: Vec<(usize, usize)> = args[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| {
                        let a = x.as_array().unwrap();
                        (i(&a[0]) as usize, i(&a[1]) as usize)
                    })
                    .collect();
                let ok = F::remove_funnel_parts(&mut sh, &lines, &curves);
                let a = out.as_array().unwrap();
                assert_eq!(ok, b(&a[0]), "{ctx}: 返回值");
                assert_poly(&sh.pts, &a[1], &format!("{ctx}: pts"));
                let ws = a[2].as_array().unwrap();
                assert_eq!(sh.starts.len(), ws.len(), "{ctx}: starts 数");
                for (k, (g, wv)) in sh.starts.iter().zip(ws).enumerate() {
                    assert_start(g, wv, &format!("{ctx}: start {k}"));
                }
            }
            "clean_funnel" => {
                let g = F::clean_funnel(&args[0]);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(s), false) => {
                        assert_eq!(s.fill.name(), out["fill"].as_str().unwrap(), "{ctx}: fill");
                        assert!(close(s.gate0, f(&out["gate0"])), "{ctx}: gate0");
                        assert!(close(s.gate1, f(&out["gate1"])), "{ctx}: gate1");
                        assert_eq!(s.vary, b(&out["vary"]), "{ctx}: vary");
                        assert_eq!(
                            s.change.name(),
                            out["change"].as_str().unwrap(),
                            "{ctx}: change"
                        );
                        assert_eq!(
                            s.follow.name(),
                            out["follow"].as_str().unwrap(),
                            "{ctx}: follow"
                        );
                        assert_eq!(s.wall.name(), out["wall"].as_str().unwrap(), "{ctx}: wall");
                    }
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "clean_starts" => {
                let g = F::clean_starts(&args[0], i(&args[1]) as usize);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(starts), false) => {
                        let a = out.as_array().unwrap();
                        assert_eq!(starts.len(), a.len(), "{ctx}: starts 数");
                        for (k, (g, wv)) in starts.iter().zip(a).enumerate() {
                            assert_start(g, wv, &format!("{ctx}: start {k}"));
                        }
                    }
                    _ => panic!("{ctx}: got 状态与 want={out} 不一致"),
                }
            }
            "clean_curve" => {
                let g = F::clean_curve(&args[0]);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(c), false) => assert_curve(&c, out, &ctx),
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "clean_bends" => {
                let g = F::clean_bends(&args[0]);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(b), false) => assert_poly(&b, out, &ctx),
                    pair => panic!("{ctx}: got={pair:?} want={out}"),
                }
            }
            "old_funnel" => {
                let g = F::old_funnel(&args[0]);
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some((p, starts)), false) => {
                        let a = out.as_array().unwrap();
                        assert_poly(&p, &a[0], &format!("{ctx}: pts"));
                        json_close(&starts, &a[1], &format!("{ctx}: starts"));
                    }
                    _ => panic!("{ctx}: got 状态与 want={out} 不一致"),
                }
            }
            "old_curve_points" => {
                let g = F::old_curve_points(&pts(&args[0]));
                match (g, out.is_null()) {
                    (None, true) => {}
                    (Some(p), false) => assert_poly(&p, out, &ctx),
                    _ => panic!("{ctx}: got 状态与 want={out} 不一致"),
                }
            }
            "funnel_f" => {
                assert!(
                    close(F::funnel_f(bend_of(&args[0]), f(&args[1])), f(out)),
                    "{ctx}"
                );
            }
            "funnel_u" => {
                assert!(
                    close(F::funnel_u(bend_of(&args[0]), f(&args[1])), f(out)),
                    "{ctx}"
                );
            }
            "smooth_curve" => {
                let sc =
                    F::smooth_curve(&floats(&args[0]), &floats(&args[1])).expect("smooth_curve");
                let us = floats(&args[2]);
                let g: Vec<f64> = us.iter().map(|u| sc.eval(*u)).collect();
                assert_floats_eq(&g, out, &ctx);
            }
            "formula_curve" => {
                let fform = formula_of(args[0].as_str().unwrap());
                let g = F::formula_curve(fform.as_ref(), i(&args[1]) as usize);
                if is_err {
                    assert!(g.is_err(), "{ctx}: 应报错 got={g:?}");
                } else {
                    assert_poly(&g.expect("formula_curve"), out, &ctx);
                }
            }
            "preset_curve" => {
                let fform = args[0].as_str().map(formula_of);
                let g = F::preset_curve(fform.as_deref());
                if is_err {
                    assert!(g.is_err(), "{ctx}: 应报错 got={g:?}");
                } else {
                    assert_curve(&g.expect("preset_curve"), out, &ctx);
                }
            }
            "curve_presets" => {
                let a = out.as_array().unwrap();
                assert_eq!(F::CURVE_PRESETS.len(), a.len(), "{ctx}: 预设数");
                for (k, wv) in a.iter().enumerate() {
                    let item = wv.as_array().unwrap();
                    let (name, formula) = F::CURVE_PRESETS[k];
                    assert_eq!(name, item[0].as_str().unwrap(), "{ctx}: 名字 {k}");
                    assert_eq!(formula, item[1].as_str(), "{ctx}: 公式 {k}");
                }
            }
            other => panic!("未知用例 {other}"),
        }
        checked += 1;
    }
    assert!(checked > 150, "用例太少：{checked}");
}
