//! 线条上的肿瘤（凸起）：把 Python `notes/tumour.py` 逐函数移植。
//!
//! 线条、折线、自由笔、曲线或弧都能带肿瘤设置 `sh["tumour"]`：线条的点仍按原样绘制（仍可拖动），
//! 但它成形的路径每隔 `dist` 长出一个凸起，每个 `length` 长、`size` 个 key 高，朝向由 `side` 决定。
//! `ease` > 0 时凸起在区间两端 `ease` 内从无到有地长出来，线条平滑地进入凸起，而不是突然竖起。
//! `fit` 把距离稍微拉伸，让整数个凸起恰好铺满区间；绕闭合环（比如整圆）一周时首尾正好接上。
//! 一切按屏幕上的样子计算：`k` = 上次修改设置时屏幕上一个 key 对应多少 beat（同 arc），
//! 所以 size 的单位是 key，length / dist 是沿线条的 beat。length 0 = 尖刺：每个凸起只是
//! 一个被推向一侧的点，线条从一个尖刺直着折向下一个。

use crate::arc::arc_points;
use crate::shape::{Tumour, TumourShape, TumourSide, TumourWrap};
use crate::{dist, hypot2, round_half_even, Pt};
use serde_json::{Map, Value};

/// 最多多少个凸起（tumour.py MAX_TUMOURS）。
pub const MAX_TUMOURS: usize = 20000;

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
        if (r < 0.0) != (y < 0.0) {
            r + y
        } else {
            r
        }
    } else {
        0.0_f64.copysign(y)
    }
}

/// CPython `max(a, b)`（并列时保留先出现的 a）。
fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// CPython `min(a, b)`（并列时保留先出现的 a）。
fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
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
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
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
    if let Some(v) = obj.get("seed").and_then(py_int) {
        out.seed = v;
    }
    // on：除了 False 本身都算开（0 也算）；mirror / fit：必须是 True 本身。
    out.on = obj.get("on").and_then(Value::as_bool) != Some(false);
    out.mirror = obj.get("mirror").and_then(Value::as_bool) == Some(true);
    out.fit = obj.get("fit").and_then(Value::as_bool) == Some(true);
    Some(out)
}

// ---------------------------------------------------------------------------
// template / cut / subdivide
// ---------------------------------------------------------------------------

/// 凸起模板：从 (0, 0) 到 (length, 0) 的（沿线的 x, 侧向的 y）点列。
pub fn template(shape: TumourShape, length: f64, size: f64) -> Vec<Pt> {
    match shape {
        TumourShape::Triangle => vec![[0.0, 0.0], [length / 2.0, size], [length, 0.0]],
        TumourShape::Square => vec![[0.0, 0.0], [0.0, size], [length, size], [length, 0.0]],
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
    let lo = py_min(tm.start, tm.end) * w.total;
    let hi = py_max(tm.start, tm.end) * w.total;
    let length = py_max(0.0, tm.length / k);
    let dist = py_max(py_max(tm.dist / k, (hi - lo) / MAX_TUMOURS as f64), 1e-9);
    // 闭合环（比如整圆）且凸起绕一整圈：终点就是起点
    let is_loop = w.closed && lo < 1e-9 && hi > w.total - 1e-9;
    let mut starts: Vec<f64> = Vec::new();
    if tm.fit && hi - lo > 1e-9 {
        // 整数个凸起恰好铺满区间，最后一个正好落在区间终点；绕闭合环且左右交替时取偶数个，
        // 这样接上时左右还在交替
        let r = (hi - lo) / dist;
        let mut n = py_max(1.0, round_half_even(r)) as i64;
        if is_loop && tm.side == TumourSide::Alt {
            n = py_max(2.0, 2.0 * round_half_even(r / 2.0)) as i64;
        }
        let span = hi - lo;
        for i in 0..=n {
            starts.push(lo + span * i as f64 / n as f64);
        }
    } else {
        let mut s = lo;
        while s <= hi + 1e-9 && starts.len() < MAX_TUMOURS {
            starts.push(py_min(s, hi));
            s += dist;
        }
    }
    if starts.is_empty() {
        return path.to_vec();
    }
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
            let h = size * sides[i] * grow(starts[i]);
            block_pts.push([x[i] - uy[i] * h, y[i] + ux[i] * h]);
        }
        out.block_array(&block_pts);
        if !fit_loop {
            let mut tail = base(starts[starts.len() - 1], w.total);
            tail.push(w.pts[w.pts.len() - 1]);
            out.add(&tail);
        }
    } else {
        let shape = template(tm.shape, length, size);
        let mut bump_pts: Vec<Pt> = Vec::new();
        let mut par_simple: Vec<[f64; 6]> = Vec::new();
        let mut par_wrap: Vec<[f64; 2]> = Vec::new();
        let mut any_bump = false;
        for i in 0..starts.len() {
            let s = starts[i];
            let side = sides[i];
            let room = if i + 1 < starts.len() {
                py_min(hi, starts[i + 1])
            } else {
                hi
            };
            let e = py_min(s + length, room);
            // 只在下个凸起或区间终点挡路的地方截断：比自身一半长度还高的圆凸起会鼓过自己的
            // 端点，有地方时那部分要留着
            let bump = cut(&shape, room - s);
            if e - s < 1e-9 {
                // 没地方了（它正好从区间终点开始）
                let at = w.at(s);
                out.add(&[at]);
            } else if tm.wrap == TumourWrap::Simple {
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
                    par_simple.push([a[0], a[1], ux, uy, stretch, side]);
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
                    par_wrap.push([s, side]);
                }
                any_bump = true;
                out.block(bump.len());
            }
            let nxt = if i + 1 < starts.len() {
                starts[i + 1]
            } else {
                w.total
            };
            let tail = base(e, nxt);
            out.add(&tail);
        }
        out.add(&[w.pts[w.pts.len() - 1]]);
        if any_bump {
            if tm.wrap == TumourWrap::Simple {
                let mut fill = Vec::with_capacity(bump_pts.len());
                for (p, q) in bump_pts.iter().zip(par_simple.iter()) {
                    let x = p[0] * q[4];
                    let y = p[1];
                    fill.push([
                        q[0] + q[2] * x - q[3] * y * q[5],
                        q[1] + q[3] * x + q[2] * y * q[5],
                    ]);
                }
                out.fill(fill);
            } else {
                let mut ds = Vec::with_capacity(bump_pts.len());
                for (p, q) in bump_pts.iter().zip(par_wrap.iter()) {
                    let mut d = q[0] + p[0];
                    if w.closed {
                        // 圆凸起鼓过闭合环的起点 / 终点：绕着环走
                        d = py_mod(d, w.total);
                    }
                    ds.push(d);
                }
                let dcs: Vec<f64> = ds.iter().map(|&d| d.clamp(0.0, w.total)).collect();
                let (pxs, pys) = w.at_many(&dcs);
                let (uxs, uys) = w.directions(&dcs);
                let mut fill = Vec::with_capacity(bump_pts.len());
                for (i, (p, q)) in bump_pts.iter().zip(par_wrap.iter()).enumerate() {
                    let s = q[0];
                    let side = q[1];
                    let y = p[1] * grow(s + p[0]);
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
