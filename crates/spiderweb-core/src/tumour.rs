//! 线条上的肿瘤（凸起）：把 Python `notes/tumour.py` 逐函数移植。
//!
//! 线条、折线、自由笔、曲线或弧都能带肿瘤设置 `sh["tumour"]`：线条的点仍按原样绘制（仍可拖动），
//! 但它成形的路径每隔 `dist` 长出一个凸起，每个 `length` 长、`size` 个 key 高，朝向由 `side` 决定。
//! `ease` > 0 时凸起在区间两端 `ease` 内从无到有地长出来，线条平滑地进入凸起，而不是突然竖起。
//! `fit` 把距离稍微拉伸，让整数个凸起恰好铺满区间；绕闭合环（比如整圆）一周时首尾正好接上。
//! 一切按屏幕上的样子计算：`k` = 上次修改设置时屏幕上一个 key 对应多少 beat（同 arc），
//! 所以 size 的单位是 key，length / dist 是沿线条的 beat。length 0 = 尖刺：每个凸起只是
//! 一个被推向一侧的点，线条从一个尖刺直着折向下一个。

use std::collections::BTreeMap;

use crate::arc::arc_points;
use crate::shape::{Tumour, TumourShape, TumourSide, TumourWrap};
use crate::{Pt, dist, hypot2, round_half_even};
use serde_json::{Map, Value};

/// 最多多少个凸起（tumour.py MAX_TUMOURS）。
pub const MAX_TUMOURS: usize = 20000;

/// The settings that can follow a graph (1.2.0 tumour.GRAPH_KEYS).
pub const GRAPH_KEYS: [&str; 5] = ["size", "length", "dist", "rot", "slant"];

/// Graph values from -1000 % to 1000 % (1.2.0 tumour.GRAPH_LIMIT).
pub const GRAPH_LIMIT: f64 = 10.0;

/// 圆形模板的采样步长（Python `math.radians(10)`）。
const CIRCLE_STEP: f64 = 10.0 * (std::f64::consts::PI / 180.0);

// ---------------------------------------------------------------------------
// 小工具：CPython / NumPy 语义的逐位复刻
// ---------------------------------------------------------------------------

/// CPython `bisect.bisect_right`（并列时取右侧）。
fn bisect_right(a: &[f64], x: f64) -> usize {
    let mut lo = 0usize;
    let mut hi = a.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if x < a[mid] {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

/// CPython `bisect.bisect_left`（并列时取左侧）。
fn bisect_left(a: &[f64], x: f64) -> usize {
    let mut lo = 0usize;
    let mut hi = a.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if a[mid] < x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// NumPy `np.mod`：结果的符号与除数一致（零的符号也随除数）。
fn py_mod(x: f64, y: f64) -> f64 {
    let r = x % y;
    if r != 0.0 {
        if (r < 0.0) != (y < 0.0) { r + y } else { r }
    } else {
        0.0_f64.copysign(y)
    }
}

/// CPython `max(a, b)`（并列时保留先出现的 a）。
fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// CPython `min(a, b)`（并列时保留先出现的 a）。
fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// NumPy `np.interp(x, xp, fp)`（xp 单调不减；x 在两端之外时取端点的 fp）。
fn np_interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    let Some(&first) = xp.first() else {
        return f64::NAN;
    };
    if x <= first {
        return fp.first().copied().unwrap_or(f64::NAN);
    }
    let Some(&last) = xp.last() else {
        return f64::NAN;
    };
    if x >= last {
        return fp.last().copied().unwrap_or(f64::NAN);
    }
    let i = bisect_right(xp, x) - 1;
    let denom = xp[i + 1] - xp[i];
    if denom == 0.0 {
        return fp[i + 1];
    }
    fp[i] + (fp[i + 1] - fp[i]) * (x - xp[i]) / denom
}

/// NumPy `np.linspace(lo, hi, n)`（端点精确取 lo / hi）。
fn np_linspace(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![lo];
    }
    let step = (hi - lo) / (n - 1) as f64;
    let mut out: Vec<f64> = (0..n).map(|i| lo + step * i as f64).collect();
    if let Some(last) = out.last_mut() {
        *last = hi;
    }
    out
}

// ---------------------------------------------------------------------------
// clean_tumour
// ---------------------------------------------------------------------------

fn shape_from_name(name: &str) -> Option<TumourShape> {
    Some(match name {
        "triangle" => TumourShape::Triangle,
        "square" => TumourShape::Square,
        "circle" => TumourShape::Circle,
        "parabola" => TumourShape::Parabola,
        _ => return None,
    })
}

fn side_from_name(name: &str) -> Option<TumourSide> {
    Some(match name {
        "alt" => TumourSide::Alt,
        "left" => TumourSide::Left,
        "right" => TumourSide::Right,
        "random" => TumourSide::Random,
        _ => return None,
    })
}

fn wrap_from_name(name: &str) -> Option<TumourWrap> {
    Some(match name {
        "simple" => TumourWrap::Simple,
        "wrap" => TumourWrap::Wrap,
        _ => return None,
    })
}

/// Python `float(s)`：可选的空白、数字之间允许下划线、inf / nan 均可。
fn py_float_str(s: &str) -> Option<f64> {
    let t = s.trim();
    if !t.contains('_') {
        return t.parse::<f64>().ok();
    }
    let mut cleaned = String::with_capacity(t.len());
    let bytes = t.as_bytes();
    for (i, ch) in t.char_indices() {
        if ch == '_' {
            let prev = i > 0 && bytes[i - 1].is_ascii_digit();
            let next = i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
            if !(prev && next) {
                return None;
            }
        } else {
            cleaned.push(ch);
        }
    }
    cleaned.parse::<f64>().ok()
}

/// Python `float(x)`：数字 / 布尔 / 字符串可转，其余不可（None 表示转不了）。
fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => py_float_str(s),
        _ => None,
    }
}

/// Python `int(s)`（十进制，允许空白、正负号与数字间的下划线）。
fn py_int_str(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    for (i, ch) in body.char_indices() {
        if ch == '_' {
            let prev = i > 0 && bytes[i - 1].is_ascii_digit();
            let next = i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
            if !(prev && next) {
                return None;
            }
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        cleaned.push(ch);
    }
    let n: i64 = cleaned.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// Python `int(x)`：数字 / 布尔 / 字符串可转，其余不可（None 表示转不了）。
/// 超出 i64 的种子这里当转不了（随机数发生器用 i64 下标，Python 的任意精度整数种子在此不可达）。
fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok()
            } else {
                let f = n.as_f64()?;
                let bound = 2f64.powi(63);
                if f.is_finite() && f >= -bound && f < bound {
                    Some(f.trunc() as i64)
                } else {
                    None
                }
            }
        }
        Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        Value::String(s) => py_int_str(s),
        _ => None,
    }
}

/// 一个数值字段：取出来、转成 float、不是有限数就当没有（`None`）。
fn num(obj: &Map<String, Value>, key: &str) -> Option<f64> {
    let v = py_float(obj.get(key)?)?;
    if v.is_finite() { Some(v) } else { None }
}

/// 从文件里读出的肿瘤设置（不是字典时为 None；对应 Python `clean_tumour`）。
pub fn clean_tumour(tm: &Value) -> Option<Tumour> {
    let obj = tm.as_object()?;
    let mut out = Tumour::default();
    if let Some(shape) = obj
        .get("shape")
        .and_then(Value::as_str)
        .and_then(shape_from_name)
    {
        out.shape = shape;
    }
    if let Some(side) = obj
        .get("side")
        .and_then(Value::as_str)
        .and_then(side_from_name)
    {
        out.side = side;
    }
    if let Some(wrap) = obj
        .get("wrap")
        .and_then(Value::as_str)
        .and_then(wrap_from_name)
    {
        out.wrap = wrap;
    }
    if let Some(v) = num(obj, "size") {
        out.size = py_min(1000.0, py_max(-1000.0, v));
    }
    if let Some(v) = num(obj, "length") {
        out.length = py_min(1e6, py_max(0.0, v));
    }
    if let Some(v) = num(obj, "dist") {
        out.dist = py_min(1e6, py_max(1e-9, v));
    }
    if let Some(v) = num(obj, "start") {
        out.start = py_min(1.0, py_max(0.0, v));
    }
    if let Some(v) = num(obj, "end") {
        out.end = py_min(1.0, py_max(0.0, v));
    }
    if let Some(v) = num(obj, "ease") {
        out.ease = py_min(1e6, py_max(0.0, v));
    }
    if let Some(v) = num(obj, "k") {
        out.k = py_min(1e9, py_max(1e-9, v));
    }
    if let Some(v) = num(obj, "rot") {
        out.rot = py_min(180.0, py_max(-180.0, v));
    }
    if let Some(v) = num(obj, "slant") {
        out.slant = py_min(1.0, py_max(-1.0, v));
    }
    if let Some(v) = obj.get("seed").and_then(py_int) {
        out.seed = v;
    }
    // on：除了 False 本身都算开（0 也算）；mirror / fit：必须是 True 本身。
    out.on = obj.get("on").and_then(Value::as_bool) != Some(false);
    out.mirror = obj.get("mirror").and_then(Value::as_bool) == Some(true);
    out.fit = obj.get("fit").and_then(Value::as_bool) == Some(true);
    // graphs: only when the value is a dict (Python `isinstance(tm.get("graphs"), dict)`)
    if let Some(Value::Object(g)) = obj.get("graphs") {
        for key in GRAPH_KEYS {
            if let Some(pts) = g.get(key).and_then(clean_graph) {
                out.graphs.insert(key.to_string(), pts);
            }
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// graphs (1.2.0 tumour.py)
// ---------------------------------------------------------------------------

/// A cleaned graph from a file / another graph: [[u, f], ...] from u = 0 to u = 1 with u never
/// going back (None if it's unusable or flat at 100 %). Python `clean_graph`.
pub fn clean_graph(g: &Value) -> Option<Vec<Pt>> {
    let items = g.as_array()?;
    let mut pts: Vec<Pt> = Vec::with_capacity(items.len());
    for item in items {
        let pair = item.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let u = py_float(&pair[0])?;
        let f = py_float(&pair[1])?;
        pts.push([
            py_min(1.0, py_max(0.0, u)),
            py_min(GRAPH_LIMIT, py_max(-GRAPH_LIMIT, f)),
        ]);
    }
    clean_graph_pts(pts)
}

/// The last steps of [`clean_graph`] on already numeric points (used on file values and on
/// [`sub_graph`] results).
pub fn clean_graph_pts(mut pts: Vec<Pt>) -> Option<Vec<Pt>> {
    if pts.len() < 2 || pts.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return None;
    }
    let last = pts.len() - 1;
    pts[0][0] = 0.0;
    pts[last][0] = 1.0;
    for i in 0..last {
        let a = pts[i][0];
        if pts[i + 1][0] < a {
            pts[i + 1][0] = a;
        }
    }
    if pts.iter().all(|p| (p[1] - 1.0).abs() < 1e-12) {
        return None;
    }
    Some(pts)
}

/// The setting's graph as a function of distances along the line (Python `graph_fn`): None when
/// it has none.
#[derive(Clone, Debug)]
pub struct GraphFn {
    u: Vec<f64>,
    f: Vec<f64>,
}

impl GraphFn {
    /// The multiplier at distance `d` (NumPy `np.interp`).
    pub fn eval(&self, d: f64) -> f64 {
        np_interp(d, &self.u, &self.f)
    }
}

/// Python `graph_fn(tm, key, total)`.
pub fn graph_fn(tm: &Tumour, key: &str, total: f64) -> Option<GraphFn> {
    let g = tm.graphs.get(key)?;
    if g.is_empty() {
        return None;
    }
    Some(GraphFn {
        u: g.iter().map(|p| p[0] * total).collect(),
        f: g.iter().map(|p| p[1]).collect(),
    })
}

/// The part of a graph from u = a to u = b, stretched to 0..1 (Python `sub_graph`).
pub fn sub_graph(g: &[Pt], a: f64, b: f64) -> Vec<Pt> {
    let us: Vec<f64> = g.iter().map(|p| p[0]).collect();
    let fs: Vec<f64> = g.iter().map(|p| p[1]).collect();
    let mut out: Vec<Pt> = Vec::with_capacity(g.len() + 2);
    out.push([0.0, np_interp(a, &us, &fs)]);
    for p in g {
        if a + 1e-12 < p[0] && p[0] < b - 1e-12 {
            out.push([(p[0] - a) / (b - a), p[1]]);
        }
    }
    out.push([1.0, np_interp(b, &us, &fs)]);
    out
}

/// Where the bumps start when the distance follows the graph dg: counting bumps along the range
/// (1 / distance per unit of length), bump i starts where the count reaches i. With `fit` the
/// count is stretched so a whole number of steps fits the range (`even`: an even number). Also
/// returns how much that stretched the distance (1 = not). Python `graph_starts`.
pub fn graph_starts(
    dg: &GraphFn,
    lo: f64,
    hi: f64,
    dist: f64,
    fit: bool,
    even: bool,
) -> (Vec<f64>, f64) {
    if hi - lo < 1e-9 {
        return (vec![lo], 1.0);
    }
    let d = np_linspace(lo, hi, 4097);
    let floor_rate = py_max((hi - lo) / MAX_TUMOURS as f64, 1e-9);
    let rate: Vec<f64> = d
        .iter()
        .map(|&x| 1.0 / py_max(dist * dg.eval(x), floor_rate))
        .collect();
    let mut count: Vec<f64> = Vec::with_capacity(d.len());
    count.push(0.0);
    let mut acc = 0.0;
    for i in 0..d.len() - 1 {
        acc += (rate[i + 1] + rate[i]) / 2.0 * (d[i + 1] - d[i]);
        count.push(acc);
    }
    let total = count[count.len() - 1];
    let mut stretch = 1.0;
    let marks: Vec<f64>;
    if fit {
        let n = if even {
            py_max(2.0, 2.0 * round_half_even(total / 2.0)) as i64
        } else {
            py_max(1.0, round_half_even(total)) as i64
        };
        marks = np_linspace(0.0, total, (n + 1) as usize);
        stretch = total / n as f64;
    } else {
        // (a bump within a thousandth of a step of the end still counts: after a split, the half
        // whose Fit was turned off must still get the one Fit put right on the end)
        let n = py_min((total + 1e-3).floor(), (MAX_TUMOURS - 1) as f64);
        marks = (0..=(n as i64)).map(|i| i as f64).collect();
    }
    let starts: Vec<f64> = marks.iter().map(|&m| np_interp(m, &count, &d)).collect();
    (starts, stretch)
}

// ---------------------------------------------------------------------------
// template / cut / subdivide
// ---------------------------------------------------------------------------

/// 凸起模板：从 (0, 0) 到 (length, 0) 的（沿线的 x, 侧向的 y）点列。
/// `slant` (-1..1, square only): the square's top is narrowed by that much of its length.
pub fn template(shape: TumourShape, length: f64, size: f64, slant: f64) -> Vec<Pt> {
    match shape {
        TumourShape::Triangle => vec![[0.0, 0.0], [length / 2.0, size], [length, 0.0]],
        TumourShape::Square => {
            let m = length / 2.0 * slant;
            vec![[0.0, 0.0], [m, size], [length - m, size], [length, 0.0]]
        }
        TumourShape::Parabola => (0..=16)
            .map(|t| {
                let t = t as f64;
                [
                    length * t / 16.0,
                    size * 4.0 * (t / 16.0) * (1.0 - t / 16.0),
                ]
            })
            .collect(),
        TumourShape::Circle => {
            if size.abs() < 1e-12 {
                vec![[0.0, 0.0], [length, 0.0]]
            } else {
                // 圆形：过顶部画一个圆（每个凸起 10° 一个点就足够了）
                arc_points(
                    &[[0.0, 0.0], [length / 2.0, size], [length, 0.0]],
                    1.0,
                    CIRCLE_STEP,
                )
            }
        }
    }
}

/// 模板在 x_end 处截断的点（截到下一个凸起开始 / 区间结束的地方）。
pub fn cut(pts: &[Pt], x_end: f64) -> Vec<Pt> {
    let last_x = pts.last().map_or(0.0, |p| p[0]);
    if x_end >= last_x - 1e-12 && pts.iter().all(|p| p[0] <= x_end + 1e-12) {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len());
    if let Some(&p0) = pts.first() {
        out.push(p0);
    }
    for seg in pts.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        if b[0] <= x_end + 1e-12 {
            out.push(b);
            continue;
        }
        if a[0] < x_end && x_end < b[0] {
            let u = (x_end - a[0]) / (b[0] - a[0]);
            out.push([x_end, a[1] + (b[1] - a[1]) * u]);
        }
        break;
    }
    out
}

/// 凸起的点按顺序，再在每条边经过 xs（已排序）里某个 x 的地方补一个点，
/// 让随线条弯曲的凸起跟着线条走。按顺序（不是每个 x 一个高度），
/// 所以折回来的圆形凸起能保住整个轮廓。
pub fn subdivide(bump: &[Pt], xs: &[f64]) -> Vec<Pt> {
    let mut out = Vec::with_capacity(bump.len() + xs.len());
    if let Some(&p0) = bump.first() {
        out.push(p0);
    }
    for seg in bump.windows(2) {
        let (xa, ya) = (seg[0][0], seg[0][1]);
        let (xb, yb) = (seg[1][0], seg[1][1]);
        if (xb - xa).abs() > 1e-12 {
            let lo = bisect_right(xs, xa.min(xb) + 1e-12);
            let hi = bisect_left(xs, xa.max(xb) - 1e-12);
            if hi > lo {
                let mid = &xs[lo..hi];
                if xb > xa {
                    for &x in mid {
                        out.push([x, ya + (yb - ya) * (x - xa) / (xb - xa)]);
                    }
                } else {
                    for &x in mid.iter().rev() {
                        out.push([x, ya + (yb - ya) * (x - xa) / (xb - xa)]);
                    }
                }
            }
        }
        out.push([xb, yb]);
    }
    out
}

// ---------------------------------------------------------------------------
// Walk
// ---------------------------------------------------------------------------

/// 一条按长度测量的路径（屏幕单位）：任意距离处的点与方向（tumour.py Walk）。
#[derive(Clone, Debug)]
pub struct Walk {
    pub pts: Vec<Pt>,
    pub cum: Vec<f64>,
    pub total: f64,
    pub closed: bool,
}

impl Walk {
    pub fn new(pts: Vec<Pt>) -> Self {
        let mut cum = Vec::with_capacity(pts.len());
        cum.push(0.0);
        for seg in pts.windows(2) {
            let prev = cum.last().copied().unwrap_or(0.0);
            cum.push(prev + dist(seg[0], seg[1]));
        }
        let total = cum.last().copied().unwrap_or(0.0);
        let closed = pts.len() > 2
            && match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => dist(*a, *b) < 1e-9,
                _ => false,
            };
        Self {
            pts,
            cum,
            total,
            closed,
        }
    }

    /// 距离 d 落在第几段（0 起）。
    pub fn seg(&self, d: f64) -> usize {
        let i = bisect_right(&self.cum, d).saturating_sub(1);
        i.min(self.pts.len().saturating_sub(2))
    }

    /// 距离 d 处的点。
    pub fn at(&self, d: f64) -> Pt {
        if self.pts.len() < 2 {
            return self.pts.first().copied().unwrap_or([0.0, 0.0]);
        }
        let i = self.seg(d);
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let n = self.cum[i + 1] - self.cum[i];
        let u = if n == 0.0 {
            0.0
        } else {
            ((d - self.cum[i]) / n).clamp(0.0, 1.0)
        };
        [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]
    }

    /// 第 i 段的单位方向（零长段时为 (1, 0)）。
    fn dir_at(&self, i: usize) -> Pt {
        let (Some(a), Some(b)) = (self.pts.get(i), self.pts.get(i + 1)) else {
            return [1.0, 0.0];
        };
        let n = dist(*a, *b);
        if n == 0.0 {
            [1.0, 0.0]
        } else {
            [(b[0] - a[0]) / n, (b[1] - a[1]) / n]
        }
    }

    /// 距离 d 处的单位方向（在拐角处：两条边的中间方向）。
    pub fn direction(&self, d: f64) -> Pt {
        let i = self.seg(d);
        let mut v = self.dir_at(i);
        let mut before: Option<usize> = None;
        if (d - self.cum.get(i).copied().unwrap_or(0.0)).abs() < 1e-12 {
            before = if i > 0 {
                Some(i - 1)
            } else if self.closed {
                Some(self.pts.len().saturating_sub(2))
            } else {
                None
            };
        }
        if self.closed && d >= self.total - 1e-12 {
            // 闭合环的起点 / 终点也是个拐角：末边与首边的中间方向
            v = self.dir_at(0);
            before = Some(self.pts.len().saturating_sub(2));
        }
        if let Some(b) = before {
            let w = self.dir_at(b);
            let s = [v[0] + w[0], v[1] + w[1]];
            let n = hypot2(s[0], s[1]);
            if n > 1e-9 {
                return [s[0] / n, s[1] / n];
            }
        }
        v
    }

    /// 一整组距离上的 at（数组版；Rust 用标量循环等价实现）。
    pub fn at_many(&self, d: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let mut xs = Vec::with_capacity(d.len());
        let mut ys = Vec::with_capacity(d.len());
        for &di in d {
            let p = self.at(di);
            xs.push(p[0]);
            ys.push(p[1]);
        }
        (xs, ys)
    }

    /// 一整组距离上的 direction（数组版；Rust 用标量循环等价实现）。
    pub fn directions(&self, d: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let mut xs = Vec::with_capacity(d.len());
        let mut ys = Vec::with_capacity(d.len());
        for &di in d {
            let p = self.direction(di);
            xs.push(p[0]);
            ys.push(p[1]);
        }
        (xs, ys)
    }
}

// ---------------------------------------------------------------------------
// Out
// ---------------------------------------------------------------------------

/// 肿瘤路径的点按顺序：按原样加进来的点，以及其点稍后作为一整块数组交上来的块（fill）。
/// 对照 NumPy 版的两阶段结构：先记录块的位置，最后统一填充。
#[derive(Default)]
pub struct Out {
    lit: Vec<(usize, Pt)>,
    blocks: Vec<(usize, usize)>,
    arrays: Vec<Vec<Pt>>,
    /// 目前已占用的点数。
    pub n: usize,
}

impl Out {
    pub fn new() -> Self {
        Self::default()
    }

    /// 一组点按原样加入。
    pub fn add(&mut self, pts: &[Pt]) {
        for p in pts {
            self.lit.push((self.n, *p));
            self.n += 1;
        }
    }

    /// 登记一个稍后填充的块（只数点数，数组随 fill 来）。
    pub fn block(&mut self, n: usize) {
        self.blocks.push((self.n, n));
        self.n += n;
    }

    /// 登记一个块，点已经到手（尖刺分支的原地点阵）。
    pub fn block_array(&mut self, pts: &[Pt]) {
        self.arrays.push(pts.to_vec());
        self.blocks.push((self.n, pts.len()));
        self.n += pts.len();
    }

    /// 块的点：一个整块数组（Python Out.fill）。
    pub fn fill(&mut self, pts: Vec<Pt>) {
        self.arrays.push(pts);
    }

    /// 按登记顺序拼出全部点。
    pub fn result(&self) -> Vec<Pt> {
        let mut res = vec![[0.0, 0.0]; self.n];
        for &(at, p) in &self.lit {
            if let Some(slot) = res.get_mut(at) {
                *slot = p;
            }
        }
        let all: Vec<Pt> = self.arrays.iter().flatten().copied().collect();
        let mut next = all.iter();
        for &(at, n) in &self.blocks {
            for j in 0..n {
                if let (Some(slot), Some(p)) = (res.get_mut(at + j), next.next()) {
                    *slot = *p;
                }
            }
        }
        res
    }
}

// ---------------------------------------------------------------------------
// tumour_path
// ---------------------------------------------------------------------------

/// 去掉相邻重复点后的最终点列（Python 结尾的 keep 过滤与 x * k）。
fn finish(res: Vec<Pt>, k: f64) -> Vec<Pt> {
    let mut out = Vec::with_capacity(res.len());
    let mut prev: Option<Pt> = None;
    for p in res {
        let keep = match prev {
            None => true,
            Some(q) => p[0] != q[0] || p[1] != q[1],
        };
        if keep {
            out.push([p[0] * k, p[1]]);
        }
        prev = Some(p);
    }
    out
}

/// 带肿瘤的路径（beat, pitch 点列）。Python `tumour_path` 的逐行移植。
pub fn tumour_path(path: &[Pt], tm: &Tumour) -> Vec<Pt> {
    let k = tm.k;
    let mut pts: Vec<Pt> = Vec::new();
    for p in path {
        let q = [p[0] / k, p[1]];
        if pts.last() != Some(&q) {
            pts.push(q);
        }
    }
    let size = tm.size;
    if pts.len() < 2 || size.abs() < 1e-12 {
        return path.to_vec();
    }
    let w = Walk::new(pts);
    if w.total < 1e-12 {
        return path.to_vec();
    }
    let length = py_max(0.0, tm.length / k);
    let (lo, hi, is_loop, starts, _) = bump_starts(&w, tm);
    if starts.is_empty() {
        return path.to_vec();
    }
    let zg = graph_fn(tm, "size", w.total);
    let lg = graph_fn(tm, "length", w.total);
    let rg = graph_fn(tm, "rot", w.total);
    let sg = graph_fn(tm, "slant", w.total);
    let rot = tm.rot.to_radians();
    let lean = rot.abs() > 1e-12;
    let (sin, cos) = (rot.sin(), rot.cos());
    let mut rnd = crate::pyrandom::PyRandom::new(tm.seed);
    let flip = if tm.mirror { -1.0 } else { 1.0 };
    let mut sides: Vec<f64> = Vec::with_capacity(starts.len());
    for i in 0..starts.len() {
        let side = match tm.side {
            TumourSide::Left => Some(1.0),
            TumourSide::Right => Some(-1.0),
            TumourSide::Alt => Some(if i % 2 == 0 { 1.0 } else { -1.0 }),
            TumourSide::Random => None,
        };
        sides.push(
            flip * match side {
                Some(v) => v,
                None => rnd.choice2(),
            },
        );
    }
    let fit_loop = is_loop && tm.fit;
    if fit_loop {
        let first = sides[0];
        let last = sides.len() - 1;
        sides[last] = first; // 最后一个就是第一个
    }

    // 路径自身严格落在距离 d0 与 d1 之间的点。
    let base = |d0: f64, d1: f64| -> Vec<Pt> {
        let a = bisect_right(&w.cum, d0 + 1e-9);
        let b = bisect_left(&w.cum, d1 - 1e-9);
        if b > a {
            w.pts[a..b].to_vec()
        } else {
            Vec::new()
        }
    };

    let ease = tm.ease / k;

    // 距离 d 处一个凸起占全尺寸的比例（缓动时区间两端更小）。
    let grow = |d: f64| -> f64 {
        if ease < 1e-12 {
            return 1.0;
        }
        py_max(0.0, py_min(py_min(1.0, (d - lo) / ease), (hi - d) / ease))
    };

    // 凸起是否落在缓动区（需要额外加点让变化平滑）。
    let needs_easing = |bump: &[Pt], s: f64| {
        ease >= 1e-12 && (s < lo + ease || s + bump.last().map_or(0.0, |p| p[0]) > hi - ease)
    };

    // 缓动后的凸起点（在缓动的地方补点，让变化平滑）。
    let eased = |bump: &[Pt], s: f64| -> Vec<Pt> {
        let last_x = bump.last().map_or(0.0, |p| p[0]);
        if ease < 1e-12 || !(s < lo + ease || s + last_x > hi - ease) {
            return bump.to_vec();
        }
        let step = ease / 16.0;
        let mut out: Vec<Pt> = bump.first().copied().into_iter().collect();
        for seg in bump.windows(2) {
            let (xa, ya) = (seg[0][0], seg[0][1]);
            let (xb, yb) = (seg[1][0], seg[1][1]);
            let mut us: Vec<f64> = Vec::new();
            let n = (((xb - xa).abs() / step) as usize).min(64);
            for j in 1..=n {
                us.push(j as f64 / (n + 1) as f64);
            }
            if (xb - xa).abs() > 1e-12 {
                for m in [
                    (lo + ease - s - xa) / (xb - xa),
                    (hi - ease - s - xa) / (xb - xa),
                ] {
                    if m > 1e-9 && m < 1.0 - 1e-9 {
                        us.push(m);
                    }
                }
            }
            us.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            us.dedup();
            for u in us {
                out.push([xa + (xb - xa) * u, ya + (yb - ya) * u]);
            }
            out.push([xb, yb]);
        }
        // 折回越过区间端点的圆凸起在端点处是平的：夹在区间内，而不是沿线条越过去
        out.iter()
            .map(|p| [py_min(py_max(p[0], lo - s), hi - s), p[1] * grow(s + p[0])])
            .collect()
    };

    // 需要逐个算的点（每个凸起没几个）按原样加入；凸起上大量的点先记成块，最后按整组数组算。
    let mut out = Out::new();
    let first_start = starts[0];
    if first_start > 1e-9 {
        let mut head = vec![w.pts[0]];
        head.extend(base(0.0, first_start));
        out.add(&head);
    }
    if length < 1e-12 {
        // 尖刺：每个凸起只是一个被推向一侧的点，从一个直接到下一个
        if out.n == 0 && !fit_loop {
            // （绕闭合环时只在尖刺之间走，首 = 尾）
            let p0 = w.pts[0];
            out.add(&[p0]);
        }
        let (x, y) = w.at_many(&starts);
        let (ux, uy) = w.directions(&starts);
        let mut block_pts = Vec::with_capacity(starts.len());
        for i in 0..starts.len() {
            let mut h = size * grow(starts[i]);
            if let Some(zg) = &zg {
                h *= zg.eval(starts[i]);
            }
            if lean {
                // tilted: part of the push goes along the line
                let nx = -uy[i];
                let ny = ux[i];
                let (sn, cs) = match &rg {
                    Some(rg) => {
                        let r = rot * rg.eval(starts[i]);
                        (r.sin(), r.cos())
                    }
                    None => (sin, cos),
                };
                block_pts.push([
                    x[i] + (nx * cs * sides[i] + ux[i] * sn) * h,
                    y[i] + (ny * cs * sides[i] + uy[i] * sn) * h,
                ]);
            } else {
                let h = h * sides[i];
                block_pts.push([x[i] - uy[i] * h, y[i] + ux[i] * h]);
            }
        }
        out.block_array(&block_pts);
        if !fit_loop {
            let mut tail = base(starts[starts.len() - 1], w.total);
            tail.push(w.pts[w.pts.len() - 1]);
            out.add(&tail);
        }
    } else {
        let slant = tm.slant;
        let shape = if lg.is_none() && sg.is_none() {
            Some(template(tm.shape, length, size, slant))
        } else {
            None
        };
        let mut bump_pts: Vec<Pt> = Vec::new();
        let mut par_simple: Vec<[f64; 9]> = Vec::new();
        let mut par_wrap: Vec<[f64; 4]> = Vec::new();
        let mut any_bump = false;
        for i in 0..starts.len() {
            let s = starts[i];
            let side = sides[i];
            let room = if i + 1 < starts.len() {
                py_min(hi, starts[i + 1])
            } else {
                hi
            };
            let li = match &lg {
                None => length,
                Some(lg) => py_max(0.0, length * lg.eval(s)),
            };
            let e = py_min(s + li, room);
            let nxt = if i + 1 < starts.len() {
                starts[i + 1]
            } else {
                w.total
            };
            if e - s < 1e-9 {
                // 没地方了（它正好从区间终点开始），或者长度为 0
                let at = w.at(s);
                out.add(&[at]);
                let tail = base(e, nxt);
                out.add(&tail);
                continue;
            }
            // 只在下个凸起或区间终点挡路的地方截断：比自身一半长度还高的圆凸起会鼓过自己的
            // 端点，有地方时那部分要留着
            let bump = match &shape {
                Some(tpl) => cut(tpl, room - s),
                None => {
                    let sl = match &sg {
                        None => slant,
                        Some(sg) => py_min(1.0, py_max(-1.0, slant * sg.eval(s))),
                    };
                    cut(&template(tm.shape, li, size, sl), room - s)
                }
            };
            let r = match &rg {
                None => rot,
                Some(rg) => rot * rg.eval(s + li / 2.0),
            };
            let (sn, cs) = match &rg {
                None => (sin, cos),
                Some(_) => (r.sin(), r.cos()),
            };
            let bump = if zg.is_some() && tm.wrap == TumourWrap::Simple {
                // points for the size to change along
                let xs: Vec<f64> = (0..=16).map(|j| (e - s) * j as f64 / 16.0).collect();
                subdivide(&bump, &xs)
            } else {
                bump
            };
            if tm.wrap == TumourWrap::Simple {
                // 从起点到终点的直线段上
                let a = w.at(s);
                let b = w.at(e);
                let (mut ux, mut uy) = (b[0] - a[0], b[1] - a[1]);
                let n = hypot2(ux, uy);
                if n > 1e-12 {
                    ux /= n;
                    uy /= n;
                } else {
                    let d = w.direction(s);
                    ux = d[0];
                    uy = d[1];
                }
                let stretch = if e - s > 1e-12 { n / (e - s) } else { 1.0 };
                let bump = eased(&bump, s);
                for p in &bump {
                    bump_pts.push(*p);
                    par_simple.push([a[0], a[1], ux, uy, stretch, side, s, sn, cs]);
                }
                any_bump = true;
                out.block(bump.len());
            } else {
                // 随线条弯曲：每个点从它在线上所在的位置向侧面偏移，线条拐弯和长边处补点，
                // 让凸起跟着线条的形状走
                let x0 = s + bump.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                let x1 = s + bump.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
                let mut xs: Vec<f64> = Vec::new();
                let a = bisect_right(&w.cum, x0);
                let b = bisect_left(&w.cum, x1);
                if b > a {
                    for c in &w.cum[a..b] {
                        xs.push(c - s);
                    }
                }
                if tm.shape != TumourShape::Circle {
                    // （圆的轮廓本来就已经每 10° 一个点）
                    for j in 0..=16 {
                        xs.push((e - s) * j as f64 / 16.0);
                    }
                }
                let eb = eased(&bump, s);
                if needs_easing(&bump, s) {
                    // （缓动加出来的点，让凸起平滑地长大）
                    for p in &eb {
                        xs.push(p[0]);
                    }
                }
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                xs.dedup();
                let bump = subdivide(&bump, &xs);
                for p in &bump {
                    bump_pts.push(*p);
                    par_wrap.push([s, side, sn, cs]);
                }
                any_bump = true;
                out.block(bump.len());
            }
            if e >= hi - 1e-12 && nxt > e + 1e-12 {
                // cut off by the range's end: down onto the line there
                out.add(&[w.at(e)]);
            }
            let tail = base(e, nxt);
            out.add(&tail);
        }
        out.add(&[w.pts[w.pts.len() - 1]]);
        if any_bump {
            if tm.wrap == TumourWrap::Simple {
                let mut fill = Vec::with_capacity(bump_pts.len());
                for (p, q) in bump_pts.iter().zip(par_simple.iter()) {
                    let mut x = p[0] * q[4];
                    let mut y = p[1];
                    if let Some(zg) = &zg {
                        y *= zg.eval(q[6] + p[0]);
                    }
                    if lean {
                        let (nx, ny) = (x + y * q[7], y * q[8]);
                        x = nx;
                        y = ny;
                    }
                    fill.push([
                        q[0] + q[2] * x - q[3] * y * q[5],
                        q[1] + q[3] * x + q[2] * y * q[5],
                    ]);
                }
                out.fill(fill);
            } else {
                let mut ys: Vec<f64> = Vec::with_capacity(bump_pts.len());
                let mut ds: Vec<f64> = Vec::with_capacity(bump_pts.len());
                for (p, q) in bump_pts.iter().zip(par_wrap.iter()) {
                    let s = q[0];
                    let mut x = p[0];
                    let mut y = p[1] * grow(s + p[0]);
                    if let Some(zg) = &zg {
                        y *= zg.eval(s + p[0]);
                    }
                    if lean {
                        let (nx, ny) = (x + y * q[2], y * q[3]);
                        x = nx;
                        y = ny;
                    }
                    let mut d = s + x;
                    if w.closed {
                        // 圆凸起鼓过闭合环的起点 / 终点：绕着环走
                        d = py_mod(d, w.total);
                    }
                    ys.push(y);
                    ds.push(d);
                }
                let dcs: Vec<f64> = ds.iter().map(|&d| d.clamp(0.0, w.total)).collect();
                let (pxs, pys) = w.at_many(&dcs);
                let (uxs, uys) = w.directions(&dcs);
                let mut fill = Vec::with_capacity(bump_pts.len());
                for (i, q) in par_wrap.iter().enumerate() {
                    let side = q[1];
                    let y = ys[i];
                    let over = ds[i] - dcs[i]; // 越出线条起点 / 终点：顺着那里的方向直着走
                    let nx = -uys[i];
                    let ny = uxs[i];
                    let px = pxs[i] + ny * over;
                    let py = pys[i] - nx * over;
                    fill.push([px + nx * y * side, py + ny * y * side]);
                }
                out.fill(fill);
            }
        }
    }
    finish(out.result(), k)
}

// ---------------------------------------------------------------------------
// split_tumour
// ---------------------------------------------------------------------------

/// The path scaled by k (consecutive repeats dropped); None when there are fewer than two points
/// (Python `_walk`).
fn walk_of(path: &[Pt], k: f64) -> Option<Walk> {
    let mut pts: Vec<Pt> = Vec::new();
    for p in path {
        let q = [p[0] / k, p[1]];
        if pts.last() != Some(&q) {
            pts.push(q);
        }
    }
    if pts.len() > 1 {
        Some(Walk::new(pts))
    } else {
        None
    }
}

/// Where the bumps start along the Walk w (on-screen units): (range start, range end, whether it's
/// a closed loop with bumps all the way round, the starts, how much Fit stretched the distance
/// (1 = not at all)). Python `bump_starts`.
pub fn bump_starts(w: &Walk, tm: &Tumour) -> (f64, f64, bool, Vec<f64>, f64) {
    let k = tm.k;
    let lo = py_min(tm.start, tm.end) * w.total;
    let hi = py_max(tm.start, tm.end) * w.total;
    let dist = py_max(py_max(tm.dist / k, (hi - lo) / MAX_TUMOURS as f64), 1e-9);
    // a closed loop (e.g. a full circle) with bumps all the way round: its end is its start again
    let loop_ = w.closed && lo < 1e-9 && hi > w.total - 1e-9;
    let dg = graph_fn(tm, "dist", w.total);
    let mut stretch = 1.0;
    let starts: Vec<f64>;
    if let Some(dg) = &dg {
        let (s, st) = graph_starts(
            dg,
            lo,
            hi,
            dist,
            tm.fit,
            loop_ && tm.side == TumourSide::Alt,
        );
        starts = s;
        stretch = st;
    } else if tm.fit && hi - lo > 1e-9 {
        // a whole number of steps fits the range, so the last one lands right on its end; round a
        // loop with alternating sides, an even number, so the sides keep alternating where it meets up
        let r = (hi - lo) / dist;
        let mut n = py_max(1.0, round_half_even(r)) as i64;
        if loop_ && tm.side == TumourSide::Alt {
            n = py_max(2.0, 2.0 * round_half_even(r / 2.0)) as i64;
        }
        stretch = (hi - lo) / n as f64 / dist;
        starts = (0..=n)
            .map(|i| lo + (hi - lo) * i as f64 / n as f64)
            .collect();
    } else {
        let mut v: Vec<f64> = Vec::new();
        let mut s = lo;
        while s <= hi + 1e-9 && v.len() < MAX_TUMOURS {
            v.push(py_min(s, hi));
            s += dist;
        }
        starts = v;
    }
    (lo, hi, loop_, starts, stretch)
}

/// Tumour settings for the two halves of a line cut in two (left / right: their (beat, pitch) points,
/// both with the cut point), so the bumps stay where they were: each half gets its own part of the
/// range and graphs, Fit is turned off (keeping the distance it had worked out) and the right half
/// starts at the first bump after the cut (a bump across the cut is cut off there). Random sides are
/// picked again; Lead in works at each half's ends (tumour.split_tumour).
pub fn split_tumour(tm: &Tumour, left: &[Pt], right: &[Pt]) -> (Option<Tumour>, Option<Tumour>) {
    let mut combined: Vec<Pt> = left.to_vec();
    combined.extend_from_slice(right.get(1..).unwrap_or(&[]));
    let copy = tm.clone();
    let (Some(w), Some(wl)) = (walk_of(&combined, tm.k), walk_of(left, tm.k)) else {
        return (Some(copy.clone()), Some(tm.clone()));
    };
    if w.total - wl.total < 1e-9 * w.total {
        return (Some(copy), Some(tm.clone()));
    }
    let (lo, hi, _, starts, stretch) = bump_starts(&w, tm);
    let cut = wl.total;
    let mut base = copy;
    base.fit = false;
    base.dist = tm.dist * stretch;
    base.start = 0.0;
    base.end = 1.0;

    let half = |a: f64, b: f64, r0: f64, r1: f64| -> Tumour {
        let mut out = base.clone();
        let length = b - a;
        out.start = py_min(1.0, py_max(0.0, (r0 - a) / length));
        out.end = py_min(1.0, py_max(0.0, (r1 - a) / length));
        if r1 - r0 < 1e-9 {
            // (no bumps left on this half)
            out.on = false;
        }
        let mut graphs: BTreeMap<String, Vec<Pt>> = BTreeMap::new();
        for (key, g) in &tm.graphs {
            if let Some(g2) = clean_graph_pts(sub_graph(g, a / w.total, b / w.total)) {
                graphs.insert(key.clone(), g2);
            }
        }
        out.graphs = graphs;
        out
    };
    let first = half(0.0, cut, lo, py_min(hi, cut));
    let nxt = starts
        .iter()
        .enumerate()
        .find(|&(_, &s)| s >= cut - 1e-9)
        .map(|(i, &s)| (i, s));
    let mut second = match nxt {
        Some((_, s)) => half(cut, w.total, s, hi),
        None => half(cut, w.total, hi, hi),
    };
    if let Some((i, _)) = nxt
        && tm.side == TumourSide::Alt
        && i % 2 == 1
    {
        // (alternating: it has to start on the other side)
        second.mirror = !second.mirror;
    }
    (Some(first), Some(second))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(pairs: &[(f64, f64)]) -> Vec<Pt> {
        pairs.iter().map(|&(u, f)| [u, f]).collect()
    }

    #[test]
    fn clean_graph_clamps_and_drops_flat() {
        let got = clean_graph(&serde_json::json!([[2.0, -50.0], [0.5, 100.0]])).expect("graph");
        assert_eq!(got[0], [0.0, -10.0]);
        assert_eq!(got[1], [1.0, 10.0]);
        // the ends snap to 0 / 1; u never goes back
        let got = clean_graph(&serde_json::json!([[0.5, 1.5], [0.0, 2.0]])).expect("graph");
        assert_eq!(got[0], [0.0, 1.5]);
        assert_eq!(got[1], [1.0, 2.0]);
        // flat at 100 %: unusable
        assert!(clean_graph(&serde_json::json!([[0.0, 1.0], [1.0, 1.0]])).is_none());
        // fewer than two points / bad pairs
        assert!(clean_graph(&serde_json::json!([[0.0, 0.0]])).is_none());
        assert!(clean_graph(&serde_json::json!([[0.0, 0.0], [1.0]])).is_none());
        assert!(clean_graph(&serde_json::json!("nope")).is_none());
    }

    #[test]
    fn graph_fn_interpolates_and_clamps_at_the_ends() {
        let tm = Tumour {
            graphs: BTreeMap::from([(
                "size".to_string(),
                graph(&[(0.0, 0.5), (0.5, 2.0), (1.0, 1.0)]),
            )]),
            ..Tumour::default()
        };
        let g = graph_fn(&tm, "size", 2.0).expect("graph");
        assert!((g.eval(0.0) - 0.5).abs() < 1e-12);
        assert!((g.eval(0.5) - 1.25).abs() < 1e-12);
        assert!((g.eval(1.0) - 2.0).abs() < 1e-12);
        // outside the graph: the end values
        assert!((g.eval(-3.0) - 0.5).abs() < 1e-12);
        assert!((g.eval(9.0) - 1.0).abs() < 1e-12);
        assert!(graph_fn(&tm, "rot", 2.0).is_none());
    }

    #[test]
    fn sub_graph_keeps_the_values_at_the_cut() {
        let g = graph(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]);
        let part = sub_graph(&g, 0.25, 0.75);
        assert_eq!(part[0], [0.0, 1.0]);
        assert_eq!(part[1], [0.5, 2.0]);
        assert_eq!(part[2], [1.0, 1.5]);
    }

    #[test]
    fn graph_starts_stretch_the_count_for_fit() {
        let dg = GraphFn {
            u: vec![0.0, 1.0],
            f: vec![1.0, 1.0],
        };
        let (starts, stretch) = graph_starts(&dg, 0.0, 2.0, 0.75, false, false);
        // 1 / 0.75 per unit over 2 units: floor(2 / 0.75 + 1e-3) + 1 = 3 marks
        assert_eq!(starts.len(), 3);
        assert!((starts[0] - 0.0).abs() < 1e-9);
        assert!((starts[1] - 0.75).abs() < 1e-6);
        assert!((starts[2] - 1.5).abs() < 1e-6);
        assert!((stretch - 1.0).abs() < 1e-12);
        let (starts, stretch) = graph_starts(&dg, 0.0, 2.0, 0.9, true, false);
        assert_eq!(starts.len(), 3); // n = 2 steps, so 3 marks
        assert!((starts[2] - 2.0).abs() < 1e-9);
        assert!((stretch - 2.0 / 0.9 / 2.0).abs() < 1e-9);
    }

    #[test]
    fn split_tumour_keeps_the_bumps_where_they_were() {
        let tm = Tumour {
            fit: true,
            dist: 1.0,
            length: 0.5,
            k: 1.0,
            ..Tumour::default()
        };
        let path = vec![[0.0, 60.0], [4.0, 60.0]];
        let left = vec![[0.0, 60.0], [2.0, 60.0]];
        let right = vec![[2.0, 60.0], [4.0, 60.0]];
        let (l, r) = split_tumour(&tm, &left, &right);
        let (l, r) = (l.expect("left"), r.expect("right"));
        assert!(!l.fit && !r.fit);
        assert!(l.on && r.on);
        assert!((l.dist - 1.0).abs() < 1e-12);
        assert!((r.dist - 1.0).abs() < 1e-12);
        assert!((l.start - 0.0).abs() < 1e-12 && (l.end - 1.0).abs() < 1e-12);
        assert!((r.start - 0.0).abs() < 1e-12 && (r.end - 1.0).abs() < 1e-12);
        // Splitting a full line at its middle: each half's bumps are where the whole line's were.
        let whole = tumour_path(&path, &tm);
        let half = |p: &[Pt], t: &Tumour| tumour_path(p, t);
        for x in [0.0, 1.0, 2.0] {
            assert!(
                half(&left, &l).iter().any(|p| (p[0] - x).abs() < 1e-9),
                "left half should keep the bump at {x}"
            );
        }
        for x in [2.0, 3.0, 4.0] {
            assert!(
                half(&right, &r).iter().any(|p| (p[0] - x).abs() < 1e-9),
                "right half should keep the bump at {x}"
            );
        }
        let bumps: Vec<f64> = whole
            .iter()
            .filter(|p| p[1].abs() > 1e-9)
            .map(|p| p[0])
            .collect();
        assert!(!bumps.is_empty());
    }

    #[test]
    fn split_tumour_slices_graphs() {
        let tm = Tumour {
            graphs: BTreeMap::from([(
                "size".to_string(),
                graph(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]),
            )]),
            fit: true,
            k: 1.0,
            ..Tumour::default()
        };
        let left = vec![[0.0, 60.0], [2.0, 60.0]];
        let right = vec![[2.0, 60.0], [4.0, 60.0]];
        let (l, r) = split_tumour(&tm, &left, &right);
        let l = l.expect("left");
        let r = r.expect("right");
        let lg = l.graphs.get("size").expect("left graph");
        assert_eq!(lg[0], [0.0, 0.0]);
        assert_eq!(lg[lg.len() - 1], [1.0, 2.0]);
        let rg = r.graphs.get("size").expect("right graph");
        assert_eq!(rg[0], [0.0, 2.0]);
        assert_eq!(rg[rg.len() - 1], [1.0, 1.0]);
    }

    #[test]
    fn slant_narrows_the_square_top() {
        let sq = template(TumourShape::Square, 1.0, 2.0, 1.0);
        assert_eq!(sq[1], [0.5, 2.0]);
        assert_eq!(sq[2], [0.5, 2.0]);
        let sq = template(TumourShape::Square, 1.0, 2.0, -0.5);
        assert_eq!(sq[1], [-0.25, 2.0]);
        assert_eq!(sq[2], [1.25, 2.0]);
    }
}
