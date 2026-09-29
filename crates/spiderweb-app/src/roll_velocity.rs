//! Velocity panel (upstream window/velocity.py): the velocity bars below the roll, one
//! vertical bar per note at its start, with a cap when the note is long enough.
//!
//! Dragging (Linear / Curve / Pencil) on the panel edits the velocity envelope; with shapes
//! selected only their notes change (others fade), with nothing selected every note the drag
//! covers changes. It shares the x axis with the roll (t / sx / kb_w of `App::view`), while
//! scrolling and zooming are independent.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke};

use spiderweb_core::Pt;
use spiderweb_core::engine;
use spiderweb_core::envelope::{env_at, paint_env, tidy_env, velocity_env};
use spiderweb_core::round_half_even;
use spiderweb_core::shape::Shape;

use crate::app::App;
use crate::roll;

/// Left-hand scale (upstream LEVELS).
const LEVELS: [i64; 5] = [127, 96, 64, 32, 0];
/// Number of points sampled along a curve (upstream CURVE_STEPS).
const CURVE_STEPS: usize = 48;
/// Red of the drawn line.
const RED: Color32 = Color32::from_rgb(0xd0, 0x00, 0x00);

/// Fixed layer indices: faded slots, normal slots, selected shapes, the shape being drawn (upstream NORMAL / SELECTED / DRAFT).
const NORMAL: usize = roll::SLOT_COLORS.len();
const SELECTED: usize = 2 * roll::SLOT_COLORS.len();
const DRAFT: usize = 2 * roll::SLOT_COLORS.len() + 1;

/// Velocity panel tools (upstream app.vel_tool: line / curve / pencil).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum VelTool {
    #[default]
    Line,
    Curve,
    Pencil,
}

/// Kind of drag: drawing a new line / curve / pencil stroke, or dragging a handle of the drawn line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EditKind {
    Drag(VelTool),
    Handle(HandleKind),
}

/// Handles of a line / curve: middle (bend) and the two ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HandleKind {
    Mid,
    A,
    B,
}

/// A finished line / curve: the state while its handles can still change it (upstream self.curve).
#[derive(Clone, Debug)]
struct LiveCurve {
    a: Pt,
    b: Pt,
    c: Pt,
    kind: VelTool,
    /// Result written into each shape; used to tell whether a shape was changed elsewhere
    done: Vec<DoneShape>,
}

/// The result of writing the curve into one shape (upstream cv["done"][i] = (sh, env, base, span)).
#[derive(Clone, Debug, PartialEq)]
struct DoneShape {
    i: usize,
    /// The envelope written back
    env: Vec<Pt>,
    /// The shape's envelope before drawing (bending again restarts from here)
    base: Vec<Pt>,
    /// The shape's time span (beats)
    span: (f64, f64),
}

/// One drag (upstream self.edit).
#[derive(Clone, Debug)]
struct Edit {
    kind: EditKind,
    start: Pt,
    last: Pt,
    /// The drawn (beat, velocity) polyline
    drawn: Option<Vec<Pt>>,
    /// Preview velocity per rendered note, -1 = not covered by the drawing (upstream preview)
    preview: Option<Vec<i64>>,
    /// Shapes this drag will write to
    owners: BTreeSet<usize>,
    /// Pencil's red mouse trail (panel-local coordinates)
    trail: Vec<Pos2>,
    /// Snap points: the starts of the first / last note of the selected shapes (beats)
    ends: Vec<f64>,
    /// The line / curve currently shown
    curve: Option<(Pt, Pt, Pt)>,
    /// While dragging a handle, only these shapes change (the only of upstream mark)
    only: Option<BTreeSet<usize>>,
    /// State while dragging a handle
    handle: Option<HandleDrag>,
}

#[derive(Clone, Debug)]
struct HandleDrag {
    kind: VelTool,
    done: Vec<DoneShape>,
}

/// Panel state (upstream VelocityPane's self.edit / self.curve / self._pan + app.vel_tool).
#[derive(Clone, Debug, Default)]
pub struct VelocityState {
    tool: VelTool,
    edit: Option<Edit>,
    curve: Option<LiveCurve>,
    /// Middle-button pan: x and view.t at press time (upstream self._pan)
    pan: Option<(f32, f64)>,
}

impl VelocityState {
    /// Enter = finish the last line / curve: the handles disappear. Returns true when there was one (upstream confirm).
    pub fn confirm(&mut self) -> bool {
        self.curve.take().is_some()
    }
}

// ---------------------------------------------------------------- pure logic (unit-testable)

/// Straight drag from a to b (beat, velocity), extending pad beats past both ends (upstream segment).
fn segment(a: Pt, b: Pt, pad: f64) -> Vec<Pt> {
    let (a, b) = if a[0] > b[0] { (b, a) } else { (a, b) };
    if a[0] == b[0] {
        return vec![[a[0] - pad, b[1]], [a[0] + pad, b[1]]];
    }
    vec![
        [a[0] - pad, a[1]],
        [a[0], a[1]],
        [b[0], b[1]],
        [b[0] + pad, b[1]],
    ]
}

/// Curve from a to b bending toward c (beat, velocity), extending pad beats past both ends (upstream curve_env).
fn curve_env(a: Pt, b: Pt, c: Pt, pad: f64) -> Vec<Pt> {
    let (a, b) = if a[0] > b[0] { (b, a) } else { (a, b) };
    if a[0] == b[0] {
        return segment(a, b, pad);
    }
    let cx = c[0].clamp(a[0], b[0]);
    let mut out = Vec::with_capacity(CURVE_STEPS + 3);
    out.push([a[0] - pad, a[1]]);
    for i in 0..=CURVE_STEPS {
        let t = i as f64 / CURVE_STEPS as f64;
        let s = 1.0 - t;
        out.push([
            s * s * a[0] + 2.0 * s * t * cx + t * t * b[0],
            (s * s * a[1] + 2.0 * s * t * c[1] + t * t * b[1]).clamp(1.0, 127.0),
        ]);
    }
    out.push([b[0] + pad, b[1]]);
    out
}

/// Clamps c's velocity just enough that the a → b curve stays within 1..127 (shape unchanged, no flat top) (upstream limit_bend).
fn limit_bend(a: Pt, b: Pt, c: Pt) -> Pt {
    let hi = 127.0 + ((127.0 - a[1]) * (127.0 - b[1])).sqrt();
    let lo = 1.0 - ((a[1] - 1.0) * (b[1] - 1.0)).sqrt();
    [c[0], c[1].clamp(lo, hi)]
}

/// Curve midpoint (where the bend handle is drawn) (upstream curve_mid).
fn curve_mid(a: Pt, b: Pt, c: Pt) -> Pt {
    [
        (a[0] + 2.0 * c[0] + b[0]) / 4.0,
        (a[1] + 2.0 * c[1] + b[1]) / 4.0,
    ]
}

/// The shape's velocity span: min / max beat over all stroke points.
fn shape_span(sh: &Shape) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for stroke in engine::shape_strokes(sh) {
        for p in stroke {
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
        }
    }
    if lo > hi { (0.0, 0.0) } else { (lo, hi) }
}

/// Writes the drawn (beat, velocity) polyline into the shape's envelope (the per-shape part of
/// upstream commit) and returns (env, span).
///
/// `base`: the starting envelope — while dragging a handle, the envelope from before the curve
/// was drawn (start over), otherwise the shape's current envelope.
fn apply_env(sh: &mut Shape, drawn: &[Pt], base: &[Pt]) -> (Vec<Pt>, (f64, f64)) {
    let (lo, hi) = shape_span(sh);
    let env = if hi > lo {
        let pts: Vec<Pt> = drawn
            .iter()
            .map(|p| [(p[0] - lo) / (hi - lo), p[1]])
            .collect();
        tidy_env(&paint_env(base, &pts))
    } else {
        // Every note is at the shape's start: the whole envelope is one point
        vec![[
            0.0,
            round_half_even(env_at(drawn, lo, false)).clamp(1.0, 127.0),
        ]]
    };
    sh.vel_env = env.clone();
    sh.own_vel = false; // the pasted notes' own velocities are replaced
    sh.vel0 = round_half_even(env_at(&env, 0.0, false)).clamp(1.0, 127.0);
    sh.vel1 = round_half_even(env_at(&env, 1.0, false)).clamp(1.0, 127.0);
    (env, (lo, hi))
}

// ---------------------------------------------------------------- panel geometry

/// The panel's screen geometry (velocity axis and x axis).
#[derive(Clone, Copy)]
struct Pane {
    rect: Rect,
    /// Gap above 127 (upstream self.top)
    top: f32,
    h: f32,
    /// Position of velocity 0: the same gap is left above the bottom edge
    bottom: f32,
    kb: f32,
    w: f32,
}

impl Pane {
    fn new(rect: Rect, scale: f32, kb: f32) -> Self {
        let top = 7.0 * scale;
        let h = rect.height();
        Self {
            rect,
            top,
            h,
            bottom: h - 1.0 - top,
            kb,
            w: rect.width(),
        }
    }

    /// Width of the image area (the part overlapping the roll's x axis).
    fn image_w(self) -> f32 {
        self.w - self.kb
    }

    /// Panel-local coordinates -> screen coordinates.
    fn pos(self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.rect.min.x + x, self.rect.min.y + y)
    }

    /// Screen coordinates -> panel-local coordinates.
    fn local(self, p: Pos2) -> Pos2 {
        p - self.rect.min.to_vec2()
    }

    /// Velocity -> y (upstream v2y).
    fn v2y(self, v: f64) -> f32 {
        (self.bottom as f64 - v / 127.0 * (self.bottom - self.top) as f64).round() as f32
    }

    /// y -> velocity (clamped to 1..127) (upstream y2v).
    fn y2v(self, y: f32) -> f64 {
        let span = (self.bottom - self.top).max(1.0) as f64;
        ((self.bottom - y) as f64 / span * 127.0).clamp(1.0, 127.0)
    }

    /// Mouse position for the pencil's red trail: y clamped into the velocity range (upstream trail_pt).
    fn trail_pt(self, local: Pos2) -> Pos2 {
        Pos2::new(local.x, local.y.clamp(self.v2y(127.0), self.v2y(1.0)))
    }
}

/// Snapshot of mouse and keyboard input.
struct Inputs {
    pos: Option<Pos2>,
    interact: Option<Pos2>,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    secondary_pressed: bool,
    middle_pressed: bool,
    middle_released: bool,
    middle_down: bool,
    ctrl: bool,
    shift: bool,
    scroll_y: f32,
}

fn read_input(ui: &egui::Ui) -> Inputs {
    ui.input(|i| Inputs {
        pos: i.pointer.hover_pos(),
        interact: i.pointer.interact_pos(),
        primary_pressed: i.pointer.primary_pressed(),
        primary_released: i.pointer.primary_released(),
        primary_down: i.pointer.primary_down(),
        secondary_pressed: i.pointer.secondary_pressed(),
        middle_pressed: i.pointer.button_pressed(egui::PointerButton::Middle),
        middle_released: i.pointer.button_released(egui::PointerButton::Middle),
        middle_down: i.pointer.button_down(egui::PointerButton::Middle),
        ctrl: i.modifiers.command || i.modifiers.ctrl,
        shift: i.modifiers.shift,
        scroll_y: i.smooth_scroll_delta.y,
    })
}

/// Panel entry point: toolbar, keyboard, input, painting.
pub fn velocity_ui(app: &mut App, ui: &mut egui::Ui) {
    velocity_toolbar(app, ui);
    // A popup (context menu / dropdown) is open: the panel's input steps aside
    let popup_open = crate::roll_menu::is_popup_open(ui.ctx());
    // Enter = finish the last line / curve (upstream pianoroll.on_key -> vel.confirm)
    if !popup_open
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        app.vel.confirm();
    }
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    let pane = Pane::new(rect, app.scale(), app.view.kb_w);
    if !app.view.ready
        || rect.width() < 20.0
        || pane.image_w() < 2.0
        || pane.h < 2.0 * pane.top + 4.0
    {
        return;
    }
    let _ = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    if !popup_open {
        let input = read_input(ui);
        handle_input(app, &input, pane);
        set_cursor(app, &input, pane, ui.ctx());
    }
    paint(app, &painter, pane);
}

/// Small toolbar at the top (upstream vbar).
fn velocity_toolbar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(rust_i18n::t!("velocity.title"));
        for (tool, label) in [
            (VelTool::Line, rust_i18n::t!("velocity.linear")),
            (VelTool::Curve, rust_i18n::t!("velocity.curve")),
            (VelTool::Pencil, rust_i18n::t!("velocity.pencil")),
        ] {
            if ui.selectable_label(app.vel.tool == tool, label).clicked() {
                app.vel.tool = tool;
            }
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(rust_i18n::t!("velocity.hint"))
                .weak()
                .size(10.0),
        );
    });
}

/// Hand / crosshair cursor while hovering (upstream on_motion).
fn set_cursor(app: &App, input: &Inputs, pane: Pane, ctx: &egui::Context) {
    let Some(p) = input.pos else {
        return;
    };
    if !pane.rect.contains(p) {
        return;
    }
    let local = pane.local(p);
    if local.x < pane.kb {
        return;
    }
    let over = live_curve(app)
        .and_then(|cv| near_handle(app, &cv, local, pane))
        .is_some();
    ctx.set_cursor_icon(if over {
        egui::CursorIcon::Grab
    } else {
        egui::CursorIcon::Crosshair
    });
}

// ---------------------------------------------------------------- coordinates and snapping

/// Screen position -> (beat, velocity) (upstream event_pt).
fn event_pt(app: &App, pane: Pane, local: Pos2) -> Pt {
    [app.view.b_of(local.x), pane.y2v(local.y)]
}

/// Snap points for drag ends: the starts of the first / last note of the selected shapes (upstream snap_spots).
fn snap_spots(app: &App) -> Vec<f64> {
    if app.sels.is_empty() {
        return Vec::new();
    }
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    for n in &app.rendered {
        if app.sels.contains(&(n[5] as usize)) {
            lo = lo.min(n[0]);
            hi = hi.max(n[0]);
        }
    }
    if lo > hi {
        return Vec::new();
    }
    let ppq = app.ppq.max(1) as f64;
    let mut out = vec![lo as f64 / ppq, hi as f64 / ppq];
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    out.dedup();
    out
}

/// Snaps b: with Shift held, to the selected ends first (within 10 * scale on screen), otherwise to the grid (upstream snap_time).
fn snap_time(app: &App, b: f64, shift: bool, ends: &[f64], scale: f32) -> f64 {
    if !shift {
        return b;
    }
    let mut best: Option<f64> = None;
    for &t in ends {
        if (app.view.x_of(t) - app.view.x_of(b)).abs() <= 10.0 * scale
            && best
                .map(|bb| (t - b).abs() < (bb - b).abs())
                .unwrap_or(true)
        {
            best = Some(t);
        }
    }
    if let Some(t) = best {
        return t;
    }
    match app.snap_beats() {
        Some(sb) if sb > 0.0 => ((b / sb).round() * sb).max(0.0),
        _ => b,
    }
}

// ---------------------------------------------------------------- editing

/// The last line / curve, when its shapes were not changed elsewhere (handles still work) (the check of upstream live_curve).
fn curve_still_live(app: &App, cv: &LiveCurve) -> bool {
    for d in &cv.done {
        let Some(sh) = app.shapes.get(d.i) else {
            return false;
        };
        if sh.vel_env != d.env || shape_span(sh) != d.span {
            return false;
        }
    }
    true
}

/// The last line / curve that still works (changing the tool or a shape disqualifies it).
fn live_curve(app: &App) -> Option<LiveCurve> {
    app.vel
        .curve
        .as_ref()
        .filter(|cv| app.vel.tool == cv.kind && curve_still_live(app, cv))
        .cloned()
}

/// Which handle of the curve the mouse is on (upstream near_handle).
fn near_handle(app: &App, cv: &LiveCurve, local: Pos2, pane: Pane) -> Option<HandleKind> {
    let near = 7.0 * app.scale();
    let mid = curve_mid(cv.a, cv.b, cv.c);
    for (which, p) in [
        (HandleKind::Mid, mid),
        (HandleKind::A, cv.a),
        (HandleKind::B, cv.b),
    ] {
        if which == HandleKind::Mid && cv.kind == VelTool::Line {
            continue; // a line stays straight
        }
        if (app.view.x_of(p[0]) - local.x).abs() <= near && (pane.v2y(p[1]) - local.y).abs() <= near
        {
            return Some(which);
        }
    }
    None
}

fn on_press(app: &mut App, local: Pos2, pane: Pane, input: &Inputs) {
    if local.x < pane.kb {
        return;
    }
    app.cancel_draft();
    let pt = event_pt(app, pane, local);
    let ends = snap_spots(app);
    let scale = app.scale();
    let live = live_curve(app);
    if live.is_none() {
        app.vel.curve = None;
    }
    if let Some(cv) = live
        && let Some(which) = near_handle(app, &cv, local, pane)
    {
        let only: BTreeSet<usize> = cv.done.iter().map(|d| d.i).collect();
        app.vel.edit = Some(Edit {
            kind: EditKind::Handle(which),
            start: pt,
            last: pt,
            drawn: None,
            preview: None,
            owners: BTreeSet::new(),
            trail: Vec::new(),
            ends,
            curve: Some((cv.a, cv.b, cv.c)),
            only: Some(only),
            handle: Some(HandleDrag {
                kind: cv.kind,
                done: cv.done,
            }),
        });
        return;
    }
    app.vel.curve = None;
    let tool = app.vel.tool;
    let pt = if tool == VelTool::Pencil {
        pt
    } else {
        [snap_time(app, pt[0], input.shift, &ends, scale), pt[1]]
    };
    let trail = pane.trail_pt(local);
    app.vel.edit = Some(Edit {
        kind: EditKind::Drag(tool),
        start: pt,
        last: pt,
        drawn: None,
        preview: None,
        owners: BTreeSet::new(),
        trail: vec![trail],
        ends,
        curve: None,
        only: None,
        handle: None,
    });
    if tool == VelTool::Pencil {
        extend(app, pt);
    } else {
        draw_curve(app, pt, pt, pt, None);
    }
}

fn on_drag(app: &mut App, local: Pos2, pane: Pane, input: &Inputs) {
    if let Some(text) = position_text(app, local, pane) {
        app.position = Some(text);
    }
    let Some(kind) = app.vel.edit.as_ref().map(|e| e.kind) else {
        return;
    };
    let scale = app.scale();
    let pt = event_pt(app, pane, local);
    match kind {
        EditKind::Handle(which) => {
            let Some((a0, b0, c0)) = app.vel.edit.as_ref().and_then(|e| e.curve) else {
                return;
            };
            let (mut a, mut b, mut c) = (a0, b0, c0);
            if which == HandleKind::Mid {
                let (lo, hi) = (a0[0].min(b0[0]), a0[0].max(b0[0]));
                // The bend handle stays between the two ends
                let mx = pt[0].clamp((3.0 * lo + hi) / 4.0, (lo + 3.0 * hi) / 4.0);
                c = [
                    2.0 * mx - (a0[0] + b0[0]) / 2.0,
                    2.0 * pt[1] - (a0[1] + b0[1]) / 2.0,
                ];
            } else {
                let other = if which == HandleKind::A { b0 } else { a0 };
                let ends = app
                    .vel
                    .edit
                    .as_ref()
                    .map(|e| e.ends.clone())
                    .unwrap_or_default();
                let t = snap_time(app, pt[0], input.shift, &ends, scale);
                let v = if input.ctrl { other[1] } else { pt[1] };
                if (t - other[0]).abs() < 1e-9 {
                    return;
                }
                if which == HandleKind::A {
                    a = [t, v];
                } else {
                    b = [t, v];
                }
                let hk = app
                    .vel
                    .edit
                    .as_ref()
                    .and_then(|e| e.handle.as_ref())
                    .map(|hd| hd.kind)
                    .unwrap_or(VelTool::Line);
                if hk == VelTool::Line {
                    c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                }
            }
            let only = app.vel.edit.as_ref().and_then(|e| e.only.clone());
            draw_curve(app, a, b, c, only.as_ref());
        }
        EditKind::Drag(VelTool::Pencil) => {
            let trail = pane.trail_pt(local);
            if let Some(ed) = app.vel.edit.as_mut() {
                ed.trail.push(trail);
            }
            extend(app, pt);
        }
        EditKind::Drag(kind) => {
            let Some(a) = app.vel.edit.as_ref().map(|e| e.start) else {
                return;
            };
            let ends = app
                .vel
                .edit
                .as_ref()
                .map(|e| e.ends.clone())
                .unwrap_or_default();
            let t = snap_time(app, pt[0], input.shift, &ends, scale);
            let mut v = pt[1];
            let c = if kind == VelTool::Curve {
                // Starts flat, like an ease-in
                [(a[0] + t) / 2.0, a[1]]
            } else {
                if input.ctrl {
                    v = a[1]; // completely level
                }
                [(a[0] + t) / 2.0, (a[1] + v) / 2.0]
            };
            draw_curve(app, a, [t, v], c, None);
        }
    }
}

fn on_release(app: &mut App) {
    let Some(ed) = app.vel.edit.take() else {
        return;
    };
    let Some(drawn) = ed.drawn else {
        return;
    };
    if let Some(handle) = ed.handle {
        // Handle drag: write back only while the curve is still live; the undo step still counts as the original curve (upstream on_release)
        let live = app
            .vel
            .curve
            .as_ref()
            .map(|cv| cv.kind == handle.kind && cv.done == handle.done)
            .unwrap_or(false);
        if live {
            if let Some(cv) = app.vel.curve.as_mut()
                && let Some((a, b, c)) = ed.curve
            {
                cv.a = a;
                cv.b = b;
                cv.c = c;
            }
            let owners: BTreeSet<usize> = handle.done.iter().map(|d| d.i).collect();
            let done = commit(app, &drawn, &owners, Some(&handle.done));
            if let Some(cv) = app.vel.curve.as_mut() {
                cv.done = done;
            }
        }
        return;
    }
    if ed.owners.is_empty() {
        return;
    }
    app.push_undo(&rust_i18n::t!("velocity.draw_velocity"));
    let kind = match ed.kind {
        EditKind::Drag(t) => t,
        EditKind::Handle(_) => app.vel.tool,
    };
    let done = commit(app, &drawn, &ed.owners, None);
    if let Some((a, b, c)) = ed.curve
        && a[0] != b[0]
    {
        // The handles keep working until something changes elsewhere
        app.vel.curve = Some(LiveCurve {
            a,
            b,
            c,
            kind,
            done,
        });
    }
}

/// Pencil: splices the run from last to pt into the drawn polyline (later strokes cover earlier ones) (upstream extend).
fn extend(app: &mut App, pt: Pt) {
    let pad = 0.5 / app.view.sx.max(1e-9);
    let Some((last, old)) = app.vel.edit.as_ref().map(|e| (e.last, e.drawn.clone())) else {
        return;
    };
    let seg = segment(last, pt, pad);
    let drawn = match old {
        None => seg.clone(),
        Some(d) => paint_env(&d, &seg),
    };
    if let Some(ed) = app.vel.edit.as_mut() {
        ed.drawn = Some(drawn);
        ed.last = pt;
    }
    mark(app, &seg, None);
}

/// Makes this drag a curve from a to b bending toward c (upstream draw_curve).
fn draw_curve(app: &mut App, a: Pt, b: Pt, c: Pt, only: Option<&BTreeSet<usize>>) {
    let pad = 0.5 / app.view.sx.max(1e-9);
    let c = limit_bend(a, b, c);
    let drawn = curve_env(a, b, c, pad);
    if let Some(ed) = app.vel.edit.as_mut() {
        ed.curve = Some((a, b, c));
        ed.drawn = Some(drawn.clone());
        ed.preview = None;
        ed.owners.clear();
    }
    mark(app, &drawn, only);
}

/// Records the velocity preview and shapes of the editable notes seg covers (later records cover earlier ones) (upstream mark).
fn mark(app: &mut App, seg: &[Pt], only: Option<&BTreeSet<usize>>) {
    if seg.is_empty() {
        return;
    }
    let ppq = app.ppq.max(1) as f64;
    let lo = seg[0][0] * ppq;
    let hi = seg[seg.len() - 1][0] * ppq;
    let rendered_len = app.rendered.len();
    let sels = app.sels.clone();
    // Borrow rendered and vel separately (different fields of the same struct)
    let rendered = &app.rendered;
    let Some(ed) = app.vel.edit.as_mut() else {
        return;
    };
    if ed.preview.is_none() {
        ed.preview = Some(vec![-1; rendered_len]);
    }
    let Some(preview) = ed.preview.as_mut() else {
        return;
    };
    for (k, n) in rendered.iter().enumerate() {
        let start = n[0] as f64;
        if start < lo || start > hi {
            continue;
        }
        let owner = n[5] as usize;
        let allowed = match only {
            Some(set) => set.contains(&owner),
            None => sels.is_empty() || sels.contains(&owner),
        };
        if !allowed {
            continue;
        }
        let v = round_half_even(env_at(seg, start / ppq, false)).clamp(1.0, 127.0);
        preview[k] = v as i64;
        ed.owners.insert(owner);
    }
}

/// Writes the drawn velocities into each owner shape (upstream commit). Returns the per-shape state.
fn commit(
    app: &mut App,
    drawn: &[Pt],
    owners: &BTreeSet<usize>,
    bases: Option<&[DoneShape]>,
) -> Vec<DoneShape> {
    let mut done = Vec::new();
    for &i in owners {
        let Some(sh) = app.shapes.get_mut(i) else {
            continue;
        };
        let base = match bases.and_then(|b| b.iter().find(|d| d.i == i)) {
            Some(d) => d.base.clone(),
            None => velocity_env(sh),
        };
        let (env, span) = apply_env(sh, drawn, &base);
        done.push(DoneShape { i, env, base, span });
    }
    app.shapes_changed();
    app.panel_sel = None; // the side panel's vel0/vel1 refresh along with it
    done
}

// ---------------------------------------------------------------- input dispatch

fn handle_input(app: &mut App, input: &Inputs, pane: Pane) {
    let rect = pane.rect;
    if let Some(p) = input.pos
        && rect.contains(p)
    {
        let local = pane.local(p);
        if let Some(text) = position_text(app, local, pane) {
            app.position = Some(text);
        }
    }
    if input.primary_pressed
        && let Some(p) = input.pos
        && rect.contains(p)
    {
        on_press(app, pane.local(p), pane, input);
    }
    if input.primary_down
        && app.vel.edit.is_some()
        && let Some(p) = input.interact.or(input.pos)
    {
        on_drag(app, pane.local(p), pane, input);
    }
    if input.primary_released && app.vel.edit.is_some() {
        on_release(app);
    }
    if input.secondary_pressed
        && let Some(p) = input.pos
        && rect.contains(p)
    {
        // Right button: cancels the line / curve being drawn and deselects (upstream on_right)
        app.vel.edit = None;
        app.vel.curve = None;
        app.select(None, false);
    }
    if input.middle_pressed
        && let Some(p) = input.pos
        && rect.contains(p)
    {
        app.vel.pan = Some((pane.local(p).x, app.view.t));
    }
    if input.middle_down
        && let Some((start, t0)) = app.vel.pan
        && let Some(p) = input.interact.or(input.pos)
        && app.view.sx > 0.0
    {
        app.view.t = t0 - (pane.local(p).x - start) as f64 / app.view.sx;
        app.view.clamp();
    }
    if input.middle_released {
        app.vel.pan = None;
    }
    if input.scroll_y != 0.0
        && let Some(p) = input.pos
        && rect.contains(p)
    {
        on_wheel(app, pane.local(p), input);
    }
}

/// Wheel scrolls sideways, Ctrl+wheel zooms time (same as the roll, upstream on_wheel).
fn on_wheel(app: &mut App, local: Pos2, input: &Inputs) {
    let up = input.scroll_y > 0.0;
    if input.ctrl {
        let b = app.view.b_of(local.x);
        app.view.sx = (app.view.sx * if up { 1.25 } else { 0.8 }).clamp(0.05, 100000.0);
        app.view.t = b - (local.x - app.view.kb_w) as f64 / app.view.sx;
    } else {
        app.view.t += if up { -120.0 } else { 120.0 } / app.view.sx;
    }
    app.view.clamp();
}

/// Status-bar position text: "bar:beat:tick (tick N)     velocity V" (upstream on_motion / time_text).
fn position_text(app: &App, local: Pos2, pane: Pane) -> Option<String> {
    if local.x < pane.kb {
        return None;
    }
    Some(format!(
        "{}     velocity {}",
        time_text(app, local.x),
        round_half_even(pane.y2v(local.y)) as i64
    ))
}

/// bar:beat:tick at canvas x (sharing the x axis with the roll).
fn time_text(app: &App, x: f32) -> String {
    let ppq = app.ppq.max(1) as f64;
    let beats = app.beats.max(1) as f64;
    let ticks = (app.view.b_of(x) * ppq).max(0.0);
    let bar = (ticks / (ppq * beats)).floor() + 1.0;
    let rest = ticks % (ppq * beats);
    format!(
        "{}:{}:{:03}  (tick {})",
        bar as i64,
        (rest / ppq).floor() as i64 + 1,
        (rest % ppq) as i64,
        ticks as i64
    )
}

// ---------------------------------------------------------------- painting

/// Per-layer (fill color, border color), from low to high (upstream LAYERS).
fn layer_colors() -> Vec<(Color32, Color32)> {
    let mut out = Vec::with_capacity(2 * roll::SLOT_COLORS.len() + 2);
    for (f, b) in roll::SLOT_COLORS {
        out.push((roll::fade(f, 0.72), roll::fade(b, 0.55)));
    }
    out.extend(roll::SLOT_COLORS);
    out.push(roll::SELECTED_COLOR);
    out.push(roll::DRAFT_COLOR);
    out
}

/// A velocity bar: one column at the note's start up to its velocity, with a cap across the velocity when the note is long enough (upstream bars).
struct Bar {
    layer: usize,
    /// Image-area local x (kb_w already subtracted)
    x0: f32,
    x1: f32,
    /// y corresponding to the velocity
    y: f32,
}

fn collect_bars(app: &App, state: &VelocityState, pane: Pane) -> Vec<Bar> {
    let ppq = app.ppq.max(1) as f64;
    let v = &app.view;
    let iw = pane.image_w();
    let preview = state.edit.as_ref().and_then(|e| e.preview.as_ref());
    let mut out = Vec::new();
    let push = |note: &[i64], layer: usize, vel: f64, out: &mut Vec<Bar>| {
        let x0 = v.x_of(note[0] as f64 / ppq) - pane.kb;
        let x1 = v.x_of(note[1] as f64 / ppq) - pane.kb;
        if x1 < 0.0 || x0 >= iw {
            return;
        }
        out.push(Bar {
            layer,
            x0,
            x1,
            y: pane.v2y(vel.clamp(0.0, 127.0)),
        });
    };
    for (k, n) in app.rendered.iter().enumerate() {
        let vel = preview
            .and_then(|p| p.get(k))
            .copied()
            .filter(|&p| p >= 0)
            .unwrap_or(n[3]) as f64;
        let slot = n[4].unsigned_abs() as usize % roll::SLOT_COLORS.len();
        let layer = if app.sels.is_empty() {
            NORMAL + slot
        } else if app.sels.contains(&(n[5] as usize)) {
            SELECTED
        } else {
            slot // unselected ones fade
        };
        push(n, layer, vel, &mut out);
    }
    if let Some(d) = &app.draft {
        for n in engine::shape_notes(d, ppq, app.keys) {
            push(&n, DRAFT, n[3] as f64, &mut out);
        }
    }
    out
}

fn paint_bars(app: &App, painter: &egui::Painter, pane: Pane) {
    let iw = pane.image_w();
    if iw <= 0.0 {
        return;
    }
    let layers = layer_colors();
    let state = &app.vel;
    let mut bars = collect_bars(app, state, pane);
    // Lower layers first; in the same column and layer, higher velocity covers lower
    bars.sort_by(|p, q| {
        p.layer
            .cmp(&q.layer)
            .then(q.y.partial_cmp(&p.y).unwrap_or(Ordering::Equal))
    });
    for b in bars {
        let (fill, border) = layers[b.layer];
        let x0 = b.x0.round();
        let x1 = b.x1.round();
        if x0 >= 0.0 && x0 < iw {
            let sx = pane.kb + x0;
            painter.line_segment(
                [pane.pos(sx, b.y), pane.pos(sx, pane.bottom)],
                Stroke::new(1.0, fill),
            );
            painter.line_segment(
                [pane.pos(sx, b.y), pane.pos(sx + 1.0, b.y)],
                Stroke::new(1.0, border),
            );
        }
        if x1 - x0 >= 2.0 {
            let cx0 = x0.max(0.0);
            let cx1 = x1.min(iw - 1.0);
            if cx1 >= cx0 {
                painter.line_segment(
                    [
                        pane.pos(pane.kb + cx0, b.y),
                        pane.pos(pane.kb + cx1 + 1.0, b.y),
                    ],
                    Stroke::new(1.0, border),
                );
            }
        }
    }
}

/// The roll's vertical grid lines (simplified grid_cols): snap lines, beat lines, bar lines.
fn grid_columns(app: &App, w: f32) -> Vec<(f32, Color32)> {
    let v = &app.view;
    let mut cols = Vec::new();
    let b_lo = v.b_of(v.kb_w);
    let b_hi = v.b_of(w);
    if let Some(sb) = app.snap_beats()
        && sb > 0.0
        && sb < 1.0
        && sb * v.sx >= 8.0
    {
        let mut b = (b_lo / sb).ceil() * sb;
        while b <= b_hi {
            if (b - b.round()).abs() > 1e-9 {
                cols.push((v.x_of(b).round(), Color32::from_rgb(0xee, 0xf1, 0xf6)));
            }
            b += sb;
        }
    }
    if v.sx >= 5.0 {
        let mut b = b_lo.max(0.0).ceil();
        while b <= b_hi {
            if (b as i64) % app.beats.max(1) != 0 {
                cols.push((v.x_of(b).round(), Color32::from_rgb(0xbc, 0xc4, 0xd2)));
            }
            b += 1.0;
        }
    }
    let mut step = (app.beats.max(1)) as f64;
    while step * v.sx < 6.0 {
        step *= 2.0;
    }
    let mut b = (b_lo / step).floor() * step;
    while b <= b_hi {
        if b >= 0.0 {
            cols.push((v.x_of(b).round(), Color32::from_rgb(0x3a, 0x3a, 0x3a)));
        }
        b += step;
    }
    cols
}

/// Draws a line / curve: red line + square handles at both ends + a round middle handle (when bent) (upstream draw_curve_line).
fn draw_curve_line(
    app: &App,
    painter: &egui::Painter,
    pane: Pane,
    curve: (Pt, Pt, Pt),
    lw: f32,
    bend: bool,
) {
    let (a, b, c) = curve;
    let pts = curve_env(a, b, c, 0.0);
    if pts.len() < 4 {
        return;
    }
    let inner = &pts[1..pts.len() - 1];
    let screen: Vec<Pos2> = inner
        .iter()
        .map(|p| pane.pos(app.view.x_of(p[0]), pane.v2y(p[1])))
        .collect();
    painter.add(egui::Shape::line(screen, Stroke::new(lw, RED)));
    if a[0] != b[0] {
        let r = 4.0 * app.scale();
        if bend {
            let m = curve_mid(a, b, c);
            let center = pane.pos(app.view.x_of(m[0]), pane.v2y(m[1]));
            painter.circle_filled(center, r, Color32::WHITE);
            painter.circle_stroke(center, r, Stroke::new(lw, RED));
        }
        for p in [a, b] {
            let center = pane.pos(app.view.x_of(p[0]), pane.v2y(p[1]));
            let q = Rect::from_center_size(center, egui::Vec2::splat(r * 2.0));
            painter.rect_filled(q, 0.0, Color32::WHITE);
            painter.rect_stroke(q, 0.0, Stroke::new(lw, RED), egui::StrokeKind::Inside);
        }
    }
}

fn paint(app: &App, painter: &egui::Painter, pane: Pane) {
    painter.rect_filled(pane.rect, 0.0, Color32::WHITE);

    // Velocity level lines: 96 / 64 / 32 faint, 127 / 0 dark
    let level_color = |v: i64| {
        if v == 127 || v == 0 {
            Color32::from_rgb(0x9f, 0xb2, 0xcf)
        } else {
            Color32::from_rgb(0xd3, 0xdf, 0xf0)
        }
    };
    for v in LEVELS {
        let y = pane.v2y(v as f64);
        painter.line_segment(
            [pane.pos(pane.kb, y), pane.pos(pane.w, y)],
            Stroke::new(1.0, level_color(v)),
        );
    }
    painter.line_segment(
        [
            pane.pos(pane.kb, pane.h - 1.0),
            pane.pos(pane.w, pane.h - 1.0),
        ],
        Stroke::new(1.0, Color32::from_rgb(0x80, 0x80, 0x80)),
    );

    // vertical grid
    for (x, color) in grid_columns(app, pane.w) {
        if x >= pane.kb - 1.0 && x <= pane.w {
            painter.line_segment(
                [pane.pos(x, pane.top), pane.pos(x, pane.h)],
                Stroke::new(1.0, color),
            );
        }
    }

    // velocity bars
    paint_bars(app, painter, pane);

    // left-hand velocity scale strip
    painter.rect_filled(
        Rect::from_min_max(pane.rect.min, pane.pos(pane.kb, pane.h)),
        0.0,
        Color32::from_rgb(0xf0, 0xf0, 0xf0),
    );
    painter.line_segment(
        [
            pane.pos(pane.kb - 1.0, 0.0),
            pane.pos(pane.kb - 1.0, pane.h),
        ],
        Stroke::new(1.0, Color32::from_rgb(0x80, 0x80, 0x80)),
    );
    for v in LEVELS {
        let y = pane.v2y(v as f64).clamp(6.0, pane.h - 6.0);
        painter.text(
            pane.pos(pane.kb - 5.0, y),
            Align2::RIGHT_CENTER,
            v.to_string(),
            FontId::proportional(8.0),
            Color32::from_rgb(0x33, 0x33, 0x33),
        );
    }

    // The line / curve / pencil trail being drawn, or the last line / curve that still works
    let lw = app.scale().max(1.0);
    let state = &app.vel;
    if let Some(ed) = &state.edit {
        if let Some(curve) = ed.curve {
            let bend = match ed.kind {
                EditKind::Handle(_) => ed
                    .handle
                    .as_ref()
                    .map(|h| h.kind == VelTool::Curve)
                    .unwrap_or(false),
                EditKind::Drag(t) => t == VelTool::Curve,
            };
            draw_curve_line(app, painter, pane, curve, lw, bend);
        } else if ed.trail.len() >= 2 {
            let pts: Vec<Pos2> = ed.trail.iter().map(|p| pane.pos(p.x, p.y)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(lw, RED)));
        }
    } else if let Some(cv) = live_curve(app) {
        draw_curve_line(
            app,
            painter,
            pane,
            (cv.a, cv.b, cv.c),
            lw,
            cv.kind == VelTool::Curve,
        );
    }

    // Play line (in sync with the roll)
    let x = app.view.x_of(app.playhead).round();
    if x >= pane.kb && x <= pane.w {
        painter.line_segment(
            [pane.pos(x, 0.0), pane.pos(x, pane.h)],
            Stroke::new(lw, Color32::from_rgb(0x0a, 0x50, 0xe0)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spiderweb_core::envelope::velocity_env;
    use spiderweb_core::shape::Kind;

    fn line_shape(a: Pt, b: Pt) -> Shape {
        Shape::new(Kind::Line, vec![a, b])
    }

    #[test]
    fn segment_pads_ends_and_orders_ends() {
        let s = segment([4.0, 90.0], [0.0, 10.0], 0.25);
        assert_eq!(s.first().copied(), Some([-0.25, 10.0]));
        assert_eq!(s.last().copied(), Some([4.25, 90.0]));
        assert!(s.windows(2).all(|w| w[0][0] <= w[1][0]));
        // Same beat: both points carry pad
        let z = segment([2.0, 20.0], [2.0, 20.0], 0.5);
        assert_eq!(z, vec![[1.5, 20.0], [2.5, 20.0]]);
    }

    #[test]
    fn curve_env_keeps_ends_and_clamps_velocity() {
        let pts = curve_env([0.0, 10.0], [4.0, 127.0], [2.0, 1000.0], 0.5);
        assert_eq!(pts.first().copied(), Some([-0.5, 10.0]));
        assert_eq!(pts.last().copied(), Some([4.5, 127.0]));
        assert!(pts.iter().all(|p| (1.0..=127.0).contains(&p[1])));
        assert!(pts.windows(2).all(|w| w[0][0] <= w[1][0]));
    }

    #[test]
    fn limit_bend_keeps_curve_in_range() {
        let a = [0.0, 127.0];
        let b = [4.0, 127.0];
        let c = limit_bend(a, b, [2.0, 500.0]);
        assert_eq!(c[1], 127.0);
        let pts = curve_env(a, b, c, 0.0);
        assert!(pts.iter().all(|p| (1.0..=127.0).contains(&p[1])));
        let c = limit_bend([0.0, 1.0], [4.0, 1.0], [2.0, -500.0]);
        assert_eq!(c[1], 1.0);
    }

    #[test]
    fn apply_env_maps_draw_to_shape_span() {
        let mut sh = line_shape([0.0, 60.0], [4.0, 60.0]);
        let base = velocity_env(&sh);
        let drawn = segment([0.0, 20.0], [4.0, 100.0], 0.1);
        let (env, span) = apply_env(&mut sh, &drawn, &base);
        assert_eq!(span, (0.0, 4.0));
        assert!((env_at(&env, 0.0, false) - 20.0).abs() < 1e-9);
        assert!((env_at(&env, 1.0, false) - 100.0).abs() < 1e-9);
        assert_eq!(sh.vel0, 20.0);
        assert_eq!(sh.vel1, 100.0);
        assert_eq!(sh.vel_env, env);
        assert!(!sh.own_vel);
    }

    #[test]
    fn apply_env_starts_over_from_base() {
        // Bending again starts from the envelope before drawing (base), not stacked on the drawn result
        let mut sh = line_shape([0.0, 127.0], [4.0, 127.0]);
        sh.vel_env = vec![[0.0, 10.0], [1.0, 10.0]];
        let base = velocity_env(&sh);
        let first = segment([0.0, 30.0], [4.0, 30.0], 0.1);
        apply_env(&mut sh, &first, &base);
        let second = segment([0.0, 90.0], [4.0, 90.0], 0.1);
        let (env, _) = apply_env(&mut sh, &second, &base);
        assert!((env_at(&env, 0.5, false) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn apply_env_zero_span_uses_value_at_shape_beat() {
        let mut sh = line_shape([2.0, 60.0], [2.0, 64.0]);
        let base = velocity_env(&sh);
        let drawn = segment([0.0, 10.0], [4.0, 90.0], 0.0);
        let (env, span) = apply_env(&mut sh, &drawn, &base);
        assert_eq!(span, (2.0, 2.0));
        assert_eq!(env.len(), 1);
        assert!((env[0][1] - 50.0).abs() < 1e-9);
        assert_eq!(sh.vel0, 50.0);
        assert_eq!(sh.vel1, 50.0);
    }

    #[test]
    fn pencil_stretches_later_stretches_win_and_stay_sorted() {
        let first = segment([0.0, 10.0], [4.0, 10.0], 0.05);
        let drawn = paint_env(&first, &segment([1.0, 90.0], [2.0, 90.0], 0.05));
        assert!((env_at(&drawn, 1.5, false) - 90.0).abs() < 1e-9);
        assert!((env_at(&drawn, 0.5, false) - 10.0).abs() < 1e-9);
        assert!(drawn.windows(2).all(|w| w[0][0] <= w[1][0]));
        // Drawing another run in the opposite direction: still sorted, later strokes win (the start at 1.5 already holds the new value)
        let drawn = paint_env(&drawn, &segment([3.0, 30.0], [1.5, 50.0], 0.05));
        assert!(drawn.windows(2).all(|w| w[0][0] <= w[1][0]));
        assert!((env_at(&drawn, 1.5, true) - 50.0).abs() < 1e-9);
        assert!((env_at(&drawn, 0.5, false) - 10.0).abs() < 1e-9);
    }
}
