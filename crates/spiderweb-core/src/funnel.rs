//! Funnel: a line leading to a wall, opening out along curves and filled with spam or long notes (a function-by-function port of Python notes/funnel.py).
//!
//! `sh.pts = [line start, line end, wall end 1, wall end 2, (line 2 start, line 2 end, ...)]`: straight lines, drawn however you like
//! (only the line is drawn then). Extra lines lead to the same wall. The note grid runs from the first line's start towards the wall; a wall that is
//! earlier in time makes a reversed funnel.
//!
//! `sh.starts = [{line, at, ends: [curve to wall end 1 or None, ...wall end 2]}]`: each start opens
//! one curve towards each wall end (a half funnel has only one wall end; a wall end lying on the line has no curve).
//!
//! A curve lives in its own oblique box: from the start S, U along the line and V along the wall, S + U + V = the wall end, and S + U is where the line meets
//! the wall. The box point `[u, f]` is `S + u*U + f*V` (u = 0 at the start, 1 at the wall; f = how far it opens), so
//! the curve follows when the two lines are moved, stretched, flipped or rotated. A curve is the Bezier from `[0, 0]` (the start) to `[1, 1]` (the wall end)
//! (see [`crate::bezier`]: anchors + handles); `sharp` are the corner anchors where handles split, `link` is the group number,
//! and `flip` means turned end to end relative to the other curves in the group.
//!
//! Differences from the original (all Python exceptions are replaced by Option / Result):
//! - cleaning functions return None on bad data (Python raises); `clean_curve`'s None likewise means "no usable curve";
//! - `start_point` / `curve_box` return None when the line number is beyond `pts` (Python IndexError);
//! - `formula_curve` / `preset_curve` report formula failures with `Err(String)`;
//! - places where Python returns a closure use structs: [`Openness`] (w(d)) and [`SmoothCurve`] (eval(u)).
//!
//! [`Spans`] is an ordered map from key -> span lists, keeping Python dict insertion order (`funnel_cells`'s output
//! order follows it).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use serde_json::{Value, json};

use crate::bezier::{anchor_count, fit, handle_anchor, sample};
use crate::custom::row_spans;
use crate::paths::{EDGE, pitch_of};
use crate::shape::{FunnelCurve, FunnelFill, FunnelStart, GateChange, GateFollow, Shape, WallMode};
use crate::{Pt, floor_half, hypot2, round_half_even};

// ---------------------------------------------------------------- settings and constants

/// Fill mode (funnel.FUNNEL_FILLS).
pub const FUNNEL_FILLS: [FunnelFill; 2] = [FunnelFill::Spam, FunnelFill::Long];
/// Gate change mode (funnel.GATE_CHANGES).
pub const GATE_CHANGES: [GateChange; 2] = [GateChange::Steps, GateChange::Smooth];
/// What the gate follows (funnel.GATE_FOLLOWS).
pub const GATE_FOLLOWS: [GateFollow; 2] = [GateFollow::Time, GateFollow::Curve];
/// Wall mode (funnel.WALL_MODES).
pub const WALL_MODES: [WallMode; 2] = [WallMode::In, WallMode::Past];

/// Funnel settings (the product of clean_funnel, matching the dict Python returns).
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

/// Funnel defaults (funnel.FUNNEL_DEFAULTS).
pub const FUNNEL_DEFAULTS: FunnelSettings = FunnelSettings {
    fill: FunnelFill::Spam,
    gate0: 0.0625,
    gate1: 0.0625,
    vary: false,
    change: GateChange::Steps,
    follow: GateFollow::Time,
    wall: WallMode::In,
};

/// The first version's default curve (funnel.FUNNEL_BEND).
pub const FUNNEL_BEND: Pt = [0.75, 0.2];
/// Maximum number of old-style bends (funnel.MAX_BENDS).
pub const MAX_BENDS: usize = 32;

/// Default curve: close to the first version's default (slow start, opening fast near the wall), with only two anchors and handles at the ends.
pub const DEFAULT_CURVE: [Pt; 4] = [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]];

/// Preset curve names and formula strings (funnel.CURVE_PRESETS; the formula string is for the UI, None = default curve).
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

/// How close anchors + handles must stay to the formula (a fraction of the curve's size; funnel.FIT_TOLERANCE).
pub const FIT_TOLERANCE: f64 = 0.003;

impl FunnelFill {
    /// The setting name as a string (for JSON / the UI).
    pub fn name(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Long => "long",
        }
    }

    /// Parse from a setting name; None for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "spam" => Some(Self::Spam),
            "long" => Some(Self::Long),
            _ => None,
        }
    }
}

impl GateChange {
    /// The setting name as a string (for JSON / the UI).
    pub fn name(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Smooth => "smooth",
        }
    }

    /// Parse from a setting name; None for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "steps" => Some(Self::Steps),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }
}

impl GateFollow {
    /// The setting name as a string (for JSON / the UI).
    pub fn name(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Curve => "curve",
        }
    }

    /// Parse from a setting name; None for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "time" => Some(Self::Time),
            "curve" => Some(Self::Curve),
            _ => None,
        }
    }
}

impl WallMode {
    /// The setting name as a string (for JSON / the UI).
    pub fn name(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Past => "past",
        }
    }

    /// Parse from a setting name; None for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "in" => Some(Self::In),
            "past" => Some(Self::Past),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- JSON helpers (same as custom.rs)

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

/// A point pair: a length-2 array or string (Python's unpackable sequence).
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

// ---------------------------------------------------------------- settings sanitising

/// Sanitising of funnel settings read from a file (funnel.clean_funnel): bad values return None (Python raises).
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
    // sh.get(key, default): a key present with value null still errors (float(None))
    let gate = |key: &str, default: f64| -> Option<f64> {
        match obj.get(key) {
            None => Some(default),
            Some(v) => py_float(v),
        }
    };
    out.gate0 = 1e-6_f64.max(gate("gate0", FUNNEL_DEFAULTS.gate0)?);
    out.gate1 = 1e-6_f64.max(gate("gate1", FUNNEL_DEFAULTS.gate1)?);
    // Old files have no "vary": two different gates meant change at the time
    out.vary = match obj.get("vary") {
        Some(v) => py_bool(v),
        None => (out.gate0 - out.gate1).abs() > 1e-9,
    };
    if !out.vary {
        out.gate1 = out.gate0;
    }
    Some(out)
}

/// Sanitising of the start list read from a file (funnel.clean_starts): bad data returns None (Python raises).
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
            Some(v) if !py_bool(v) => {} // empty list / empty string / {}: list(...) is empty
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
                old_twins.push(out.len() - 1); // in the old version a start's two curves always stayed the same
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

/// Sanitising of one curve read from a file (funnel.clean_curve; None means no usable curve).
pub fn clean_curve(c: &Value) -> Option<FunnelCurve> {
    clean_curve_strict(c).ok().flatten()
}

/// Strict version of [`clean_curve`]: Err = Python would raise, Ok(None) = no curve.
fn clean_curve_strict(c: &Value) -> Result<Option<FunnelCurve>, ()> {
    if !py_bool(c) {
        return Ok(None);
    }
    if let Value::Array(a) = c {
        // old-style bends: one = the 1/x curve through it, several = monotone cubic
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
        // an empty {} / "" iterates empty (like Python's for loop)
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

/// A new curve (funnel.new_curve): when pts is missing use [`DEFAULT_CURVE`].
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

/// Each curve as `(start number, wall end, curve)` (funnel.all_curves).
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

/// The next group number (funnel.next_link).
pub fn next_link(sh: &Shape) -> i64 {
    next_link_of(&sh.starts)
}

/// The next group number for a start list (the internal form of [`next_link`]).
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

/// The other curves linked with this one (funnel.partners): `(start, wall end, turned end to end)`.
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

/// Turn the curve's points end to end (the steep part moves to the other side; funnel.turned).
pub fn turned(pts: &[Pt]) -> Vec<Pt> {
    pts.iter().rev().map(|p| [1.0 - p[0], 1.0 - p[1]]).collect()
}

/// The curve as a partner sees it (funnel.turned_curve): turned end to end when flip, otherwise the same; without the group number.
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

/// The curve turned inside out (funnel.inside_out): bulges the other way (slow start <-> fast start, S <-> reverse S); without the group number.
pub fn inside_out(c: &FunnelCurve) -> FunnelCurve {
    FunnelCurve {
        pts: c.pts.iter().map(|p| [p[1], p[0]]).collect(),
        sharp: c.sharp.clone(),
        link: None,
        flip: false,
    }
}

/// Give curve c the points and sharp anchors of shape (a curve), keeping the group number (funnel.set_shape).
pub fn set_shape(c: &mut FunnelCurve, shape: &FunnelCurve) {
    c.pts = shape.pts.clone();
    c.sharp = shape.sharp.clone();
}

// ---------------------------------------------------------------- first-version bends (old projects)

/// Sanitising of old-style bends (funnel.clean_bends): bad data returns None (Python raises).
pub fn clean_bends(bends: &Value) -> Option<Vec<Pt>> {
    clean_bends_strict(bends).ok()
}

/// Strict version of [`clean_bends`].
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

/// Points on the curve through old-style bends (funnel.old_curve_points): one bend = the 1/x curve
/// through it, several = monotone cubic. None with no bends (Python raises).
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

/// The first version's funnel (funnel.old_funnel): `[start, wall top] + sides` -> its lines + wall, plus the start.
/// The returned starts have the same shape as [`clean_starts`]'s input (ends holds a list of bends or null).
pub fn old_funnel(sh: &Value) -> Option<(Vec<Pt>, Value)> {
    let obj = sh.as_object()?;
    let raw = obj.get("pts")?.as_array()?;
    if raw.len() != 2 {
        return None; // Python unpacking is exactly two
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

/// Clamp one bend to its valid range (funnel.clamp_bend).
pub fn clamp_bend(u: f64, f: f64) -> Pt {
    [
        0.99_f64.min(0.01_f64.max(u)),
        0.999_f64.min(0.001_f64.max(f)),
    ]
}

/// The 1/x curve through the bend point as `(a, mirrored)`; None for a straight line (funnel._bend_curve).
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

/// How far one bend's curve is open at u (0..1; 0 = start, 1 = wall; funnel.funnel_f).
pub fn funnel_f(bend: Pt, u: f64) -> f64 {
    let Some((a, mirrored)) = bend_curve(bend) else {
        return u;
    };
    let u = if mirrored { 1.0 - u } else { u };
    let y = a * u / (1.0 + a - u);
    if mirrored { 1.0 - y } else { y }
}

/// Position at which one bend's curve is open to y (0..1, the inverse of [`funnel_f`]; funnel.funnel_u).
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

/// Monotone cubic through these points (Fritsch-Carlson, funnel._smooth_curve): smooth and never overshooting.
#[derive(Clone, Debug)]
pub struct SmoothCurve {
    xs: Vec<f64>,
    ys: Vec<f64>,
    h: Vec<f64>,
    m: Vec<f64>,
}

/// Build a monotone cubic through `(xs, ys)`; None when there are too few points or the lengths differ (Python raises).
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
    // Python's h[-1] / h[-2]: the last and second-to-last
    m[n - 1] = end_slope(h[n - 2], h[n - 3], d[n - 2], d[n - 3]);
    Some(SmoothCurve {
        xs: xs.to_vec(),
        ys: ys.to_vec(),
        h,
        m,
    })
}

impl SmoothCurve {
    /// The curve's value at u (same as the fn(u) Python returns).
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

// ---------------------------------------------------------------- curve shapes (presets, formulas)

/// The formula's points at n+1 evenly spaced x (funnel.formula_curve): y stretched to 0 -> 1.
/// Err when the formula cannot be worked out (Err / non-finite) or the ends are equally high.
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

/// The curve for a formula (funnel.preset_curve; None = default curve).
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

// ---------------------------------------------------------------- lines / starts / boxes

/// Remove lines (by number) and curves `((start, wall end))` from the funnel (funnel.remove_funnel_parts):
/// curves of starts on a removed line go too, and the next line takes over as the first. Returns false when no line is left (delete the funnel).
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

/// `(start, end)` of each funnel line (excluding the wall; funnel.funnel_lines).
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

/// Index in `sh.pts` where line number `line` starts (funnel.line_index).
pub fn line_index(line: usize) -> usize {
    if line == 0 { 0 } else { 2 + 2 * line }
}

/// The point at `at` (0..1) on line `line` (funnel.start_point); None when the line number is beyond `pts`.
pub fn start_point(sh: &Shape, at: f64, line: usize) -> Option<Pt> {
    let i = line_index(line);
    let b = sh.pts.get(i)?;
    let c = sh.pts.get(i + 1)?;
    Some([b[0] + (c[0] - b[0]) * at, b[1] + (c[1] - b[1]) * at])
}

/// The curve box `(S, U, V)` from the start at `at` on line `line` to wall end `end` (0 / 1);
/// when that wall end lies on the line there is nothing to open, so None (funnel.curve_box).
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
        // the line is parallel to the wall (or is a point): a plain time / pitch box
        ([ex, 0.0], [0.0, ey])
    };
    if (u[0] * v[1] - u[1] * v[0]).abs() < 1e-9 {
        return None;
    }
    Some((s, u, v))
}

/// The point at `(u, f)` in the box (funnel.box_point).
pub fn box_point(box_: &(Pt, Pt, Pt), u: f64, f: f64) -> Pt {
    let (s, ub, vb) = box_;
    [s[0] + u * ub[0] + f * vb[0], s[1] + u * ub[1] + f * vb[1]]
}

/// The `(u, f)` of point `(beat, pitch)` in the curve box (funnel.box_uf).
pub fn box_uf(box_: &(Pt, Pt, Pt), b: f64, p: f64) -> Pt {
    let (s, ub, vb) = box_;
    let det = ub[0] * vb[1] - ub[1] * vb[0];
    let x = b - s[0];
    let y = p - s[1];
    [(x * vb[1] - y * vb[0]) / det, (ub[0] * y - ub[1] * x) / det]
}

/// Each curve as `(start number, wall end, point list from start to wall end, where the line meets the wall)` (funnel.funnel_curves).
/// short: each curve stops a little inside the last key, which is only touched on the wall (for notes that start exactly at the wall).
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

/// A new start at `at` on line `line`, opening towards both wall ends (funnel.new_start); the two curves
/// are linked into one group (on opposite sides of the line, so they look mirrored). None when neither can open.
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

/// The identity of a handle point (the id in funnel.funnel_handles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandleId {
    /// One per start: `(start number)`.
    Start(usize),
    /// A handle point pulled out of an anchor: `(start number, wall end, point number)`.
    Ctrl(usize, usize, usize),
    /// An anchor between the ends: `(start number, wall end, point number)`.
    Anchor(usize, usize, usize),
}

/// `[(beat, pitch, id)]` (funnel.funnel_handles): one "start" per start,
/// one "ctrl" per handle pulled out of an anchor, one "anchor" per anchor between the ends.
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

/// Handle lines `(anchor, handle point, (start number, wall end))`, points are `(beat, pitch)` (funnel.funnel_handle_lines).
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

// ---------------------------------------------------------------- straight parts and areas

/// The straight parts: each line, then the wall, as `(beat, pitch)` polylines (funnel.funnel_strokes).
pub fn funnel_strokes(sh: &Shape) -> Vec<Vec<Pt>> {
    let mut out: Vec<Vec<Pt>> = funnel_segments(sh)
        .into_iter()
        .map(|seg| seg.to_vec())
        .collect();
    out.extend(funnel_curves(sh, false).into_iter().map(|(_, _, c, _)| c));
    out
}

/// Straight segments: each line, then the wall (funnel.funnel_segments).
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

/// Each curve's area: the curve first, then back along the wall and the line (funnel.funnel_polys).
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

/// Is `(beat, pitch)` inside one of the funnel's curve areas (funnel.funnel_contains)?
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

/// The `(first beat, last beat)` where straight line a -> b is on key q, None if never (funnel.line_band).
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

/// The key range covered by a segment / list of points (funnel._keys).
fn keys(ps: &[f64]) -> RangeInclusive<i64> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &y in ps {
        lo = lo.min(y);
        hi = hi.max(y);
    }
    0.max(pitch_of(lo))..=crate::paths::TOP_KEY.min(pitch_of(hi))
}

/// key -> span list (keeping insertion order, a Python dict; `funnel_cells`'s output order follows it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spans {
    entries: Vec<(i64, Vec<[f64; 2]>)>,
}

impl Spans {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this key is present.
    pub fn get(&self, k: i64) -> Option<&[[f64; 2]]> {
        self.entries
            .iter()
            .find(|e| e.0 == k)
            .map(|e| e.1.as_slice())
    }

    /// The span list for key, creating one (in insertion order) when absent.
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

    /// Put in a key's span list (replacing an existing one).
    pub fn insert(&mut self, k: i64, v: Vec<[f64; 2]>) {
        match self.entries.iter().position(|e| e.0 == k) {
            Some(i) => self.entries[i].1 = v,
            None => self.entries.push((k, v)),
        }
    }

    /// Iterate `(key, span list)` in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (i64, &[[f64; 2]])> {
        self.entries.iter().map(|(k, v)| (*k, v.as_slice()))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `{key: [[first beat, last beat], ...]}` (funnel.funnel_key_spans):
/// the range each key plays (sorted, overlaps merged).
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

// ---------------------------------------------------------------- layout and notes

/// The note grid's axis `(t0, sign, length)` (funnel.funnel_axis): the grid runs from the line's start
/// towards the wall (sign -1 = backwards in time: reversed funnel); length = tick distance from start to wall.
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
        // just a vertical line (the wall, drawn first): a column of wall gates ends on it
        return Some((lo, -1, sh.gate1));
    }
    Some((lo, 1, hi - lo)) // the line and wall start at the same time (a turned funnel): left to right
}

/// Is the wall earlier in time than the line's start (funnel.funnel_reversed)?
pub fn funnel_reversed(sh: &Shape) -> bool {
    let pts = &sh.pts;
    pts.len() >= 4 && (pts[2][0] + pts[3][0]) / 2.0 < pts[0][0] - 1e-9
}

/// Everything the notes come from (funnel.funnel_layout): `(spans in grid distance, wall ranges, grid length, t0 tick, sign)`.
/// Grid distance is ticks from the line's start towards the wall. None when the funnel makes no notes.
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelLayout {
    /// Each key's range (grid distance).
    pub dspans: Spans,
    /// The wall's range on each key (grid distance).
    pub walls: BTreeMap<i64, [f64; 2]>,
    /// Tick distance from start to wall (at least 1).
    pub length: f64,
    /// The line start's tick on the time axis.
    pub t0: f64,
    /// The grid's direction towards the wall (-1 = backwards).
    pub sign: i64,
}

/// The funnel's layout (funnel.funnel_layout); None when it makes no notes.
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

/// w(d): 0..1, how open the grid is at distance d, from how many keys play there (1 key = 0, the most = 1; funnel.funnel_openness).
#[derive(Clone, Debug)]
pub struct Openness {
    xs: Vec<f64>,
    counts: Vec<i64>,
    top: i64,
}

/// Build [`Openness`] from spans (funnel.funnel_openness).
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
    /// How open the grid is at distance d.
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

/// The gate of a note w deep (ticks, funnel.funnel_gate): 0 = start gate, 1 = wall gate.
pub fn funnel_gate(sh: &Shape, g0: f64, g1: f64, w: f64) -> f64 {
    let mut g = g0 * (g1 / g0).powf(w);
    if sh.change == GateChange::Steps && g0 != g1 {
        g = g0 * 2.0_f64.powf(round_half_even((g / g0).log2()));
        g = g.clamp(g0.min(g1), g0.max(g1));
    }
    g.max(1.0)
}

/// The note grid shared by every key of a spam funnel (funnel.funnel_grid), in grid distance: from the
/// line's start to the wall (a leftover under half a gate joins the last note), then one more note past everything.
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
        return Vec::new(); // Python raises ValueError for empty dspans (the caller guarantees non-empty)
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
    marks.push(last + gate(last)); // make room for the column beyond the wall
    while marks[0] > lo {
        let first = marks[0];
        marks.insert(0, first - g0);
    }
    marks
}

/// Index in `xs` nearest to x (funnel._nearest).
fn nearest(xs: &[f64], x: f64) -> i64 {
    let mut i = xs.partition_point(|&v| v < x);
    if i > 0 && (i == xs.len() || x - xs[i - 1] <= xs[i] - x) {
        i -= 1;
    }
    i as i64
}

/// spam: `(grid ticks, [(key, first grid line, last grid line)])`, each key's notes run from grid
/// line to grid line. long: `(None, [(key, start tick, end tick)])` (funnel.funnel_cells).
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
                // one whole gate beyond the wall
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
                j = (j + 1).min(ds.len() as i64 - 1); // one note past the wall (for a key on the wall: just that one)
            } else if j <= i {
                // shorter than one note: the note it sits in (on a grid line, the one ending there)
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

/// The funnel's note count (funnel.funnel_note_count).
pub fn funnel_note_count(sh: &Shape, ppq: f64) -> i64 {
    let (ticks, cells) = funnel_cells(sh, ppq);
    match ticks {
        None => cells.len() as i64,
        Some(_) => cells.iter().map(|c| c[2] - c[1]).sum(),
    }
}

/// The funnel's note rows `[start, end, key]` (funnel.funnel_notes).
pub fn funnel_notes(sh: &Shape, ppq: f64) -> Vec<[i64; 3]> {
    let (ticks, cells) = funnel_cells(sh, ppq);
    match ticks {
        None => cells.iter().map(|c| [c[1], c[2], c[0]]).collect(),
        Some(ticks) => {
            // each key: from each grid line to the next, from its first line to last
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
