//! Spiderweb core: the pure algorithm layer turning shapes into notes (no UI / platform dependencies).
//!
//! One-to-one module mapping with the original Python:
//! - [`paths`] lines / polylines / freehand / curves -> notes
//! - [`bezier`] Bezier curve sampling, editing and fitting
//! - [`arc`] three-point arcs
//! - [`smooth`] freehand "Straighten" (line / smooth curve / perfect shape)
//! - [`tumour`] tumours (bumps) on a line
//! - [`envelope`] velocity envelope
//! - [`custom`] custom shapes (outline / fill / spam) and pasted notes
//! - [`joined`] joining lines / curves / arcs into one Curve shape (1.2.0)
//! - [`convert`] "Turn into live shape": lines / curves / arcs / custom shapes -> one custom shape
//! - [`funnel`] funnel
//! - [`text`] text -> glyph outlines
//! - [`engine`] assembly: shapes -> notes, overlap handling, channel assignment

pub mod arc;
pub mod bezier;
pub mod convert;
pub mod custom;
pub mod engine;
pub mod envelope;
pub mod fonts;
pub mod funnel;
pub mod joined;
pub mod paths;
pub mod pyrandom;
pub mod shape;
pub mod smooth;
pub mod text;
pub mod tumour;

/// A shape point: (beat, pitch), both floating point.
pub type Pt = [f64; 2];

/// (start, end, key) note row; ticks are integers.
pub type Note3 = [i64; 3];

/// (start, end, key, velocity) note row.
pub type Note4 = [i64; 4];

/// (start, end, key, velocity, slot, owner) note row (the final form of engine.render).
pub type Note6 = [i64; 6];

/// Reimplementation of CPython 3.9 `math.hypot`: scale by the largest component,
/// then use Neumaier compensated summation
/// (see vector_norm in CPython Modules/mathmodule.c; a plain sqrt(x²+y²) has ulp
/// differences, while the differential vectors require bit-for-bit equality).
pub fn hypot2(x0: f64, x1: f64) -> f64 {
    let x0 = x0.abs();
    let x1 = x1.abs();
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

/// Distance between two points (same as CPython `math.dist`).
pub fn dist(a: Pt, b: Pt) -> f64 {
    hypot2(a[0] - b[0], a[1] - b[1])
}

/// Banker's rounding, as in Python `round()` / NumPy `round()` (.5 rounds to even).
pub fn round_half_even(x: f64) -> f64 {
    let f = x.floor();
    let frac = x - f;
    if frac == 0.5 {
        if (f as i64) % 2 == 0 { f } else { f + 1.0 }
    } else {
        x.round()
    }
}

/// Python `round()`, to i64.
pub fn round_i64(x: f64) -> i64 {
    round_half_even(x) as i64
}

/// Python `math.floor(x + 0.5)`: the usual way to round a tick.
pub fn floor_half(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}
