//! 圆弧：过三点的完美圆弧（起点、经过点、终点），对应 Python notes/arc.py 的逐函数移植。
//!
//! 弧记住 `k` = 绘制时屏幕上多少个 beat 对应一个 key（屏幕上 x = beats / k，y 直接用 key）。
//! k 为 1（两个方向等比例）时弧就是正圆的一段。

use crate::Pt;

/// 每 1.5° 一个采样点（对应 Python `math.radians(1.5)`）。
pub const STEP: f64 = 1.5 * (std::f64::consts::PI / 180.0);

/// CPython 3.9 `math.dist` 的复刻（按最大分量缩放 + Neumaier 补偿求和，
/// 普通 sqrt(x²+y²) 会有 ulp 偏差）。
fn dist(a: Pt, b: Pt) -> f64 {
    let (x0, x1) = ((a[0] - b[0]).abs(), (a[1] - b[1]).abs());
    let mut max = 0.0;
    if x0 > max {
        max = x0;
    }
    if x1 > max {
        max = x1;
    }
    if max.is_infinite() {
        return max;
    }
    if x0.is_nan() || x1.is_nan() {
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

/// Python 的浮点取模（结果的符号与除数一致，且零的符号随除数）。
fn py_mod(x: f64, y: f64) -> f64 {
    let r = x % y;
    if r != 0.0 {
        if (r < 0.0) != (y < 0.0) { r + y } else { r }
    } else {
        0.0_f64.copysign(y)
    }
}

/// 过 a、b、c 三点的圆心与半径；三点共线或有两点重合时返回 None。
pub fn circle(a: Pt, b: Pt, c: Pt) -> Option<(Pt, f64)> {
    let a_sq = (b[0] - c[0]).powf(2.0) + (b[1] - c[1]).powf(2.0);
    let b_sq = (a[0] - c[0]).powf(2.0) + (a[1] - c[1]).powf(2.0);
    let c_sq = (a[0] - b[0]).powf(2.0) + (a[1] - b[1]).powf(2.0);
    let size = a_sq.max(b_sq).max(c_sq);
    if size == 0.0 || a_sq.min(b_sq).min(c_sq) < 1e-12 * size {
        return None;
    }
    let s = a_sq * (b_sq + c_sq - a_sq);
    let t = b_sq * (a_sq + c_sq - b_sq);
    let u = c_sq * (a_sq + b_sq - c_sq);
    let total = s + t + u;
    if total.abs() < 1e-9 * size * size {
        return None;
    }
    let centre = [
        (s * a[0] + t * b[0] + u * c[0]) / total,
        (s * a[1] + t * b[1] + u * c[1]) / total,
    ];
    Some((centre, dist(a, centre)))
}

/// 起始角与（有符号的）扫过角度：从 a 经过 b 到 c。
fn angles(a: Pt, b: Pt, c: Pt, centre: Pt) -> (f64, f64) {
    let t0 = (a[1] - centre[1]).atan2(a[0] - centre[0]);
    let t1 = (c[1] - centre[1]).atan2(c[0] - centre[0]);
    let mut span = py_mod(t1 - t0, 2.0 * std::f64::consts::PI);
    // 走哪一边：经过 b 的那边（b 在 a -> c 连线的左侧还是右侧）
    if (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0]) > 0.0 {
        span -= 2.0 * std::f64::consts::PI;
    }
    (t0, span)
}

/// 终点回到起点（中间点在别处）：整圆，a -> b 横穿，屏幕上逆时针。
/// 返回（圆心, 半径, 起始角, 2π）；a != c 或 a == b 时返回 None。
pub fn full_circle(a: Pt, b: Pt, c: Pt) -> Option<(Pt, f64, f64, f64)> {
    if a != c || a == b {
        return None;
    }
    let centre = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    Some((
        centre,
        dist(a, centre),
        (a[1] - centre[1]).atan2(a[0] - centre[0]),
        2.0 * std::f64::consts::PI,
    ))
}

/// 过三点的（圆心, 半径, 起始角, 有符号扫角）；没有圆弧时返回 None。
fn arc_through(m: [Pt; 3]) -> Option<(Pt, f64, f64, f64)> {
    if let Some(whole) = full_circle(m[0], m[1], m[2]) {
        return Some(whole);
    }
    let (centre, r) = circle(m[0], m[1], m[2])?;
    let (t0, span) = angles(m[0], m[1], m[2], centre);
    Some((centre, r, t0, span))
}

/// 从 pts[0] 经过 pts[1] 到 pts[2] 的弧的点列（共线时为直线；pts[2] 等于 pts[0] 时为整圆）。
/// 起点与终点精确落在端点上。不足三点时用首尾两点。`k` 见模块说明，`step` 为相邻采样点的角度。
pub fn arc_points(pts: &[Pt], k: f64, step: f64) -> Vec<Pt> {
    if pts.is_empty() {
        return Vec::new();
    }
    let (a, b, c) = if pts.len() >= 3 {
        (pts[0], Some(pts[1]), pts[2])
    } else {
        (pts[0], None, pts[pts.len() - 1])
    };
    let Some(b) = b else {
        return vec![a, c];
    };
    let m = [[a[0] / k, a[1]], [b[0] / k, b[1]], [c[0] / k, c[1]]];
    let Some((centre, r, t0, span)) = arc_through(m) else {
        return if b != a && b != c {
            vec![a, b, c]
        } else {
            vec![a, c]
        };
    };
    let n = ((span.abs() / step).ceil() as usize).max(2);
    let mut out = Vec::with_capacity(n + 1);
    out.push(a);
    for i in 1..n {
        let t = t0 + span * i as f64 / n as f64;
        out.push([(centre[0] + r * t.cos()) * k, centre[1] + r * t.sin()]);
    }
    out.push(c);
    out
}

/// 弧的贝塞尔曲线点列（bezier.py 的 anchor, handle, handle, anchor, ...），
/// 每段最多四分之一圆；共线时为直线。
pub fn arc_bezier(pts: &[Pt], k: f64) -> Vec<Pt> {
    if pts.len() < 3 {
        return Vec::new();
    }
    let (a, b, c) = (pts[0], pts[1], pts[2]);
    let m = [[a[0] / k, a[1]], [b[0] / k, b[1]], [c[0] / k, c[1]]];
    let Some((centre, r, t0, span)) = arc_through(m) else {
        return line_bezier(a, c);
    };
    let n = ((span.abs() / (std::f64::consts::PI / 2.0) - 1e-9).ceil() as usize).max(1);
    let step = span / n as f64;
    let h = 4.0 / 3.0 * (step / 4.0).tan() * r; // 一段圆弧的手柄长度

    let on = |t: f64| [centre[0] + r * t.cos(), centre[1] + r * t.sin()];

    let mut out = vec![a];
    for i in 0..n {
        let ta = t0 + step * i as f64;
        let tb = t0 + step * (i + 1) as f64;
        let pa = on(ta);
        let pb = on(tb);
        let h1 = [pa[0] - h * ta.sin(), pa[1] + h * ta.cos()];
        let h2 = [pb[0] + h * tb.sin(), pb[1] - h * tb.cos()];
        out.push([h1[0] * k, h1[1]]);
        out.push([h2[0] * k, h2[1]]);
        out.push([pb[0] * k, pb[1]]);
    }
    if let Some(last) = out.last_mut() {
        *last = c;
    }
    out
}

/// 填满 box (x0, y0, x1, y1) 的椭圆，4 个四分之一圆的闭合贝塞尔曲线，起点在左端。
pub fn ellipse_bezier(box_: [f64; 4]) -> Vec<Pt> {
    let (x0, y0, x1, y1) = (box_[0], box_[1], box_[2], box_[3]);
    let (cx, cy, rx, ry) = (
        (x0 + x1) / 2.0,
        (y0 + y1) / 2.0,
        (x1 - x0) / 2.0,
        (y1 - y0) / 2.0,
    );
    let h = 4.0 / 3.0 * (std::f64::consts::PI / 8.0).tan();
    let mut out = vec![[cx - rx, cy]];
    for q in 0..4 {
        // 左 -> 上 -> 右 -> 下 -> 左
        let a0 = std::f64::consts::PI - q as f64 * std::f64::consts::PI / 2.0;
        let a1 = std::f64::consts::PI / 2.0 - q as f64 * std::f64::consts::PI / 2.0;
        let p0 = (a0.cos(), a0.sin());
        let p1 = (a1.cos(), a1.sin());
        let h1 = (p0.0 + h * a0.sin(), p0.1 - h * a0.cos());
        let h2 = (p1.0 - h * a1.sin(), p1.1 + h * a1.cos());
        for (x, y) in [h1, h2, p1] {
            out.push([cx + rx * x, cy + ry * y]);
        }
    }
    let first = out[0];
    if let Some(last) = out.last_mut() {
        *last = first;
    }
    out
}

/// 从 a 到 c 的直线贝塞尔（4 个点）。
pub fn line_bezier(a: Pt, c: Pt) -> Vec<Pt> {
    vec![
        a,
        [a[0] + (c[0] - a[0]) / 3.0, a[1] + (c[1] - a[1]) / 3.0],
        [
            a[0] + (c[0] - a[0]) * 2.0 / 3.0,
            a[1] + (c[1] - a[1]) * 2.0 / 3.0,
        ],
        c,
    ]
}

/// 形状 / 笔画文件里读出的 k 的清洗（缺失或非法时为 1）。
pub fn clean_k(k: f64) -> f64 {
    if 1e-9 < k && k < 1e9 && k.is_finite() {
        k
    } else {
        1.0
    }
}
