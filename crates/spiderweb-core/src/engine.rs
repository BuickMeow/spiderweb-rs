//! engine：形状汇总成音符、重叠处理与通道分配（Python notes/engine.py 的逐函数移植）。
//!
//! 形状字典在 Rust 里就是 [`Shape`]；`shape_notes` 把任意形状变成音符行，
//! `assign_slots` / `resolve_overlaps` / `render` 负责多形状时的通道与重叠。
//! Python 版里的路径缓存（_cached / cached_strokes / cached_arrays / cached_path）
//! 只是加速用的缓存，这里不实现（结果不变）。

use std::collections::{HashMap, HashSet};

use crate::arc;
use crate::bezier;
use crate::custom::{block_notes, custom_notes_groups, custom_strokes};
use crate::envelope::{env_values, velocity_env};
use crate::funnel::{funnel_notes, funnel_strokes};
use crate::joined::{is_joined, joined_paths};
use crate::paths::{dedupe, dot_segment_notes, path_notes};
use crate::shape::{Kind, Shape};
use crate::smooth::smooth_path;
use crate::tumour::tumour_path;
use crate::{Note4, Note6, Pt, round_half_even, round_i64};

/// kind → 显示名（engine.KINDS）。
pub const KINDS: [(Kind, &str); 7] = [
    (Kind::Line, "Line"),
    (Kind::Poly, "Polyline"),
    (Kind::Free, "Freehand"),
    (Kind::Curve, "Curve"),
    (Kind::Arc, "Arc"),
    (Kind::Custom, "Custom"),
    (Kind::Funnel, "Funnel"),
];

/// line / arc / funnel 的固定点名字（engine.POINT_NAMES）。
pub const POINT_NAMES: [(Kind, &[&str]); 3] = [
    (Kind::Line, &["A", "B"]),
    (Kind::Arc, &["Start", "Through", "End"]),
    (
        Kind::Funnel,
        &["Line start", "Line end", "Wall 1", "Wall 2"],
    ),
];

/// 新形状的默认速度与尾点（engine.SHAPE_DEFAULTS 的值）。
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

/// 除 10（鼓）以外的所有 MIDI 通道；每个 slot 一条轨道（engine.CHANNELS）。
pub const CHANNELS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];

/// 面板点框的名字；没有点框的形状（折线 / 自由笔 / 自定义）为 None（engine.point_names）。
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

// ---------------------------------------------------------------- 形状

/// 用 defaults 的设置造一个 kind 形状（engine.make_shape）。line / funnel 只留首尾点；
/// curve 生成一条温和的 S 曲线；funnel 的 starts 从空开始（先画线、再画墙、再加曲线）。
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

/// 形状为一条折线点列（自定义形状：所有笔画首尾接起来）（engine.shape_path）。
pub fn shape_path(sh: &Shape) -> Vec<Pt> {
    if matches!(sh.kind, Kind::Custom | Kind::Funnel) {
        return shape_strokes(sh).into_iter().flatten().collect();
    }
    let mut pts = sh.pts.clone();
    match sh.kind {
        Kind::Curve => pts = bezier::sample(&pts, 240),
        Kind::Arc => pts = arc::arc_points(&pts, sh.k, arc::STEP),
        // 画整齐的 / 完美的形状（画下的点还留着）
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

// ---------------------------------------------------------------- 音符

/// a 里重复出现的行只留第一次（顺序保持）（engine.unique_rows）。
///
/// NumPy 的 lexsort 是稳定排序，所以首次出现的行就是每组里的第一行；
/// `new.all()` 时原样返回，否则按首次出现的原下标排序返回。
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

/// 一个形状的音符 (start, end, pitch, velocity)，tick；keys 是工程的按键范围
/// （音符在 0..keys）（engine.shape_notes）。
pub fn shape_notes(sh: &Shape, ppq: f64, keys: i64) -> Vec<Note4> {
    shape_notes_tracks(sh, ppq, keys).0
}

/// [`shape_notes`] 的默认 128 键便捷包装（原版 keys=128）。
pub fn shape_notes_default(sh: &Shape, ppq: f64) -> Vec<Note4> {
    shape_notes(sh, ppq, crate::paths::KEYS[0])
}

/// shape_notes；粘贴的音符额外返回每条音符来自哪条轨道（每行一个数，其它形状 None）
/// （engine.shape_notes_tracks）。
pub fn shape_notes_tracks(sh: &Shape, ppq: f64, keys: i64) -> (Vec<Note4>, Option<Vec<i64>>) {
    let end_dot = sh.end_dot;
    // 画线用的是同一批点；dedupe 也跨笔画边界去掉重复点
    let strokes = shape_strokes(sh);
    let mut path: Vec<Pt> = dedupe(&strokes.iter().flatten().copied().collect::<Vec<Pt>>());
    if path.is_empty() {
        return (Vec::new(), None);
    }
    if path[path.len() - 1][0] < path[0][0] {
        // 从右往左画的：把"最后一点"当成时间上更晚的那端，与从左往右一致
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
    let mut tracks: Option<Vec<i64>> = None;
    if let Some(own) = own {
        let own: Vec<[i64; 2]> = own
            .into_iter()
            .zip(keep.iter())
            .filter(|(_, k)| **k)
            .map(|(r, _)| r)
            .collect();
        if sh.own_vel {
            // （同一条音符在两条轨道里就留两条：它们可以分到不同通道）
            let rows: Vec<[i64; 5]> = raw
                .iter()
                .zip(own.iter())
                .map(|(r, o)| [r[0], r[1], r[2], o[0], o[1]])
                .collect();
            let got = unique_rows(&rows);
            return (
                got.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect(),
                Some(got.iter().map(|r| r[4]).collect()),
            );
        }
        let rows: Vec<[i64; 4]> = raw
            .iter()
            .zip(own.iter())
            .map(|(r, o)| [r[0], r[1], r[2], o[1]])
            .collect();
        let got = unique_rows(&rows);
        raw = got.iter().map(|r| [r[0], r[1], r[2]]).collect();
        tracks = Some(got.iter().map(|r| r[3]).collect());
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
        tracks = Some(got.iter().map(|r| r[3]).collect());
    } else {
        raw = unique_rows(&raw);
    }
    let vel: Vec<i64> = if env.iter().all(|p| p[1] == env[0][1]) {
        // 哪都一样的速度
        vec![round_i64(env[0][1]).clamp(1, 127); raw.len()]
    } else {
        let frac: Vec<f64> = if t_hi > t_lo {
            raw.iter()
                .map(|r| ((r[0] as f64 - t_lo) / (t_hi - t_lo)).clamp(0.0, 1.0))
                .collect()
        } else {
            vec![0.0; raw.len()]
        };
        // （rounds halves to even，和 round 一样）
        env_values(&env, &frac)
            .iter()
            .map(|&v| round_half_even(v).clamp(1.0, 127.0) as i64)
            .collect()
    };
    let notes = raw
        .iter()
        .zip(vel.iter())
        .map(|(r, &v)| [r[0], r[1], r[2], v])
        .collect();
    (notes, tracks)
}

/// [`shape_notes_tracks`] 的默认 128 键便捷包装（原版 keys=128）。
pub fn shape_notes_tracks_default(sh: &Shape, ppq: f64) -> (Vec<Note4>, Option<Vec<i64>>) {
    shape_notes_tracks(sh, ppq, crate::paths::KEYS[0])
}

// ---------------------------------------------------------------- 重叠与通道

/// 通道拆分方式（engine.assign_slots 的 split）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// 只有同一时间同一 key 的音符算冲突。
    Key,
    /// 同一时间响的音符都算冲突，不管 key。
    Time,
}

/// 每个位置到目前（含）为止本组最大的值，换组重新开始；groups 已排好序、同组连在一起
/// （engine.running_max）。
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

/// 自动通道：音符重叠的形状分到不同的 slot，不冲突的复用最小的空 slot；越早的形状 slot 越小
/// （engine.assign_slots）。每个形状同 key 背靠背的音符先并成一段（spam 连成一片 = 一段），
/// 任何与这段长度重叠的都和它其中一条音符重叠；没有长度的音符各自算。
///
/// `apart`: groups of note list numbers that always get different slots (a custom shape's outline
/// and inside, or convert.py's per-source groups).
pub fn assign_slots(note_lists: &[Vec<Note4>], split: Split, apart: &[Vec<usize>]) -> Vec<usize> {
    let n = note_lists.len();
    let key_of = |r: &Note4| if split == Split::Key { r[2] } else { 0 };
    let mut by_pitch: HashMap<i64, Vec<(i64, i64, usize)>> = HashMap::new();
    for (owner, notes) in note_lists.iter().enumerate() {
        for r in notes {
            if r[1] <= r[0] {
                by_pitch
                    .entry(key_of(r))
                    .or_default()
                    .push((r[0], r[1], owner));
            }
        }
        let long: Vec<&Note4> = notes.iter().filter(|r| r[1] > r[0]).collect();
        if long.is_empty() {
            continue;
        }
        let mut order: Vec<usize> = (0..long.len()).collect();
        order.sort_by(|&i, &j| (key_of(long[i]), long[i][0]).cmp(&(key_of(long[j]), long[j][0])));
        let s: Vec<i64> = order.iter().map(|&i| long[i][0]).collect();
        let e: Vec<i64> = order.iter().map(|&i| long[i][1]).collect();
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
                .map(|r| r[0])
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

/// 同一 pitch、同一 slot 里重叠的音符：早的一条在后一条开始的地方被切掉，后一条拉伸到早的
/// 本来会结束的地方（如果更远的话）。同一 tick 开始的音符并成一条：力度最大的赢、留最长的
/// 长度。notes：(start, end, pitch, velocity, slot, owner) 行；返回修好的行，按 slot + key
/// 分组（组按第一次出现的顺序），组内排好序（engine.resolve_overlaps）。
pub fn resolve_overlaps(notes: &[Note6]) -> Vec<Note6> {
    if notes.is_empty() {
        return Vec::new();
    }
    // 键 = slot * 256 + pitch（256 键：slot 之间不会撞）；组号按在输入里第一次出现的顺序编
    let mut group_of_key: HashMap<i64, usize> = HashMap::new();
    let mut group: Vec<usize> = Vec::with_capacity(notes.len());
    for r in notes {
        let key = r[4] * 256 + r[2];
        let next = group_of_key.len();
        group.push(*group_of_key.entry(key).or_insert(next));
    }
    // 同一 start：力度大的排在后面，留的就是它
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&i, &j| {
        group[i]
            .cmp(&group[j])
            .then(notes[i][0].cmp(&notes[j][0]))
            .then(notes[i][3].cmp(&notes[j][3]))
            .then(notes[j][1].cmp(&notes[i][1]))
    });
    let a: Vec<Note6> = order.iter().map(|&i| notes[i]).collect();
    let group: Vec<usize> = order.iter().map(|&i| group[i]).collect();
    let s: Vec<i64> = a.iter().map(|r| r[0]).collect();
    let e: Vec<i64> = a.iter().map(|r| r[1]).collect();
    let groups: Vec<i64> = group.iter().map(|&g| g as i64).collect();
    // 本组里前面所有音符响到哪
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
            end[i] = end[i].max(before); // 拉伸到本来会结束的地方
        }
    }
    for i in 0..a.len().saturating_sub(1) {
        if over[i + 1] {
            end[i] = s[i + 1]; // 前一条在这里被切掉
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
            r[1] = end[i];
            out.push(r);
        }
    }
    out
}

/// 通道模式（engine.CHANNEL_MODES）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 一条通道，音符保持原样（允许重叠）。
    Raw,
    /// 一条通道，修好重叠。
    Single,
    /// 重叠的形状各得一条通道。
    Auto,
}

/// 每个形状的 slot 分配结果：整条形状一个 slot，或每条音符一个 slot。
enum SlotMap {
    One(usize),
    Each(Vec<usize>),
}

/// 一组形状的音符 ->（最终音符, 用掉的 slot 数）（engine.render）。
///
/// note_lists: every shape's shape_notes result. mode: see [`Mode`]. tracks: per shape None, or the
/// track of each of its notes (pasted notes and convert.py's groups, shape_notes_tracks): with
/// "auto" each track of the shape gets channels as if it were a shape of its own. apart: per shape
/// True if its tracks must get different channels (Fill / Spam "Outline").
pub fn render(
    note_lists: &[Vec<Note4>],
    mode: Mode,
    split: Split,
    tracks: Option<&[Option<Vec<i64>>]>,
    apart: Option<&[bool]>,
) -> (Vec<Note6>, usize) {
    let empty: Vec<Option<Vec<i64>>> = Vec::new();
    let tracks = tracks.unwrap_or(&empty);
    let no_apart: Vec<bool> = Vec::new();
    let apart = apart.unwrap_or(&no_apart);
    let (slot_of, count) = if mode == Mode::Auto {
        // 形状、按轨道拆开的粘贴音符；unit_of = 每条音符的 unit
        let mut units: Vec<Vec<Note4>> = Vec::new();
        let mut unit_of: Vec<SlotMap> = Vec::new();
        let mut forced: Vec<Vec<usize>> = Vec::new();
        for (o, lst) in note_lists.iter().enumerate() {
            let tr = tracks.get(o).and_then(|t| t.as_ref());
            match tr {
                Some(tr) if !lst.is_empty() => {
                    // np.unique：ids 升序，which = 每条音符的轨道在 ids 里的下标
                    let mut ids: Vec<i64> = tr.clone();
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
    let mut notes: Vec<Note6> = Vec::new();
    for (o, lst) in note_lists.iter().enumerate() {
        for (i, r) in lst.iter().enumerate() {
            let slot = match &slot_of[o] {
                SlotMap::One(s) => *s,
                SlotMap::Each(v) => v[i],
            };
            let slot = slot as i64;
            notes.push([r[0], r[1], r[2], r[3], slot, o as i64]);
        }
    }
    if mode != Mode::Raw {
        notes = resolve_overlaps(&notes);
    }
    (notes, count)
}

/// slot 号 ->（轨道号, MIDI 通道 0-15），每条轨道一个通道，跳过鼓通道（engine.slot_track_channel）。
pub fn slot_track_channel(slot: usize) -> (usize, u8) {
    (slot, CHANNELS[slot % CHANNELS.len()])
}
