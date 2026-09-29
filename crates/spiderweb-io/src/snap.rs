//! Snap choices (port of Python `files/snap.py`).
//!
//! The toolbar list (`off`, `bar`, note lengths `"3/16"` ...) and custom snaps
//! `"c:<count>/<note>/<divided by>"` where count is empty (one plain note), a number,
//! `"."` (dotted: 1.5 notes) or `".."` (double dotted: 1.75 notes).
//!
//! - [`custom_parts`] / [`custom_snap`]: parse / spell custom snaps;
//! - [`whole_notes`]: the snap's exact length in whole notes;
//! - [`snap_beats`] / [`snap_ticks`]: the snap step in beats / ticks;
//! - [`clean_snap`]: migrate old project files to the new spellings.

/// The little note the toolbar draws for a list entry: (whole-note fraction denominator,
/// dots, triplet), like upstream `SNAP_LIST`'s tuple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapNote {
    pub note: i64,
    pub dots: i64,
    pub triplet: bool,
}

/// The list, top to bottom (upstream `SNAP_LIST`): (snap, the note it shows).
pub const SNAP_LIST: [(&str, Option<SnapNote>); 18] = [
    ("off", None),
    ("bar", None),
    (
        "1/1",
        Some(SnapNote {
            note: 1,
            dots: 0,
            triplet: false,
        }),
    ),
    (
        "3/4",
        Some(SnapNote {
            note: 2,
            dots: 1,
            triplet: false,
        }),
    ),
    (
        "1/2",
        Some(SnapNote {
            note: 2,
            dots: 0,
            triplet: false,
        }),
    ),
    (
        "1/3",
        Some(SnapNote {
            note: 2,
            dots: 0,
            triplet: true,
        }),
    ),
    (
        "3/8",
        Some(SnapNote {
            note: 4,
            dots: 1,
            triplet: false,
        }),
    ),
    (
        "1/4",
        Some(SnapNote {
            note: 4,
            dots: 0,
            triplet: false,
        }),
    ),
    (
        "1/6",
        Some(SnapNote {
            note: 4,
            dots: 0,
            triplet: true,
        }),
    ),
    (
        "3/16",
        Some(SnapNote {
            note: 8,
            dots: 1,
            triplet: false,
        }),
    ),
    (
        "1/8",
        Some(SnapNote {
            note: 8,
            dots: 0,
            triplet: false,
        }),
    ),
    (
        "1/12",
        Some(SnapNote {
            note: 8,
            dots: 0,
            triplet: true,
        }),
    ),
    (
        "3/32",
        Some(SnapNote {
            note: 16,
            dots: 1,
            triplet: false,
        }),
    ),
    (
        "1/16",
        Some(SnapNote {
            note: 16,
            dots: 0,
            triplet: false,
        }),
    ),
    (
        "1/24",
        Some(SnapNote {
            note: 16,
            dots: 0,
            triplet: true,
        }),
    ),
    (
        "3/64",
        Some(SnapNote {
            note: 32,
            dots: 1,
            triplet: false,
        }),
    ),
    (
        "1/32",
        Some(SnapNote {
            note: 32,
            dots: 0,
            triplet: false,
        }),
    ),
    (
        "1/48",
        Some(SnapNote {
            note: 32,
            dots: 0,
            triplet: true,
        }),
    ),
];

/// The list's snap texts (upstream `SNAPS`).
pub const SNAPS: [&str; 18] = [
    "off", "bar", "1/1", "3/4", "1/2", "1/3", "3/8", "1/4", "1/6", "3/16", "1/8", "1/12", "3/32",
    "1/16", "1/24", "3/64", "1/32", "1/48",
];

/// The snap a new project starts with (upstream `DEFAULT_SNAP`).
pub const DEFAULT_SNAP: &str = "1/16";

/// The custom window's limits (upstream `COUNT_RANGE`, `NOTE_RANGE`, `DIV_RANGE`).
pub const COUNT_RANGE: (i64, i64) = (3, 100);
pub const NOTE_RANGE: (i64, i64) = (1, 128);
pub const DIV_RANGE: (i64, i64) = (1, 100);

/// The parts of a custom snap (upstream `custom_parts`): count is `""`, `"."`, `".."` or a
/// number as text (normalised, so `"007"` becomes `"7"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomParts {
    pub count: String,
    pub note: i64,
    pub div: i64,
}

/// An exact rational (the equivalent of Python's `Fraction`): reduced, denominator positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: i128,
    pub den: i128,
}

impl Ratio {
    /// `num / den`, reduced (None when den is 0).
    pub fn new(num: i128, den: i128) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let sign = if den < 0 { -1 } else { 1 };
        let (n, d) = (num.checked_mul(sign)?, den.unsigned_abs());
        let g = gcd(n.unsigned_abs(), d);
        Some(Self {
            num: n / g as i128,
            den: (d / g) as i128,
        })
    }

    /// Python `float(Fraction)`.
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// Python `int(text)`: surrounding whitespace and a leading `+` / `-` are fine, and
/// underscores may separate digits (`"1_0"` is 10). None when it isn't a whole number
/// (or doesn't fit an i64, which is far beyond every value the snap window allows).
pub fn py_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let (sign, digits) = match t.as_bytes().first()? {
        b'+' => (1i64, &t[1..]),
        b'-' => (-1i64, &t[1..]),
        _ => (1i64, t),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    let mut after_digit = false;
    for ch in digits.chars() {
        if ch.is_ascii_digit() {
            value = value
                .checked_mul(10)?
                .checked_add((ch as u8 - b'0') as i64)?;
            after_digit = true;
        } else if ch == '_' && after_digit {
            after_digit = false;
        } else {
            return None;
        }
    }
    if !after_digit {
        return None;
    }
    Some(sign * value)
}

/// `"c:3/16/1"` -> ("3", 16, 1), or None if it isn't a valid custom snap.
pub fn custom_parts(snap: &str) -> Option<CustomParts> {
    let rest = snap.strip_prefix("c:")?;
    let mut pieces = rest.split('/');
    let count = pieces.next()?;
    let note = pieces.next()?;
    let div = pieces.next()?;
    if pieces.next().is_some() {
        return None;
    }
    // Python a, b, c = text.split("/") raises for the wrong number of pieces.
    let note = py_int(note)?;
    let div = py_int(div)?;
    let count = if count.is_empty() || count == "." || count == ".." {
        count.to_string()
    } else {
        let n = py_int(count)?;
        if !(COUNT_RANGE.0..=COUNT_RANGE.1).contains(&n) {
            return None;
        }
        n.to_string()
    };
    if !(NOTE_RANGE.0..=NOTE_RANGE.1).contains(&note) || !(DIV_RANGE.0..=DIV_RANGE.1).contains(&div)
    {
        return None;
    }
    Some(CustomParts { count, note, div })
}

/// Build a custom snap text from its parts (upstream `custom_snap`).
pub fn custom_snap(count: &str, note: i64, div: i64) -> String {
    format!("c:{count}/{note}/{div}")
}

fn div_int(r: Ratio, n: i128) -> Option<Ratio> {
    Ratio::new(r.num, r.den.checked_mul(n)?)
}

/// The snap's length in whole notes, None for `"off"` / `"bar"` / anything unknown.
pub fn whole_notes(snap: &str) -> Option<Ratio> {
    if SNAPS.contains(&snap) && snap.contains('/') {
        let (a, b) = snap.split_once('/')?;
        return Ratio::new(a.trim().parse().ok()?, b.trim().parse().ok()?);
    }
    let parts = custom_parts(snap)?;
    let many = match parts.count.as_str() {
        "" => Ratio::new(1, 1)?,
        "." => Ratio::new(3, 2)?,
        ".." => Ratio::new(7, 4)?,
        n => Ratio::new(n.parse().ok()?, 1)?,
    };
    div_int(div_int(many, parts.note as i128)?, parts.div as i128)
}

/// The snap's length in beats (`whole_notes * 4`), used by the custom window's info line.
pub fn whole_note_beats(snap: &str) -> Option<f64> {
    let w = whole_notes(snap)?;
    Ratio::new(w.num.checked_mul(4)?, w.den).map(Ratio::to_f64)
}

/// The snap step in beats (quarter notes), None when snapping is off (upstream `snap_beats`).
pub fn snap_beats(snap: &str, beats_per_bar: f64) -> Option<f64> {
    if snap == "bar" {
        return Some(beats_per_bar);
    }
    whole_note_beats(snap)
}

/// The snap step in ticks (one tick with snapping off, upstream `App.snap_ticks`;
/// Python `round` is half to even).
pub fn snap_ticks(snap: &str, beats_per_bar: f64, ppq: i64) -> i64 {
    match snap_beats(snap, beats_per_bar) {
        Some(sb) => ((sb * ppq as f64).round_ties_even() as i64).max(1),
        None => 1,
    }
}

/// A snap read from a file, made valid (upstream `clean_snap`). Older versions had `1/64`
/// and `1/128` in the list: they become custom ones; `"Off"` was written with a capital.
pub fn clean_snap(snap: &str) -> String {
    if SNAPS.contains(&snap) || custom_parts(snap).is_some() {
        return snap.to_string();
    }
    match snap {
        "Off" => "off".to_string(),
        "1/64" => custom_snap("", 64, 1),
        "1/128" => custom_snap("", 128, 1),
        _ => DEFAULT_SNAP.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list and its picture column stay in sync (upstream has them in one literal).
    #[test]
    fn snap_list_matches_snaps() {
        for (i, (snap, _)) in SNAP_LIST.iter().enumerate() {
            assert_eq!(*snap, SNAPS[i], "SNAP_LIST and SNAPS disagree");
        }
    }

    /// The old list values migrate to the new spellings, everything else to the default.
    #[test]
    fn clean_snap_migrates_old_values() {
        assert_eq!(clean_snap("1/16"), "1/16");
        assert_eq!(clean_snap("Off"), "off");
        assert_eq!(clean_snap("1/64"), "c:/64/1");
        assert_eq!(clean_snap("1/128"), "c:/128/1");
        assert_eq!(clean_snap("nope"), "1/16");
        assert_eq!(clean_snap("c:./8/3"), "c:./8/3");
    }
}
