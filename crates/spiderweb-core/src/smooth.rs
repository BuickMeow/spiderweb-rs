//! smooth: freehand "Straighten", a function-by-function port of Python `notes/smooth.py`.
//!
//! At a given sensitivity (0-100) a shaky stroke becomes straight and smooth; a stroke whose
//! ends meet also becomes a perfect shape: circle / ellipse, square / rectangle, or a
//! straight-edged polygon (triangle, pentagon, hexagon). The sensitivity decides how far the
//! result may stray from the original drawing (within one fifth of its size); within the
//! tolerance simpler shapes win, so raising it makes the result simpler and simpler: a slightly
//! oval loop becomes an ellipse, then a circle; a rectangle becomes a square. A loop keeps its
//! category (a box does not become a circle), and tilt is kept too (it is not straightened up).
//! Only the setting is stored, so the stroke can return to the original at any time (0 = as drawn).

use crate::bezier::{fit, seg_point, segments};
use crate::{Pt, dist, hypot2, round_half_even, round_i64};

/// Default sensitivity.
pub const SMOOTH_DEFAULT: i64 = 0;
/// A turn greater than this within a short span counts as a corner (as in Python `math.radians(40)`).
pub const CORNER: f64 = 40.0 * (std::f64::consts::PI / 180.0);
/// A loop with more corners than this is no longer a shape; only lines and curves remain.
pub const MAX_POLYGON: usize = 6;
/// The end this close to the start (as a fraction of the stroke size) means a closed loop.
pub const LOOP: f64 = 0.1;
/// Number of sample points on the loop when fitting a shape.
pub const SHAPE_SAMPLES: usize = 120;
/// Number of sample points on the stroke when finding corners and fitting lines / curves.
pub const PIECE_SAMPLES: usize = 600;
/// How much extra deviation each additional corner / setting allows (relative to the loop size).
pub const CORNER_COST: f64 = 0.012;

/// How far the result may stray from the original drawing: size is the diagonal of the stroke's bounding box, sensitivity is 0-100.
pub fn tolerance(level: f64, size: f64) -> f64 {
    // Python's max(0.0, min(100.0, level)): with NaN, min picks 100
    let capped = if level.is_nan() {
        100.0
    } else {
        level.clamp(0.0, 100.0)
    };
    0.2 * size * (capped / 100.0).powf(2.5)
}

/// Sanitise a sensitivity setting: round then clamp to 0-100; NaN (the fallback for non-numeric input in Python) uses the default.
pub fn clean_level(value: f64) -> i64 {
    if value.is_nan() {
        return SMOOTH_DEFAULT;
    }
    (round_half_even(value) as i64).clamp(0, 100)
}

/// Straighten a stroke (a list of x, y points) at the given sensitivity (see the module docs); 0 = as drawn.
pub fn smooth_path(pts: &[Pt], level: f64, k: f64) -> Vec<Pt> {
    if level <= 0.0 || pts.len() < 3 || k <= 0.0 {
        return pts.to_vec();
    }
    let scaled: Vec<Pt> = pts.iter().map(|p| [p[0] / k, p[1]]).collect();
    let q = dedupe(&scaled);
    if q.len() < 3 {
        return pts.to_vec();
    }
    let min_x = q.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let max_x = q.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let min_y = q.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let max_y = q.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let size = hypot2(max_x - min_x, max_y - min_y);
    if size < 1e-9 {
        return pts.to_vec();
    }
    let tol = tolerance(level, size);
    let gap = dist(q[0], q[q.len() - 1]);
    let mut out = None;
    if gap <= LOOP * size {
        out = shape_ladder(&q)
            .into_iter()
            .find(|(_, dev)| *dev <= tol)
            .map(|(shape, _)| shape);
    }
    let out = match out {
        Some(shape) => shape,
        None => pieces(&q, tol, gap <= tol),
    };
    out.iter().map(|p| [p[0] * k, p[1]]).collect()
}

// ---------------------------------------------------------------- perfect shapes (closed loops)

/// Category of a loop: round (circle / ellipse), boxy (square / rectangle), or another polygon.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShapeKind {
    Round,
    Box,
    Polygon(usize),
}

/// Shape ladder of a loop `[(shape, deviation)]`: simplest first, all of the loop's category. The
/// category is decided by the best fit, and each extra corner or setting costs a little. Independent of sensitivity.
fn shape_ladder(q: &[Pt]) -> Vec<(Vec<Pt>, f64)> {
    let mut closed = q.to_vec();
    closed.push(q[0]);
    let r = resample(&closed, SHAPE_SAMPLES);
    let samples: Vec<Pt> = r.iter().step_by(6).copied().collect();
    let mut size = 0.0f64;
    for a in &samples {
        for b in &samples {
            let d = dist(*a, *b);
            if d > size {
                size = d;
            }
        }
    }

    let ellipse = ellipse(&r);
    let box_shape = box_shape(&r);
    let mut polygons: [Option<Vec<Pt>>; MAX_POLYGON + 1] = Default::default();
    let mut kinds: Vec<(ShapeKind, f64, f64)> = Vec::new();
    if let Some(e) = &ellipse {
        kinds.push((ShapeKind::Round, dev(&r, e), 5.0));
    }
    if let Some(b) = &box_shape {
        kinds.push((
            ShapeKind::Box,
            dev(&r, &box_points(b.0, b.1, b.2, b.3)),
            5.0,
        ));
    }
    for (n, slot) in polygons.iter_mut().enumerate().skip(3) {
        let Some(p) = best_polygon(&r, n) else {
            continue;
        };
        if p.len() - 1 == n {
            kinds.push((ShapeKind::Polygon(n), dev(&r, &p), (2 * n) as f64));
            *slot = Some(p);
        }
    }
    let mut best: Option<(ShapeKind, f64)> = None;
    for (kind, dev, cost) in &kinds {
        let score = dev + CORNER_COST * size * cost;
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((*kind, score));
        }
    }
    let Some((kind, _)) = best else {
        return Vec::new();
    };
    let shapes: Vec<Option<Vec<Pt>>> = match kind {
        ShapeKind::Round => vec![Some(circle(&r)), ellipse],
        ShapeKind::Box | ShapeKind::Polygon(4) => vec![
            box_shape.map(|b| box_points(b.0, b.1, (b.2 + b.3) / 2.0, (b.2 + b.3) / 2.0)),
            box_shape.map(|b| box_points(b.0, b.1, b.2, b.3)),
            polygons[4].clone(),
        ],
        ShapeKind::Polygon(n) => vec![polygons[n].clone()],
    };
    shapes
        .into_iter()
        .flatten()
        .map(|shape| {
            let d = dev(&r, &shape);
            (shape, d)
        })
        .collect()
}

/// Circle: centre is the loop's mean, radius is the mean distance from the points to the centre.
fn circle(r: &[Pt]) -> Vec<Pt> {
    let c = mean(r);
    let rad = r.iter().map(|p| dist(*p, c)).sum::<f64>() / r.len() as f64;
    ellipse_points(c[0], c[1], rad, rad, 0.0)
}

/// Tilted ellipse: its axes follow the loop's principal directions, scaled so the loop fits on average. None when the points are not spread enough.
fn ellipse(r: &[Pt]) -> Option<Vec<Pt>> {
    let c = mean(r);
    let n = r.len() as f64;
    let sxx = r.iter().map(|p| (p[0] - c[0]).powf(2.0)).sum::<f64>() / n;
    let syy = r.iter().map(|p| (p[1] - c[1]).powf(2.0)).sum::<f64>() / n;
    let sxy = r.iter().map(|p| (p[0] - c[0]) * (p[1] - c[1])).sum::<f64>() / n;
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let half = hypot2((sxx - syy) / 2.0, sxy);
    let mid = (sxx + syy) / 2.0;
    let a = (2.0 * (mid + half)).sqrt();
    let b = (2.0 * (mid - half).max(0.0)).sqrt();
    if b < 1e-9 {
        return None;
    }
    let ca = angle.cos();
    let sa = angle.sin();
    let rho = r
        .iter()
        .map(|p| {
            hypot2(
                ((p[0] - c[0]) * ca + (p[1] - c[1]) * sa) / a,
                (-(p[0] - c[0]) * sa + (p[1] - c[1]) * ca) / b,
            )
        })
        .sum::<f64>()
        / n;
    Some(ellipse_points(c[0], c[1], a * rho, b * rho, angle))
}

/// n points on the ellipse, closed, starting at the leftmost point (like the other closed shapes).
fn ellipse_points(cx: f64, cy: f64, a: f64, b: f64, angle: f64) -> Vec<Pt> {
    let ca = angle.cos();
    let sa = angle.sin();
    let mut pts = Vec::with_capacity(96);
    for i in 0..96 {
        let t = 2.0 * std::f64::consts::PI * i as f64 / 96.0;
        let ct = t.cos();
        let st = t.sin();
        pts.push([
            cx + a * ct * ca - b * st * sa,
            cy + a * ct * sa + b * st * ca,
        ]);
    }
    closed_from_left(&pts)
}

/// Rectangle around a loop: rotate into the minimum bounding box direction, each side averaged over nearby points.
/// `(centre, direction, half width, half height)`, None when degenerate.
fn box_shape(r: &[Pt]) -> Option<(Pt, Pt, f64, f64)> {
    let hull = hull(r);
    let mut best: Option<(f64, Pt, [f64; 4])> = None;
    let hn = hull.len();
    for k in 0..hn {
        let a = hull[k];
        let b = hull[(k + 1) % hn];
        let d = dist(a, b);
        if d < 1e-12 {
            continue;
        }
        let ux = (b[0] - a[0]) / d;
        let uy = (b[1] - a[1]) / d;
        let mut min_u = f64::INFINITY;
        let mut max_u = f64::NEG_INFINITY;
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for p in &hull {
            let u = p[0] * ux + p[1] * uy;
            let v = -p[0] * uy + p[1] * ux;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
        let area = (max_u - min_u) * (max_v - min_v);
        let better = match best {
            None => true,
            Some((best_area, _, _)) => area < best_area,
        };
        if better {
            best = Some((area, [ux, uy], [min_u, max_u, min_v, max_v]));
        }
    }
    let (_, d, edges) = best?;
    let (ux, uy) = (d[0], d[1]);
    // left, right, bottom, top: the points nearest each side
    let mut sides: [Vec<f64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for p in r {
        let u = p[0] * ux + p[1] * uy;
        let v = -p[0] * uy + p[1] * ux;
        let ds = [u - edges[0], edges[1] - u, v - edges[2], edges[3] - v];
        let mut i = 0;
        for k in 1..4 {
            if ds[k] < ds[i] {
                i = k;
            }
        }
        sides[i].push(if i < 2 { u } else { v });
    }
    let mut vals = [0.0f64; 4];
    for (k, side) in sides.iter().enumerate() {
        vals[k] = if side.is_empty() {
            edges[k]
        } else {
            side.iter().sum::<f64>() / side.len() as f64
        };
    }
    let (u0, u1, v0, v1) = (vals[0], vals[1], vals[2], vals[3]);
    if u1 - u0 < 1e-9 || v1 - v0 < 1e-9 {
        return None;
    }
    let cu = (u0 + u1) / 2.0;
    let cv = (v0 + v1) / 2.0;
    Some((
        [cu * ux - cv * uy, cu * uy + cv * ux],
        [ux, uy],
        (u1 - u0) / 2.0,
        (v1 - v0) / 2.0,
    ))
}

/// The rectangle's four corners (closed, starting at the leftmost point).
fn box_points(c: Pt, d: Pt, hw: f64, hh: f64) -> Vec<Pt> {
    let (cx, cy) = (c[0], c[1]);
    let (ux, uy) = (d[0], d[1]);
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let pts: Vec<Pt> = corners
        .iter()
        .map(|&(su, sv)| {
            [
                cx + su * hw * ux - sv * hh * uy,
                cy + su * hw * uy + sv * hh * ux,
            ]
        })
        .collect();
    closed_from_left(&pts)
}

/// The polygon with at most n corners that fits the loop best: Ramer-Douglas-Peucker over a bisected
/// tolerance, keeping the tightest tolerance whose point count is at most n.
fn best_polygon(r: &[Pt], n: usize) -> Option<Vec<Pt>> {
    let loop0 = &r[..r.len() - 1];
    let c = mean(loop0);
    let mut i = 0;
    for (j, p) in loop0.iter().enumerate() {
        if dist(*p, c) > dist(loop0[i], c) {
            i = j;
        }
    }
    // Always start from the farthest point; that is certainly a corner
    let mut lp = Vec::with_capacity(loop0.len() + 1);
    lp.extend_from_slice(&loop0[i..]);
    lp.extend_from_slice(&loop0[..i]);
    lp.push(loop0[i]);
    let mut lo = 0.0f64;
    let mut hi = lp
        .iter()
        .map(|p| dist(lp[0], *p))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut best: Option<Vec<Pt>> = None;
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        let found = rdp(&lp, mid);
        let idx = &found[..found.len() - 1];
        if idx.len() > n {
            lo = mid;
        } else {
            hi = mid;
            if idx.len() >= 3 {
                best = Some(idx.iter().map(|&j| lp[j]).collect());
            }
        }
    }
    best.map(|b| closed_from_left(&b))
}

/// How far the shape is from the loop, in both directions (the farthest loop point to the shape, and the farthest shape point to the loop).
fn dev(r: &[Pt], shape: &[Pt]) -> f64 {
    let a = r
        .iter()
        .map(|p| dist_to_path(*p, shape))
        .fold(f64::NEG_INFINITY, f64::max);
    let b = resample(shape, 60)
        .iter()
        .map(|p| dist_to_path(*p, r))
        .fold(f64::NEG_INFINITY, f64::max);
    a.max(b)
}

/// A closed shape starting at the leftmost (then lowest) point, first and last joined.
fn closed_from_left(pts: &[Pt]) -> Vec<Pt> {
    let mut left = 0;
    for (i, p) in pts.iter().enumerate().skip(1) {
        if p[0] < pts[left][0] || (p[0] == pts[left][0] && p[1] < pts[left][1]) {
            left = i;
        }
    }
    let mut out = Vec::with_capacity(pts.len() + 1);
    out.extend_from_slice(&pts[left..]);
    out.extend_from_slice(&pts[..left]);
    out.push(pts[left]);
    out
}

// ---------------------------------------------------------------- lines and curves

/// Lines and smooth curves: cut at corners (sharp turns); each piece is a straight line if it hugs one
/// (within tol of it), otherwise it is fitted to a curve within tol. closed: the ends meet.
fn pieces(q: &[Pt], tol: f64, closed: bool) -> Vec<Pt> {
    let mut qq = q.to_vec();
    if closed && dist(qq[0], qq[qq.len() - 1]) > 1e-12 {
        qq.push(qq[0]);
    }
    let mut r = resample(&qq, PIECE_SAMPLES);
    let mut corners = corners(&r, tol, closed);
    if closed && !corners.is_empty() {
        // Start at a corner so there is no kink where it joins back to itself
        let c = corners[0];
        let mut rotated = Vec::with_capacity(r.len());
        rotated.extend_from_slice(&r[c..]);
        rotated.extend_from_slice(&r[1..c + 1]);
        r = rotated;
        corners = corners.iter().map(|&i| (i - c) % PIECE_SAMPLES).collect();
    }
    let mut cut = Vec::with_capacity(corners.len() + 2);
    cut.push(0);
    cut.extend(corners.iter().copied());
    cut.push(PIECE_SAMPLES);
    cut.sort_unstable();
    cut.dedup();
    let step = dist(r[0], r[1]);
    let mut out = vec![r[0]];
    for pair in cut.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let part = &r[a..b + 1];
        let last = part[part.len() - 1];
        let straight = part.len() < 3
            || part
                .iter()
                .map(|p| seg_dist(*p, part[0], last))
                .fold(f64::NEG_INFINITY, f64::max)
                <= tol;
        if straight {
            out.push(last);
            continue;
        }
        for seg in segments(&fit(part, tol)) {
            // Each curve segment gets roughly as many points as the original stroke had there
            let count = round_i64(path_len(&seg) / (step * 4.0).max(tol));
            let n = count.clamp(2, 24) as usize;
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
    }
    if closed {
        let last = out.len() - 1;
        out[last] = out[0];
    }
    out
}

/// Where an (evenly resampled) path turns sharply: the point indices. The span used for the
/// decision grows with the tolerance; jitter smaller than it does not count. A corner does most of
/// its turn at one point; a smooth bend (the crest of a wave) turns gradually and does not count.
fn corners(r: &[Pt], tol: f64, closed: bool) -> Vec<usize> {
    let n = r.len() - 1;
    let d = dist(r[0], r[1]);
    let step = if d == 0.0 { 1e-12 } else { d };
    let m = 3.max(round_i64((4.0 * tol).max(0.03 * step * n as f64) / step)) as usize;
    if !closed && 2 * m >= n {
        return Vec::new();
    }
    let idx: Vec<usize> = if closed {
        (0..n).collect()
    } else {
        (m..=n - m).collect()
    };
    let at = |i: isize| -> Pt {
        if closed {
            r[i.rem_euclid(n as isize) as usize]
        } else {
            r[i as usize]
        }
    };
    let mut turns = vec![0.0f64; n];
    for &i in &idx {
        turns[i] = turn(
            at(i as isize - m as isize),
            at(i as isize),
            at(i as isize + m as isize),
        );
    }
    let mut out = Vec::new();
    for &i in &idx {
        let t = turns[i];
        let third = (m / 3) as isize;
        if t <= CORNER
            || turn(
                at(i as isize - third),
                at(i as isize),
                at(i as isize + third),
            ) < 0.6 * t
        {
            continue;
        }
        let mut sharpest = true;
        for j in (i as isize - m as isize)..=(i as isize + m as isize) {
            if j == i as isize {
                continue;
            }
            let jj = if closed {
                j.rem_euclid(n as isize) as usize
            } else {
                j as usize
            };
            let other = if jj < turns.len() { turns[jj] } else { 0.0 };
            if !(t > other || (t == other && i < jj)) {
                sharpest = false;
                break;
            }
        }
        if sharpest {
            out.push(i);
        }
    }
    out
}

// ---------------------------------------------------------------- helpers

/// Drop points that almost coincide with the previous one (within 1e-9).
fn dedupe(pts: &[Pt]) -> Vec<Pt> {
    let mut out = vec![pts[0]];
    for &p in &pts[1..] {
        if dist(p, out[out.len() - 1]) > 1e-9 {
            out.push(p);
        }
    }
    out
}

/// Mean position of the points.
fn mean(pts: &[Pt]) -> Pt {
    let n = pts.len() as f64;
    let sx: f64 = pts.iter().map(|p| p[0]).sum();
    let sy: f64 = pts.iter().map(|p| p[1]).sum();
    [sx / n, sy / n]
}

/// Total length of the polyline.
fn path_len(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// n + 1 evenly spaced points along the path (including both ends).
fn resample(pts: &[Pt], n: usize) -> Vec<Pt> {
    if pts.is_empty() {
        return Vec::new();
    }
    if pts.len() < 2 {
        return vec![pts[0]; n + 1];
    }
    let mut lens = vec![0.0];
    for w in pts.windows(2) {
        let last = lens[lens.len() - 1];
        lens.push(last + dist(w[0], w[1]));
    }
    let total = lens[lens.len() - 1];
    if total < 1e-12 {
        return vec![pts[0]; n + 1];
    }
    let mut out = Vec::with_capacity(n + 1);
    let mut j = 0;
    for i in 0..=n {
        let s = total * i as f64 / n as f64;
        while j < lens.len() - 2 && lens[j + 1] < s {
            j += 1;
        }
        let seg = lens[j + 1] - lens[j];
        let t = if seg < 1e-12 {
            0.0
        } else {
            (s - lens[j]) / seg
        };
        let (ax, ay) = (pts[j][0], pts[j][1]);
        let (bx, by) = (pts[j + 1][0], pts[j + 1][1]);
        out.push([ax + (bx - ax) * t, ay + (by - ay) * t]);
    }
    out
}

/// Distance from a point to segment ab.
fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let ll = dx * dx + dy * dy;
    let t = if ll < 1e-24 {
        0.0
    } else {
        min_max(((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / ll, 0.0, 1.0)
    };
    hypot2(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)
}

/// Distance from a point to a polyline (path).
fn dist_to_path(p: Pt, path: &[Pt]) -> f64 {
    path.windows(2)
        .map(|w| seg_dist(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// How sharply the path turns at b (0 = straight ahead, pi = doubling back).
fn turn(a: Pt, b: Pt, c: Pt) -> f64 {
    let d1 = [b[0] - a[0], b[1] - a[1]];
    let d2 = [c[0] - b[0], c[1] - b[1]];
    let l1 = hypot2(d1[0], d1[1]);
    let l2 = hypot2(d2[0], d2[1]);
    if l1 < 1e-12 || l2 < 1e-12 {
        return 0.0;
    }
    min_max((d1[0] * d2[0] + d1[1] * d2[1]) / (l1 * l2), -1.0, 1.0).acos()
}

/// Python's `max(lo, min(hi, x))`: with NaN, Python's comparison picks hi.
fn min_max(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() { hi } else { x.clamp(lo, hi) }
}

/// Ramer-Douglas-Peucker: indices of the points that keep the path within tol (first and last are always kept).
fn rdp(pts: &[Pt], tol: f64) -> Vec<usize> {
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut worst = -1.0f64;
        let mut at: Option<usize> = None;
        for i in a + 1..b {
            let d = seg_dist(pts[i], pts[a], pts[b]);
            if d > worst {
                worst = d;
                at = Some(i);
            }
        }
        if let Some(at) = at
            && worst > tol
        {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    keep.iter()
        .enumerate()
        .filter(|&(_, &k)| k)
        .map(|(i, _)| i)
        .collect()
}

/// Convex hull (Andrew's monotone chain), counter-clockwise.
fn hull(pts: &[Pt]) -> Vec<Pt> {
    let mut sorted = pts.to_vec();
    sorted.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
    });
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |o: Pt, a: Pt, b: Pt| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<Pt> = Vec::new();
    for &p in &sorted {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Pt> = Vec::new();
    for &p in sorted.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}
