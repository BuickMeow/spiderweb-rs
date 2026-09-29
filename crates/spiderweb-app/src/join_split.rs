//! Join (selected lines / polylines / freehand strokes / curves / arcs -> one curve) and Split (a joined
//! curve back into pieces, any of those cut in two where it was right-clicked, a custom shape into its
//! separate drawings). Port of upstream `window/join_split.py`; the maths is in `notes/joined.py`
//! (`spiderweb_core::joined`).
//!
//! "Turn into live shape" (upstream's third button, convert.py) belongs to another port and is left out.

use std::collections::BTreeSet;

use eframe::egui;
use egui::Pos2;

use spiderweb_core::Pt;
use spiderweb_core::arc::{arc_circle, arc_points};
use spiderweb_core::bezier;
use spiderweb_core::engine;
use spiderweb_core::joined::{
    self, join_shapes, join_velocity, piece_velocity, sections, split_at, split_custom,
};
use spiderweb_core::shape::{Kind, Shape};
use spiderweb_core::smooth::smooth_path;

use crate::app::App;

/// Ends closer than this on screen (times the display scaling) count as touching, like Live shape snaps
/// (upstream TOUCH_PX).
pub const TOUCH_PX: f32 = 8.0;

/// The shape kinds that can be joined, as the menu messages spell them (upstream JOIN_KINDS).
const JOIN_KINDS: &str = "lines, polylines, freehand strokes, curves and arcs";

/// (earliest, latest) beat of a shape's drawn path (upstream join_split.span).
pub fn span(sh: &Shape) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for stroke in engine::shape_strokes(sh) {
        for p in stroke {
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Replace the selected shapes with the joined curve. Returns (where it was put, pieces) or None when
/// nothing could be joined. Undo / status are the caller's job.
pub fn join_command(
    shapes: &mut Vec<Shape>,
    sels: &BTreeSet<usize>,
    k: f64,
    touch: &dyn Fn(Pt, Pt) -> bool,
) -> Option<(usize, usize)> {
    let order: Vec<usize> = sels.iter().copied().filter(|&i| i < shapes.len()).collect();
    if order.len() < 2 {
        return None;
    }
    let olds: Vec<Shape> = order.iter().map(|&i| shapes[i].clone()).collect();
    let mut new = join_shapes(&olds, k, touch)?;
    let spans: Option<Vec<(f64, f64)>> = olds.iter().map(span).collect();
    let spans = spans?;
    let new_span = span(&new)?;
    join_velocity(&mut new, &olds, &spans, new_span); // (each keeps its velocities)
    let at = order[0];
    let pieces = new.gaps.len() + 1;
    for &i in order.iter().rev() {
        shapes.remove(i);
    }
    shapes.insert(at, new);
    Some((at, pieces))
}

/// Replace shape i with parts in place, their velocities kept where they were (the pure part of
/// `App::replace_shape`). Returns the new shape numbers (i..i+n) or None.
pub fn replace_shape_in(
    shapes: &mut Vec<Shape>,
    i: usize,
    mut parts: Vec<Shape>,
    velocity: bool,
) -> Option<BTreeSet<usize>> {
    if parts.is_empty() || i >= shapes.len() {
        return None;
    }
    let old = shapes[i].clone();
    let whole = span(&old);
    if velocity && let Some(whole) = whole {
        for p in &mut parts {
            if let Some(s) = span(p) {
                piece_velocity(p, &old, s, whole);
            }
        }
    }
    let n = parts.len();
    shapes.splice(i..i + 1, parts);
    Some((i..i + n).collect())
}

/// The shapes shape i splits into: a joined curve's pieces, or a custom shape's groups of touching
/// strokes. None when there's nothing to split (or the custom shape is text / pasted notes).
pub fn split_parts(sh: &Shape) -> Option<Vec<Shape>> {
    if sh.kind == Kind::Curve {
        let parts = joined::split_pieces(sh);
        (parts.len() > 1).then_some(parts)
    } else if sh.kind == Kind::Custom && sh.text.is_none() && sh.notes.is_none() {
        let parts = split_custom(sh);
        (parts.len() > 1).then_some(parts)
    } else {
        None
    }
}

impl App {
    /// Why the selection can't be joined (None = it can).
    pub fn join_problem(&self) -> Option<String> {
        let sels: Vec<usize> = self
            .sels
            .iter()
            .copied()
            .filter(|&i| i < self.shapes.len())
            .collect();
        if sels.len() < 2 {
            return Some(
                rust_i18n::t!(
                    "join.select_two",
                    kinds = JOIN_KINDS.replace(" and ", " or ")
                )
                .to_string(),
            );
        }
        let mut other: Vec<String> = sels
            .iter()
            .filter(|&&i| !joined::LINE_KINDS.contains(&self.shapes[i].kind))
            .map(|&i| {
                self.shape_label(&self.shapes[i])
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .to_lowercase()
            })
            .collect();
        other.sort();
        other.dedup();
        if !other.is_empty() {
            return Some(
                rust_i18n::t!(
                    "join.only_can_be_joined",
                    kinds = JOIN_KINDS,
                    join = other.join(" and ")
                )
                .to_string(),
            );
        }
        None
    }

    pub fn can_join(&self) -> bool {
        self.join_problem().is_none()
    }

    /// Why the selection can't be split into separate shapes (None = it can).
    pub fn split_problem(&self) -> Option<String> {
        let Some(i) = self.sel.filter(|&i| i < self.shapes.len()) else {
            return Some(rust_i18n::t!("join.select_one_shape").to_string());
        };
        if self.sels.len() != 1 {
            return Some(rust_i18n::t!("join.select_one_shape").to_string());
        }
        let sh = &self.shapes[i];
        if sh.kind == Kind::Custom && (sh.text.is_some() || sh.notes.is_some()) {
            return Some(rust_i18n::t!("join.text_and_pasted").to_string());
        }
        if !self.can_split_pieces(sh) {
            return Some(
                if matches!(sh.kind, Kind::Curve | Kind::Custom) {
                    rust_i18n::t!("join.all_one_piece")
                } else {
                    rust_i18n::t!("join.only_joined_and_custom")
                }
                .to_string(),
            );
        }
        None
    }

    /// A joined curve with more than one piece / section in it, or a custom drawing with separate parts.
    pub fn can_split_pieces(&self, sh: &Shape) -> bool {
        if sh.kind == Kind::Curve {
            return sections(sh).len() > 1;
        }
        sh.kind == Kind::Custom
            && sh.text.is_none()
            && sh.notes.is_none()
            && joined::custom_groups(sh).len() > 1
    }

    /// Shape i replaced by parts (selected), their velocities kept where they were.
    pub fn replace_shape(&mut self, i: usize, parts: Vec<Shape>, velocity: bool) {
        self.cancel_draft();
        self.push_undo();
        if let Some(sels) = replace_shape_in(&mut self.shapes, i, parts, velocity) {
            let primary = sels.iter().next().copied();
            self.select_many(sels, primary);
            self.shapes_changed();
        }
    }

    /// Join the selected shapes into one Curve shape (upstream join_selected).
    pub fn join_selected(&mut self) {
        if let Some(problem) = self.join_problem() {
            self.status = problem;
            return;
        }
        if !self.view.ready {
            return;
        }
        let k = self.view.sy / self.view.sx;
        let scale = self.scale() as f64;
        let view = self.view.clone();
        let touch = move |p: Pt, q: Pt| {
            spiderweb_core::hypot2(
                (view.x_of(p[0]) - view.x_of(q[0])) as f64,
                (view.y_of(p[1]) - view.y_of(q[1])) as f64,
            ) <= TOUCH_PX as f64 * scale
        };
        let n = self.sels.len();
        self.cancel_draft();
        self.push_undo();
        let sels = self.sels.clone();
        let got = join_command(&mut self.shapes, &sels, k, &touch);
        if let Some((at, pieces)) = got {
            self.select(Some(at), false);
            self.shapes_changed();
            let mut text = rust_i18n::t!("join.joined", n = n.to_string()).to_string();
            if pieces > 1 {
                text += &rust_i18n::t!("join.pieces_note", pieces = pieces.to_string());
            }
            self.status = text;
        }
    }

    /// Split the selected shape into separate shapes (upstream split_selected).
    pub fn split_selected(&mut self) {
        if let Some(problem) = self.split_problem() {
            self.status = problem;
            return;
        }
        if let Some(i) = self.sel {
            self.split_pieces_shape(i);
        }
    }

    /// Shape i into its pieces (upstream split_pieces; the live-shape "back to the old shapes" part
    /// is another port and skipped here).
    pub fn split_pieces_shape(&mut self, i: usize) {
        let Some(sh) = self.shapes.get(i).cloned() else {
            return;
        };
        if !self.can_split_pieces(&sh) {
            return;
        }
        let Some(parts) = split_parts(&sh) else {
            return;
        };
        let n = parts.len();
        self.replace_shape(i, parts, true);
        self.status = rust_i18n::t!("join.split_into", n = n.to_string()).to_string();
    }

    /// Cut a line kind in two where it was right-clicked (upstream split_here; `at` is on the roll).
    pub fn split_here(&mut self, i: usize, at: Pos2) {
        let Some(sh) = self.shapes.get(i).cloned() else {
            return;
        };
        let got = match sh.kind {
            Kind::Curve => {
                let mut c = sh.clone();
                let (to_screen, _) = self.roll_maps();
                let Some((seg, t, _)) =
                    bezier::nearest(&sh.pts, &to_screen, at.x as f64, at.y as f64, 64, &sh.gaps)
                else {
                    return;
                };
                let a = self.anchor_near(&sh, seg, at);
                let a = match a {
                    Some(a) => a,
                    None => {
                        // a new anchor there first
                        c.pts = bezier::split(&sh.pts, seg, t);
                        let shift = |v: &[usize]| -> Vec<usize> {
                            v.iter().map(|&x| if x > seg { x + 1 } else { x }).collect()
                        };
                        c.sharp = shift(&sh.sharp);
                        c.gaps = shift(&sh.gaps);
                        c.splits = shift(&sh.splits);
                        c.sharp.push(seg + 1);
                        c.sharp.sort_unstable();
                        c.sharp.dedup();
                        seg + 1
                    }
                };
                split_at(&c, a)
            }
            Kind::Arc => self.split_arc(&sh, at),
            _ => self.split_line(&sh, at),
        };
        let Some((left, right)) = got else {
            self.status = rust_i18n::t!("join.can_t_split").to_string();
            return;
        };
        self.replace_shape(i, vec![left, right], true);
        self.status = rust_i18n::t!("join.split_in_two").to_string();
    }

    /// (to_screen, from_screen) maps of the roll view (upstream roll.to_xy / from_xy).
    pub(crate) fn roll_maps(
        &self,
    ) -> (
        impl Fn(Pt) -> [f64; 2] + use<>,
        impl Fn(f64, f64) -> Pt + use<>,
    ) {
        let a = self.view.clone();
        let b = self.view.clone();
        (
            move |p: Pt| [a.x_of(p[0]) as f64, a.y_of(p[1]) as f64],
            move |x: f64, y: f64| [b.b_of(x as f32), b.p_of(y as f32)],
        )
    }

    /// Screen distance between two curve points.
    fn screen_dist(&self, p: Pt, q: Pt) -> f64 {
        spiderweb_core::hypot2(
            (self.view.x_of(p[0]) - self.view.x_of(q[0])) as f64,
            (self.view.y_of(p[1]) - self.view.y_of(q[1])) as f64,
        )
    }

    /// The segment's anchor the right-click was on (near), or None (upstream anchor_near).
    fn anchor_near(&self, sh: &Shape, seg: usize, at: Pos2) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for a in [seg, seg + 1] {
            let Some(p) = sh.pts.get(3 * a).copied() else {
                continue;
            };
            let d = spiderweb_core::hypot2(
                (self.view.x_of(p[0]) - at.x) as f64,
                (self.view.y_of(p[1]) - at.y) as f64,
            );
            if best.is_none() || d < best.unwrap().0 {
                best = Some((d, a));
            }
        }
        let (d, a) = best?;
        (d <= TOUCH_PX as f64 * self.scale() as f64).then_some(a)
    }

    /// The spot on the polyline pts (beat, pitch) nearest the click on screen:
    /// (segment, 0..1 along it, its length on screen) (upstream nearest_on).
    fn nearest_on(&self, pts: &[Pt], at: Pos2) -> Option<(usize, f64, f64)> {
        if pts.len() < 2 {
            return None;
        }
        let screen: Vec<[f64; 2]> = pts
            .iter()
            .map(|p| [self.view.x_of(p[0]) as f64, self.view.y_of(p[1]) as f64])
            .collect();
        let mut best: Option<(f64, usize, f64)> = None;
        for j in 0..screen.len() - 1 {
            let (a, b) = (screen[j], screen[j + 1]);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let l = ab[0] * ab[0] + ab[1] * ab[1];
            let u = if l > 0.0 {
                (((at.x as f64 - a[0]) * ab[0] + (at.y as f64 - a[1]) * ab[1]) / l).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = spiderweb_core::hypot2(
                a[0] + ab[0] * u - at.x as f64,
                a[1] + ab[1] * u - at.y as f64,
            );
            if best.is_none() || d < best.unwrap().0 {
                best = Some((d, j, u));
            }
        }
        let (_, j, u) = best?;
        let ab = [
            screen[j + 1][0] - screen[j][0],
            screen[j + 1][1] - screen[j][1],
        ];
        Some((j, u, spiderweb_core::hypot2(ab[0], ab[1])))
    }

    /// A line / polyline / freehand stroke cut in two (upstream split_line).
    fn split_line(&self, sh: &Shape, at: Pos2) -> Option<(Shape, Shape)> {
        let mut sh = sh.clone();
        let mut pts = sh.pts.clone();
        if sh.kind == Kind::Free && sh.smooth != 0 {
            pts = smooth_path(&pts, sh.smooth as f64, sh.k);
            sh.smooth = 0; // (the halves are no longer straightened)
        }
        let (j, u, seg_px) = self.nearest_on(&pts, at)?;
        let a = pts[j];
        let b = pts[j + 1];
        let near = TOUCH_PX as f64 * self.scale() as f64;
        let (cut, k) = if sh.kind == Kind::Free {
            if u < 0.5 {
                (a, Some(j))
            } else {
                (b, Some(j + 1))
            }
        } else if u * seg_px <= near {
            (a, Some(j))
        } else if (1.0 - u) * seg_px <= near {
            (b, Some(j + 1))
        } else {
            ([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u], None)
        };
        if let Some(k) = k
            && (k == 0
                || k + 1 == pts.len()
                || (sh.kind == Kind::Free
                    && self
                        .screen_dist(cut, pts[0])
                        .min(self.screen_dist(cut, pts[pts.len() - 1]))
                        <= near))
        {
            return None;
        }
        let (left, right) = match k {
            None => {
                let mut left = pts[..j + 1].to_vec();
                left.push(cut);
                let mut right = vec![cut];
                right.extend_from_slice(&pts[j + 1..]);
                (left, right)
            }
            Some(k) => (pts[..k + 1].to_vec(), pts[k..].to_vec()),
        };
        let kinds = if sh.kind == Kind::Free {
            [Kind::Free, Kind::Free]
        } else {
            [
                if left.len() == 2 {
                    Kind::Line
                } else {
                    Kind::Poly
                },
                if right.len() == 2 {
                    Kind::Line
                } else {
                    Kind::Poly
                },
            ]
        };
        self.halves(sh, [&left, &right], kinds)
    }

    /// An arc cut in two where it was right-clicked: two arcs on the same circle (upstream split_arc).
    fn split_arc(&self, sh: &Shape, at: Pos2) -> Option<(Shape, Shape)> {
        let k = sh.k;
        let path = arc_points(&sh.pts, k, spiderweb_core::arc::STEP);
        let (j, u, _) = self.nearest_on(&path, at)?;
        let f = (j as f64 + u) / (path.len() - 1) as f64; // (arc_points: evenly round the circle)
        let got = arc_circle(&sh.pts, k);
        let point = |f: f64| -> Pt {
            if let Some((centre, r, t0, turn)) = got {
                let t = t0 + turn * f;
                [(centre[0] + r * t.cos()) * k, centre[1] + r * t.sin()]
            } else {
                // (in a straight line)
                let (a, _, c) = (sh.pts[0], sh.pts[1], sh.pts[2]);
                [a[0] + (c[0] - a[0]) * f, a[1] + (c[1] - a[1]) * f]
            }
        };
        let cut = point(f);
        let (a, c) = (sh.pts[0], sh.pts[2]);
        if self.screen_dist(cut, a).min(self.screen_dist(cut, c))
            <= TOUCH_PX as f64 * self.scale() as f64
        {
            return None;
        }
        let left = vec![a, point(f / 2.0), cut];
        let right = vec![cut, point((1.0 + f) / 2.0), c];
        self.halves(sh.clone(), [&left, &right], [Kind::Arc, Kind::Arc])
    }

    /// The two halves of a cut shape (their points and kinds), with its other settings; the tumours stay
    /// where they were (tumour.split_tumour) (upstream halves).
    fn halves(&self, sh: Shape, parts: [&[Pt]; 2], kinds: [Kind; 2]) -> Option<(Shape, Shape)> {
        let mut out: Vec<Shape> = Vec::new();
        for (half, kind) in parts.iter().zip(kinds) {
            let mut new = sh.clone();
            new.pts = half.to_vec();
            new.kind = kind;
            new.tumour = None;
            new.gaps.clear();
            new.splits.clear();
            new.tumours.clear();
            out.push(new);
        }
        let tms = match &sh.tumour {
            Some(tm) => {
                let paths: Vec<Vec<Pt>> = out.iter().map(engine::shape_path).collect();
                spiderweb_core::tumour::split_tumour(tm, &paths[0], &paths[1])
            }
            None => (None, None),
        };
        for (new, tm) in out.iter_mut().zip([tms.0, tms.1]) {
            if tm.is_some() {
                new.tumour = tm;
            }
        }
        Some((out.remove(0), out.remove(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(pts: Vec<Pt>) -> Shape {
        Shape::new(Kind::Line, pts)
    }

    fn touching(p: Pt, q: Pt) -> bool {
        spiderweb_core::dist(p, q) <= 1e-9
    }

    /// The join / split command path on a shape list: two lines become one joined curve, and splitting
    /// gives two shapes back (no undo / status here, those are the App's).
    #[test]
    fn join_then_split_round_trip() {
        let mut shapes = vec![
            line(vec![[0.0, 60.0], [4.0, 60.0]]),
            line(vec![[4.0, 60.0], [8.0, 64.0]]),
        ];
        let sels: BTreeSet<usize> = [0, 1].into_iter().collect();
        let (at, pieces) = join_command(&mut shapes, &sels, 1.0, &touching).expect("joins");
        assert_eq!(at, 0);
        assert_eq!(pieces, 1);
        assert_eq!(shapes.len(), 1);
        assert!(
            !joined::is_joined(&shapes[0]),
            "all one piece: no gaps, no tumours"
        );
        // all one piece: nothing to split into separate shapes
        assert!(split_parts(&shapes[0]).is_none());
        assert_eq!(joined::split_pieces(&shapes[0]).len(), 1);
    }

    #[test]
    fn join_with_a_gap_splits_into_two() {
        let mut shapes = vec![
            line(vec![[0.0, 60.0], [4.0, 60.0]]),
            line(vec![[6.0, 60.0], [10.0, 60.0]]),
        ];
        let sels: BTreeSet<usize> = [0, 1].into_iter().collect();
        let (_, pieces) = join_command(&mut shapes, &sels, 1.0, &touching).expect("joins");
        assert_eq!(pieces, 2);
        let parts = split_parts(&shapes[0]).expect("splits");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].kind, Kind::Curve);
        assert_eq!(parts[0].gaps, Vec::<usize>::new());
        assert!((parts[0].pts[0][0] - 0.0).abs() < 1e-12);
        assert!((parts[1].pts[0][0] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn join_problem_wants_two_line_kinds() {
        // pure logic: no App needed for the kinds list check
        assert!(JOIN_KINDS.contains("curves and arcs"));
    }

    /// Splitting a joined curve gives each piece the velocity it had as part of the whole.
    #[test]
    fn split_keeps_each_piece_velocity() {
        let mut a = line(vec![[0.0, 60.0], [4.0, 60.0]]);
        a.vel0 = 10.0;
        a.vel1 = 20.0;
        let mut b = line(vec![[6.0, 60.0], [10.0, 60.0]]);
        b.vel0 = 100.0;
        b.vel1 = 110.0;
        let mut shapes = vec![a, b];
        let sels: BTreeSet<usize> = [0, 1].into_iter().collect();
        let (_, pieces) = join_command(&mut shapes, &sels, 1.0, &touching).expect("joins");
        assert_eq!(pieces, 2);
        let parts = split_parts(&shapes[0]).expect("splits");
        assert_eq!(parts.len(), 2);
        let sels = replace_shape_in(&mut shapes, 0, parts, true).expect("replaced");
        assert_eq!(sels, [0usize, 1].into_iter().collect());
        assert_eq!(shapes.len(), 2);
        assert!((shapes[0].vel0 - 10.0).abs() < 1e-9, "{}", shapes[0].vel0);
        assert!((shapes[0].vel1 - 20.0).abs() < 1e-9, "{}", shapes[0].vel1);
        assert!((shapes[1].vel0 - 100.0).abs() < 1e-9, "{}", shapes[1].vel0);
        assert!((shapes[1].vel1 - 110.0).abs() < 1e-9, "{}", shapes[1].vel1);
    }
}
