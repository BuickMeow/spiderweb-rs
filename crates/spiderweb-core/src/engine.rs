//! engine: assembling shapes into notes, overlap handling and channel assignment (a function-by-function port of Python notes/engine.py).
//!
//! The shape dictionary in Rust is [`Shape`]; `shape_notes` turns any shape into note rows,
//! `assign_slots` / `resolve_overlaps` / `render` handle channels and overlaps for multiple shapes.
//! The path caches in the Python version (_cached / cached_strokes / cached_arrays / cached_path)
//! are only there for speed and are not implemented here (the result is unchanged).

use std::collections::{HashMap, HashSet};

use crate::arc;
use crate::bezier;
use crate::custom::{block_notes, custom_notes_groups, custom_strokes};
use crate::envelope::{env_values, velocity_env};
use crate::funnel::{funnel_notes, funnel_strokes};
use crate::joined::{is_joined, joined_paths};
use crate::note::Note;
use crate::paths::{dedupe, dot_segment_notes, path_notes};
use crate::shape::{Kind, Shape};
use crate::smooth::smooth_path;
use crate::tumour::tumour_path;
use crate::{Pt, round_half_even, round_i64};

/// kind -> display name (engine.KINDS).
pub const KINDS: [(Kind, &str); 7] = [
    (Kind::Line, "Line"),
    (Kind::Poly, "Polyline"),
    (Kind::Free, "Freehand"),
    (Kind::Curve, "Curve"),
    (Kind::Arc, "Arc"),
    (Kind::Custom, "Custom"),
    (Kind::Funnel, "Funnel"),
];

/// Fixed point names for line / arc / funnel (engine.POINT_NAMES).
pub const POINT_NAMES: [(Kind, &[&str]); 3] = [
    (Kind::Line, &["A", "B"]),
    (Kind::Arc, &["Start", "Through", "End"]),
    (
        Kind::Funnel,
        &["Line start", "Line end", "Wall 1", "Wall 2"],
    ),
];

/// Default velocity and tail dot for new shapes (the values of engine.SHAPE_DEFAULTS).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeDefaults {
    pub vel0: f64,
    pub vel1: f64,
    pub end_dot: bool,
}

/// engine.SHAPE_DEFAULTS。
pub const SHAPE_DEFAULTS: ShapeDefaults = ShapeDefaults {
    vel0: 127.0,
    vel1: 127.0,
    end_dot: false,
};

/// All MIDI channels except 10 (drums); one track per slot (engine.CHANNELS).
pub const CHANNELS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];

/// Names for the panel's point boxes; None for shapes without point boxes (polyline / freehand / custom) (engine.point_names).
pub fn point_names(sh: &Shape) -> Option<Vec<String>> {
    if sh.kind == Kind::Curve {
        let pts = &sh.pts;
        if pts.len() == 4 {
            return Some(
                ["Start", "Handle 1", "Handle 2", "End"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            );
        }
        let last = bezier::anchor_count(pts).saturating_sub(1);
        return Some(
            (0..pts.len())
                .map(|i| {
                    if i % 3 == 0 {
                        if i == 0 {
                            "Start".to_string()
                        } else if i / 3 == last {
                            "End".to_string()
                        } else {
                            format!("Anchor {}", i / 3 + 1)
                        }
                    } else {
                        "  handle".to_string()
                    }
                })
                .collect(),
        );
    }
    if sh.kind == Kind::Funnel {
        let mut out: Vec<String> = POINT_NAMES[2].1.iter().map(|s| s.to_string()).collect();
        for k in 2..sh.pts.len() / 2 {
            out.push(format!("Line {k} start"));
            out.push(format!("Line {k} end"));
        }
        return Some(out);
    }
    POINT_NAMES
        .iter()
        .find(|(kind, _)| *kind == sh.kind)
        .map(|(_, names)| names.iter().map(|s| s.to_string()).collect())
}

// ---------------------------------------------------------------- shapes

/// Build a kind shape with defaults' settings (engine.make_shape). line / funnel keep only the first
/// and last points; curve generates a gentle S curve; funnel's starts begin empty (draw the line first, then the walls, then add curves).
pub fn make_shape(kind: Kind, pts: &[Pt], defaults: &Shape) -> Shape {
    let mut sh = defaults.clone();
    sh.kind = kind;
    sh.pts = match kind {
        Kind::Line | Kind::Funnel => {
            if pts.is_empty() {
                Vec::new()
            } else {
                vec![pts[0], pts[pts.len() - 1]]
            }
        }
        Kind::Curve => {
            if pts.is_empty() {
                Vec::new()
            } else {
                let a = pts[0];
                let b = pts[pts.len() - 1];
                let mid = (a[0] + b[0]) / 2.0;
                vec![a, [mid, a[1]], [mid, b[1]], b]
            }
        }
        _ => pts.to_vec(),
    };
    if kind == Kind::Funnel {
        sh.starts = Vec::new();
    }
    sh
}

/// The shape as one polyline point list (custom shapes: all strokes joined end to end) (engine.shape_path).
pub fn shape_path(sh: &Shape) -> Vec<Pt> {
    if matches!(sh.kind, Kind::Custom | Kind::Funnel) {
        return shape_strokes(sh).into_iter().flatten().collect();
    }
    let mut pts = sh.pts.clone();
    match sh.kind {
        Kind::Curve => pts = bezier::sample(&pts, 240),
        Kind::Arc => pts = arc::arc_points(&pts, sh.k, arc::STEP),
        // Straightened / perfect shapes (the drawn points are still kept)
        Kind::Free if sh.smooth != 0 => pts = smooth_path(&pts, sh.smooth as f64, sh.k),
        _ => {}
    }
    if let Some(tm) = &sh.tumour
        && tm.on
    {
        return tumour_path(&pts, tm);
    }
    pts
}

/// The shape's polylines (only custom shapes, funnels and joined curves have more than one)
/// (engine.shape_strokes).
pub fn shape_strokes(sh: &Shape) -> Vec<Vec<Pt>> {
    match sh.kind {
        Kind::Custom => custom_strokes(sh),
        Kind::Funnel => funnel_strokes(sh),
        _ if is_joined(sh) => joined_paths(sh), // a joined curve: one path per piece
        _ => vec![shape_path(sh)],
    }
}

// ---------------------------------------------------------------- notes

/// Of repeated rows in a keep only the first (order preserved) (engine.unique_rows).
///
/// NumPy's lexsort is a stable sort, so the first occurrence is the first row in each group;
/// when `new.all()` return as is, otherwise return sorted by the original index of first occurrence.
pub fn unique_rows<T: Ord + Clone>(rows: &[T]) -> Vec<T> {
    if rows.len() < 2 {
        return rows.to_vec();
    }
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&i, &j| rows[i].cmp(&rows[j]));
    let mut new = vec![true; rows.len()];
    for k in 1..rows.len() {
        new[k] = rows[order[k]] != rows[order[k - 1]];
    }
    if new.iter().all(|&x| x) {
        return rows.to_vec();
    }
    let mut firsts: Vec<usize> = (0..rows.len())
        .filter(|&k| new[k])
        .map(|k| order[k])
        .collect();
    firsts.sort_unstable();
    firsts.into_iter().map(|i| rows[i].clone()).collect()
}

/// One shape's notes (start, end, pitch, velocity), ticks; keys is the project's key range
/// (notes are in 0..keys) (engine.shape_notes).
pub fn shape_notes(sh: &Shape, ppq: f64, keys: i64) -> Vec<Note> {
    shape_notes_tracks(sh, ppq, keys).0
}

/// Convenience wrapper for [`shape_notes`] with the default 128 keys (the original keys=128).
pub fn shape_notes_default(sh: &Shape, ppq: f64) -> Vec<Note> {
    shape_notes(sh, ppq, crate::paths::KEYS[0])
}

/// shape_notes; for pasted notes it additionally returns which track each note came from (one number
/// per row, None for other shapes) (engine.shape_notes_tracks). Per-shape notes keep slot/owner 0.
pub fn shape_notes_tracks(sh: &Shape, ppq: f64, keys: i64) -> (Vec<Note>, Option<Vec<u32>>) {
    let end_dot = sh.end_dot;
    // Drawing uses the same batch of points; dedupe also drops duplicates across stroke boundaries
    let strokes = shape_strokes(sh);
    let mut path: Vec<Pt> = dedupe(&strokes.iter().flatten().copied().collect::<Vec<Pt>>());
    if path.is_empty() {
        return (Vec::new(), None);
    }
    if path[path.len() - 1][0] < path[0][0] {
        // Drawn right to left: treat the "last point" as the later end in time, consistent with left to right
        path.reverse();
    }
    for p in &mut path {
        p[0] *= ppq; // beats -> ticks
    }
    let mut own: Option<Vec<[i64; 2]>> = None;
    // A custom shape made of other shapes (convert.py): which of them each note came from.
    let mut groups: Option<Vec<i64>> = None;
    let mut raw: Vec<[i64; 3]>;
    if sh.kind == Kind::Custom && sh.notes.is_some() {
        let rows = block_notes(sh, ppq).unwrap_or_default();
        raw = rows.iter().map(|r| [r[0], r[1], r[2]]).collect();
        own = Some(rows.iter().map(|r| [r[3], r[4]]).collect());
    } else if sh.kind == Kind::Custom {
        let (rows, ids) = custom_notes_groups(sh, ppq);
        raw = rows;
        groups = ids;
    } else if sh.kind == Kind::Funnel {
        raw = funnel_notes(sh, ppq);
    } else if crate::joined::LINE_KINDS.contains(&sh.kind) && strokes.len() > 1 {
        // every piece of a joined curve makes its own notes, like a line of its own (no notes across a gap)
        let mut pieces: Vec<[i64; 3]> = Vec::new();
        for a in &strokes {
            let mut a = dedupe(a);
            if let (Some(first), Some(last)) = (a.first().copied(), a.last().copied())
                && last[0] < first[0]
            {
                a.reverse();
            }
            let scaled: Vec<Pt> = a.iter().map(|p| [p[0] * ppq, p[1]]).collect();
            pieces.extend(path_notes(&scaled, end_dot));
        }
        raw = pieces;
    } else if end_dot && sh.kind == Kind::Poly && path.len() > 2 {
        raw = dot_segment_notes(&path);
    } else {
        raw = path_notes(&path, end_dot);
    }
    let t_lo = path.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let t_hi = path.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let env = velocity_env(sh);
    let keep: Vec<bool> = raw
        .iter()
        .map(|r| r[2] >= 0 && r[2] < keys && r[1] > 0)
        .collect();
    raw = raw
        .into_iter()
        .zip(keep.iter())
        .filter(|(_, k)| **k)
        .map(|(r, _)| r)
        .collect();
    for r in &mut raw {
        r[0] = r[0].max(0);
    }
    let mut tracks: Option<Vec<u32>> = None;
    if let Some(own) = own {
        let own: Vec<[i64; 2]> = own
            .into_iter()
            .zip(keep.iter())
            .filter(|(_, k)| **k)
            .map(|(r, _)| r)
            .collect();
        if sh.own_vel {
            // (the same note in two tracks stays twice: they can be assigned different channels)
            let rows: Vec<[i64; 5]> = raw
                .iter()
                .zip(own.iter())
                .map(|(r, o)| [r[0], r[1], r[2], o[0], o[1]])
                .collect();
            let got = unique_rows(&rows);
            return (
                got.iter()
                    .map(|r| Note::new(r[0], r[1], r[2], r[3]))
                    .collect(),
                Some(got.iter().map(|r| r[4].max(0) as u32).collect()),
            );
        }
        let rows: Vec<[i64; 4]> = raw
            .iter()
            .zip(own.iter())
            .map(|(r, o)| [r[0], r[1], r[2], o[1]])
            .collect();
        let got = unique_rows(&rows);
        raw = got.iter().map(|r| [r[0], r[1], r[2]]).collect();
        tracks = Some(got.iter().map(|r| r[3].max(0) as u32).collect());
    } else if let Some(groups) = groups {
        let groups: Vec<i64> = groups
            .into_iter()
            .zip(keep.iter())
            .filter(|(_, k)| **k)
            .map(|(g, _)| g)
            .collect();
        // (the same note from two of them stays twice, like two shapes)
        let rows: Vec<[i64; 4]> = raw
            .iter()
            .zip(groups.iter())
            .map(|(r, &g)| [r[0], r[1], r[2], g])
            .collect();
        let got = unique_rows(&rows);
        raw = got.iter().map(|r| [r[0], r[1], r[2]]).collect();
        tracks = Some(got.iter().map(|r| r[3].max(0) as u32).collect());
    } else {
        raw = unique_rows(&raw);
    }
    let vel: Vec<i64> = if env.iter().all(|p| p[1] == env[0][1]) {
        // the same velocity everywhere
        vec![round_i64(env[0][1]).clamp(1, 127); raw.len()]
    } else {
        let frac: Vec<f64> = if t_hi > t_lo {
            raw.iter()
                .map(|r| ((r[0] as f64 - t_lo) / (t_hi - t_lo)).clamp(0.0, 1.0))
                .collect()
        } else {
            vec![0.0; raw.len()]
        };
        // (rounds halves to even, same as round)
        env_values(&env, &frac)
            .iter()
            .map(|&v| round_half_even(v).clamp(1.0, 127.0) as i64)
            .collect()
    };
    let notes = raw
        .iter()
        .zip(vel.iter())
        .map(|(r, &v)| Note::new(r[0], r[1], r[2], v))
        .collect();
    (notes, tracks)
}

/// Convenience wrapper for [`shape_notes_tracks`] with the default 128 keys (the original keys=128).
pub fn shape_notes_tracks_default(sh: &Shape, ppq: f64) -> (Vec<Note>, Option<Vec<u32>>) {
    shape_notes_tracks(sh, ppq, crate::paths::KEYS[0])
}

// ---------------------------------------------------------------- overlaps and channels

/// Channel split mode (the split of engine.assign_slots).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// Only notes with the same time and key count as a clash.
    Key,
    /// Any notes sounding at the same time count as a clash, regardless of key.
    Time,
}

/// The group's maximum so far (inclusive) at each position, restarting on a new group; groups are
/// already sorted with equal groups adjacent (engine.running_max).
pub fn running_max(values: &[i64], groups: &[i64]) -> Vec<i64> {
    let mut out = vec![0i64; values.len()];
    for i in 0..values.len() {
        if i > 0 && groups[i] == groups[i - 1] {
            out[i] = out[i - 1].max(values[i]);
        } else {
            out[i] = values[i];
        }
    }
    out
}

/// Auto channels: shapes with overlapping notes get different slots, non-clashing ones reuse the
/// smallest free slot; earlier shapes get smaller slots (engine.assign_slots). Back-to-back notes of
/// a shape with the same key are first merged into one span (spam joined into a run = one span);
/// anything overlapping that span overlaps one of its notes; zero-length notes count individually.
///
/// `apart`: groups of note list numbers that always get different slots (a custom shape's outline
/// and inside, or convert.py's per-source groups).
pub fn assign_slots(note_lists: &[Vec<Note>], split: Split, apart: &[Vec<usize>]) -> Vec<usize> {
    let n = note_lists.len();
    let key_of = |r: &Note| if split == Split::Key { r.key as i64 } else { 0 };
    let mut by_pitch: HashMap<i64, Vec<(i64, i64, usize)>> = HashMap::new();
    for (owner, notes) in note_lists.iter().enumerate() {
        for r in notes {
            if r.end <= r.start {
                by_pitch
                    .entry(key_of(r))
                    .or_default()
                    .push((r.start as i64, r.end as i64, owner));
            }
        }
        let long: Vec<&Note> = notes.iter().filter(|r| r.end > r.start).collect();
        if long.is_empty() {
            continue;
        }
        let mut order: Vec<usize> = (0..long.len()).collect();
        order.sort_by(|&i, &j| {
            (key_of(long[i]), long[i].start).cmp(&(key_of(long[j]), long[j].start))
        });
        let s: Vec<i64> = order.iter().map(|&i| long[i].start as i64).collect();
        let e: Vec<i64> = order.iter().map(|&i| long[i].end as i64).collect();
        let k: Vec<i64> = order.iter().map(|&i| key_of(long[i])).collect();
        let run = running_max(&e, &k);
        let mut new = vec![true; s.len()];
        for i in 1..s.len() {
            new[i] = k[i] != k[i - 1] || s[i] > run[i - 1];
        }
        let at: Vec<usize> = (0..s.len()).filter(|&i| new[i]).collect();
        for (idx, &st) in at.iter().enumerate() {
            let stop = at.get(idx + 1).copied().unwrap_or(s.len());
            let mut e0 = e[st];
            for &x in &e[st..stop] {
                e0 = e0.max(x);
            }
            by_pitch.entry(k[st]).or_default().push((s[st], e0, owner));
        }
    }
    let mut clashes: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for items in by_pitch.values_mut() {
        items.sort_unstable();
        let mut active: Vec<(i64, usize)> = Vec::new();
        for &(s, e, o) in items.iter() {
            active.retain(|&(ae, _)| ae > s);
            for &(_, ao) in &active {
                if ao != o {
                    clashes[o].insert(ao);
                    clashes[ao].insert(o);
                }
            }
            active.push((e, o));
        }
    }
    for group in apart {
        for &a in group {
            if a >= n {
                continue;
            }
            for &b in group {
                if b != a && b < n {
                    clashes[a].insert(b);
                }
            }
        }
    }
    let first: Vec<f64> = note_lists
        .iter()
        .map(|notes| {
            notes
                .iter()
                .map(|r| r.start as i64)
                .min()
                .map(|m| m as f64)
                .unwrap_or(f64::INFINITY)
        })
        .collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| first[i].total_cmp(&first[j]).then(i.cmp(&j)));
    let mut slots = vec![0usize; n];
    let mut done = vec![false; n];
    for i in order {
        let used: HashSet<usize> = clashes[i]
            .iter()
            .filter(|&&j| done[j])
            .map(|&j| slots[j])
            .collect();
        let mut k = 0;
        while used.contains(&k) {
            k += 1;
        }
        slots[i] = k;
        done[i] = true;
    }
    slots
}

/// Overlapping notes at the same pitch and slot: the earlier one is cut where the later one starts,
/// and the later one is stretched to where the earlier would have ended (if farther). Notes starting
/// at the same tick merge into one: the highest velocity wins and keeps the longest length.
/// `notes` carry start, end, pitch, velocity, slot and owner; returns the fixed notes grouped by
/// slot + key (groups in order of first appearance), sorted within a group (engine.resolve_overlaps).
pub fn resolve_overlaps(notes: &[Note]) -> Vec<Note> {
    if notes.is_empty() {
        return Vec::new();
    }
    // key = slot * 256 + pitch (256 keys, so slots never collide); group numbers follow first appearance in the input
    let mut group_of_key: HashMap<i64, usize> = HashMap::new();
    let mut group: Vec<usize> = Vec::with_capacity(notes.len());
    for r in notes {
        let key = r.slot as i64 * 256 + r.key as i64;
        let next = group_of_key.len();
        group.push(*group_of_key.entry(key).or_insert(next));
    }
    // Same start: the higher velocity sorts last, so that is the one kept
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&i, &j| {
        group[i]
            .cmp(&group[j])
            .then(notes[i].start.cmp(&notes[j].start))
            .then(notes[i].vel.cmp(&notes[j].vel))
            .then(notes[j].end.cmp(&notes[i].end))
    });
    let a: Vec<Note> = order.iter().map(|&i| notes[i]).collect();
    let group: Vec<usize> = order.iter().map(|&i| group[i]).collect();
    let s: Vec<i64> = a.iter().map(|r| r.start as i64).collect();
    let e: Vec<i64> = a.iter().map(|r| r.end as i64).collect();
    let groups: Vec<i64> = group.iter().map(|&g| g as i64).collect();
    // Where all earlier notes in the group ring until
    let run = running_max(&e, &groups);
    let mut same = vec![false; a.len()];
    for i in 1..a.len() {
        same[i] = group[i] == group[i - 1];
    }
    let mut over = vec![false; a.len()];
    for i in 0..a.len() {
        let before = if i > 0 { run[i - 1] } else { 0 };
        over[i] = same[i] && s[i] < before;
    }
    let mut end = e.clone();
    for i in 0..a.len() {
        if over[i] {
            let before = if i > 0 { run[i - 1] } else { 0 };
            end[i] = end[i].max(before); // stretched to where it would have ended
        }
    }
    for i in 0..a.len().saturating_sub(1) {
        if over[i + 1] {
            end[i] = s[i + 1]; // the earlier one is cut here
        }
    }
    let mut last = vec![true; a.len()];
    for i in 0..a.len().saturating_sub(1) {
        last[i] = !same[i + 1];
    }
    let mut out = Vec::new();
    for i in 0..a.len() {
        if last[i] || end[i] > s[i] {
            let mut r = a[i];
            r.end = end[i] as u32;
            out.push(r);
        }
    }
    out
}

/// Channel mode (engine.CHANNEL_MODES).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One channel, notes stay as they are (overlaps allowed).
    Raw,
    /// One channel, overlaps fixed.
    Single,
    /// Overlapping shapes each get a channel.
    Auto,
}

/// Slot assignment per shape: one slot for the whole shape, or one slot per note.
enum SlotMap {
    One(usize),
    Each(Vec<usize>),
}

/// Notes of a group of shapes -> (final notes, number of slots used) (engine.render).
///
/// note_lists: every shape's shape_notes result. mode: see [`Mode`]. tracks: per shape None, or the
/// track of each of its notes (pasted notes and convert.py's groups, shape_notes_tracks): with
/// "auto" each track of the shape gets channels as if it were a shape of its own. apart: per shape
/// True if its tracks must get different channels (Fill / Spam "Outline").
pub fn render(
    note_lists: &[Vec<Note>],
    mode: Mode,
    split: Split,
    tracks: Option<&[Option<Vec<u32>>]>,
    apart: Option<&[bool]>,
) -> (Vec<Note>, usize) {
    let empty: Vec<Option<Vec<u32>>> = Vec::new();
    let tracks = tracks.unwrap_or(&empty);
    let no_apart: Vec<bool> = Vec::new();
    let apart = apart.unwrap_or(&no_apart);
    let (slot_of, count) = if mode == Mode::Auto {
        // shapes, and pasted notes split by track; unit_of = each note's unit
        let mut units: Vec<Vec<Note>> = Vec::new();
        let mut unit_of: Vec<SlotMap> = Vec::new();
        let mut forced: Vec<Vec<usize>> = Vec::new();
        for (o, lst) in note_lists.iter().enumerate() {
            let tr = tracks.get(o).and_then(|t| t.as_ref());
            match tr {
                Some(tr) if !lst.is_empty() => {
                    // np.unique: ids ascending, which = index of each note's track in ids
                    let mut ids: Vec<u32> = tr.clone();
                    ids.sort_unstable();
                    ids.dedup();
                    let base = units.len();
                    let which: Vec<usize> = (0..lst.len())
                        .map(|i| {
                            let t = tr.get(i).copied().unwrap_or(0);
                            ids.binary_search(&t).unwrap_or(0)
                        })
                        .collect();
                    for k in 0..ids.len() {
                        units.push(
                            lst.iter()
                                .zip(which.iter())
                                .filter(|&(_, &w)| w == k)
                                .map(|(r, _)| *r)
                                .collect(),
                        );
                    }
                    unit_of.push(SlotMap::Each(which.iter().map(|&w| base + w).collect()));
                    if apart.get(o).copied().unwrap_or(false) {
                        forced.push((base..base + ids.len()).collect());
                    }
                }
                _ => {
                    unit_of.push(SlotMap::One(units.len()));
                    units.push(lst.clone());
                }
            }
        }
        let unit_slots = assign_slots(&units, split, &forced);
        let count = unit_slots.iter().max().map_or(0, |&m| m + 1);
        let slot_of: Vec<SlotMap> = unit_of
            .into_iter()
            .map(|u| match u {
                SlotMap::One(k) => SlotMap::One(unit_slots[k]),
                SlotMap::Each(v) => SlotMap::Each(v.iter().map(|&k| unit_slots[k]).collect()),
            })
            .collect();
        (slot_of, count)
    } else {
        (
            note_lists.iter().map(|_| SlotMap::One(0)).collect(),
            usize::from(!note_lists.is_empty()),
        )
    };
    let mut notes: Vec<Note> = Vec::new();
    for (o, lst) in note_lists.iter().enumerate() {
        for (i, r) in lst.iter().enumerate() {
            let slot = match &slot_of[o] {
                SlotMap::One(s) => *s,
                SlotMap::Each(v) => v[i],
            };
            notes.push(Note {
                start: r.start,
                end: r.end,
                key: r.key,
                vel: r.vel,
                slot: slot.min(254) as u8,
                flags: 0,
                owner: o as u32,
            });
        }
    }
    if mode != Mode::Raw {
        notes = resolve_overlaps(&notes);
    }
    (notes, count)
}

/// Slot number -> (track number, MIDI channel 0-15), one channel per track, skipping the drum channel (engine.slot_track_channel).
pub fn slot_track_channel(slot: usize) -> (usize, u8) {
    (slot, CHANNELS[slot % CHANNELS.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compact model must not change the rendered rows (the vectors cover this exhaustively;
    /// this is the small explicit case of the first Python `render` vectors).
    #[test]
    fn render_rows_are_unchanged_by_the_compact_model() {
        let a = vec![Note::new(0, 200, 60, 100)];
        let b = vec![Note::new(100, 300, 60, 90)];
        let (notes, count) = render(&[a.clone(), b.clone()], Mode::Raw, Split::Key, None, None);
        let rows: Vec<[i64; 6]> = notes.iter().map(|n| n.row6()).collect();
        assert_eq!(
            rows,
            vec![[0, 200, 60, 100, 0, 0], [100, 300, 60, 90, 0, 1]]
        );
        assert_eq!(count, 1);

        // Auto: the two clashing shapes get one slot each
        let (notes, count) = render(&[a, b], Mode::Auto, Split::Key, None, None);
        let rows: Vec<[i64; 6]> = notes.iter().map(|n| n.row6()).collect();
        assert_eq!(
            rows,
            vec![[0, 200, 60, 100, 0, 0], [100, 300, 60, 90, 1, 1]]
        );
        assert_eq!(count, 2);
    }

    #[test]
    fn pasted_notes_keep_their_tracks() {
        // Domino rows: tick, gate, key, velocity, track (two tracks on the same key)
        let rows = vec![
            [0i64, 200, 60, 100, 0],
            [100, 200, 60, 90, 1],
            [300, 100, 64, 100, 0],
            [300, 100, 64, 90, 1],
        ];
        let sh = crate::custom::notes_shape(&rows, 960.0, "Pasted notes").expect("shape");
        let (notes, tracks) = shape_notes_tracks(&sh, 960.0, 128);
        let tracks = tracks.expect("pasted notes must report tracks");
        assert_eq!(tracks.len(), notes.len());
        assert!(tracks.contains(&1), "track 1 survives the round trip");
        // Multi channel: each pasted track gets its own slot
        let (rendered, count) = render(
            &[notes],
            Mode::Auto,
            Split::Key,
            Some(&[Some(tracks)]),
            None,
        );
        assert_eq!(count, 2, "two tracks -> two slots");
        assert!(rendered.iter().any(|n| n.slot == 0));
        assert!(rendered.iter().any(|n| n.slot == 1));
    }

    #[test]
    fn shape_notes_keep_slot_and_owner_zero() {
        let sh = Shape::new(Kind::Line, vec![[0.0, 60.0], [1.0, 64.0]]);
        let notes = shape_notes(&sh, 960.0, 128);
        assert!(!notes.is_empty());
        assert!(
            notes
                .iter()
                .all(|n| n.slot == 0 && n.flags == 0 && n.owner == 0)
        );
    }
}
