//! Paths ((beat, pitch) point lists) -> notes (a function-by-function port of Python notes/paths.py).
//!
//! A line produces one note per key it passes through: the note starts at the tick where the line reaches that pitch and lasts until the next note starts.
//! With `end_dot` on, the last note starts exactly at the shape's last point instead of ending there.

use crate::{Pt, floor_half};

/// Half a key row, nudged slightly inwards.
pub const EDGE: f64 = 0.5 - 1e-6;

/// Key range of the project: MIDI standard 0-127, or 0-255 (256-key MIDI) (paths.KEYS).
pub const KEYS: [i64; 2] = [128, 256];

/// Highest key a shape can generate (paths.TOP_KEY); the project's range filters out the rest.
pub const TOP_KEY: i64 = KEYS[1] - 1;

/// Rounding the pitch of a (tick, pitch) point: `floor(y + 0.5)`.
pub fn pitch_of(y: f64) -> i64 {
    (y + 0.5).floor() as i64
}

/// Drop points that repeat the previous one.
pub fn dedupe(path: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(path.len());
    for &p in path {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

/// Segment indices where the direction changes (relative to the last direction actually moved in); segment i goes from point i to i+1.
pub fn direction_changes(v: &[f64]) -> Vec<usize> {
    let mut nz: Vec<usize> = Vec::new();
    let mut dn: Vec<i8> = Vec::new();
    for i in 0..v.len().saturating_sub(1) {
        let d = v[i + 1] - v[i];
        let s = if d > 0.0 {
            1
        } else if d < 0.0 {
            -1
        } else {
            0
        };
        if s != 0 {
            nz.push(i);
            dn.push(s);
        }
    }
    let mut out = Vec::new();
    for k in 1..nz.len() {
        if dn[k] != dn[k - 1] {
            out.push(nz[k]);
        }
    }
    out
}

/// Concatenate the indices of each lo..hi span (`rev`: that span runs backwards hi..lo). Corresponds to paths.spans.
pub fn spans(lo: &[usize], hi: &[usize], rev: Option<&[bool]>) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..lo.len() {
        let n = hi[i] - lo[i] + 1;
        out.reserve(n);
        if rev.map(|r| r[i]).unwrap_or(false) {
            for k in 0..n {
                out.push(hi[i] - k);
            }
        } else {
            for k in 0..n {
                out.push(lo[i] + k);
            }
        }
    }
    out
}

/// Stretch the pitches of pts[i..=j] so the first/last pitch row is fully covered instead of only half (paths._remap).
fn remap(pts: &mut [Pt], i: usize, j: usize, move_start: bool, move_end: bool, end_dot: bool) {
    let ys = pts[i][1];
    let ye = pts[j][1];
    let ps = pitch_of(ys);
    let pe = pitch_of(ye);
    if ps == pe {
        return;
    }
    let d = if pe > ps { 1.0 } else { -1.0 };
    let ns = if move_start { ps as f64 - d * EDGE } else { ys };
    let ne = if move_end {
        if end_dot {
            pe as f64 - d * EDGE
        } else {
            pe as f64 + d * EDGE
        }
    } else {
        ye
    };
    let k = (ne - ns) / (ye - ys);
    for p in &mut pts[i..=j] {
        p[1] = ns + (p[1] - ys) * k;
    }
}

/// Stretch only the start and end of the path so every pitch it passes gets an equal share of time (paths.stretch_ends).
pub fn stretch_ends(path: &[Pt], end_dot: bool) -> Vec<Pt> {
    let mut pts = path.to_vec();
    let n = pts.len();
    if n < 2 {
        return pts;
    }
    let ys: Vec<f64> = pts.iter().map(|p| p[1]).collect();
    let mut turns = vec![0usize];
    turns.extend(direction_changes(&ys));
    turns.push(n - 1);
    if turns.len() == 2 {
        remap(&mut pts, 0, n - 1, true, true, end_dot);
    } else {
        remap(&mut pts, 0, turns[1], true, false, false);
        remap(
            &mut pts,
            turns[turns.len() - 2],
            n - 1,
            false,
            true,
            end_dot,
        );
    }
    pts
}

/// Of duplicate notes with the same tick and key keep the longest (in order of first appearance); drop notes ending before 0 (paths.keep_longest).
pub fn keep_longest(raw: &[[i64; 3]]) -> Vec<[i64; 3]> {
    let mut rows: Vec<([i64; 3], usize)> = raw
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, r)| r[1] >= 0)
        .map(|(i, r)| (r, i))
        .collect();
    if rows.len() < 2 {
        return rows.into_iter().map(|(r, _)| r).collect();
    }
    rows.sort_by_key(|(r, _)| (r[0], r[2]));
    let mut groups: Vec<([i64; 3], usize)> = Vec::new();
    for (r, orig) in rows {
        if let Some(last) = groups.last_mut()
            && last.0[0] == r[0]
            && last.0[2] == r[2]
        {
            if r[1] > last.0[1] {
                last.0[1] = r[1];
            }
            continue;
        }
        groups.push((r, orig));
    }
    groups.sort_by_key(|(_, orig)| *orig);
    groups.into_iter().map(|(r, _)| r).collect()
}

/// Restart a closed loop (first point = last point) from its leftmost point so every piece runs left to right (paths.loop_from_left).
pub fn loop_from_left(path: &[Pt]) -> Vec<Pt> {
    let path = &path[..path.len() - 1];
    let mut i = 0usize;
    for (j, p) in path.iter().enumerate() {
        if (p[0], p[1]) < (path[i][0], path[i][1]) {
            i = j;
        }
    }
    let mut out = path[i..].to_vec();
    out.extend_from_slice(&path[..=i]);
    out
}

/// Whether the path moves forward in time (false when the last non-zero displacement is backwards; paths.ends_forward).
pub fn ends_forward(path: &[Pt]) -> bool {
    let mut last: Option<f64> = None;
    for w in path.windows(2) {
        let dt = w[1][0] - w[0][0];
        if dt.abs() > 1e-12 {
            last = Some(dt);
        }
    }
    last.is_none_or(|dt| dt > 0.0)
}

/// The path (ticks) of a line / curve / arc -> (start, end, pitch) (paths.path_notes).
pub fn path_notes(path: &[Pt], end_dot: bool) -> Vec<[i64; 3]> {
    if path.len() > 3 && path[0] == path[path.len() - 1] {
        let p = loop_from_left(path);
        return keep_longest(&line_notes(&p, false));
    }
    let end_dot = end_dot && ends_forward(path);
    let p = stretch_ends(path, end_dot);
    keep_longest(&line_notes(&p, end_dot))
}

/// (tick, pitch) point list -> notes, without merging duplicates yet (paths.line_notes).
pub fn line_notes(pts: &[Pt], tail: bool) -> Vec<[i64; 3]> {
    let n = pts.len();
    let xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
    let cuts = direction_changes(&xs);
    let mut lo: Vec<usize> = vec![0];
    lo.extend(&cuts);
    let mut hi: Vec<usize> = cuts.clone();
    hi.push(n - 1);
    let rev: Vec<bool> = (0..lo.len())
        .map(|k| pts[hi[k]][0] < pts[lo[k]][0])
        .collect();
    let q_idx = spans(&lo, &hi, Some(&rev));
    let q: Vec<Pt> = q_idx.iter().map(|&i| pts[i]).collect();

    let size: Vec<usize> = (0..lo.len()).map(|k| hi[k] - lo[k] + 1).collect();
    let mut piece_end = Vec::with_capacity(size.len());
    let mut acc = 0usize;
    for &s in &size {
        acc += s;
        piece_end.push(acc - 1);
    }

    let mut same = vec![true; q.len().saturating_sub(1)];
    for &pe in &piece_end[..piece_end.len() - 1] {
        same[pe] = false;
    }
    let up: Vec<bool> = (0..q.len() - 1)
        .map(|i| (q[i + 1][0] - q[i][0]).abs() < 1e-9)
        .collect();
    let mut split: Vec<usize> = Vec::new();
    for i in 1..up.len() {
        if same[i] && same[i - 1] && up[i] != up[i - 1] {
            split.push(i);
        }
    }

    let mut first: Vec<usize> = (0..size.len())
        .map(|k| piece_end[k] + 1 - size[k])
        .collect();
    first.extend(&split);
    first.sort_unstable();
    let mut last: Vec<usize> = split;
    last.extend(&piece_end);
    last.sort_unstable();

    let tails: Vec<bool> = if tail {
        let mut pe_sorted = piece_end.clone();
        pe_sorted.sort_unstable();
        last.iter()
            .map(|&l| pe_sorted.binary_search(&l).is_ok() && q[l] == pts[n - 1])
            .collect()
    } else {
        vec![false; first.len()]
    };

    let size2: Vec<usize> = (0..first.len()).map(|k| last[k] - first[k] + 1).collect();
    let r_idx = spans(&first, &last, None);
    let r: Vec<Pt> = r_idx.iter().map(|&i| q[i]).collect();
    let mut starts2 = Vec::with_capacity(size2.len());
    let mut acc = 0usize;
    for &s in &size2 {
        starts2.push(acc);
        acc += s;
    }
    parts_notes(&r, &starts2, &tails, false).0
}

/// Left-to-right pieces -> one note per pitch crossed (paths.parts_notes). Returns (notes, note count per piece).
pub fn parts_notes(
    r: &[Pt],
    first: &[usize],
    tails: &[bool],
    counts: bool,
) -> (Vec<[i64; 3]>, Vec<usize>) {
    let k_parts = first.len();
    let mut last: Vec<usize> = first[1..].iter().map(|&f| f - 1).collect();
    last.push(r.len() - 1);
    let mut seg = vec![true; r.len().saturating_sub(1)];
    for &l in &last[..last.len() - 1] {
        seg[l] = false;
    }
    let p: Vec<i64> = r.iter().map(|q| pitch_of(q[1])).collect();

    let mut jj: Vec<usize> = Vec::new();
    let mut qq: Vec<i64> = Vec::new();
    for (i, &on) in seg.iter().enumerate() {
        if !on || p[i] == p[i + 1] {
            continue;
        }
        let (pa, pb) = (p[i], p[i + 1]);
        let step = if pb > pa { 1 } else { -1 };
        let c = (pb - pa).unsigned_abs() as usize;
        for k in 0..c {
            jj.push(i);
            qq.push(pa + step * (k as i64 + 1));
        }
    }

    let mut tt = Vec::with_capacity(qq.len());
    for (idx, &seg_i) in jj.iter().enumerate() {
        let step = if p[seg_i + 1] > p[seg_i] { 1.0 } else { -1.0 };
        let yy = qq[idx] as f64 - 0.5 * step;
        let (ta, ya) = (r[seg_i][0], r[seg_i][1]);
        let (tb, yb) = (r[seg_i + 1][0], r[seg_i + 1][1]);
        tt.push(ta + (tb - ta) * (yy - ya) / (yb - ya));
    }

    let mut per = vec![1usize; k_parts];
    for &seg_i in &jj {
        let pid = first.partition_point(|&f| f <= seg_i) - 1;
        per[pid] += 1;
    }
    let m: usize = per.iter().sum();
    let mut off = Vec::with_capacity(k_parts);
    let mut acc = 0usize;
    for &p_ in &per {
        off.push(acc);
        acc += p_;
    }

    let mut et = vec![0.0f64; m];
    let mut ep = vec![0i64; m];
    for k in 0..k_parts {
        et[off[k]] = r[first[k]][0];
        ep[off[k]] = p[first[k]];
    }
    for (c, (&seg_i, &t)) in jj.iter().zip(tt.iter()).enumerate() {
        let pid = first.partition_point(|&f| f <= seg_i) - 1;
        let pos = c + pid + 1;
        et[pos] = t;
        ep[pos] = qq[c];
    }

    let starts_i: Vec<i64> = et.iter().map(|&x| floor_half(x)).collect();
    let end_tick: Vec<i64> = last.iter().map(|&l| floor_half(r[l][0])).collect();
    let mut part = Vec::with_capacity(m);
    for (k, &n) in per.iter().enumerate() {
        for _ in 0..n {
            part.push(k);
        }
    }
    let e_last: Vec<usize> = (0..k_parts).map(|k| off[k] + per[k] - 1).collect();

    let mut later = vec![false; m];
    for i in 0..m.saturating_sub(1) {
        if part[i + 1] == part[i] && starts_i[i + 1] > starts_i[i] {
            later[i] = true;
        }
    }
    let mut nxt_at = vec![m; m];
    let mut next = m;
    for i in (0..m).rev() {
        if later[i] {
            next = i;
        }
        nxt_at[i] = next;
    }
    let mut ends = vec![0i64; m];
    for i in 0..m {
        let inside = nxt_at[i] < e_last[part[i]];
        let nxt = if inside {
            starts_i[(nxt_at[i] + 1).min(m - 1)]
        } else {
            end_tick[part[i]]
        };
        ends[i] = nxt.max(starts_i[i] + 1);
    }

    for k in 0..k_parts {
        if tails.get(k).copied().unwrap_or(false) && per[k] >= 2 {
            let t = e_last[k];
            if starts_i[t] == end_tick[k] && t > 0 {
                ends[t] = starts_i[t] + (ends[t - 1] - starts_i[t - 1]).max(1);
            }
        }
    }

    let notes: Vec<[i64; 3]> = (0..m).map(|i| [starts_i[i], ends[i], ep[i]]).collect();
    let _ = counts;
    (notes, per)
}

/// Polyline "start at each point": every segment behaves like its own line (paths.dot_segment_notes).
pub fn dot_segment_notes(path: &[Pt]) -> Vec<[i64; 3]> {
    let n = path.len() - 1;
    let mut pts: Vec<Pt> = Vec::with_capacity(2 * n);
    for i in 0..n {
        let (mut a, mut b) = (path[i], path[i + 1]);
        if a[0] > b[0] || (a[0] == b[0] && a[1] > b[1]) {
            std::mem::swap(&mut a, &mut b);
        }
        let ps = pitch_of(a[1]);
        let pe = pitch_of(b[1]);
        let d = if pe > ps { 1.0 } else { -1.0 };
        let ns = ps as f64 - d * EDGE;
        let ne = pe as f64 - d * EDGE;
        let moved = ps != pe;
        let (y0, y1) = if moved {
            let k = (ne - ns) / (b[1] - a[1]);
            (ns, ns + (b[1] - a[1]) * k)
        } else {
            (a[1], b[1])
        };
        pts.push([a[0], y0]);
        pts.push([b[0], y1]);
    }
    let first: Vec<usize> = (0..n).map(|i| i * 2).collect();
    let tails = vec![true; n];
    let (raw, per) = parts_notes(&pts, &first, &tails, true);

    let row = unique_ranks3(&raw);
    let mut cum = Vec::with_capacity(per.len());
    let mut acc = 0usize;
    for &p_ in &per {
        acc += p_;
        cum.push(acc - 1);
    }
    let tail_rows: Vec<usize> = (0..per.len())
        .filter(|&k| per[k] >= 2)
        .map(|k| row[cum[k]])
        .collect();
    let in_tails: Vec<bool> = row.iter().map(|r| tail_rows.contains(r)).collect();
    let at = unique_ranks2(&raw.iter().map(|r| (r[0], r[2])).collect::<Vec<_>>());
    let non_tail_rows: Vec<usize> = (0..raw.len())
        .filter(|&i| !in_tails[i])
        .map(|i| at[i])
        .collect();
    (0..raw.len())
        .filter(|&i| !(in_tails[i] && non_tail_rows.contains(&at[i])))
        .map(|i| raw[i])
        .collect()
}

/// Rank of each row after sorting and deduplication (np.unique(axis=0, return_inverse)).
pub fn unique_ranks3(rows: &[[i64; 3]]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| rows[i]);
    let mut ranks = vec![0usize; rows.len()];
    let mut rank = 0usize;
    for (k, &i) in order.iter().enumerate() {
        if k > 0 && rows[i] != rows[order[k - 1]] {
            rank += 1;
        }
        ranks[i] = rank;
    }
    ranks
}

/// Rank of (start, key) pairs.
pub fn unique_ranks2(rows: &[(i64, i64)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| rows[i]);
    let mut ranks = vec![0usize; rows.len()];
    let mut rank = 0usize;
    for (k, &i) in order.iter().enumerate() {
        if k > 0 && rows[i] != rows[order[k - 1]] {
            rank += 1;
        }
        ranks[i] = rank;
    }
    ranks
}
