//! custom：画笔在卷轴上画出的自定义形状与粘贴音符，对应 Python notes/custom.py 的逐函数移植。
//!
//! 自定义形状是画笔在自己框里的画：笔画用 u = 0..1（左到右）与 v = 0..1（下到上）表示，
//! 有折线（poly）、贝塞尔曲线（curve，见 [`crate::bezier`]）、三点弧（arc，见 [`crate::arc`]）
//! 与椭圆（ellipse）四种。在卷轴上 `sh.pts` 是框的三个角 `[u=0 v=0, u=1 v=0, u=0 v=1]`，
//! 移动、翻转、旋转形状只是移动这三个点，画会跟着走。
//!
//! 内部填充方式（[`FILLS`]）：empty = 只有轮廓；fill = 每个 key 内的每段一个音符；
//! spam = 每段按门限铺满；outline_spam = 轮廓按门限切断、内部留空。
//! 形状也可以不装画而装粘贴的音符：`sh.notes` 是 [`pack_notes`] 的文本（zlib + base64 的
//! little-endian int32 行），与 Python 版字节级互通。
//!
//! 与原版的差异（都用 Option / Result 代替 Python 的异常）：
//! - 框不够三个点时 `frame_to_bp` / `frame_to_uv` 返回 None（Python 抛 ValueError）；
//! - `add_stroke` 在框退化（`frame_to_uv` 为 None）时返回 None；
//! - `notes_shape` 的行数为 0 时返回 None；
//! - `unpack_notes` / `block_notes` 用 [`NotesError`] 报坏数据（Python 抛异常）；
//! - `clean_strokes` / `clean_curve` 接受 JSON 值，`int` / `float` 的边角（下划线数字、
//!   容器 repr）不复刻，坏值一律按原版跳过。

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::sync::LazyLock;

use base64::Engine as _;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use serde_json::Value;

use crate::arc::{STEP as ARC_STEP, arc_points, clean_k, ellipse_bezier};
use crate::bezier::sample as bezier_sample;
use crate::paths::{dedupe, keep_longest, line_notes, loop_from_left, pitch_of, stretch_ends};
use crate::shape::{Align, Fill, Kind, Shape, Stroke, Sym};
use crate::smooth::{clean_level, smooth_path};
use crate::text::{text_polys, threshold_spans};
use crate::{Pt, dist, floor_half, hypot2, round_half_even, round_i64};

/// 填充方式（custom.FILLS）。
pub const FILLS: [Fill; 4] = [Fill::Empty, Fill::Fill, Fill::Spam, Fill::OutlineSpam];
/// 用门限与 spam 起点的填充（custom.SPAM_FILLS）。
pub const SPAM_FILLS: [Fill; 2] = [Fill::Spam, Fill::OutlineSpam];
/// spam 起点对齐（custom.ALIGNS）。
pub const ALIGNS: [Align; 2] = [Align::Auto, Align::Aligned];

/// 自定义形状的默认设置（custom.CUSTOM_DEFAULTS）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomDefaults {
    pub fill: Fill,
    pub gate: f64,
    pub align: Align,
}

impl Default for CustomDefaults {
    fn default() -> Self {
        Self {
            fill: Fill::Empty,
            gate: 0.0625,
            align: Align::Auto,
        }
    }
}

/// CUSTOM_DEFAULTS 的值（fill = empty，gate = 1/16 拍 = PPQ 960 的 60 ticks，align = auto）。
pub const CUSTOM_DEFAULTS: CustomDefaults = CustomDefaults {
    fill: Fill::Empty,
    gate: 0.0625,
    align: Align::Auto,
};

/// 椭圆采样点数（custom.ELLIPSE_STEPS）。
pub const ELLIPSE_STEPS: usize = 360;
/// 曲线每段的采样点数（custom.CURVE_STEPS）。
pub const CURVE_STEPS: usize = 240;

/// 粘贴音符形状的轮廓：就是一个框（custom.BOX_STROKE）。
pub static BOX_STROKE: LazyLock<Stroke> = LazyLock::new(|| Stroke::Poly {
    pts: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
    free: false,
    smooth: 0,
    k: 1.0,
});

/// 带 track 列的 packed 音符的前缀（custom.TRACKS）。
pub const TRACKS: &str = "t:";

/// 粘贴音符的打包 / 解包错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotesError {
    #[error("形状里没有粘贴的音符")]
    Missing,
    #[error("base64 解码失败")]
    Base64,
    #[error("zlib 解压失败")]
    Zlib,
    #[error("音符数据不是 4 / 5 列 int32 行")]
    Shape,
    #[error("形状的框缺少三个点")]
    Frame,
}

// ---------------------------------------------------------------- 设置清理

/// Python `float(x)`：数字、布尔与数字字符串；别的（含 null）失败。
fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Python `int(x)`：数字截断、布尔与整数字符串；别的失败。
fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok()
            } else {
                n.as_f64().map(|f| f.trunc() as i64)
            }
        }
        Value::Bool(b) => Some(i64::from(*b)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Python 真值判断：0 / 空串 / 空表 / null 为假。
fn py_bool(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `st.get("k", 1.0)` 的 float() + [`clean_k`]（custom 里的 `arc_k`）。
fn arc_k(st: &Value) -> f64 {
    match st.get("k") {
        None => 1.0,
        Some(v) => match py_float(v) {
            Some(k) => clean_k(k),
            None => 1.0,
        },
    }
}

/// Python `round(x, 6)`（银行家舍入）。
fn round6(x: f64) -> f64 {
    let m = 1e6;
    round_half_even(x * m) / m
}

/// b > 0 时的向上取整除法（对应 Python 的 `-(-a // b)`）。
fn div_ceil_pos(a: i64, b: i64) -> i64 {
    let q = a.div_euclid(b);
    if a.rem_euclid(b) == 0 { q } else { q + 1 }
}

/// 一个点列 / 点对子：长度为 2 的数组或字符串（Python 的可解包序列）。
fn pair_of(v: &Value) -> Option<[f64; 2]> {
    match v {
        Value::Array(a) if a.len() == 2 => Some([py_float(&a[0])?, py_float(&a[1])?]),
        Value::String(s) if s.chars().count() == 2 => {
            let mut it = s.chars();
            let u = py_float(&Value::String(it.next()?.to_string()))?;
            let v = py_float(&Value::String(it.next()?.to_string()))?;
            Some([u, v])
        }
        _ => None,
    }
}

/// `[[float(u), float(v)] for u, v in st["pts"]]`。
fn point_list(v: &Value) -> Option<Vec<Pt>> {
    match v {
        Value::Array(a) => a.iter().map(pair_of).collect(),
        _ => None,
    }
}

/// `[float(x) for x in st["box"]]`（数组或字符串，长度在调用方检查）。
fn float_list(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Array(a) => a.iter().map(py_float).collect(),
        Value::String(s) => s
            .chars()
            .map(|c| py_float(&Value::String(c.to_string())))
            .collect(),
        _ => None,
    }
}

/// 一条笔画从文件里读出的清洗（custom.clean_curve）。
pub fn clean_curve(st: &Value, pts: &[Pt]) -> Stroke {
    let last = (pts.len() as i64 - 1) / 3;
    let sharp = curve_sharp(st, last);
    let sym = curve_sym(st, last);
    Stroke::Curve {
        pts: pts.to_vec(),
        sharp,
        sym,
    }
}

/// `sorted({int(a) for a in st.get("sharp", ()) if 0 < int(a) < last})`：
/// 一个值坏掉就整张表作废（Python 的 try 包住整个推导）。
fn curve_sharp(st: &Value, last: i64) -> Vec<usize> {
    let Some(v) = st.get("sharp") else {
        return Vec::new();
    };
    let values: Vec<i64> = match v {
        Value::Array(a) => {
            let mut out = Vec::with_capacity(a.len());
            for x in a {
                match py_int(x) {
                    Some(i) => out.push(i),
                    None => return Vec::new(),
                }
            }
            out
        }
        Value::String(s) => {
            let mut out = Vec::new();
            for c in s.chars() {
                match c.to_string().trim().parse::<i64>() {
                    Ok(i) => out.push(i),
                    Err(_) => return Vec::new(),
                }
            }
            out
        }
        _ => return Vec::new(),
    };
    let mut sharp: Vec<i64> = values.into_iter().filter(|&a| a > 0 && a < last).collect();
    sharp.sort_unstable();
    sharp.dedup();
    sharp.into_iter().map(|a| a as usize).collect()
}

/// `st.get("sym") in ("mirror", "turn") and last % 2 == 0`。
fn curve_sym(st: &Value, last: i64) -> Option<Sym> {
    if last % 2 != 0 {
        return None;
    }
    match st.get("sym").and_then(Value::as_str) {
        Some("mirror") => Some(Sym::Mirror),
        Some("turn") => Some(Sym::Turn),
        _ => None,
    }
}

/// 笔画列表从文件里读出的清洗（custom.clean_strokes）：坏数据跳过。
pub fn clean_strokes(strokes: &Value) -> Vec<Stroke> {
    let Some(items) = strokes.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for st in items {
        let Some(d) = st.as_object() else {
            continue;
        };
        if d.get("kind").and_then(Value::as_str) == Some("ellipse") {
            let Some(box_) = d.get("box").and_then(float_list) else {
                continue;
            };
            if box_.len() == 4 {
                out.push(Stroke::Ellipse {
                    box_: [box_[0], box_[1], box_[2], box_[3]],
                });
            }
            continue;
        }
        let Some(pts) = d.get("pts").and_then(point_list) else {
            continue;
        };
        match d.get("kind").and_then(Value::as_str) {
            Some("curve") => {
                if pts.len() >= 4 {
                    let n = pts.len() - (pts.len() - 1) % 3;
                    out.push(clean_curve(st, &pts[..n]));
                }
            }
            Some("arc") => {
                if pts.len() == 3 {
                    out.push(Stroke::Arc { pts, k: arc_k(st) });
                }
            }
            _ => {
                if !pts.is_empty() {
                    let free = d.get("free").is_some_and(py_bool);
                    let smooth =
                        clean_level(d.get("smooth").and_then(py_float).unwrap_or(f64::NAN));
                    out.push(Stroke::Poly {
                        pts,
                        free,
                        smooth,
                        k: arc_k(st),
                    });
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- 笔画 -> 点

/// 一条笔画作为 (u, v) 点列；椭圆从最左点开始、终点精确回到起点（custom.stroke_points）。
pub fn stroke_points(st: &Stroke) -> Vec<Pt> {
    match st {
        Stroke::Ellipse { box_ } => {
            let (u0, v0, u1, v1) = (box_[0], box_[1], box_[2], box_[3]);
            let (cu, cv, ru, rv) = (
                (u0 + u1) / 2.0,
                (v0 + v1) / 2.0,
                (u1 - u0) / 2.0,
                (v1 - v0) / 2.0,
            );
            let mut pts = Vec::with_capacity(ELLIPSE_STEPS + 1);
            for i in 0..ELLIPSE_STEPS {
                let a = 2.0 * std::f64::consts::PI * i as f64 / ELLIPSE_STEPS as f64;
                pts.push([cu - ru * a.cos(), cv + rv * a.sin()]);
            }
            pts.push(pts[0]);
            pts
        }
        Stroke::Curve { pts, .. } => bezier_sample(pts, CURVE_STEPS),
        Stroke::Arc { pts, k } => arc_points(pts, *k, ARC_STEP),
        Stroke::Poly { pts, smooth, k, .. } => {
            if *smooth != 0 {
                smooth_path(pts, *smooth as f64, *k)
            } else {
                pts.clone()
            }
        }
    }
}

/// 笔画的原始点（ellipse 没有；用于闭合判断等）。
fn stroke_raw_pts(st: &Stroke) -> Option<&Vec<Pt>> {
    match st {
        Stroke::Poly { pts, .. } | Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. } => Some(pts),
        Stroke::Ellipse { .. } => None,
    }
}

/// 路径是否闭合：点数 ≥ 3 且首尾距离 < 1e-6（custom.path_closed）。
pub fn path_closed(path: &[Pt]) -> bool {
    path.len() >= 3 && dist(path[0], path[path.len() - 1]) < 1e-6
}

/// 笔画是否闭合（椭圆总是；别的看原始点，custom.stroke_closed）。
pub fn stroke_closed(st: &Stroke) -> bool {
    if matches!(st, Stroke::Ellipse { .. }) {
        return true;
    }
    stroke_raw_pts(st).is_some_and(|pts| path_closed(pts))
}

/// 首尾相接的点列合成一条（custom.join_paths）：闭合的留在前面，按原顺序。
pub fn join_paths(paths: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    let meet = |a: Pt, b: Pt| dist(a, b) < 1e-6;
    let mut done: Vec<Vec<Pt>> = Vec::new();
    let mut lines: Vec<Vec<Pt>> = Vec::new();
    for p in paths {
        if path_closed(p) {
            done.push(p.clone());
        } else {
            lines.push(p.clone());
        }
    }
    let mut joined = true;
    while joined {
        joined = false;
        'outer: for i in 0..lines.len() {
            for j in 0..lines.len() {
                if i == j {
                    continue;
                }
                let (a, b) = (&lines[i], &lines[j]);
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                let (a0, al) = (a[0], a[a.len() - 1]);
                let (b0, bl) = (b[0], b[b.len() - 1]);
                let new: Vec<Pt> = if meet(al, b0) {
                    let mut v = a.clone();
                    v.extend_from_slice(&b[1..]);
                    v
                } else if meet(al, bl) {
                    let mut v = a.clone();
                    v.extend(b[..b.len() - 1].iter().rev().copied());
                    v
                } else if meet(a0, b0) {
                    let mut v: Vec<Pt> = a.iter().rev().copied().collect();
                    v.extend_from_slice(&b[1..]);
                    v
                } else {
                    continue;
                };
                lines[i] = new;
                lines.remove(j);
                joined = true;
                break 'outer;
            }
        }
    }
    done.extend(lines);
    done
}

/// 笔画的两端 `[起点, 终点]`；闭合时为 None（曲线按原始点判断，采样太慢，custom.stroke_span）。
pub fn stroke_span(st: &Stroke) -> Option<[Pt; 2]> {
    let (pts, closed) = match st {
        Stroke::Curve { pts, .. } => {
            if pts.is_empty() {
                return None;
            }
            (pts.clone(), dist(pts[0], pts[pts.len() - 1]) < 1e-6)
        }
        _ => {
            let pts = stroke_points(st);
            let closed = path_closed(&pts);
            (pts, closed)
        }
    };
    if closed || pts.is_empty() {
        None
    } else {
        Some([pts[0], pts[pts.len() - 1]])
    }
}

/// 相接的笔画连起来后，图形里开放的轮廓（custom.open_paths）：每条都是一个缺口。
pub fn open_paths(strokes: &[Stroke]) -> Vec<Vec<Pt>> {
    let spans: Vec<Vec<Pt>> = strokes
        .iter()
        .filter_map(stroke_span)
        .map(|s| s.to_vec())
        .collect();
    join_paths(&spans)
        .into_iter()
        .filter(|p| !path_closed(p))
        .collect()
}

/// 连起来后图形所有松着的端（custom.open_ends）。
pub fn open_ends(strokes: &[Stroke]) -> Vec<Pt> {
    let mut out = Vec::new();
    for path in open_paths(strokes) {
        out.push(path[0]);
        out.push(path[path.len() - 1]);
    }
    out
}

/// 每条轮廓都回到起点（custom.strokes_closed）。
pub fn strokes_closed(strokes: &[Stroke]) -> bool {
    !strokes.is_empty() && open_paths(strokes).is_empty()
}

/// Fill / Spam 能用：轮廓闭合，或只有一个缺口（custom.fillable）。
pub fn fillable(strokes: &[Stroke]) -> bool {
    !strokes.is_empty() && open_paths(strokes).len() <= 1
}

/// 只有一个缺口的形状：把它补上的直线，两点 (beat, pitch)（从终点回起点）。
/// 文本形状或缺口数不为 1 时为 None（custom.gap_line）。
pub fn gap_line(sh: &Shape) -> Option<[Pt; 2]> {
    if sh.text.is_some() {
        return None;
    }
    let paths = open_paths(&sh.strokes);
    if paths.len() != 1 {
        return None;
    }
    let to_bp = frame_to_bp(&sh.pts)?;
    let path = &paths[0];
    Some([
        to_bp(path[path.len() - 1][0], path[path.len() - 1][1]),
        to_bp(path[0][0], path[0][1]),
    ])
}

/// 首尾相接的开放折线连成一条（曲线与圆保持原样，custom.join_strokes）。
pub fn join_strokes(strokes: &[Stroke]) -> Vec<Stroke> {
    let mut others: Vec<Stroke> = Vec::new();
    let mut lines: Vec<Vec<Pt>> = Vec::new();
    for st in strokes {
        if matches!(st, Stroke::Poly { .. }) && !stroke_closed(st) {
            if let Some(pts) = stroke_raw_pts(st) {
                lines.push(pts.clone());
            }
        } else {
            others.push(st.clone());
        }
    }
    for pts in join_paths(&lines) {
        others.push(Stroke::Poly {
            pts,
            free: false,
            smooth: 0,
            k: 1.0,
        });
    }
    others
}

/// 框的三个角点（custom.frame_to_bp 用的内部表示）。
fn frame3(pts: &[Pt]) -> Option<[Pt; 3]> {
    Some([*pts.first()?, *pts.get(1)?, *pts.get(2)?])
}

/// 自定义形状的点列（custom.custom_strokes）：文本形状是字母轮廓，别的按框投影。
pub fn custom_strokes(sh: &Shape) -> Vec<Vec<Pt>> {
    if sh.text.is_some() {
        return text_polys(sh);
    }
    let Some([a, b, c]) = frame3(&sh.pts) else {
        return Vec::new();
    };
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    sh.strokes
        .iter()
        .map(|st| {
            stroke_points(st)
                .into_iter()
                .map(|[u, v]| [b0 + u * ub + v * vb, p0 + u * up + v * vp])
                .collect()
        })
        .collect()
}

/// 从 (b0, p0) 到 (b1, p1) 的框的三个角点（custom.box_frame）。
pub fn box_frame(b0: f64, p0: f64, b1: f64, p1: f64) -> [Pt; 3] {
    let (bl, bh) = (b0.min(b1), b0.max(b1));
    let (pl, ph) = (p0.min(p1), p0.max(p1));
    [[bl, pl], [bh, pl], [bl, ph]]
}

/// 笔画拉伸到正好填满 0..1 的框，返回（新笔画, 宽 / 高；平的时候 None）（custom.normalize_strokes）。
pub fn normalize_strokes(strokes: &[Stroke]) -> (Vec<Stroke>, Option<f64>) {
    let pts: Vec<Pt> = strokes.iter().flat_map(stroke_points).collect();
    if pts.is_empty() {
        return (Vec::new(), None);
    }
    let ul = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let vl = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let w = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max) - ul;
    let h = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max) - vl;
    let fix = |u: f64, v: f64| -> Pt {
        [
            if w > 1e-9 { round6((u - ul) / w) } else { 0.5 },
            if h > 1e-9 { round6((v - vl) / h) } else { 0.5 },
        ]
    };
    let mut out = Vec::with_capacity(strokes.len());
    for st in strokes {
        if let Stroke::Ellipse { box_ } = st {
            let a = fix(box_[0], box_[1]);
            let b = fix(box_[2], box_[3]);
            out.push(Stroke::Ellipse {
                box_: [a[0], a[1], b[0], b[1]],
            });
            continue;
        }
        let mut new = st.clone();
        let stretch = w > 1e-9 && h > 1e-9;
        match &mut new {
            Stroke::Poly { pts, free, k, .. } => {
                for p in pts.iter_mut() {
                    *p = fix(p[0], p[1]);
                }
                if *free && stretch {
                    *k = *k * h / w;
                }
            }
            Stroke::Curve { pts, .. } => {
                for p in pts.iter_mut() {
                    *p = fix(p[0], p[1]);
                }
            }
            Stroke::Arc { pts, k } => {
                for p in pts.iter_mut() {
                    *p = fix(p[0], p[1]);
                }
                if stretch {
                    *k = *k * h / w;
                }
            }
            Stroke::Ellipse { .. } => {}
        }
        out.push(new);
    }
    let ratio = if w > 1e-9 && h > 1e-9 {
        Some(w / h)
    } else {
        None
    };
    (out, ratio)
}

// ---------------------------------------------------------------- 现场画（卷轴上直接画的笔画）

/// (u, v) -> (beat, pitch)（custom.frame_to_bp）；框不够三点时 None。
pub fn frame_to_bp(pts: &[Pt]) -> Option<impl Fn(f64, f64) -> Pt + use<>> {
    let [a, b, c] = frame3(pts)?;
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    Some(move |u: f64, v: f64| [b0 + u * ub + v * vb, p0 + u * up + v * vp])
}

/// (beat, pitch) -> (u, v)；框退化（平的）或不够三点时 None（custom.frame_to_uv）。
pub fn frame_to_uv(pts: &[Pt]) -> Option<impl Fn(f64, f64) -> Pt + use<>> {
    let [a, b, c] = frame3(pts)?;
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    let det = ub * vp - up * vb;
    if det.abs() < 1e-12 {
        return None;
    }
    Some(move |b: f64, p: f64| {
        [
            ((b - b0) * vp - (p - p0) * vb) / det,
            (ub * (p - p0) - up * (b - b0)) / det,
        ]
    })
}

/// 形状的框在屏幕上（每 key k 拍）一个 v 是几个 u（custom.uv_k）；退化时 1.0。
pub fn uv_k(pts: &[Pt], k: f64) -> f64 {
    let Some([a, b, c]) = frame3(pts) else {
        return 1.0;
    };
    let lu = hypot2((b[0] - a[0]) / k, b[1] - a[1]);
    let lv = hypot2((c[0] - a[0]) / k, c[1] - a[1]);
    if lu > 1e-12 && lv > 1e-12 {
        lv / lu
    } else {
        1.0
    }
}

/// 框没被转动（u 沿时间、v 沿音高），椭圆在里头还是椭圆（custom.frame_upright）。
pub fn frame_upright(pts: &[Pt]) -> bool {
    let Some([a, b, c]) = frame3(pts) else {
        return false;
    };
    (b[1] - a[1]).abs() < 1e-12 && (c[0] - a[0]).abs() < 1e-12
}

/// 笔画每个点过一遍 fn(u, v) -> (u, v)（custom.map_stroke）。
/// su / sv：这让笔画宽 / 高了几倍（弧保持圆、椭圆框方向正确用）。
pub fn map_stroke<F: Fn(f64, f64) -> Pt>(st: &Stroke, f: F, su: f64, sv: f64) -> Stroke {
    if let Stroke::Ellipse { box_ } = st {
        let a = f(box_[0], box_[1]);
        let b = f(box_[2], box_[3]);
        return Stroke::Ellipse {
            box_: [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[0].max(b[0]),
                a[1].max(b[1]),
            ],
        };
    }
    let mut new = st.clone();
    let scale = (su / sv).abs();
    match &mut new {
        Stroke::Poly { pts, free, k, .. } => {
            for p in pts.iter_mut() {
                *p = f(p[0], p[1]);
            }
            if *free {
                *k *= scale;
            }
        }
        Stroke::Curve { pts, .. } => {
            for p in pts.iter_mut() {
                *p = f(p[0], p[1]);
            }
        }
        Stroke::Arc { pts, k } => {
            for p in pts.iter_mut() {
                *p = f(p[0], p[1]);
            }
            *k *= scale;
        }
        Stroke::Ellipse { .. } => {}
    }
    new
}

/// 给形状的框重新套住它的画（custom.refit）：笔画回到填满 0..1，框点跟着挪，卷轴上不动。
pub fn refit(sh: &mut Shape) {
    let pts: Vec<Pt> = sh.strokes.iter().flat_map(stroke_points).collect();
    if pts.is_empty() {
        return;
    }
    let mut ul = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let mut vl = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let mut w = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max) - ul;
    let mut h = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max) - vl;
    if w < 1e-9 {
        ul -= 0.5;
        w = 1.0;
    }
    if h < 1e-9 {
        vl -= 0.5;
        h = 1.0;
    }
    if ul.abs() < 1e-12 && vl.abs() < 1e-12 && (w - 1.0).abs() < 1e-12 && (h - 1.0).abs() < 1e-12 {
        return;
    }
    let Some(to_bp) = frame_to_bp(&sh.pts) else {
        return;
    };
    let new_pts = vec![to_bp(ul, vl), to_bp(ul + w, vl), to_bp(ul, vl + h)];
    let new_strokes: Vec<Stroke> = sh
        .strokes
        .iter()
        .map(|st| map_stroke(st, |u, v| [(u - ul) / w, (v - vl) / h], 1.0 / w, 1.0 / h))
        .collect();
    sh.pts = new_pts;
    sh.strokes = new_strokes;
}

/// 别的笔画可以接上去的点：折线的每个点、曲线与弧的两端（custom.stroke_ends）。
pub fn stroke_ends(strokes: &[Stroke]) -> Vec<Pt> {
    let mut out = Vec::new();
    for st in strokes {
        match st {
            Stroke::Poly { pts, .. } => out.extend_from_slice(pts),
            Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. } => {
                if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                    out.push(*a);
                    out.push(*b);
                }
            }
            Stroke::Ellipse { .. } => {}
        }
    }
    out
}

/// 笔画的 k（Python `st.get("k", 1.0)`）：折线 / 弧有，别的 1.0。
fn stroke_k(st: &Stroke) -> f64 {
    match st {
        Stroke::Poly { k, .. } | Stroke::Arc { k, .. } => *k,
        _ => 1.0,
    }
}

/// 把在卷轴上画的笔画 st 放进形状 sh 并重新套框（custom.add_stroke）；
/// 端点落在别的笔画点上的精确吸附过去。返回新笔画的编号；框退化时 None。
pub fn add_stroke(sh: &mut Shape, st: &Stroke) -> Option<usize> {
    let to_uv = frame_to_uv(&sh.pts)?;
    let converted;
    let st = if matches!(st, Stroke::Ellipse { .. }) && !frame_upright(&sh.pts) {
        let Stroke::Ellipse { box_ } = st else {
            return None;
        };
        converted = Stroke::Curve {
            pts: ellipse_bezier(*box_),
            sharp: Vec::new(),
            sym: None,
        };
        &converted
    } else {
        st
    };
    let mut new = map_stroke(st, to_uv, 1.0, 1.0);
    if let Stroke::Poly { free: true, k, .. } = &mut new {
        *k = uv_k(&sh.pts, stroke_k(st));
    }
    let is_poly = matches!(new, Stroke::Poly { .. });
    let pts = match &mut new {
        Stroke::Poly { pts, .. } | Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. } => Some(pts),
        Stroke::Ellipse { .. } => None,
    };
    if let Some(pts) = pts {
        let near = stroke_ends(&sh.strokes);
        let idxs: Vec<usize> = if is_poly {
            (0..pts.len()).collect()
        } else if pts.is_empty() {
            Vec::new()
        } else {
            vec![0, pts.len() - 1]
        };
        for i in idxs {
            let p = pts[i];
            let mut best: Option<Pt> = None;
            let mut best_d = f64::INFINITY;
            for &q in &near {
                let d = dist(p, q);
                if d < best_d {
                    best_d = d;
                    best = Some(q);
                }
            }
            if let Some(q) = best
                && dist(p, q) < 1e-7
            {
                pts[i] = q;
            }
        }
        if is_poly && pts.len() >= 3 && dist(pts[0], pts[pts.len() - 1]) < 1e-7 {
            let first = pts[0];
            let last = pts.len() - 1;
            pts[last] = first;
        }
    }
    sh.strokes.push(new);
    refit(sh);
    Some(sh.strokes.len() - 1)
}

/// 空的自定义形状（现场画用）：1 拍 × 1 key 的框在 (0, 0)，画了东西再套框（custom.new_live_shape）。
pub fn new_live_shape(defaults: &Shape, custom_defaults: &CustomDefaults) -> Shape {
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = "Live drawing".to_string();
    sh.strokes = Vec::new();
    sh.fill = custom_defaults.fill;
    sh.gate = custom_defaults.gate;
    sh.align = custom_defaults.align;
    sh.pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    sh
}

// ---------------------------------------------------------------- 轮廓与填充 -> 音符

/// 自定义形状每条笔画上的音符，像线条一样（custom.outline_notes）。
pub fn outline_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    let mut raw: Vec<[i64; 3]> = Vec::new();
    for path in join_paths(&custom_strokes(sh)) {
        let closed = path_closed(&path);
        let path = dedupe(&path);
        if path.is_empty() {
            continue;
        }
        if path.len() < 2 {
            let t = floor_half(path[0][0] * ppq);
            raw.push([t, t + 1, pitch_of(path[0][1])]);
            continue;
        }
        let p = if closed {
            let mut closed_path = path;
            if closed_path[closed_path.len() - 1] != closed_path[0] {
                let first = closed_path[0];
                closed_path.push(first);
            }
            loop_from_left(&closed_path)
        } else {
            let mut path = path;
            if path[path.len() - 1][0] < path[0][0] {
                path.reverse();
            }
            stretch_ends(&path, false)
        };
        let scaled: Vec<Pt> = p.iter().map(|q| [q[0] * ppq, q[1]]).collect();
        raw.extend(line_notes(&scaled, false));
    }
    keep_longest(&raw)
}

/// 闭合多边形内部（even-odd，洞留空）碰到 pitch 行 q 的拍范围（custom.row_spans）。
pub fn row_spans(polys: &[Vec<Pt>], q: f64) -> Vec<[f64; 2]> {
    let (lo, hi) = (q - 0.5, q + 0.5);
    let mut edges: Vec<[Pt; 2]> = Vec::new();
    for poly in polys {
        for w in poly.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a[1] != b[1] && a[1].min(b[1]) < hi && a[1].max(b[1]) > lo {
                edges.push([a, b]);
            }
        }
    }
    if edges.is_empty() {
        return Vec::new();
    }
    let x_at =
        |e: &[Pt; 2], y: f64| e[0][0] + (e[1][0] - e[0][0]) * (y - e[0][1]) / (e[1][1] - e[0][1]);
    // 相邻两个水平之间没有角，每条边都是直的，每对交点就是一个梯形：时间范围取两边里宽的那个。
    let mut levels: Vec<f64> = vec![lo, hi];
    for e in &edges {
        for p in e {
            if lo < p[1] && p[1] < hi {
                levels.push(p[1]);
            }
        }
    }
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    levels.dedup();
    let mut spans: Vec<[f64; 2]> = Vec::new();
    for pair in levels.windows(2) {
        let (ya, yb) = (pair[0], pair[1]);
        let mid = (ya + yb) / 2.0;
        let mut cross: Vec<&[Pt; 2]> = edges
            .iter()
            .filter(|e| (e[0][1] <= mid) != (e[1][1] <= mid))
            .collect();
        cross.sort_by(|a, b| {
            x_at(a, mid)
                .partial_cmp(&x_at(b, mid))
                .unwrap_or(Ordering::Equal)
        });
        let mut k = 0;
        while k + 1 < cross.len() {
            let (left, right) = (cross[k], cross[k + 1]);
            spans.push([
                x_at(left, ya).min(x_at(left, yb)),
                x_at(right, ya).max(x_at(right, yb)),
            ]);
            k += 2;
        }
    }
    spans.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap_or(Ordering::Equal)
            .then(a[1].partial_cmp(&b[1]).unwrap_or(Ordering::Equal))
    });
    let mut merged: Vec<[f64; 2]> = Vec::new();
    for s in spans {
        if let Some(last) = merged.last_mut()
            && s[0] <= last[1]
        {
            last[1] = last[1].max(s[1]);
        } else {
            merged.push(s);
        }
    }
    merged
}

/// 形状内每个 key 的每段 `[pitch, start tick, end tick]`（custom.inside_spans 的顺序：
/// 按 pitch 递增，每个 pitch 内按 [`row_spans`] 的顺序）。一个缺口用直线补上。
pub fn inside_spans(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    let mut polys = custom_strokes(sh);
    if let Some(gap) = gap_line(sh) {
        polys.push(gap.to_vec());
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in &polys {
        for q in p {
            lo = lo.min(q[1]);
            hi = hi.max(q[1]);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Vec::new();
    }
    let first = 0.max(pitch_of(lo));
    let last = crate::paths::TOP_KEY.min(pitch_of(hi));
    let mut out = Vec::new();
    for q in first..=last {
        let qf = q as f64;
        let spans = match &sh.text {
            Some(tx) => threshold_spans(&polys, qf, tx.threshold),
            None => row_spans(&polys, qf),
        };
        for [a, b] in spans {
            let s = floor_half(a * ppq);
            out.push([q, s, floor_half(b * ppq).max(s + 1)]);
        }
    }
    out
}

/// spam 的门限，ticks（custom.spam_gate）。
pub fn spam_gate(sh: &Shape, ppq: f64) -> i64 {
    1.max(floor_half(sh.gate * ppq))
}

/// 一段从 s 到 e 的第一个音符起点与能放几个整门限（custom.spam_starts）。
pub fn spam_starts(sh: &Shape, s: i64, e: i64, g: i64) -> (i64, i64) {
    let s = if sh.align == Align::Aligned && g > 0 {
        div_ceil_pos(s, g) * g
    } else {
        s
    };
    let n = if g > 0 { (e - s).max(0) / g } else { 0 };
    (s, n)
}

/// stretches: (start, end, key) 行（ticks）→ 每段用门限 g 铺满的音符（custom.chop）。
/// 放不下一个整门限的丢掉；keep_short：短于一个门限的段保持原样。
pub fn chop(sh: &Shape, stretches: &[[i64; 3]], g: i64, keep_short: bool) -> Vec<[i64; 3]> {
    if g <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &[s0, e0, q] in stretches {
        let s = if sh.align == Align::Aligned {
            div_ceil_pos(s0, g) * g
        } else {
            s0
        };
        let n = (e0 - s).max(0) / g;
        if n == 0 {
            if keep_short {
                out.push([s0, e0, q]);
            }
            continue;
        }
        for k in 0..n {
            let start = s + k * g;
            out.push([start, start + g, q]);
        }
    }
    out
}

/// 轮廓的音符按 spam 门限切成背靠背的音符（custom.outline_spam）。
pub fn outline_spam(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    chop(sh, &outline_notes(sh, ppq), spam_gate(sh, ppq), true)
}

/// 形状会出多少个音符，不真的生成（custom.custom_note_count）；不确定时 None。
pub fn custom_note_count(sh: &Shape, ppq: f64) -> Option<i64> {
    if let Some(text) = &sh.notes {
        return unpack_notes(text).ok().map(|rows| rows.len() as i64);
    }
    if sh.fill == Fill::OutlineSpam {
        let g = spam_gate(sh, ppq);
        let total = outline_notes(sh, ppq)
            .iter()
            .map(|[s, e, _]| 1.max(spam_starts(sh, *s, *e, g).1))
            .sum();
        return Some(total);
    }
    if sh.fill == Fill::Empty || !fillable(&sh.strokes) {
        return None;
    }
    if sh.fill == Fill::Fill {
        return Some(inside_spans(sh, ppq).len() as i64);
    }
    let g = spam_gate(sh, ppq);
    let total = inside_spans(sh, ppq)
        .iter()
        .map(|[_, s, e]| spam_starts(sh, *s, *e, g).1)
        .sum();
    Some(total)
}

/// 形状的音符 (start, end, key)（custom.custom_notes）：
/// empty = 轮廓；fill = 每个 key 的每段一个；spam = 每段填满门限；outline_spam = 轮廓切断。
pub fn custom_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    if sh.notes.is_some() {
        return block_notes(sh, ppq)
            .map(|rows| rows.iter().map(|r| [r[0], r[1], r[2]]).collect())
            .unwrap_or_default();
    }
    if sh.fill == Fill::OutlineSpam {
        return outline_spam(sh, ppq);
    }
    if sh.fill == Fill::Empty || !fillable(&sh.strokes) {
        return outline_notes(sh, ppq);
    }
    let spans: Vec<[i64; 3]> = inside_spans(sh, ppq)
        .iter()
        .map(|&[q, s, e]| [s, e, q])
        .collect();
    if sh.fill == Fill::Fill {
        spans
    } else {
        chop(sh, &spans, spam_gate(sh, ppq), false)
    }
}

// ---------------------------------------------------------------- 粘贴的音符
// 自定义形状可以装从别的程序粘贴来的音符（而不是画）：sh["notes"] = pack_notes 的文本，
// strokes 只是框的轮廓。框里一个音符从 u = start / T 到 end / T、在 v = (row + 0.5) / K
// （T = 最后一个音符的结束 tick，K = 从最低到最高的 key 数），移动 / 拉伸 / 翻转 / 旋转框
// 都会带着音符走。sh["own_vel"]：音符保留自己的力度。

/// (start, end, row, velocity, track) 行 -> 存档文本（zlib + base64）（custom.pack_notes）。
pub fn pack_notes(rows: &[[i64; 5]]) -> String {
    let mut bytes = Vec::with_capacity(rows.len() * 20);
    for r in rows {
        for v in r {
            bytes.extend_from_slice(&(*v as i32).to_le_bytes());
        }
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(1));
    let _ = enc.write_all(&bytes);
    let compressed = enc.finish().unwrap_or_default();
    let mut out = String::from(TRACKS);
    out.push_str(&base64::engine::general_purpose::STANDARD.encode(&compressed));
    out
}

/// pack_notes 的文本 -> (start, end, row, velocity, track) 行（custom.unpack_notes）。
/// 旧版没有 track 列（也没有 `t:` 前缀）的数据补 0。
pub fn unpack_notes(text: &str) -> Result<Vec<[i64; 5]>, NotesError> {
    let tracks = text.starts_with(TRACKS);
    let body = if tracks { &text[TRACKS.len()..] } else { text };
    let raw = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| NotesError::Base64)?;
    let mut bytes = Vec::new();
    ZlibDecoder::new(&raw[..])
        .read_to_end(&mut bytes)
        .map_err(|_| NotesError::Zlib)?;
    if !bytes.len().is_multiple_of(4) {
        return Err(NotesError::Shape);
    }
    let cols = if tracks { 5 } else { 4 };
    let values: Vec<i64> = bytes
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as i64)
        .collect();
    if !values.len().is_multiple_of(cols) {
        return Err(NotesError::Shape);
    }
    let rows = values
        .chunks_exact(cols)
        .map(|c| {
            if tracks {
                [c[0], c[1], c[2], c[3], c[4]]
            } else {
                [c[0], c[1], c[2], c[3], 0]
            }
        })
        .collect();
    Ok(rows)
}

/// 文本是不是 Spiderweb 能用的 packed 音符（custom.check_notes）。
pub fn check_notes(text: &str) -> bool {
    let Ok(rows) = unpack_notes(text) else {
        return false;
    };
    !rows.is_empty()
        && rows.iter().all(|r| {
            r[0] >= 0 && r[1] > r[0] && r[2] >= 0 && (1..=127).contains(&r[3]) && r[4] >= 0
        })
}

/// (tick, gate, key, velocity, track) 行 -> 装它们的自定义形状（custom.notes_shape）：
/// 框从第一个音符的 tick / ppq 拍与最低的 key 开始；行数为 0 时 None。
pub fn notes_shape(notes: &[[i64; 5]], ppq: f64, name: &str) -> Option<Shape> {
    if notes.is_empty() {
        return None;
    }
    let t0 = notes.iter().map(|r| r[0]).min()?;
    let k0 = notes.iter().map(|r| r[2]).min()?;
    let rows: Vec<[i64; 5]> = notes
        .iter()
        .map(|r| [r[0] - t0, r[0] - t0 + r[1], r[2] - k0, r[3], r[4]])
        .collect();
    let max_end = rows.iter().map(|r| r[1]).max()?;
    let max_key = rows.iter().map(|r| r[2]).max()?;
    let b0 = t0 as f64 / ppq;
    let b1 = (t0 + max_end) as f64 / ppq;
    let mean = notes.iter().map(|r| r[3] as f64).sum::<f64>() / notes.len() as f64;
    let vel = round_i64(mean).clamp(1, 127);
    let sh = Shape {
        kind: Kind::Custom,
        name: name.to_string(),
        strokes: vec![BOX_STROKE.clone()],
        fill: Fill::Empty,
        notes: Some(pack_notes(&rows)),
        own_vel: true,
        vel0: vel as f64,
        vel1: vel as f64,
        pts: box_frame(b0, k0 as f64 - 0.5, b1, k0 as f64 + 0.5 + max_key as f64).to_vec(),
        ..Shape::default()
    };
    Some(sh)
}

/// 粘贴音符形状的音符 (start, end, pitch, velocity, track)，在它框现在的位置（custom.block_notes）。
pub fn block_notes(sh: &Shape, ppq: f64) -> Result<Vec<[i64; 5]>, NotesError> {
    let text = sh.notes.as_deref().ok_or(NotesError::Missing)?;
    let rows = unpack_notes(text)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let [a, b, c] = frame3(&sh.pts).ok_or(NotesError::Frame)?;
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    let t_all = rows.iter().map(|r| r[1]).max().unwrap_or(0) as f64;
    let k_all = rows.iter().map(|r| r[2]).max().unwrap_or(0) as f64 + 1.0;
    let n = rows.len();
    let mut rr: Vec<Pt> = Vec::with_capacity(2 * n);
    let mut first: Vec<usize> = Vec::with_capacity(n);
    for r in &rows {
        let v = (r[2] as f64 + 0.5) / k_all;
        let mut end_a = [
            (b0 + (r[0] as f64 / t_all) * ub + v * vb) * ppq,
            p0 + (r[0] as f64 / t_all) * up + v * vp,
        ];
        let mut end_b = [
            (b0 + (r[1] as f64 / t_all) * ub + v * vb) * ppq,
            p0 + (r[1] as f64 / t_all) * up + v * vp,
        ];
        if end_a[0] > end_b[0] || (end_a[0] == end_b[0] && end_a[1] > end_b[1]) {
            std::mem::swap(&mut end_a, &mut end_b);
        }
        first.push(rr.len());
        rr.push(end_a);
        rr.push(end_b);
    }
    let tails = vec![false; n];
    let (raw, per) = crate::paths::parts_notes(&rr, &first, &tails, true);
    let mut out = Vec::with_capacity(raw.len());
    let mut it = raw.iter();
    for (i, &count) in per.iter().enumerate() {
        for row in it.by_ref().take(count) {
            out.push([row[0], row[1], row[2], rows[i][3], rows[i][4]]);
        }
    }
    Ok(out)
}
