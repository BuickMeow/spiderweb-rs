//! Live drawing and custom shape strokes (upstream roll/roll_live.py).
//!
//! With Live on, things drawn with line / poly / freehand / curve / arc / square / circle /
//! triangle become strokes of the same custom shape (the selected one, otherwise a new one);
//! square / circle / triangle always produce their own custom shape regardless of Live.
//! This module also handles: picking a custom shape's stroke ([`stroke_at`]), deleting it
//! ([`delete_stroke`]), and bending the picked curve stroke (anchors / handles; logic in
//! [`spiderweb_core::bezier`]). Strokes live in the shape's own frame (core custom's
//! `add_stroke` / `refit`).

use eframe::egui;
use egui::{Color32, Pos2, Rect, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::arc::arc_bezier;
use spiderweb_core::bezier::{self, HandleKind};
use spiderweb_core::custom::{
    CustomDefaults as CoreCustomDefaults, add_stroke, box_frame, frame_to_bp, frame_to_uv,
    new_live_shape, refit, stroke_ends,
};
use spiderweb_core::engine;
use spiderweb_core::shape::{Kind, Shape, Stroke as PathStroke};

use crate::app::{App, Tool};

/// Tools whose drawings become strokes when Live is on (roll_live.STROKE_TOOLS).
pub const STROKE_TOOLS: [Tool; 8] = [
    Tool::Line,
    Tool::Poly,
    Tool::Free,
    Tool::Curve,
    Tool::Arc,
    Tool::Square,
    Tool::Circle,
    Tool::Triangle,
];

/// Square / Circle / Triangle: always produce a custom shape (their own, or a stroke of the Live shape).
pub const BOX_TOOLS: [Tool; 3] = [Tool::Square, Tool::Circle, Tool::Triangle];

/// Stroke points of the built-in square (roll_live.SQUARE).
pub const SQUARE: [Pt; 5] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
/// Stroke points of the built-in triangle (roll_live.TRIANGLE, same as Drawer's built-in Triangle).
pub const TRIANGLE: [Pt; 4] = [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0], [0.0, 0.0]];
/// Point-count threshold for a long freehand stroke: above it, points appear only when the stroke is picked (roll_live.LONG_STROKE).
pub const LONG_STROKE: usize = 64;

/// Purple of the picked stroke (roll_draw.STROKE_POINT_COLOR / roll_live.draw_picked_stroke).
const PICK_COLOR: Color32 = Color32::from_rgb(0x7a, 0x1f, 0xe0);
/// Blue of curve anchors / handles (roll_curve.HANDLE_COLOR).
const HANDLE_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);

/// Width / height ratio kept for square / circle / triangle with Ctrl (roll_live.BOX_ASPECT).
pub fn box_aspect(tool: Tool) -> f64 {
    match tool {
        Tool::Triangle => 2.0 / 3.0_f64.sqrt(),
        _ => 1.0,
    }
}

pub fn is_box_tool(tool: Tool) -> bool {
    BOX_TOOLS.contains(&tool)
}

pub fn is_stroke_tool(tool: Tool) -> bool {
    STROKE_TOOLS.contains(&tool)
}

/// The project's custom defaults -> core custom defaults.
pub fn core_custom_defaults(app: &App) -> CoreCustomDefaults {
    CoreCustomDefaults {
        fill: app.custom_defaults.fill,
        gate: app.custom_defaults.gate,
        align: app.custom_defaults.align,
        ends: app.custom_defaults.ends,
        union: app.custom_defaults.union,
        apart: app.custom_defaults.apart,
    }
}

/// Strokes and width / height of the built-in templates (Circle / Square / Triangle).
pub fn builtin_shape(name: &str) -> Option<(Vec<PathStroke>, f64)> {
    let poly = |pts: &[Pt]| PathStroke::Poly {
        pts: pts.to_vec(),
        free: false,
        smooth: 0,
        k: 1.0,
        src: None,
    };
    match name {
        "Circle" => Some((
            vec![PathStroke::Ellipse {
                box_: [0.0, 0.0, 1.0, 1.0],
                src: None,
            }],
            1.0,
        )),
        "Square" => Some((vec![poly(&SQUARE)], 1.0)),
        "Triangle" => Some((vec![poly(&TRIANGLE)], 1.0)),
        _ => None,
    }
}

/// Shape template: first look for a same-named shape in the library `shapes/*.json` (normalized), otherwise fall back to the built-in template (upstream custom_template).
pub fn builtin_template(dir: &std::path::Path, name: &str) -> Option<(Vec<PathStroke>, f64)> {
    crate::drawer::library_template(dir, name).or_else(|| builtin_shape(name))
}

/// Custom shape dragged out with square / circle / triangle (pure logic of roll_live.box_draft).
pub fn box_draft_parts(
    defaults: &Shape,
    cd: &CoreCustomDefaults,
    tool: Tool,
    a: Pt,
    b: Pt,
) -> Shape {
    let poly = |pts: &[Pt]| PathStroke::Poly {
        pts: pts.to_vec(),
        free: false,
        smooth: 0,
        k: 1.0,
        src: None,
    };
    let strokes = match tool {
        Tool::Circle => vec![PathStroke::Ellipse {
            box_: [0.0, 0.0, 1.0, 1.0],
            src: None,
        }],
        Tool::Triangle => vec![poly(&TRIANGLE)],
        _ => vec![poly(&SQUARE)],
    };
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = tool.label().to_string();
    sh.strokes = strokes;
    cd.apply(&mut sh);
    sh.pts = box_frame(a[0], a[1], b[0], b[1]).to_vec();
    sh
}

/// Puts a built-in template picked in the panel into the frame (upstream new_custom).
pub fn new_custom_parts(
    defaults: &Shape,
    cd: &CoreCustomDefaults,
    name: &str,
    strokes: &[PathStroke],
    a: Pt,
    b: Pt,
) -> Shape {
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = name.to_string();
    sh.strokes = strokes.to_vec();
    cd.apply(&mut sh);
    sh.pts = box_frame(a[0], a[1], b[0], b[1]).to_vec();
    sh
}

/// Custom shape dragged out with square / circle / triangle.
pub fn box_draft(app: &App, tool: Tool, a: Pt, b: Pt) -> Shape {
    box_draft_parts(
        &app.defaults.clone(),
        &core_custom_defaults(app),
        tool,
        a,
        b,
    )
}

/// Box from (start) to (pt); corrects pt to the drawing's width / height ratio (Ctrl)
/// (roll_custom.keep_aspect). keys is the project's key range: pitch is clamped to
/// 0 .. keys - 1.
pub fn keep_aspect_xy(start: Pt, pt: Pt, aspect: f64, sx: f64, sy: f64, keys: i64) -> Pt {
    let mut dx = (pt[0] - start[0]) * sx;
    let mut dy = (pt[1] - start[1]) * sy;
    if dx.abs() > dy.abs() * aspect {
        dy = (dx.abs() / aspect).copysign(if dy == 0.0 { 1.0 } else { dy });
    } else {
        dx = (dy.abs() * aspect).copysign(if dx == 0.0 { 1.0 } else { dx });
    }
    [
        (start[0] + dx / sx).max(0.0),
        (start[1] + dy / sy).clamp(0.0, (keys - 1) as f64),
    ]
}

/// Box aspect correction for Ctrl.
pub fn keep_aspect(app: &App, start: Pt, pt: Pt, aspect: f64) -> Pt {
    keep_aspect_xy(start, pt, aspect, app.view.sx, app.view.sy, app.keys)
}

// ---------------------------------------------------------------- Live drawing

/// Live is on and the current tool draws strokes (roll_live.live_drawing).
pub fn live_drawing(app: &App) -> bool {
    app.live && is_stroke_tool(app.tool)
}

/// The custom shape new strokes are drawn into (Live on and exactly one custom shape selected): returns its index.
pub fn live_target(app: &App) -> Option<usize> {
    let i = app.sel?;
    let sh = app.shapes.get(i)?;
    (app.live
        && sh.kind == Kind::Custom
        && sh.text.is_none()
        && sh.notes.is_none()
        && app.sels.len() == 1)
        .then_some(i)
}

/// event_pt, and while Live drawing: snaps exactly onto a stroke end of the target shape (or the start of the polyline being drawn) when close enough (roll_live.draw_pt).
pub fn draw_pt(app: &App, pos: Pos2, snap: bool, shift: bool) -> Pt {
    let pt = crate::roll::event_pt(app, pos, snap, shift);
    if !live_drawing(app) {
        return pt;
    }
    let mut near: Vec<Pt> = Vec::new();
    if let Some(i) = live_target(app)
        && let Some(to_bp) = frame_to_bp(&app.shapes[i].pts)
    {
        near.extend(
            stroke_ends(&app.shapes[i].strokes)
                .into_iter()
                .map(|q| to_bp(q[0], q[1])),
        );
    }
    if let Some(d) = &app.draft
        && d.kind == Kind::Poly
        && d.pts.len() >= 3
    {
        near.push(d.pts[0]);
    }
    let mut best: Option<Pt> = None;
    let mut reach = 8.0 * app.scale() as f64;
    for q in near {
        let d = ((app.view.x_of(q[0]) - pos.x).powi(2) + (app.view.y_of(q[1]) - pos.y).powi(2))
            .sqrt() as f64;
        if d < reach {
            best = Some(q);
            reach = d;
        }
    }
    best.unwrap_or(pt)
}

/// Finished draft -> a stroke (an arc becomes a curve so it can bend like one) (roll_live.draft_stroke).
pub fn draft_stroke(sh: &Shape, draw: Option<Tool>) -> Option<PathStroke> {
    if let Some(tool) = draw
        && is_box_tool(tool)
    {
        let a = *sh.pts.first()?;
        let b = *sh.pts.get(1)?;
        let c = *sh.pts.get(2)?;
        let (b0, p0, b1, p1) = (a[0], a[1], b[0], c[1]);
        let poly = |pts: Vec<Pt>| PathStroke::Poly {
            pts,
            free: false,
            smooth: 0,
            k: 1.0,
            src: None,
        };
        return Some(match tool {
            Tool::Circle => PathStroke::Ellipse {
                box_: [b0, p0, b1, p1],
                src: None,
            },
            Tool::Triangle => poly(vec![[b0, p0], [b1, p0], [(b0 + b1) / 2.0, p1], [b0, p0]]),
            _ => poly(vec![[b0, p0], [b1, p0], [b1, p1], [b0, p1], [b0, p0]]),
        });
    }
    match sh.kind {
        Kind::Curve => Some(PathStroke::Curve {
            pts: sh.pts.clone(),
            sharp: sh.sharp.clone(),
            sym: sh.sym,
            src: None,
        }),
        Kind::Arc => Some(PathStroke::Curve {
            pts: arc_bezier(&sh.pts, sh.k),
            sharp: Vec::new(),
            sym: None,
            src: None,
        }),
        Kind::Free => Some(PathStroke::Poly {
            pts: sh.pts.clone(),
            free: true,
            smooth: sh.smooth,
            k: sh.k,
            src: None,
        }),
        _ => Some(PathStroke::Poly {
            pts: sh.pts.clone(),
            free: false,
            smooth: 0,
            k: 1.0,
            src: None,
        }),
    }
}

/// The finished draft becomes a stroke of the Live shape (or a new one); square / circle /
/// triangle without Live become their own custom shape. true = handled (the caller should not
/// add a shape), false = ordinary shape (the caller adds it) (roll_live.live_commit).
pub fn live_commit(app: &mut App, sh: &Shape) -> bool {
    let draw = app.draft_draw.take();
    let is_box = draw.is_some_and(is_box_tool);
    if !is_box {
        let kind_tool = draw.or(match sh.kind {
            Kind::Line => Some(Tool::Line),
            Kind::Poly => Some(Tool::Poly),
            Kind::Free => Some(Tool::Free),
            Kind::Curve => Some(Tool::Curve),
            Kind::Arc => Some(Tool::Arc),
            _ => None,
        });
        if !(app.live && kind_tool.is_some_and(is_stroke_tool)) {
            return false;
        }
    }
    let Some(st) = draft_stroke(sh, draw) else {
        return true; // frame has too few points: drop it
    };
    if is_box {
        let (Some(a), Some(b), Some(c)) = (sh.pts.first(), sh.pts.get(1), sh.pts.get(2)) else {
            return true;
        };
        if a[0] == b[0] || a[1] == c[1] {
            return true; // no width or height: drop it
        }
    } else if let PathStroke::Poly { pts, .. }
    | PathStroke::Curve { pts, .. }
    | PathStroke::Arc { pts, .. } = &st
        && pts.first().is_none_or(|p| pts.iter().all(|q| q == p))
    {
        return true; // all points coincide: drop it
    }
    let is_curve_stroke =
        |target: &Shape, k: usize| matches!(target.strokes.get(k), Some(PathStroke::Curve { .. }));
    match live_target(app) {
        None if !app.live => {
            app.add_shape(sh.clone());
            true
        }
        None => {
            let defaults = app.defaults.clone();
            let cd = core_custom_defaults(app);
            let mut target = new_live_shape(&defaults, &cd);
            let Some(k) = add_stroke(&mut target, &st, None) else {
                return true;
            };
            let curve = is_curve_stroke(&target, k);
            app.add_shape(target);
            app.set_stroke(curve.then_some(k));
            true
        }
        Some(i) => {
            app.push_undo(&rust_i18n::t!("roll_live.draw_into_the_live_shape"));
            let k = app
                .shapes
                .get_mut(i)
                .and_then(|target| add_stroke(target, &st, None));
            if let Some(k) = k {
                let curve = app
                    .shapes
                    .get(i)
                    .is_some_and(|target| is_curve_stroke(target, k));
                app.set_stroke(curve.then_some(k));
            }
            app.shapes_changed();
            true
        }
    }
}

// ---------------------------------------------------------------- strokes

/// Distance from a screen point to a segment (roll_funnel.seg_dist).
fn seg_dist(x: f32, y: f32, a: Pos2, b: Pos2) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let ll = dx * dx + dy * dy;
    let u = if ll == 0.0 {
        0.0
    } else {
        (((x - a.x) * dx + (y - a.y) * dy) / ll).clamp(0.0, 1.0)
    };
    ((x - a.x - u * dx).powi(2) + (y - a.y - u * dy).powi(2)).sqrt()
}

/// A shape stroke's path on screen.
fn screen_path(app: &App, path: &[Pt]) -> Vec<Pos2> {
    path.iter()
        .map(|p| Pos2::new(app.view.x_of(p[0]), app.view.y_of(p[1])))
        .collect()
}

/// Stroke number under screen point (x, y) (roll_live.stroke_at).
pub fn stroke_at(app: &App, sh: &Shape, pos: Pos2, near: f32) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (k, path) in engine::shape_strokes(sh).iter().enumerate() {
        let pts = screen_path(app, path);
        let mut d = f32::INFINITY;
        for w in pts.windows(2) {
            d = d.min(seg_dist(pos.x, pos.y, w[0], w[1]));
        }
        if d <= near && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

/// The picked stroke number (roll_live.picked_stroke).
pub fn picked_stroke(app: &App, sh: &Shape) -> Option<usize> {
    let k = app.stroke?;
    (sh.kind == Kind::Custom && k < sh.strokes.len()).then_some(k)
}

/// Returns the picked stroke when it is a curve in this stroke list (roll_live.stroke_curve).
fn picked_curve_in(app: &App, strokes: &[PathStroke]) -> Option<usize> {
    let k = app.stroke?;
    matches!(strokes.get(k), Some(PathStroke::Curve { .. })).then_some(k)
}

/// Returns the picked stroke when it is a curve (roll_live.stroke_curve).
fn picked_curve_stroke(app: &App, sh: &Shape) -> Option<usize> {
    picked_stroke(app, sh)?;
    picked_curve_in(app, &sh.strokes)
}

/// Raw point count of a stroke (an ellipse has no raw points).
fn stroke_raw_len(st: &PathStroke) -> usize {
    match st {
        PathStroke::Poly { pts, .. }
        | PathStroke::Curve { pts, .. }
        | PathStroke::Arc { pts, .. } => pts.len(),
        PathStroke::Ellipse { .. } => usize::MAX,
    }
}

/// Strokes that show points (and can be dragged with the Select tool): all of them when Live
/// is on and exactly one shape is selected (long freehand strokes only when picked), otherwise
/// just the picked one (roll_live.point_strokes).
pub fn point_strokes(app: &App, sh: &Shape) -> Vec<usize> {
    if sh.text.is_some() || sh.notes.is_some() {
        return Vec::new();
    }
    let k = picked_stroke(app, sh);
    if app.live && app.sels.len() == 1 {
        sh.strokes
            .iter()
            .enumerate()
            .filter(|(i, st)| Some(*i) == k || stroke_raw_len(st) <= LONG_STROKE)
            .map(|(i, _)| i)
            .collect()
    } else {
        k.into_iter().collect()
    }
}

/// Draggable points of a stroke as (point number, (u, v)): polyline points, the two ends of a
/// curve, the left / right / bottom / top of an ellipse (point numbers 0-3) (roll_live.stroke_spots).
pub fn stroke_spots(st: &PathStroke) -> Vec<(usize, Pt)> {
    match st {
        PathStroke::Ellipse { box_, .. } => {
            let (u0, v0, u1, v1) = (box_[0], box_[1], box_[2], box_[3]);
            let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
            vec![(0, [u0, cv]), (1, [u1, cv]), (2, [cu, v0]), (3, [cu, v1])]
        }
        PathStroke::Curve { pts, .. } => {
            if pts.is_empty() {
                Vec::new()
            } else {
                vec![(0, pts[0]), (pts.len() - 1, pts[pts.len() - 1])]
            }
        }
        PathStroke::Poly { pts, .. } | PathStroke::Arc { pts, .. } => {
            pts.iter().copied().enumerate().collect()
        }
    }
}

/// A stroke handle of a custom shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeHandleId {
    /// A stroke point (Select tool): stroke number and point number
    Pt { k: usize, j: usize },
    /// Anchor / handle of the picked curve stroke: ctrl distinguishes handles, j is the point number
    Curve { ctrl: bool, j: usize },
}

/// Stroke handles to display for the selected custom shape as
/// `(beat, pitch, handle, available with any tool)` (roll_live.stroke_handles).
pub fn stroke_handles(app: &App, sh: &Shape) -> Vec<(Pt, StrokeHandleId, bool)> {
    let Some(to_bp) = frame_to_bp(&sh.pts) else {
        return Vec::new();
    };
    let mut out: Vec<(Pt, StrokeHandleId, bool)> = Vec::new();
    for k in point_strokes(app, sh) {
        if let Some(st) = sh.strokes.get(k) {
            for (j, p) in stroke_spots(st) {
                out.push((to_bp(p[0], p[1]), StrokeHandleId::Pt { k, j }, false));
            }
        }
    }
    if let Some(k) = picked_curve_stroke(app, sh)
        && let Some(PathStroke::Curve { pts, .. }) = sh.strokes.get(k)
    {
        for (j, kind) in bezier::pen_handles(pts, true, &[]) {
            if kind == HandleKind::End {
                continue;
            }
            if let Some(p) = pts.get(j) {
                out.push((
                    to_bp(p[0], p[1]),
                    StrokeHandleId::Curve {
                        ctrl: kind == HandleKind::Ctrl,
                        j,
                    },
                    true,
                ));
            }
        }
    }
    out
}

/// Hit tests stroke handles on screen; stroke points are also hit testable when `any` is true (Select tool).
pub fn hit_stroke_handle(app: &App, sh: &Shape, pos: Pos2, any: bool) -> Option<StrokeHandleId> {
    let near = 7.0_f32.max(8.0 * app.scale());
    for (p, id, free) in stroke_handles(app, sh).into_iter().rev() {
        if !(free || any) {
            continue;
        }
        let x = app.view.x_of(p[0]);
        let y = app.view.y_of(p[1]);
        if (x - pos.x).abs() <= near && (y - pos.y).abs() <= near {
            return Some(id);
        }
    }
    None
}

/// The shape state a stroke drag edits: (u, v) mapping and frame points.
struct StrokeFrame {
    frame: Vec<Pt>,
    strokes: Vec<PathStroke>,
}

fn stroke_frame(app: &App) -> Option<(usize, StrokeFrame)> {
    let i = app.sel?;
    let sh = app.shapes.get(i)?;
    if sh.kind != Kind::Custom {
        return None;
    }
    Some((
        i,
        StrokeFrame {
            frame: sh.pts.clone(),
            strokes: sh.strokes.clone(),
        },
    ))
}

/// Writes the changes back to the shape and refits the frame.
fn commit_strokes(app: &mut App, i: usize, frame: Vec<Pt>, strokes: Vec<PathStroke>) {
    if let Some(target) = app.shapes.get_mut(i) {
        target.pts = frame;
        target.strokes = strokes;
        refit(target);
    }
}

/// Drags a stroke point ("pt"): snaps to the grid (Shift = free) and lands on another
/// stroke's point within a few pixels (so outlines can join up). All stroke points at the same
/// position move together; a curve end takes its handles along, and an ellipse edge point
/// changes that side. Returns the handle to keep using (dragging an ellipse edge past the
/// opposite side turns into that side) (roll_live.drag_stroke_point).
pub fn drag_stroke_point(
    app: &mut App,
    hid: StrokeHandleId,
    pos: Pos2,
    shift: bool,
) -> StrokeHandleId {
    let StrokeHandleId::Pt { k, j } = hid else {
        return hid;
    };
    let Some((i, fr)) = stroke_frame(app) else {
        return hid;
    };
    let Some(to_uv) = frame_to_uv(&fr.frame) else {
        return hid;
    };
    let Some(st) = fr.strokes.get(k).cloned() else {
        return hid;
    };
    if let PathStroke::Ellipse { box_, .. } = st {
        let mut box_ = box_;
        let pt = crate::roll::event_pt(app, pos, true, shift);
        let uv = to_uv(pt[0], pt[1]);
        let idx = [0, 2, 1, 3][j.min(3)];
        box_[idx] = if j < 2 { uv[0] } else { uv[1] };
        let flipped = if j < 2 {
            box_[0] > box_[2]
        } else {
            box_[1] > box_[3]
        };
        if let Some(target) = app.shapes.get_mut(i) {
            if let Some(PathStroke::Ellipse { box_: b, .. }) = target.strokes.get_mut(k) {
                *b = box_;
            }
            refit(target);
        }
        return if flipped {
            StrokeHandleId::Pt { k, j: j ^ 1 }
        } else {
            hid
        };
    }
    let Some(old) = stroke_spots(&st)
        .into_iter()
        .find(|(n, _)| *n == j)
        .map(|(_, p)| p)
    else {
        return hid;
    };
    let near_old = |p: Pt| ((p[0] - old[0]).powi(2) + (p[1] - old[1]).powi(2)).sqrt() < 1e-7;
    let mut joined: Vec<(usize, usize)> = Vec::new();
    for (ii, other) in fr.strokes.iter().enumerate() {
        if matches!(other, PathStroke::Ellipse { .. }) {
            continue;
        }
        for (n, p) in stroke_spots(other) {
            if near_old(p) {
                joined.push((ii, n));
            }
        }
    }
    if joined.is_empty() {
        joined.push((k, j));
    }
    let Some(to_bp) = frame_to_bp(&fr.frame) else {
        return hid;
    };
    let pt = crate::roll::event_pt(app, pos, true, shift);
    let mut best: Option<Pt> = None;
    let mut reach = 8.0 * app.scale() as f64;
    for (ii, other) in fr.strokes.iter().enumerate() {
        if matches!(other, PathStroke::Ellipse { .. }) {
            continue;
        }
        for (n, p) in stroke_spots(other) {
            if joined.contains(&(ii, n)) {
                continue;
            }
            let bp = to_bp(p[0], p[1]);
            let d = ((app.view.x_of(bp[0]) - pos.x).powi(2)
                + (app.view.y_of(bp[1]) - pos.y).powi(2))
            .sqrt() as f64;
            if d < reach {
                best = Some(bp);
                reach = d;
            }
        }
    }
    let new = to_uv(best.unwrap_or(pt)[0], best.unwrap_or(pt)[1]);
    let mut strokes = fr.strokes;
    for (ii, n) in joined {
        let curve = matches!(strokes.get(ii), Some(PathStroke::Curve { .. }));
        let pts = match strokes.get_mut(ii) {
            Some(
                PathStroke::Poly { pts, .. }
                | PathStroke::Curve { pts, .. }
                | PathStroke::Arc { pts, .. },
            ) => pts,
            _ => continue,
        };
        if n >= pts.len() {
            continue;
        }
        let du = new[0] - pts[n][0];
        let dv = new[1] - pts[n][1];
        pts[n] = new;
        if curve {
            let h = if n == 0 { 1 } else { n - 1 };
            if h < pts.len() {
                pts[h][0] += du;
                pts[h][1] += dv;
            }
        }
    }
    commit_strokes(app, i, fr.frame, strokes);
    hid
}

/// (u, v) <-> screen mapping (roll_live.stroke_maps).
#[allow(clippy::type_complexity)] // the two closure types cannot be given an alias
fn stroke_maps(
    app: &App,
    frame: &[Pt],
) -> Option<(
    impl Fn(Pt) -> [f64; 2] + use<>,
    impl Fn(f64, f64) -> Pt + use<>,
)> {
    let to_bp = frame_to_bp(frame)?;
    let to_uv = frame_to_uv(frame)?;
    let view = app.view.clone();
    let view2 = app.view.clone();
    Some((
        move |p: Pt| {
            let bp = to_bp(p[0], p[1]);
            [view.x_of(bp[0]) as f64, view.y_of(bp[1]) as f64]
        },
        move |x: f64, y: f64| to_uv(view2.b_of(x as f32), view2.p_of(y as f32)),
    ))
}

/// Drags a stroke handle: stroke points go to [`drag_stroke_point`], anchors / handles of the
/// picked curve go through bezier (snapped, Alt as in bezier.drag_point). Returns the handle
/// to keep using (roll_live.drag_stroke).
pub fn drag_stroke_handle(
    app: &mut App,
    hid: StrokeHandleId,
    pos: Pos2,
    shift: bool,
    alt: bool,
) -> StrokeHandleId {
    match hid {
        StrokeHandleId::Pt { .. } => drag_stroke_point(app, hid, pos, shift),
        StrokeHandleId::Curve { j, .. } => {
            let Some((i, fr)) = stroke_frame(app) else {
                return hid;
            };
            let Some(k) = picked_curve_in(app, &fr.strokes) else {
                return hid;
            };
            let Some(PathStroke::Curve {
                pts, sharp, sym, ..
            }) = fr.strokes.get(k)
            else {
                return hid;
            };
            let mut c = bezier::Curve {
                pts: pts.clone(),
                sharp: sharp.clone(),
                sym: *sym,
                ..Default::default()
            };
            let Some((to_screen, from_screen)) = stroke_maps(app, &fr.frame) else {
                return hid;
            };
            let Some(to_uv) = frame_to_uv(&fr.frame) else {
                return hid;
            };
            let pt = crate::roll::event_pt(app, pos, true, shift);
            let uv = to_uv(pt[0], pt[1]);
            bezier::drag_point(&mut c, j, uv, alt, &to_screen, &from_screen, false);
            let mut strokes = fr.strokes;
            if let Some(PathStroke::Curve {
                pts, sharp, sym, ..
            }) = strokes.get_mut(k)
            {
                *pts = c.pts;
                *sharp = c.sharp;
                *sym = c.sym;
            }
            commit_strokes(app, i, fr.frame, strokes);
            hid
        }
    }
}

/// Right-click on a point of the picked curve stroke: an anchor is deleted and handles retract (roll_live.delete_stroke_handle).
pub fn delete_stroke_handle(app: &mut App, hid: StrokeHandleId) {
    let StrokeHandleId::Curve { j, .. } = hid else {
        return; // stroke point: right-click counts as being on the stroke (menu / deselect)
    };
    let Some((i, fr)) = stroke_frame(app) else {
        return;
    };
    let Some(k) = picked_curve_in(app, &fr.strokes) else {
        return;
    };
    let Some(PathStroke::Curve {
        pts, sharp, sym, ..
    }) = fr.strokes.get(k)
    else {
        return;
    };
    let mut c = bezier::Curve {
        pts: pts.clone(),
        sharp: sharp.clone(),
        sym: *sym,
        ..Default::default()
    };
    match bezier::can_delete(&c, j) {
        None => {}
        Some(bezier::CanDelete::Middle) => {
            app.status = rust_i18n::t!("status.middle_anchor").to_string();
        }
        Some(_) => {
            let Some((to_screen, _)) = stroke_maps(app, &fr.frame) else {
                return;
            };
            app.push_undo(&rust_i18n::t!("roll_live.remove_a_point"));
            bezier::delete_point(&mut c, j, &to_screen, false);
            let mut strokes = fr.strokes;
            if let Some(PathStroke::Curve {
                pts, sharp, sym, ..
            }) = strokes.get_mut(k)
            {
                *pts = c.pts;
                *sharp = c.sharp;
                *sym = c.sym;
            }
            commit_strokes(app, i, fr.frame, strokes);
            app.shapes_changed();
        }
    }
}

/// Adds an anchor at the point of the picked curve stroke closest to the mouse and moves it
/// there; only within `near`. true = added (roll_live.stroke_click).
pub fn stroke_click(app: &mut App, pos: Pos2, near: Option<f64>, shift: bool) -> bool {
    let Some((i, fr)) = stroke_frame(app) else {
        return false;
    };
    let Some(k) = picked_curve_in(app, &fr.strokes) else {
        return false;
    };
    let Some(PathStroke::Curve {
        pts, sharp, sym, ..
    }) = fr.strokes.get(k)
    else {
        return false;
    };
    let mut c = bezier::Curve {
        pts: pts.clone(),
        sharp: sharp.clone(),
        sym: *sym,
        ..Default::default()
    };
    let Some((to_screen, _)) = stroke_maps(app, &fr.frame) else {
        return false;
    };
    let Some((seg, t, d)) =
        bezier::nearest(&c.pts, &to_screen, pos.x as f64, pos.y as f64, 64, &[])
    else {
        return false;
    };
    if near.is_some_and(|n| d > n) {
        return false;
    }
    let Some(to_uv) = frame_to_uv(&fr.frame) else {
        return false;
    };
    let pt = crate::roll::event_pt(app, pos, true, shift);
    let uv = to_uv(pt[0], pt[1]);
    if !bezier::add_anchor(&mut c, seg, t, uv, &to_screen, false) {
        return false;
    }
    app.push_undo(&rust_i18n::t!("roll_live.add_an_anchor"));
    let mut strokes = fr.strokes;
    if let Some(PathStroke::Curve {
        pts, sharp, sym, ..
    }) = strokes.get_mut(k)
    {
        *pts = c.pts.clone();
        *sharp = c.sharp.clone();
        *sym = c.sym;
    }
    commit_strokes(app, i, fr.frame, strokes);
    app.shapes_changed();
    true
}

/// Deletes stroke k from the custom shape (the last one: deletes the whole shape) (roll_live.delete_stroke).
pub fn delete_stroke(app: &mut App, i: usize, k: usize) {
    let n = app.shapes.get(i).map(|s| s.strokes.len()).unwrap_or(0);
    if n <= 1 {
        app.delete_selected();
        return;
    }
    if k >= n {
        return;
    }
    app.push_undo(&rust_i18n::t!("roll_live.delete_a_stroke"));
    if let Some(target) = app.shapes.get_mut(i) {
        target.strokes.remove(k);
        refit(target);
    }
    app.set_stroke(None);
    app.shapes_changed();
}

/// Position of (b, p) on screen.
fn at(app: &App, rect: Rect, p: Pt) -> Pos2 {
    Pos2::new(
        rect.min.x + app.view.x_of(p[0]),
        rect.min.y + app.view.y_of(p[1]),
    )
}

// ---------------------------------------------------------------- painting

/// The picked stroke: a thick purple line, under the handles (roll_live.draw_picked_stroke).
pub fn paint_picked_stroke(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let Some(k) = picked_stroke(app, sh) else {
        return;
    };
    let paths = engine::shape_strokes(sh);
    let Some(path) = paths.get(k) else {
        return;
    };
    if path.len() < 2 {
        return;
    }
    let pts: Vec<Pos2> = path.iter().map(|p| at(app, rect, *p)).collect();
    painter.add(egui::Shape::line(
        pts,
        egui::Stroke::new((3.0 * app.scale()).max(3.0), PICK_COLOR),
    ));
}

/// Stroke points of the selected custom shape and anchors / handles of the picked curve stroke (the custom branch of roll_draw.draw_handles).
pub fn paint_custom_handles(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let s = app.scale();
    if let Some(k) = picked_curve_stroke(app, sh)
        && let Some(PathStroke::Curve { pts, .. }) = sh.strokes.get(k)
        && let Some(to_bp) = frame_to_bp(&sh.pts)
    {
        for (a, h) in bezier::handle_lines(pts, &[]) {
            let pa = at(app, rect, to_bp(a[0], a[1]));
            let ph = at(app, rect, to_bp(h[0], h[1]));
            painter.line_segment(
                [pa, ph],
                egui::Stroke::new((3.5 * s).max(3.0), Color32::WHITE),
            );
            painter.line_segment(
                [pa, ph],
                egui::Stroke::new((1.5 * s).max(1.0), HANDLE_COLOR),
            );
        }
    }
    for (p, id, _) in stroke_handles(app, sh) {
        let q = at(app, rect, p);
        match id {
            StrokeHandleId::Pt { .. } => {
                let r = 3.0 * s;
                let box_ = Rect::from_center_size(q, Vec2::splat(r * 2.0));
                painter.rect_filled(box_, 0.0, Color32::WHITE);
                painter.rect_stroke(
                    box_,
                    0.0,
                    egui::Stroke::new(2.0, PICK_COLOR),
                    egui::StrokeKind::Inside,
                );
            }
            StrokeHandleId::Curve { ctrl: true, .. } => {
                let r = 4.5 * s;
                painter.circle_filled(q, r, HANDLE_COLOR);
                painter.circle_stroke(q, r, egui::Stroke::new(1.0 * s, Color32::WHITE));
            }
            StrokeHandleId::Curve { ctrl: false, .. } => {
                let r = 5.5 * s;
                painter.circle_filled(q, r, Color32::WHITE);
                painter.circle_stroke(q, r, egui::Stroke::new((2.0 * s).max(2.0), HANDLE_COLOR));
            }
        }
    }
}

/// Even-odd rule: inside when a ray to the right from (b, p) crosses the outline an odd number of times (roll_custom.inside_strokes).
pub fn inside_strokes(strokes: &[Vec<Pt>], b: f64, p: f64) -> bool {
    let mut inside = false;
    for poly in strokes {
        for w in poly.windows(2) {
            let (xa, ya) = (w[0][0], w[0][1]);
            let (xb, yb) = (w[1][0], w[1][1]);
            if (ya <= p) != (yb <= p) && b < xa + (xb - xa) * (p - ya) / (yb - ya) {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use spiderweb_core::custom::CustomDefaults;

    fn defaults() -> Shape {
        Shape::default()
    }

    fn cd() -> CustomDefaults {
        CustomDefaults {
            fill: spiderweb_core::shape::Fill::Spam,
            gate: 0.125,
            align: spiderweb_core::shape::Align::Aligned,
            ends: spiderweb_core::shape::Ends::Keep,
            union: true,
            apart: true,
        }
    }

    #[test]
    fn box_draft_square_uses_poly_and_box_frame() {
        let sh = box_draft_parts(&defaults(), &cd(), Tool::Square, [1.0, 3.0], [4.0, 8.0]);
        assert_eq!(sh.kind, Kind::Custom);
        assert_eq!(sh.name, "Square");
        assert_eq!(sh.pts, vec![[1.0, 3.0], [4.0, 3.0], [1.0, 8.0]]);
        assert_eq!(sh.fill, spiderweb_core::shape::Fill::Spam);
        assert_eq!(sh.gate, 0.125);
        assert_eq!(sh.ends, spiderweb_core::shape::Ends::Keep);
        assert!(sh.union && sh.apart);
        match &sh.strokes[..] {
            [PathStroke::Poly { pts, .. }] => assert_eq!(pts.as_slice(), &SQUARE),
            other => panic!("expected poly stroke, got {other:?}"),
        }
    }

    #[test]
    fn box_draft_circle_is_ellipse() {
        let sh = box_draft_parts(&defaults(), &cd(), Tool::Circle, [0.0, 0.0], [2.0, 1.0]);
        assert_eq!(sh.name, "Circle");
        assert_eq!(sh.pts, vec![[0.0, 0.0], [2.0, 0.0], [0.0, 1.0]]);
        assert_eq!(
            sh.strokes,
            vec![PathStroke::Ellipse {
                box_: [0.0, 0.0, 1.0, 1.0],
                src: None
            }]
        );
    }

    #[test]
    fn box_draft_triangle_uses_builtin_triangle() {
        let sh = box_draft_parts(&defaults(), &cd(), Tool::Triangle, [0.0, 5.0], [4.0, 7.0]);
        assert_eq!(sh.name, "Triangle");
        match &sh.strokes[..] {
            [PathStroke::Poly { pts, .. }] => assert_eq!(pts.as_slice(), &TRIANGLE),
            other => panic!("expected poly stroke, got {other:?}"),
        }
    }

    #[test]
    fn box_aspect_matches_original() {
        assert_eq!(box_aspect(Tool::Square), 1.0);
        assert_eq!(box_aspect(Tool::Circle), 1.0);
        let t = box_aspect(Tool::Triangle);
        assert!((t - 2.0 / 3.0_f64.sqrt()).abs() < 1e-12);
        assert!((t - 1.1547005383792515).abs() < 1e-12);
    }

    #[test]
    fn keep_aspect_wider_than_tall_fixes_dy() {
        let got = keep_aspect_xy([0.0, 0.0], [4.0, 1.0], 2.0, 10.0, 10.0, 128);
        assert_eq!(got, [4.0, 2.0]);
    }

    #[test]
    fn keep_aspect_taller_than_wide_fixes_dx() {
        // 40px tall, 10px wide, aspect 2 -> width must be 80px = 8 beats
        let got = keep_aspect_xy([0.0, 0.0], [1.0, 4.0], 2.0, 10.0, 10.0, 128);
        assert_eq!(got, [8.0, 4.0]);
    }

    #[test]
    fn keep_aspect_takes_screen_ratio() {
        // 20px per beat, 5px per key: (2 beats, 8 keys) is 40px on screen both ways, aspect 2 -> width 80px = 4 beats
        let got = keep_aspect_xy([0.0, 0.0], [2.0, 8.0], 2.0, 20.0, 5.0, 128);
        assert!((got[0] - 4.0).abs() < 1e-12);
        assert!((got[1] - 8.0).abs() < 1e-12);
    }

    #[test]
    fn keep_aspect_clamps_inside_roll() {
        let got = keep_aspect_xy([0.0, 0.0], [-3.0, 2.0], 1.0, 10.0, 10.0, 128);
        assert_eq!(got, [0.0, 3.0]);
        // The tall dimension is 800px: width is stretched to 800px = 80 beats, pitch clamped to 127
        let got = keep_aspect_xy([0.0, 120.0], [5.0, 200.0], 1.0, 10.0, 10.0, 128);
        assert_eq!(got, [80.0, 127.0]);
    }

    #[test]
    fn builtin_templates_are_known() {
        let dir = std::path::Path::new("/nonexistent-spiderweb-shapes");
        assert!(builtin_template(dir, "Circle").is_some());
        assert!(builtin_template(dir, "Square").is_some());
        assert!(builtin_template(dir, "Triangle").is_some());
        assert!(builtin_template(dir, "Spider").is_none());
        assert!(builtin_shape("Circle").is_some());
        assert!(builtin_shape("Spider").is_none());
    }

    #[test]
    fn inside_strokes_even_odd() {
        let square = vec![SQUARE.to_vec()];
        assert!(inside_strokes(&square, 0.5, 0.5));
        assert!(!inside_strokes(&square, 1.5, 0.5));
        // A hole: one outer box + one reversed inner box; the middle does not count as inside
        let hole = vec![
            SQUARE.to_vec(),
            vec![
                [0.25, 0.25],
                [0.25, 0.75],
                [0.75, 0.75],
                [0.75, 0.25],
                [0.25, 0.25],
            ],
        ];
        assert!(!inside_strokes(&hole, 0.5, 0.5));
        assert!(inside_strokes(&hole, 0.1, 0.5));
    }
}
