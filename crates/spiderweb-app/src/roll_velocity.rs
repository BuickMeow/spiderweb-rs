//! 力度面板（原版 window/velocity.py）：卷帘下方的力度条，每个音符在起点一根竖条、门限长一道横帽。
//!
//! 在面板上拖动画线（Linear / Curve / Pencil）改速度包络；选中形状时只改它的音符（其它淡显），
//! 没选中时拖动覆盖到的每个音符都改。与卷帘共享 x 轴（`App::view` 的 t / sx / kb_w），滚动缩放各自独立。

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

/// 左侧刻度（原版 LEVELS）。
const LEVELS: [i64; 5] = [127, 96, 64, 32, 0];
/// 曲线取的点数（原版 CURVE_STEPS）。
const CURVE_STEPS: usize = 48;
/// 画线的红色。
const RED: Color32 = Color32::from_rgb(0xd0, 0x00, 0x00);

/// 图层里的固定编号：淡显槽、正常槽、选中形状、正在画的形状（原版 NORMAL / SELECTED / DRAFT）。
const NORMAL: usize = roll::SLOT_COLORS.len();
const SELECTED: usize = 2 * roll::SLOT_COLORS.len();
const DRAFT: usize = 2 * roll::SLOT_COLORS.len() + 1;

/// 力度面板的工具（原版 app.vel_tool：line / curve / pencil）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum VelTool {
    #[default]
    Line,
    Curve,
    Pencil,
}

/// 拖动的种类：画新的线 / 曲线 / 铅笔，或拖已画线的把手。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EditKind {
    Drag(VelTool),
    Handle(HandleKind),
}

/// 线 / 曲线的把手：中间（弯度）、两端。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HandleKind {
    Mid,
    A,
    B,
}

/// 已画完的线 / 曲线：把手还能改它时的现场（原版 self.curve）。
#[derive(Clone, Debug)]
struct LiveCurve {
    a: Pt,
    b: Pt,
    c: Pt,
    kind: VelTool,
    /// 写进各形状的结果；用来判断形状有没有被别处改过
    done: Vec<DoneShape>,
}

/// 曲线写进一个形状的结果（原版 cv["done"][i] = (sh, env, base, span)）。
#[derive(Clone, Debug, PartialEq)]
struct DoneShape {
    i: usize,
    /// 写回的包络
    env: Vec<Pt>,
    /// 画之前形状的包络（再弯一次从这里重来）
    base: Vec<Pt>,
    /// 形状的时间跨度（beat）
    span: (f64, f64),
}

/// 一次拖动（原版 self.edit）。
#[derive(Clone, Debug)]
struct Edit {
    kind: EditKind,
    start: Pt,
    last: Pt,
    /// 画出的 (beat, velocity) 折线
    drawn: Option<Vec<Pt>>,
    /// 每条 rendered 音符的预览力度，-1 = 没画到（原版 preview）
    preview: Option<Vec<i64>>,
    /// 这次拖动会写到的形状
    owners: BTreeSet<usize>,
    /// pencil 的红色鼠标轨迹（面板局部坐标）
    trail: Vec<Pos2>,
    /// 吸附点：选中形状第一 / 最后一个音符的起点（beat）
    ends: Vec<f64>,
    /// 当前显示的线 / 曲线
    curve: Option<(Pt, Pt, Pt)>,
    /// 拖把手时只改这些形状（原版 mark 的 only）
    only: Option<BTreeSet<usize>>,
    /// 拖把手时的现场
    handle: Option<HandleDrag>,
}

#[derive(Clone, Debug)]
struct HandleDrag {
    kind: VelTool,
    done: Vec<DoneShape>,
}

/// 面板状态（原版 VelocityPane 的 self.edit / self.curve / self._pan + app.vel_tool）。
#[derive(Clone, Debug, Default)]
pub struct VelocityState {
    tool: VelTool,
    edit: Option<Edit>,
    curve: Option<LiveCurve>,
    /// 中键平移：按下时的 x 与 view.t（原版 self._pan）
    pan: Option<(f32, f64)>,
}

impl VelocityState {
    /// Enter = 完成最后的线 / 曲线：把手消失。有曲线时返回 true（原版 confirm）。
    pub fn confirm(&mut self) -> bool {
        self.curve.take().is_some()
    }
}

// ---------------------------------------------------------------- 纯逻辑（可单测）

/// 从 a 到 b 的直线拖动（beat, velocity），两端各伸出 pad 个 beat（原版 segment）。
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

/// 从 a 到 b、向 c 弯的曲线（beat, velocity），两端各伸出 pad 个 beat（原版 curve_env）。
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

/// 把 c 的力度收进刚好让 a → b 的曲线不超出 1..127（形状不变、不出现平顶）（原版 limit_bend）。
fn limit_bend(a: Pt, b: Pt, c: Pt) -> Pt {
    let hi = 127.0 + ((127.0 - a[1]) * (127.0 - b[1])).sqrt();
    let lo = 1.0 - ((a[1] - 1.0) * (b[1] - 1.0)).sqrt();
    [c[0], c[1].clamp(lo, hi)]
}

/// 曲线中点（画弯度把手的位置）（原版 curve_mid）。
fn curve_mid(a: Pt, b: Pt, c: Pt) -> Pt {
    [
        (a[0] + 2.0 * c[0] + b[0]) / 4.0,
        (a[1] + 2.0 * c[1] + b[1]) / 4.0,
    ]
}

/// 形状的力度跨度：所有笔画点的 beat 最小 / 最大。
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

/// 把画出的 (beat, velocity) 折线写进形状的包络（原版 commit 的逐形状部分），返回 (env, span)。
///
/// `base`：起点包络——拖把手时用曲线画之前的包络（重来一次），否则用形状当前的包络。
fn apply_env(sh: &mut Shape, drawn: &[Pt], base: &[Pt]) -> (Vec<Pt>, (f64, f64)) {
    let (lo, hi) = shape_span(sh);
    let env = if hi > lo {
        let pts: Vec<Pt> = drawn
            .iter()
            .map(|p| [(p[0] - lo) / (hi - lo), p[1]])
            .collect();
        tidy_env(&paint_env(base, &pts))
    } else {
        // 每个音符都在形状起点：整条包络就一个点
        vec![[
            0.0,
            round_half_even(env_at(drawn, lo, false)).clamp(1.0, 127.0),
        ]]
    };
    sh.vel_env = env.clone();
    sh.own_vel = false; // 粘贴音符自己的力度被替换
    sh.vel0 = round_half_even(env_at(&env, 0.0, false)).clamp(1.0, 127.0);
    sh.vel1 = round_half_even(env_at(&env, 1.0, false)).clamp(1.0, 127.0);
    (env, (lo, hi))
}

// ---------------------------------------------------------------- 面板几何

/// 面板的屏幕几何（力度轴与 x 轴）。
#[derive(Clone, Copy)]
struct Pane {
    rect: Rect,
    /// 127 上面的空隙（原版 self.top）
    top: f32,
    h: f32,
    /// 速度 0 的位置：底边上面也留同样空隙
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

    /// 图像区宽度（卷帘 x 轴重叠的部分）。
    fn image_w(self) -> f32 {
        self.w - self.kb
    }

    /// 面板局部坐标 -> 屏幕坐标。
    fn pos(self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.rect.min.x + x, self.rect.min.y + y)
    }

    /// 屏幕坐标 -> 面板局部坐标。
    fn local(self, p: Pos2) -> Pos2 {
        p - self.rect.min.to_vec2()
    }

    /// 力度 -> y（原版 v2y）。
    fn v2y(self, v: f64) -> f32 {
        (self.bottom as f64 - v / 127.0 * (self.bottom - self.top) as f64).round() as f32
    }

    /// y -> 力度（夹在 1..127）（原版 y2v）。
    fn y2v(self, y: f32) -> f64 {
        let span = (self.bottom - self.top).max(1.0) as f64;
        ((self.bottom - y) as f64 / span * 127.0).clamp(1.0, 127.0)
    }

    /// 铅笔红线的鼠标位置：y 收进力度范围内（原版 trail_pt）。
    fn trail_pt(self, local: Pos2) -> Pos2 {
        Pos2::new(local.x, local.y.clamp(self.v2y(127.0), self.v2y(1.0)))
    }
}

/// 鼠标键盘输入快照。
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

/// 面板入口：工具条、键盘、输入、绘制。
pub fn velocity_ui(app: &mut App, ui: &mut egui::Ui) {
    velocity_toolbar(app, ui);
    // Enter = 完成最后的线 / 曲线（原版 pianoroll.on_key -> vel.confirm）
    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
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
    let input = read_input(ui);
    handle_input(app, &input, pane);
    set_cursor(app, &input, pane, ui.ctx());
    paint(app, &painter, pane);
}

/// 顶部小工具条（原版 vbar）。
fn velocity_toolbar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label("Velocity");
        for (tool, label) in [
            (VelTool::Line, "Linear"),
            (VelTool::Curve, "Curve"),
            (VelTool::Pencil, "Pencil"),
        ] {
            if ui.selectable_label(app.vel.tool == tool, label).clicked() {
                app.vel.tool = tool;
            }
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(
                "Ctrl = flat · Shift = snap · Enter = done · select a shape to edit only its notes",
            )
            .weak()
            .size(10.0),
        );
    });
}

/// 悬停时的手形 / 十字光标（原版 on_motion）。
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

// ---------------------------------------------------------------- 坐标与吸附

/// 屏幕位置 -> (beat, velocity)（原版 event_pt）。
fn event_pt(app: &App, pane: Pane, local: Pos2) -> Pt {
    [app.view.b_of(local.x), pane.y2v(local.y)]
}

/// 拖动端点的吸附点：选中形状第一 / 最后一个音符的起点（原版 snap_spots）。
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

/// b 的吸附：Shift 按住时先吸到选中的两端（屏幕上 10 * scale 内），否则吸到网格（原版 snap_time）。
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

// ---------------------------------------------------------------- 编辑

/// 最后的线 / 曲线，若它的形状没被别处改过（把手还能动）（原版 live_curve 的判定）。
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

/// 还能动的最后一条线 / 曲线（工具换了或形状变了就不算）。
fn live_curve(app: &App) -> Option<LiveCurve> {
    app.vel
        .curve
        .as_ref()
        .filter(|cv| app.vel.tool == cv.kind && curve_still_live(app, cv))
        .cloned()
}

/// 鼠标在曲线的哪个把手上（原版 near_handle）。
fn near_handle(app: &App, cv: &LiveCurve, local: Pos2, pane: Pane) -> Option<HandleKind> {
    let near = 7.0 * app.scale();
    let mid = curve_mid(cv.a, cv.b, cv.c);
    for (which, p) in [
        (HandleKind::Mid, mid),
        (HandleKind::A, cv.a),
        (HandleKind::B, cv.b),
    ] {
        if which == HandleKind::Mid && cv.kind == VelTool::Line {
            continue; // 直线保持直
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
                // 弯度把手保持在两端之间
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
                // 平着起步，像 ease-in
                [(a[0] + t) / 2.0, a[1]]
            } else {
                if input.ctrl {
                    v = a[1]; // 完全水平
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
        // 拖把手：曲线还活着才写回，撤销步仍算原来的那条曲线（原版 on_release）
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
    app.push_undo();
    let kind = match ed.kind {
        EditKind::Drag(t) => t,
        EditKind::Handle(_) => app.vel.tool,
    };
    let done = commit(app, &drawn, &ed.owners, None);
    if let Some((a, b, c)) = ed.curve
        && a[0] != b[0]
    {
        // 把手还能动，直到别处变化
        app.vel.curve = Some(LiveCurve {
            a,
            b,
            c,
            kind,
            done,
        });
    }
}

/// 铅笔：把从 last 到 pt 的一段接进画出的折线（后画的盖住先画的）（原版 extend）。
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

/// 让这次拖动成为从 a 到 b、向 c 弯的曲线（原版 draw_curve）。
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

/// 记下 seg 覆盖到的可编辑音符的力度预览与它们的形状（后记的盖住先记的）（原版 mark）。
fn mark(app: &mut App, seg: &[Pt], only: Option<&BTreeSet<usize>>) {
    if seg.is_empty() {
        return;
    }
    let ppq = app.ppq.max(1) as f64;
    let lo = seg[0][0] * ppq;
    let hi = seg[seg.len() - 1][0] * ppq;
    let rendered_len = app.rendered.len();
    let sels = app.sels.clone();
    // 分开借 rendered 与 vel（同一结构体的不同字段）
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

/// 把画出的力度写进每个 owner 形状（原版 commit）。返回各形状的现场。
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
    app.panel_sel = None; // 侧栏的 vel0/vel1 跟着刷新
    done
}

// ---------------------------------------------------------------- 输入分发

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
        // 右键：取消正在画的线 / 曲线，并取消选择（原版 on_right）
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

/// 滚轮左右滚，Ctrl+滚轮缩放时间（与卷帘一致，原版 on_wheel）。
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

/// 状态栏位置文本："bar:beat:tick (tick N)     velocity V"（原版 on_motion / time_text）。
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

/// canvas x 处的 bar:beat:tick（与卷帘共用一个 x 轴）。
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

// ---------------------------------------------------------------- 绘制

/// 各图层的（填充色, 边框色），从低到高（原版 LAYERS）。
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

/// 力度竖条：音符起点一列、顶在力度上；门限够长时在力度处横一道帽（原版 bars）。
struct Bar {
    layer: usize,
    /// 图像区局部 x（已减去 kb_w）
    x0: f32,
    x1: f32,
    /// 力度对应的 y
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
            slot // 没选中的淡显
        };
        push(n, layer, vel, &mut out);
    }
    if let Some(d) = &app.draft {
        for n in engine::shape_notes(d, ppq) {
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
    // 低图层先画；同一列同层里力度高的盖住低的
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

/// 卷帘的竖直网格线（简化版 grid_cols）：吸附线、拍线、小节线。
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

/// 画一条线 / 曲线：红线 + 两端方块把手 +（弯的）中间的圆把手（原版 draw_curve_line）。
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

    // 力度档位线：96 / 64 / 32 淡，127 / 0 深
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

    // 竖直网格
    for (x, color) in grid_columns(app, pane.w) {
        if x >= pane.kb - 1.0 && x <= pane.w {
            painter.line_segment(
                [pane.pos(x, pane.top), pane.pos(x, pane.h)],
                Stroke::new(1.0, color),
            );
        }
    }

    // 力度条
    paint_bars(app, painter, pane);

    // 左侧力度刻度条
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

    // 正在画的线 / 曲线 / 铅笔轨迹，或最后一条还能动的线 / 曲线
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

    // 播放线（与卷帘同步）
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
        // 同一个 beat：两点都带 pad
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
        // 再弯一次：从画之前的包络（base）重来，而不是叠在已画的结果上
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
        // 反向再画一段：仍然有序，后画的赢（起点 1.5 处已经是新值）
        let drawn = paint_env(&drawn, &segment([3.0, 30.0], [1.5, 50.0], 0.05));
        assert!(drawn.windows(2).all(|w| w[0][0] <= w[1][0]));
        assert!((env_at(&drawn, 1.5, true) - 50.0).abs() < 1e-9);
        assert!((env_at(&drawn, 0.5, false) - 10.0).abs() < 1e-9);
    }
}
