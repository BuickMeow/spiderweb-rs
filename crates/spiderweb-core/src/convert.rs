//! convert: "Turn into live shape" (Python notes/convert.py).
//!
//! Lines, polylines, freehand strokes, curves and arcs (and custom shapes) become one custom shape,
//! each as strokes of its own kind (a curve stays a curve, an arc an arc), so it can be filled.
//!
//! The notes stay the same: every stroke remembers which shape it came from ([`Stroke::src`]), and
//! each shape's strokes make their outline notes on their own ([`crate::custom::outline_groups`]), so
//! with Multi channel the old shapes still get channels of their own. Tumours become plain points (the
//! bumps as drawn), "Last note: starts on it" is dropped.
//!
//! The old shapes are kept in the new one ([`Shape::from`]), so Split into separate shapes can give
//! them back as long as the drawing wasn't changed (moving the whole shape is fine; resizing, turning,
//! flipping or editing strokes isn't).
//!
//! [`crate::joined::join_velocity`] maps the old shapes' velocity envelopes onto the new shape's
//! time span, so every part keeps its velocities.

use std::collections::{BTreeMap, BTreeSet};

use crate::Pt;
use crate::bezier::anchor_count;
use crate::custom::{CustomDefaults, add_stroke, custom_strokes, new_live_shape, stroke_bp};
use crate::engine::shape_strokes;
use crate::joined::join_velocity;
use crate::shape::{Kind, Shape, ShapeFrom, Stroke};

/// Kinds that can be turned into a live shape (convert.CAN_TURN).
pub const CAN_TURN: [Kind; 6] = [
    Kind::Line,
    Kind::Poly,
    Kind::Free,
    Kind::Curve,
    Kind::Arc,
    Kind::Custom,
];

/// Kinds a shape's tumours belong to (tumour.LINE_KINDS; not funnels/custom shapes).
const LINE_KINDS: [Kind; 5] = [Kind::Line, Kind::Poly, Kind::Free, Kind::Curve, Kind::Arc];

/// Whether the shape has tumours that are on (`convert.has_tumours`).
///
/// A joined curve can carry one setting per joined shape (`sh["tumours"]`); any of them counts.
pub fn has_tumours(sh: &Shape) -> bool {
    sh.tumour.as_ref().is_some_and(|tm| tm.on) || sh.tumours.iter().flatten().any(|tm| tm.on)
}

/// What "Turn into live shape" changes about the selection, as warning lines (`convert.losses`).
/// Empty = nothing is lost. The app turns each one into the matching sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    /// Tumours become fixed points.
    Tumours,
    /// "Last note: starts on it" is dropped.
    EndDot,
}

/// A line kind as strokes in beats / pitch (`convert.line_strokes`); `path` is the shape's
/// `engine.shape_strokes`, used for tumours (the bumps become points).
pub fn line_strokes(sh: &Shape, paths: &[Vec<Pt>]) -> Vec<Stroke> {
    if has_tumours(sh) {
        return paths
            .iter()
            .map(|path| Stroke::Poly {
                pts: path.clone(),
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            })
            .collect();
    }
    match sh.kind {
        Kind::Curve => {
            let pts = &sh.pts;
            let sharp: BTreeSet<i64> = sh.sharp.iter().map(|&a| a as i64).collect();
            let last = anchor_count(pts) as i64 - 1;
            let mut ends: Vec<i64> = sh.gaps.iter().map(|&g| g as i64).collect();
            ends.push(last); // one curve per piece of a joined curve
            let mut out = Vec::new();
            let mut a0 = 0i64;
            for a1 in ends {
                let start = 3 * a0;
                let stop = 3 * a1 + 1;
                if start < 0 || stop as usize > pts.len() || start as usize >= stop as usize {
                    break;
                }
                let mut st = Stroke::Curve {
                    pts: pts[start as usize..stop as usize].to_vec(),
                    sharp: Vec::new(),
                    sym: None,
                    src: None,
                };
                let own: Vec<usize> = sharp
                    .iter()
                    .filter(|&&a| a0 < a && a < a1)
                    .map(|&a| (a - a0) as usize)
                    .collect();
                if let Stroke::Curve { sharp, .. } = &mut st {
                    *sharp = own;
                }
                if sh.sym.is_some()
                    && sh.gaps.is_empty()
                    && let Stroke::Curve { sym, .. } = &mut st
                {
                    *sym = sh.sym;
                }
                out.push(st);
                a0 = a1 + 1;
            }
            out
        }
        Kind::Arc => vec![Stroke::Arc {
            pts: sh.pts.clone(),
            k: sh.k,
            src: None,
        }],
        Kind::Free => vec![Stroke::Poly {
            pts: sh.pts.clone(),
            free: true,
            smooth: sh.smooth,
            k: sh.k,
            src: None,
        }],
        _ => vec![Stroke::Poly {
            pts: sh.pts.clone(),
            free: false,
            smooth: 0,
            k: 1.0,
            src: None,
        }],
    }
}

/// What turning these into a live shape changes, for the warning (`convert.losses`).
pub fn losses(shapes: &[Shape]) -> Vec<Loss> {
    let mut out = Vec::new();
    if shapes
        .iter()
        .any(|sh| LINE_KINDS.contains(&sh.kind) && has_tumours(sh))
    {
        out.push(Loss::Tumours);
    }
    if shapes
        .iter()
        .any(|sh| LINE_KINDS.contains(&sh.kind) && sh.end_dot)
    {
        out.push(Loss::EndDot);
    }
    out
}

/// The shapes as one new custom shape (`convert.to_live`). `paths[i]` is `engine.shape_strokes`
/// of `shapes[i]`. The first custom shape among them gives its name and fill settings.
pub fn to_live(
    shapes: &[Shape],
    paths: &[Vec<Vec<Pt>>],
    defaults: &Shape,
    custom_defaults: &CustomDefaults,
) -> Shape {
    let first = shapes.iter().find(|sh| sh.kind == Kind::Custom);
    let mut new = new_live_shape(defaults, custom_defaults);
    if let Some(first) = first {
        if !first.name.is_empty() {
            new.name = first.name.clone();
        }
        new.fill = first.fill;
        new.gate = first.gate;
        new.align = first.align;
        new.ends = first.ends;
    }
    let mut src: i64 = 0;
    for (sh, path) in shapes.iter().zip(paths.iter()) {
        if sh.kind == Kind::Custom {
            // A shape turned into a live shape before keeps its groups apart.
            let mut ids: BTreeMap<i64, i64> = BTreeMap::new();
            for k in 0..sh.strokes.len() {
                let key = sh.strokes[k].src().unwrap_or(-1);
                let id = *ids.entry(key).or_insert_with(|| {
                    let got = src;
                    src += 1;
                    got
                });
                let Some(mut got) = stroke_bp(sh, k) else {
                    continue;
                };
                got.set_src(Some(id));
                add_stroke(&mut new, &got, None);
            }
            continue;
        }
        for mut st in line_strokes(sh, path) {
            st.set_src(Some(src));
            add_stroke(&mut new, &st, None);
        }
        src += 1;
    }
    let spans: Vec<(f64, f64)> = paths
        .iter()
        .map(|ps| {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for p in ps {
                for q in p {
                    lo = lo.min(q[0]);
                    hi = hi.max(q[0]);
                }
            }
            (lo, hi)
        })
        .collect();
    let mine: Vec<f64> = custom_strokes(&new)
        .iter()
        .flat_map(|p| p.iter().map(|q| q[0]))
        .collect();
    if let (Some(lo), Some(hi)) = (
        mine.iter().copied().reduce(f64::min),
        mine.iter().copied().reduce(f64::max),
    ) {
        join_velocity(&mut new, shapes, &spans, (lo, hi)); // (each keeps its velocities)
    }
    new.from = Some(ShapeFrom {
        shapes: shapes.to_vec(),
        strokes: new.strokes.clone(),
        pts: new.pts.clone(),
    });
    new
}

/// Two strokes equal, numbers within `tol` (part of `convert._same`'s dict/list comparison).
fn same_stroke(a: &Stroke, b: &Stroke, tol: f64) -> bool {
    match (a, b) {
        (
            Stroke::Poly {
                pts: ap,
                free: af,
                smooth: asm,
                k: ak,
                src: asrc,
            },
            Stroke::Poly {
                pts: bp,
                free: bf,
                smooth: bsm,
                k: bk,
                src: bsrc,
            },
        ) => {
            af == bf
                && asm == bsm
                && asrc == bsrc
                && same_num(*ak, *bk, tol)
                && same_pts(ap, bp, tol)
        }
        (
            Stroke::Curve {
                pts: ap,
                sharp: ash,
                sym: asy,
                src: asrc,
            },
            Stroke::Curve {
                pts: bp,
                sharp: bsh,
                sym: bsy,
                src: bsrc,
            },
        ) => asrc == bsrc && ash == bsh && asy == bsy && same_pts(ap, bp, tol),
        (
            Stroke::Arc {
                pts: ap,
                k: ak,
                src: asrc,
            },
            Stroke::Arc {
                pts: bp,
                k: bk,
                src: bsrc,
            },
        ) => asrc == bsrc && same_num(*ak, *bk, tol) && same_pts(ap, bp, tol),
        (
            Stroke::Ellipse {
                box_: ab,
                src: asrc,
            },
            Stroke::Ellipse {
                box_: bb,
                src: bsrc,
            },
        ) => asrc == bsrc && ab.iter().zip(bb.iter()).all(|(x, y)| same_num(*x, *y, tol)),
        _ => false,
    }
}

fn same_pts(a: &[Pt], b: &[Pt], tol: f64) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| same_num(x[0], y[0], tol) && same_num(x[1], y[1], tol))
}

/// Python `math.isclose(a, b, rel_tol=tol, abs_tol=tol)`; ints and floats compare the same way.
fn same_num(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol.max(tol * a.abs().max(b.abs()))
}

/// The shapes a live shape was made of, moved to where it is now, or None if they can't come back
/// (none kept, or the drawing was changed: only moving the whole shape keeps them) (`convert.originals`).
pub fn originals(sh: &Shape) -> Option<Vec<Shape>> {
    let fr = sh.from.as_ref()?;
    if !same_value_strokes(&sh.strokes, &fr.strokes) || sh.pts.len() != 3 || fr.pts.len() != 3 {
        return None;
    }
    let (a0, b0) = (fr.pts[0][0], fr.pts[0][1]);
    let (a1, b1, a2, b2) = (fr.pts[1][0], fr.pts[1][1], fr.pts[2][0], fr.pts[2][1]);
    let (c0, d0) = (sh.pts[0][0], sh.pts[0][1]);
    let (c1, d1, c2, d2) = (sh.pts[1][0], sh.pts[1][1], sh.pts[2][0], sh.pts[2][1]);
    let (db, dp) = (c0 - a0, d0 - b0);
    let now = [c1 - a1, d1 - b1, c2 - a2, d2 - b2];
    let was = [db, dp, db, dp];
    if !now
        .iter()
        .zip(was.iter())
        .all(|(x, y)| same_num(*x, *y, 1e-9))
    {
        return None;
    }
    let mut out = fr.shapes.clone();
    for old in &mut out {
        for p in &mut old.pts {
            p[0] += db;
            p[1] += dp;
        }
    }
    Some(out)
}

fn same_value_strokes(a: &[Stroke], b: &[Stroke]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| same_stroke(x, y, 1e-7))
}

/// Convenience for the app: `engine.shape_strokes` of every shape, as [`to_live`]'s `paths`.
pub fn paths_of(shapes: &[Shape]) -> Vec<Vec<Vec<Pt>>> {
    shapes.iter().map(shape_strokes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::spam_gate;
    use crate::shape::Fill;

    fn line(kind: Kind, pts: Vec<Pt>) -> Shape {
        Shape {
            kind,
            pts,
            ..Shape::default()
        }
    }

    /// A curve with a gap becomes one stroke per piece; sharp anchors move with their piece.
    #[test]
    fn curve_gaps_split_into_pieces() {
        // 13 points = 5 anchors (0..4); a valid gap is 1..last-2 = 1..2.
        let pts: Vec<Pt> = (0..13).map(|i| [i as f64, 60.0 + (i % 3) as f64]).collect();
        let mut sh = line(Kind::Curve, pts);
        sh.gaps = vec![1];
        sh.sharp = vec![3];
        let strokes = line_strokes(&sh, &[]);
        assert_eq!(strokes.len(), 2);
        match &strokes[0] {
            Stroke::Curve { pts, sharp, .. } => {
                assert_eq!(pts.len(), 4);
                assert!(sharp.is_empty());
            }
            other => panic!("first piece should be a curve: {other:?}"),
        }
        match &strokes[1] {
            Stroke::Curve { pts, sharp, .. } => {
                assert_eq!(pts.len(), 7);
                assert_eq!(*sharp, vec![1]);
            }
            other => panic!("second piece should be a curve: {other:?}"),
        }
    }

    /// Tumour bumps become the sampled path as plain points.
    #[test]
    fn tumours_flatten_to_points() {
        let mut sh = line(Kind::Poly, vec![[0.0, 60.0], [1.0, 60.0]]);
        sh.tumour = Some(crate::shape::Tumour {
            on: true,
            ..Default::default()
        });
        let path = crate::engine::shape_strokes(&sh);
        let strokes = line_strokes(&sh, &path);
        assert_eq!(strokes.len(), 1);
        match &strokes[0] {
            Stroke::Poly { pts, .. } => assert!(pts.len() > 2, "bumps should add points"),
            other => panic!("expected a poly: {other:?}"),
        }
    }

    /// The warning lines: tumours and "last note" are the only losses, in that order.
    #[test]
    fn losses_report_tumours_then_end_dot() {
        let mut with_tm = line(Kind::Line, vec![[0.0, 60.0], [1.0, 60.0]]);
        with_tm.tumour = Some(crate::shape::Tumour {
            on: true,
            ..Default::default()
        });
        let mut with_dot = line(Kind::Poly, vec![[0.0, 60.0], [1.0, 60.0]]);
        with_dot.end_dot = true;
        let arc = line(Kind::Arc, vec![[0.0, 60.0], [0.5, 60.5], [1.0, 60.0]]);
        assert_eq!(losses(std::slice::from_ref(&arc)), Vec::new());
        assert_eq!(
            losses(&[with_tm, with_dot, arc]),
            vec![Loss::Tumours, Loss::EndDot]
        );
    }

    /// `originals` gives the old shapes back when the whole shape was moved; editing strokes or
    /// resizing the frame says they can't come back.
    #[test]
    fn originals_only_after_a_plain_move() {
        let src = vec![
            line(Kind::Poly, vec![[0.0, 60.0], [1.0, 60.0], [1.0, 62.0]]),
            line(Kind::Line, vec![[1.0, 62.0], [2.0, 62.0]]),
        ];
        let paths = paths_of(&src);
        let new = to_live(&src, &paths, &Shape::default(), &CustomDefaults::default());
        let back = originals(&new).expect("straight after converting they come back");
        assert_eq!(back, src);
        let mut moved = new.clone();
        moved.pts[0][0] += 3.0;
        moved.pts[1][0] += 3.0;
        moved.pts[2][0] += 3.0;
        let back = originals(&moved).expect("a move keeps them");
        assert_eq!(back[0].pts[0], [3.0, 60.0]);
        let mut resized = new.clone();
        resized.pts[1][1] += 1.0;
        assert!(originals(&resized).is_none());
        let mut edited = new.clone();
        if let Stroke::Poly { pts, .. } = &mut edited.strokes[0] {
            pts[0][0] += 0.5;
        }
        assert!(originals(&edited).is_none());
    }

    /// Spam still works after converting: the outline's notes don't change and `src` groups them.
    #[test]
    fn converted_notes_get_per_source_groups() {
        let src = vec![
            line(
                Kind::Poly,
                vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
            ),
            line(Kind::Line, vec![[2.0, 0.0], [2.0, 1.0]]),
        ];
        let paths = paths_of(&src);
        let new = to_live(&src, &paths, &Shape::default(), &CustomDefaults::default());
        assert_eq!(spam_gate(&new, 960.0), 60);
        let groups = crate::custom::stroke_groups(&new).expect("two source groups");
        assert_eq!(groups, vec![vec![0], vec![1]]);
        assert_eq!(new.strokes[0].src(), Some(0));
        assert_eq!(new.strokes[1].src(), Some(1));
    }

    /// The note rows of the converted shape match the originals' own rows.
    #[test]
    fn converted_shape_keeps_the_original_notes() {
        let src = vec![
            line(
                Kind::Curve,
                vec![[0.0, 60.0], [0.2, 61.0], [0.4, 61.0], [0.6, 60.0]],
            ),
            line(Kind::Arc, vec![[1.0, 60.0], [1.5, 61.0], [2.0, 60.0]]),
            line(Kind::Free, vec![[2.5, 61.0], [3.0, 60.0], [3.5, 61.0]]),
            line(Kind::Line, vec![[3.5, 61.0], [4.0, 61.0]]),
        ];
        let ppq = 960.0;
        let row4 = |n: &crate::Note| [n.start as i64, n.end as i64, n.key as i64, n.vel as i64];
        let before: Vec<Vec<crate::Note>> = src
            .iter()
            .map(|sh| crate::engine::shape_notes(sh, ppq, 128))
            .collect();
        let paths = paths_of(&src);
        let new = to_live(&src, &paths, &Shape::default(), &CustomDefaults::default());
        let after = crate::engine::shape_notes(&new, ppq, 128);
        let mut want: Vec<[i64; 4]> = before.iter().flatten().map(row4).collect();
        want.sort_unstable();
        let mut got: Vec<[i64; 4]> = after.iter().map(row4).collect();
        got.sort_unstable();
        assert_eq!(got, want, "converted notes should be the originals'");

        // Per-source channels: the converted shape's tracks tell each note's source apart.
        let (notes, tracks) = crate::engine::shape_notes_tracks(&new, ppq, 128);
        assert_eq!(notes, after);
        let tracks = tracks.expect("groups give tracks");
        let groups = crate::custom::stroke_groups(&new).expect("two or more source groups");
        assert!(groups.len() >= 2);
        assert_eq!(tracks.len(), after.len());
    }

    /// Auto channels: the converted shape's per-source groups get the same slots as the original
    /// shapes, so every note row (including its channel) stays the same.
    #[test]
    fn converted_channels_match_the_original_shapes() {
        use crate::engine::{Mode, Split};
        let ppq = 960.0;
        let src = vec![
            line(Kind::Line, vec![[0.0, 60.0], [4.0, 60.0]]),
            line(Kind::Line, vec![[1.0, 60.0], [3.0, 62.0]]),
            line(Kind::Arc, vec![[5.0, 60.0], [6.0, 61.0], [7.0, 60.0]]),
        ];
        let before_lists: Vec<Vec<crate::Note>> = src
            .iter()
            .map(|sh| crate::engine::shape_notes(sh, ppq, 128))
            .collect();
        let (before, _) = crate::engine::render(&before_lists, Mode::Auto, Split::Key, None, None);
        let paths = paths_of(&src);
        let new = to_live(&src, &paths, &Shape::default(), &CustomDefaults::default());
        let (notes, tracks) = crate::engine::shape_notes_tracks(&new, ppq, 128);
        let tracks = tracks.expect("per-source groups give tracks");
        let (after, _) = crate::engine::render(
            std::slice::from_ref(&notes),
            Mode::Auto,
            Split::Key,
            Some(&[Some(tracks)]),
            None,
        );
        let row = |r: &crate::Note| {
            [
                r.start as i64,
                r.end as i64,
                r.key as i64,
                r.vel as i64,
                r.slot as i64,
            ]
        };
        let mut a: Vec<[i64; 5]> = before.iter().map(row).collect();
        let mut b: Vec<[i64; 5]> = after.iter().map(row).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "note rows and channels should be the originals'");
    }

    /// The notes don't change even when the first custom shape gives the fill settings.
    #[test]
    fn converted_custom_shape_keeps_its_fill() {
        let mut custom = Shape {
            kind: Kind::Custom,
            name: "square".to_string(),
            fill: Fill::Fill,
            gate: 0.25,
            align: crate::shape::Align::Centred,
            pts: vec![[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]],
            strokes: vec![Stroke::Poly {
                pts: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            }],
            ..Shape::default()
        };
        custom.ends = crate::shape::Ends::Keep;
        let src = vec![
            line(Kind::Line, vec![[0.0, 66.0], [1.0, 66.0]]),
            custom.clone(),
        ];
        let paths = paths_of(&src);
        let new = to_live(&src, &paths, &Shape::default(), &CustomDefaults::default());
        assert_eq!(new.name, "square");
        assert_eq!(new.fill, Fill::Fill);
        assert_eq!(new.gate, 0.25);
        assert_eq!(new.align, crate::shape::Align::Centred);
        assert_eq!(new.ends, crate::shape::Ends::Keep);
        assert!(new.from.is_some());
    }
}
