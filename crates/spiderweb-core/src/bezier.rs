//! 贝塞尔曲线：带锚点与手柄的曲线（钢笔工具），对应 Python notes/bezier.py 的逐函数移植：
//! 采样、增删锚点、钢笔编辑、对称曲线，以及在容差内用尽量少的锚点拟合点列。
//!
//! 曲线是一个扁平点列 `[anchor, handle, handle, anchor, handle, handle, anchor, ...]`：
//! 每第三个点（0, 3, 6, ...）是曲线经过的锚点，两个锚点之间是两个手柄：
//! 前一个锚点的出手柄与后一个锚点的入手柄。

use crate::Pt;
use crate::shape::Sym;

/// CPython 3.9 `math.dist` / `math.hypot` 的复刻：先按最大分量缩放，
/// 再用 Neumaier 补偿求和（float 逐位一致所需，普通 sqrt(x²+y²) 会有 ulp 偏差）。
fn norm2(a: f64, b: f64) -> f64 {
    let x0 = a.abs();
    let x1 = b.abs();
    let mut max = 0.0;
    if x0 > max {
        max = x0;
    }
    if x1 > max {
        max = x1;
    }
    let found_nan = x0.is_nan() || x1.is_nan();
    // vector_norm(2, ...) （见 CPython Modules/mathmodule.c）
    if max.is_infinite() {
        return max;
    }
    if found_nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return max;
    }
    let (mut csum, mut frac) = (1.0, 0.0);
    for x in [x0, x1] {
        let x = x / max;
        let x = x * x;
        let oldcsum = csum;
        csum += x;
        frac += (oldcsum - csum) + x;
    }
    max * (csum - 1.0 + frac).sqrt()
}

/// `math.dist(a, b)`。
fn dist(a: Pt, b: Pt) -> f64 {
    norm2(a[0] - b[0], a[1] - b[1])
}

/// `math.hypot(x, y)`。
fn hypot(x: f64, y: f64) -> f64 {
    norm2(x, y)
}

/// 一条曲线：`pts` 为扁平点列，`sharp` 为尖角锚点序号，`sym` 为对称方式。
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub pts: Vec<Pt>,
    pub sharp: Vec<usize>,
    pub sym: Option<Sym>,
}

impl Curve {
    pub fn new(pts: Vec<Pt>) -> Self {
        Self {
            pts,
            sharp: Vec::new(),
            sym: None,
        }
    }

    /// 锚点数。
    pub fn anchor_count(&self) -> usize {
        anchor_count(&self.pts)
    }
}

/// 锚点数：点列长度决定（对应 Python `anchor_count`）。
pub fn anchor_count(pts: &[Pt]) -> usize {
    if pts.is_empty() {
        0
    } else {
        (pts.len() - 1) / 3 + 1
    }
}

/// 一段三次贝塞尔上的点。
pub fn seg_point(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f64) -> Pt {
    let mt = 1.0 - t;
    let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
    [
        a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
        a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
    ]
}

/// 每段的 4 个点（对应 Python `segments`）。
pub fn segments(pts: &[Pt]) -> Vec<[Pt; 4]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= pts.len() {
        out.push([pts[i], pts[i + 1], pts[i + 2], pts[i + 3]]);
        i += 3;
    }
    out
}

/// 曲线上的采样点，每段 `n` 个（对应 Python `sample`，默认 n=48）。
pub fn sample(pts: &[Pt], n: usize) -> Vec<Pt> {
    let mut out = Vec::new();
    if pts.is_empty() {
        return out;
    }
    out.push(pts[0]);
    for seg in segments(pts) {
        for i in 1..=n {
            out.push(seg_point(
                seg[0],
                seg[1],
                seg[2],
                seg[3],
                i as f64 / n as f64,
            ));
        }
    }
    out
}

fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// 在第 s 段的 t（0..1）处加一个锚点；曲线形状完全不变（对应 Python `split`）。
pub fn split(pts: &[Pt], s: usize, t: f64) -> Vec<Pt> {
    let i = 3 * s;
    let Some(seg) = pts.get(i..i + 4) else {
        return pts.to_vec();
    };
    let (p0, p1, p2, p3) = (seg[0], seg[1], seg[2], seg[3]);
    let (a, b, c) = (lerp(p0, p1, t), lerp(p1, p2, t), lerp(p2, p3, t));
    let (d, e) = (lerp(a, b, t), lerp(b, c, t));
    let mut out = pts[..i + 1].to_vec();
    out.push(a);
    out.push(d);
    out.push(lerp(d, e, t));
    out.push(e);
    out.push(c);
    out.extend_from_slice(&pts[i + 3..]);
    out
}

/// 去掉锚点 a（不是首尾）及其两个手柄（对应 Python `remove_anchor`）。
pub fn remove_anchor(pts: &[Pt], a: usize) -> Vec<Pt> {
    let i = 3 * a;
    if i == 0 {
        // Python 对 a=0 的切片行为（调用方不会这样用）
        if pts.len() < 2 {
            return pts.to_vec();
        }
        let mut out = pts[..pts.len() - 1].to_vec();
        out.extend_from_slice(&pts[2..]);
        return out;
    }
    let mut out = pts[..i - 1].to_vec();
    out.extend_from_slice(&pts[i + 2..]);
    out
}

/// 手柄点所属的锚点（1 -> 0, 2 -> 3, 4 -> 3, ...）。
pub fn handle_anchor(i: usize) -> usize {
    if i % 3 == 1 { i - 1 } else { i + 1 }
}

// ---------------------------------------------------------------- 对称曲线
// 对称曲线有奇数个锚点：中间那个位于对称线（或点）上，一半的第 i 点与另一半的 len-1-i 点配对。
// "turn"：另一半是这一半绕两端中点转半圈（S 形）；"mirror"：隔着过中点的线镜像（拱形）。
// 镜线沿 `axis`：0 = 时间方向，1 = 音高方向；None = 垂直于两端连线，精确镜像。

/// 对称变换（对应 Python `Symmetry`）。
#[derive(Clone, Debug)]
pub struct Symmetry {
    pub mode: Sym,
    pub m: Pt,
    pub d: Pt,
    pub w: Pt,
    pub det: f64,
    pub ok: bool,
}

impl Symmetry {
    /// 由曲线的首尾点构造；`axis` = Some(0) / Some(1) / None。点数不足 2 时返回 None。
    pub fn new(pts: &[Pt], mode: Sym, axis: Option<u8>) -> Option<Self> {
        if pts.len() < 2 {
            return None;
        }
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let d = [(b[0] - a[0]) / 2.0, (b[1] - a[1]) / 2.0];
        let w = match axis {
            Some(0) => [1.0, 0.0],
            Some(1) => [0.0, 1.0],
            _ => [-d[1], d[0]],
        };
        let det = d[0] * w[1] - d[1] * w[0];
        let ok = if mode == Sym::Mirror {
            det.abs() > 1e-12
        } else {
            true
        };
        Some(Self {
            mode,
            m,
            d,
            w,
            det,
            ok,
        })
    }

    /// v = alpha * d + beta * w（w 沿镜线）。
    pub fn split(&self, v: Pt) -> (f64, f64) {
        let (d, w) = (self.d, self.w);
        (
            (v[0] * w[1] - v[1] * w[0]) / self.det,
            (d[0] * v[1] - d[1] * v[0]) / self.det,
        )
    }

    /// alpha * d + beta * w。
    pub fn join(&self, alpha: f64, beta: f64) -> Pt {
        [
            alpha * self.d[0] + beta * self.w[0],
            alpha * self.d[1] + beta * self.w[1],
        ]
    }

    /// p 的对称点。
    pub fn reflect(&self, p: Pt) -> Pt {
        let m = self.m;
        if self.mode == Sym::Turn {
            return [2.0 * m[0] - p[0], 2.0 * m[1] - p[1]];
        }
        let (alpha, beta) = self.split([p[0] - m[0], p[1] - m[1]]);
        let v = self.join(-alpha, beta);
        [m[0] + v[0], m[1] + v[1]]
    }

    /// 中间锚点的最近落点：中点（turn）或镜线上。
    pub fn onto_line(&self, p: Pt) -> Pt {
        if self.mode == Sym::Turn {
            return self.m;
        }
        let (_, beta) = self.split([p[0] - self.m[0], p[1] - self.m[1]]);
        let v = self.join(0.0, beta);
        [self.m[0] + v[0], self.m[1] + v[1]]
    }

    /// 镜像曲线平滑中间锚点的手柄：沿两端方向（拱顶是圆的），保留往那个方向伸出的长度。
    pub fn flat(&self, anchor: Pt, h: Pt) -> Pt {
        let (alpha, _) = self.split([h[0] - anchor[0], h[1] - anchor[1]]);
        let v = self.join(alpha, 0.0);
        [anchor[0] + v[0], anchor[1] + v[1]]
    }
}

/// 把一半（source 0 = 从前半，1 = 从后半）复制到另一半，返回 (pts, sharp)。
/// 需要奇数个锚点（见 `make_symmetric`）；两端保持不动。
pub fn symmetric(
    pts: &[Pt],
    sharp: &[usize],
    mode: Sym,
    axis: Option<u8>,
    source: u8,
) -> (Vec<Pt>, Vec<usize>) {
    let n = pts.len();
    if n < 7 || !(n - 1).is_multiple_of(6) {
        return (pts.to_vec(), sharp.to_vec());
    }
    let Some(sym) = Symmetry::new(pts, mode, axis) else {
        return (pts.to_vec(), sharp.to_vec());
    };
    if !sym.ok {
        return (pts.to_vec(), sharp.to_vec());
    }
    let mut pts = pts.to_vec();
    let c = (n - 1) / 2;
    let mid = c / 3;
    let last = anchor_count(&pts) - 1;
    if source == 0 {
        for i in 1..c - 1 {
            pts[n - 1 - i] = sym.reflect(pts[i]);
        }
    } else {
        for i in c + 2..n - 1 {
            pts[n - 1 - i] = sym.reflect(pts[i]);
        }
    }
    // 中间锚点落到对称线 / 点上，手柄跟着走
    let new = sym.onto_line(pts[c]);
    let h = if source == 0 { c - 1 } else { c + 1 };
    let mut hp = [
        pts[h][0] + new[0] - pts[c][0],
        pts[h][1] + new[1] - pts[c][1],
    ];
    pts[c] = new;
    let smooth_mid = mode == Sym::Turn || !sharp.contains(&mid);
    if mode == Sym::Mirror && smooth_mid {
        hp = sym.flat(new, hp);
    }
    pts[h] = hp;
    pts[2 * c - h] = sym.reflect(hp);
    let mut out: Vec<usize> = sharp
        .iter()
        .copied()
        .filter(|&a| if source == 0 { a < mid } else { a > mid })
        .collect();
    let mirrored: Vec<usize> = out.iter().filter_map(|&a| last.checked_sub(a)).collect();
    out.extend(mirrored);
    if !smooth_mid {
        out.push(mid);
    }
    out.sort_unstable();
    out.dedup();
    (pts, out)
}

/// 同 `symmetric`，先给曲线补一个中间锚点（把中间一段一分为二）。
pub fn make_symmetric(
    pts: &[Pt],
    sharp: &[usize],
    mode: Sym,
    axis: Option<u8>,
    source: u8,
) -> (Vec<Pt>, Vec<usize>) {
    let segs = segments(pts).len();
    let (pts, sharp) = if segs % 2 == 1 {
        let s = segs / 2;
        let pts = split(pts, s, 0.5);
        let sharp = sharp
            .iter()
            .map(|&a| if a > s { a + 1 } else { a })
            .collect();
        (pts, sharp)
    } else {
        (pts.to_vec(), sharp.to_vec())
    };
    symmetric(&pts, &sharp, mode, axis, source)
}

// ---------------------------------------------------------------- 钢笔编辑

impl Curve {
    /// 设置尖角锚点（空则清除）。
    pub fn set_sharp(&mut self, sharp: &[usize]) {
        if sharp.is_empty() {
            self.sharp.clear();
        } else {
            let mut s = sharp.to_vec();
            s.sort_unstable();
            s.dedup();
            self.sharp = s;
        }
    }
}

/// 镜像曲线的镜线方向：屏幕上看垂直于两端连线（1 = 上下，0 = 左右），exact 时为 None（精确镜像）。
pub fn sym_axis(pts: &[Pt], to_screen: &dyn Fn(Pt) -> [f64; 2], exact: bool) -> Option<u8> {
    if exact || pts.is_empty() {
        return None;
    }
    let s = to_screen(pts[0]);
    let (ax, ay) = (s[0], s[1]);
    let s = to_screen(pts[pts.len() - 1]);
    let (bx, by) = (s[0], s[1]);
    Some(if (bx - ax).abs() >= (by - ay).abs() {
        1
    } else {
        0
    })
}

/// 对称曲线的另一半跟着点 i 所在的一半；是对称曲线时返回 true。
pub fn keep_symmetric(
    c: &mut Curve,
    i: usize,
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    exact: bool,
) -> bool {
    let Some(mode) = c.sym else {
        return false;
    };
    let n = c.pts.len();
    let source = if n > 0 && i > (n - 1) / 2 { 1 } else { 0 };
    let (pts, sharp) = symmetric(
        &c.pts,
        &c.sharp,
        mode,
        sym_axis(&c.pts, to_screen, exact),
        source,
    );
    c.pts = pts;
    c.set_sharp(&sharp);
    true
}

/// 把点 i 拖到 new：
/// 锚点带着手柄走（alt：从它拉出新的两侧手柄，这一侧对称）；
/// 手柄点移动，平滑锚点上另一个手柄跟着转以保持平滑，屏幕上保持长度（alt：只动这一个，锚点变尖角）。
/// 对称曲线的另一半跟着走。
pub fn drag_point(
    c: &mut Curve,
    i: usize,
    new: Pt,
    alt: bool,
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    from_screen: &dyn Fn(f64, f64) -> Pt,
    exact: bool,
) {
    let n = c.pts.len();
    if i >= n {
        return;
    }
    if i.is_multiple_of(3) {
        let d = [new[0] - c.pts[i][0], new[1] - c.pts[i][1]];
        if alt {
            for j in [i.checked_sub(1), Some(i + 1)].into_iter().flatten() {
                if j < n {
                    let s = if j > i { 1.0 } else { -1.0 };
                    c.pts[j] = [c.pts[i][0] + s * d[0], c.pts[i][1] + s * d[1]];
                }
            }
            let a = i / 3;
            let sharp: Vec<usize> = c.sharp.iter().copied().filter(|&x| x != a).collect();
            c.set_sharp(&sharp);
        } else {
            for j in [i.checked_sub(1), Some(i), Some(i + 1)]
                .into_iter()
                .flatten()
            {
                if j < n {
                    c.pts[j] = [c.pts[j][0] + d[0], c.pts[j][1] + d[1]];
                }
            }
        }
    } else {
        c.pts[i] = new;
        let a = handle_anchor(i);
        if a > 0 && a < n - 1 && !c.sharp.contains(&(a / 3)) {
            if alt {
                let mut sharp = c.sharp.clone();
                sharp.push(a / 3);
                c.set_sharp(&sharp);
            } else {
                let other = 2 * a - i;
                let s = to_screen(c.pts[a]);
                let (ax, ay) = (s[0], s[1]);
                let s = to_screen(new);
                let (hx, hy) = (s[0], s[1]);
                let s = to_screen(c.pts[other]);
                let (ox, oy) = (s[0], s[1]);
                let d = hypot(hx - ax, hy - ay);
                let length = hypot(ox - ax, oy - ay);
                if d > 0.0 && length > 0.0 {
                    c.pts[other] =
                        from_screen(ax - (hx - ax) / d * length, ay - (hy - ay) / d * length);
                }
            }
        }
    }
    keep_symmetric(c, i, to_screen, exact);
}

/// 在第 seg 段的 t 处加一个锚点并移到 new（曲线经过那里）；对称曲线另一半也加一个。
/// 那里已经是锚点（t = 0 / 1）时不加，返回 false。
pub fn add_anchor(
    c: &mut Curve,
    seg: usize,
    t: f64,
    new: Pt,
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    exact: bool,
) -> bool {
    if t == 0.0 || t == 1.0 {
        return false;
    }
    let mut splits = vec![(seg, t)];
    if c.sym.is_some() {
        // 另一半的同一位置
        splits.push((segments(&c.pts).len() - 1 - seg, 1.0 - t));
    }
    let mut at = 3 * (seg + 1);
    let mut pts = c.pts.clone();
    let mut sharp = c.sharp.clone();
    // 后切的先做，先切的位置才不会被挪动
    splits.sort_by(|x, y| y.0.cmp(&x.0).then(y.1.total_cmp(&x.1)));
    for (s, tt) in splits {
        pts = split(&pts, s, tt);
        sharp = sharp
            .iter()
            .map(|&a| if a > s { a + 1 } else { a })
            .collect();
        if s < seg {
            at += 3;
        }
    }
    if at < pts.len() {
        let d = [new[0] - pts[at][0], new[1] - pts[at][1]];
        for j in [at.checked_sub(1), Some(at), Some(at + 1)]
            .into_iter()
            .flatten()
        {
            if j < pts.len() {
                pts[j] = [pts[j][0] + d[0], pts[j][1] + d[1]];
            }
        }
    }
    c.pts = pts;
    c.set_sharp(&sharp);
    keep_symmetric(c, at, to_screen, exact);
    true
}

/// 右键点 i 会发生什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanDelete {
    /// 锚点被删掉
    Anchor,
    /// 手柄被收回锚点
    Handle,
    /// 对称曲线的中间锚点：保留
    Middle,
}

/// `None` = 端点及其手柄保留（删了就抓不回来了）。
pub fn can_delete(c: &Curve, i: usize) -> Option<CanDelete> {
    let n = c.pts.len();
    if i >= n {
        return None;
    }
    let a = if !i.is_multiple_of(3) {
        handle_anchor(i)
    } else {
        i
    };
    if a == 0 || a + 1 >= n {
        return None;
    }
    if !i.is_multiple_of(3) {
        return Some(CanDelete::Handle);
    }
    if c.sym.is_some() && a == (n - 1) / 2 {
        Some(CanDelete::Middle)
    } else {
        Some(CanDelete::Anchor)
    }
}

/// 右键点 i（见 `can_delete`）：中间的锚点被删掉（对称曲线的搭档也删），手柄收回锚点（那里成尖角）。
pub fn delete_point(c: &mut Curve, i: usize, to_screen: &dyn Fn(Pt) -> [f64; 2], exact: bool) {
    let Some(what) = can_delete(c, i) else {
        return;
    };
    match what {
        CanDelete::Anchor => {
            let a = i / 3;
            let mut ks = vec![a];
            if c.sym.is_some() {
                ks.push(anchor_count(&c.pts) - 1 - a);
            }
            ks.sort_unstable_by(|x, y| y.cmp(x));
            ks.dedup();
            let mut pts = c.pts.clone();
            let mut sharp = c.sharp.clone();
            for k in ks {
                pts = remove_anchor(&pts, k);
                sharp = sharp
                    .iter()
                    .filter(|&&b| b != k)
                    .map(|&b| if b > k { b - 1 } else { b })
                    .collect();
            }
            c.pts = pts;
            c.set_sharp(&sharp);
        }
        CanDelete::Handle => {
            let a = handle_anchor(i);
            c.pts[i] = c.pts[a];
            let mut sharp = c.sharp.clone();
            sharp.push(a / 3);
            c.set_sharp(&sharp);
        }
        CanDelete::Middle => return,
    }
    keep_symmetric(c, i, to_screen, exact);
}

/// 打开（mode = Mirror / Turn）或关闭（None）对称；`source` 那一半保留形状（0 = 前半，1 = 后半）。
pub fn set_symmetry(
    c: &mut Curve,
    mode: Option<Sym>,
    source: u8,
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    exact: bool,
) {
    let Some(mode) = mode else {
        c.sym = None;
        return;
    };
    let (pts, sharp) = make_symmetric(
        &c.pts,
        &c.sharp,
        mode,
        sym_axis(&c.pts, to_screen, exact),
        source,
    );
    c.pts = pts;
    c.sym = Some(mode);
    c.set_sharp(&sharp);
}

/// 屏幕上的 (x, y) 离哪一半最近（0 = 前半，1 = 后半）。
pub fn half_at(pts: &[Pt], to_screen: &dyn Fn(Pt) -> [f64; 2], x: f64, y: f64) -> usize {
    let segs = segments(pts).len();
    match nearest(pts, to_screen, x, y, 64) {
        Some((seg, t, _)) if (seg as f64 + t) * 2.0 > segs as f64 => 1,
        _ => 0,
    }
}

/// 要显示的点（对应 Python `pen_handles`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// 手柄点
    Ctrl,
    /// 锚点
    Anchor,
    /// 端点
    End,
}

/// `[(点号, 种类)]`：按绘制顺序（锚点在最上）。未选中时只显示两个端点。
pub fn pen_handles(pts: &[Pt], selected: bool) -> Vec<(usize, HandleKind)> {
    let n = pts.len();
    if !selected {
        if n == 0 {
            return Vec::new();
        }
        return vec![(0, HandleKind::End), (n - 1, HandleKind::End)];
    }
    let mut out: Vec<(usize, HandleKind)> = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        if i % 3 == 0 {
            continue;
        }
        let a = handle_anchor(i);
        let first_or_last = i == 1 || i == n - 2;
        let differs = a >= n || *p != pts[a];
        if first_or_last || differs {
            out.push((i, HandleKind::Ctrl));
        }
    }
    let mut i = 3;
    while i + 1 < n {
        out.push((i, HandleKind::Anchor));
        i += 3;
    }
    if n > 0 {
        out.push((0, HandleKind::End));
        out.push((n - 1, HandleKind::End));
    }
    out
}

/// 要画的手柄线 `[(锚点, 手柄点)]`。
pub fn handle_lines(pts: &[Pt]) -> Vec<(Pt, Pt)> {
    let mut out = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        if i % 3 == 0 {
            continue;
        }
        let a = handle_anchor(i);
        if a < pts.len() && *p != pts[a] {
            out.push((pts[a], *p));
        }
    }
    out
}

/// 屏幕上离 (x, y) 最近的曲线点 `(段号, t, 距离)`；没有段时返回 None。
pub fn nearest(
    pts: &[Pt],
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    x: f64,
    y: f64,
    n: usize,
) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (s, seg) in segments(pts).iter().enumerate() {
        for i in 0..=n {
            let p = seg_point(seg[0], seg[1], seg[2], seg[3], i as f64 / n as f64);
            let sp = to_screen(p);
            let d = hypot(sp[0] - x, sp[1] - y);
            let better = match best {
                None => true,
                Some(b) => d < b.2,
            };
            if better {
                best = Some((s, i as f64 / n as f64, d));
            }
        }
    }
    best
}

/// 折线按长度均匀取 n+1 个点（对应 Python `resample`，默认 n=300）。
pub fn resample(points: &[Pt], n: usize) -> Vec<Pt> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut lens = vec![0.0];
    for w in points.windows(2) {
        let last = lens[lens.len() - 1];
        lens.push(last + dist(w[0], w[1]));
    }
    let total = lens[lens.len() - 1];
    if total == 0.0 {
        return vec![points[0], points[points.len() - 1]];
    }
    let mut out = Vec::with_capacity(n + 1);
    let mut j = 0;
    for i in 0..=n {
        let l = total * i as f64 / n as f64;
        while j < lens.len() - 2 && lens[j + 1] < l {
            j += 1;
        }
        let seg = lens[j + 1] - lens[j];
        let t = if seg == 0.0 { 0.0 } else { (l - lens[j]) / seg };
        out.push(lerp(points[j], points[j + 1], t));
    }
    out
}

/// 两条曲线差多少：均匀比较两者后，最大点距（对应 Python `difference`）。
pub fn difference(pts_a: &[Pt], pts_b: &[Pt]) -> f64 {
    let a = resample(&sample(pts_a, 48), 100);
    let b = resample(&sample(pts_b, 48), 100);
    a.iter()
        .zip(b.iter())
        .fold(0.0, |m, (p, q)| m.max(dist(*p, *q)))
}

// ---------------------------------------------------------------- 拟合（Philip Schneider 算法）

fn unit(v: Pt) -> Pt {
    let d = hypot(v[0], v[1]);
    if d != 0.0 {
        [v[0] / d, v[1] / d]
    } else {
        [0.0, 0.0]
    }
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

/// 用尽量少的锚点拟合 points，曲线经过首尾且与所有点的距离不超过 tol。
pub fn fit(points: &[Pt], tol: f64) -> Vec<Pt> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut pts = vec![points[0]];
    for &p in &points[1..] {
        if dist(p, pts[pts.len() - 1]) > 1e-9 {
            pts.push(p);
        }
    }
    if pts.len() < 2 {
        let p = pts[0];
        return vec![p, p, p, p];
    }
    let pts = resample(&pts, 300);
    let k = 3.min(pts.len() - 1);
    let t_left = unit(sub(pts[k], pts[0]));
    let t_right = unit(sub(pts[pts.len() - 1 - k], pts[pts.len() - 1]));
    let mut out = vec![pts[0]];
    fit_rec(&pts, t_left, t_right, tol, &mut out, 0);
    out
}

fn fit_rec(pts: &[Pt], t_left: Pt, t_right: Pt, tol: f64, out: &mut Vec<Pt>, depth: u32) {
    let p0 = pts[0];
    let p3 = pts[pts.len() - 1];
    if pts.len() == 2 {
        let d = dist(p0, p3) / 3.0;
        out.push([p0[0] + t_left[0] * d, p0[1] + t_left[1] * d]);
        out.push([p3[0] + t_right[0] * d, p3[1] + t_right[1] * d]);
        out.push(p3);
        return;
    }
    let mut u = chord_params(pts);
    let mut bez = generate(pts, &u, t_left, t_right);
    let (mut err, mut at) = max_error(pts, &bez, &u);
    if err > tol && err < tol * 16.0 {
        for _ in 0..6 {
            u = pts
                .iter()
                .zip(u.iter())
                .map(|(p, &t)| newton(&bez, *p, t))
                .collect();
            bez = generate(pts, &u, t_left, t_right);
            let (e, a) = max_error(pts, &bez, &u);
            err = e;
            at = a;
            if err <= tol {
                break;
            }
        }
    }
    if err <= tol || depth > 10 || pts.len() < 5 {
        out.push(bez[1]);
        out.push(bez[2]);
        out.push(p3);
        return;
    }
    let mut t_mid = unit(sub(pts[at - 1], pts[at + 1]));
    if t_mid == [0.0, 0.0] {
        t_mid = unit(sub(pts[at - 1], pts[at]));
    }
    fit_rec(&pts[..at + 1], t_left, t_mid, tol, out, depth + 1);
    fit_rec(
        &pts[at..],
        [-t_mid[0], -t_mid[1]],
        t_right,
        tol,
        out,
        depth + 1,
    );
}

fn chord_params(pts: &[Pt]) -> Vec<f64> {
    let mut u = vec![0.0];
    for w in pts.windows(2) {
        let last = u[u.len() - 1];
        u.push(last + dist(w[0], w[1]));
    }
    let total = u[u.len() - 1];
    u.iter().map(|&x| x / total).collect()
}

/// 沿给定两端方向的最佳手柄（最小二乘）。
fn generate(pts: &[Pt], u: &[f64], t_left: Pt, t_right: Pt) -> [Pt; 4] {
    let p0 = pts[0];
    let p3 = pts[pts.len() - 1];
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, &t) in pts.iter().zip(u.iter()) {
        let mt = 1.0 - t;
        let b0 = mt.powf(3.0);
        let b1 = 3.0 * mt * mt * t;
        let b2 = 3.0 * mt * t * t;
        let b3 = t.powf(3.0);
        let a0 = [t_left[0] * b1, t_left[1] * b1];
        let a1 = [t_right[0] * b2, t_right[1] * b2];
        c00 += a0[0] * a0[0] + a0[1] * a0[1];
        c01 += a0[0] * a1[0] + a0[1] * a1[1];
        c11 += a1[0] * a1[0] + a1[1] * a1[1];
        let tmp = [
            p[0] - (p0[0] * (b0 + b1) + p3[0] * (b2 + b3)),
            p[1] - (p0[1] * (b0 + b1) + p3[1] * (b2 + b3)),
        ];
        x0 += a0[0] * tmp[0] + a0[1] * tmp[1];
        x1 += a1[0] * tmp[0] + a1[1] * tmp[1];
    }
    let det = c00 * c11 - c01 * c01;
    let length = dist(p0, p3);
    let (mut al, mut ar) = (0.0, 0.0);
    if det.abs() > 1e-12 {
        al = (x0 * c11 - x1 * c01) / det;
        ar = (c00 * x1 - c01 * x0) / det;
    }
    if al < 1e-6 * length || ar < 1e-6 * length {
        al = length / 3.0;
        ar = length / 3.0;
    }
    [
        p0,
        [p0[0] + t_left[0] * al, p0[1] + t_left[1] * al],
        [p3[0] + t_right[0] * ar, p3[1] + t_right[1] * ar],
        p3,
    ]
}

fn max_error(pts: &[Pt], bez: &[Pt; 4], u: &[f64]) -> (f64, usize) {
    let mut worst = 0.0;
    let mut at = pts.len() / 2;
    for i in 1..pts.len().saturating_sub(1) {
        let d = dist(seg_point(bez[0], bez[1], bez[2], bez[3], u[i]), pts[i]);
        if d > worst {
            worst = d;
            at = i;
        }
    }
    (worst, at)
}

/// t 移到曲线上离 p 最近的位置附近。
fn newton(bez: &[Pt; 4], p: Pt, t: f64) -> f64 {
    let q = seg_point(bez[0], bez[1], bez[2], bez[3], t);
    let mt = 1.0 - t;
    let d1 = [
        3.0 * (mt * mt * (bez[1][0] - bez[0][0])
            + 2.0 * mt * t * (bez[2][0] - bez[1][0])
            + t * t * (bez[3][0] - bez[2][0])),
        3.0 * (mt * mt * (bez[1][1] - bez[0][1])
            + 2.0 * mt * t * (bez[2][1] - bez[1][1])
            + t * t * (bez[3][1] - bez[2][1])),
    ];
    let d2 = [
        6.0 * (mt * (bez[2][0] - 2.0 * bez[1][0] + bez[0][0])
            + t * (bez[3][0] - 2.0 * bez[2][0] + bez[1][0])),
        6.0 * (mt * (bez[2][1] - 2.0 * bez[1][1] + bez[0][1])
            + t * (bez[3][1] - 2.0 * bez[2][1] + bez[1][1])),
    ];
    let num = (q[0] - p[0]) * d1[0] + (q[1] - p[1]) * d1[1];
    let den = d1[0].powf(2.0) + d1[1].powf(2.0) + (q[0] - p[0]) * d2[0] + (q[1] - p[1]) * d2[1];
    if den == 0.0 {
        return t;
    }
    // Python 的 min(1.0, max(0.0, x))：NaN 时取 0，与 clamp 不同
    let x = t - num / den;
    if x > 1.0 {
        1.0
    } else if x > 0.0 {
        x
    } else {
        0.0
    }
}
