//! Piano roll: view (zoom / scroll), painting and mouse interaction (upstream roll/*).

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::engine;
use spiderweb_core::funnel;
use spiderweb_core::joined;
use spiderweb_core::note::Note;
use spiderweb_core::shape::{Fill, Kind, Shape};

use crate::app::{App, PartId, Tool};
use crate::roll_curve::paint_curve_handles;
use crate::roll_funnel;

pub const BLACK: [i64; 5] = [1, 3, 6, 8, 10];
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
pub const PIANO_88_LO: i64 = 21;
pub const PIANO_88_HI: i64 = 109;

/// Color of each channel slot (upstream roll_shared.SLOT_COLORS).
pub const SLOT_COLORS: [(Color32, Color32); 15] = [
    (
        Color32::from_rgb(0x7e, 0xa6, 0xf5),
        Color32::from_rgb(0x1f, 0x3a, 0x93),
    ),
    (
        Color32::from_rgb(0xf5, 0x8e, 0x8e),
        Color32::from_rgb(0x8f, 0x1f, 0x1f),
    ),
    (
        Color32::from_rgb(0x8f, 0xd6, 0x8f),
        Color32::from_rgb(0x1f, 0x6f, 0x1f),
    ),
    (
        Color32::from_rgb(0xe6, 0xc6, 0x5c),
        Color32::from_rgb(0x7a, 0x5f, 0x00),
    ),
    (
        Color32::from_rgb(0xc7, 0x9b, 0xf2),
        Color32::from_rgb(0x5a, 0x2a, 0x8f),
    ),
    (
        Color32::from_rgb(0x6f, 0xd6, 0xd6),
        Color32::from_rgb(0x13, 0x6b, 0x6b),
    ),
    (
        Color32::from_rgb(0xf2, 0xa3, 0x6b),
        Color32::from_rgb(0x8a, 0x43, 0x10),
    ),
    (
        Color32::from_rgb(0xb0, 0xb0, 0xb0),
        Color32::from_rgb(0x40, 0x40, 0x40),
    ),
    (
        Color32::from_rgb(0xf5, 0x9a, 0xd0),
        Color32::from_rgb(0x8f, 0x1f, 0x63),
    ),
    (
        Color32::from_rgb(0xc2, 0xe3, 0x6b),
        Color32::from_rgb(0x4f, 0x6b, 0x0f),
    ),
    (
        Color32::from_rgb(0x9a, 0xa0, 0xf5),
        Color32::from_rgb(0x2a, 0x2f, 0x8f),
    ),
    (
        Color32::from_rgb(0xc9, 0xa2, 0x7e),
        Color32::from_rgb(0x5c, 0x3a, 0x1a),
    ),
    (
        Color32::from_rgb(0x7f, 0xd6, 0xb0),
        Color32::from_rgb(0x13, 0x5c, 0x3f),
    ),
    (
        Color32::from_rgb(0xe3, 0x8a, 0xe3),
        Color32::from_rgb(0x7a, 0x1f, 0x7a),
    ),
    (
        Color32::from_rgb(0x8f, 0xb8, 0xd6),
        Color32::from_rgb(0x1f, 0x4a, 0x6b),
    ),
];
pub const SELECTED_COLOR: (Color32, Color32) = (
    Color32::from_rgb(0xff, 0xb6, 0x5c),
    Color32::from_rgb(0x9a, 0x4b, 0x00),
);
#[allow(dead_code)] // preview of the shape being drawn yet to be wired up
pub const DRAFT_COLOR: (Color32, Color32) = (
    Color32::from_rgb(0x9b, 0xe3, 0x9b),
    Color32::from_rgb(0x1d, 0x6b, 0x1d),
);

/// Extra keys kept above / below the visible pitch range in the GPU cull rectangle, so small
/// vertical pans never re-upload (the tick margin is one viewport width, see [`View::cull_range`]).
pub const CULL_MARGIN_KEYS: i64 = 8;

/// Piano roll view state.
#[derive(Clone, Debug)]
pub struct View {
    pub t: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
    /// The project's key range (upstream app.keys): the view's upper limit and visible range follow it
    pub keys: i64,
    pub kb_w: f32,
    pub ruler_h: f32,
    pub w: f32,
    pub h: f32,
    pub ready: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            t: 0.0,
            top: 127.5,
            sx: 60.0,
            sy: 6.0,
            keys: spiderweb_core::paths::KEYS[0],
            kb_w: 56.0,
            ruler_h: 20.0,
            w: 0.0,
            h: 0.0,
            ready: false,
        }
    }
}

impl View {
    pub fn x_of(&self, b: f64) -> f32 {
        self.kb_w + ((b - self.t) * self.sx) as f32
    }

    pub fn b_of(&self, x: f32) -> f64 {
        (x - self.kb_w) as f64 / self.sx + self.t
    }

    pub fn y_of(&self, p: f64) -> f32 {
        self.ruler_h + ((self.top - p) * self.sy) as f32
    }

    pub fn p_of(&self, y: f32) -> f64 {
        self.top - (y - self.ruler_h) as f64 / self.sy
    }

    pub fn row_y(&self, p: f64) -> (f32, f32) {
        (self.y_of(p + 0.5).round(), self.y_of(p - 0.5).round())
    }

    pub fn visible_pitches(&self) -> (i64, i64) {
        let lo = (self.p_of(self.h) - 0.5).ceil() as i64;
        let hi = (self.p_of(self.ruler_h) + 0.5).floor() as i64;
        (lo.max(0), hi.min(self.keys - 1))
    }

    /// Visible rectangle used to cull GPU note instances, in ticks and keys. Expanded by one
    /// viewport width of ticks on each side and [`CULL_MARGIN_KEYS`] keys above / below, so
    /// panning inside the margin reuses the uploaded buffers.
    pub fn cull_range(&self, ppq: i64) -> crate::note_gpu::CullRange {
        let ppq = ppq.max(1) as f64;
        let b_lo = self.b_of(self.kb_w);
        let b_hi = self.b_of(self.w);
        let margin = (b_hi - b_lo).max(0.0);
        let (p_lo, p_hi) = self.visible_pitches();
        crate::note_gpu::CullRange {
            tick_lo: ticks_floor(b_lo - margin, ppq),
            tick_hi: ticks_ceil(b_hi + margin, ppq),
            key_lo: (p_lo - CULL_MARGIN_KEYS).clamp(0, 255) as u32,
            key_hi: (p_hi + CULL_MARGIN_KEYS).clamp(0, 255) as u32,
        }
    }

    /// Upper limit of the scroll position (pianoroll.clamp_view): hugs the highest key when the whole range fits above.
    fn top_limit(&self) -> f64 {
        self.keys as f64 - 0.5
    }

    pub fn set_screen(&mut self, rect: Rect) {
        self.kb_w = 56.0;
        self.ruler_h = 20.0;
        self.w = rect.width();
        self.h = rect.height();
    }

    pub fn clamp(&mut self) {
        let rows = (self.h - self.ruler_h) as f64 / self.sy;
        let top = self.top_limit();
        self.top = if rows >= self.keys as f64 {
            top
        } else {
            self.top.clamp(rows - 0.5, top)
        };
        self.t = self.t.max(0.0);
    }

    pub fn fit_shapes(&mut self, shapes: &[Shape], beats: i64) {
        if self.w < 50.0 {
            return;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for sh in shapes {
            for stroke in engine::shape_strokes(sh) {
                for p in stroke {
                    lo = lo.min(p[0]);
                    hi = hi.max(p[0]);
                }
            }
        }
        let (lo, hi) = if lo.is_finite() {
            (lo, hi)
        } else {
            (0.0, 4.0 * beats as f64)
        };
        let span = (hi - lo).max(1.0);
        self.sx = ((self.w - self.kb_w) as f64 / (span * 1.06)).max(0.05);
        self.t = (lo - span * 0.03).max(0.0);
        self.sy = (self.h - self.ruler_h) as f64 / self.keys as f64;
        self.top = self.top_limit();
        self.ready = true;
    }

    #[allow(dead_code)] // for tests / later use
    pub fn fit(&mut self, app: &App) {
        self.fit_shapes(&app.shapes, app.beats);
    }

    pub fn state(&self) -> Option<spiderweb_io::project::ViewState> {
        self.ready.then_some(spiderweb_io::project::ViewState {
            t: self.t,
            top: self.top,
            sx: self.sx,
            sy: self.sy,
        })
    }

    pub fn apply_state(&mut self, v: Option<spiderweb_io::project::ViewState>) {
        if let Some(v) = v {
            self.sx = v.sx.clamp(0.05, 100000.0);
            self.sy = v.sy.clamp(1.0, 60.0);
            self.t = v.t;
            self.top = v.top;
            self.ready = true;
            self.clamp();
        }
    }
}

/// Beat position -> tick, rounding down and saturating at the `Note` tick range.
fn ticks_floor(beats: f64, ppq: f64) -> u32 {
    (beats * ppq).floor().clamp(0.0, u32::MAX as f64) as u32
}

/// Beat position -> tick, rounding up and saturating at the `Note` tick range.
fn ticks_ceil(beats: f64, ppq: f64) -> u32 {
    (beats * ppq).ceil().clamp(0.0, u32::MAX as f64) as u32
}

/// Primary-button drag state machine (upstream self.drag).
#[derive(Clone, Debug)]
pub enum Drag {
    Pan {
        start: Pos2,
        t: f64,
        top: f64,
        moved: bool,
    },
    Seek {
        was_playing: bool,
    },
    Create {
        start: Pt,
        screen: Pos2,
    },
    Free {
        last: Pos2,
    },
    Segment {
        screen: Pos2,
    },
    Arc {
        screen: Pos2,
    },
    Move {
        start: Pt,
        orig: Vec<(usize, Vec<Pt>)>,
        one: Option<usize>,
        moved: bool,
        /// The click is on the only selected custom shape: pick the stroke under it on release (None = unpick)
        part: Option<Option<usize>>,
        /// Clicking a single funnel again: remembers what is being clicked (highlight / clear part on release).
        funnel_again: Option<usize>,
        funnel_part: Option<PartId>,
    },
    Handle {
        i: usize,
    },
    /// Dragging after a press with the text tool: select up to the mouse (upstream ("textsel",))
    TextSel,
    /// Dragging a stroke handle of a custom shape (upstream drag's handle + the hid tuple)
    StrokeHandle(crate::roll_live::StrokeHandleId),
    /// A box dragged out with square / circle / triangle (upstream create + draft["draw"])
    BoxCreate {
        start: Pt,
        screen: Pos2,
        tool: Tool,
    },
    /// Placing the custom shape selected in the panel (upstream place)
    Place {
        start: Pt,
        screen: Pos2,
        aspect: Option<f64>,
    },
    /// Corner / edge midpoint resize drag (upstream resize)
    Resize {
        k: usize,
        orig: Vec<Pt>,
        side: bool,
        start: Pt,
    },
    /// Skew from outside an edge midpoint (upstream skew)
    Skew {
        k: usize,
        orig: Vec<Pt>,
        start: Pt,
    },
    /// Rotate from outside a corner (upstream turn)
    Turn {
        orig: Vec<Pt>,
        a0: f64,
    },
    /// Drawing a funnel wall (upstream drag's "wall").
    Wall {
        screen: Pos2,
    },
}

/// A hit handle: a point number of an ordinary shape, or a stroke handle of a custom shape.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HandleId {
    Point(usize),
    Stroke(crate::roll_live::StrokeHandleId),
}

/// Right-drag state (upstream pianoroll's self._scrub): `tick` is None until the drag exceeds 4px.
#[derive(Clone, Debug)]
pub struct RightDrag {
    /// Position at press time (roll-local coordinates): the shape for the menu is looked up here on release
    pub start: Pos2,
    /// Current tick once previewing has started; None = not started
    pub tick: Option<f64>,
    /// No drag and no shape hit on release: deselect (upstream deselect)
    pub deselect: bool,
    /// Whether Shift was held at press time (used by the menu's "add anchor here")
    pub shift: bool,
}

struct Inputs {
    pos: Option<Pos2>,
    interact: Option<Pos2>,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    secondary_pressed: bool,
    secondary_released: bool,
    secondary_down: bool,
    secondary_double: bool,
    middle_pressed: bool,
    middle_released: bool,
    double: bool,
    ctrl: bool,
    shift: bool,
    alt: bool,
    scroll: Vec2,
    /// Touchpad pinch factor (1.0 = none)
    pinch: f32,
    /// egui wheel zoom factor, ctrl/cmd + wheel (1.0 = none)
    zoom: f32,
    /// Two-finger / touchscreen pan of the current gesture
    pan: Vec2,
}

/// Converts pointer positions to roll-local coordinates (painting adds rect.min, hit testing works in local coordinates).
fn inputs(ui: &egui::Ui) -> Inputs {
    let origin = ui.max_rect().min.to_vec2();
    let local = move |p: Option<Pos2>| p.map(|p| p - origin);
    ui.input(|i| Inputs {
        pos: local(i.pointer.hover_pos()),
        interact: local(i.pointer.interact_pos()),
        primary_pressed: i.pointer.primary_pressed(),
        primary_released: i.pointer.primary_released(),
        primary_down: i.pointer.primary_down(),
        secondary_pressed: i.pointer.secondary_pressed(),
        secondary_released: i.pointer.secondary_released(),
        secondary_down: i.pointer.secondary_down(),
        middle_pressed: i.pointer.button_pressed(egui::PointerButton::Middle),
        middle_released: i.pointer.button_released(egui::PointerButton::Middle),
        secondary_double: i
            .pointer
            .button_double_clicked(egui::PointerButton::Secondary),
        double: i
            .pointer
            .button_double_clicked(egui::PointerButton::Primary),
        ctrl: i.modifiers.command || i.modifiers.ctrl,
        shift: i.modifiers.shift,
        alt: i.modifiers.alt,
        scroll: i.smooth_scroll_delta,
        // Trackpad pinch (multi-touch) and egui's wheel zoom factor (ctrl/cmd+wheel)
        pinch: i.multi_touch().map(|t| t.zoom_delta).unwrap_or(1.0),
        zoom: if i.multi_touch().is_some() {
            1.0
        } else {
            i.zoom_delta()
        },
        pan: i
            .multi_touch()
            .map(|t| t.translation_delta)
            .unwrap_or(Vec2::ZERO),
    })
}

/// Piano roll entry point: layout, input, painting.
pub fn roll_ui(app: &mut App, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    app.view.set_screen(rect);
    if !app.view.ready && rect.width() > 50.0 {
        app.view.fit_shapes(&app.shapes, app.beats);
    }
    if !app.view.ready {
        crate::roll_menu::menu_ui(app, ui);
        return;
    }
    // A popup (context menu / dropdown) is open: the roll's input steps aside (upstream tk menus grab events)
    if !crate::roll_menu::is_popup_open(ui.ctx()) {
        let input = inputs(ui);
        handle_input(
            app,
            &input,
            Rect::from_min_size(Pos2::ZERO, rect.size()),
            response.contains_pointer(),
        );
        if app.drag.is_none()
            && let Some(pos) = input.pos
            && pos.x >= app.view.kb_w
            && pos.y >= app.view.ruler_h
            && pos.x <= app.view.w
            && pos.y <= app.view.h
        {
            let icon = if app.draft.is_none() && hit_handle(app, pos).is_some() {
                egui::CursorIcon::Move // grabbing a handle (upstream fleur)
            } else {
                crate::roll_custom::custom_cursor(app, crate::roll_custom::custom_hit(app, pos))
            };
            ui.ctx().set_cursor_icon(icon);
        }
    }
    let _ = response;
    // GPU notes: rebuild the culling index only when the revision is dirty; pan / zoom upload
    // only when the margin-expanded view leaves the uploaded range
    if let Some(gpu) = &app.note_gpu {
        gpu.sync(
            &app.rendered,
            &app.sels,
            app.notes_revision,
            app.view.cull_range(app.ppq),
        );
    }
    paint(app, &painter, rect);
    crate::roll_menu::menu_ui(app, ui);
}

// ---------------------------------------------------------------- coordinates and hit testing

pub(crate) fn event_pt(app: &App, p: Pos2, snap: bool, shift: bool) -> Pt {
    let v = &app.view;
    let x = p.x.clamp(v.kb_w, v.w);
    let y = p.y.clamp(v.ruler_h, v.h);
    let mut b = v.b_of(x);
    let mut q = v.p_of(y);
    if snap
        && !shift
        && let Some(sb) = app.snap_beats()
    {
        b = (b / sb).round() * sb;
        q = q.round();
    }
    [b.max(0.0), q.clamp(0.0, (app.keys - 1) as f64)]
}

/// Draggable points of the selected shape as (beat, pitch, index); curves / custom shapes use their own handles.
fn handles(app: &App, sh: &Shape) -> Vec<(Pt, usize)> {
    if sh.kind == Kind::Curve {
        return app.curve_handle_indices(sh);
    }
    if sh.kind == Kind::Custom {
        return Vec::new();
    }
    if sh.kind == Kind::Free || sh.pts.len() > 300 {
        return Vec::new();
    }
    sh.pts
        .iter()
        .copied()
        .enumerate()
        .map(|(i, p)| (p, i))
        .collect()
}

fn hit_handle(app: &App, p: Pos2) -> Option<HandleId> {
    let sh = app.selected()?;
    if sh.kind == Kind::Curve {
        // Curves: anchors / pulled-out handles are grabbable with any tool; ends are left to Select (the free flag of upstream curve_handles)
        return app
            .curve_hit_handle(sh, p, app.tool == Tool::Select)
            .map(HandleId::Point);
    }
    if sh.kind == Kind::Custom {
        // Anchors / handles of the picked curve stroke are grabbable with any tool; stroke points only with Select
        return crate::roll_live::hit_stroke_handle(app, sh, p, app.tool == Tool::Select)
            .map(HandleId::Stroke);
    }
    if sh.kind == Kind::Funnel {
        // Funnels: curve handles / the start point are grabbable with any tool; line / wall points are left to Select
        return roll_funnel::hit_funnel_handle(app, sh, p, app.tool == Tool::Select)
            .map(HandleId::Point);
    }
    let near = 7.0_f32.max(8.0 * app.scale());
    for (pt, i) in handles(app, sh).into_iter().rev() {
        let x = app.view.x_of(pt[0]);
        let y = app.view.y_of(pt[1]);
        if (x - p.x).abs() <= near && (y - p.y).abs() <= near {
            return Some(HandleId::Point(i));
        }
    }
    None
}

/// Distance from a point to a shape's polyline (screen coordinates).
fn stroke_hit(app: &App, strokes: &[Vec<Pt>], p: Pos2) -> bool {
    for stroke in strokes {
        let pts: Vec<(f32, f32)> = stroke
            .iter()
            .map(|q| (app.view.x_of(q[0]), app.view.y_of(q[1])))
            .collect();
        if pts.len() == 1 {
            let (x, y) = pts[0];
            if ((x - p.x).powi(2) + (y - p.y).powi(2)).sqrt() < 6.0 {
                return true;
            }
            continue;
        }
        for w in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let ll = dx * dx + dy * dy;
            let u = if ll == 0.0 {
                0.0
            } else {
                (((p.x - ax) * dx + (p.y - ay) * dy) / ll).clamp(0.0, 1.0)
            };
            let d = ((p.x - ax - u * dx).powi(2) + (p.y - ay - u * dy).powi(2)).sqrt();
            if d < 6.0 {
                return true;
            }
        }
    }
    false
}

fn hit_shape(app: &App, p: Pos2) -> Option<usize> {
    let (b, q) = (app.view.b_of(p.x), app.view.p_of(p.y));
    for i in (0..app.shapes.len()).rev() {
        let sh = &app.shapes[i];
        let mut strokes = engine::shape_strokes(sh);
        if joined::all_tumours(sh).iter().any(|tm| tm.on) {
            let mut plain = sh.clone();
            plain.tumour = None;
            plain.tumours.clear();
            strokes.extend(engine::shape_strokes(&plain));
        }
        if stroke_hit(app, &strokes, p) {
            return Some(i);
        }
        // Pasted notes: anywhere inside the box counts as a hit
        if sh.notes.is_some()
            && crate::roll_live::inside_strokes(&spiderweb_core::custom::custom_strokes(sh), b, q)
        {
            return Some(i);
        }
        // Fill / Spam shapes: anywhere inside the outline counts as a hit (gaps closed with straight lines)
        if sh.kind == Kind::Custom && matches!(sh.fill, Fill::Fill | Fill::Spam) {
            let polys = if sh.text.is_some() {
                spiderweb_core::custom::custom_strokes(sh)
            } else {
                spiderweb_core::custom::fill_plan(sh).polys
            };
            let inside = if sh.union {
                // Overlaps are filled too: inside any ring counts
                polys
                    .iter()
                    .any(|poly| crate::roll_live::inside_strokes(std::slice::from_ref(poly), b, q))
            } else {
                crate::roll_live::inside_strokes(&polys, b, q)
            };
            if inside {
                return Some(i);
            }
        }
        // The inside of a funnel also counts as a hit (upstream hit_shape's funnel_contains)
        if sh.kind == Kind::Funnel
            && funnel::funnel_contains(sh, app.view.b_of(p.x), app.view.p_of(p.y))
        {
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------- input

fn handle_input(app: &mut App, input: &Inputs, rect: Rect, pointer_over: bool) {
    let (kb_w, ruler_h) = (app.view.kb_w, app.view.ruler_h);
    let pos_in_roll = move |p: Pos2| rect.contains(p) && p.x >= kb_w && p.y >= ruler_h;
    if pointer_over
        && let Some(pos) = input.pos
        && pos_in_roll(pos)
    {
        app.position = Some(position_text(app, pos));
    }
    if pointer_over
        && input.primary_pressed
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        on_press(app, pos, input);
    }
    if input.primary_down {
        if app.drag.is_some() {
            if let Some(pos) = input.interact.or(input.pos) {
                on_drag(app, pos, input);
            }
        } else if app.follow.is_some()
            && let Some(pos) = input.interact.or(input.pos)
        {
            let follow = app.follow.take();
            app.drag = follow;
            on_drag(app, pos, input);
        }
    }
    if input.primary_released
        && let Some(pos) = input.interact.or(input.pos)
    {
        on_release(app, pos, input, false);
    }
    if pointer_over
        && input.double
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        on_double(app, pos, input.shift);
    }
    if pointer_over
        && input.secondary_pressed
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        app.right_drag = on_right(app, pos, input);
    }
    if input.secondary_down
        && let Some(pos) = input.interact.or(input.pos)
    {
        on_right_drag(app, pos);
    }
    if input.secondary_released {
        on_right_release(app);
        app.right_drag = None;
    }
    if input.secondary_double {
        app.toggle_select_tool();
    }
    if pointer_over
        && input.middle_pressed
        && let Some(pos) = input.pos
        && pos_in_roll(pos)
    {
        app.drag = Some(Drag::Pan {
            start: pos,
            t: app.view.t,
            top: app.view.top,
            moved: false,
        });
    }
    if input.middle_released {
        if let Some(pos) = input.interact.or(input.pos) {
            on_middle_release(app, pos, input);
        }
        app.drag = None;
    }
    if input.scroll != Vec2::ZERO
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        on_wheel(app, pos, input);
    }
}

fn on_press(app: &mut App, pos: Pos2, input: &Inputs) {
    // First finish the "click to start, follow the mouse" shape (upstream follow)
    if let Some(follow) = app.follow.take() {
        app.drag = Some(follow);
        on_drag(app, pos, input);
        on_release(app, pos, input, true);
        return;
    }
    if pos.x < app.view.kb_w || !app.view.ready {
        return;
    }
    if app.tool == Tool::Select {
        // Select is the startup tool; its tip pops on the first click in the roll (upstream pianoroll.on_press)
        app.tips.show_waiting("select");
    }
    if pos.y < app.view.ruler_h {
        let playing = app.player.running();
        app.stop_play();
        app.set_playhead(event_pt(app, pos, true, input.shift)[0]);
        app.drag = Some(Drag::Seek {
            was_playing: playing,
        });
        return;
    }
    let pt = crate::roll_live::draw_pt(app, pos, true, input.shift);

    if app.draft.is_none()
        && let Some(h) = hit_handle(app, pos)
    {
        app.push_undo(&rust_i18n::t!("pianoroll.drag_a_point"));
        app.drag = Some(match h {
            HandleId::Point(i) => Drag::Handle { i },
            HandleId::Stroke(id) => Drag::StrokeHandle(id),
        });
        return;
    }

    // Box of the selected custom shape: corner / edge resize, rotate outside a corner, skew outside an edge
    let custom_hit = crate::roll_custom::custom_hit(app, pos);
    if let Some(hit) = custom_hit
        && let Some(drag) = custom_box_drag(app, hit, pos, input.shift)
    {
        let name = match hit {
            crate::roll_custom::CustomHit::Turn(_) => rust_i18n::t!("pianoroll.turn"),
            crate::roll_custom::CustomHit::Skew(_) => rust_i18n::t!("pianoroll.skew"),
            _ => std::borrow::Cow::Borrowed("Resize"),
        };
        app.push_undo(&name);
        app.drag = Some(drag);
        return;
    }

    match app.tool {
        Tool::Select => {
            let mut i = hit_shape(app, pos);
            if i.is_none()
                && matches!(custom_hit, Some(crate::roll_custom::CustomHit::Inside))
                && !input.ctrl
            {
                i = app.sel; // anywhere inside the selected custom shape's box counts as a move
            }
            // The click is on an already singly-selected custom shape: pick the stroke under it on release (upstream decides this here first)
            let was_only_selected = i.is_some() && app.sel == i && app.sels.len() == 1;
            // Clicking the selected funnel again: the line / curve under the mouse gets highlighted (upstream again / part)
            let funnel_again = i.filter(|&j| {
                app.sels.len() == 1
                    && app.sels.contains(&j)
                    && app
                        .shapes
                        .get(j)
                        .map(|s| s.kind == Kind::Funnel)
                        .unwrap_or(false)
            });
            let funnel_part = funnel_again.and_then(|j| {
                app.shapes
                    .get(j)
                    .and_then(|sh| roll_funnel::part_at(app, sh, pos, 6.0))
            });
            if input.ctrl
                && let Some(part) = funnel_part
            {
                // Ctrl+click on a part: add / remove this one
                app.toggle_part(part);
                return;
            }
            if input.ctrl {
                if let Some(i) = i {
                    app.select(Some(i), true);
                    if !app.sels.contains(&i) {
                        return;
                    }
                } else {
                    app.drag = Some(Drag::Pan {
                        start: pos,
                        t: app.view.t,
                        top: app.view.top,
                        moved: false,
                    });
                    return;
                }
            } else if let Some(i) = i {
                if !app.sels.contains(&i) {
                    app.select(Some(i), false);
                } else if app.sel != Some(i) {
                    app.select_many(app.sels.clone(), Some(i));
                }
            } else {
                app.select(None, false);
                app.drag = Some(Drag::Pan {
                    start: pos,
                    t: app.view.t,
                    top: app.view.top,
                    moved: false,
                });
                return;
            }
            // The only selected custom shape: pick the stroke under the mouse (within 6px of the outline), otherwise unpick
            let mut part: Option<Option<usize>> = None;
            if was_only_selected
                && !input.ctrl
                && let Some(i) = i
                && let Some(sh) = app.shapes.get(i)
                && sh.kind == Kind::Custom
                && sh.text.is_none()
                && sh.notes.is_none()
            {
                part = Some(crate::roll_live::stroke_at(app, sh, pos, 6.0));
            }
            app.push_undo(&rust_i18n::t!("pianoroll.move"));
            let orig: Vec<(usize, Vec<Pt>)> = app
                .sels
                .iter()
                .map(|&j| (j, app.shapes[j].pts.clone()))
                .collect();
            let one = if input.ctrl { None } else { i };
            app.drag = Some(Drag::Move {
                start: pt,
                orig,
                one,
                moved: false,
                part,
                funnel_again,
                funnel_part,
            });
        }
        Tool::Poly => {
            if app.draft.is_none() {
                let defaults = app.defaults.clone();
                app.draft = Some(engine::make_shape(Kind::Poly, &[pt, pt], &defaults));
                app.drag = Some(Drag::Segment { screen: pos });
            } else if !poly_point(app, pt) {
                app.drag = Some(Drag::Segment { screen: pos });
            }
        }
        Tool::Arc => {
            if app.draft.is_none() {
                let defaults = app.defaults.clone();
                let mut sh = engine::make_shape(Kind::Arc, &[pt, pt], &defaults);
                sh.k = app.view.sy / app.view.sx;
                app.draft = Some(sh);
                app.drag = Some(Drag::Arc { screen: pos });
            } else if app.arc_bend {
                if let Some(d) = app.draft.as_mut() {
                    d.pts[1] = pt;
                }
                app.arc_bend = false;
                finish_arc(app);
            } else {
                let len = app.draft.as_ref().map(|d| d.pts.len()).unwrap_or(0);
                if len == 2 {
                    if let Some(d) = app.draft.as_mut() {
                        d.pts[1] = pt;
                        d.pts.push(pt);
                    }
                } else {
                    if let Some(d) = app.draft.as_mut()
                        && let Some(last) = d.pts.last_mut()
                    {
                        *last = pt;
                    }
                    finish_arc(app);
                }
            }
        }
        Tool::Free => {
            let mut sh = engine::make_shape(
                Kind::Free,
                &[event_pt(app, pos, false, input.shift)],
                &app.defaults.clone(),
            );
            sh.smooth = app.free_smooth;
            app.draft = Some(sh);
            app.drag = Some(Drag::Free { last: pos });
        }
        Tool::Line => {
            let defaults = app.defaults.clone();
            app.draft = Some(engine::make_shape(Kind::Line, &[pt, pt], &defaults));
            app.drag = Some(Drag::Create {
                start: pt,
                screen: pos,
            });
        }
        Tool::Curve => {
            // Drag to create an S curve, commit on release (drags out the two ends; handle editing comes once the shape is selected)
            let defaults = app.defaults.clone();
            app.draft = Some(engine::make_shape(Kind::Curve, &[pt, pt], &defaults));
            app.drag = Some(Drag::Create {
                start: pt,
                screen: pos,
            });
        }
        Tool::Square | Tool::Circle | Tool::Triangle => {
            let tool = app.tool;
            app.draft_draw = Some(tool);
            let sh = crate::roll_live::box_draft(app, tool, pt, pt);
            app.draft = Some(sh);
            app.drag = Some(Drag::BoxCreate {
                start: pt,
                screen: pos,
                tool,
            });
        }
        Tool::Custom => {
            let name = app.custom_defaults.shape.clone();
            if let Some((strokes, aspect)) =
                crate::roll_live::builtin_template(&app.library_dir, &name)
            {
                let defaults = app.defaults.clone();
                let cd = crate::roll_live::core_custom_defaults(app);
                let sh =
                    crate::roll_live::new_custom_parts(&defaults, &cd, &name, &strokes, pt, pt);
                app.draft = Some(sh);
                app.drag = Some(Drag::Place {
                    start: pt,
                    screen: pos,
                    aspect: Some(aspect),
                });
            } else {
                app.status = rust_i18n::t!("status.no_shape").to_string();
            }
        }
        Tool::Text => {
            crate::roll_text::text_press_shift(app, pos, pt, input.shift);
        }
        Tool::Funnel => {
            roll_funnel::funnel_press(app, pos, input.shift);
        }
    }
}

/// Clicking the selected custom shape's box -> the matching drag (returns None inside the box).
fn custom_box_drag(
    app: &App,
    hit: crate::roll_custom::CustomHit,
    pos: Pos2,
    shift: bool,
) -> Option<Drag> {
    use crate::roll_custom::CustomHit;
    let orig = app.selected().map(|sh| sh.pts.clone()).unwrap_or_default();
    Some(match hit {
        CustomHit::Turn(_) => Drag::Turn {
            a0: crate::roll_custom::screen_angle(app, &orig, pos),
            orig,
        },
        CustomHit::Skew(k) => Drag::Skew {
            k,
            orig,
            start: event_pt(app, pos, false, shift),
        },
        CustomHit::Corner(k) | CustomHit::Side(k) => Drag::Resize {
            k,
            orig,
            side: matches!(hit, CustomHit::Side(_)),
            start: event_pt(app, pos, false, shift),
        },
        CustomHit::Inside => return None,
    })
}

/// Puts the polyline's next point at pt (followed by another point tracking the mouse).
/// true = finished (Live drawing: returning to the first point = close) (upstream poly_point).
fn poly_point(app: &mut App, pt: Pt) -> bool {
    let closed = {
        let Some(d) = app.draft.as_mut() else {
            return false;
        };
        if let Some(last) = d.pts.last_mut() {
            *last = pt;
        }
        d.pts.push(pt);
        d.pts.len() >= 4 && d.pts[0] == pt
    };
    if closed && crate::roll_live::live_drawing(app) {
        finish_poly(app);
        true
    } else {
        false
    }
}

fn finish_arc(app: &mut App) {
    let pts = app
        .draft
        .as_ref()
        .map(|d| d.pts.clone())
        .unwrap_or_default();
    if pts.len() >= 3 {
        if pts[0] == pts[1] && pts[1] == pts[2] {
            if let Some(d) = app.draft.as_mut() {
                d.pts.pop();
            }
        } else {
            app.commit_draft();
        }
    }
}

fn on_drag(app: &mut App, pos: Pos2, input: &Inputs) {
    app.position = Some(position_text(app, pos));
    let Some(drag) = app.drag.clone() else {
        return;
    };
    match drag {
        Drag::Pan {
            start,
            t,
            top,
            moved,
        } => {
            let (sx, sy) = (app.view.sx, app.view.sy);
            app.view.t = t - (pos.x - start.x) as f64 / sx;
            app.view.top = top + (pos.y - start.y) as f64 / sy;
            app.view.clamp();
            let moved = moved || (pos - start).length() > 3.0;
            app.drag = Some(Drag::Pan {
                start,
                t,
                top,
                moved,
            });
        }
        Drag::Seek { .. } => {
            app.set_playhead(event_pt(app, pos, true, input.shift)[0]);
        }
        Drag::Handle { i } => match app.selected().map(|sh| sh.kind) {
            Some(Kind::Curve) => {
                let pt = event_pt(app, pos, true, input.shift);
                app.curve_drag(i, pt, input.alt);
                app.shapes_changed();
            }
            Some(Kind::Funnel) => {
                roll_funnel::funnel_drag_handle(app, i, pos, input.shift, input.alt, input.ctrl);
            }
            _ => {
                let pt = event_pt(app, pos, true, input.shift);
                if let Some(sh) = app.selected_mut()
                    && i < sh.pts.len()
                {
                    sh.pts[i] = pt;
                }
                app.shapes_changed();
            }
        },
        Drag::Wall { .. } => {
            let pt = event_pt(app, pos, true, input.shift);
            roll_funnel::funnel_drag_wall(app, pt, input.ctrl);
        }
        Drag::StrokeHandle(h) => {
            let h = crate::roll_live::drag_stroke_handle(app, h, pos, input.shift, input.alt);
            app.drag = Some(Drag::StrokeHandle(h));
            app.shapes_changed();
        }
        Drag::Move {
            start,
            orig,
            one,
            moved,
            part,
            funnel_again,
            funnel_part,
        } => {
            let pt = event_pt(app, pos, true, input.shift);
            let (db, dp) = (pt[0] - start[0], pt[1] - start[1]);
            if !moved && db == 0.0 && dp == 0.0 {
                return;
            }
            app.drag = Some(Drag::Move {
                start,
                orig: orig.clone(),
                one,
                moved: true,
                part,
                funnel_again,
                funnel_part,
            });
            for (j, pts) in &orig {
                if let Some(sh) = app.shapes.get_mut(*j) {
                    sh.pts = pts.iter().map(|p| [p[0] + db, p[1] + dp]).collect();
                }
            }
            app.shapes_changed();
        }
        Drag::Free { last } => {
            let dist = (pos - last).length();
            if dist >= 3.0 {
                let pt = crate::roll_live::draw_pt(app, pos, false, input.shift);
                if let Some(d) = app.draft.as_mut() {
                    d.pts.push(pt);
                }
                app.drag = Some(Drag::Free { last: pos });
            }
        }
        Drag::Segment { .. } => {
            let pt = crate::roll_live::draw_pt(app, pos, true, input.shift);
            if let Some(d) = app.draft.as_mut()
                && let Some(last) = d.pts.last_mut()
            {
                *last = pt;
            }
        }
        Drag::Arc { .. } => {
            let pt = event_pt(app, pos, true, input.shift);
            if let Some(d) = app.draft.as_mut()
                && d.pts.len() >= 2
            {
                d.pts[1] = pt;
            }
        }
        Drag::TextSel => {
            crate::roll_text::text_drag(app, pos);
        }
        Drag::Create { start, .. } => {
            let pt = event_pt(app, pos, true, input.shift);
            let kind = app.draft.as_ref().map(|d| d.kind).unwrap_or(Kind::Line);
            let defaults = if kind == Kind::Funnel {
                roll_funnel::new_funnel_defaults(app)
            } else {
                app.defaults.clone()
            };
            let mut sh = if kind == Kind::Curve {
                let mut c = engine::make_shape(Kind::Curve, &[start, pt], &defaults);
                // Gentle S curve: consistent with make_shape
                let mid = (start[0] + pt[0]) / 2.0;
                c.pts = vec![start, [mid, start[1]], [mid, pt[1]], pt];
                c
            } else {
                engine::make_shape(kind, &[start, pt], &defaults)
            };
            if kind == Kind::Arc {
                sh.k = app.view.sy / app.view.sx;
            }
            app.draft = Some(sh);
        }
        Drag::BoxCreate { start, tool, .. } => {
            let mut pt = crate::roll_live::draw_pt(app, pos, true, input.shift);
            if input.ctrl {
                pt = crate::roll_live::keep_aspect(
                    app,
                    start,
                    pt,
                    crate::roll_live::box_aspect(tool),
                );
            }
            app.draft = Some(crate::roll_live::box_draft(app, tool, start, pt));
        }
        Drag::Place { start, aspect, .. } => {
            let mut pt = crate::roll_live::draw_pt(app, pos, true, input.shift);
            if input.ctrl
                && let Some(asp) = aspect
            {
                pt = crate::roll_live::keep_aspect(app, start, pt, asp);
            }
            if let Some(d) = app.draft.as_mut() {
                d.pts =
                    spiderweb_core::custom::box_frame(start[0], start[1], pt[0], pt[1]).to_vec();
            }
        }
        Drag::Resize {
            k,
            orig,
            side,
            start,
        } => {
            let pt = crate::roll_custom::resize_point(app, &orig, k, side, start, pos, input.shift);
            let new_pts = crate::roll_custom::resize_custom(app, &orig, k, pt, input.ctrl, side);
            if let Some(sh) = app.selected_mut() {
                sh.pts = new_pts;
            }
            app.shapes_changed();
        }
        Drag::Skew { k, orig, start } => {
            let (db, dp) = crate::roll_custom::drag_steps(app, start, pos, input.shift);
            let new_pts = crate::roll_custom::skew_custom(app, &orig, k, db, dp);
            if let Some(sh) = app.selected_mut() {
                sh.pts = new_pts;
            }
            app.shapes_changed();
        }
        Drag::Turn { orig, a0 } => {
            let mut angle = crate::roll_custom::screen_angle(app, &orig, pos) - a0;
            angle = (angle + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI)
                - std::f64::consts::PI;
            if !input.shift {
                let step = 15.0_f64.to_radians();
                angle = (angle / step).round() * step;
            }
            let new_pts = crate::roll_custom::turn_custom(app, &orig, angle);
            if let Some(sh) = app.selected_mut() {
                sh.pts = new_pts;
            }
            app.shapes_changed();
            app.position = Some(
                rust_i18n::t!(
                    "status.turned",
                    angle = format!("{:+.1}", angle.to_degrees())
                )
                .to_string(),
            );
        }
    }
}

fn on_release(app: &mut App, pos: Pos2, input: &Inputs, second: bool) {
    let Some(drag) = app.drag.take() else {
        return;
    };
    match drag {
        Drag::Seek { was_playing } => {
            if was_playing {
                app.start_play();
            }
        }
        Drag::Pan {
            start: _, moved, ..
        } => {
            if !moved {
                let playing = app.player.running();
                app.stop_play();
                app.set_playhead(event_pt(app, pos, true, input.shift)[0]);
                if playing {
                    app.start_play();
                }
            }
        }
        Drag::Move {
            moved,
            one,
            part,
            funnel_again,
            funnel_part,
            ..
        } => {
            if !moved {
                if let Some(one) = one
                    && app.sels.len() > 1
                {
                    app.select(Some(one), false);
                } else if let Some(j) = funnel_again {
                    // Clicking that funnel again: highlight the part under the mouse (clear it if nothing was hit)
                    let group = app
                        .shapes
                        .get(j)
                        .and_then(|sh| funnel_part.map(|p| roll_funnel::part_group(sh, p)))
                        .unwrap_or_default();
                    app.set_parts(group, funnel_part);
                } else if let Some(stroke) = part {
                    app.set_stroke(stroke); // click on a custom shape: pick / unpick a stroke
                }
            }
        }
        Drag::Create { screen, .. } => {
            let still = (pos - screen).length() < 4.0;
            let funnel = app
                .draft
                .as_ref()
                .map(|d| d.kind == Kind::Funnel)
                .unwrap_or(false);
            if still && !second {
                app.follow = Some(Drag::Create {
                    start: event_pt(app, pos, true, input.shift),
                    screen: pos,
                });
            } else if still {
                app.cancel_draft();
            } else if funnel {
                // Drawn onto the selected funnel's wall = a new line for it; otherwise keep waiting for the wall
                roll_funnel::funnel_finish_line(app);
            } else if app.confirm_big_draft() {
                app.commit_draft();
            }
        }
        Drag::BoxCreate {
            start,
            screen,
            tool,
        } => {
            let still = (pos - screen).length() < 4.0;
            if still && !second {
                app.follow = Some(Drag::BoxCreate {
                    start,
                    screen: pos,
                    tool,
                });
            } else if still {
                app.cancel_draft();
            } else if app.confirm_big_draft() {
                app.commit_draft();
            } else {
                app.cancel_draft();
            }
        }
        Drag::Place {
            start,
            screen,
            aspect,
        } => {
            let still = (pos - screen).length() < 4.0;
            if still && !second {
                app.follow = Some(Drag::Place {
                    start,
                    screen: pos,
                    aspect,
                });
            } else if still {
                app.cancel_draft();
            } else if app.confirm_big_draft() {
                app.commit_draft();
            } else {
                app.cancel_draft();
            }
        }
        Drag::Wall { screen } => {
            let still = (pos - screen).length() < 4.0;
            roll_funnel::funnel_finish_wall(app, still);
        }
        Drag::Segment { screen } => {
            let still = (pos - screen).length() < 4.0;
            if !still {
                let pt = crate::roll_live::draw_pt(app, pos, true, input.shift);
                poly_point(app, pt);
            }
        }
        Drag::Arc { screen } => {
            let still = (pos - screen).length() < 4.0;
            if !still {
                let end = event_pt(app, pos, true, input.shift);
                if let Some(d) = app.draft.as_mut() {
                    let start = d.pts[0];
                    d.pts = vec![
                        start,
                        [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0],
                        end,
                    ];
                }
                app.arc_bend = true;
            }
        }
        Drag::Free { .. } => {
            // Live drawing: releasing near the start = close
            if crate::roll_live::live_drawing(app) {
                let close = app
                    .draft
                    .as_ref()
                    .filter(|d| d.pts.len() >= 3)
                    .map(|d| {
                        let first = d.pts[0];
                        let sx = app.view.x_of(first[0]);
                        let sy = app.view.y_of(first[1]);
                        ((pos.x - sx).powi(2) + (pos.y - sy).powi(2)).sqrt() < 12.0 * app.scale()
                    })
                    .unwrap_or(false);
                if close && let Some(d) = app.draft.as_mut() {
                    let first = d.pts[0];
                    d.pts.push(first);
                }
            }
            let n = app.draft.as_ref().map(|d| d.pts.len()).unwrap_or(0);
            if n >= 2 {
                if let Some(d) = app.draft.as_mut() {
                    d.smooth = app.free_smooth;
                    d.k = app.view.sy / app.view.sx;
                }
                if app.confirm_big_draft() {
                    app.commit_draft();
                }
            } else {
                app.cancel_draft();
            }
        }
        Drag::Handle { .. } | Drag::StrokeHandle(_) => {
            app.shapes_changed();
        }
        Drag::Resize { .. } | Drag::Skew { .. } | Drag::Turn { .. } => {
            // The box is already updated; the panel recomputes gap / note counts next frame
            app.shapes_changed();
        }
        Drag::TextSel => {}
    }
    if matches!(app.tool, Tool::Poly) && app.draft.is_some() {
        // keep waiting for the next point
    }
}

fn on_double(app: &mut App, pos: Pos2, shift: bool) {
    let is_poly = app
        .draft
        .as_ref()
        .map(|d| d.kind == Kind::Poly)
        .unwrap_or(false);
    if is_poly {
        finish_poly(app);
        return;
    }
    // Double-click on text: Select tool = switch to the text tool and keep typing; text tool = select the word under the mouse (upstream on_double)
    if app.tool == Tool::Select && crate::roll_text::text_at(app, pos).is_some() {
        crate::roll_text::text_edit(app, pos);
        return;
    }
    if app.tool == Tool::Text {
        crate::roll_text::text_double(app, pos);
        return;
    }
    // Double-click on the selected curve: add an anchor at the closest point on the curve (same as middle-click, see on_middle_release)
    if app.draft.is_none()
        && app.tool == Tool::Select
        && app
            .selected()
            .map(|sh| sh.kind == Kind::Curve)
            .unwrap_or(false)
        && hit_handle(app, pos).is_none()
    {
        let pt = event_pt(app, pos, true, shift);
        app.curve_click(pos, pt, Some((12.0 * app.scale()) as f64));
    }
}

fn finish_poly(app: &mut App) {
    let Some(d) = app.draft.as_mut() else {
        return;
    };
    let mut pts: Vec<Pt> = Vec::new();
    for p in d.pts.iter().take(d.pts.len().saturating_sub(1)) {
        if pts.last() != Some(p) {
            pts.push(*p);
        }
    }
    if pts.len() >= 2 {
        d.pts = pts;
        app.commit_draft();
    } else {
        app.cancel_draft();
    }
}

/// Middle-click (without dragging; upstream on_middle_release): add an anchor on the selected
/// curve; on a selected funnel: add a start point on a line / an anchor near a curve.
fn on_middle_release(app: &mut App, pos: Pos2, input: &Inputs) {
    let clicked = matches!(
        &app.drag,
        Some(Drag::Pan { start, .. }) if (pos - *start).length() <= 3.0
    );
    if !clicked || app.draft.is_some() || pos.x < app.view.kb_w || pos.y < app.view.ruler_h {
        return;
    }
    if app
        .selected()
        .map(|sh| sh.kind == Kind::Custom)
        .unwrap_or(false)
    {
        // Picked curve stroke: add an anchor at the point
        let near = Some((12.0 * app.scale()) as f64);
        if crate::roll_live::stroke_click(app, pos, near, input.shift) {
            return;
        }
    }
    match app.selected().map(|sh| sh.kind) {
        Some(Kind::Curve) => {
            let pt = event_pt(app, pos, true, input.shift);
            app.curve_click(pos, pt, Some((12.0 * app.scale()) as f64));
        }
        Some(Kind::Funnel) => {
            roll_funnel::funnel_click(app, pos, input.shift, input.ctrl);
        }
        _ => {}
    }
}

/// Right press: finishes a half-drawn polyline / arc, deletes funnel / curve / stroke handles;
/// if nothing handled it and it is inside the roll area, returns the state for right-drag
/// previewing (upstream on_right).
fn on_right(app: &mut App, pos: Pos2, input: &Inputs) -> Option<RightDrag> {
    app.right_done = false;
    let kind = app.draft.as_ref().map(|d| d.kind);
    let finished = app.follow.is_some() || matches!(kind, Some(Kind::Poly) | Some(Kind::Arc));
    if app.follow.is_some() {
        app.cancel_draft();
    } else if kind == Some(Kind::Poly) {
        finish_poly(app);
    } else if kind == Some(Kind::Arc) {
        app.cancel_draft();
    }
    // Right-click a funnel's curve start / anchor / handle: delete or retract it (upstream delete_funnel_handle)
    let funnel_hit = if app.draft.is_none() {
        app.selected()
            .filter(|sh| sh.kind == Kind::Funnel)
            .and_then(|sh| {
                roll_funnel::hit_funnel_handle(app, sh, pos, true).filter(|&i| i >= sh.pts.len())
            })
    } else {
        None
    };
    if let Some(i) = funnel_hit
        && roll_funnel::funnel_delete_handle(app, i, input.ctrl)
    {
        app.right_done = true;
    }
    // Right-click a curve anchor = delete it, handle = retract into the anchor (ends untouched; release falls through to deselect)
    if app.draft.is_none()
        && let Some(sh) = app.selected()
        && sh.kind == Kind::Curve
        && let Some(i) = app.curve_hit_handle(sh, pos, true)
        && app.curve_delete_handle(i)
    {
        app.right_done = true;
        return None;
    }
    // Right-click an anchor of the picked curve stroke = delete it, handle = retract (stroke points are not treated specially)
    let custom_hid = if app.draft.is_none() {
        app.selected().and_then(|sh| {
            (sh.kind == Kind::Custom)
                .then(|| crate::roll_live::hit_stroke_handle(app, sh, pos, true))
                .flatten()
        })
    } else {
        None
    };
    if let Some(h) = custom_hid
        && matches!(h, crate::roll_live::StrokeHandleId::Curve { .. })
    {
        crate::roll_live::delete_stroke_handle(app, h);
        app.right_done = true;
    }
    if app.right_done {
        return None;
    }
    if pos.x < app.view.kb_w {
        // On the keyboard column: deselect (the else branch of upstream on_right)
        if !finished {
            app.cancel_draft();
            app.select(None, false);
        }
        return None;
    }
    Some(RightDrag {
        start: pos,
        tick: None,
        deselect: !finished,
        shift: input.shift,
    })
}

/// Tick during a right-drag (upstream scrub_tick): right of the keyboard column, never below 0.
fn scrub_tick(app: &App, pos: Pos2) -> f64 {
    let x = pos.x.max(app.view.kb_w);
    (app.view.b_of(x) * app.ppq as f64).max(0.0)
}

/// Right-drag: the first 4px decide previewing rather than a menu, then it sweeps past notes and sounds them (upstream on_right_drag).
fn on_right_drag(app: &mut App, pos: Pos2) {
    app.position = Some(position_text(app, pos));
    let Some(sc) = app.right_drag.as_ref() else {
        return;
    };
    if sc.tick.is_none() {
        if (pos - sc.start).length() < 4.0 {
            return;
        }
        app.stop_play();
        let tick = scrub_tick(app, pos);
        if let Some(sc) = app.right_drag.as_mut() {
            sc.tick = Some(tick);
        }
        if !app.scrub(tick, tick) {
            app.right_drag = None;
        }
        return;
    }
    let prev = app.right_drag.as_ref().and_then(|s| s.tick);
    let tick = scrub_tick(app, pos);
    if let Some(prev) = prev {
        if !app.scrub(prev, tick) {
            app.right_drag = None;
            return;
        }
        if let Some(sc) = app.right_drag.as_mut() {
            sc.tick = Some(tick);
        }
    }
}

/// Right release: dragged = stop previewing; hit a shape = menu; empty space = deselect (upstream on_right_release).
fn on_right_release(app: &mut App) {
    let sc = app.right_drag.take();
    if std::mem::take(&mut app.right_done) {
        return; // already handled on press (curve point deletion / handle retraction)
    }
    let Some(sc) = sc else {
        return;
    };
    if sc.tick.is_some() {
        app.scrub_end();
        return;
    }
    if !sc.deselect {
        return; // a polyline / arc was just finished, or a mouse-following draft was cancelled
    }
    if app.draft.is_none()
        && let Some(i) = hit_shape(app, sc.start)
    {
        crate::roll_menu::open_menu(app, i, sc.start, sc.shift);
        return;
    }
    app.cancel_draft();
    app.select(None, false);
}

/// Zooms both axes around `pos` (touchpad pinch).
fn zoom_both(view: &mut View, pos: Pos2, f: f64) {
    let b = view.b_of(pos.x);
    let p = view.p_of(pos.y);
    view.sx = (view.sx * f).clamp(0.05, 100000.0);
    view.sy = (view.sy * f).clamp(1.0, 60.0);
    view.t = b - (pos.x - view.kb_w) as f64 / view.sx;
    view.top = p + (pos.y - view.ruler_h) as f64 / view.sy;
}

fn on_wheel(app: &mut App, pos: Pos2, input: &Inputs) {
    let v = &mut app.view;
    // Two-finger / touchscreen pan (and pinch in the same gesture).
    if input.pan != Vec2::ZERO {
        v.t -= input.pan.x as f64 / v.sx;
        v.top += input.pan.y as f64 / v.sy;
    }
    // Touchpad pinch: zoom both axes around the pointer.
    if (input.pinch - 1.0).abs() > 1e-4 {
        zoom_both(v, pos, input.pinch as f64);
    }
    if input.pan != Vec2::ZERO || (input.pinch - 1.0).abs() > 1e-4 {
        v.clamp();
        return;
    }
    // Ctrl/Cmd + wheel: egui turns it into a zoom factor and sends no scroll
    // delta, so the old sign-based path never saw it; zoom time like upstream.
    if (input.zoom - 1.0).abs() > 1e-4 {
        let b = v.b_of(pos.x);
        v.sx = (v.sx * input.zoom as f64).clamp(0.05, 100000.0);
        v.t = b - (pos.x - v.kb_w) as f64 / v.sx;
        v.clamp();
        return;
    }
    let dx = input.scroll.x as f64;
    let dy = input.scroll.y as f64;
    if input.alt {
        // Alt + wheel: pitch zoom (egui consumes Ctrl+wheel for its own zoom).
        let f = if dy > 0.0 { 1.25 } else { 0.8 };
        let p = v.p_of(pos.y);
        v.sy = (v.sy * f).clamp(1.0, 60.0);
        v.top = p + (pos.y - v.ruler_h) as f64 / v.sy;
    } else if input.shift {
        // Shift + wheel: time only (the mouse habit; touchpads use scroll.x).
        v.t += if dx != 0.0 {
            -dx / v.sx
        } else if dy > 0.0 {
            -120.0 / v.sx
        } else {
            120.0 / v.sx
        };
    } else {
        // Two-finger scroll / wheel: proportional panning, 1:1 on a touchpad.
        v.t -= dx / v.sx;
        v.top += dy / v.sy;
    }
    v.clamp();
}

fn position_text(app: &App, pos: Pos2) -> String {
    let ppq = app.ppq as f64;
    let beats = app.beats as f64;
    let ticks = (app.view.b_of(pos.x) * ppq).max(0.0);
    let bar = (ticks / (ppq * beats)).floor() + 1.0;
    let rest = ticks % (ppq * beats);
    let mut text = format!(
        "{}:{}:{:03}  (tick {})",
        bar as i64,
        (rest / ppq).floor() as i64 + 1,
        (rest % ppq) as i64,
        ticks as i64
    );
    let p = app.view.p_of(pos.y).round() as i64;
    if (0..app.keys).contains(&p) {
        text += &format!("     {} ({})", note_name(p), p);
    }
    text
}

pub fn note_name(p: i64) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p / 12 - 1)
}

// ---------------------------------------------------------------- painting

/// Mixes a color toward white (upstream roll_shared.fade), also used by the velocity panel.
pub(crate) fn fade(c: Color32, amount: f32) -> Color32 {
    let mix = |v: u8| -> u8 { (v as f32 + (255.0 - v as f32) * amount).round() as u8 };
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

fn paint(app: &App, painter: &egui::Painter, rect: Rect) {
    let v = &app.view;
    let area = Rect::from_min_max(
        Pos2::new(rect.min.x + v.kb_w, rect.min.y + v.ruler_h),
        rect.max,
    );
    painter.rect_filled(area, 0.0, Color32::WHITE);

    // Rows: black key background, white key separator lines, C lines
    let (p_lo, p_hi) = v.visible_pitches();
    for p in p_lo..=p_hi {
        let (y0, y1) = v.row_y(p as f64);
        let y0 = rect.min.y + y0;
        let y1 = rect.min.y + y1;
        if BLACK.contains(&(p % 12)) {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(area.min.x, y0), Pos2::new(area.max.x, y1)),
                0.0,
                Color32::from_rgb(0xe7, 0xee, 0xfa),
            );
        } else if v.sy >= 4.0 {
            painter.line_segment(
                [Pos2::new(area.min.x, y1), Pos2::new(area.max.x, y1)],
                Stroke::new(1.0, Color32::from_rgb(0xdf, 0xe6, 0xf2)),
            );
        }
    }
    for p in p_lo..=p_hi {
        if p % 12 == 0 {
            let (_, y1) = v.row_y(p as f64);
            let y = rect.min.y + y1;
            painter.line_segment(
                [Pos2::new(area.min.x, y), Pos2::new(area.max.x, y)],
                Stroke::new(1.0, Color32::from_rgb(0x60, 0x60, 0x60)),
            );
        }
    }
    if v.y_of(-0.5) < v.h {
        let (_, y) = v.row_y(0.0);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(area.min.x, rect.min.y + y), area.max),
            0.0,
            Color32::from_rgb(0xec, 0xec, 0xec),
        );
    }
    if v.y_of(v.keys as f64 - 0.5) > v.ruler_h {
        let (y0, _) = v.row_y((v.keys - 1) as f64);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(area.min.x, area.min.y),
                Pos2::new(area.max.x, rect.min.y + y0),
            ),
            0.0,
            Color32::from_rgb(0xec, 0xec, 0xec),
        );
    }

    // Columns: snap lines, beat lines, bar lines
    let b_lo = v.b_of(v.kb_w);
    let b_hi = v.b_of(v.w);
    if let Some(sb) = app.snap_beats()
        && sb < 1.0
        && sb * v.sx >= 8.0
    {
        let mut b = (b_lo / sb).ceil() * sb;
        while b <= b_hi {
            if (b - b.round()).abs() > 1e-9 {
                let x = rect.min.x + v.x_of(b).round();
                painter.line_segment(
                    [Pos2::new(x, area.min.y), Pos2::new(x, area.max.y)],
                    Stroke::new(1.0, Color32::from_rgb(0xee, 0xf1, 0xf6)),
                );
            }
            b += sb;
        }
    }
    let beats = app.beats as f64;
    if v.sx >= 5.0 {
        let mut b = b_lo.max(0.0).ceil();
        while b <= b_hi {
            if (b as i64) % app.beats != 0 {
                let x = rect.min.x + v.x_of(b).round();
                painter.line_segment(
                    [Pos2::new(x, area.min.y), Pos2::new(x, area.max.y)],
                    Stroke::new(1.0, Color32::from_rgb(0xbc, 0xc4, 0xd2)),
                );
            }
            b += 1.0;
        }
    }
    let mut step = beats.max(1.0);
    while step * v.sx < 6.0 {
        step *= 2.0;
    }
    let mut b = (b_lo / step).floor() * step;
    while b <= b_hi {
        if b >= 0.0 {
            let x = rect.min.x + v.x_of(b).round();
            painter.line_segment(
                [Pos2::new(x, area.min.y), Pos2::new(x, area.max.y)],
                Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x3a)),
            );
        }
        b += step;
    }

    // Notes
    if app.show_notes {
        if let Some(gpu) = &app.note_gpu {
            // GPU path: one instanced draw (one segment for normal, one for selected), no decimation
            let globals = crate::note_gpu::globals_for(&app.view, app.ppq, rect.min);
            painter.add(gpu.callback(globals, rect));
        } else {
            paint_notes(app, painter, rect, area);
        }
        // Green preview of what the shape being drawn would make
        paint_draft_notes(app, painter, rect);
    }

    // Shape lines: faint dashed tumour lines, then unselected / selected
    for (i, sh) in app.shapes.iter().enumerate() {
        if joined::all_tumours(sh).iter().any(|tm| tm.on)
            && (app.sels.contains(&i) || app.show_lines)
        {
            let mut plain = sh.clone();
            plain.tumour = None;
            plain.tumours.clear();
            let color = if app.sels.contains(&i) {
                Color32::from_rgb(0xe8, 0x9a, 0x9a)
            } else {
                Color32::from_rgb(0xef, 0xc0, 0xc0)
            };
            paint_path(app, painter, rect, &plain, color, 1.0);
        }
    }
    if app.show_lines {
        for (i, sh) in app.shapes.iter().enumerate() {
            if !app.sels.contains(&i) {
                paint_path(
                    app,
                    painter,
                    rect,
                    sh,
                    Color32::from_rgb(0xc0, 0x39, 0x2b),
                    1.0,
                );
            }
        }
    }
    for &i in &app.sels {
        if let Some(sh) = app.shapes.get(i) {
            paint_path(
                app,
                painter,
                rect,
                sh,
                Color32::from_rgb(0xff, 0x1f, 0x1f),
                2.0,
            );
        }
    }
    roll_funnel::paint_parts(app, painter, rect);
    if let Some(sel) = app.selected() {
        if sel.kind == Kind::Custom {
            crate::roll_live::paint_picked_stroke(app, painter, rect, sel);
            if app.tool != Tool::Text {
                crate::roll_custom::paint_custom_box(app, painter, rect, sel);
            }
        }
        paint_handles(app, painter, rect, sel);
    }
    if let Some(d) = &app.draft {
        paint_path(
            app,
            painter,
            rect,
            d,
            Color32::from_rgb(0x0a, 0x8f, 0x0a),
            2.0,
        );
        paint_draft_points(app, painter, rect, d);
    }

    paint_keyboard(app, painter, rect);
    paint_ruler(app, painter, rect);
    paint_playhead(app, painter, rect);
    crate::roll_text::paint_text_caret(app, painter, rect);
}

/// Painter fallback path (no wgpu render state): visibility filtering + decimation when there
/// are too many. The GPU path in note_gpu.rs does not decimate.
/// Green preview of the notes the shape being drawn would make (upstream draws
/// the draft's notes on top, in DRAFT_COLOR). Skipped for huge drafts, which
/// preview as their outline only (upstream PREVIEW_LIMIT).
fn paint_draft_notes(app: &App, painter: &egui::Painter, rect: Rect) {
    const PREVIEW_LIMIT: i64 = 200_000;
    let Some(d) = &app.draft else {
        return;
    };
    if app.note_count(d) > PREVIEW_LIMIT {
        return;
    }
    let ppq = app.ppq as f64;
    let notes = engine::shape_notes(d, ppq, app.keys);
    let v = &app.view;
    let t_lo = v.b_of(v.kb_w) * ppq;
    let t_hi = v.b_of(v.w) * ppq;
    let (p_lo, p_hi) = v.visible_pitches();
    for n in &notes {
        if (n.end as f64) < t_lo
            || (n.start as f64) > t_hi
            || (n.key as i64) < p_lo
            || (n.key as i64) > p_hi
        {
            continue;
        }
        let x0 = v.x_of(n.start as f64 / ppq);
        let x1 = v.x_of(n.end as f64 / ppq);
        let lx0 = x0.round().max(v.kb_w - 2.0);
        let lx1 = x1.round().min(v.w + 2.0);
        if lx1 < v.kb_w || lx0 > v.w {
            continue;
        }
        let (y0, y1) = v.row_y(n.key as f64);
        let (fill, outline) = DRAFT_COLOR;
        let level = (n.vel.clamp(0, 127) / 4) as f32;
        let fill = fade(fill, 1.0 - level * 4.0 / 124.0);
        let r = Rect::from_min_max(
            Pos2::new(rect.min.x + lx0, rect.min.y + y0),
            Pos2::new(
                rect.min.x + lx1.max(lx0 + 1.0),
                rect.min.y + y1.max(y0 + 1.0),
            ),
        );
        painter.rect_filled(r, 0.0, fill);
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
    }
}

fn paint_notes(app: &App, painter: &egui::Painter, rect: Rect, area: Rect) {
    let v = &app.view;
    let ppq = app.ppq as f64;
    let t_lo = v.b_of(v.kb_w) * ppq;
    let t_hi = v.b_of(v.w) * ppq;
    let (p_lo, p_hi) = v.visible_pitches();
    let mut visible: Vec<&Note> = app
        .rendered
        .iter()
        .filter(|n| {
            n.end as f64 >= t_lo
                && n.start as f64 <= t_hi
                && (n.key as i64) >= p_lo
                && (n.key as i64) <= p_hi
        })
        .collect();
    // Decimate by stride when there are too many (upstream uses per-pixel images; simple decimation for now)
    let cap = 40_000usize;
    let stride = visible.len().div_ceil(cap).max(1);
    if stride > 1 {
        visible = visible.into_iter().step_by(stride).collect();
    }
    let selected_owners: std::collections::BTreeSet<u32> =
        app.sels.iter().map(|&i| i as u32).collect();
    for n in &visible {
        let x0 = v.x_of(n.start as f64 / ppq);
        let x1 = v.x_of(n.end as f64 / ppq);
        let lx0 = x0.round().max(v.kb_w - 2.0);
        let lx1 = x1.round().min(v.w + 2.0);
        if lx1 < v.kb_w || lx0 > v.w {
            continue;
        }
        let (y0, y1) = v.row_y(n.key as f64);
        let (fill, outline) = if selected_owners.contains(&n.owner) {
            SELECTED_COLOR
        } else {
            SLOT_COLORS[n.slot as usize % SLOT_COLORS.len()]
        };
        let level = (n.vel / 4) as f32;
        let fill = fade(fill, 1.0 - level * 4.0 / 124.0);
        let r = Rect::from_min_max(
            Pos2::new(rect.min.x + lx0, rect.min.y + y0),
            Pos2::new(
                rect.min.x + lx1.max(lx0 + 1.0),
                rect.min.y + y1.max(y0 + 1.0),
            ),
        );
        painter.rect_filled(r, 0.0, fill);
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
    }
    let _ = area;
}

fn paint_path(
    app: &App,
    painter: &egui::Painter,
    rect: Rect,
    sh: &Shape,
    color: Color32,
    width: f32,
) {
    let v = &app.view;
    let clip = Rect::from_min_max(
        Pos2::new(rect.min.x + v.kb_w, rect.min.y + v.ruler_h),
        rect.max,
    );
    for stroke in engine::shape_strokes(sh) {
        if stroke.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = stroke
            .iter()
            .map(|p| Pos2::new(rect.min.x + v.x_of(p[0]), rect.min.y + v.y_of(p[1])))
            .collect();
        let clipped: Vec<Pos2> = if pts.len() > 200_000 {
            pts.iter()
                .step_by(pts.len() / 200_000 + 1)
                .copied()
                .collect()
        } else {
            pts
        };
        painter.add(egui::Shape::line(clipped, Stroke::new(width, color)));
        let _ = clip;
    }
    // Straight lines closing outline gaps: faint dashed (upstream draw_path's gap_lines)
    if sh.kind == Kind::Custom && matches!(sh.fill, Fill::Fill | Fill::Spam) {
        for [a, b] in spiderweb_core::custom::gap_lines(sh) {
            let p0 = Pos2::new(rect.min.x + v.x_of(a[0]), rect.min.y + v.y_of(a[1]));
            let p1 = Pos2::new(rect.min.x + v.x_of(b[0]), rect.min.y + v.y_of(b[1]));
            painter.extend(egui::Shape::dashed_line(
                &[p0, p1],
                Stroke::new(1.0, color),
                4.0,
                3.0,
            ));
        }
    }
}

fn paint_handles(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    if sh.kind == Kind::Curve {
        paint_curve_handles(app, painter, rect, sh);
        return;
    }
    if sh.kind == Kind::Custom {
        // The box's corners / edge midpoints are in paint_custom_box; here are the stroke points and the anchors / handles of the picked curve stroke
        crate::roll_live::paint_custom_handles(app, painter, rect, sh);
        return;
    }
    if sh.kind == Kind::Funnel {
        roll_funnel::paint_funnel_handles(app, painter, rect, sh);
        return;
    }
    let r = 4.0 * app.scale();
    for (p, i) in handles(app, sh) {
        let x = rect.min.x + app.view.x_of(p[0]);
        let y = rect.min.y + app.view.y_of(p[1]);
        let free = sh.kind == Kind::Arc && i == 1;
        let outline = if free {
            Color32::from_rgb(0x00, 0x50, 0xd0)
        } else {
            Color32::from_rgb(0xc0, 0x00, 0x00)
        };
        let circle = free;
        if circle {
            let q = r + 1.5;
            painter.circle_filled(Pos2::new(x, y), q, Color32::WHITE);
            painter.circle_stroke(Pos2::new(x, y), q, Stroke::new(2.0, outline));
        } else {
            let q = Rect::from_center_size(Pos2::new(x, y), Vec2::splat(r * 2.0));
            painter.rect_filled(q, 0.0, Color32::WHITE);
            painter.rect_stroke(q, 0.0, Stroke::new(2.0, outline), egui::StrokeKind::Inside);
        }
    }
}

fn paint_draft_points(app: &App, painter: &egui::Painter, rect: Rect, d: &Shape) {
    let kind = d.kind;
    if kind == Kind::Free {
        return;
    }
    let pts: Vec<Pt> = match kind {
        Kind::Curve => vec![d.pts[0], *d.pts.last().unwrap_or(&d.pts[0])],
        Kind::Custom => {
            // Square / circle / triangle / custom shape draft: the box's four corners
            match (d.pts.first(), d.pts.get(1), d.pts.get(2)) {
                (Some(a), Some(b), Some(c)) => {
                    vec![*a, *b, [b[0] + c[0] - a[0], b[1] + c[1] - a[1]], *c]
                }
                _ => Vec::new(),
            }
        }
        _ => d.pts.clone(),
    };
    let r = 4.0 * app.scale();
    for (i, p) in pts.iter().enumerate() {
        let x = rect.min.x + app.view.x_of(p[0]);
        let y = rect.min.y + app.view.y_of(p[1]);
        if kind == Kind::Arc && i == 1 && pts.len() == 3 {
            painter.circle_filled(Pos2::new(x, y), r + 1.5, Color32::WHITE);
            painter.circle_stroke(
                Pos2::new(x, y),
                r + 1.5,
                Stroke::new(2.0, Color32::from_rgb(0x00, 0x50, 0xd0)),
            );
        } else {
            let q = Rect::from_center_size(Pos2::new(x, y), Vec2::splat(r * 2.0));
            painter.rect_filled(q, 0.0, Color32::WHITE);
            painter.rect_stroke(
                q,
                0.0,
                Stroke::new(2.0, Color32::from_rgb(0xc0, 0x00, 0x00)),
                egui::StrokeKind::Inside,
            );
        }
    }
}

fn paint_keyboard(app: &App, painter: &egui::Painter, rect: Rect) {
    let v = &app.view;
    let kb = rect.min.x + v.kb_w;
    let top = rect.min.y + v.ruler_h;
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.min.x, top), Pos2::new(kb, rect.max.y)),
        0.0,
        Color32::WHITE,
    );
    let (p_lo, p_hi) = v.visible_pitches();
    let font = FontId::proportional((v.sy as f32 * 0.6).clamp(7.0, 11.0));
    // Greyed out: with 128 keys, outside the 88-key piano; with 256 keys, outside the standard 128 (upstream roll_draw.piano_keys)
    let keys_128 = v.keys == spiderweb_core::paths::KEYS[0];
    for p in p_lo..=p_hi {
        let (y0, y1) = v.row_y(p as f64);
        let (ry0, ry1) = (rect.min.y + y0, rect.min.y + y1);
        let n = p % 12;
        let in88 = if keys_128 {
            (PIANO_88_LO..PIANO_88_HI).contains(&p)
        } else {
            p < spiderweb_core::paths::KEYS[0]
        };
        if !in88 {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(rect.min.x, ry0), Pos2::new(kb, ry1)),
                0.0,
                Color32::from_rgb(0xe2, 0xe2, 0xe2),
            );
        }
        if BLACK.contains(&n) {
            let c = if in88 {
                Color32::from_rgb(0x22, 0x22, 0x22)
            } else {
                Color32::from_rgb(0x6a, 0x6a, 0x6a)
            };
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.min.x, ry0),
                    Pos2::new(rect.min.x + v.kb_w * 0.6, ry1),
                ),
                0.0,
                c,
            );
        }
        if n == 0 || n == 5 {
            let color = if n == 0 {
                Color32::from_rgb(0x60, 0x60, 0x60)
            } else {
                Color32::from_rgb(0xb0, 0xb0, 0xb0)
            };
            painter.line_segment(
                [Pos2::new(rect.min.x, ry1), Pos2::new(kb, ry1)],
                Stroke::new(1.0, color),
            );
        }
        let show_c = n == 0 && v.sy >= 6.0;
        let show_all = !BLACK.contains(&n) && v.sy >= 16.0;
        let label = if show_c || show_all {
            Some(note_name(p))
        } else {
            None
        };
        if let Some(text) = label {
            painter.text(
                Pos2::new(kb - 3.0, (ry0 + ry1) / 2.0),
                Align2::RIGHT_CENTER,
                text,
                font.clone(),
                Color32::from_rgb(0x33, 0x33, 0x33),
            );
        }
    }
    painter.line_segment(
        [Pos2::new(kb, top), Pos2::new(kb, rect.max.y)],
        Stroke::new(1.0, Color32::from_rgb(0x80, 0x80, 0x80)),
    );
}

fn paint_ruler(app: &App, painter: &egui::Painter, rect: Rect) {
    let v = &app.view;
    let top = rect.min.y + v.ruler_h;
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, top)),
        0.0,
        Color32::from_rgb(0xf0, 0xf0, 0xf0),
    );
    painter.line_segment(
        [Pos2::new(rect.min.x, top), Pos2::new(rect.max.x, top)],
        Stroke::new(1.0, Color32::from_rgb(0x80, 0x80, 0x80)),
    );
    let beats = app.beats as f64;
    let mut step = beats.max(1.0);
    while step * v.sx < 40.0 {
        step *= 2.0;
    }
    let mut b = (v.b_of(v.kb_w).max(0.0) / step).floor() * step;
    while b <= v.b_of(v.w) {
        // Same whole-pixel rounding as the roll's grid columns, so the ruler
        // ticks and the grid lines line up exactly.
        let x = rect.min.x + v.x_of(b).round();
        if x >= rect.min.x + v.kb_w {
            painter.line_segment(
                [Pos2::new(x, top - 6.0), Pos2::new(x, top)],
                Stroke::new(1.0, Color32::from_rgb(0x55, 0x55, 0x55)),
            );
            painter.text(
                Pos2::new(x + 3.0, rect.min.y + v.ruler_h / 2.0),
                Align2::LEFT_CENTER,
                format!("{}", (b / beats) as i64 + 1),
                FontId::proportional(10.0),
                Color32::from_rgb(0x33, 0x33, 0x33),
            );
        }
        b += step;
    }
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(rect.min.x + v.kb_w, top)),
        0.0,
        Color32::from_rgb(0xe4, 0xe4, 0xe4),
    );
}

fn paint_playhead(app: &App, painter: &egui::Painter, rect: Rect) {
    let v = &app.view;
    let x = rect.min.x + v.x_of(app.playhead).round();
    if x >= rect.min.x + v.kb_w && x <= rect.max.x {
        let top = rect.min.y + v.ruler_h;
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, rect.max.y)],
            Stroke::new(1.0, Color32::from_rgb(0x0a, 0x50, 0xe0)),
        );
        let tri = vec![
            Pos2::new(x - 5.0, top - 8.0),
            Pos2::new(x + 5.0, top - 8.0),
            Pos2::new(x, top),
        ];
        painter.add(egui::Shape::convex_polygon(
            tri,
            Color32::from_rgb(0x0a, 0x50, 0xe0),
            Stroke::NONE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_both_keeps_the_pointer_anchored() {
        let mut v = View {
            t: 10.0,
            top: 60.0,
            sx: 100.0,
            sy: 10.0,
            kb_w: 50.0,
            ruler_h: 20.0,
            w: 800.0,
            h: 600.0,
            ready: true,
            keys: 128,
        };
        let pos = Pos2::new(150.0, 120.0);
        let (b0, p0) = (v.b_of(pos.x), v.p_of(pos.y));
        zoom_both(&mut v, pos, 2.0);
        assert!((v.b_of(pos.x) - b0).abs() < 1e-9);
        assert!((v.p_of(pos.y) - p0).abs() < 1e-9);
        assert_eq!(v.sx, 200.0);
        assert_eq!(v.sy, 20.0);
    }
}
