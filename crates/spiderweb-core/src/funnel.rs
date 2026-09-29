//! 漏斗：通向一堵墙的线，沿曲线张开，用 spam 或 long 音符填充（Python notes/funnel.py 的逐函数移植）。
//!
//! `sh.pts = [线起点, 线终点, 墙端1, 墙端2, (线2起点, 线2终点, ...)]`：直线，怎么画都行
//! （画的时候只有线）。额外的线通向同一堵墙。音符网格从第一条线的起点朝墙走；墙在时间上
//! 靠前时是反向漏斗。
//!
//! `sh.starts = [{line, at, ends: [通向墙端1的曲线或 None, ...墙端2]}]`：每个起点向每个墙端
//! 张开一条曲线（半漏斗只有一个墙端；墙端落在线上的没有曲线）。
//!
//! 曲线在自己的斜方盒里：从起点 S 出发，U 沿线、V 沿墙，S + U + V = 墙端，S + U 是线与墙的
//! 交点。盒里的点 `[u, f]` 就是 `S + u*U + f*V`（u = 0 在起点、1 在墙，f = 张开多少），所以
//! 移动、拉伸、翻转、转动两条线时曲线跟着走。曲线是从 `[0, 0]`（起点）到 `[1, 1]`（墙端）的
//! Bézier（见 [`crate::bezier`]：锚点 + 手柄），`sharp` 是手柄分开的尖角锚点，`link` 是组号，
//! `flip` 表示相对同组其他曲线首尾对调。
//!
//! 与原版的差异（都用 Option / Result 代替 Python 异常）：
//! - 清洗函数遇到坏数据返回 None（Python 抛异常）；`clean_curve` 的 None 同样表示"没有可用曲线"；
//! - `start_point` / `curve_box` 在线号超出 `pts` 时返回 None（Python IndexError）；
//! - `formula_curve` / `preset_curve` 用 `Err(String)` 报公式算不出来的错误；
//! - Python 返回闭包的地方用结构体：[`Openness`]（w(d)）与 [`SmoothCurve`]（eval(u)）。
//!
//! [`Spans`] 是 key → 区间列表的有序表，保持 Python dict 的插入顺序（`funnel_cells` 的输出
//! 顺序跟着它走）。

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use serde_json::{Value, json};

use crate::bezier::{anchor_count, fit, handle_anchor, sample};
use crate::custom::row_spans;
use crate::paths::{EDGE, pitch_of};
use crate::shape::{FunnelCurve, FunnelFill, FunnelStart, GateChange, GateFollow, Shape, WallMode};
use crate::{Pt, floor_half, hypot2, round_half_even};

// ---------------------------------------------------------------- 设置与常量

/// 填充方式（funnel.FUNNEL_FILLS）。
pub const FUNNEL_FILLS: [FunnelFill; 2] = [FunnelFill::Spam, FunnelFill::Long];
/// 门限变化方式（funnel.GATE_CHANGES）。
pub const GATE_CHANGES: [GateChange; 2] = [GateChange::Steps, GateChange::Smooth];
/// 门限跟随对象（funnel.GATE_FOLLOWS）。
pub const GATE_FOLLOWS: [GateFollow; 2] = [GateFollow::Time, GateFollow::Curve];
/// 墙模式（funnel.WALL_MODES）。
pub const WALL_MODES: [WallMode; 2] = [WallMode::In, WallMode::Past];

/// 漏斗设置（clean_funnel 的产物，对应 Python 返回的 dict）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FunnelSettings {
    pub fill: FunnelFill,
    pub gate0: f64,
    pub gate1: f64,
    pub vary: bool,
    pub change: GateChange,
    pub follow: GateFollow,
    pub wall: WallMode,
}

/// 漏斗默认设置（funnel.FUNNEL_DEFAULTS）。
pub const FUNNEL_DEFAULTS: FunnelSettings = FunnelSettings {
    fill: FunnelFill::Spam,
    gate0: 0.0625,
    gate1: 0.0625,
    vary: false,
    change: GateChange::Steps,
    follow: GateFollow::Time,
    wall: WallMode::In,
};

/// 第一版的默认曲线（funnel.FUNNEL_BEND）。
pub const FUNNEL_BEND: Pt = [0.75, 0.2];
/// 旧版 bends 的最多条数（funnel.MAX_BENDS）。
pub const MAX_BENDS: usize = 32;

/// 默认曲线：接近第一版的默认（慢起、靠近墙快速张开），只有首尾两个锚点与手柄。
pub const DEFAULT_CURVE: [Pt; 4] = [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]];

/// 预设曲线名字与公式字符串（funnel.CURVE_PRESETS；公式字符串给界面用，None = 默认曲线）。
pub const CURVE_PRESETS: &[(&str, Option<&str>)] = &[
    ("Default", None),
    ("Straight", Some("x")),
    ("Slow start (x²)", Some("x^2")),
    ("Slower start (x³)", Some("x^3")),
    ("Very slow start (x⁵)", Some("x^5")),
    ("Fast start (x² flipped)", Some("1-(1-x)^2")),
    ("Faster start (x³ flipped)", Some("1-(1-x)^3")),
    ("S-curve (slow, fast, slow)", Some("x*x*(3-2*x)")),
    ("Steep S-curve", Some("x^3*(x*(6*x-15)+10)")),
    (
        "Reverse S (fast, slow, fast)",
        Some("0.5-sin(asin(1-2*x)/3)"),
    ),
    ("Quarter circle (slow start)", Some("1-sqrt(1-x^2)")),
    ("Quarter circle (fast start)", Some("sqrt(1-(1-x)^2)")),
    ("Exponential", Some("exp(5*x)")),
    ("Logarithmic", Some("ln(1+20*x)")),
];

/// 锚点 + 手柄离公式多近（曲线尺寸的一部分，funnel.FIT_TOLERANCE）。
pub const FIT_TOLERANCE: f64 = 0.003;

impl FunnelFill {
    /// 设置名的字符串形式（JSON / 界面用）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Long => "long",
        }
    }

    /// 从设置名解析；未知名字返回 None。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "spam" => Some(Self::Spam),
            "long" => Some(Self::Long),
            _ => None,
        }
    }
}

impl GateChange {
    /// 设置名的字符串形式（JSON / 界面用）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Smooth => "smooth",
        }
    }

    /// 从设置名解析；未知名字返回 None。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "steps" => Some(Self::Steps),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }
}

impl GateFollow {
    /// 设置名的字符串形式（JSON / 界面用）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Curve => "curve",
        }
    }

    /// 从设置名解析；未知名字返回 None。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "time" => Some(Self::Time),
            "curve" => Some(Self::Curve),
            _ => None,
        }
    }
}

impl WallMode {
    /// 设置名的字符串形式（JSON / 界面用）。
    pub fn name(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Past => "past",
        }
    }

    /// 从设置名解析；未知名字返回 None。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "in" => Some(Self::In),
            "past" => Some(Self::Past),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- JSON 小工具（同 custom.rs）

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

/// 一个点对子：长度为 2 的数组或字符串（Python 的可解包序列）。
fn pair_of(v: &Value) -> Option<Pt> {
    match v {
        Value::Array(a) if a.len() == 2 => Some([py_float(&a[0])?, py_float(&a[1])?]),
        Value::String(s) if s.chars().count() == 2 => {
            let mut it = s.chars();
            let u = py_float(&Value::String(it.next()?.to_string()))?;
            let f = py_float(&Value::String(it.next()?.to_string()))?;
            Some([u, f])
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- 设置清理

/// 漏斗设置从文件里读出的清洗（funnel.clean_funnel）：坏值返回 None（Python 抛异常）。
pub fn clean_funnel(sh: &Value) -> Option<FunnelSettings> {
    let obj = sh.as_object()?;
    let mut out = FUNNEL_DEFAULTS;
    out.fill = match obj.get("fill").and_then(Value::as_str) {
        Some("spam") => FunnelFill::Spam,
        Some("long") => FunnelFill::Long,
        _ => FUNNEL_DEFAULTS.fill,
    };
    out.change = match obj.get("change").and_then(Value::as_str) {
        Some("steps") => GateChange::Steps,
        Some("smooth") => GateChange::Smooth,
        _ => FUNNEL_DEFAULTS.change,
    };
    out.follow = match obj.get("follow").and_then(Value::as_str) {
        Some("time") => GateFollow::Time,
        Some("curve") => GateFollow::Curve,
        _ => FUNNEL_DEFAULTS.follow,
    };
    out.wall = match obj.get("wall").and_then(Value::as_str) {
        Some("in") => WallMode::In,
        Some("past") => WallMode::Past,
        _ => FUNNEL_DEFAULTS.wall,
    };
    // sh.get(key, 默认值)：键在但值是 null 也要报错（float(None)）
    let gate = |key: &str, default: f64| -> Option<f64> {
        match obj.get(key) {
            None => Some(default),
            Some(v) => py_float(v),
        }
    };
    out.gate0 = 1e-6_f64.max(gate("gate0", FUNNEL_DEFAULTS.gate0)?);
    out.gate1 = 1e-6_f64.max(gate("gate1", FUNNEL_DEFAULTS.gate1)?);
    // 旧文件没有 "vary"：两个不同的 gate 当时就是想变化
    out.vary = match obj.get("vary") {
        Some(v) => py_bool(v),
        None => (out.gate0 - out.gate1).abs() > 1e-9,
    };
    if !out.vary {
        out.gate1 = out.gate0;
    }
    Some(out)
}

/// 起点列表从文件里读出的清洗（funnel.clean_starts）：坏数据返回 None（Python 抛异常）。
pub fn clean_starts(starts: &Value, lines: usize) -> Option<Vec<FunnelStart>> {
    if !py_bool(starts) {
        return Some(Vec::new()); // starts or []
    }
    let Value::Array(items) = starts else {
        return None;
    };
    let mut out: Vec<FunnelStart> = Vec::new();
    let mut old_twins: Vec<usize> = Vec::new();
    for st in items {
        let obj = st.as_object()?; // Python AttributeError
        let mut raw: [Option<&Value>; 2] = [None, None];
        match obj.get("ends") {
            None => {}
            Some(v) if !py_bool(v) => {} // 空表 / 空串 / {}：list(...) 是空的
            Some(Value::Array(a)) => {
                for (i, v) in a.iter().take(2).enumerate() {
                    raw[i] = Some(v);
                }
            }
            Some(_) => return None,
        }
        let mut ends: [Option<FunnelCurve>; 2] = [None, None];
        for (i, r) in raw.iter().enumerate() {
            ends[i] = match r {
                None => None,
                Some(v) => clean_curve_strict(v).ok()?,
            };
        }
        let line = match obj.get("line") {
            None => 0, // st.get("line", 0)
            Some(v) => py_int(v)?,
        };
        let any_end = ends.iter().any(Option::is_some);
        if any_end && 0 <= line && line < lines as i64 {
            let at = match obj.get("at") {
                None => 0.0, // st.get("at", 0)
                Some(v) => py_float(v)?,
            };
            let at = at.clamp(0.0, 1.0);
            let both = ends.iter().all(Option::is_some);
            let had_list = raw.iter().any(|r| matches!(r, Some(Value::Array(_))));
            out.push(FunnelStart {
                line: line as usize,
                at,
                ends,
            });
            if both && had_list {
                old_twins.push(out.len() - 1); // 旧版一个起点的两条曲线总是保持一样
            }
        }
    }
    for idx in old_twins {
        let link = next_link_of(&out);
        for c in out[idx].ends.iter_mut().flatten() {
            c.link = Some(link);
            c.flip = false;
        }
    }
    out.sort_by(|a, b| {
        (a.line, a.at)
            .partial_cmp(&(b.line, b.at))
            .unwrap_or(Ordering::Equal)
    });
    Some(out)
}

/// 一条曲线从文件里读出的清洗（funnel.clean_curve；None 表示没有可用曲线）。
pub fn clean_curve(c: &Value) -> Option<FunnelCurve> {
    clean_curve_strict(c).ok().flatten()
}

/// [`clean_curve`] 的严格版：Err = Python 会抛异常，Ok(None) = 没有曲线。
fn clean_curve_strict(c: &Value) -> Result<Option<FunnelCurve>, ()> {
    if !py_bool(c) {
        return Ok(None);
    }
    if let Value::Array(a) = c {
        // 旧版的 bends：一根 = 过它的 1/x 曲线，多根 = 单调三次
        if a.is_empty() {
            return Ok(None);
        }
        let bends = clean_bends_strict(c)?;
        let pts = old_curve_points(&bends).ok_or(())?;
        let fitted = fit(&pts, FIT_TOLERANCE);
        return Ok(Some(new_curve(Some(&fitted))));
    }
    let obj = c.as_object().ok_or(())?;
    let pts: Vec<Pt> = match obj.get("pts") {
        None => Vec::new(),
        Some(Value::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for item in a {
                out.push(pair_of(item).ok_or(())?);
            }
            out
        }
        // 空的 {} / "" 迭代起来是空的（同 Python 的 for 循环）
        Some(v) if !py_bool(v) && matches!(v, Value::Object(_) | Value::String(_)) => Vec::new(),
        Some(_) => return Err(()),
    };
    if pts.len() < 4 || !(pts.len() - 1).is_multiple_of(3) {
        return Ok(Some(new_curve(None)));
    }
    let mut pts = pts;
    let last = pts.len() - 1;
    pts[0] = [0.0, 0.0];
    pts[last] = [1.0, 1.0];
    let n = anchor_count(&pts) as i64;
    let sharp = match obj.get("sharp") {
        None => Vec::new(),
        Some(Value::Array(a)) => {
            let mut out: Vec<i64> = Vec::new();
            for item in a {
                let x = py_int(item).ok_or(())?;
                if 0 < x && x < n - 1 {
                    out.push(x);
                }
            }
            out.sort_unstable();
            out.dedup();
            out.into_iter().map(|x| x as usize).collect()
        }
        Some(v) if !py_bool(v) && matches!(v, Value::Object(_) | Value::String(_)) => Vec::new(),
        Some(_) => return Err(()),
    };
    let (link, flip) = match obj.get("link") {
        None | Some(Value::Null) => (None, false),
        Some(v) => (
            Some(py_int(v).ok_or(())?),
            py_bool(obj.get("flip").unwrap_or(&Value::Null)),
        ),
    };
    Ok(Some(FunnelCurve {
        pts,
        sharp,
        link,
        flip,
    }))
}

/// 一条新曲线（funnel.new_curve）：pts 缺省时用 [`DEFAULT_CURVE`]。
pub fn new_curve(pts: Option<&[Pt]>) -> FunnelCurve {
    FunnelCurve {
        pts: match pts {
            Some(p) => p.to_vec(),
            None => DEFAULT_CURVE.to_vec(),
        },
        sharp: Vec::new(),
        link: None,
        flip: false,
    }
}

/// 每条曲线的 `(起点号, 墙端, 曲线)`（funnel.all_curves）。
pub fn all_curves(sh: &Shape) -> Vec<(usize, usize, &FunnelCurve)> {
    let mut out = Vec::new();
    for (k, st) in sh.starts.iter().enumerate() {
        for (end, c) in st.ends.iter().enumerate() {
            if let Some(c) = c {
                out.push((k, end, c));
            }
        }
    }
    out
}

/// 下一个组号（funnel.next_link）。
pub fn next_link(sh: &Shape) -> i64 {
    next_link_of(&sh.starts)
}

/// 起点列表的下一个组号（[`next_link`] 的内部形态）。
fn next_link_of(starts: &[FunnelStart]) -> i64 {
    let mut best = 0;
    for st in starts {
        for c in st.ends.iter().flatten() {
            if let Some(link) = c.link {
                best = best.max(link);
            }
        }
    }
    1 + best
}

/// 与这条曲线连在一起的其他曲线（funnel.partners）：`(起点, 墙端, 是否首尾对调)`。
pub fn partners(sh: &Shape, k: usize, end: usize) -> Vec<(usize, usize, bool)> {
    let c = sh
        .starts
        .get(k)
        .and_then(|st| st.ends.get(end))
        .and_then(|c| c.as_ref());
    let Some(c) = c else { return Vec::new() };
    let Some(link) = c.link else {
        return Vec::new();
    };
    all_curves(sh)
        .into_iter()
        .filter(|(k2, e2, c2)| (*k2, *e2) != (k, end) && c2.link == Some(link))
        .map(|(k2, e2, c2)| (k2, e2, c2.flip != c.flip))
        .collect()
}

/// 曲线的点首尾对调（陡的部分换到另一头，funnel.turned）。
pub fn turned(pts: &[Pt]) -> Vec<Pt> {
    pts.iter().rev().map(|p| [1.0 - p[0], 1.0 - p[1]]).collect()
}

/// 曲线按伙伴拿到的样子（funnel.turned_curve）：flip 时首尾对调，否则一样；不带组号。
pub fn turned_curve(c: &FunnelCurve, flip: bool) -> FunnelCurve {
    if !flip {
        return FunnelCurve {
            pts: c.pts.clone(),
            sharp: c.sharp.clone(),
            link: None,
            flip: false,
        };
    }
    let n = anchor_count(&c.pts);
    let mut sharp: Vec<usize> = c
        .sharp
        .iter()
        .map(|&a| n.saturating_sub(1).saturating_sub(a))
        .collect();
    sharp.sort_unstable();
    FunnelCurve {
        pts: turned(&c.pts),
        sharp,
        link: None,
        flip: false,
    }
}

/// 里外翻过来的曲线（funnel.inside_out）：向另一边鼓（慢起 <-> 快起，S <-> 反 S）；不带组号。
pub fn inside_out(c: &FunnelCurve) -> FunnelCurve {
    FunnelCurve {
        pts: c.pts.iter().map(|p| [p[1], p[0]]).collect(),
        sharp: c.sharp.clone(),
        link: None,
        flip: false,
    }
}

/// 把 shape（一条曲线）的点与尖角给曲线 c，组号不变（funnel.set_shape）。
pub fn set_shape(c: &mut FunnelCurve, shape: &FunnelCurve) {
    c.pts = shape.pts.clone();
    c.sharp = shape.sharp.clone();
}

// ---------------------------------------------------------------- 第一版的 bends（旧工程）

/// 旧版 bends 的清洗（funnel.clean_bends）：坏数据返回 None（Python 抛异常）。
pub fn clean_bends(bends: &Value) -> Option<Vec<Pt>> {
    clean_bends_strict(bends).ok()
}

/// [`clean_bends`] 的严格版。
fn clean_bends_strict(bends: &Value) -> Result<Vec<Pt>, ()> {
    let arr = bends.as_array().ok_or(())?;
    let mut sorted: Vec<Pt> = Vec::with_capacity(arr.len());
    for item in arr {
        let p = pair_of(item).ok_or(())?;
        sorted.push(clamp_bend(p[0], p[1]));
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    sorted.truncate(MAX_BENDS);
    let mut out: Vec<Pt> = Vec::new();
    for p in sorted {
        if out.last().is_none_or(|last| p[0] > last[0] + 1e-4) {
            out.push(p);
        }
    }
    Ok(out)
}

/// 沿旧 bends 的曲线上的点（funnel.old_curve_points）：一根 bend = 过它的 1/x 曲线，
/// 多根 = 单调三次。没有 bend 时 None（Python 抛异常）。
pub fn old_curve_points(bends: &[Pt]) -> Option<Vec<Pt>> {
    let steps = 400;
    let mut us: Vec<f64> = (0..=steps).map(|i| i as f64 / steps as f64).collect();
    if bends.len() == 1 {
        for i in 0..=steps {
            us.push(funnel_u(bends[0], i as f64 / steps as f64));
        }
    }
    us.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    us.dedup();
    if bends.len() == 1 {
        return Some(us.into_iter().map(|u| [u, funnel_f(bends[0], u)]).collect());
    }
    let mut xs = Vec::with_capacity(bends.len() + 2);
    let mut ys = Vec::with_capacity(bends.len() + 2);
    xs.push(0.0);
    ys.push(0.0);
    for p in bends {
        xs.push(p[0]);
        ys.push(p[1]);
    }
    xs.push(1.0);
    ys.push(1.0);
    let curve = smooth_curve(&xs, &ys)?;
    Some(us.into_iter().map(|u| [u, curve.eval(u)]).collect())
}

/// 第一版的漏斗（funnel.old_funnel）：`[起点, 墙顶] + sides` → 它的线 + 墙，以及起点。
/// 返回的 starts 与 [`clean_starts`] 的输入同形（ends 里是 bends 的 list 或 null）。
pub fn old_funnel(sh: &Value) -> Option<(Vec<Pt>, Value)> {
    let obj = sh.as_object()?;
    let raw = obj.get("pts")?.as_array()?;
    if raw.len() != 2 {
        return None; // Python 解包恰好两个
    }
    let p0 = pair_of(&raw[0])?;
    let p1 = pair_of(&raw[1])?;
    let bend = match obj.get("bend") {
        Some(v) if py_bool(v) => pair_of(v)?,
        _ => FUNNEL_BEND,
    };
    let bend = clamp_bend(bend[0], bend[1]);
    let (b0, pitch0, b1, pitch1) = (p0[0], p0[1], p1[0], p1[1]);
    if obj.get("sides").and_then(Value::as_str) == Some("one") {
        let pts = vec![[b0, pitch0], [b1, pitch0], [b1, pitch1], [b1, pitch0]];
        let starts = json!([{"line": 0, "at": 0.0, "ends": [[bend], Value::Null]}]);
        Some((pts, starts))
    } else {
        let h = (pitch1 - pitch0).abs();
        let pts = vec![
            [b0, pitch0],
            [b1, pitch0],
            [b1, pitch0 + h],
            [b1, pitch0 - h],
        ];
        let starts = json!([{"line": 0, "at": 0.0, "ends": [[bend], [bend]]}]);
        Some((pts, starts))
    }
}

/// 把一个 bend 夹到有效范围（funnel.clamp_bend）。
pub fn clamp_bend(u: f64, f: f64) -> Pt {
    [
        0.99_f64.min(0.01_f64.max(u)),
        0.999_f64.min(0.001_f64.max(f)),
    ]
}

/// 1/x 曲线过 bend 点的 `(a, 是否镜像)`；直线时 None（funnel._bend_curve）。
fn bend_curve(bend: Pt) -> Option<(f64, bool)> {
    let [u, f] = clamp_bend(bend[0], bend[1]);
    if (u - f).abs() < 1e-4 {
        return None;
    }
    if f < u {
        return Some((f * (1.0 - u) / (u - f), false));
    }
    Some(((1.0 - f) * u / (f - u), true))
}

/// 一根 bend 的曲线在 u 处张开多少（0..1；0 = 起点，1 = 墙，funnel.funnel_f）。
pub fn funnel_f(bend: Pt, u: f64) -> f64 {
    let Some((a, mirrored)) = bend_curve(bend) else {
        return u;
    };
    let u = if mirrored { 1.0 - u } else { u };
    let y = a * u / (1.0 + a - u);
    if mirrored { 1.0 - y } else { y }
}

/// 一根 bend 的曲线在 y 张开处的位置（0..1，与 [`funnel_f`] 相反，funnel.funnel_u）。
pub fn funnel_u(bend: Pt, y: f64) -> f64 {
    let Some((a, mirrored)) = bend_curve(bend) else {
        return y;
    };
    let y = if mirrored { 1.0 - y } else { y };
    let u = if a + y > 0.0 {
        y * (1.0 + a) / (a + y)
    } else {
        0.0
    };
    if mirrored { 1.0 - u } else { u }
}

/// 过这些点的单调三次（Fritsch-Carlson，funnel._smooth_curve）：平滑且不会冲过这些点。
#[derive(Clone, Debug)]
pub struct SmoothCurve {
    xs: Vec<f64>,
    ys: Vec<f64>,
    h: Vec<f64>,
    m: Vec<f64>,
}

/// 建一条过 `(xs, ys)` 的单调三次；点数不足或长度不一致返回 None（Python 抛异常）。
pub fn smooth_curve(xs: &[f64], ys: &[f64]) -> Option<SmoothCurve> {
    let n = xs.len();
    if n < 3 || ys.len() != n {
        return None;
    }
    let h: Vec<f64> = (0..n - 1).map(|i| xs[i + 1] - xs[i]).collect();
    let d: Vec<f64> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / h[i]).collect();
    let mut m = vec![0.0; n];
    for i in 1..n - 1 {
        if d[i - 1] * d[i] > 0.0 {
            let w1 = 2.0 * h[i] + h[i - 1];
            let w2 = h[i] + 2.0 * h[i - 1];
            m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
        }
    }
    let end_slope = |h0: f64, h1: f64, d0: f64, d1: f64| -> f64 {
        let s = ((2.0 * h0 + h1) * d0 - h0 * d1) / (h0 + h1);
        if s * d0 <= 0.0 {
            return 0.0;
        }
        if d0 * d1 <= 0.0 && s.abs() > 3.0 * d0.abs() {
            return 3.0 * d0;
        }
        s
    };
    m[0] = end_slope(h[0], h[1], d[0], d[1]);
    // Python 的 h[-1] / h[-2]：最后一根与倒数第二根
    m[n - 1] = end_slope(h[n - 2], h[n - 3], d[n - 2], d[n - 3]);
    Some(SmoothCurve {
        xs: xs.to_vec(),
        ys: ys.to_vec(),
        h,
        m,
    })
}

impl SmoothCurve {
    /// 曲线在 u 处的值（同 Python 返回的 fn(u)）。
    pub fn eval(&self, u: f64) -> f64 {
        let n = self.xs.len();
        let i = self
            .xs
            .partition_point(|&x| x <= u)
            .saturating_sub(1)
            .min(n - 2);
        let t = (u - self.xs[i]) / self.h[i];
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * self.ys[i]
            + (t3 - 2.0 * t2 + t) * self.h[i] * self.m[i]
            + (3.0 * t2 - 2.0 * t3) * self.ys[i + 1]
            + (t3 - t2) * self.h[i] * self.m[i + 1]
    }
}

// ---------------------------------------------------------------- 曲线形状（预设、公式）

/// 公式在 n+1 个等距 x 上的点（funnel.formula_curve）：y 拉伸到 0 -> 1。
/// 公式算不出来（Err / 非有限）或首尾一样高时返回 Err。
pub fn formula_curve(f: &dyn Fn(f64) -> Result<f64, String>, n: usize) -> Result<Vec<Pt>, String> {
    if n == 0 {
        return Err("x = i / n 需要 n > 0".to_string());
    }
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let x = i as f64 / n as f64;
        let y = match f(x) {
            Ok(y) => y,
            Err(_) => return Err(format!("it can't be worked out at x = {x}")),
        };
        if !y.is_finite() {
            return Err(format!("it can't be worked out at x = {x}"));
        }
        pts.push([x, y]);
    }
    let y0 = pts[0][1];
    let y1 = pts[pts.len() - 1][1];
    if (y1 - y0).abs() < 1e-12 {
        return Err(
            "it has to end at a different height than it starts (x = 0 and x = 1)".to_string(),
        );
    }
    Ok(pts
        .into_iter()
        .map(|[x, y]| [x, (y - y0) / (y1 - y0)])
        .collect())
}

/// 公式对应的曲线（funnel.preset_curve；None = 默认曲线）。
pub fn preset_curve(
    formula: Option<&dyn Fn(f64) -> Result<f64, String>>,
) -> Result<FunnelCurve, String> {
    let Some(f) = formula else {
        return Ok(new_curve(None));
    };
    let mut pts = fit(&formula_curve(f, 400)?, FIT_TOLERANCE);
    if pts.len() < 2 {
        return Ok(new_curve(None));
    }
    pts[0] = [0.0, 0.0];
    let last = pts.len() - 1;
    pts[last] = [1.0, 1.0];
    Ok(new_curve(Some(&pts)))
}

// ---------------------------------------------------------------- 线 / 起点 / 方盒

/// 把线（编号）与曲线 `((起点, 墙端))` 从漏斗里拿掉（funnel.remove_funnel_parts）：
/// 起点在被拿掉的线上的曲线跟着走，下一条线接任第一条。没有线剩下时返回 false（删漏斗）。
pub fn remove_funnel_parts(sh: &mut Shape, lines: &[usize], curves: &[(usize, usize)]) -> bool {
    let old = funnel_lines(sh);
    let keep: Vec<usize> = (0..old.len()).filter(|n| !lines.contains(n)).collect();
    if keep.is_empty() {
        return false;
    }
    for &(k, end) in curves {
        if let Some(st) = sh.starts.get_mut(k)
            && let Some(slot) = st.ends.get_mut(end)
        {
            *slot = None;
        }
    }
    let mut number: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, &n) in keep.iter().enumerate() {
        number.insert(n, i);
    }
    let mut pts: Vec<Pt> = old[keep[0]].to_vec();
    pts.extend(sh.pts.iter().skip(2).take(2).copied());
    for &n in &keep[1..] {
        pts.extend_from_slice(&old[n]);
    }
    sh.pts = pts;
    let mut starts: Vec<FunnelStart> = Vec::new();
    for st in &sh.starts {
        if st.ends.iter().any(Option::is_some)
            && let Some(&line) = number.get(&st.line)
        {
            starts.push(FunnelStart {
                line,
                at: st.at,
                ends: st.ends.clone(),
            });
        }
    }
    sh.starts = starts;
    true
}

/// 漏斗每条线的 `(起点, 终点)`（不含墙，funnel.funnel_lines）。
pub fn funnel_lines(sh: &Shape) -> Vec<[Pt; 2]> {
    let pts = &sh.pts;
    let mut out = Vec::new();
    if pts.len() >= 2 {
        out.push([pts[0], pts[1]]);
    }
    let mut i = 4;
    while i + 1 < pts.len() {
        out.push([pts[i], pts[i + 1]]);
        i += 2;
    }
    out
}

/// 线号 `line` 在 `sh.pts` 里的起点（funnel.line_index）。
pub fn line_index(line: usize) -> usize {
    if line == 0 { 0 } else { 2 + 2 * line }
}

/// 线 `line` 上 `at`（0..1）处的点（funnel.start_point）；线号超出 `pts` 时 None。
pub fn start_point(sh: &Shape, at: f64, line: usize) -> Option<Pt> {
    let i = line_index(line);
    let b = sh.pts.get(i)?;
    let c = sh.pts.get(i + 1)?;
    Some([b[0] + (c[0] - b[0]) * at, b[1] + (c[1] - b[1]) * at])
}

/// 线 `line` 上 `at` 处的起点到墙端 `end`（0 / 1）的曲线方盒 `(S, U, V)`；
/// 那个墙端落在线上的没有可张开的，返回 None（funnel.curve_box）。
pub fn curve_box(sh: &Shape, at: f64, end: usize, line: usize) -> Option<(Pt, Pt, Pt)> {
    if sh.pts.len() < 4 {
        return None;
    }
    let s = start_point(sh, at, line)?;
    let i = line_index(line);
    let b0 = *sh.pts.get(i)?;
    let b1 = *sh.pts.get(i + 1)?;
    let w0 = sh.pts[2];
    let w1 = sh.pts[3];
    let e = *sh.pts.get(2 + end)?;
    let d1 = [b1[0] - b0[0], b1[1] - b0[1]];
    let d2 = [w1[0] - w0[0], w1[1] - w0[1]];
    let ex = e[0] - s[0];
    let ey = e[1] - s[1];
    let det = d1[0] * d2[1] - d1[1] * d2[0];
    let thr = 1e-6 * hypot2(d1[0], d1[1]) * hypot2(d2[0], d2[1]);
    let (u, v) = if det.abs() > thr && thr > 0.0 {
        let x = (ex * d2[1] - ey * d2[0]) / det;
        let y = (d1[0] * ey - d1[1] * ex) / det;
        ([x * d1[0], x * d1[1]], [y * d2[0], y * d2[1]])
    } else {
        // 线与墙平行（或是一个点）：普通的时间 / 音高方盒
        ([ex, 0.0], [0.0, ey])
    };
    if (u[0] * v[1] - u[1] * v[0]).abs() < 1e-9 {
        return None;
    }
    Some((s, u, v))
}

/// 方盒里 `(u, f)` 处的点（funnel.box_point）。
pub fn box_point(box_: &(Pt, Pt, Pt), u: f64, f: f64) -> Pt {
    let (s, ub, vb) = box_;
    [s[0] + u * ub[0] + f * vb[0], s[1] + u * ub[1] + f * vb[1]]
}

/// 点 `(beat, pitch)` 在曲线方盒里的 `(u, f)`（funnel.box_uf）。
pub fn box_uf(box_: &(Pt, Pt, Pt), b: f64, p: f64) -> Pt {
    let (s, ub, vb) = box_;
    let det = ub[0] * vb[1] - ub[1] * vb[0];
    let x = b - s[0];
    let y = p - s[1];
    [(x * vb[1] - y * vb[0]) / det, (ub[0] * y - ub[1] * x) / det]
}

/// 每条曲线 `(起点号, 墙端, 从起点到墙端的点列, 线与墙的交点)`（funnel.funnel_curves）。
/// short：每条曲线停在最后一个 key 里一点点，那个 key 只在墙上碰到（给正好从墙开始的音符用）。
pub fn funnel_curves(sh: &Shape, short: bool) -> Vec<(usize, usize, Vec<Pt>, Pt)> {
    let mut out = Vec::new();
    for (k, st) in sh.starts.iter().enumerate() {
        for (end, c) in st.ends.iter().enumerate() {
            let Some(c) = c else { continue };
            let Some(mut b) = curve_box(sh, st.at, end, st.line) else {
                continue;
            };
            if short && b.2[1].abs() > 2.0 * EDGE {
                let kk = 1.0 - EDGE / b.2[1].abs();
                b.2 = [b.2[0] * kk, b.2[1] * kk];
            }
            let corner = box_point(&b, 1.0, 0.0);
            let curve = sample(&c.pts, 64)
                .into_iter()
                .map(|[u, f]| box_point(&b, u, f))
                .collect();
            out.push((k, end, curve, corner));
        }
    }
    out
}

/// 线 `line` 上 `at` 处的新起点，向两个墙端张开（funnel.new_start）；两条曲线连成一组
/// （隔在线的两侧，看起来是镜像）。都张不开时 None。
pub fn new_start(sh: &Shape, at: f64, line: usize) -> Option<FunnelStart> {
    let mut ends: [Option<FunnelCurve>; 2] = [None, None];
    for (end, slot) in ends.iter_mut().enumerate() {
        if curve_box(sh, at, end, line).is_some() {
            *slot = Some(new_curve(None));
        }
    }
    if ends.iter().all(Option::is_some) {
        let link = next_link(sh);
        for c in ends.iter_mut().flatten() {
            c.link = Some(link);
            c.flip = false;
        }
    }
    if ends.iter().any(Option::is_some) {
        Some(FunnelStart { line, at, ends })
    } else {
        None
    }
}

/// 手柄点的身份（funnel.funnel_handles 的 id）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandleId {
    /// 每个起点：`(起点号)`。
    Start(usize),
    /// 拉出锚点的手柄点：`(起点号, 墙端, 点号)`。
    Ctrl(usize, usize, usize),
    /// 两端之间的锚点：`(起点号, 墙端, 点号)`。
    Anchor(usize, usize, usize),
}

/// `[(beat, pitch, id)]`（funnel.funnel_handles）：每个起点一个 "start"，
/// 每个拉出锚点的手柄一个 "ctrl"，两端之间的锚点一个 "anchor"。
pub fn funnel_handles(sh: &Shape) -> Vec<(Pt, HandleId)> {
    let mut out = Vec::new();
    let mut ctrls = Vec::new();
    let mut anchors = Vec::new();
    for (k, st) in sh.starts.iter().enumerate() {
        if let Some(p) = start_point(sh, st.at, st.line) {
            out.push((p, HandleId::Start(k)));
        }
        for (end, c) in st.ends.iter().enumerate() {
            let Some(c) = c else { continue };
            let Some(b) = curve_box(sh, st.at, end, st.line) else {
                continue;
            };
            for (i, &[u, f]) in c.pts.iter().enumerate() {
                let ha = handle_anchor(i);
                if i % 3 != 0 && ha < c.pts.len() && c.pts[i] != c.pts[ha] {
                    ctrls.push((box_point(&b, u, f), HandleId::Ctrl(k, end, i)));
                } else if i % 3 == 0 && i > 0 && i < c.pts.len() - 1 {
                    anchors.push((box_point(&b, u, f), HandleId::Anchor(k, end, i)));
                }
            }
        }
    }
    out.extend(ctrls);
    out.extend(anchors);
    out
}

/// 手柄线 `(锚点, 手柄点, (起点号, 墙端))`，点是 `(beat, pitch)`（funnel.funnel_handle_lines）。
pub fn funnel_handle_lines(sh: &Shape) -> Vec<(Pt, Pt, (usize, usize))> {
    let mut out = Vec::new();
    for (k, end, c) in all_curves(sh) {
        let st = &sh.starts[k];
        let Some(b) = curve_box(sh, st.at, end, st.line) else {
            continue;
        };
        for i in 0..c.pts.len() {
            let ha = handle_anchor(i);
            if i % 3 != 0 && ha < c.pts.len() && c.pts[i] != c.pts[ha] {
                let anchor = box_point(&b, c.pts[ha][0], c.pts[ha][1]);
                let handle = box_point(&b, c.pts[i][0], c.pts[i][1]);
                out.push((anchor, handle, (k, end)));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- 直线部分与区域

/// 直线部分：每条线，然后墙，都是 `(beat, pitch)` 折线（funnel.funnel_strokes）。
pub fn funnel_strokes(sh: &Shape) -> Vec<Vec<Pt>> {
    let mut out: Vec<Vec<Pt>> = funnel_segments(sh)
        .into_iter()
        .map(|seg| seg.to_vec())
        .collect();
    out.extend(funnel_curves(sh, false).into_iter().map(|(_, _, c, _)| c));
    out
}

/// 直的段：每条线，然后墙（funnel.funnel_segments）。
pub fn funnel_segments(sh: &Shape) -> Vec<[Pt; 2]> {
    let lines = funnel_lines(sh);
    let mut out: Vec<[Pt; 2]> = Vec::new();
    if let Some(first) = lines.first() {
        out.push(*first);
    }
    if sh.pts.len() >= 4 {
        out.push([sh.pts[2], sh.pts[3]]);
    }
    out.extend(lines.iter().skip(1).copied());
    out
}

/// 每条曲线的区域：先曲线，再沿墙与线回来（funnel.funnel_polys）。
pub fn funnel_polys(sh: &Shape, short: bool) -> Vec<Vec<Pt>> {
    funnel_curves(sh, short)
        .into_iter()
        .map(|(_, _, curve, corner)| {
            let first = curve[0];
            let mut poly = curve;
            poly.push(corner);
            poly.push(first);
            poly
        })
        .collect()
}

/// `(beat, pitch)` 在漏斗某条曲线的区域里吗（funnel.funnel_contains）？
pub fn funnel_contains(sh: &Shape, b: f64, p: f64) -> bool {
    for poly in funnel_polys(sh, false) {
        let mut inside = false;
        for w in poly.windows(2) {
            let (a, c) = (w[0], w[1]);
            if (a[1] <= p) != (c[1] <= p) && b < a[0] + (c[0] - a[0]) * (p - a[1]) / (c[1] - a[1]) {
                inside = !inside;
            }
        }
        if inside {
            return true;
        }
    }
    false
}

/// 直线 a -> b 落在 key q 上的 `(第一个 beat, 最后一个 beat)`，没有时 None（funnel.line_band）。
pub fn line_band(a: Pt, b: Pt, q: i64) -> Option<(f64, f64)> {
    let (ta, ya) = (a[0], a[1]);
    let (tb, yb) = (b[0], b[1]);
    if ya == yb {
        return if pitch_of(ya) == q {
            Some((ta.min(tb), ta.max(tb)))
        } else {
            None
        };
    }
    let q = q as f64;
    let mut u0 = (q - 0.5 - ya) / (yb - ya);
    let mut u1 = (q + 0.5 - ya) / (yb - ya);
    if u0 > u1 {
        std::mem::swap(&mut u0, &mut u1);
    }
    u0 = 0.0_f64.max(u0);
    u1 = 1.0_f64.min(u1);
    if u0 > u1 {
        return None;
    }
    let x0 = ta + (tb - ta) * u0;
    let x1 = ta + (tb - ta) * u1;
    Some((x0.min(x1), x0.max(x1)))
}

/// 一段 / 一列点覆盖的 key 范围（funnel._keys）。
fn keys(ps: &[f64]) -> RangeInclusive<i64> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &y in ps {
        lo = lo.min(y);
        hi = hi.max(y);
    }
    0.max(pitch_of(lo))..=crate::paths::TOP_KEY.min(pitch_of(hi))
}

/// key → 区间列表（保持插入顺序，Python dict；`funnel_cells` 的输出顺序跟着它走）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spans {
    entries: Vec<(i64, Vec<[f64; 2]>)>,
}

impl Spans {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 有没有这个 key。
    pub fn get(&self, k: i64) -> Option<&[[f64; 2]]> {
        self.entries
            .iter()
            .find(|e| e.0 == k)
            .map(|e| e.1.as_slice())
    }

    /// key 对应的区间列表，没有就（按插入顺序）新建一个。
    pub fn entry(&mut self, k: i64) -> &mut Vec<[f64; 2]> {
        match self.entries.iter().position(|e| e.0 == k) {
            Some(i) => &mut self.entries[i].1,
            None => {
                self.entries.push((k, Vec::new()));
                let i = self.entries.len() - 1;
                &mut self.entries[i].1
            }
        }
    }

    /// 放入一个 key 的区间列表（已有就替换）。
    pub fn insert(&mut self, k: i64, v: Vec<[f64; 2]>) {
        match self.entries.iter().position(|e| e.0 == k) {
            Some(i) => self.entries[i].1 = v,
            None => self.entries.push((k, v)),
        }
    }

    /// 按插入顺序遍历 `(key, 区间列表)`。
    pub fn iter(&self) -> impl Iterator<Item = (i64, &[[f64; 2]])> {
        self.entries.iter().map(|(k, v)| (*k, v.as_slice()))
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是不是空的。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `{key: [[第一个 beat, 最后一个 beat], ...]}`（funnel.funnel_key_spans）：
/// 每个 key 演奏的范围（排好序、重叠的合并）。
pub fn funnel_key_spans(sh: &Shape) -> Spans {
    let mut pieces = Spans::new();
    for [a, b] in funnel_segments(sh) {
        for q in keys(&[a[1], b[1]]) {
            if let Some((s0, s1)) = line_band(a, b, q) {
                pieces.entry(q).push([s0, s1]);
            }
        }
    }
    for poly in funnel_polys(sh, sh.wall == WallMode::Past) {
        if poly.is_empty() {
            continue;
        }
        let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &poly {
            ylo = ylo.min(p[1]);
            yhi = yhi.max(p[1]);
        }
        for q in keys(&[ylo, yhi]) {
            let spans = row_spans(std::slice::from_ref(&poly), q as f64);
            pieces.entry(q).extend(spans);
        }
    }
    let mut out = Spans::new();
    for (q, spans) in pieces.iter() {
        let mut sorted = spans.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        let mut merged: Vec<[f64; 2]> = Vec::new();
        for s in sorted {
            match merged.last_mut() {
                Some(last) if s[0] <= last[1] + 1e-9 => last[1] = last[1].max(s[1]),
                _ => merged.push(s),
            }
        }
        if !merged.is_empty() {
            out.insert(q, merged);
        }
    }
    out
}

// ---------------------------------------------------------------- 布局与音符

/// 音符网格的轴 `(t0, sign, length)`（funnel.funnel_axis）：网格从线的起点朝墙走
/// （sign -1 = 时间上倒退：反向漏斗），length = 起点到墙的 tick 距离。
pub fn funnel_axis(sh: &Shape, spans: &Spans) -> Option<(f64, i64, f64)> {
    let pts = &sh.pts;
    let p0 = pts.first()?[0];
    let ref_ = if pts.len() >= 4 {
        (pts[2][0] + pts[3][0]) / 2.0
    } else {
        pts.get(1)?[0]
    };
    if (ref_ - p0).abs() > 1e-9 {
        return Some((p0, if ref_ > p0 { 1 } else { -1 }, (ref_ - p0).abs()));
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut any = false;
    for (_, s) in spans.iter() {
        for span in s {
            lo = lo.min(span[0]);
            hi = hi.max(span[1]);
            any = true;
        }
    }
    if !any {
        return None;
    }
    if hi - lo < 1e-9 {
        // 只是一条竖线（墙，先画的）：一列墙 gate 结束在它上面
        return Some((lo, -1, sh.gate1));
    }
    Some((lo, 1, hi - lo)) // 线与墙同一时间开始（转过来的漏斗）：从左到右
}

/// 墙在时间上比线的起点早吗（funnel.funnel_reversed）？
pub fn funnel_reversed(sh: &Shape) -> bool {
    let pts = &sh.pts;
    pts.len() >= 4 && (pts[2][0] + pts[3][0]) / 2.0 < pts[0][0] - 1e-9
}

/// 音符来源的一切（funnel.funnel_layout）：`(网格距离里的 spans, 墙的范围, 网格长度, t0 tick, sign)`。
/// 网格距离是从线的起点朝墙的 tick 数。漏斗不出音符时 None。
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelLayout {
    /// 每个 key 的范围（网格距离）。
    pub dspans: Spans,
    /// 墙在每个 key 上的范围（网格距离）。
    pub walls: BTreeMap<i64, [f64; 2]>,
    /// 起点到墙的 tick 距离（至少 1）。
    pub length: f64,
    /// 线起点在时间轴上的 tick。
    pub t0: f64,
    /// 网格朝墙的方向（-1 = 倒退）。
    pub sign: i64,
}

/// 漏斗的布局（funnel.funnel_layout）；不出音符时 None。
pub fn funnel_layout(sh: &Shape, ppq: f64) -> Option<FunnelLayout> {
    let spans = funnel_key_spans(sh);
    if spans.is_empty() {
        return None;
    }
    let (t0b, sign, length) = funnel_axis(sh, &spans)?;
    let t0 = t0b * ppq;
    let dist = |beat: f64| sign as f64 * (beat * ppq - t0);
    let mut dspans = Spans::new();
    for (q, s) in spans.iter() {
        let mut v = Vec::with_capacity(s.len());
        for span in s {
            let (a, b) = (dist(span[0]), dist(span[1]));
            v.push([a.min(b), a.max(b)]);
        }
        dspans.insert(q, v);
    }
    let mut walls = BTreeMap::new();
    if sh.pts.len() >= 4 {
        let (w0, w1) = (sh.pts[2], sh.pts[3]);
        for q in keys(&[w0[1], w1[1]]) {
            if let Some((s0, s1)) = line_band(w0, w1, q) {
                let (a, b) = (dist(s0), dist(s1));
                walls.insert(q, [a.min(b), a.max(b)]);
            }
        }
    }
    Some(FunnelLayout {
        dspans,
        walls,
        length: (length * ppq).max(1.0),
        t0,
        sign,
    })
}

/// w(d)：0..1，网格距离 d 处有多少 key 在演奏（1 个 key = 0，最多 = 1；funnel.funnel_openness）。
#[derive(Clone, Debug)]
pub struct Openness {
    xs: Vec<f64>,
    counts: Vec<i64>,
    top: i64,
}

/// 从 spans 建 [`Openness`]（funnel.funnel_openness）。
pub fn funnel_openness(dspans: &Spans) -> Openness {
    let mut events: Vec<(f64, i8)> = Vec::new();
    for (_, spans) in dspans.iter() {
        for s in spans {
            events.push((s[0], 1));
            events.push((s[1], -1));
        }
    }
    events.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut xs: Vec<f64> = Vec::new();
    let mut counts: Vec<i64> = Vec::new();
    let mut c = 0_i64;
    for (x, step) in events {
        c += i64::from(step);
        if xs.last() == Some(&x) {
            let last = counts.len() - 1;
            counts[last] = c;
        } else {
            xs.push(x);
            counts.push(c);
        }
    }
    let top = counts.iter().copied().max().unwrap_or(0);
    Openness { xs, counts, top }
}

impl Openness {
    /// 网格距离 d 处的张开程度。
    pub fn w(&self, d: f64) -> f64 {
        let i = self.xs.partition_point(|&x| x <= d) as i64 - 1;
        let n = if i >= 0 { self.counts[i as usize] } else { 0 };
        if self.top > 1 {
            ((n - 1) as f64 / (self.top - 1) as f64).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// w 那么深的音符的 gate（tick，funnel.funnel_gate）：0 = 起点 gate，1 = 墙 gate。
pub fn funnel_gate(sh: &Shape, g0: f64, g1: f64, w: f64) -> f64 {
    let mut g = g0 * (g1 / g0).powf(w);
    if sh.change == GateChange::Steps && g0 != g1 {
        g = g0 * 2.0_f64.powf(round_half_even((g / g0).log2()));
        g = g.clamp(g0.min(g1), g0.max(g1));
    }
    g.max(1.0)
}

/// spam 漏斗每个 key 共用的音符网格（funnel.funnel_grid），以网格距离表示：从线的起点到墙
/// （剩下不到半个 gate 的并入最后一个音符），然后越过一切再多一个音符。
pub fn funnel_grid(sh: &Shape, ppq: f64, dspans: &Spans, length: f64) -> Vec<f64> {
    let g0 = sh.gate0 * ppq;
    let g1 = sh.gate1 * ppq;
    let opened = if sh.follow == GateFollow::Curve {
        Some(funnel_openness(dspans))
    } else {
        None
    };
    let gate = |d: f64| -> f64 {
        if d >= length {
            return funnel_gate(sh, g0, g1, 1.0);
        }
        let w = match &opened {
            Some(o) => o.w(d),
            None => (d / length).clamp(0.0, 1.0),
        };
        funnel_gate(sh, g0, g1, w)
    };
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut any = false;
    for (_, spans) in dspans.iter() {
        for s in spans {
            lo = lo.min(s[0]);
            hi = hi.max(s[1]);
            any = true;
        }
    }
    if !any {
        return Vec::new(); // Python 对空 dspans 会 ValueError（调用方保证非空）
    }
    let mut marks = vec![0.0_f64];
    let mut d = 0.0;
    while d + 1.5 * gate(d) <= length {
        d += gate(d);
        marks.push(d);
    }
    marks.push(length);
    while marks[marks.len() - 1] < hi {
        let last = marks[marks.len() - 1];
        marks.push(last + gate(last));
    }
    let last = marks[marks.len() - 1];
    marks.push(last + gate(last)); // 给墙后面那列留位置
    while marks[0] > lo {
        let first = marks[0];
        marks.insert(0, first - g0);
    }
    marks
}

/// 离 x 最近的 `xs` 序号（funnel._nearest）。
fn nearest(xs: &[f64], x: f64) -> i64 {
    let mut i = xs.partition_point(|&v| v < x);
    if i > 0 && (i == xs.len() || x - xs[i - 1] <= xs[i] - x) {
        i -= 1;
    }
    i as i64
}

/// spam：`(网格 tick, [(key, 第一条网格线, 最后一条网格线)])`，每个 key 的音符从网格线
/// 走到网格线。long：`(None, [(key, 起始 tick, 结束 tick)])`（funnel.funnel_cells）。
pub fn funnel_cells(sh: &Shape, ppq: f64) -> (Option<Vec<i64>>, Vec<[i64; 3]>) {
    let Some(lay) = funnel_layout(sh, ppq) else {
        return (None, Vec::new());
    };
    let FunnelLayout {
        dspans,
        walls,
        length,
        t0,
        sign,
    } = lay;
    let past = sh.wall == WallMode::Past;
    let g1 = 1.max(floor_half(sh.gate1 * ppq));
    let at_wall = |q: i64, d: f64| -> bool {
        match walls.get(&q) {
            Some(w) => w[0] - 1.0 <= d && d <= w[1] + 1.0,
            None => false,
        }
    };
    let tick = |d: f64| floor_half(t0 + sign as f64 * d);
    if sh.funnel_fill == FunnelFill::Long {
        let mut out = Vec::new();
        for (q, spans) in dspans.iter() {
            for s in spans {
                let (a, b) = (s[0], s[1]);
                // 墙外的一整个 gate
                let far = tick(b) + if past && at_wall(q, b) { sign * g1 } else { 0 };
                let (ta, tb) = (tick(a), far);
                let (s0, e0) = if ta <= tb { (ta, tb) } else { (tb, ta) };
                out.push([q, s0, e0.max(s0 + 1)]);
            }
        }
        return (None, out);
    }
    let marks = funnel_grid(sh, ppq, &dspans, length);
    let mut ticks: Vec<i64> = Vec::new();
    let mut ds: Vec<f64> = Vec::new();
    for d in marks {
        let t = tick(d);
        if ticks.last() != Some(&t) {
            ticks.push(t);
            ds.push(sign as f64 * (t as f64 - t0));
        }
    }
    let mut out = Vec::new();
    for (q, spans) in dspans.iter() {
        let mut ranges: Vec<[i64; 2]> = Vec::new();
        for s in spans {
            let (a, b) = (s[0], s[1]);
            let mut i = nearest(&ds, a);
            let mut j = nearest(&ds, b);
            if past && at_wall(q, b) {
                j = (j + 1).min(ds.len() as i64 - 1); // 到墙后一个音符（墙上的 key：就那一个）
            } else if j <= i {
                // 比一个音符短：它所在的音符（在网格线上时就是结束在那里的那个）
                let mid = (a + b) / 2.0 - 1e-9;
                let at = ds.partition_point(|&v| v < mid) as i64;
                j = at.max(1).min(ds.len() as i64 - 1);
                i = j - 1;
            }
            match ranges.last_mut() {
                Some(last) if i <= last[1] => last[1] = last[1].max(j),
                _ => ranges.push([i, j]),
            }
        }
        for r in ranges {
            out.push([q, r[0], r[1]]);
        }
    }
    (Some(ticks), out)
}

/// 漏斗的音符数（funnel.funnel_note_count）。
pub fn funnel_note_count(sh: &Shape, ppq: f64) -> i64 {
    let (ticks, cells) = funnel_cells(sh, ppq);
    match ticks {
        None => cells.len() as i64,
        Some(_) => cells.iter().map(|c| c[2] - c[1]).sum(),
    }
}

/// 漏斗的音符行 `[start, end, key]`（funnel.funnel_notes）。
pub fn funnel_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    let (ticks, cells) = funnel_cells(sh, ppq);
    match ticks {
        None => cells.iter().map(|c| [c[1], c[2], c[0]]).collect(),
        Some(ticks) => {
            // 每个 key：从每条网格线到下一线，从它的第一条线到 last
            let mut out = Vec::new();
            for c in cells {
                let (q, i, j) = (c[0], c[1], c[2]);
                for k in 0..(j - i) {
                    if let (Some(&a), Some(&b)) =
                        (ticks.get((i + k) as usize), ticks.get((i + k + 1) as usize))
                    {
                        out.push([a.min(b), a.max(b), q]);
                    }
                }
            }
            out
        }
    }
}
