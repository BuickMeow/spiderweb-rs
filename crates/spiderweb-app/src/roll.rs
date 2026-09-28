//! 钢琴卷帘：视图（缩放 / 滚动）、绘制与鼠标交互（原版 roll/*）。

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::engine;
use spiderweb_core::shape::{Kind, Shape};

use crate::app::{App, Tool};
use crate::roll_curve::paint_curve_handles;

pub const BLACK: [i64; 5] = [1, 3, 6, 8, 10];
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
pub const PIANO_88_LO: i64 = 21;
pub const PIANO_88_HI: i64 = 109;

/// 每个通道槽的颜色（原版 roll_shared.SLOT_COLORS）。
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
#[allow(dead_code)] // 正在画的形状预览待接上
pub const DRAFT_COLOR: (Color32, Color32) = (
    Color32::from_rgb(0x9b, 0xe3, 0x9b),
    Color32::from_rgb(0x1d, 0x6b, 0x1d),
);

/// 卷帘视图状态。
#[derive(Clone, Debug)]
pub struct View {
    pub t: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
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
        (lo.max(0), hi.min(127))
    }

    pub fn set_screen(&mut self, rect: Rect) {
        self.kb_w = 56.0;
        self.ruler_h = 20.0;
        self.w = rect.width();
        self.h = rect.height();
    }

    pub fn clamp(&mut self) {
        let rows = (self.h - self.ruler_h) as f64 / self.sy;
        self.top = if rows >= 128.0 {
            127.5
        } else {
            self.top.clamp(rows - 0.5, 127.5)
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
        self.sy = (self.h - self.ruler_h) as f64 / 128.0;
        self.top = 127.5;
        self.ready = true;
    }

    #[allow(dead_code)] // 供测试/后续调用
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

/// 左键拖动状态机（原版 self.drag）。
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
    },
    Handle {
        i: usize,
    },
}

struct Inputs {
    pos: Option<Pos2>,
    interact: Option<Pos2>,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    secondary_pressed: bool,
    secondary_released: bool,
    #[allow(dead_code)] // 右拖试听待移植
    secondary_down: bool,
    middle_pressed: bool,
    middle_released: bool,
    double: bool,
    ctrl: bool,
    shift: bool,
    alt: bool,
    scroll: Vec2,
}

fn inputs(ui: &egui::Ui) -> Inputs {
    ui.input(|i| Inputs {
        pos: i.pointer.hover_pos(),
        interact: i.pointer.interact_pos(),
        primary_pressed: i.pointer.primary_pressed(),
        primary_released: i.pointer.primary_released(),
        primary_down: i.pointer.primary_down(),
        secondary_pressed: i.pointer.secondary_pressed(),
        secondary_released: i.pointer.secondary_released(),
        secondary_down: i.pointer.secondary_down(),
        middle_pressed: i.pointer.button_pressed(egui::PointerButton::Middle),
        middle_released: i.pointer.button_released(egui::PointerButton::Middle),
        double: i
            .pointer
            .button_double_clicked(egui::PointerButton::Primary),
        ctrl: i.modifiers.command || i.modifiers.ctrl,
        shift: i.modifiers.shift,
        alt: i.modifiers.alt,
        scroll: i.smooth_scroll_delta,
    })
}

/// 卷帘入口：布局、输入、绘制。
pub fn roll_ui(app: &mut App, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    app.view.set_screen(rect);
    if !app.view.ready && rect.width() > 50.0 {
        app.view.fit_shapes(&app.shapes, app.beats);
    }
    if !app.view.ready {
        return;
    }
    let input = inputs(ui);
    handle_input(app, &input, rect);
    let _ = response;
    paint(app, &painter, rect);
    if app.position.is_some() {
        // 位置文本在下一帧由输入处理清除
    }
}

// ---------------------------------------------------------------- 坐标与命中

fn event_pt(app: &App, p: Pos2, snap: bool, shift: bool) -> Pt {
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
    [b.max(0.0), q.clamp(0.0, 127.0)]
}

/// 选中形状的可拖点 (beat, pitch, 序号)；曲线用 roll_curve 的钢笔把手。
fn handles(app: &App, sh: &Shape) -> Vec<(Pt, usize)> {
    if sh.kind == Kind::Curve {
        return app.curve_handle_indices(sh);
    }
    if sh.kind == Kind::Custom {
        return Vec::new(); // 自定义形状的把手待移植
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

fn hit_handle(app: &App, p: Pos2) -> Option<usize> {
    let sh = app.selected()?;
    if sh.kind == Kind::Curve {
        // 曲线：锚点 / 拉出的手柄任何工具都能抓，两端留给 Select（原版 curve_handles 的 free）
        return app.curve_hit_handle(sh, p, app.tool == Tool::Select);
    }
    let near = 7.0_f32.max(8.0 * app.scale());
    for (pt, i) in handles(app, sh).into_iter().rev() {
        let x = app.view.x_of(pt[0]);
        let y = app.view.y_of(pt[1]);
        if (x - p.x).abs() <= near && (y - p.y).abs() <= near {
            return Some(i);
        }
    }
    None
}

/// 点到形状折线的距离（屏幕坐标）。
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
    for i in (0..app.shapes.len()).rev() {
        let sh = &app.shapes[i];
        let mut strokes = engine::shape_strokes(sh);
        if sh.tumour.as_ref().map(|t| t.on).unwrap_or(false) {
            let mut plain = sh.clone();
            plain.tumour = None;
            strokes.extend(engine::shape_strokes(&plain));
        }
        if stroke_hit(app, &strokes, p) {
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------- 输入

fn handle_input(app: &mut App, input: &Inputs, rect: Rect) {
    let (kb_w, ruler_h) = (app.view.kb_w, app.view.ruler_h);
    let pos_in_roll = move |p: Pos2| rect.contains(p) && p.x >= kb_w && p.y >= ruler_h;
    if let Some(pos) = input.pos
        && pos_in_roll(pos)
    {
        app.position = Some(position_text(app, pos));
    }
    if input.primary_pressed
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
        on_release(app, pos, input);
    }
    if input.double
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        on_double(app, pos, input.shift);
    }
    if input.secondary_pressed
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        on_right(app, pos);
    }
    if input.secondary_released
        && let Some(pos) = input.pos
        && rect.contains(pos)
    {
        on_right_release(app, pos);
    }
    if input.middle_pressed
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
            on_middle_release(app, pos, input.shift);
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
    // 先完成"点一下开始、跟随鼠标"的形状（原版 follow）
    if let Some(follow) = app.follow.take() {
        app.drag = Some(follow);
        on_drag(app, pos, input);
        on_release(app, pos, input);
        return;
    }
    if pos.x < app.view.kb_w || !app.view.ready {
        return;
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
    let pt = event_pt(app, pos, true, input.shift);

    if app.draft.is_none()
        && let Some(i) = hit_handle(app, pos)
    {
        app.push_undo();
        app.drag = Some(Drag::Handle { i });
        return;
    }

    match app.tool {
        Tool::Select => {
            let i = hit_shape(app, pos);
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
            app.push_undo();
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
            });
        }
        Tool::Poly => {
            if app.draft.is_none() {
                let defaults = app.defaults.clone();
                app.draft = Some(engine::make_shape(Kind::Poly, &[pt, pt], &defaults));
            } else if let Some(d) = app.draft.as_mut() {
                d.pts.pop();
                d.pts.push(pt);
                d.pts.push(pt);
            }
            app.drag = Some(Drag::Segment { screen: pos });
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
            // 拖拽创建 S 曲线，松手提交（拖出两端；手柄编辑在形状选中后）
            let defaults = app.defaults.clone();
            app.draft = Some(engine::make_shape(Kind::Curve, &[pt, pt], &defaults));
            app.drag = Some(Drag::Create {
                start: pt,
                screen: pos,
            });
        }
        Tool::Square | Tool::Circle | Tool::Triangle | Tool::Custom | Tool::Funnel | Tool::Text => {
            app.status = format!("{} 工具待移植", app.tool.label());
        }
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
        Drag::Handle { i } => {
            let pt = event_pt(app, pos, true, input.shift);
            if app
                .selected()
                .map(|sh| sh.kind == Kind::Curve)
                .unwrap_or(false)
            {
                app.curve_drag(i, pt, input.alt);
            } else if let Some(sh) = app.selected_mut()
                && i < sh.pts.len()
            {
                sh.pts[i] = pt;
            }
            app.shapes_changed();
        }
        Drag::Move {
            start,
            orig,
            one,
            moved,
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
                let pt = event_pt(app, pos, false, input.shift);
                if let Some(d) = app.draft.as_mut() {
                    d.pts.push(pt);
                }
                app.drag = Some(Drag::Free { last: pos });
            }
        }
        Drag::Segment { .. } => {
            let pt = event_pt(app, pos, true, input.shift);
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
        Drag::Create { start, .. } => {
            let pt = event_pt(app, pos, true, input.shift);
            let kind = app.draft.as_ref().map(|d| d.kind).unwrap_or(Kind::Line);
            let defaults = app.defaults.clone();
            let mut sh = if kind == Kind::Curve {
                let mut c = engine::make_shape(Kind::Curve, &[start, pt], &defaults);
                // 温和 S 曲线：与 make_shape 一致
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
    }
}

fn on_release(app: &mut App, pos: Pos2, input: &Inputs) {
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
        Drag::Move { moved, one, .. } => {
            if !moved
                && let Some(one) = one
                && app.sels.len() > 1
            {
                app.select(Some(one), false);
            }
        }
        Drag::Create { screen, .. } => {
            let still = (pos - screen).length() < 4.0;
            if still {
                app.follow = Some(Drag::Create {
                    start: event_pt(app, pos, true, input.shift),
                    screen: pos,
                });
            } else if app.confirm_big_draft() {
                app.commit_draft();
            }
        }
        Drag::Segment { screen } => {
            let still = (pos - screen).length() < 4.0;
            if !still {
                let pt = event_pt(app, pos, true, input.shift);
                if let Some(d) = app.draft.as_mut() {
                    if let Some(last) = d.pts.last_mut() {
                        *last = pt;
                    }
                    d.pts.push(pt);
                }
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
        Drag::Handle { .. } => {
            app.shapes_changed();
        }
    }
    if matches!(app.tool, Tool::Poly) && app.draft.is_some() {
        // 继续等下一个点
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
    // 双击选中的曲线：在曲线上离鼠标最近的地方加锚点（中键同理，见 on_middle_release）
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

/// 中键单击（没有拖动，原版 on_middle_release）在选中的曲线上加锚点。
fn on_middle_release(app: &mut App, pos: Pos2, shift: bool) {
    let clicked = matches!(
        &app.drag,
        Some(Drag::Pan { start, .. }) if (pos - *start).length() <= 3.0
    );
    if !clicked || app.draft.is_some() || pos.x < app.view.kb_w || pos.y < app.view.ruler_h {
        return;
    }
    if app
        .selected()
        .map(|sh| sh.kind == Kind::Curve)
        .unwrap_or(false)
    {
        let pt = event_pt(app, pos, true, shift);
        app.curve_click(pos, pt, Some((12.0 * app.scale()) as f64));
    }
}

fn on_right(app: &mut App, pos: Pos2) {
    app.right_done = false;
    let is_poly = app
        .draft
        .as_ref()
        .map(|d| d.kind == Kind::Poly)
        .unwrap_or(false);
    let is_arc = app
        .draft
        .as_ref()
        .map(|d| d.kind == Kind::Arc)
        .unwrap_or(false);
    if app.follow.is_some() {
        app.cancel_draft();
    } else if is_poly {
        finish_poly(app);
    } else if is_arc {
        app.cancel_draft();
    }
    // 右键曲线锚点 = 删掉，手柄 = 收回锚点（端点不动，松开时走取消选择）
    if app.draft.is_none()
        && let Some(sh) = app.selected()
        && sh.kind == Kind::Curve
        && let Some(i) = app.curve_hit_handle(sh, pos, true)
        && app.curve_delete_handle(i)
    {
        app.right_done = true;
    }
}

fn on_right_release(app: &mut App, pos: Pos2) {
    if std::mem::take(&mut app.right_done) {
        return; // 按下时已处理（曲线删点 / 收手柄）
    }
    if app.draft.is_none() && !app.sels.is_empty() {
        let i = hit_shape(app, pos);
        if i.is_none() {
            app.select(None, false);
        }
    }
}

fn on_wheel(app: &mut App, pos: Pos2, input: &Inputs) {
    let up = input.scroll.y > 0.0;
    let f = if up { 1.25 } else { 0.8 };
    let zoom_time = input.ctrl && !input.alt;
    let zoom_pitch = (input.ctrl && !input.shift) || input.alt;
    if zoom_time {
        let b = app.view.b_of(pos.x);
        app.view.sx = (app.view.sx * f).clamp(0.05, 100000.0);
        app.view.t = b - (pos.x - app.view.kb_w) as f64 / app.view.sx;
    }
    if zoom_pitch {
        let p = app.view.p_of(pos.y);
        app.view.sy = (app.view.sy * f).clamp(1.0, 60.0);
        app.view.top = p + (pos.y - app.view.ruler_h) as f64 / app.view.sy;
    }
    if !(zoom_time || zoom_pitch) {
        if input.shift {
            app.view.t += if up { -120.0 } else { 120.0 } / app.view.sx;
        } else {
            app.view.top += if up { 3.0 } else { -3.0 };
        }
    }
    app.view.clamp();
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
    if (0..=127).contains(&p) {
        text += &format!("     {} ({})", note_name(p), p);
    }
    text
}

pub fn note_name(p: i64) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p / 12 - 1)
}

// ---------------------------------------------------------------- 绘制

fn fade(c: Color32, amount: f32) -> Color32 {
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

    // 行：黑键底色、白键分隔线、C 线
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
    if v.y_of(127.5) > v.ruler_h {
        let (y0, _) = v.row_y(127.0);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(area.min.x, area.min.y),
                Pos2::new(area.max.x, rect.min.y + y0),
            ),
            0.0,
            Color32::from_rgb(0xec, 0xec, 0xec),
        );
    }

    // 列：吸附线、拍线、小节线
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

    // 音符
    if app.show_notes {
        paint_notes(app, painter, rect, area);
    }

    // 形状线：肿瘤的淡虚线，然后未选中 / 选中
    for (i, sh) in app.shapes.iter().enumerate() {
        if sh.tumour.as_ref().map(|t| t.on).unwrap_or(false)
            && (app.sels.contains(&i) || app.show_lines)
        {
            let mut plain = sh.clone();
            plain.tumour = None;
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
    if let Some(sel) = app.selected() {
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
}

fn paint_notes(app: &App, painter: &egui::Painter, rect: Rect, area: Rect) {
    let v = &app.view;
    let ppq = app.ppq as f64;
    let t_lo = v.b_of(v.kb_w) * ppq;
    let t_hi = v.b_of(v.w) * ppq;
    let (p_lo, p_hi) = v.visible_pitches();
    let mut visible: Vec<&[i64; 6]> = app
        .rendered
        .iter()
        .filter(|n| n[1] as f64 >= t_lo && n[0] as f64 <= t_hi && n[2] >= p_lo && n[2] <= p_hi)
        .collect();
    // 太多时按步长抽稀（原版用图片逐像素，这里先简单抽稀）
    let cap = 40_000usize;
    let stride = visible.len().div_ceil(cap).max(1);
    if stride > 1 {
        visible = visible.into_iter().step_by(stride).collect();
    }
    let selected_owners: std::collections::BTreeSet<i64> =
        app.sels.iter().map(|&i| i as i64).collect();
    for n in &visible {
        let x0 = v.x_of(n[0] as f64 / ppq);
        let x1 = v.x_of(n[1] as f64 / ppq);
        let lx0 = x0.round().max(v.kb_w - 2.0);
        let lx1 = x1.round().min(v.w + 2.0);
        if lx1 < v.kb_w || lx0 > v.w {
            continue;
        }
        let (y0, y1) = v.row_y(n[2] as f64);
        let (fill, outline) = if selected_owners.contains(&n[5]) {
            SELECTED_COLOR
        } else {
            SLOT_COLORS[(n[4].unsigned_abs() as usize) % SLOT_COLORS.len()]
        };
        let level = (n[3].clamp(0, 127) / 4) as f32;
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
}

fn paint_handles(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    if sh.kind == Kind::Curve {
        paint_curve_handles(app, painter, rect, sh);
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
    for p in p_lo..=p_hi {
        let (y0, y1) = v.row_y(p as f64);
        let (ry0, ry1) = (rect.min.y + y0, rect.min.y + y1);
        let n = p % 12;
        let in88 = (PIANO_88_LO..PIANO_88_HI).contains(&p);
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
        let x = rect.min.x + v.x_of(b);
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
