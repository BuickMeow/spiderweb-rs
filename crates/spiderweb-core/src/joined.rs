//! Joining lines, polylines, freehand strokes, curves and arcs into one Curve shape
//! (port of Python `notes/joined.py`).
//!
//! Every shape becomes a Bézier curve first ([`crate::bezier`]); the curves are then chained end to end, each
//! next one attached to the nearer end of the chain (turned round when that is nearer). Ends that touch (or
//! that `touch` counts as touching) are joined into one point with a corner there; shapes that don't touch stay
//! another piece of the same curve with a gap between them that isn't drawn and makes no notes
//! (`Shape::gaps`, `piece_ends` in [`crate::bezier`]).
//!
//! Tumours: every joined shape keeps its own settings, so joining moves no bumps: `Shape::tumours` has one
//! setting (or None) per joined shape in chain order, `Shape::splits` are the anchors where a new section starts
//! inside a piece ([`set_tumours`]). Only when every piece is one whole shape and they all had the same tumour
//! settings (or none) does the curve get one setting (each piece gets its own row of bumps anyway). Changing any
//! tumour setting of the joined curve gives it one setting for all (then "tumours" / "splits" are gone): the
//! bumps run on across the joints.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::arc::{arc_bezier, line_bezier};
use crate::bezier::{self, anchor_count, fit, sample};
use crate::envelope::{env_values, velocity_env};
use crate::shape::{Kind, Shape, Stroke, Tumour};
use crate::smooth::smooth_path;
use crate::tumour::{clean_tumour, split_tumour, tumour_path};
use crate::{Pt, dist, round_half_even, round_i64};

/// How closely a freehand stroke's curve follows it, in keys (as the piano roll looks) (joined.FREE_TOLERANCE).
pub const FREE_TOLERANCE: f64 = 0.08;

/// The shape kinds that can be joined (tumour.LINE_KINDS).
pub const LINE_KINDS: [Kind; 5] = [Kind::Line, Kind::Poly, Kind::Free, Kind::Curve, Kind::Arc];

/// The shape as a Bézier curve (points, sharp anchor numbers); `k` = beats per key on screen
/// (for fitting freehand strokes).
pub fn to_bezier(sh: &Shape, k: f64) -> (Vec<Pt>, Vec<usize>) {
    match sh.kind {
        Kind::Curve => (sh.pts.clone(), sh.sharp.clone()),
        Kind::Arc => (arc_bezier(&sh.pts, sh.k), Vec::new()),
        Kind::Free => {
            let mut pts = sh.pts.clone();
            if sh.smooth != 0 {
                pts = smooth_path(&pts, sh.smooth as f64, sh.k);
            }
            if pts.len() < 2
                && let Some(&p) = pts.first()
            {
                pts = vec![p, p];
            }
            let scaled: Vec<Pt> = pts.iter().map(|p| [p[0] / k, p[1]]).collect();
            let got = fit(&scaled, FREE_TOLERANCE);
            (got.iter().map(|p| [p[0] * k, p[1]]).collect(), Vec::new())
        }
        // line / polyline: straight pieces, corners at every point
        _ => {
            let mut out = match (sh.pts.first(), sh.pts.get(1)) {
                (Some(&a), Some(&b)) => line_bezier(a, b),
                _ => Vec::new(),
            };
            for w in sh.pts.windows(2).skip(1) {
                let mut seg = line_bezier(w[0], w[1]);
                seg.remove(0);
                out.extend(seg);
            }
            (out, (1..sh.pts.len().saturating_sub(1)).collect())
        }
    }
}

/// The curve the other way round (anchors renumbered too) (Python `reversed_bezier`).
pub fn reversed_bezier(pts: &[Pt], sharp: &[usize]) -> (Vec<Pt>, Vec<usize>) {
    let last = anchor_count(pts) as i64 - 1;
    let pts = pts.iter().rev().copied().collect();
    let mut sharp: Vec<usize> = sharp.iter().map(|&a| (last - a as i64) as usize).collect();
    sharp.sort_unstable();
    (pts, sharp)
}

/// Tumour settings for the line run the other way round: same bumps (sides, range and graphs turned round)
/// (Python `reversed_tumour`; graph entries aren't in [`Tumour`] yet).
pub fn reversed_tumour(tm: &Tumour) -> Tumour {
    let mut out = tm.clone();
    out.mirror = !out.mirror;
    let (s, e) = (out.start, out.end);
    out.start = 1.0 - e;
    out.end = 1.0 - s;
    out
}

/// One piece of the chain (points, sharp anchors, tumour settings).
struct Part {
    pts: Vec<Pt>,
    sharp: Vec<usize>,
    tm: Option<Tumour>,
}

/// One Curve shape from the shapes (in the order they are listed; the first one's velocity and last-note
/// settings). `touch(p, q)`: whether two ends count as touching. None if fewer than two can be joined.
pub fn join_shapes(shapes: &[Shape], k: f64, touch: &dyn Fn(Pt, Pt) -> bool) -> Option<Shape> {
    let shapes: Vec<&Shape> = shapes
        .iter()
        .filter(|sh| LINE_KINDS.contains(&sh.kind))
        .collect();
    if shapes.len() < 2 {
        return None;
    }
    let mut parts: Vec<Part> = Vec::with_capacity(shapes.len());
    for sh in &shapes {
        let (pts, sharp) = to_bezier(sh, k);
        parts.push(Part {
            pts,
            sharp,
            tm: sh.tumour.clone(),
        });
    }
    let mut chain: Vec<Part> = vec![parts.remove(0)];
    let mut rest = parts;
    while !rest.is_empty() {
        // the nearest end to either end of the chain (turned round if that's nearer)
        let head = chain[0].pts[0];
        let tail = chain[chain.len() - 1].pts[chain[chain.len() - 1].pts.len() - 1];
        let mut best: Option<(f64, usize, bool, bool)> = None;
        for (i, p) in rest.iter().enumerate() {
            let a = p.pts[0];
            let b = p.pts[p.pts.len() - 1];
            for (d, at_end, flip) in [
                (dist(tail, a), true, false),
                (dist(tail, b), true, true),
                (dist(head, b), false, false),
                (dist(head, a), false, true),
            ] {
                if best.is_none() || d < best.unwrap().0 - 1e-12 {
                    best = Some((d, i, at_end, flip));
                }
            }
        }
        let Some((_, i, at_end, flip)) = best else {
            break;
        };
        let mut p = rest.remove(i);
        if flip {
            let (pts, sharp) = reversed_bezier(&p.pts, &p.sharp);
            p.pts = pts;
            p.sharp = sharp;
            p.tm = p.tm.as_ref().map(reversed_tumour);
        }
        if at_end {
            chain.push(p);
        } else {
            chain.insert(0, p);
        }
    }
    // put the chain together: touching ends become one point (the later piece moves onto it), others get a gap
    let mut pts: Vec<Pt> = chain[0].pts.clone();
    let mut sharp: Vec<usize> = chain[0].sharp.clone();
    let mut gaps: Vec<usize> = Vec::new();
    let mut splits: Vec<usize> = Vec::new();
    for p in &chain[1..] {
        let last = anchor_count(&pts) as i64 - 1;
        let mut q = p.pts.clone();
        let s = &p.sharp;
        if touch(pts[pts.len() - 1], q[0]) {
            let d = [
                pts[pts.len() - 1][0] - q[0][0],
                pts[pts.len() - 1][1] - q[0][1],
            ];
            for item in q.iter_mut().take(2) {
                item[0] += d[0];
                item[1] += d[1];
            }
            pts.extend_from_slice(&q[1..]);
            sharp.push(last as usize);
            splits.push(last as usize);
            sharp.extend(s.iter().map(|&a| a + last as usize));
        } else {
            gaps.push(last as usize);
            let mut bridge = line_bezier(pts[pts.len() - 1], q[0]);
            bridge.drain(0..1);
            bridge.truncate(2);
            pts.extend(bridge);
            pts.extend(q.iter().copied());
            sharp.extend(s.iter().map(|&a| a + last as usize + 1));
        }
    }
    let first = shapes[0];
    let mut out = Shape::new(Kind::Curve, pts);
    out.vel0 = first.vel0;
    out.vel1 = first.vel1;
    out.end_dot = first.end_dot;
    out.vel_env = first.vel_env.clone();
    let mut ends: BTreeSet<usize> = [0, anchor_count(&out.pts).saturating_sub(1)]
        .into_iter()
        .collect();
    for &g in &gaps {
        ends.insert(g);
        ends.insert(g + 1);
    }
    let mut sharp: Vec<usize> = sharp.into_iter().filter(|a| !ends.contains(a)).collect();
    sharp.sort_unstable();
    sharp.dedup();
    out.sharp = sharp;
    out.gaps = gaps;
    let tms: Vec<Option<Tumour>> = chain.iter().map(|p| p.tm.clone()).collect();
    set_tumours(&mut out, &tms, &splits);
    Some(out)
}

// ---------------------------------------------------------------- cleaning

/// A `gaps` / `splits` list from a file: values that `int(x)` works on (strings char by char).
/// `None` gives an empty list; an explicit null or a value that won't convert fails the whole read
/// (Python swallows the exception).
fn py_int_list(v: Option<&Value>) -> Option<Vec<i64>> {
    match v {
        None => Some(Vec::new()),
        Some(Value::Null) => None,
        Some(Value::Array(a)) => a.iter().map(value_int).collect(),
        Some(Value::String(s)) => s
            .chars()
            .map(|c| c.to_digit(10).map(|d| d as i64).ok_or(()))
            .collect::<Result<Vec<_>, ()>>()
            .ok(),
        Some(_) => None,
    }
}

/// Python `int(x)`: truncating numbers, booleans and integer strings.
fn value_int(v: &Value) -> Option<i64> {
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

/// A joined curve's gaps / splits / tumours from a file into `out` (a cleaned curve); anything that doesn't
/// fit the curve is dropped (Python `clean_joined`).
pub fn clean_joined(sh: &Map<String, Value>, out: &mut Shape) {
    let last = anchor_count(&out.pts).saturating_sub(1) as i64;
    let Some(want) = py_int_list(sh.get("gaps")) else {
        return;
    };
    let Some(raw_splits) = py_int_list(sh.get("splits")) else {
        return;
    };
    let want: Vec<i64> = want
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut splits: Vec<usize> = raw_splits
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|&a| 0 < a && a < last)
        .map(|a| a as usize)
        .collect();
    // (every piece needs at least one segment of its own)
    let mut gaps: Vec<usize> = Vec::new();
    for g in want {
        if (1..=last - 2).contains(&g) && (gaps.is_empty() || g >= *gaps.last().unwrap() as i64 + 2)
        {
            gaps.push(g as usize);
        }
    }
    splits.retain(|&a| !gaps.contains(&a) && !gaps.contains(&(a - 1)));
    if !gaps.is_empty() {
        out.gaps = gaps.clone();
    }
    let Some(tms) = sh.get("tumours").and_then(Value::as_array) else {
        return;
    };
    if tms.len() == splits.len() + gaps.len() + 1 {
        out.tumours = tms.iter().map(clean_tumour).collect();
        if !splits.is_empty() {
            out.splits = splits;
        }
        out.tumour = None;
    }
}

/// Whether the shape is a joined curve (it has gaps or per-piece tumours) (Python `is_joined`).
pub fn is_joined(sh: &Shape) -> bool {
    sh.kind == Kind::Curve && (!sh.gaps.is_empty() || !sh.tumours.is_empty())
}

// ---------------------------------------------------------------- per-piece paths and tumours

/// The joined curve as one path per piece, with its tumours (`tumour_path` puts them on).
pub fn joined_paths(sh: &Shape) -> Vec<Vec<Pt>> {
    let pts = &sh.pts;
    let gaps = &sh.gaps;
    let tms = &sh.tumours;
    let splits = if tms.is_empty() {
        &[][..]
    } else {
        &sh.splits[..]
    };
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut a0 = 0usize;
    for &g in gaps {
        pieces.push((a0, g));
        a0 = g + 1;
    }
    pieces.push((a0, anchor_count(pts).saturating_sub(1)));
    let mut out: Vec<Vec<Pt>> = Vec::new();
    let mut section = 0usize;
    for &(p0, p1) in &pieces {
        if tms.is_empty() {
            let path = sample(&pts[3 * p0..3 * p1 + 1], 240);
            let drawn = match &sh.tumour {
                Some(tm) if tm.on => tumour_path(&path, tm),
                _ => path,
            };
            out.push(drawn);
            continue;
        }
        let mut bounds = vec![p0];
        bounds.extend(splits.iter().copied().filter(|&a| p0 < a && a < p1));
        bounds.push(p1);
        let mut path: Vec<Pt> = Vec::new();
        for w in bounds.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mut part = sample(&pts[3 * a..3 * b + 1], 240);
            let tm = tms.get(section);
            section += 1;
            if let Some(Some(t)) = tm
                && t.on
            {
                part = tumour_path(&part, t);
            }
            if !path.is_empty() && !part.is_empty() && part[0] == path[path.len() - 1] {
                path.extend_from_slice(&part[1..]);
            } else {
                path.extend(part);
            }
        }
        out.push(path);
    }
    out
}

/// Every tumour setting on the shape (a joined curve can have one per joined shape), to flip / turn them.
pub fn all_tumours(sh: &Shape) -> Vec<&Tumour> {
    sh.tumour
        .iter()
        .chain(sh.tumours.iter().flatten())
        .collect()
}

/// [`all_tumours`] mutably (flipping / turning a joined curve's bumps too).
pub fn all_tumours_mut(sh: &mut Shape) -> Vec<&mut Tumour> {
    sh.tumour
        .iter_mut()
        .chain(sh.tumours.iter_mut().flatten())
        .collect()
}

/// The tumour settings the tumour window shows: the shape's own, or on a joined curve whose shapes kept
/// their own ones, the first one there is (Python `shown_tumour`).
pub fn shown_tumour(sh: &Shape) -> Option<&Tumour> {
    if !sh.tumours.is_empty() {
        return sh.tumours.iter().find_map(|t| t.as_ref());
    }
    sh.tumour.as_ref()
}

/// A joined curve whose shapes kept their own tumours: from now on the shown one is the whole curve's
/// (Python `unify_tumours`).
pub fn unify_tumours(sh: &mut Shape) {
    if sh.tumours.is_empty() {
        return;
    }
    let tm = shown_tumour(sh).cloned();
    sh.tumours.clear();
    sh.splits.clear();
    sh.tumour = tm;
}

// ---------------------------------------------------------------- splitting

/// Anchors a0..a1 of a curve as a curve of their own (a copy of sh's other settings, tumour settings tm).
fn cut(sh: &Shape, a0: usize, a1: usize, tm: Option<&Tumour>) -> Shape {
    let mut out = sh.clone();
    out.pts = sh.pts[3 * a0..3 * a1 + 1].to_vec();
    out.sharp = Vec::new();
    out.sym = None;
    out.gaps = sh
        .gaps
        .iter()
        .copied()
        .filter(|&g| a0 <= g && g < a1)
        .map(|g| g - a0)
        .collect();
    out.splits = Vec::new();
    out.tumours = Vec::new();
    out.tumour = None;
    let sharp: Vec<usize> = sh
        .sharp
        .iter()
        .copied()
        .filter(|&a| a0 < a && a < a1)
        .map(|a| a - a0)
        .collect();
    if !sharp.is_empty() {
        out.sharp = sharp;
    }
    if let Some(t) = tm {
        out.tumour = Some(t.clone());
    }
    out
}

/// (first anchor, last anchor, tumour settings) of a curve: its pieces, and inside them the joined shapes
/// that kept their own tumours.
pub fn sections(sh: &Shape) -> Vec<(usize, usize, Option<Tumour>)> {
    let gaps = &sh.gaps;
    let tms = &sh.tumours;
    let splits = if tms.is_empty() {
        &[][..]
    } else {
        &sh.splits[..]
    };
    let last = anchor_count(&sh.pts).saturating_sub(1);
    let mut bounds: BTreeSet<usize> = [0, last].into_iter().collect();
    bounds.extend(gaps.iter().copied());
    bounds.extend(gaps.iter().map(|&g| g + 1));
    bounds.extend(splits.iter().copied());
    let bounds: Vec<usize> = bounds.into_iter().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        if gaps.contains(&a) {
            continue; // (a gap, not a piece)
        }
        let tm = if tms.is_empty() {
            sh.tumour.clone()
        } else {
            tms.get(i).cloned().flatten()
        };
        out.push((a, b, tm));
        i += 1;
    }
    out
}

/// A joined curve back as one curve per piece (a piece whose shapes kept their own tumours: one per shape).
pub fn split_pieces(sh: &Shape) -> Vec<Shape> {
    sections(sh)
        .into_iter()
        .map(|(a, b, tm)| cut(sh, a, b, tm.as_ref()))
        .collect()
}

/// Give a curve one tumour setting per section (`tms`, in order; `splits` = where sections start inside a
/// piece). When they're all the same and every section is a whole piece, that's one setting for the curve
/// (each piece gets its own row of bumps either way, so it looks the same).
pub fn set_tumours(sh: &mut Shape, tms: &[Option<Tumour>], splits: &[usize]) {
    sh.tumour = None;
    sh.tumours.clear();
    sh.splits.clear();
    if tms.iter().all(|t| t.is_none()) {
        return;
    }
    let uniform = tms.windows(2).all(|w| w[0] == w[1]);
    if splits.is_empty() && uniform {
        if let Some(t) = &tms[0] {
            sh.tumour = Some(t.clone());
        }
        return;
    }
    sh.tumours = tms.to_vec();
    if !splits.is_empty() {
        sh.splits = splits.to_vec();
    }
}

/// A curve cut in two at anchor a (a joined curve's tumours go with their sections; the section that's cut is
/// shared out so its bumps stay where they were, see tumour.split_tumour). None if a is an end of a piece.
pub fn split_at(sh: &Shape, a: usize) -> Option<(Shape, Shape)> {
    let ends = bezier::piece_ends(&bezier::Curve {
        pts: sh.pts.clone(),
        gaps: sh.gaps.clone(),
        ..Default::default()
    });
    if ends.contains(&a) {
        return None;
    }
    let left = cut(sh, 0, a, None);
    let right = cut(sh, a, anchor_count(&sh.pts).saturating_sub(1), None);
    let mut tl: Vec<Option<Tumour>> = Vec::new();
    let mut tr: Vec<Option<Tumour>> = Vec::new();
    for (s0, s1, tm) in sections(sh) {
        if s1 <= a {
            tl.push(tm);
        } else if s0 >= a {
            tr.push(tm);
        } else {
            let (l, r) = match &tm {
                Some(t) => {
                    let lp = sample(&sh.pts[3 * s0..3 * a + 1], 240);
                    let rp = sample(&sh.pts[3 * a..3 * s1 + 1], 240);
                    split_tumour(t, &lp, &rp)
                }
                None => (None, None),
            };
            tl.push(l);
            tr.push(r);
        }
    }
    let splits = if sh.tumours.is_empty() {
        Vec::new()
    } else {
        sh.splits.clone()
    };
    let mut left = left;
    let mut right = right;
    set_tumours(
        &mut left,
        &tl,
        &splits
            .iter()
            .copied()
            .filter(|&s| s < a)
            .collect::<Vec<_>>(),
    );
    set_tumours(
        &mut right,
        &tr,
        &splits
            .iter()
            .copied()
            .filter(|&s| s > a)
            .map(|s| s - a)
            .collect::<Vec<_>>(),
    );
    Some((left, right))
}

// ---------------------------------------------------------------- velocity

/// Give `new` (a part of `old`) the velocity it had as part of `old`: its own envelope over its own time span
/// (spans: (earliest, latest) beat) (Python `piece_velocity`).
pub fn piece_velocity(new: &mut Shape, old: &Shape, new_span: (f64, f64), old_span: (f64, f64)) {
    let env = velocity_env(old);
    let ((n0, n1), (o0, o1)) = (new_span, old_span);
    if o1 - o0 < 1e-12 || n1 - n0 < 1e-12 {
        return;
    }
    let ua = (n0 - o0) / (o1 - o0);
    let ub = (n1 - o0) / (o1 - o0);
    let inner: Vec<Pt> = env
        .iter()
        .filter(|p| ua < p[0] && p[0] < ub)
        .map(|p| [(p[0] - ua) / (ub - ua), p[1]])
        .collect();
    let got = env_values(&env, &[ua, ub]);
    let (va, vb) = (got[0], got[1]);
    new.vel_env.clear();
    new.vel0 = round_i64(va) as f64;
    new.vel1 = round_i64(vb) as f64;
    if !inner.is_empty() {
        let mut e = vec![[0.0, va]];
        e.extend(inner);
        e.push([1.0, vb]);
        new.vel_env = e;
    } else if (va - round_half_even(va)).abs() > 1e-9 || (vb - round_half_even(vb)).abs() > 1e-9 {
        new.vel_env = vec![[0.0, va], [1.0, vb]];
    }
}

/// Give `new` (olds joined) the velocities its parts had: one envelope over its time span made of each old
/// shape's own, over the time that shape covered (spans: (earliest, latest) beat of each). Where shapes cover
/// the same time, the one listed first wins; in time between shapes the velocity goes straight from one to
/// the next (Python `join_velocity`).
pub fn join_velocity(new: &mut Shape, olds: &[Shape], spans: &[(f64, f64)], new_span: (f64, f64)) {
    let envs: Vec<Vec<Pt>> = olds.iter().map(velocity_env).collect();
    let (t0, t1) = new_span;
    if t1 - t0 < 1e-12 {
        return;
    }
    let value = |i: usize, t: f64| -> f64 {
        let (a, b) = spans[i];
        let u = if b - a < 1e-12 {
            0.0
        } else {
            (t - a) / (b - a)
        };
        env_values(&envs[i], &[u])[0]
    };
    let mut times: Vec<f64> = Vec::new();
    for &(a, b) in spans {
        times.push(a);
        times.push(b);
    }
    for ((a, b), env) in spans.iter().zip(envs.iter()) {
        for p in env {
            times.push(a + (b - a) * p[0]);
        }
    }
    times.sort_by(|x, y| x.total_cmp(y));
    times.dedup();
    let mut pts: Vec<Pt> = Vec::new();
    for w in times.windows(2) {
        let (ta, tb) = (w[0], w[1]);
        let m = (ta + tb) / 2.0;
        let owner = spans
            .iter()
            .position(|&(a, b)| a - 1e-12 <= m && m <= b + 1e-12);
        let Some(owner) = owner else {
            continue; // (no shape here: straight on to the next one)
        };
        for t in [ta, tb] {
            let p = [(t - t0) / (t1 - t0), value(owner, t)];
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        }
    }
    if pts.is_empty() {
        return;
    }
    let pts: Vec<Pt> = pts
        .into_iter()
        .map(|p| [p[0].clamp(0.0, 1.0), p[1]])
        .collect();
    // (points on a straight stretch aren't needed)
    let mut keep: Vec<Pt> = vec![pts[0]];
    for w in pts.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let straight = b[0] - a[0] > 1e-12
            && c[0] - b[0] > 1e-12
            && ((b[1] - a[1]) / (b[0] - a[0]) - (c[1] - b[1]) / (c[0] - b[0])).abs() < 1e-9;
        if !straight {
            keep.push(b);
        }
    }
    keep.push(pts[pts.len() - 1]);
    if keep[0][0] > 1e-12 {
        keep.insert(0, [0.0, keep[0][1]]);
    }
    if keep[keep.len() - 1][0] < 1.0 - 1e-12 {
        keep.push([1.0, keep[keep.len() - 1][1]]);
    }
    new.vel_env.clear();
    new.vel0 = round_i64(keep[0][1]) as f64;
    new.vel1 = round_i64(keep[keep.len() - 1][1]) as f64;
    let straight = keep.len() == 2
        && keep
            .iter()
            .all(|p| (p[1] - round_half_even(p[1])).abs() < 1e-9);
    if !straight {
        new.vel_env = keep;
    }
}

// ---------------------------------------------------------------- custom shapes

/// Coordinates rounded to 6 decimals as an integer key (Python `round(c, 6)`).
fn key6(c: f64) -> i64 {
    round_half_even(c * 1e6) as i64
}

/// A custom shape's strokes in groups that touch each other (end on end or on a point), as lists of stroke
/// numbers; one group = nothing to split (Python `custom_groups`).
pub fn custom_groups(sh: &Shape) -> Vec<Vec<usize>> {
    let strokes = &sh.strokes;
    let pts: Vec<Vec<[i64; 2]>> = strokes
        .iter()
        .map(|st| {
            crate::custom::stroke_points(st)
                .iter()
                .map(|p| [key6(p[0]), key6(p[1])])
                .collect()
        })
        .collect();
    let ends: Vec<Vec<[i64; 2]>> = strokes
        .iter()
        .zip(pts.iter())
        .map(|(st, p)| {
            if p.is_empty() {
                Vec::new()
            } else if matches!(st, Stroke::Poly { .. }) {
                p.clone()
            } else {
                vec![p[0], p[p.len() - 1]]
            }
        })
        .collect();
    let n = strokes.len();
    let mut group: Vec<usize> = (0..n).collect();

    fn root(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let shares = ends[i].iter().any(|e| pts[j].contains(e))
                || ends[j].iter().any(|e| pts[i].contains(e));
            if shares {
                let (ri, rj) = (root(&mut group, i), root(&mut group, j));
                group[ri] = rj;
            }
        }
    }
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut at: Vec<(usize, usize)> = Vec::new(); // (root, out index)
    for i in 0..n {
        let r = root(&mut group, i);
        match at.iter().find(|(rr, _)| *rr == r) {
            Some(&(_, k)) => out[k].push(i),
            None => {
                at.push((r, out.len()));
                out.push(vec![i]);
            }
        }
    }
    out
}

/// A custom shape (e.g. drawn with Live shape) as one custom shape per group of touching strokes.
pub fn split_custom(sh: &Shape) -> Vec<Shape> {
    let mut out = Vec::new();
    for g in custom_groups(sh) {
        let mut new = sh.clone();
        new.strokes = g.iter().map(|&i| sh.strokes[i].clone()).collect();
        crate::custom::refit(&mut new);
        out.push(new);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: Pt, b: Pt) -> Shape {
        Shape::new(Kind::Line, vec![a, b])
    }

    fn touching(p: Pt, q: Pt) -> bool {
        dist(p, q) <= 1e-9
    }

    #[test]
    fn joins_two_touching_lines_into_one_curve() {
        let shapes = vec![
            line([0.0, 60.0], [4.0, 60.0]),
            line([4.0, 60.0], [8.0, 64.0]),
        ];
        let got = join_shapes(&shapes, 1.0, &touching).expect("joins");
        assert_eq!(got.kind, Kind::Curve);
        assert_eq!(got.gaps, Vec::<usize>::new());
        // the joint is a corner (anchor 1), not an end of the curve
        assert!(got.sharp.contains(&1));
        let notes = crate::engine::shape_notes(&got, 960.0, 128);
        assert!(!notes.is_empty());
    }

    #[test]
    fn far_apart_shapes_get_a_gap() {
        let shapes = vec![
            line([0.0, 60.0], [4.0, 60.0]),
            line([6.0, 60.0], [10.0, 60.0]),
        ];
        let got = join_shapes(&shapes, 1.0, &touching).expect("joins");
        assert_eq!(got.gaps, vec![1]);
        let paths = joined_paths(&got);
        assert_eq!(paths.len(), 2);
        assert!(!is_joined(&Shape::new(Kind::Curve, got.pts.clone())));
        assert!(is_joined(&got));
    }

    #[test]
    fn reversed_tumour_swaps_range_and_side() {
        let tm = Tumour {
            mirror: false,
            start: 0.25,
            end: 0.75,
            ..Tumour::default()
        };
        let r = reversed_tumour(&tm);
        assert!(r.mirror);
        assert!((r.start - 0.25).abs() < 1e-12);
        assert!((r.end - 0.75).abs() < 1e-12);
    }

    #[test]
    fn clean_joined_drops_marks_that_do_not_fit() {
        let mut out = Shape::new(
            Kind::Curve,
            vec![
                [0.0, 60.0],
                [1.0, 60.0],
                [2.0, 60.0],
                [3.0, 60.0],
                [4.0, 60.0],
                [5.0, 60.0],
                [6.0, 60.0],
                [7.0, 60.0],
                [8.0, 60.0],
                [9.0, 60.0],
            ],
        );
        let sh: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "gaps": [1, 999, -2, 1],
            "splits": [0, 2, 2, 99],
            "tumours": [null, {"on": true}],
        }))
        .unwrap();
        clean_joined(&sh, &mut out);
        assert_eq!(out.gaps, vec![1]);
        assert!(out.splits.is_empty());
        assert_eq!(out.tumours.len(), 2);
        assert!(out.tumours[0].is_none());
        assert!(out.tumours[1].as_ref().unwrap().on);
        let _ = clean_tumour(&Value::Null);
    }
}
