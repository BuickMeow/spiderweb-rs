//! text: laying out text in installed fonts and turning it into a custom shape's letter outlines (a function-by-function port of Python notes/text.py).
//!
//! A text shape is a custom shape; `sh.text` stores the settings (TEXT_DEFAULTS plus text / bbox / cap / k / holes).
//! Letters are laid out in em units (fonts: 1.0 = font size, y goes up from the first line's baseline); bbox = the outline's extent in em units,
//! and `sh.pts` (the custom shape's frame) says where that bbox sits on the roll. So em units -> beats / pitch is decided by the shape itself
//! (text_axes), and re-entering or changing settings keeps the text in place no matter how it was moved, scaled, rotated or sheared.
//!
//! How letters become notes: Fill / Spam use the non-zero winding rule (overlapping strokes stay filled, O / A / B holes stay empty),
//! plus a threshold (how much of a key's height is inside the letters for that key to play there). grow makes strokes thicker (keys, can be negative);
//! it is computed as it looked on screen when entered (k = beats per key at that time).
//!
//! Differences from the original: font lookup is not done here, the caller handles [`text_font`] / [`crate::fonts::get_font`];
//! functions that only need the font cap ([`em_keys`], [`new_axes`], [`shown_size`], [`restyle`]) take the parsed `font_cap` directly,
//! so the pure logic does not depend on which fonts are installed.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::Pt;
use crate::bezier::segments;
use crate::fonts::{Font, get_font};
use crate::shape::{Shape, Stroke, TextAlign, TextSettings, TextUnit};
use crate::{dist, hypot2, round_half_even};

/// Size unit (text.UNITS): font = size is how many keys one font size (em) spans, rows = capital letters are exactly that tall.
pub const UNITS: [TextUnit; 2] = [TextUnit::Font, TextUnit::Rows];

/// Alignment (text.TEXT_ALIGNS).
pub const TEXT_ALIGNS: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

/// The threshold splits each key into this many rows (text.SUB_ROWS, 5% per step).
pub const SUB_ROWS: usize = 20;

/// Flattening tolerance: glyph outlines are flattened with this tolerance (the default of text.flatten).
pub const FLATTEN_TOL: f64 = 0.004;

/// Text shape frame / axes: (O, X, Y); the em point (x, y) is O + x·X + y·Y on the roll (beats, pitch).
pub type Axes = (Pt, Pt, Pt);

/// One laid-out outline: which character it belongs to (glyph numbers increase per character) and the letter outline (a cubic Bezier point list).
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    /// Character number (one per character, spaces included; corresponds to the glyph number in Python layout).
    pub glyph: usize,
    /// Outline point list (anchor, handle, handle, anchor, ..., first and last the same).
    pub pts: Vec<Pt>,
}

/// The equivalent of TEXT_DEFAULTS (with sensible initial bbox / cap / k / holes).
pub fn text_defaults() -> TextSettings {
    TextSettings::default()
}

// ---------------------------------------------------------------- settings sanitising

/// Take text settings from a value in a file (None if broken, corresponds to text.clean_text).
///
/// Matches Python point by point: missing keys use defaults; `float` / `int` accept only numbers, booleans and numeric strings;
/// `unit` / `align` fall back to defaults when not in the whitelist; weight is clamped to 1..1000, threshold to 0..100;
/// cap / k of 0 fall back to 0.7 / 1.0; bbox is required and must be exactly 4 floats; holes are sorted and deduplicated.
///
/// Differences from the original: edge cases of Python `str()` / non-numeric strings (underscore digits, container reprs) are not reproduced,
/// and return None here; negative indices in holes cannot go into `Vec<usize>` and also return None.
pub fn clean_text(value: &Value) -> Option<TextSettings> {
    let d = value.as_object()?;
    let defaults = TextSettings::default();
    let mut out = defaults.clone();
    out.text = match d.get("text") {
        None => String::new(),
        Some(v) => py_str(v)?,
    };
    out.font = match d.get("font") {
        Some(v) if py_bool(v) => py_str(v)?,
        _ => defaults.font.clone(),
    };
    out.size = opt_float(d, "size", defaults.size)?;
    out.tracking = opt_float(d, "tracking", defaults.tracking)?;
    out.leading = opt_float(d, "leading", defaults.leading)?;
    out.threshold = opt_float(d, "threshold", defaults.threshold)?;
    out.grow = opt_float(d, "grow", defaults.grow)?;
    out.unit = match d.get("unit").and_then(Value::as_str) {
        Some("rows") => TextUnit::Rows,
        _ => TextUnit::Font,
    };
    out.align = match d.get("align").and_then(Value::as_str) {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ => TextAlign::Left,
    };
    out.weight = match d.get("weight") {
        None => defaults.weight,
        Some(v) => py_int(v)? as i32,
    };
    out.weight = out.weight.clamp(1, 1000);
    out.italic = d.get("italic").map_or(defaults.italic, py_bool);
    // Python min(100.0, max(0.0, x)): NaN and <= 0 give 0 (even -0.0 becomes +0.0),
    // +inf gives 100; clamp cannot be used directly: it leaves NaN as is and does not change -0.0.
    out.threshold = if out.threshold.is_nan() || out.threshold <= 0.0 {
        0.0
    } else if out.threshold > 100.0 {
        100.0
    } else {
        out.threshold
    };
    let bbox = d.get("bbox")?;
    let bbox = match bbox {
        Value::Array(a) => a.iter().map(py_float).collect::<Option<Vec<f64>>>()?,
        Value::String(s) => s
            .chars()
            .map(|c| py_float(&Value::String(c.to_string())))
            .collect::<Option<Vec<f64>>>()?,
        _ => return None,
    };
    if bbox.len() != 4 {
        return None;
    }
    out.bbox = [bbox[0], bbox[1], bbox[2], bbox[3]];
    let cap = opt_float(d, "cap", 0.7)?;
    out.cap = if cap != 0.0 { cap } else { 0.7 };
    let k = opt_float(d, "k", 1.0)?;
    out.k = if k != 0.0 { k } else { 1.0 };
    out.holes = match d.get("holes") {
        None => Vec::new(),
        Some(Value::Array(a)) => {
            let mut holes: Vec<usize> = Vec::with_capacity(a.len());
            for item in a {
                holes.push(usize::try_from(py_int(item)?).ok()?);
            }
            holes.sort_unstable();
            holes.dedup();
            holes
        }
        Some(Value::String(s)) => {
            let mut holes: Vec<usize> = Vec::with_capacity(s.chars().count());
            for c in s.chars() {
                let v = Value::String(c.to_string());
                holes.push(usize::try_from(py_int(&v)?).ok()?);
            }
            holes.sort_unstable();
            holes.dedup();
            holes
        }
        Some(_) => return None,
    };
    Some(out)
}

/// The text's font (text.text_font).
pub fn text_font(tx: &TextSettings) -> Arc<Font> {
    get_font(&tx.font, tx.weight, tx.italic)
}

/// The equivalent of `tx.get("cap") or font.cap`: cap of 0 (never set) uses the font's cap.
fn cap_or(cap: f64, font_cap: f64) -> f64 {
    if cap != 0.0 { cap } else { font_cap }
}

/// How many keys one em is at font size size (the number in the size box; corresponds to text.em_keys).
pub fn em_keys(tx: &TextSettings, size: Option<f64>, font_cap: f64) -> f64 {
    let size = size.unwrap_or(tx.size);
    if tx.unit == TextUnit::Font {
        size
    } else {
        size / cap_or(tx.cap, font_cap)
    }
}

// ---------------------------------------------------------------- layout

/// Lay out `tx.text` at the font size (corresponds to text.layout): (outlines, carets).
/// Outlines = each character's glyph outlines shifted in em units; carets = every position a caret can go (before each character and at the line end).
pub fn layout(tx: &TextSettings, font: &Font) -> (Vec<Contour>, Vec<Pt>) {
    let step = font.line_height * tx.leading / 100.0;
    let track = tx.tracking / 1000.0;
    let mut contours: Vec<Contour> = Vec::new();
    let mut carets: Vec<Pt> = Vec::new();
    let mut g: usize = 0;
    for (n, line) in tx.text.split('\n').enumerate() {
        let y = -(n as f64) * step;
        let mut xs: Vec<f64> = Vec::new();
        let mut x = 0.0;
        let mut prev: Option<char> = None;
        for ch in line.chars() {
            if let Some(p) = prev {
                x += font.kerning.get(&(p, ch)).copied().unwrap_or(0.0) + track;
            }
            xs.push(x);
            x += font.glyph(ch).advance;
            prev = Some(ch);
        }
        let shift = match tx.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -x / 2.0,
            TextAlign::Right => -x,
        };
        for (ch, &cx) in line.chars().zip(xs.iter()) {
            for c in font.glyph(ch).contours {
                contours.push(Contour {
                    glyph: g,
                    pts: c.iter().map(|p| [p[0] + cx + shift, p[1] + y]).collect(),
                });
            }
            g += 1;
        }
        carets.extend(xs.iter().map(|&cx| [cx + shift, y]));
        carets.push([x + shift, y]);
    }
    (contours, carets)
}

/// Flatten one Bezier curve into a point list: a straight piece takes one step, a curved one enough steps (corresponds to text.flatten).
pub fn flatten(pts: &[Pt], tol: f64) -> Vec<Pt> {
    let Some(&first) = pts.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    for seg in segments(pts) {
        let (p0, p1, p2, p3) = (seg[0], seg[1], seg[2], seg[3]);
        let bend = line_dist(p1, p0, p3).max(line_dist(p2, p0, p3));
        // Python: bend < 1e-9 takes one step, otherwise max(2, min(24, ceil(sqrt(bend/tol) * 2)))
        let n = if bend < 1e-9 {
            1
        } else {
            let n = ((bend / tol).sqrt() * 2.0).ceil();
            if n < 2.0 {
                2
            } else if n > 24.0 {
                24
            } else {
                n as usize
            }
        };
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let mt = 1.0 - t;
            let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
            out.push([
                a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
            ]);
        }
    }
    out
}

/// Distance from point p to segment a-b (corresponds to text._line_dist).
fn line_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let ll = hypot2(dx, dy);
    if ll < 1e-12 {
        dist(p, a)
    } else {
        ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / ll
    }
}

/// Signed area of a closed polygon (corresponds to text.area).
pub fn area(poly: &[Pt]) -> f64 {
    let mut sum = 0.0;
    for w in poly.windows(2) {
        sum += w[0][0] * w[1][1] - w[1][0] * w[0][1];
    }
    sum / 2.0
}

/// Winding number of point (x, y) around the polygon (corresponds to text.winding).
pub fn winding(poly: &[Pt], x: f64, y: f64) -> i64 {
    let mut w = 0i64;
    for e in poly.windows(2) {
        let (xa, ya) = (e[0][0], e[0][1]);
        let (xb, yb) = (e[1][0], e[1][1]);
        if (ya <= y) != (yb <= y) && x < xa + (xb - xa) * (y - ya) / (yb - ya) {
            w += if yb > ya { 1 } else { -1 };
        }
    }
    w
}

/// Which outlines are holes (inside an odd number of the same letter's other outlines; the middle of O; corresponds to text.find_holes).
pub fn find_holes(contours: &[Contour]) -> Vec<usize> {
    let flat: Vec<Vec<Pt>> = contours
        .iter()
        .map(|c| flatten(&c.pts, FLATTEN_TOL))
        .collect();
    let mut holes = Vec::new();
    for (i, c) in contours.iter().enumerate() {
        let Some(first) = flat[i].first() else {
            continue;
        };
        let (x, y) = (first[0], first[1]);
        let depth = contours
            .iter()
            .enumerate()
            .filter(|(j, h)| *j != i && h.glyph == c.glyph && winding(&flat[*j], x, y) != 0)
            .count();
        if depth % 2 == 1 {
            holes.push(i);
        }
    }
    holes
}

// ---------------------------------------------------------------- em units <-> roll

/// (O, X, Y): where the em point (x, y) sits on the roll (corresponds to text.text_axes).
/// None when this is not a text shape (no text, or fewer than 3 frame points).
pub fn text_axes(sh: &Shape) -> Option<Axes> {
    let tx = sh.text.as_ref()?;
    if sh.pts.len() < 3 {
        return None;
    }
    let (b0, p0) = (sh.pts[0][0], sh.pts[0][1]);
    let (b1, p1) = (sh.pts[1][0], sh.pts[1][1]);
    let (b2, p2) = (sh.pts[2][0], sh.pts[2][1]);
    let [x0, y0, x1, y1] = tx.bbox;
    let w = x1 - x0;
    let h = y1 - y0;
    let u = [(b1 - b0) / w, (p1 - p0) / w];
    let v = [(b2 - b0) / h, (p2 - p0) / h];
    let o = [b0 - x0 * u[0] - y0 * v[0], p0 - x0 * u[1] - y0 * v[1]];
    Some((o, u, v))
}

/// Axes for new text: the first line starts at (b, p), the baseline is half a key below p, and capital letters sit on the key clicked and grow upwards
/// (corresponds to text.new_axes).
pub fn new_axes(tx: &TextSettings, b: f64, p: f64, k: f64, font_cap: f64) -> Axes {
    let e = em_keys(tx, None, font_cap);
    ([b, p - 0.5], [e * k, 0.0], [0.0, e])
}

/// How many keys one em is along the axes (the letter height when entered; corresponds to text.axes_em).
pub fn axes_em(axes: Axes, k: f64) -> f64 {
    let (_, _, y) = axes;
    hypot2(y[0] / k, y[1])
}

/// Scale the axes by f (corresponds to text.scale_axes).
pub fn scale_axes(axes: Axes, f: f64) -> Axes {
    let (o, x, y) = axes;
    (o, [x[0] * f, x[1] * f], [y[0] * f, y[1] * f])
}

/// em point (x, y) -> roll (corresponds to text.to_roll).
pub fn to_roll(axes: Axes, x: f64, y: f64) -> Pt {
    let (o, u, v) = axes;
    [o[0] + x * u[0] + y * v[0], o[1] + x * u[1] + y * v[1]]
}

/// (beats, pitch) -> em point; None when the axes are degenerate (flat) (corresponds to text.from_roll).
pub fn from_roll(axes: Axes, b: f64, p: f64) -> Option<Pt> {
    let (o, x, y) = axes;
    let det = x[0] * y[1] - x[1] * y[0];
    if det.abs() < 1e-15 {
        return None;
    }
    let db = b - o[0];
    let dp = p - o[1];
    Some([(db * y[1] - dp * y[0]) / det, (x[0] * dp - x[1] * db) / det])
}

/// Lay tx into shape sh according to axes (strokes, frame and sh.text); nothing to show (no text or only spaces)
/// returns false and leaves sh unchanged (corresponds to text.build).
pub fn build(sh: &mut Shape, tx: &TextSettings, font: &Font, axes: Axes) -> bool {
    let (contours, _) = layout(tx, font);
    if contours.is_empty() {
        return false;
    }
    let pts: Vec<Pt> = contours
        .iter()
        .flat_map(|c| flatten(&c.pts, FLATTEN_TOL))
        .collect();
    let Some(&first) = pts.first() else {
        return false;
    };
    let (mut x0, mut x1) = (first[0], first[0]);
    let (mut y0, mut y1) = (first[1], first[1]);
    for p in &pts {
        x0 = x0.min(p[0]);
        x1 = x1.max(p[0]);
        y0 = y0.min(p[1]);
        y1 = y1.max(p[1]);
    }
    if x1 - x0 < 1e-6 {
        x0 -= 0.01;
        x1 += 0.01;
    }
    if y1 - y0 < 1e-6 {
        y0 -= 0.01;
        y1 += 0.01;
    }
    let w = x1 - x0;
    let h = y1 - y0;
    sh.strokes = contours
        .iter()
        .map(|c| Stroke::Curve {
            pts: c
                .pts
                .iter()
                .map(|p| [round_dp((p[0] - x0) / w, 7), round_dp((p[1] - y0) / h, 7)])
                .collect(),
            sharp: Vec::new(),
            sym: None,
            src: None,
        })
        .collect();
    sh.pts = vec![
        to_roll(axes, x0, y0),
        to_roll(axes, x1, y0),
        to_roll(axes, x0, y1),
    ];
    let mut settings = tx.clone();
    settings.bbox = [x0, y0, x1, y1];
    settings.cap = font.cap;
    settings.holes = find_holes(&contours);
    sh.text = Some(settings);
    sh.name = text_name(&tx.text);
    true
}

/// The equivalent of Python `round(x, 7)` (banker's rounding).
fn round_dp(x: f64, dp: i32) -> f64 {
    let m = 10f64.powi(dp);
    round_half_even(x * m) / m
}

/// The number in the size box for these axes (it follows scaling the frame on the roll; corresponds to text.shown_size).
pub fn shown_size(tx: &TextSettings, axes: Axes, font_cap: f64) -> f64 {
    let e = axes_em(axes, tx.k);
    if tx.unit == TextUnit::Font {
        e
    } else {
        e * cap_or(tx.cap, font_cap)
    }
}

/// One settings change: `None` = leave this item alone (corresponds to a key absent from the Python changes dict).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextChange {
    pub text: Option<String>,
    pub font: Option<String>,
    pub size: Option<f64>,
    pub unit: Option<TextUnit>,
    pub weight: Option<i32>,
    pub italic: Option<bool>,
    pub tracking: Option<f64>,
    pub leading: Option<f64>,
    pub align: Option<TextAlign>,
    pub threshold: Option<f64>,
    pub grow: Option<f64>,
}

impl TextChange {
    /// Merge the changes into the settings (corresponds to Python `dict(tx, **changes)`).
    pub fn apply(&self, tx: &TextSettings) -> TextSettings {
        let mut new = tx.clone();
        if let Some(v) = &self.text {
            new.text = v.clone();
        }
        if let Some(v) = &self.font {
            new.font = v.clone();
        }
        if let Some(v) = self.size {
            new.size = v;
        }
        if let Some(v) = self.unit {
            new.unit = v;
        }
        if let Some(v) = self.weight {
            new.weight = v;
        }
        if let Some(v) = self.italic {
            new.italic = v;
        }
        if let Some(v) = self.tracking {
            new.tracking = v;
        }
        if let Some(v) = self.leading {
            new.leading = v;
        }
        if let Some(v) = self.align {
            new.align = v;
        }
        if let Some(v) = self.threshold {
            new.threshold = v;
        }
        if let Some(v) = self.grow {
            new.grow = v;
        }
        new
    }
}

/// Settings changed (font, size, unit, ...): the new settings and axes (corresponds to text.restyle).
/// The number in the size box stays put unless it was typed in this time, so changing font / unit can make the letters bigger or smaller; the first line's start stays where it was.
///
/// `font_cap` = the old font's cap (used by shown_size only when `tx.cap` is 0);
/// `new_cap` = the new font's cap (corresponds to Python `text_font(new).cap`).
pub fn restyle(
    tx: &TextSettings,
    axes: Axes,
    changes: &TextChange,
    font_cap: f64,
    new_cap: f64,
) -> (TextSettings, Axes) {
    let size = changes
        .size
        .unwrap_or_else(|| shown_size(tx, axes, font_cap));
    let mut new = changes.apply(tx);
    new.cap = new_cap;
    new.size = size;
    let before = axes_em(axes, tx.k);
    if before > 1e-12 {
        let axes = scale_axes(axes, em_keys(&new, Some(size), new_cap) / before);
        (new, axes)
    } else {
        (new, axes)
    }
}

/// Shape name: "the first 24 characters of the first line…" (corresponds to text.text_name).
pub fn text_name(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    if one.chars().count() > 25 {
        let head: String = one.chars().take(24).collect();
        format!("“{head}…”")
    } else {
        format!("“{one}”")
    }
}

// ---------------------------------------------------------------- outlines -> notes

/// The letter outlines as closed polygons in beats / pitch, thickened / thinned by grow (corresponds to text.text_polys).
pub fn text_polys(sh: &Shape) -> Vec<Vec<Pt>> {
    let Some(tx) = sh.text.as_ref() else {
        return Vec::new();
    };
    if sh.pts.len() < 3 {
        return Vec::new();
    }
    let (b0, p0) = (sh.pts[0][0], sh.pts[0][1]);
    let (b1, p1) = (sh.pts[1][0], sh.pts[1][1]);
    let (b2, p2) = (sh.pts[2][0], sh.pts[2][1]);
    let (ub, up, vb, vp) = (b1 - b0, p1 - p0, b2 - b0, p2 - p0);
    let mut polys: Vec<Vec<Pt>> = Vec::new();
    for st in &sh.strokes {
        let stroke_pts: &[Pt] = match st {
            Stroke::Poly { pts, .. } | Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. } => pts,
            Stroke::Ellipse { .. } => continue,
        };
        polys.push(
            flatten(stroke_pts, 0.002)
                .into_iter()
                .map(|[u, v]| [b0 + u * ub + v * vb, p0 + u * up + v * vp])
                .collect(),
        );
    }
    if tx.grow != 0.0 {
        let k = tx.k;
        let holes: HashSet<usize> = tx.holes.iter().copied().collect();
        polys = polys
            .iter()
            .enumerate()
            .map(|(i, poly)| {
                let d = if holes.contains(&i) {
                    -tx.grow
                } else {
                    tx.grow
                };
                let scaled: Vec<Pt> = poly.iter().map(|p| [p[0] / k, p[1]]).collect();
                offset(&scaled, d)
                    .into_iter()
                    .map(|p| [p[0] * k, p[1]])
                    .collect()
            })
            .collect();
    }
    polys
}

/// Grow the "inside" of a closed polygon (first point = last point) outwards by d (negative = shrink); sharp corners are bevelled so they do not fly off
/// (corresponds to text.offset).
pub fn offset(poly: &[Pt], d: f64) -> Vec<Pt> {
    let mut pts: Vec<Pt> = Vec::new();
    for w in poly.windows(2) {
        if dist(w[0], w[1]) > 1e-9 {
            pts.push(w[0]);
        }
    }
    let n = pts.len();
    if n < 3 {
        return poly.to_vec();
    }
    let mut closed = pts.clone();
    closed.push(pts[0]);
    // Counter-clockwise: the inside is on the left, so outwards is to the right
    let s = if area(&closed) > 0.0 { 1.0 } else { -1.0 };
    let mut normals: Vec<Pt> = Vec::with_capacity(n);
    for (i, &a) in pts.iter().enumerate() {
        let b = pts[(i + 1) % n];
        let ll = hypot2(b[0] - a[0], b[1] - a[1]);
        if ll == 0.0 {
            normals.push([0.0, 0.0]);
        } else {
            normals.push([s * (b[1] - a[1]) / ll, -s * (b[0] - a[0]) / ll]);
        }
    }
    let mut out: Vec<Pt> = Vec::new();
    for (i, &p) in pts.iter().enumerate() {
        let a = normals[(i + n - 1) % n];
        let b = normals[i];
        let dot = a[0] * b[0] + a[1] * b[1];
        if dot > -0.5 {
            // At most about 120 degrees: the two moved edges meet at one point
            let m = d / (1.0 + dot);
            out.push([p[0] + (a[0] + b[0]) * m, p[1] + (a[1] + b[1]) * m]);
        } else {
            out.push([p[0] + a[0] * d, p[1] + a[1] * d]);
            out.push([p[0] + b[0] * d, p[1] + b[1] * d]);
        }
    }
    if let Some(&first) = out.first() {
        out.push(first);
    }
    out
}

/// Edges that intersect the row [lo, hi) (corresponds to text.row_edges).
pub fn row_edges(polys: &[Vec<Pt>], lo: f64, hi: f64) -> Vec<[Pt; 2]> {
    let mut out = Vec::new();
    for poly in polys {
        for e in poly.windows(2) {
            let (a, b) = (e[0], e[1]);
            if a[1] != b[1] && a[1].min(b[1]) < hi && a[1].max(b[1]) > lo {
                out.push([a, b]);
            }
        }
    }
    out
}

/// Where the line at height y is inside the polygons (non-zero rule): [(x0, x1)] (corresponds to text.line_spans).
pub fn line_spans(edges: &[[Pt; 2]], y: f64) -> Vec<[f64; 2]> {
    let mut cross: Vec<(f64, i32)> = Vec::new();
    for e in edges {
        let (xa, ya) = (e[0][0], e[0][1]);
        let (xb, yb) = (e[1][0], e[1][1]);
        if (ya <= y) != (yb <= y) {
            cross.push((
                xa + (xb - xa) * (y - ya) / (yb - ya),
                if yb > ya { 1 } else { -1 },
            ));
        }
    }
    // Python tuple sorting: by x first, then direction (-1 before 1); -0.0 and 0.0 compare equal
    cross.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut out = Vec::new();
    let mut w = 0i32;
    let mut start = 0.0f64;
    for (x, d) in cross {
        let before = w;
        w += d;
        if before == 0 && w != 0 {
            start = x;
        } else if before != 0 && w == 0 {
            out.push([start, x]);
        }
    }
    out
}

/// Beat ranges where at least `threshold`% of key q's height is inside the letters (corresponds to text.threshold_spans).
pub fn threshold_spans(polys: &[Vec<Pt>], q: f64, threshold: f64) -> Vec<[f64; 2]> {
    let lo = q - 0.5;
    let edges = row_edges(polys, lo, lo + 1.0);
    if edges.is_empty() {
        return Vec::new();
    }
    let need = 1i64.max((threshold / 100.0 * SUB_ROWS as f64 - 1e-9).ceil() as i64);
    let mut events: Vec<(f64, i32)> = Vec::new();
    for j in 0..SUB_ROWS {
        let y = lo + (j as f64 + 0.5) / SUB_ROWS as f64;
        for s in line_spans(&edges, y) {
            events.push((s[0], 1));
            events.push((s[1], -1));
        }
    }
    events.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut out: Vec<[f64; 2]> = Vec::new();
    let mut count = 0i64;
    let mut start: Option<f64> = None;
    for (x, d) in events {
        let before = count;
        count += i64::from(d);
        if before < need && need <= count {
            start = Some(x);
        } else if count < need && need <= before {
            let Some(s) = start else {
                continue;
            };
            let mut merged = false;
            if let Some(last) = out.last_mut()
                && s - last[1] < 1e-12
            {
                last[1] = x;
                merged = true;
            }
            if !merged && x > s {
                out.push([s, x]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------- Python float() / int() / str() / truthiness

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

/// Python `str(x)`: strings, numbers, booleans, None; container reprs are not reproduced.
fn py_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "True".into() } else { "False".into() }),
        Value::Null => Some("None".into()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(_) | Value::Object(_) => None,
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

/// `tx.get(key, default)` followed by `float()`.
fn opt_float(d: &Map<String, Value>, key: &str, default: f64) -> Option<f64> {
    match d.get(key) {
        None => Some(default),
        Some(v) => py_float(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed square contour as a Bezier point list (one line segment per side).
    fn square(cx: f64, cy: f64, r: f64) -> Contour {
        let corners = [
            [cx - r, cy - r],
            [cx + r, cy - r],
            [cx + r, cy + r],
            [cx - r, cy + r],
        ];
        let mut pts = Vec::new();
        for i in 0..4 {
            pts.extend(crate::arc::line_bezier(corners[i], corners[(i + 1) % 4]));
        }
        Contour { glyph: 0, pts }
    }

    #[test]
    fn find_holes_marks_the_inner_contour() {
        let outer = square(0.0, 0.0, 4.0);
        let inner = square(0.0, 0.0, 1.5);
        assert_eq!(find_holes(&[outer.clone(), inner.clone()]), vec![1]);
        // The contour order differs between system fonts; the result must not.
        assert_eq!(find_holes(&[inner, outer]), vec![0]);
    }
}
