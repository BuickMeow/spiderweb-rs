//! 钢琴卷帘：视图（缩放 / 滚动）、绘制与鼠标交互（原版 roll/*）。

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::engine;
use spiderweb_core::funnel;
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
        /// 点的是唯一选中的自定义形状：松开时拾取它下面的笔画（None = 取消拾取）
        part: Option<Option<usize>>,
        /// 再次点击单个漏斗：记下它在点啥（释放时高亮 / 清空 part）。
        funnel_again: Option<usize>,
        funnel_part: Option<PartId>,
    },
    Handle {
        i: usize,
    },
    /// 文本工具点下后拖动：选到鼠标（原版 ("textsel",)）
    TextSel,
    /// 拖自定义形状的笔画把手（原版 drag 的 handle + 元组 hid）
    StrokeHandle(crate::roll_live::StrokeHandleId),
    /// 方 / 圆 / 三角拖出来的框（原版 create + draft["draw"]）
    BoxCreate {
        start: Pt,
        screen: Pos2,
        tool: Tool,
    },
    /// 放置面板里选中的自定义形状（原版 place）
    Place {
        start: Pt,
        screen: Pos2,
        aspect: Option<f64>,
    },
    /// 拖角点 / 边中缩放（原版 resize）
    Resize {
        k: usize,
        orig: Vec<Pt>,
        side: bool,
        start: Pt,
    },
    /// 边中外面斜切（原版 skew）
    Skew {
        k: usize,
        orig: Vec<Pt>,
        start: Pt,
    },
    /// 角点外面旋转（原版 turn）
    Turn {
        orig: Vec<Pt>,
        a0: f64,
    },
    /// 正在画漏斗的墙（原版 drag 的 "wall"）。
    Wall {
        screen: Pos2,
    },
}

/// 命中的把手：普通形状的点号，或自定义形状的笔画把手。
#[derive(Clone, Copy, Debug)]
pub(crate) enum HandleId {
    Point(usize),
    Stroke(crate::roll_live::StrokeHandleId),
}

/// 右键拖动的现场（原版 pianoroll 的 self._scrub）：还没拖够 4px 时 `tick` 为 None。
#[derive(Clone, Debug)]
pub struct RightDrag {
    /// 按下时的位置（卷帘局部坐标）：松开时在这里找形状开菜单
    pub start: Pos2,
    /// 开始试听后的当前 tick；None = 还没开始
    pub tick: Option<f64>,
    /// 松开时没拖动也没命中形状：取消选择（原版 deselect）
    pub deselect: bool,
    /// 按下时有没有按住 Shift（菜单里"就地加锚点"用）
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
}

/// 把指针位置换算到卷帘自己的坐标（绘制都带 rect.min，命中都在局部坐标里算）。
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
        crate::roll_menu::menu_ui(app, ui);
        return;
    }
    // 有弹出层（右键菜单 / 下拉框）开着：卷帘输入让路（原版 tk 菜单会抓走事件）
    if !crate::roll_menu::is_popup_open(ui.ctx()) {
        let input = inputs(ui);
        handle_input(app, &input, Rect::from_min_size(Pos2::ZERO, rect.size()));
        if app.drag.is_none()
            && let Some(pos) = input.pos
            && pos.x >= app.view.kb_w
            && pos.y >= app.view.ruler_h
            && pos.x <= app.view.w
            && pos.y <= app.view.h
        {
            let icon = if app.draft.is_none() && hit_handle(app, pos).is_some() {
                egui::CursorIcon::Move // 抓住把手（原版 fleur）
            } else {
                crate::roll_custom::custom_cursor(app, crate::roll_custom::custom_hit(app, pos))
            };
            ui.ctx().set_cursor_icon(icon);
        }
    }
    let _ = response;
    // GPU 音符：revision 脏了才重建 CPU instances（全部音符，不 cull；平移 / 缩放不动）
    if let Some(gpu) = &app.note_gpu {
        gpu.sync(&app.rendered, &app.sels, app.notes_revision);
    }
    paint(app, &painter, rect);
    crate::roll_menu::menu_ui(app, ui);
}

// ---------------------------------------------------------------- 坐标与命中

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
    [b.max(0.0), q.clamp(0.0, 127.0)]
}

/// 选中形状的可拖点 (beat, pitch, 序号)；曲线 / 自定义形状用各自的把手。
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
        // 曲线：锚点 / 拉出的手柄任何工具都能抓，两端留给 Select（原版 curve_handles 的 free）
        return app
            .curve_hit_handle(sh, p, app.tool == Tool::Select)
            .map(HandleId::Point);
    }
    if sh.kind == Kind::Custom {
        // 被拾取曲线笔画的锚点 / 手柄任何工具都能抓；笔画点只有 Select 工具
        return crate::roll_live::hit_stroke_handle(app, sh, p, app.tool == Tool::Select)
            .map(HandleId::Stroke);
    }
    if sh.kind == Kind::Funnel {
        // 漏斗：曲线把手 / 起点任何工具都能抓，线 / 墙的点留给 Select
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
    let (b, q) = (app.view.b_of(p.x), app.view.p_of(p.y));
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
        // 粘贴的音符：框里任何地方都算命中
        if sh.notes.is_some()
            && crate::roll_live::inside_strokes(&spiderweb_core::custom::custom_strokes(sh), b, q)
        {
            return Some(i);
        }
        // Fill / Spam 的形状：轮廓里（缺口用直线补上）都算命中
        if sh.kind == Kind::Custom && matches!(sh.fill, Fill::Fill | Fill::Spam) {
            let polys = if sh.text.is_some() {
                spiderweb_core::custom::custom_strokes(sh)
            } else {
                spiderweb_core::custom::fill_plan(sh).polys
            };
            let inside = if sh.union {
                // 重叠也填上：任意一个环里都算
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
        // 漏斗的里面也算命中（原版 hit_shape 的 funnel_contains）
        if sh.kind == Kind::Funnel
            && funnel::funnel_contains(sh, app.view.b_of(p.x), app.view.p_of(p.y))
        {
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
        on_release(app, pos, input, false);
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
    // 先完成"点一下开始、跟随鼠标"的形状（原版 follow）
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
        // Select 是启动工具，它的 tip 在卷帘上第一次点击时弹（原版 pianoroll.on_press）
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
        app.push_undo();
        app.drag = Some(match h {
            HandleId::Point(i) => Drag::Handle { i },
            HandleId::Stroke(id) => Drag::StrokeHandle(id),
        });
        return;
    }

    // 选中自定义形状的盒子：角点 / 边缩放，角外旋转，边中外斜切
    let custom_hit = crate::roll_custom::custom_hit(app, pos);
    if let Some(hit) = custom_hit
        && let Some(drag) = custom_box_drag(app, hit, pos, input.shift)
    {
        app.push_undo();
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
                i = app.sel; // 选中的自定义形状框里任何地方都算移动
            }
            // 点的是“已经唯一选中”的自定义形状：松开时拾取它下面的笔画（原版在这里先判定）
            let was_only_selected = i.is_some() && app.sel == i && app.sels.len() == 1;
            // 再次点击那个选中的漏斗：鼠标下的线 / 曲线要高亮（原版 again / part）
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
                // Ctrl+点击 part：加上 / 去掉这一个
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
            // 唯一选中的自定义形状：拾取鼠标下的笔画（离轮廓 6px 内），否则取消拾取
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
            // 拖拽创建 S 曲线，松手提交（拖出两端；手柄编辑在形状选中后）
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

/// 选中自定义形状的盒子被点中 -> 对应的拖动（盒内返回 None）。
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

/// 折线的下一个点放在 pt（后面再跟一个随鼠标的点）。true = 画完了
/// （Live 绘制：回到第一个点 = 闭合）（原版 poly_point）。
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
                    // 再次点击那个漏斗：高亮鼠标下的 part（没点到就清空）
                    let group = app
                        .shapes
                        .get(j)
                        .and_then(|sh| funnel_part.map(|p| roll_funnel::part_group(sh, p)))
                        .unwrap_or_default();
                    app.set_parts(group, funnel_part);
                } else if let Some(stroke) = part {
                    app.set_stroke(stroke); // 点一下自定义形状：拾取 / 取消拾取笔画
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
                // 画到选中漏斗的墙上 = 它的一条新线；否则留着等墙
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
            // Live 绘制：在起点附近松手 = 闭合
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
            // 框已经改好；面板的缺口 / 音符数下一帧自己算
            app.shapes_changed();
        }
        Drag::TextSel => {}
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
    // 双击文本：Select 工具 = 切到文本工具接着打；文本工具 = 选中鼠标下的词（原版 on_double）
    if app.tool == Tool::Select && crate::roll_text::text_at(app, pos).is_some() {
        crate::roll_text::text_edit(app, pos);
        return;
    }
    if app.tool == Tool::Text {
        crate::roll_text::text_double(app, pos);
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

/// 中键单击（没有拖动，原版 on_middle_release）：选中的曲线上加锚点；
/// 选中的漏斗上：在线上加起点 / 靠近曲线加锚点。
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
        // 被拾取的曲线笔画：点上加锚点
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

/// 右键按下：画一半的折线 / 弧收尾，删漏斗 / 曲线 / 笔画把手；
/// 没被处理又在卷帘区里就返回右拖试听的现场（原版 on_right）。
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
    // 右键漏斗的曲线起点 / 锚点 / 手柄：删掉或收回（原版 delete_funnel_handle）
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
    // 右键曲线锚点 = 删掉，手柄 = 收回锚点（端点不动，松开时走取消选择）
    if app.draft.is_none()
        && let Some(sh) = app.selected()
        && sh.kind == Kind::Curve
        && let Some(i) = app.curve_hit_handle(sh, pos, true)
        && app.curve_delete_handle(i)
    {
        app.right_done = true;
        return None;
    }
    // 右键被拾取曲线笔画的锚点 = 删掉，手柄 = 收回（笔画点不特殊处理）
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
        // 键盘列上：取消选择（原版 on_right 的 else 分支）
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

/// 右键拖动时的 tick（原版 scrub_tick）：键盘列以右、不小于 0。
fn scrub_tick(app: &App, pos: Pos2) -> f64 {
    let x = pos.x.max(app.view.kb_w);
    (app.view.b_of(x) * app.ppq as f64).max(0.0)
}

/// 右键拖动：先按 4px 判断是试听而不是菜单，然后扫过哪些音符就响哪些（原版 on_right_drag）。
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

/// 右键松开：拖过 = 停试听；点形状 = 菜单；点空白 = 取消选择（原版 on_right_release）。
fn on_right_release(app: &mut App) {
    let sc = app.right_drag.take();
    if std::mem::take(&mut app.right_done) {
        return; // 按下时已处理（曲线删点 / 收手柄）
    }
    let Some(sc) = sc else {
        return;
    };
    if sc.tick.is_some() {
        app.scrub_end();
        return;
    }
    if !sc.deselect {
        return; // 刚画完折线 / 弧，或取消了跟鼠标的草稿
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

/// 把颜色往白色方向混（原版 roll_shared.fade），力度面板也用。
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
        if let Some(gpu) = &app.note_gpu {
            // GPU 路径：一次 instanced draw（普通一段 + 选中一段），无抽稀
            let globals = crate::note_gpu::globals_for(&app.view, app.ppq, rect.min);
            painter.add(gpu.callback(globals, rect));
        } else {
            paint_notes(app, painter, rect, area);
        }
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

/// painter 回退路径（没有 wgpu render state 时）：可见性过滤 + 超量抽稀。
/// GPU 路径在 note_gpu.rs，不做抽稀。
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
    // 补轮廓缺口的直线：淡虚线（原版 draw_path 的 gap_lines）
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
        // 盒子的角点 / 边中在 paint_custom_box 里；这里是笔画点与被拾取曲线笔画的锚点 / 手柄
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
            // 方 / 圆 / 三角 / 自定义形状的草稿：盒子的四个角
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
