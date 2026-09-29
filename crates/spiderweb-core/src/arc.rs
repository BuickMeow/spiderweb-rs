//! Arcs: the perfect circular arc through three points (start, through, end), a function-by-function port of Python notes/arc.py.
//!
//! An arc remembers `k` = how many beats per key were on screen when it was drawn (on screen x = beats / k, y uses key directly).
//! With k = 1 (equal scale in both directions) the arc is a segment of a true circle.

use crate::{Pt, dist};

/// One sample point every 1.5° (as in Python `math.radians(1.5)`).
pub const STEP: f64 = 1.5 * (std::f64::consts::PI / 180.0);

/// Python's floating-point modulo (the result takes the divisor's sign, and a zero result takes the divisor's sign).
fn py_mod(x: f64, y: f64) -> f64 {
    let r = x % y;
    if r != 0.0 {
        if (r < 0.0) != (y < 0.0) { r + y } else { r }
    } else {
        0.0_f64.copysign(y)
    }
}

/// Centre and radius of the circle through a, b, c; None if the three points are collinear or two coincide.
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

/// Start angle and (signed) swept angle: from a through b to c.
fn angles(a: Pt, b: Pt, c: Pt, centre: Pt) -> (f64, f64) {
    let t0 = (a[1] - centre[1]).atan2(a[0] - centre[0]);
    let t1 = (c[1] - centre[1]).atan2(c[0] - centre[0]);
    let mut span = py_mod(t1 - t0, 2.0 * std::f64::consts::PI);
    // Which side to go: the side through b (whether b is left or right of the a -> c line)
    if (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0]) > 0.0 {
        span -= 2.0 * std::f64::consts::PI;
    }
    (t0, span)
}

/// The end returns to the start (the middle point is elsewhere): a full circle, a -> b across, counter-clockwise on screen.
/// Returns (centre, radius, start angle, 2π); None if a != c or a == b.
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

/// (Centre, radius, start angle, signed span) through the three points; None if there is no arc.
fn arc_through(m: [Pt; 3]) -> Option<(Pt, f64, f64, f64)> {
    if let Some(whole) = full_circle(m[0], m[1], m[2]) {
        return Some(whole);
    }
    let (centre, r) = circle(m[0], m[1], m[2])?;
    let (t0, span) = angles(m[0], m[1], m[2], centre);
    Some((centre, r, t0, span))
}

/// The arc through pts[0], pts[1], pts[2] as (centre, radius, start angle, signed span), worked out with
/// beats divided by k (as arc_points does: multiply a point's x by k again), or None if they're in a
/// straight line (arc.arc_circle).
pub fn arc_circle(pts: &[Pt], k: f64) -> Option<(Pt, f64, f64, f64)> {
    if pts.len() < 3 {
        return None;
    }
    arc_through([
        [pts[0][0] / k, pts[0][1]],
        [pts[1][0] / k, pts[1][1]],
        [pts[2][0] / k, pts[2][1]],
    ])
}

/// Point list of the arc from pts[0] through pts[1] to pts[2] (a straight line when collinear; a full circle when pts[2] equals pts[0]).
/// Start and end land exactly on the endpoints. With fewer than three points the first and last are used. `k` is described in the module
/// docs, `step` is the angle between adjacent samples.
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

/// Bezier point list of the arc (bezier.py's anchor, handle, handle, anchor, ...),
/// each segment at most a quarter circle; a straight line when collinear.
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
    let h = 4.0 / 3.0 * (step / 4.0).tan() * r; // handle length for one arc segment

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

/// Ellipse filling box (x0, y0, x1, y1), a closed Bezier curve of 4 quarter circles, starting at the left end.
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
        // left -> top -> right -> bottom -> left
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

/// Straight-line Bezier from a to c (4 points).
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

/// Sanitise k read from a shape / stroke file (1 when missing or invalid).
pub fn clean_k(k: f64) -> f64 {
    if 1e-9 < k && k < 1e9 && k.is_finite() {
        k
    } else {
        1.0
    }
}
