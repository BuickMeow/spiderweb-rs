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
//!   容器 repr）不复刻，坏值一律按原版跳过；
//! - `fill_plan` 不做 Python 的按框记忆（只影响速度，不影响结果）；
//! - 笔画还没有 `src`（convert.py 的 "Turn into live shape" 属于后续波次），所以
//!   [`stroke_groups`] 现在总是 None（所有笔画一组）。
//!

use std::cmp::Ordering;
use std::collections::BTreeMap;
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
use crate::shape::{Align, Ends, Fill, Kind, Shape, Stroke, Sym};
use crate::smooth::{clean_level, smooth_path};
use crate::text::{text_polys, threshold_spans};
use crate::{Pt, dist, floor_half, hypot2, round_half_even, round_i64};

/// 填充方式（custom.FILLS）。
pub const FILLS: [Fill; 4] = [Fill::Empty, Fill::Fill, Fill::Spam, Fill::OutlineSpam];
/// 用门限与 spam 起点的填充（custom.SPAM_FILLS）。
pub const SPAM_FILLS: [Fill; 2] = [Fill::Spam, Fill::OutlineSpam];
/// spam 起点对齐（custom.ALIGNS）。
pub const ALIGNS: [Align; 3] = [Align::Auto, Align::Aligned, Align::Centred];
/// spam 收尾（custom.ENDS），按面板下拉的顺序。
pub const ENDS: [Ends; 5] = [
    Ends::Round,
    Ends::Keep,
    Ends::Drop,
    Ends::Min,
    Ends::Stretch,
];
/// 只在打开时才存在的开关（custom.CUSTOM_FLAGS）。
pub const CUSTOM_FLAGS: [&str; 2] = ["union", "apart"];

/// 自定义形状的默认设置（custom.CUSTOM_DEFAULTS）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomDefaults {
    pub fill: Fill,
    pub gate: f64,
    pub align: Align,
    pub ends: Ends,
    pub union: bool,
    pub apart: bool,
}

impl Default for CustomDefaults {
    fn default() -> Self {
        Self {
            fill: Fill::Empty,
            gate: 0.0625,
            align: Align::Auto,
            ends: Ends::Round,
            union: false,
            apart: false,
        }
    }
}

impl CustomDefaults {
    /// 新自定义形状从默认设置里拿到的填充设置（custom.custom_settings）：
    /// ends 与 fill / gate / align 一样总是有，union / apart 只在打开时才有。
    pub fn apply(&self, sh: &mut Shape) {
        sh.fill = self.fill;
        sh.gate = self.gate;
        sh.align = self.align;
        sh.ends = self.ends;
        sh.union = self.union;
        sh.apart = self.apart;
    }
}

/// CUSTOM_DEFAULTS 的值（fill = empty，gate = 1/16 拍 = PPQ 960 的 60 ticks，align = auto，
/// ends = round：1.2.0 起新形状的默认；旧形状没有 ends，读作 drop）。
pub const CUSTOM_DEFAULTS: CustomDefaults = CustomDefaults {
    fill: Fill::Empty,
    gate: 0.0625,
    align: Align::Auto,
    ends: Ends::Round,
    union: false,
    apart: false,
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

/// Fill / Spam 能用（任何画都行：缺口用直线补上，见 [`fill_plan`]）（custom.fillable）。
pub fn fillable(strokes: &[Stroke]) -> bool {
    !strokes.is_empty()
}

// 轮廓的缺口，供 Fill / Spam 用（拍 / key，不是屏幕，缩放不会改变音符）：
/// 松着的端最多差这么远就算相接（直线连起来）。
pub const TOUCH_BEATS: f64 = 1.0 / 64.0;
/// 同上，key 方向。
pub const TOUCH_KEYS: f64 = 1.0;
/// 开放段离收尾直线最远不超过半个 key（上下）或 1/64 拍（左右）就没有值得填的里面。
pub const FLAT_KEYS: f64 = 0.5;
/// 同上，拍方向。
pub const FLAT_BEATS: f64 = 1.0 / 64.0;

/// 两个端算不算相接（custom.near_ends）。
pub fn near_ends(p: Pt, q: Pt) -> bool {
    (p[0] - q[0]).abs() <= TOUCH_BEATS + 1e-9 && (p[1] - q[1]).abs() <= TOUCH_KEYS + 1e-9
}

/// 开放路径离它两端的连线从未超过半个 key（上下）或 1/64 拍（左右）：没有值得填的里面
/// （一条直线、很缓的曲线）（custom.flat_path）。
pub fn flat_path(path: &[Pt]) -> bool {
    if path.is_empty() {
        return true;
    }
    let (b0, p0) = (path[0][0], path[0][1]);
    let (b1, p1) = (path[path.len() - 1][0], path[path.len() - 1][1]);
    let (db, dp) = (b1 - b0, p1 - p0);
    for (along, across, lim, d_along, d_across, s) in [
        (0usize, 1usize, FLAT_KEYS, db, dp, b0),
        (1usize, 0usize, FLAT_BEATS, dp, db, p0),
    ] {
        if d_along.abs() < 1e-12 {
            continue;
        }
        if path
            .iter()
            .any(|q| (q[along] - s) / d_along < -1e-9 || (q[along] - s) / d_along > 1.0 + 1e-9)
        {
            continue; // （越过了自己的两端）
        }
        let ok = path.iter().all(|q| {
            let t = (q[along] - s) / d_along;
            let line = path[0][across] + t * d_across;
            (q[across] - line).abs() <= lim + 1e-9
        });
        if ok {
            return true;
        }
    }
    db.abs() < 1e-12 && dp.abs() < 1e-12
}

/// Fill / Spam 眼里的自定义形状轮廓（拍 / pitch）（custom.fill_plan）：
/// `polys` = 组成里面的闭合环，`closers` = 补缺口加的直线（几乎相接的松端直接接上，
/// 剩下的每个开放段从终点直线连回起点），`flat` = 扁得没有里面的开放段（只保留轮廓音符）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FillPlan {
    pub polys: Vec<Vec<Pt>>,
    pub closers: Vec<[Pt; 2]>,
    pub flat: Vec<Vec<Pt>>,
}

/// 算出 [`FillPlan`]（custom.fill_plan）。Python 版按框记住结果，这里只影响速度，不缓存。
pub fn fill_plan(sh: &Shape) -> FillPlan {
    let paths = join_paths(&custom_strokes(sh));
    let mut polys: Vec<Vec<Pt>> = paths.iter().filter(|p| path_closed(p)).cloned().collect();
    let mut opens: Vec<Vec<Pt>> = paths.iter().filter(|p| !path_closed(p)).cloned().collect();
    let mut closers: Vec<[Pt; 2]> = Vec::new();
    loop {
        // 最接近的一对相接的松端，接上，直到没有
        let mut best: Option<(f64, usize, usize, u8, Pt, Pt)> = None;
        for i in 0..opens.len() {
            for j in i..opens.len() {
                let a = &opens[i];
                let b = &opens[j];
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                let pairs: Vec<(Pt, Pt, u8)> = if i == j {
                    if a.len() < 3 {
                        Vec::new()
                    } else {
                        vec![(a[a.len() - 1], a[0], 0)]
                    }
                } else {
                    vec![
                        (a[a.len() - 1], b[0], 1),
                        (a[a.len() - 1], b[b.len() - 1], 2),
                        (a[0], b[0], 3),
                        (a[0], b[b.len() - 1], 4),
                    ]
                };
                for (p, q, how) in pairs {
                    if near_ends(p, q) {
                        let d = ((p[0] - q[0]).abs() / TOUCH_BEATS)
                            .max((p[1] - q[1]).abs() / TOUCH_KEYS);
                        if best.as_ref().is_none_or(|b| d < b.0) {
                            best = Some((d, i, j, how, p, q));
                        }
                    }
                }
            }
        }
        let Some((_, i, j, how, p, q)) = best else {
            break;
        };
        closers.push([p, q]);
        if how == 0 {
            let mut a = opens[i].clone();
            let first = a[0];
            a.push(first);
            polys.push(a);
            opens.remove(i);
            continue;
        }
        let mut a = opens[i].clone();
        let mut b = opens[j].clone();
        if !(how == 1 || how == 2) {
            a.reverse();
        }
        if !(how == 1 || how == 3) {
            b.reverse();
        }
        a.extend_from_slice(&b);
        opens[i] = a;
        opens.remove(j);
    }
    let mut flat = Vec::new();
    for path in opens {
        if flat_path(&path) {
            flat.push(path);
        } else {
            let mut p = path.clone();
            let first = p[0];
            p.push(first);
            polys.push(p);
            closers.push([path[path.len() - 1], path[0]]);
        }
    }
    FillPlan {
        polys,
        closers,
        flat,
    }
}

/// 缺口补上的直线（虚线画）（custom.gap_lines）。
pub fn gap_lines(sh: &Shape) -> Vec<[Pt; 2]> {
    if sh.text.is_some() {
        Vec::new()
    } else {
        fill_plan(sh).closers
    }
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

/// [`uv_k`] 反着算：屏幕上每 key k 拍时框里一个 v 是几个 u（为了从框里取出笔画的 k）；
/// 没有就 1（custom.bp_k）。
pub fn bp_k(pts: &[Pt], k: f64) -> f64 {
    let Some([a, b, c]) = frame3(pts) else {
        return 1.0;
    };
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    // uv_k = hypot(vb / K, vp) / hypot(ub / K, up) = k，解出 x = 1 / K²
    let num = k * k * up * up - vp * vp;
    let den = vb * vb - k * k * ub * ub;
    let x = if den.abs() > 1e-18 { num / den } else { -1.0 };
    if x > 1e-18 { 1.0 / x.sqrt() } else { 1.0 }
}

/// 自定义形状 sh 的第 k 条笔画，换成拍 / pitch（像画在卷轴上的一条，供 [`add_stroke`] 用）：
/// 转过框里的椭圆变成曲线；弧 / 自由笔画的 k 换成每 key 几拍（custom.stroke_bp）。
pub fn stroke_bp(sh: &Shape, k: usize) -> Option<Stroke> {
    let mut st = sh.strokes.get(k)?.clone();
    let to_bp = frame_to_bp(&sh.pts)?;
    if matches!(st, Stroke::Ellipse { .. }) && !frame_upright(&sh.pts) {
        let Stroke::Ellipse { box_ } = &st else {
            return None;
        };
        st = Stroke::Curve {
            pts: ellipse_bezier(*box_),
            sharp: Vec::new(),
            sym: None,
        };
    }
    let has_k = matches!(st, Stroke::Poly { free: true, .. } | Stroke::Arc { .. });
    let k_in = stroke_k(&st);
    let mut new = map_stroke(&st, to_bp, 1.0, 1.0);
    if has_k {
        match &mut new {
            Stroke::Poly { k, .. } | Stroke::Arc { k, .. } => *k = bp_k(&sh.pts, k_in),
            _ => {}
        }
    }
    Some(new)
}

/// 把在卷轴上画的笔画 st 放进形状 sh 并重新套框（custom.add_stroke）；
/// 端点落在别的笔画点上的精确吸附过去。at：新笔画的编号（默认排在最后）。
/// 返回新笔画的编号；框退化时 None。
pub fn add_stroke(sh: &mut Shape, st: &Stroke, at: Option<usize>) -> Option<usize> {
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
    // 它的 k：屏幕上每 key 几拍 -> 框里一个 v 是几个 u（自由笔画与弧都有）
    let has_k = matches!(new, Stroke::Poly { free: true, .. } | Stroke::Arc { .. });
    let k_in = stroke_k(st);
    if has_k {
        match &mut new {
            Stroke::Poly { k, .. } | Stroke::Arc { k, .. } => *k = uv_k(&sh.pts, k_in),
            _ => {}
        }
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
    // Python 的 list.insert：越界的编号插在最后，但返回的还是传来的编号
    let at = at.unwrap_or(sh.strokes.len());
    sh.strokes.insert(at.min(sh.strokes.len()), new);
    refit(sh);
    Some(at)
}

/// 空的自定义形状（现场画用）：1 拍 × 1 key 的框在 (0, 0)，画了东西再套框（custom.new_live_shape）。
pub fn new_live_shape(defaults: &Shape, custom_defaults: &CustomDefaults) -> Shape {
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = "Live drawing".to_string();
    sh.strokes = Vec::new();
    custom_defaults.apply(&mut sh);
    sh.pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    sh
}

// ---------------------------------------------------------------- 轮廓与填充 -> 音符

/// 自定义形状每条笔画上的音符，像线条一样（custom.outline_notes）。
pub fn outline_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    paths_outline(&join_paths(&custom_strokes(sh)), ppq)
}

/// 同上，只要这些编号的笔画（custom.outline_notes 的 only；越界的编号跳过）。
pub fn outline_notes_only(sh: &Shape, ppq: f64, only: &[usize]) -> Vec<[i64; 3]> {
    let paths = custom_strokes(sh);
    let picked: Vec<Vec<Pt>> = only.iter().filter_map(|&k| paths.get(k).cloned()).collect();
    paths_outline(&join_paths(&picked), ppq)
}

/// 这些（接好的）路径上的音符，像线条一样（custom.paths_outline）。
pub fn paths_outline(paths: &[Vec<Pt>], ppq: f64) -> Vec<[i64; 3]> {
    let mut raw: Vec<[i64; 3]> = Vec::new();
    for path in paths {
        let closed = path_closed(path);
        let path = dedupe(path);
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

/// [`row_spans`]，但任意一个环的里面都算（重叠处也填，洞也填）（custom.union_spans）。
pub fn union_spans(polys: &[Vec<Pt>], q: f64) -> Vec<[f64; 2]> {
    let mut spans: Vec<[f64; 2]> = Vec::new();
    for poly in polys {
        spans.extend(row_spans(std::slice::from_ref(poly), q));
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
/// 按 pitch 递增，每个 pitch 内按 [`row_spans`] 的顺序）。文本：nonzero 规则与阈值（text.py）。
/// 轮廓的缺口用直线补上（[`fill_plan`]）。
pub fn inside_spans(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    let text_strokes = if sh.text.is_some() {
        Some(custom_strokes(sh))
    } else {
        None
    };
    let plan = if sh.text.is_none() {
        Some(fill_plan(sh))
    } else {
        None
    };
    let polys: &[Vec<Pt>] = match (&text_strokes, &plan) {
        (Some(v), _) => v,
        (_, Some(p)) => &p.polys,
        _ => &[],
    };
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in polys {
        for q in p {
            lo = lo.min(q[1]);
            hi = hi.max(q[1]);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Vec::new();
    }
    let union = sh.union && sh.text.is_none();
    let first = 0.max(pitch_of(lo));
    let last = crate::paths::TOP_KEY.min(pitch_of(hi));
    let mut out = Vec::new();
    for q in first..=last {
        let qf = q as f64;
        let spans = match &sh.text {
            Some(tx) => threshold_spans(polys, qf, tx.threshold),
            None if union => union_spans(polys, qf),
            None => row_spans(polys, qf),
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

/// stretches：(start, end, key) 行（ticks）→ 每段用门限 g 铺满背靠背的音符（custom.chop）。
/// 从哪儿开始看 ALIGNS，放不下整门限的零头看 ENDS。
fn chop_core(
    sh: &Shape,
    stretches: &[[i64; 3]],
    g: i64,
    want_notes: bool,
) -> (Vec<i64>, Vec<[i64; 3]>) {
    if g <= 0 {
        return (vec![0; stretches.len()], Vec::new());
    }
    let ends = sh.ends;
    let align = sh.align;
    let mut counts = Vec::with_capacity(stretches.len());
    let mut out = Vec::new();
    for &[s0, e0, q] in stretches {
        let size = e0 - s0;
        if matches!(ends, Ends::Round | Ends::Stretch) {
            let n = 1.max((2 * size + g).div_euclid(2 * g)); // 整门限数，四舍五入（半进）
            if ends == Ends::Stretch {
                counts.push(n);
                if want_notes {
                    for k in 0..n {
                        out.push([
                            s0 + (size * k).div_euclid(n),
                            s0 + (size * (k + 1)).div_euclid(n),
                            q,
                        ]);
                    }
                }
                continue;
            }
            let (n, first) = if align == Align::Aligned {
                // 门限网格里至少装了半个门限的格子
                let lo = -((g - 2 * s0).div_euclid(2 * g));
                let n = (2 * e0 + g).div_euclid(2 * g) - lo;
                if n <= 0 {
                    // （一个都没装：段中间所在的格子）
                    (1, (s0 + e0).div_euclid(2).div_euclid(g) * g)
                } else {
                    (n, lo * g)
                }
            } else if align == Align::Centred {
                (n, s0 + (size - n * g).div_euclid(2))
            } else {
                (n, s0)
            };
            counts.push(n);
            if want_notes {
                for k in 0..n {
                    let start = first + k * g;
                    out.push([start, start + g, q]);
                }
            }
            continue;
        }
        let s = match align {
            Align::Aligned => div_ceil_pos(s0, g) * g, // s0 起第一条门限线
            Align::Centred => s0 + size.rem_euclid(g).div_euclid(2),
            Align::Auto => s0,
        };
        let n = 0.max((e0 - s).div_euclid(g));
        if ends == Ends::Keep {
            // 第一个整门限前 / 最后一个后的零头留成短线（aligned / centred）
            let head_end = s.min(e0);
            let head = i64::from(head_end > s0);
            let tail = i64::from(if s < e0 { s + n * g } else { e0 } < e0);
            let c = n + head + tail;
            counts.push(c);
            if want_notes {
                for k in 0..c {
                    let first = head == 1 && k == 0;
                    let last = tail == 1 && k == c - 1;
                    let mut start = s + (k - head) * g;
                    let mut stop = start + g;
                    if first {
                        start = s0;
                        stop = head_end;
                    } else if last {
                        stop = e0;
                    }
                    out.push([start, stop, q]);
                }
            }
            continue;
        }
        let short = n == 0; // 短于一个门限：原样留一个音符（"min"：至少四分之一门限）
        let n = if short { 1 } else { n };
        counts.push(n);
        if want_notes {
            if short {
                let (mut a, mut b) = (s0, e0);
                if ends == Ends::Min {
                    let least = 1.max(div_ceil_pos(g, 4));
                    if b - a < least {
                        a += (b - a - least).div_euclid(2);
                        b = a + least;
                    }
                }
                out.push([a, b, q]);
            } else {
                for k in 0..n {
                    let start = s + k * g;
                    out.push([start, start + g, q]);
                }
            }
        }
    }
    (counts, out)
}

/// stretches：(start, end, key) 行（ticks）→ 每段用门限 g 铺满的音符（custom.chop）。
pub fn chop(sh: &Shape, stretches: &[[i64; 3]], g: i64) -> Vec<[i64; 3]> {
    chop_core(sh, stretches, g, true).1
}

/// chop 会把 stretches 切成多少个音符（custom.chop_count）。
pub fn chop_count(sh: &Shape, stretches: &[[i64; 3]], g: i64) -> i64 {
    if stretches.is_empty() {
        return 0;
    }
    chop_core(sh, stretches, g, false).0.iter().sum()
}

/// 形状笔画的分组 `{组: [笔画编号]}`，来自不同形状的笔画（convert.py：每个形状的笔画各自出
/// 自己的音符）（custom.stroke_groups）；全是一组时 None。
///
/// 我们的 [`Stroke`] 还没有 `src`（"Turn into live shape" 属于后续波次），所以现在总是 None；
/// 分组的消费逻辑（[`outline_groups`] / [`custom_notes_groups`]）已经就位。
pub fn stroke_groups(sh: &Shape) -> Option<Vec<Vec<usize>>> {
    let _ = sh;
    None
}

/// 轮廓的音符（spam：像 Outline spam 那样切断）和每个音符属于哪个笔画组（笔画全一组时 None，
/// 见 [`stroke_groups`]）（custom.outline_groups）。
pub fn outline_groups(sh: &Shape, ppq: f64, spam: bool) -> (Vec<[i64; 3]>, Option<Vec<i64>>) {
    match stroke_groups(sh) {
        None => {
            let notes = outline_notes(sh, ppq);
            let notes = if spam {
                chop_outline(sh, &notes, ppq)
            } else {
                notes
            };
            (notes, None)
        }
        Some(groups) => {
            let mut parts: Vec<[i64; 3]> = Vec::new();
            let mut ids: Vec<i64> = Vec::new();
            for (n, strokes) in groups.iter().enumerate() {
                let mut notes = outline_notes_only(sh, ppq, strokes);
                if spam {
                    notes = chop_outline(sh, &notes, ppq);
                }
                ids.extend(std::iter::repeat_n(n as i64, notes.len()));
                parts.extend(notes);
            }
            (parts, Some(ids))
        }
    }
}

/// 轮廓的音符切成 spam 门限的背靠背音符（spam 起点与收尾同 Spam；短于一个门限的音符不会
/// 消失，陡的轮廓段还在）（custom.chop_outline）。
pub fn chop_outline(sh: &Shape, notes: &[[i64; 3]], ppq: f64) -> Vec<[i64; 3]> {
    chop(sh, notes, spam_gate(sh, ppq))
}

/// 轮廓按 spam 门限出的音符（custom.outline_spam）。
pub fn outline_spam(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    outline_groups(sh, ppq, true).0
}

/// 填充的形状里扁得没法填的开放段自己的轮廓音符（fill_plan 的 flat），让它们不消失
/// （custom.flat_notes）。
pub fn flat_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    let flat = if sh.text.is_some() {
        Vec::new()
    } else {
        fill_plan(sh).flat
    };
    if flat.is_empty() {
        Vec::new()
    } else {
        paths_outline(&flat, ppq)
    }
}

/// 形状会出多少个音符，不真的生成（spam 可能上百万）（custom.custom_note_count）；不确定时 None。
pub fn custom_note_count(sh: &Shape, ppq: f64) -> Option<i64> {
    if let Some(text) = &sh.notes {
        return unpack_notes(text).ok().map(|rows| rows.len() as i64);
    }
    if sh.apart && matches!(sh.fill, Fill::Fill | Fill::Spam) {
        return None; // （生成出来再数）
    }
    if sh.fill == Fill::OutlineSpam {
        return Some(chop_count(
            sh,
            &outline_groups(sh, ppq, false).0,
            spam_gate(sh, ppq),
        ));
    }
    if sh.fill == Fill::Empty || !fillable(&sh.strokes) {
        return None;
    }
    let flat = flat_notes(sh, ppq);
    if sh.fill == Fill::Fill {
        return Some(inside_spans(sh, ppq).len() as i64 + flat.len() as i64);
    }
    let g = spam_gate(sh, ppq);
    let spans: Vec<[i64; 3]> = inside_spans(sh, ppq)
        .iter()
        .map(|&[q, s, e]| [s, e, q])
        .collect();
    Some(chop_count(sh, &spans, g) + chop_count(sh, &flat, g))
}

/// 形状的音符 (start, end, key)（custom.custom_notes）：
/// empty = 轮廓；fill = 每个 key 的每段一个；spam = 每段填满门限；outline_spam = 轮廓切断。
pub fn custom_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    custom_notes_groups(sh, ppq).0
}

/// custom_notes，以及每个音符属于哪组（None = 全一组：见 [`outline_groups`]）。
pub fn custom_notes_groups(sh: &Shape, ppq: f64) -> (Vec<[i64; 3]>, Option<Vec<i64>>) {
    if sh.notes.is_some() {
        let rows = block_notes(sh, ppq)
            .map(|rows| rows.iter().map(|r| [r[0], r[1], r[2]]).collect())
            .unwrap_or_default();
        return (rows, None);
    }
    if sh.fill == Fill::OutlineSpam {
        return outline_groups(sh, ppq, true);
    }
    if sh.fill == Fill::Empty || !fillable(&sh.strokes) {
        return outline_groups(sh, ppq, false);
    }
    let spans: Vec<[i64; 3]> = inside_spans(sh, ppq)
        .iter()
        .map(|&[q, s, e]| [s, e, q])
        .collect(); // (start, end, key)
    let flat = flat_notes(sh, ppq); // （Spam 里像 Outline spam）
    if sh.fill == Fill::Fill {
        let mut notes = spans;
        notes.extend(flat);
        if sh.apart {
            // 边缘的音符，和它们之间里面的长音符
            let outline = edge_parts(&notes);
            let inside = cut_out(&notes, &outline);
            let mut ids = vec![0i64; outline.len()];
            ids.extend(std::iter::repeat_n(1i64, inside.len()));
            let mut all = outline;
            all.extend(inside);
            return (all, Some(ids));
        }
        return (notes, None);
    }
    let mut notes = chop(sh, &spans, spam_gate(sh, ppq));
    notes.extend(chop_outline(sh, &flat, ppq));
    if sh.apart {
        // 同样的 spam；填出来的边缘上的音符算轮廓的
        let ids = on_edge(&notes)
            .into_iter()
            .map(|edge| if edge { 0 } else { 1 })
            .collect();
        return (notes, Some(ids));
    }
    (notes, None)
}

/// Fill / Spam 带 "Outline"：轮廓和里面要各走各的通道（custom.outline_apart）。
pub fn outline_apart(sh: &Shape) -> bool {
    sh.kind == Kind::Custom
        && sh.apart
        && matches!(sh.fill, Fill::Fill | Fill::Spam)
        && sh.notes.is_none()
}

/// (start, end, key) 音符 → 每个 key 覆盖的段：(key, start, end) 数组，按 key 与 start 排序、
/// 互不重叠（custom.merged_by_key）。
pub fn merged_by_key(notes: &[[i64; 3]]) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
    if notes.is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let mut a: Vec<[i64; 3]> = notes.to_vec();
    a.sort_by(|x, y| x[2].cmp(&y[2]).then(x[0].cmp(&y[0])));
    let mut keys: Vec<i64> = Vec::new();
    let mut starts: Vec<i64> = Vec::new();
    let mut ends: Vec<i64> = Vec::new();
    let mut run = 0i64;
    for (i, r) in a.iter().enumerate() {
        let new = i == 0 || r[2] != a[i - 1][2] || r[0] > run;
        if new {
            keys.push(r[2]);
            starts.push(r[0]);
            ends.push(r[1]);
            run = r[1];
        } else {
            run = run.max(r[1]);
            if let Some(last) = ends.last_mut() {
                *last = (*last).max(r[1]);
            }
        }
    }
    (keys, starts, ends)
}

/// 哪些音符整个在 others 覆盖的同一 key 范围里（custom.covered）。
pub fn covered(notes: &[[i64; 3]], others: &[[i64; 3]]) -> Vec<bool> {
    let (ks, ss, es) = merged_by_key(others);
    if ks.is_empty() || notes.is_empty() {
        return vec![false; notes.len()];
    }
    let big = 1i64 << 40;
    notes
        .iter()
        .map(|n| {
            let target = n[2].wrapping_mul(big).wrapping_add(n[0]);
            // searchsorted(..., "right")：起点在它之前（或同时）的最后一段
            let (mut lo, mut hi) = (0usize, ks.len());
            while lo < hi {
                let mid = (lo + hi) / 2;
                if ks[mid].wrapping_mul(big).wrapping_add(ss[mid]) <= target {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if lo == 0 {
                return false;
            }
            let i = lo - 1;
            ks[i] == n[2] && es[i] >= n[1]
        })
        .collect()
}

/// 音符的 key 平移 keys（custom.shifted）。
pub fn shifted(notes: &[[i64; 3]], keys: i64) -> Vec<[i64; 3]> {
    notes.iter().map(|n| [n[0], n[1], n[2] + keys]).collect()
}

/// Fill / Spam "Outline"：哪些 (start, end, key) 音符在它们填的区域的边缘上：不是整个被上面
/// 或下面 key 的音符盖住，或者在自己 key 上第一个 / 最后一个。所以只有填出来的区域自己的
/// 边缘算数：它里面的轮廓（重叠填上的）被排除；重叠抵消的地方每个填出来的小块的每条边都是
/// 轮廓，不管往哪斜（custom.on_edge）。
pub fn on_edge(notes: &[[i64; 3]]) -> Vec<bool> {
    let up = covered(notes, &shifted(notes, -1));
    let down = covered(notes, &shifted(notes, 1));
    let before: Vec<[i64; 3]> = notes.iter().map(|n| [n[0] - 1, n[0], n[2]]).collect();
    let after: Vec<[i64; 3]> = notes.iter().map(|n| [n[1], n[1] + 1, n[2]]).collect();
    let b = covered(&before, notes);
    let a = covered(&after, notes);
    (0..notes.len())
        .map(|i| !(up[i] && down[i] && b[i] && a[i]))
        .collect()
}

/// Fill "Outline"：填出来的 (start, end, key) 长音符在区域边缘上的部分：上面或下面 key 没盖
/// 住的时间，以及每段的第一个和最后一个 tick（像线条的竖段），作为 (start, end, key) 音符
/// （custom.edge_parts）。
pub fn edge_parts(notes: &[[i64; 3]]) -> Vec<[i64; 3]> {
    let (ks, ss, es) = merged_by_key(notes);
    let mut ends: Vec<[i64; 3]> = Vec::with_capacity(2 * ks.len());
    for i in 0..ks.len() {
        ends.push([ss[i], ss[i] + 1, ks[i]]);
    }
    for i in 0..ks.len() {
        ends.push([es[i] - 1, es[i], ks[i]]);
    }
    let mut parts = cut_out(notes, &shifted(notes, -1));
    parts.extend(cut_out(notes, &shifted(notes, 1)));
    parts.extend(ends);
    let (ks, ss, es) = merged_by_key(&parts);
    (0..ks.len()).map(|i| [ss[i], es[i], ks[i]]).collect()
}

/// (start, end, key) 段去掉 others 在同一 key 上盖住的时间（custom.cut_out）。
pub fn cut_out(spans: &[[i64; 3]], others: &[[i64; 3]]) -> Vec<[i64; 3]> {
    let (ks, ss, es) = merged_by_key(others);
    let mut by_key: BTreeMap<i64, Vec<(i64, i64)>> = BTreeMap::new();
    for i in 0..ks.len() {
        by_key.entry(ks[i]).or_default().push((ss[i], es[i]));
    }
    let mut out: Vec<[i64; 3]> = Vec::new();
    for &[s0, e0, k] in spans {
        let mut s = s0;
        let e = e0;
        if let Some(ranges) = by_key.get(&k) {
            for &(os, oe) in ranges {
                if oe <= s || os >= e {
                    continue;
                }
                if os > s {
                    out.push([s, os, k]);
                }
                s = s.max(oe);
                if s >= e {
                    break;
                }
            }
        }
        if s < e {
            out.push([s, e, k]);
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(pts: &[[f64; 2]]) -> Stroke {
        Stroke::Poly {
            pts: pts.to_vec(),
            free: false,
            smooth: 0,
            k: 1.0,
        }
    }

    fn square() -> Stroke {
        poly(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]])
    }

    fn shape(fill: Fill, align: Align, ends: Ends, union: bool, strokes: Vec<Stroke>) -> Shape {
        let mut sh = Shape {
            kind: Kind::Custom,
            strokes,
            fill,
            align,
            ends,
            union,
            pts: vec![[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]],
            ..Shape::default()
        };
        sh.gate = 0.0625;
        sh
    }

    /// "centred"：放不下整门限的零头两端各分一半。
    #[test]
    fn centred_shares_the_leftover() {
        let sh = shape(
            Fill::Spam,
            Align::Centred,
            Ends::Round,
            false,
            vec![square()],
        );
        let got = chop(&sh, &[[0, 100, 60]], 60);
        assert_eq!(got, vec![[-10, 50, 60], [50, 110, 60]]);
        let auto = shape(Fill::Spam, Align::Auto, Ends::Round, false, vec![square()]);
        assert_eq!(
            chop(&auto, &[[0, 100, 60]], 60),
            vec![[0, 60, 60], [60, 120, 60]]
        );
    }

    /// ENDS 的五种收尾：round 至少一个、stretch 正好铺满、keep 留零头、drop / min 短段。
    #[test]
    fn ends_round_and_stretch() {
        let round = shape(Fill::Spam, Align::Auto, Ends::Round, false, vec![square()]);
        assert_eq!(
            chop(&round, &[[0, 30, 60]], 60),
            vec![[0, 60, 60]],
            "round：半个门限算一个"
        );
        let stretch = shape(
            Fill::Spam,
            Align::Aligned,
            Ends::Stretch,
            false,
            vec![square()],
        );
        assert_eq!(
            chop(&stretch, &[[5, 200, 60]], 60),
            vec![[5, 70, 60], [70, 135, 60], [135, 200, 60]]
        );
    }

    #[test]
    fn ends_keep_and_min() {
        let keep = shape(
            Fill::Spam,
            Align::Aligned,
            Ends::Keep,
            false,
            vec![square()],
        );
        assert_eq!(
            chop(&keep, &[[5, 200, 60], [0, 3, 61]], 60),
            vec![
                [5, 60, 60],
                [60, 120, 60],
                [120, 180, 60],
                [180, 200, 60],
                [0, 3, 61]
            ]
        );
        let min = shape(Fill::Spam, Align::Aligned, Ends::Min, false, vec![square()]);
        assert_eq!(
            chop(&min, &[[0, 3, 61]], 60),
            vec![[-6, 9, 61]],
            "min：长到四分之一门限，居中"
        );
        let drop = shape(
            Fill::Spam,
            Align::Aligned,
            Ends::Drop,
            false,
            vec![square()],
        );
        assert_eq!(
            chop(&drop, &[[0, 3, 61]], 60),
            vec![[0, 3, 61]],
            "drop：短于一个门限的段原样留着"
        );
    }

    /// union：轮廓重叠处也填上（不开时重叠互相抵消，洞留空）。
    #[test]
    fn union_fills_where_outlines_overlap() {
        let b = poly(&[[0.5, 0.0], [1.5, 0.0], [1.5, 1.0], [0.5, 1.0], [0.5, 0.0]]);
        let even = shape(
            Fill::Fill,
            Align::Auto,
            Ends::Drop,
            false,
            vec![square(), b.clone()],
        );
        let got = inside_spans(&even, 960.0);
        assert_eq!(got.len(), 6, "重叠抵消：中间留空");
        assert_eq!(got[0], [60, 0, 960]);
        assert_eq!(got[1], [60, 1920, 2880]);
        let union = shape(Fill::Fill, Align::Auto, Ends::Drop, true, vec![square(), b]);
        assert_eq!(
            inside_spans(&union, 960.0),
            vec![[60, 0, 2880], [61, 0, 2880], [62, 0, 2880]]
        );
        assert_eq!(custom_note_count(&union, 960.0), Some(3));
    }

    /// 几乎相接的松端直接接上（补线也算一条虚线），剩下的开放段从终点直线连回起点。
    #[test]
    fn fill_plan_joins_ends_that_nearly_touch() {
        let near = vec![
            poly(&[[0.0, 0.0], [0.5, 0.5]]),
            poly(&[[0.5 + 1e-4, 0.5], [1.0, 1.0]]),
        ];
        let sh = shape(Fill::Fill, Align::Auto, Ends::Drop, false, near);
        let plan = fill_plan(&sh);
        assert!(plan.polys.is_empty(), "接上后是扁的");
        assert_eq!(plan.flat.len(), 1);
        assert_eq!(plan.closers.len(), 1);
        assert!(near_ends(plan.closers[0][0], plan.closers[0][1]));
        // 两条对角线（单位框）：差一个 key 的松端也算相接，接完还剩两个缺口
        let two = vec![
            poly(&[[0.0, 0.0], [1.0, 1.0]]),
            poly(&[[0.0, 1.0], [1.0, 0.0]]),
        ];
        let mut sh = shape(Fill::Fill, Align::Auto, Ends::Drop, false, two);
        sh.pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let plan = fill_plan(&sh);
        assert_eq!(plan.closers.len(), 2, "两个缺口两条虚线");
        assert_eq!(plan.polys.len(), 1);
        assert!(plan.flat.is_empty());
    }

    /// 没有 src 时笔画全是一组（convert 的 groups 属于后续波次）。
    #[test]
    fn groups_are_one_without_src() {
        let sh = shape(Fill::Empty, Align::Auto, Ends::Drop, false, vec![square()]);
        assert!(stroke_groups(&sh).is_none());
        let (notes, ids) = outline_groups(&sh, 960.0, false);
        assert!(!notes.is_empty());
        assert!(ids.is_none());
        assert!(!outline_apart(&shape(
            Fill::Fill,
            Align::Auto,
            Ends::Drop,
            false,
            vec![square()]
        )));
        let mut apart = shape(Fill::Fill, Align::Auto, Ends::Drop, false, vec![square()]);
        apart.apart = true;
        assert!(outline_apart(&apart));
    }
}
