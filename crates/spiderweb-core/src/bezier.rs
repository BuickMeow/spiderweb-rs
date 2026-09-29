//! Bezier curves: curves with anchors and handles (the pen tool), a function-by-function port of Python notes/bezier.py:
//! sampling, adding / removing anchors, pen editing, symmetric curves, and fitting a point list with as few anchors as possible within a tolerance.
//!
//! A curve is a flat point list `[anchor, handle, handle, anchor, handle, handle, anchor, ...]`:
//! every third point (0, 3, 6, ...) is an anchor the curve passes through, and between two anchors are two handles:
//! the outgoing handle of the previous anchor and the incoming handle of the next anchor.

use std::collections::BTreeSet;

use crate::shape::Sym;
use crate::{Pt, dist, hypot2};

/// A curve: `pts` is the flat point list, `sharp` the corner anchor numbers, `sym` the symmetry mode.
/// A joined curve (joined.rs) can also carry `gaps` (segment numbers that aren't drawn: the curve is in
/// pieces) and `splits` (anchor numbers where a piece's next tumour section starts); their anchors stay,
/// like the ends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub pts: Vec<Pt>,
    pub sharp: Vec<usize>,
    pub sym: Option<Sym>,
    pub gaps: Vec<usize>,
    pub splits: Vec<usize>,
}

impl Curve {
    pub fn new(pts: Vec<Pt>) -> Self {
        Self {
            pts,
            ..Self::default()
        }
    }

    /// Number of anchors.
    pub fn anchor_count(&self) -> usize {
        anchor_count(&self.pts)
    }
}

/// Number of anchors: decided by the point list length (corresponds to Python `anchor_count`).
pub fn anchor_count(pts: &[Pt]) -> usize {
    if pts.is_empty() {
        0
    } else {
        (pts.len() - 1) / 3 + 1
    }
}

/// A point on one cubic Bezier segment.
pub fn seg_point(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f64) -> Pt {
    let mt = 1.0 - t;
    let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
    [
        a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
        a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
    ]
}

/// The 4 points of each segment (corresponds to Python `segments`).
pub fn segments(pts: &[Pt]) -> Vec<[Pt; 4]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= pts.len() {
        out.push([pts[i], pts[i + 1], pts[i + 2], pts[i + 3]]);
        i += 3;
    }
    out
}

/// Sample points on the curve, `n` per segment (corresponds to Python `sample`, default n=48).
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

/// Add an anchor at t (0..1) on segment s; the curve shape stays exactly the same (corresponds to Python `split`).
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

/// Remove anchor a (not the first or last) and its two handles (corresponds to Python `remove_anchor`).
pub fn remove_anchor(pts: &[Pt], a: usize) -> Vec<Pt> {
    let i = 3 * a;
    if i == 0 {
        // Python's slicing behaviour for a=0 (callers do not use it this way)
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

/// The anchor a handle point belongs to (1 -> 0, 2 -> 3, 4 -> 3, ...).
pub fn handle_anchor(i: usize) -> usize {
    if i % 3 == 1 { i - 1 } else { i + 1 }
}

// ---------------------------------------------------------------- symmetric curves
// A symmetric curve has an odd number of anchors: the middle one lies on the mirror line (or point), and point i of one half pairs with point len-1-i of the other half.
// "turn": the other half is this half rotated half a turn about the midpoint of the ends (S shape); "mirror": mirrored across the line through the midpoint (arch shape).
// The mirror line runs along `axis`: 0 = time direction, 1 = pitch direction; None = perpendicular to the line between the ends, exact mirror.

/// Symmetry transform (corresponds to Python `Symmetry`).
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
    /// Built from the curve's first and last points; `axis` = Some(0) / Some(1) / None. None when there are fewer than 2 points.
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

    /// v = alpha * d + beta * w (w runs along the mirror line).
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

    /// The mirror point of p.
    pub fn reflect(&self, p: Pt) -> Pt {
        let m = self.m;
        if self.mode == Sym::Turn {
            return [2.0 * m[0] - p[0], 2.0 * m[1] - p[1]];
        }
        let (alpha, beta) = self.split([p[0] - m[0], p[1] - m[1]]);
        let v = self.join(-alpha, beta);
        [m[0] + v[0], m[1] + v[1]]
    }

    /// Nearest landing spot for the middle anchor: the midpoint (turn) or the mirror line.
    pub fn onto_line(&self, p: Pt) -> Pt {
        if self.mode == Sym::Turn {
            return self.m;
        }
        let (_, beta) = self.split([p[0] - self.m[0], p[1] - self.m[1]]);
        let v = self.join(0.0, beta);
        [self.m[0] + v[0], self.m[1] + v[1]]
    }

    /// Handle of the middle anchor on a smoothed mirror curve: along the end direction (the arch top is round), keeping the length it reached in that direction.
    pub fn flat(&self, anchor: Pt, h: Pt) -> Pt {
        let (alpha, _) = self.split([h[0] - anchor[0], h[1] - anchor[1]]);
        let v = self.join(alpha, 0.0);
        [anchor[0] + v[0], anchor[1] + v[1]]
    }
}

/// Copy one half (source 0 = from the first half, 1 = from the second half) to the other half,
/// returning (pts, sharp). Requires an odd number of anchors (see `make_symmetric`); the two ends stay put.
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
    // The middle anchor lands on the mirror line / point and its handle follows
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

/// Same as `symmetric`, but first adds a middle anchor to the curve (splitting the middle segment in two).
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

// ---------------------------------------------------------------- pen editing

/// Anchor numbers that end a piece: the curve's two ends and the anchors on either side of each gap
/// (Python `piece_ends`).
pub fn piece_ends(c: &Curve) -> BTreeSet<usize> {
    let last = anchor_count(&c.pts).saturating_sub(1);
    let mut out: BTreeSet<usize> = [0, last].into_iter().collect();
    for &g in &c.gaps {
        out.insert(g);
        out.insert(g + 1);
    }
    out
}

/// Anchors that can't be removed: piece ends and tumour section starts (Python `fixed_anchors`).
pub fn fixed_anchors(c: &Curve) -> BTreeSet<usize> {
    let mut out = piece_ends(c);
    out.extend(c.splits.iter().copied());
    out
}

/// Anchor (and gap segment) numbers after `after` moved by `d` (an anchor added / removed there)
/// (Python `shift_marks`).
pub fn shift_marks(c: &mut Curve, after: usize, d: i64) {
    for marks in [&mut c.gaps, &mut c.splits] {
        if marks.is_empty() {
            continue;
        }
        *marks = marks
            .iter()
            .filter_map(|&a| {
                if a > after {
                    let n = a as i64 + d;
                    (n >= 0).then_some(n as usize)
                } else {
                    Some(a)
                }
            })
            .collect();
    }
}

impl Curve {
    /// Set the sharp anchors (empty clears them).
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

/// Mirror line direction for a mirrored curve: on screen, perpendicular to the line between the ends (1 = vertical, 0 = horizontal); None when exact (exact mirror).
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

/// The other half of a symmetric curve follows the half point i is on; returns true when it is a symmetric curve.
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

/// Drag point i to new:
/// an anchor takes its handles along (alt: pulls out new handles on both sides of it, symmetric on this side);
/// a handle point moves, and on a smooth anchor the other handle turns to stay smooth, keeping its length on screen (alt: only this one moves, the anchor becomes sharp).
/// The other half of a symmetric curve follows.
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
                let d = hypot2(hx - ax, hy - ay);
                let length = hypot2(ox - ax, oy - ay);
                if d > 0.0 && length > 0.0 {
                    c.pts[other] =
                        from_screen(ax - (hx - ax) / d * length, ay - (hy - ay) / d * length);
                }
            }
        }
    }
    keep_symmetric(c, i, to_screen, exact);
}

/// Add an anchor at t (0..1) on segment seg and move it to new (the curve goes through there); a
/// symmetric curve gets one on the other half too. False (nothing added) if that's an anchor already
/// or the segment is a gap.
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
        // the same position on the other half
        splits.push((segments(&c.pts).len() - 1 - seg, 1.0 - t));
    }
    let mut at = 3 * (seg + 1);
    if c.gaps.contains(&seg) {
        return false; // a gap between pieces isn't part of the curve
    }
    let mut pts = c.pts.clone();
    let mut sharp = c.sharp.clone();
    // Cut the later ones first so the earlier cut positions are not shifted
    splits.sort_by(|x, y| y.0.cmp(&x.0).then(y.1.total_cmp(&x.1)));
    for (s, tt) in splits {
        pts = split(&pts, s, tt);
        sharp = sharp
            .iter()
            .map(|&a| if a > s { a + 1 } else { a })
            .collect();
        shift_marks(c, s, 1);
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

/// What happens when point i is right-clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanDelete {
    /// the anchor is removed
    Anchor,
    /// the handle is retracted into its anchor
    Handle,
    /// the middle anchor of a symmetric curve: kept
    Middle,
}

/// `None` = the end point and its handle are kept (removing them would make them impossible to grab again).
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
    if piece_ends(c).contains(&(a / 3)) {
        return None;
    }
    if i.is_multiple_of(3) && fixed_anchors(c).contains(&(a / 3)) {
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

/// Right-click point i (see `can_delete`): a middle anchor is removed (its symmetric partner too), a handle is retracted into its anchor (that spot becomes sharp).
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
                shift_marks(c, k, -1);
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

/// Turn symmetry on (mode = Mirror / Turn) or off (None); the half in `source` keeps its shape (0 = first half, 1 = second half).
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

/// Which half is nearest to screen point (x, y) (0 = first half, 1 = second half).
pub fn half_at(pts: &[Pt], to_screen: &dyn Fn(Pt) -> [f64; 2], x: f64, y: f64) -> usize {
    let segs = segments(pts).len();
    match nearest(pts, to_screen, x, y, 64, &[]) {
        Some((seg, t, _)) if (seg as f64 + t) * 2.0 > segs as f64 => 1,
        _ => 0,
    }
}

/// The points to show (corresponds to Python `pen_handles`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// a handle point
    Ctrl,
    /// an anchor
    Anchor,
    /// an end point
    End,
}

/// `[(point number, kind)]` in drawing order (anchors on top). Not selected: the piece ends only.
/// `gaps`: segments between the pieces of a joined curve (their handles aren't shown, the anchors beside
/// them are ends).
pub fn pen_handles(pts: &[Pt], selected: bool, gaps: &[usize]) -> Vec<(usize, HandleKind)> {
    let n = pts.len();
    if n == 0 {
        return Vec::new();
    }
    let mut ends: BTreeSet<usize> = [0, n - 1].into_iter().collect();
    for &g in gaps {
        ends.insert(3 * g);
        ends.insert(3 * g + 3);
    }
    if !selected {
        return ends.into_iter().map(|i| (i, HandleKind::End)).collect();
    }
    let mut out: Vec<(usize, HandleKind)> = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        if i % 3 == 0 {
            continue;
        }
        let a = handle_anchor(i);
        if gaps.contains(&((i - 1) / 3)) {
            continue;
        }
        if ends.contains(&a) || *p != pts[a] {
            out.push((i, HandleKind::Ctrl));
        }
    }
    let mut i = 3;
    while i + 1 < n {
        if !ends.contains(&i) {
            out.push((i, HandleKind::Anchor));
        }
        i += 3;
    }
    out.extend(ends.into_iter().map(|i| (i, HandleKind::End)));
    out
}

/// The handle lines to draw `[(anchor, handle point)]`; handle lines in a gap aren't shown.
pub fn handle_lines(pts: &[Pt], gaps: &[usize]) -> Vec<(Pt, Pt)> {
    let mut out = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        if i % 3 == 0 {
            continue;
        }
        let a = handle_anchor(i);
        if a >= pts.len() || gaps.contains(&((i - 1) / 3)) {
            continue;
        }
        if *p != pts[a] {
            out.push((pts[a], *p));
        }
    }
    out
}

/// `(segment, t, distance)` of the curve point nearest to (x, y) on screen; segments in `gaps` are left
/// out. None when there are no segments.
pub fn nearest(
    pts: &[Pt],
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    x: f64,
    y: f64,
    n: usize,
    gaps: &[usize],
) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (s, seg) in segments(pts).iter().enumerate() {
        if gaps.contains(&s) {
            continue;
        }
        for i in 0..=n {
            let p = seg_point(seg[0], seg[1], seg[2], seg[3], i as f64 / n as f64);
            let sp = to_screen(p);
            let d = hypot2(sp[0] - x, sp[1] - y);
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

/// n+1 points spaced evenly by length along the polyline (corresponds to Python `resample`, default n=300).
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

/// How far apart two curves are: resample both evenly, then take the maximum point distance (corresponds to Python `difference`).
pub fn difference(pts_a: &[Pt], pts_b: &[Pt]) -> f64 {
    let a = resample(&sample(pts_a, 48), 100);
    let b = resample(&sample(pts_b, 48), 100);
    a.iter()
        .zip(b.iter())
        .fold(0.0, |m, (p, q)| m.max(dist(*p, *q)))
}

// ---------------------------------------------------------------- fitting (Philip Schneider's algorithm)

fn unit(v: Pt) -> Pt {
    let d = hypot2(v[0], v[1]);
    if d != 0.0 {
        [v[0] / d, v[1] / d]
    } else {
        [0.0, 0.0]
    }
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

/// Fit points with as few anchors as possible; the curve passes through the ends and stays within tol of all points.
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

/// The best handles along the given end directions (least squares).
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

/// Move t to near the point on the curve nearest to p.
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
    // Python's min(1.0, max(0.0, x)): with NaN it takes 0, unlike clamp
    let x = t - num / den;
    if x > 1.0 {
        1.0
    } else if x > 0.0 {
        x
    } else {
        0.0
    }
}
