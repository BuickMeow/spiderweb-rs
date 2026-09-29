//! custom: custom shapes drawn with the brush on the roll, and pasted notes; a function-by-function port of Python notes/custom.py.
//!
//! A custom shape is a drawing in its own frame: strokes use u = 0..1 (left to right) and v = 0..1 (bottom to top),
//! and come in four kinds: polyline (poly), Bezier curve (curve, see [`crate::bezier`]), three-point arc (arc, see [`crate::arc`])
//! and ellipse. On the roll `sh.pts` is the frame's three corners `[u=0 v=0, u=1 v=0, u=0 v=1]`,
//! so moving, flipping or rotating the shape just moves these three points and the drawing follows.
//!
//! Inside fill modes ([`FILLS`]): empty = outline only; fill = one note per stretch inside each key;
//! spam = each stretch tiled with gates; outline_spam = the outline cut into gates and the inside left empty.
//! A shape can also hold pasted notes instead of a drawing: `sh.notes` is the text from [`pack_notes`] (zlib + base64
//! little-endian int32 rows), byte-compatible with the Python version.
//!
//! Differences from the original (all Python exceptions are replaced by Option / Result):
//! - `frame_to_bp` / `frame_to_uv` return None with fewer than three frame points (Python raises ValueError);
//! - `add_stroke` returns None when the frame is degenerate (`frame_to_uv` is None);
//! - `notes_shape` returns None when there are 0 rows;
//! - `unpack_notes` / `block_notes` report bad data with [`NotesError`] (Python raises);
//! - `clean_strokes` / `clean_curve` accept JSON values; `int` / `float` edge cases (underscore digits,
//!   container reprs) are not reproduced, and bad values are skipped as in the original;
//! - `fill_plan` does not do Python's per-frame memoisation (it only affects speed, not the result);
//! - strokes do not have `src` yet (convert.py's "Turn into live shape" belongs to a later wave), so
//!   [`stroke_groups`] is always None for now (all strokes in one group).
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

/// Fill modes (custom.FILLS).
pub const FILLS: [Fill; 4] = [Fill::Empty, Fill::Fill, Fill::Spam, Fill::OutlineSpam];
/// The fill modes that use gates and spam starts (custom.SPAM_FILLS).
pub const SPAM_FILLS: [Fill; 2] = [Fill::Spam, Fill::OutlineSpam];
/// Spam start alignment (custom.ALIGNS).
pub const ALIGNS: [Align; 3] = [Align::Auto, Align::Aligned, Align::Centred];
/// Spam endings (custom.ENDS), in the panel dropdown's order.
pub const ENDS: [Ends; 5] = [
    Ends::Round,
    Ends::Keep,
    Ends::Drop,
    Ends::Min,
    Ends::Stretch,
];
/// Flags that exist only when turned on (custom.CUSTOM_FLAGS).
pub const CUSTOM_FLAGS: [&str; 2] = ["union", "apart"];

/// Custom shape defaults (custom.CUSTOM_DEFAULTS).
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
    /// The fill settings a new custom shape takes from the defaults (custom.custom_settings):
    /// ends is always there, like fill / gate / align; union / apart only when turned on.
    pub fn apply(&self, sh: &mut Shape) {
        sh.fill = self.fill;
        sh.gate = self.gate;
        sh.align = self.align;
        sh.ends = self.ends;
        sh.union = self.union;
        sh.apart = self.apart;
    }
}

/// The values of CUSTOM_DEFAULTS (fill = empty, gate = 1/16 beat = 60 ticks at PPQ 960, align = auto,
/// ends = round: the default for new shapes from 1.2.0 on; old shapes have no ends and read as drop).
pub const CUSTOM_DEFAULTS: CustomDefaults = CustomDefaults {
    fill: Fill::Empty,
    gate: 0.0625,
    align: Align::Auto,
    ends: Ends::Round,
    union: false,
    apart: false,
};

/// Number of ellipse samples (custom.ELLIPSE_STEPS).
pub const ELLIPSE_STEPS: usize = 360;
/// Number of samples per curve segment (custom.CURVE_STEPS).
pub const CURVE_STEPS: usize = 240;

/// The outline of a pasted-note shape: just a frame (custom.BOX_STROKE).
pub static BOX_STROKE: LazyLock<Stroke> = LazyLock::new(|| Stroke::Poly {
    pts: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
    free: false,
    smooth: 0,
    k: 1.0,
    src: None,
});

/// Prefix of packed notes that carry a track column (custom.TRACKS).
pub const TRACKS: &str = "t:";

/// Pack / unpack errors for pasted notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotesError {
    #[error("shape has no pasted notes")]
    Missing,
    #[error("base64 decode failed")]
    Base64,
    #[error("zlib decompress failed")]
    Zlib,
    #[error("note data is not 4 / 5 int32 columns")]
    Shape,
    #[error("shape frame has fewer than three points")]
    Frame,
}

// ---------------------------------------------------------------- settings sanitising

/// Python `float(x)`: numbers, booleans and numeric strings; anything else (including null) fails.
fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Python `int(x)`: numbers truncated, booleans and integer strings; anything else fails.
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

/// Python `isinstance(x, int)`: integers and bools (True = 1); floats and strings don't count.
fn py_src(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else {
                n.as_u64().and_then(|u| i64::try_from(u).ok())
            }
        }
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

/// Python truthiness: 0 / empty string / empty list / null are false.
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

/// float() of `st.get("k", 1.0)`, plus [`clean_k`] (`arc_k` in custom).
fn arc_k(st: &Value) -> f64 {
    match st.get("k") {
        None => 1.0,
        Some(v) => match py_float(v) {
            Some(k) => clean_k(k),
            None => 1.0,
        },
    }
}

/// Python `round(x, 6)` (banker's rounding).
fn round6(x: f64) -> f64 {
    let m = 1e6;
    round_half_even(x * m) / m
}

/// Ceiling division for b > 0 (corresponds to Python's `-(-a // b)`).
fn div_ceil_pos(a: i64, b: i64) -> i64 {
    let q = a.div_euclid(b);
    if a.rem_euclid(b) == 0 { q } else { q + 1 }
}

/// A point list / pair: a length-2 array or string (Python's unpackable sequence).
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

/// `[float(x) for x in st["box"]]` (array or string; the length is checked by the caller).
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

/// Sanitising of one stroke read from a file (custom.clean_curve).
pub fn clean_curve(st: &Value, pts: &[Pt]) -> Stroke {
    let last = (pts.len() as i64 - 1) / 3;
    let sharp = curve_sharp(st, last);
    let sym = curve_sym(st, last);
    Stroke::Curve {
        pts: pts.to_vec(),
        sharp,
        sym,
        src: None,
    }
}

/// `sorted({int(a) for a in st.get("sharp", ()) if 0 < int(a) < last})`:
/// one bad value throws away the whole set (Python's try wraps the whole comprehension).
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

/// Sanitising of the stroke list read from a file (custom.clean_strokes): bad data is skipped.
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
                    src: None,
                });
            }
        } else {
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
                        out.push(Stroke::Arc {
                            pts,
                            k: arc_k(st),
                            src: None,
                        });
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
                            src: None,
                        });
                    }
                }
            }
        }
        // Python attaches the source to the last accepted stroke, even when this one was bad.
        if let Some(src) = d.get("src").and_then(py_src)
            && let Some(last) = out.last_mut()
        {
            last.set_src(Some(src));
        }
    }
    out
}

// ---------------------------------------------------------------- strokes -> points

/// One stroke as a (u, v) point list; an ellipse starts at its leftmost point and returns exactly to the start (custom.stroke_points).
pub fn stroke_points(st: &Stroke) -> Vec<Pt> {
    match st {
        Stroke::Ellipse { box_, .. } => {
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
        Stroke::Arc { pts, k, .. } => arc_points(pts, *k, ARC_STEP),
        Stroke::Poly { pts, smooth, k, .. } => {
            if *smooth != 0 {
                smooth_path(pts, *smooth as f64, *k)
            } else {
                pts.clone()
            }
        }
    }
}

/// The stroke's raw points (not for ellipse; used for closure tests and such).
fn stroke_raw_pts(st: &Stroke) -> Option<&Vec<Pt>> {
    match st {
        Stroke::Poly { pts, .. } | Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. } => Some(pts),
        Stroke::Ellipse { .. } => None,
    }
}

/// Whether a path is closed: at least 3 points and first-to-last distance < 1e-6 (custom.path_closed).
pub fn path_closed(path: &[Pt]) -> bool {
    path.len() >= 3 && dist(path[0], path[path.len() - 1]) < 1e-6
}

/// Whether a stroke is closed (ellipse always; others by their raw points; custom.stroke_closed).
pub fn stroke_closed(st: &Stroke) -> bool {
    if matches!(st, Stroke::Ellipse { .. }) {
        return true;
    }
    stroke_raw_pts(st).is_some_and(|pts| path_closed(pts))
}

/// Join end-to-end point lists into one (custom.join_paths): closed ones stay in front, keeping their order.
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

/// The stroke's two ends `[start, end]`; None when closed (curves use their raw points, sampling is too slow; custom.stroke_span).
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

/// The open outlines of the figure after joining strokes that meet (custom.open_paths): each one is a gap.
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

/// All loose ends of the figure after joining (custom.open_ends).
pub fn open_ends(strokes: &[Stroke]) -> Vec<Pt> {
    let mut out = Vec::new();
    for path in open_paths(strokes) {
        out.push(path[0]);
        out.push(path[path.len() - 1]);
    }
    out
}

/// Every outline returns to its start (custom.strokes_closed).
pub fn strokes_closed(strokes: &[Stroke]) -> bool {
    !strokes.is_empty() && open_paths(strokes).is_empty()
}

/// Fill / Spam can be used (any drawing will do: gaps are closed with straight lines, see [`fill_plan`]) (custom.fillable).
pub fn fillable(strokes: &[Stroke]) -> bool {
    !strokes.is_empty()
}

// Outline gaps, for Fill / Spam (beats / keys, not screen, so scaling does not change the notes):
/// Loose ends no farther apart than this count as meeting (joined by a straight line).
pub const TOUCH_BEATS: f64 = 1.0 / 64.0;
/// Same, in the key direction.
pub const TOUCH_KEYS: f64 = 1.0;
/// An open piece stays within half a key (vertically) or 1/64 beat (horizontally) of the line joining its ends: nothing worth filling inside.
pub const FLAT_KEYS: f64 = 0.5;
/// Same, in the beat direction.
pub const FLAT_BEATS: f64 = 1.0 / 64.0;

/// Whether two ends count as meeting (custom.near_ends).
pub fn near_ends(p: Pt, q: Pt) -> bool {
    (p[0] - q[0]).abs() <= TOUCH_BEATS + 1e-9 && (p[1] - q[1]).abs() <= TOUCH_KEYS + 1e-9
}

/// An open path never strays more than half a key (vertically) or 1/64 beat (horizontally) from the line between its ends: nothing worth filling
/// (a straight line, a very gentle curve) (custom.flat_path).
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
            continue; // (it goes past its own two ends)
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

/// The custom shape's outline as Fill / Spam sees it (beats / pitch) (custom.fill_plan):
/// `polys` = the closed loops that make up the inside, `closers` = the straight lines added to close gaps (loose ends that nearly touch are joined
/// directly; every remaining open piece is connected straight back from its end to its start), `flat` = open pieces too flat to have an inside (outline notes only).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FillPlan {
    pub polys: Vec<Vec<Pt>>,
    pub closers: Vec<[Pt; 2]>,
    pub flat: Vec<Vec<Pt>>,
}

/// Work out the [`FillPlan`] (custom.fill_plan). The Python version memoises by frame; here that only affects speed, so it is not cached.
pub fn fill_plan(sh: &Shape) -> FillPlan {
    let paths = join_paths(&custom_strokes(sh));
    let mut polys: Vec<Vec<Pt>> = paths.iter().filter(|p| path_closed(p)).cloned().collect();
    let mut opens: Vec<Vec<Pt>> = paths.iter().filter(|p| !path_closed(p)).cloned().collect();
    let mut closers: Vec<[Pt; 2]> = Vec::new();
    loop {
        // The closest pair of loose ends that meet, joined, until there is none left
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

/// The straight lines closing gaps (drawn dashed) (custom.gap_lines).
pub fn gap_lines(sh: &Shape) -> Vec<[Pt; 2]> {
    if sh.text.is_some() {
        Vec::new()
    } else {
        fill_plan(sh).closers
    }
}

/// Join open polylines that meet end to end into one (curves and circles stay as they are; custom.join_strokes).
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
            src: None,
        });
    }
    others
}

/// The frame's three corner points (the internal representation used by custom.frame_to_bp).
fn frame3(pts: &[Pt]) -> Option<[Pt; 3]> {
    Some([*pts.first()?, *pts.get(1)?, *pts.get(2)?])
}

/// The custom shape's point lists (custom.custom_strokes): a text shape gives letter outlines, others are projected through the frame.
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

/// The three corner points of the frame from (b0, p0) to (b1, p1) (custom.box_frame).
pub fn box_frame(b0: f64, p0: f64, b1: f64, p1: f64) -> [Pt; 3] {
    let (bl, bh) = (b0.min(b1), b0.max(b1));
    let (pl, ph) = (p0.min(p1), p0.max(p1));
    [[bl, pl], [bh, pl], [bl, ph]]
}

/// Stretch the strokes so they fill a 0..1 frame exactly; returns (new strokes, width / height; None when flat) (custom.normalize_strokes).
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
        if let Stroke::Ellipse { box_, .. } = st {
            let a = fix(box_[0], box_[1]);
            let b = fix(box_[2], box_[3]);
            out.push(Stroke::Ellipse {
                box_: [a[0], a[1], b[0], b[1]],
                src: None,
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
            Stroke::Arc { pts, k, .. } => {
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

// ---------------------------------------------------------------- live drawing (strokes drawn straight on the roll)

/// (u, v) -> (beat, pitch) (custom.frame_to_bp); None with fewer than three frame points.
pub fn frame_to_bp(pts: &[Pt]) -> Option<impl Fn(f64, f64) -> Pt + use<>> {
    let [a, b, c] = frame3(pts)?;
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    Some(move |u: f64, v: f64| [b0 + u * ub + v * vb, p0 + u * up + v * vp])
}

/// (beat, pitch) -> (u, v); None when the frame is degenerate (flat) or has fewer than three points (custom.frame_to_uv).
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

/// How many us one v is for the shape's frame on screen (k beats per key) (custom.uv_k); 1.0 when degenerate.
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

/// The frame is not turned (u along time, v along pitch), so an ellipse inside stays an ellipse (custom.frame_upright).
pub fn frame_upright(pts: &[Pt]) -> bool {
    let Some([a, b, c]) = frame3(pts) else {
        return false;
    };
    (b[1] - a[1]).abs() < 1e-12 && (c[0] - a[0]).abs() < 1e-12
}

/// Run every point of the stroke through fn(u, v) -> (u, v) (custom.map_stroke).
/// su / sv: how many times wider / taller this makes the stroke (used to keep arcs round and ellipse frames correctly oriented).
pub fn map_stroke<F: Fn(f64, f64) -> Pt>(st: &Stroke, f: F, su: f64, sv: f64) -> Stroke {
    // Python builds a fresh dict here, so an ellipse loses its `src`; the other kinds keep it.
    if let Stroke::Ellipse { box_, .. } = st {
        let a = f(box_[0], box_[1]);
        let b = f(box_[2], box_[3]);
        return Stroke::Ellipse {
            box_: [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[0].max(b[0]),
                a[1].max(b[1]),
            ],
            src: None,
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
        Stroke::Arc { pts, k, .. } => {
            for p in pts.iter_mut() {
                *p = f(p[0], p[1]);
            }
            *k *= scale;
        }
        Stroke::Ellipse { .. } => {}
    }
    new
}

/// Refit the shape's frame around its drawing (custom.refit): strokes return to filling 0..1, the frame points move with them, and nothing moves on the roll.
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

/// The points other strokes can connect to: every point of a polyline, the two ends of a curve or arc (custom.stroke_ends).
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

/// The stroke's k (Python `st.get("k", 1.0)`): polyline / arc have one, others are 1.0.
fn stroke_k(st: &Stroke) -> f64 {
    match st {
        Stroke::Poly { k, .. } | Stroke::Arc { k, .. } => *k,
        _ => 1.0,
    }
}

/// The inverse of [`uv_k`]: how many us one v is in the frame with k beats per key on screen (to get the stroke's k out of the frame);
/// 1 when there is none (custom.bp_k).
pub fn bp_k(pts: &[Pt], k: f64) -> f64 {
    let Some([a, b, c]) = frame3(pts) else {
        return 1.0;
    };
    let (b0, p0) = (a[0], a[1]);
    let (ub, up, vb, vp) = (b[0] - b0, b[1] - p0, c[0] - b0, c[1] - p0);
    // uv_k = hypot(vb / K, vp) / hypot(ub / K, up) = k, solving gives x = 1 / K²
    let num = k * k * up * up - vp * vp;
    let den = vb * vb - k * k * ub * ub;
    let x = if den.abs() > 1e-18 { num / den } else { -1.0 };
    if x > 1e-18 { 1.0 / x.sqrt() } else { 1.0 }
}

/// Custom shape sh's stroke k, converted to beats / pitch (like one drawn on the roll, for [`add_stroke`]):
/// an ellipse in a turned frame becomes a curve; the k of an arc / freehand stroke becomes beats per key (custom.stroke_bp).
pub fn stroke_bp(sh: &Shape, k: usize) -> Option<Stroke> {
    let mut st = sh.strokes.get(k)?.clone();
    let to_bp = frame_to_bp(&sh.pts)?;
    if matches!(st, Stroke::Ellipse { .. }) && !frame_upright(&sh.pts) {
        let Stroke::Ellipse { box_, .. } = &st else {
            return None;
        };
        st = Stroke::Curve {
            pts: ellipse_bezier(*box_),
            sharp: Vec::new(),
            sym: None,
            src: None,
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

/// Put stroke st, drawn on the roll, into shape sh and refit the frame (custom.add_stroke);
/// ends landing on another stroke's point snap exactly to it. at: the new stroke's number (defaults to last).
/// Returns the new stroke's number; None when the frame is degenerate.
pub fn add_stroke(sh: &mut Shape, st: &Stroke, at: Option<usize>) -> Option<usize> {
    let to_uv = frame_to_uv(&sh.pts)?;
    let converted;
    let st = if matches!(st, Stroke::Ellipse { .. }) && !frame_upright(&sh.pts) {
        let Stroke::Ellipse { box_, .. } = st else {
            return None;
        };
        converted = Stroke::Curve {
            pts: ellipse_bezier(*box_),
            sharp: Vec::new(),
            sym: None,
            src: None,
        };
        &converted
    } else {
        st
    };
    let mut new = map_stroke(st, to_uv, 1.0, 1.0);
    // its k: beats per key on screen -> how many us one v is in the frame (freehand strokes and arcs have one)
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
    // Python's list.insert: an out-of-range number inserts last, but the number passed in is still returned
    let at = at.unwrap_or(sh.strokes.len());
    sh.strokes.insert(at.min(sh.strokes.len()), new);
    refit(sh);
    Some(at)
}

/// An empty custom shape (for live drawing): a 1 beat × 1 key frame at (0, 0), refitted once something is drawn (custom.new_live_shape).
pub fn new_live_shape(defaults: &Shape, custom_defaults: &CustomDefaults) -> Shape {
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = "Live drawing".to_string();
    sh.strokes = Vec::new();
    custom_defaults.apply(&mut sh);
    sh.pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    sh
}

// ---------------------------------------------------------------- outline and fill -> notes

/// Notes on every stroke of a custom shape, like lines (custom.outline_notes).
pub fn outline_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    paths_outline(&join_paths(&custom_strokes(sh)), ppq)
}

/// Same, but only for these stroke numbers (the only of custom.outline_notes; out-of-range numbers are skipped).
pub fn outline_notes_only(sh: &Shape, ppq: f64, only: &[usize]) -> Vec<[i64; 3]> {
    let paths = custom_strokes(sh);
    let picked: Vec<Vec<Pt>> = only.iter().filter_map(|&k| paths.get(k).cloned()).collect();
    paths_outline(&join_paths(&picked), ppq)
}

/// Notes on these (joined) paths, like lines (custom.paths_outline).
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

/// The beat spans where the inside of the closed polygons (even-odd, holes empty) meets pitch row q (custom.row_spans).
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
    // Between adjacent levels there are no corners and every edge is straight, so each pair of crossings is a trapezium: take the wider of the two sides for the time range.
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

/// [`row_spans`], but the inside of any single loop counts (overlaps fill too, and so do holes) (custom.union_spans).
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

/// Each stretch inside the shape as `[pitch, start tick, end tick]` for every key (custom.inside_spans's order:
/// pitch increasing, and within a pitch in [`row_spans`] order). Text: non-zero rule with a threshold (text.py).
/// Outline gaps are closed with straight lines ([`fill_plan`]).
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

/// The spam gate, in ticks (custom.spam_gate).
pub fn spam_gate(sh: &Shape, ppq: f64) -> i64 {
    1.max(floor_half(sh.gate * ppq))
}

/// stretches: (start, end, key) rows (ticks) -> each stretch tiled with back-to-back notes one gate g long (custom.chop).
/// ALIGNS decides where it starts; ENDS decides what happens to a leftover that cannot fit a whole gate.
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
            let n = 1.max((2 * size + g).div_euclid(2 * g)); // whole-gate count, rounded (half rounds up)
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
                // the cells of the gate grid that hold at least half a gate
                let lo = -((g - 2 * s0).div_euclid(2 * g));
                let n = (2 * e0 + g).div_euclid(2 * g) - lo;
                if n <= 0 {
                    // (none fits: the cell the middle of the stretch falls in)
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
            Align::Aligned => div_ceil_pos(s0, g) * g, // the first gate line at or after s0
            Align::Centred => s0 + size.rem_euclid(g).div_euclid(2),
            Align::Auto => s0,
        };
        let n = 0.max((e0 - s).div_euclid(g));
        if ends == Ends::Keep {
            // the leftovers before the first whole gate / after the last become short notes (aligned / centred)
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
        let short = n == 0; // shorter than one gate: keep one note as it is ("min": at least a quarter gate)
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

/// stretches: (start, end, key) rows (ticks) -> the notes tiling each stretch with gate g (custom.chop).
pub fn chop(sh: &Shape, stretches: &[[i64; 3]], g: i64) -> Vec<[i64; 3]> {
    chop_core(sh, stretches, g, true).1
}

/// How many notes chop turns stretches into (custom.chop_count).
pub fn chop_count(sh: &Shape, stretches: &[[i64; 3]], g: i64) -> i64 {
    if stretches.is_empty() {
        return 0;
    }
    chop_core(sh, stretches, g, false).0.iter().sum()
}

/// Stroke groups of a custom shape whose strokes came from different shapes, as `[[stroke numbers]]`
/// ordered by the `src` they came from (custom.stroke_groups; convert.py: each shape's strokes make
/// their notes on their own, like the shapes did). None if they all belong to one group.
pub fn stroke_groups(sh: &Shape) -> Option<Vec<Vec<usize>>> {
    let mut groups: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (k, st) in sh.strokes.iter().enumerate() {
        groups.entry(st.src().unwrap_or(-1)).or_default().push(k);
    }
    if groups.len() > 1 {
        Some(groups.into_values().collect())
    } else {
        None
    }
}

/// Outline notes (spam: chopped as in Outline spam) and which stroke group each note belongs to (None when all strokes are one group,
/// see [`stroke_groups`]) (custom.outline_groups).
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

/// Chop outline notes into back-to-back notes one spam gate long (spam start and ending as in Spam; notes shorter than one gate do not
/// disappear, so steep outline pieces stay) (custom.chop_outline).
pub fn chop_outline(sh: &Shape, notes: &[[i64; 3]], ppq: f64) -> Vec<[i64; 3]> {
    chop(sh, notes, spam_gate(sh, ppq))
}

/// The notes the outline makes with the spam gate (custom.outline_spam).
pub fn outline_spam(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    outline_groups(sh, ppq, true).0
}

/// The outline notes of the open pieces of a filled shape that are too flat to fill (fill_plan's flat), so they do not vanish
/// (custom.flat_notes).
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

/// How many notes the shape makes, without actually generating them (spam can run to millions) (custom.custom_note_count); None when unsure.
pub fn custom_note_count(sh: &Shape, ppq: f64) -> Option<i64> {
    if let Some(text) = &sh.notes {
        return unpack_notes(text).ok().map(|rows| rows.len() as i64);
    }
    if sh.apart && matches!(sh.fill, Fill::Fill | Fill::Spam) {
        return None; // (generate them and count)
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

/// The shape's notes (start, end, key) (custom.custom_notes):
/// empty = outline; fill = one per stretch in each key; spam = each stretch tiled with gates; outline_spam = the outline chopped.
pub fn custom_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    custom_notes_groups(sh, ppq).0
}

/// custom_notes, plus which group each note belongs to (None = all one group: see [`outline_groups`]).
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
    let flat = flat_notes(sh, ppq); // (in Spam, like Outline spam)
    if sh.fill == Fill::Fill {
        let mut notes = spans;
        notes.extend(flat);
        if sh.apart {
            // the notes on the edges, and the long notes of the inside between them
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
        // the same spam; notes on the filled edges count as the outline's
        let ids = on_edge(&notes)
            .into_iter()
            .map(|edge| if edge { 0 } else { 1 })
            .collect();
        return (notes, Some(ids));
    }
    (notes, None)
}

/// Fill / Spam with "Outline": the outline and the inside need separate channels (custom.outline_apart).
pub fn outline_apart(sh: &Shape) -> bool {
    sh.kind == Kind::Custom
        && sh.apart
        && matches!(sh.fill, Fill::Fill | Fill::Spam)
        && sh.notes.is_none()
}

/// (start, end, key) notes -> the spans each key covers: (key, start, end) arrays sorted by key and start,
/// non-overlapping (custom.merged_by_key).
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

/// Which notes lie entirely within a span others cover on the same key (custom.covered).
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
            // searchsorted(..., "right"): the last span whose start is before (or at) it
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

/// Shift the notes' keys by keys (custom.shifted).
pub fn shifted(notes: &[[i64; 3]], keys: i64) -> Vec<[i64; 3]> {
    notes.iter().map(|n| [n[0], n[1], n[2] + keys]).collect()
}

/// Fill / Spam "Outline": which (start, end, key) notes are on the edge of the area they fill: not entirely covered by notes on the key above
/// or below, or the first / last one on their own key. So only the edges of the filled area itself count: an outline inside it (filled by an overlap)
/// is excluded; where overlaps cancel, every side of each filled patch is outline, no matter which way it slopes (custom.on_edge).
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

/// Fill "Outline": the parts of the long filled (start, end, key) notes that are on the area's edge: the times not covered by the key above or
/// below, plus the first and last tick of each stretch (like the vertical parts of a line), as (start, end, key) notes
/// (custom.edge_parts).
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

/// (start, end, key) spans with the times others cover on the same key removed (custom.cut_out).
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

// ---------------------------------------------------------------- pasted notes
// A custom shape can hold notes pasted in from another program (instead of a drawing): sh["notes"] = pack_notes text,
// and strokes are just the frame's outline. In the frame a note runs from u = start / T to end / T at v = (row + 0.5) / K
// (T = the end tick of the last note, K = the number of keys from the lowest to the highest), and moving / stretching / flipping / rotating the frame
// carries the notes with it. sh["own_vel"]: notes keep their own velocity.

/// (start, end, row, velocity, track) rows -> save text (zlib + base64) (custom.pack_notes).
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

/// pack_notes text -> (start, end, row, velocity, track) rows (custom.unpack_notes).
/// Data from older versions has no track column (and no `t:` prefix), so track is filled in as 0.
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| i32::from_le_bytes(*c) as i64)
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

/// Whether the text is packed notes Spiderweb can use (custom.check_notes).
pub fn check_notes(text: &str) -> bool {
    let Ok(rows) = unpack_notes(text) else {
        return false;
    };
    !rows.is_empty()
        && rows.iter().all(|r| {
            r[0] >= 0 && r[1] > r[0] && r[2] >= 0 && (1..=127).contains(&r[3]) && r[4] >= 0
        })
}

/// (tick, gate, key, velocity, track) rows -> a custom shape holding them (custom.notes_shape):
/// the frame starts at the first note's tick / ppq beats and the lowest key; None with 0 rows.
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

/// The notes of a pasted-note shape (start, end, pitch, velocity, track) where its frame is now (custom.block_notes).
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
            src: None,
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

    /// "centred": the leftover that cannot fit a whole gate is split half to each end.
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

    /// The five ENDS endings: round at least one, stretch fills exactly, keep leaves the leftover, drop / min for short stretches.
    #[test]
    fn ends_round_and_stretch() {
        let round = shape(Fill::Spam, Align::Auto, Ends::Round, false, vec![square()]);
        assert_eq!(
            chop(&round, &[[0, 30, 60]], 60),
            vec![[0, 60, 60]],
            "round: half a gate counts as one"
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
            "min: grows to a quarter gate, centred"
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
            "drop: stretches shorter than one gate stay as they are"
        );
    }

    /// union: overlapping outline areas fill too (off, overlaps cancel out and holes stay empty).
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
        assert_eq!(got.len(), 6, "overlap cancels: a gap in the middle");
        assert_eq!(got[0], [60, 0, 960]);
        assert_eq!(got[1], [60, 1920, 2880]);
        let union = shape(Fill::Fill, Align::Auto, Ends::Drop, true, vec![square(), b]);
        assert_eq!(
            inside_spans(&union, 960.0),
            vec![[60, 0, 2880], [61, 0, 2880], [62, 0, 2880]]
        );
        assert_eq!(custom_note_count(&union, 960.0), Some(3));
    }

    /// Loose ends that nearly touch are joined directly (the closing line counts as a dashed line); every remaining open piece is connected straight from its end back to its start.
    #[test]
    fn fill_plan_joins_ends_that_nearly_touch() {
        let near = vec![
            poly(&[[0.0, 0.0], [0.5, 0.5]]),
            poly(&[[0.5 + 1e-4, 0.5], [1.0, 1.0]]),
        ];
        let sh = shape(Fill::Fill, Align::Auto, Ends::Drop, false, near);
        let plan = fill_plan(&sh);
        assert!(plan.polys.is_empty(), "flat after joining");
        assert_eq!(plan.flat.len(), 1);
        assert_eq!(plan.closers.len(), 1);
        assert!(near_ends(plan.closers[0][0], plan.closers[0][1]));
        // two diagonals (unit frame): loose ends one key apart still count as meeting, and two gaps remain after joining
        let two = vec![
            poly(&[[0.0, 0.0], [1.0, 1.0]]),
            poly(&[[0.0, 1.0], [1.0, 0.0]]),
        ];
        let mut sh = shape(Fill::Fill, Align::Auto, Ends::Drop, false, two);
        sh.pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let plan = fill_plan(&sh);
        assert_eq!(plan.closers.len(), 2, "two gaps, two dashed lines");
        assert_eq!(plan.polys.len(), 1);
        assert!(plan.flat.is_empty());
    }

    /// Without `src` the strokes are all one group; `outline_groups` only splits real sources.
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
