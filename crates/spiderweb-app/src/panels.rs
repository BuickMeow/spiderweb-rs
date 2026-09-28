//! 工具栏与右侧面板（原版 window/app.py 的 _build / _build_side 及各面板）。

use eframe::egui;

use spiderweb_core::funnel::{self, CURVE_PRESETS, FunnelSettings};
use spiderweb_core::shape::{
    Align, Fill, FunnelCurve, FunnelFill, GateChange, GateFollow, Kind, Stroke, TextAlign,
    TextSettings, TextUnit, Tumour, TumourShape, TumourSide, TumourWrap, WallMode,
};
use spiderweb_core::smooth::clean_level;
use spiderweb_core::text::{self, TextChange};
use spiderweb_io::project::{ChannelMode, ChannelSplit, FunnelDefaults};

use crate::app::{App, Tool};
use crate::roll_funnel;

const SNAPS: [&str; 7] = ["Off", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64"];

/// 字重选择（fonts.WEIGHTS）。
const WEIGHTS: [(i32, &str); 9] = [
    (100, "Thin"),
    (200, "Extra light"),
    (300, "Light"),
    (400, "Regular"),
    (500, "Medium"),
    (600, "Semibold"),
    (700, "Bold"),
    (800, "Extra bold"),
    (900, "Black"),
];

/// 最接近的字重名（原版 sync_text 的 min(WEIGHTS, ...)）。
fn weight_name(w: i32) -> &'static str {
    WEIGHTS
        .iter()
        .min_by_key(|(v, _)| (v - w).abs())
        .map(|(_, n)| *n)
        .unwrap_or("Regular")
}

/// 面板的一行数字框（原版 ENTRIES 的 label / unit / range / scrub 步长）。
struct NumberRow {
    label: String,
    unit: String,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
}

fn number_row(
    ui: &mut egui::Ui,
    row: NumberRow,
    changes: &mut TextChange,
    any: &mut bool,
    set: impl FnOnce(&mut TextChange, f64),
) {
    ui.horizontal(|ui| {
        ui.label(row.label);
        let mut v = row.value;
        if ui
            .add(
                egui::DragValue::new(&mut v)
                    .speed(row.speed)
                    .range(row.range)
                    .max_decimals(4),
            )
            .changed()
        {
            set(changes, v);
            *any = true;
        }
        if !row.unit.is_empty() {
            ui.weak(row.unit);
        }
    });
}

fn parse_int(text: &str, lo: i64, hi: i64) -> Option<i64> {
    spiderweb_io::mathexpr::calc_int(text, Some(lo), Some(hi)).ok()
}

/// 自由笔面板要改的东西：一个 free 形状，或唯一选中的自定义形状里被拾取的 free 笔画。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FreeTarget {
    Shape(usize),
    Stroke(usize, usize),
}

/// 肿瘤设置的一次改动：按字段单独写回，多选时各形状的其它设置不动
/// （原版 set_tumour 只设一个键）。字段名用来给同一手势的连续改动分组撤销。
#[derive(Clone, Copy, Debug, PartialEq)]
enum TumourChange {
    On(bool),
    Shape(TumourShape),
    Size(f64),
    Length(f64),
    Dist(f64),
    Ease(f64),
    Side(TumourSide),
    Wrap(TumourWrap),
    Start(f64),
    End(f64),
    Fit(bool),
    Mirror(bool),
    Seed(i64),
}

impl TumourChange {
    fn key(self) -> &'static str {
        match self {
            TumourChange::On(_) => "on",
            TumourChange::Shape(_) => "shape",
            TumourChange::Size(_) => "size",
            TumourChange::Length(_) => "length",
            TumourChange::Dist(_) => "dist",
            TumourChange::Ease(_) => "ease",
            TumourChange::Side(_) => "side",
            TumourChange::Wrap(_) => "wrap",
            TumourChange::Start(_) => "start",
            TumourChange::End(_) => "end",
            TumourChange::Fit(_) => "fit",
            TumourChange::Mirror(_) => "mirror",
            TumourChange::Seed(_) => "seed",
        }
    }

    fn apply(self, tm: &mut Tumour) {
        match self {
            TumourChange::On(v) => tm.on = v,
            TumourChange::Shape(v) => tm.shape = v,
            TumourChange::Size(v) => tm.size = v,
            TumourChange::Length(v) => tm.length = v,
            TumourChange::Dist(v) => tm.dist = v,
            TumourChange::Ease(v) => tm.ease = v,
            TumourChange::Side(v) => tm.side = v,
            TumourChange::Wrap(v) => tm.wrap = v,
            TumourChange::Start(v) => tm.start = v,
            TumourChange::End(v) => tm.end = v,
            TumourChange::Fit(v) => tm.fit = v,
            TumourChange::Mirror(v) => tm.mirror = v,
            TumourChange::Seed(v) => tm.seed = v,
        }
    }
}

/// 能带肿瘤的形状（原版 LINE_KINDS）。
fn tumour_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Line | Kind::Poly | Kind::Free | Kind::Curve | Kind::Arc
    )
}

fn tumour_shape_name(s: TumourShape) -> String {
    match s {
        TumourShape::Triangle => rust_i18n::t!("tool.triangle"),
        TumourShape::Square => rust_i18n::t!("tool.square"),
        TumourShape::Circle => rust_i18n::t!("tool.circle"),
        TumourShape::Parabola => rust_i18n::t!("panel.tumour.parabola"),
    }
    .to_string()
}

fn tumour_side_name(s: TumourSide) -> String {
    match s {
        TumourSide::Alt => rust_i18n::t!("panel.tumour.side_alt"),
        TumourSide::Left => rust_i18n::t!("panel.tumour.side_left"),
        TumourSide::Right => rust_i18n::t!("panel.tumour.side_right"),
        TumourSide::Random => rust_i18n::t!("panel.tumour.side_random"),
    }
    .to_string()
}

fn tumour_wrap_name(w: TumourWrap) -> String {
    match w {
        TumourWrap::Simple => rust_i18n::t!("panel.tumour.wrap_straight"),
        TumourWrap::Wrap => rust_i18n::t!("panel.tumour.wrap_bent"),
    }
    .to_string()
}

/// 一行肿瘤数字框；改了就把（新值, 是不是新一次拖动）记进 `changes`。
fn tumour_num_row(
    ui: &mut egui::Ui,
    row: NumberRow,
    make: impl Fn(f64) -> TumourChange,
    changes: &mut Vec<(TumourChange, bool)>,
    fresh: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label(row.label);
        let mut v = row.value;
        let resp = ui.add(
            egui::DragValue::new(&mut v)
                .speed(row.speed)
                .range(row.range)
                .max_decimals(3),
        );
        *fresh |= resp.drag_started();
        if resp.changed() {
            changes.push((make(v), *fresh));
        }
        if !row.unit.is_empty() {
            ui.weak(row.unit);
        }
    });
}

/// "New random" 的种子：1..10^9（原版 random.randrange(1, 10**9)）。
fn random_seed() -> i64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(1);
    let mut rnd = spiderweb_core::pyrandom::PyRandom::new(t);
    rnd.randbelow(999_999_999) as i64 + 1
}

/// 主题的 tip 文案（工具栏按钮的悬浮提示；找不到就是空）。
fn tip_of(topic: &str) -> &'static str {
    crate::help_texts::by_id(topic).map(|t| t.tip).unwrap_or("")
}

impl App {
    pub fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let box_tools_on = self.tool == Tool::Custom || self.tool.is_box();
            for tool in Tool::ALL {
                if tool.is_box() && !box_tools_on {
                    continue;
                }
                let selected = self.tool == tool;
                let text = format!("{} ({})", tool.ui_label(), tool.hotkey().to_uppercase());
                // 按钮上显示这个工具的 tip（原版 widgets.Tooltip）
                let tip = tip_of(crate::help::tool_topic(tool));
                if ui
                    .selectable_label(selected, text)
                    .on_hover_text(tip)
                    .clicked()
                    && self.tool != tool
                {
                    if tool != Tool::Select {
                        self.draw_tool = tool;
                    }
                    self.tool = tool;
                    self.cancel_draft();
                    // 换工具时弹这个工具的 tip（看过的不会再弹）
                    self.tips.show(crate::help::tool_topic(tool));
                }
            }
            ui.separator();
            if ui
                .checkbox(&mut self.live, rust_i18n::t!("toolbar.live_shape"))
                .on_hover_text(tip_of("live"))
                .changed()
                && self.live
            {
                self.tips.show("live");
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(rust_i18n::t!("toolbar.snap"));
            egui::ComboBox::from_id_salt("snap")
                .selected_text(self.snap.clone())
                .width(70.0)
                .show_ui(ui, |ui| {
                    for s in SNAPS {
                        if ui.selectable_label(self.snap == s, s).clicked() {
                            self.snap = s.to_string();
                        }
                    }
                });
            ui.checkbox(&mut self.show_lines, rust_i18n::t!("toolbar.show_lines"))
                .changed();
            ui.checkbox(&mut self.show_notes, rust_i18n::t!("toolbar.show_notes"));
            if ui
                .checkbox(
                    &mut self.show_velocity,
                    rust_i18n::t!("toolbar.velocity_pane"),
                )
                .changed()
                && self.show_velocity
            {
                self.tips.show("velocity");
            }
            if ui.button(rust_i18n::t!("toolbar.fit_view")).clicked() {
                self.view.fit_shapes(&self.shapes, self.beats);
            }
            if ui.button(rust_i18n::t!("toolbar.undo")).clicked() {
                self.undo();
            }
            if ui.button(rust_i18n::t!("toolbar.redo")).clicked() {
                self.redo();
            }
            let play_label = if self.player.running() {
                rust_i18n::t!("toolbar.stop")
            } else {
                rust_i18n::t!("toolbar.play")
            };
            if ui.button(play_label).clicked() {
                self.toggle_play();
            }
            if ui
                .button(rust_i18n::t!("toolbar.help"))
                .on_hover_text(rust_i18n::t!("toolbar.help_tip"))
                .clicked()
            {
                crate::help::open_help(self, None);
            }
        });
    }

    pub fn side_panel_ui(&mut self, ui: &mut egui::Ui) {
        if self.panel_sel != self.sel {
            self.panel_sel = self.sel;
            let t = self
                .selected()
                .cloned()
                .unwrap_or_else(|| self.defaults.clone());
            self.vel_text = [fmt_num(t.vel0), fmt_num(t.vel1)];
            self.refresh_custom_gate_text();
            self.sync_funnel_text();
            // 第一次选中某种形状：显示怎么编辑它（原版 sync_panel）
            self.show_kind_tip();
        }
        self.project_section(ui);
        self.shapes_section(ui);
        self.defaults_section(ui);
        self.freehand_section(ui);
        self.tumour_section(ui);
        self.text_section(ui);
        self.custom_section(ui);
        self.points_section(ui);
        self.funnel_section(ui);
        // 侧栏底部：当前工具的帮助
        crate::help::side_help_ui(self, ui);
    }

    /// 选中形状的种类对应的 tip（custom / funnel / free 各一个）。
    fn show_kind_tip(&mut self) {
        let kinds: Vec<Kind> = self
            .sels
            .iter()
            .filter_map(|&i| self.shapes.get(i).map(|sh| sh.kind))
            .collect();
        for (kind, topic) in [
            (Kind::Custom, "custom_edit"),
            (Kind::Funnel, "funnel_curves"),
            (Kind::Free, "straighten"),
        ] {
            if kinds.contains(&kind) {
                self.tips.show(topic);
                return;
            }
        }
    }

    // ------------------------------------------------------------ 自由笔

    /// 面板里"画整齐"要改的东西：选中的 free 形状；没有就找唯一选中的自定义形状
    /// 里被拾取的 free 笔画（原版 free_targets）。
    fn free_targets(&self) -> Vec<FreeTarget> {
        let shapes: Vec<FreeTarget> = self
            .sels
            .iter()
            .copied()
            .filter(|&i| {
                self.shapes
                    .get(i)
                    .map(|s| s.kind == Kind::Free)
                    .unwrap_or(false)
            })
            .map(FreeTarget::Shape)
            .collect();
        if !shapes.is_empty() {
            return shapes;
        }
        if self.sels.len() != 1 {
            return Vec::new();
        }
        let Some(i) = self.sel else {
            return Vec::new();
        };
        let Some(sh) = self.shapes.get(i) else {
            return Vec::new();
        };
        if sh.kind != Kind::Custom || sh.text.is_some() || sh.notes.is_some() {
            return Vec::new();
        }
        let Some(k) = crate::roll_live::picked_stroke(self, sh) else {
            return Vec::new();
        };
        match sh.strokes.get(k) {
            Some(Stroke::Poly { free: true, .. }) => vec![FreeTarget::Stroke(i, k)],
            _ => Vec::new(),
        }
    }

    /// 自由笔面板（原版 panel_freehand）：Straighten 灵敏度，0 = 保持原样。
    fn freehand_section(&mut self, ui: &mut egui::Ui) {
        let targets = self.free_targets();
        if targets.is_empty() && !self.sels.is_empty() {
            return; // 选中的都不是自由笔
        }
        if targets.is_empty() && self.tool != Tool::Free {
            return; // 没选东西：只有 Freehand 工具下才给新笔画调灵敏度
        }
        let mut value = match targets.first() {
            Some(FreeTarget::Shape(i)) => self
                .shapes
                .get(*i)
                .map(|s| s.smooth)
                .unwrap_or(self.free_smooth),
            Some(FreeTarget::Stroke(i, k)) => {
                match self.shapes.get(*i).and_then(|s| s.strokes.get(*k)) {
                    Some(Stroke::Poly { smooth, .. }) => *smooth,
                    _ => self.free_smooth,
                }
            }
            None => self.free_smooth,
        };
        let mut changed = false;
        let mut fresh = false;
        egui::CollapsingHeader::new(rust_i18n::t!("panel.freehand.title"))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.freehand.straighten"));
                    let resp = ui
                        .add(egui::Slider::new(&mut value, 0..=100))
                        .on_hover_text(rust_i18n::t!("panel.freehand.tip"));
                    fresh = resp.drag_started();
                    if resp.changed() {
                        changed = true;
                    }
                });
                ui.label(
                    egui::RichText::new(rust_i18n::t!("panel.freehand.note"))
                        .weak()
                        .size(10.0),
                );
            });
        if changed {
            let v = clean_level(value as f64);
            self.set_free_smooth(v, &targets, fresh);
        }
    }

    /// 改了 Straighten：新笔画用这个值；选中目标写回（free 形状 / 自定义形状的自由笔画）。
    fn set_free_smooth(&mut self, value: i64, targets: &[FreeTarget], fresh: bool) {
        self.free_smooth = value; // 新自由笔画也用它（原版 free_smooth）
        if targets.is_empty() {
            self.schedule_autosave();
            return;
        }
        if fresh {
            self.edit_key = None; // 新一次拖动 = 新的一步撤销
        }
        let key = format!("smooth:{:?}", self.sels);
        if self.edit_key.as_deref() != Some(key.as_str()) {
            self.push_undo();
            self.edit_key = Some(key);
        }
        let k = self.tumour_k();
        let mut owners: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for t in targets {
            match t {
                FreeTarget::Shape(i) => {
                    if let Some(sh) = self.shapes.get_mut(*i) {
                        sh.smooth = value;
                        sh.k = k;
                    }
                }
                FreeTarget::Stroke(i, kk) => {
                    if let Some(sh) = self.shapes.get_mut(*i) {
                        let uk = spiderweb_core::custom::uv_k(&sh.pts, k);
                        if let Some(Stroke::Poly {
                            smooth,
                            k: stroke_k,
                            free: true,
                            ..
                        }) = sh.strokes.get_mut(*kk)
                        {
                            *smooth = value;
                            *stroke_k = uk;
                        }
                        owners.insert(*i);
                    }
                }
            }
        }
        for i in owners {
            if let Some(sh) = self.shapes.get_mut(i) {
                spiderweb_core::custom::refit(sh);
            }
        }
        self.shapes_changed();
    }

    // ------------------------------------------------------------ 肿瘤

    /// 当前屏幕比例：几个 beat 对应一个 key（原版 `roll.sy / roll.sx`，兜底 0.25）。
    fn tumour_k(&self) -> f64 {
        if self.view.sx != 0.0 {
            self.view.sy / self.view.sx
        } else {
            0.25
        }
    }

    /// 面板要改的肿瘤形状：选中的 line / poly / free / curve / arc（原版 tumour_targets）。
    fn tumour_targets(&self) -> Vec<usize> {
        self.sels
            .iter()
            .copied()
            .filter(|&i| {
                self.shapes
                    .get(i)
                    .map(|s| tumour_kind(s.kind))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// 肿瘤面板（原版 panel_tumour）：开关 / 形状 / 尺寸 / 长度 / 间距 / 方向 / 跟随 / 范围 /
    /// 缓入 / 适配 / 种子 / 镜像。没选形状时改的是新形状默认设置（`Shape.tumour`）。
    fn tumour_section(&mut self, ui: &mut egui::Ui) {
        let targets = self.tumour_targets();
        if targets.is_empty() && !self.sels.is_empty() {
            return; // 选中的都不能带肿瘤
        }
        if targets.is_empty() && !self.line_tool() {
            return; // 没选东西：只有画线类工具下才给新形状调肿瘤
        }
        let placed = !targets.is_empty();
        let current = if placed {
            self.shapes[targets[0]].tumour.clone()
        } else {
            self.defaults.tumour.clone()
        }
        .unwrap_or(Tumour {
            on: false, // 还没有肿瘤：原版 `get("tumour") or {"on": False}`（其余用默认值）
            ..Tumour::default()
        });
        let ppq = self.ppq.max(1) as f64;
        let on = current.on;
        let shape = current.shape;
        let side = current.side;
        let wrap = current.wrap;
        let fit = current.fit;
        let mirror = current.mirror;
        let size = current.size;
        let length = (current.length * ppq).round();
        let dist = (current.dist * ppq).round();
        let ease = (current.ease * ppq).round();
        let start = (current.start * 100.0 * 1000.0).round() / 1000.0;
        let end = (current.end * 100.0 * 1000.0).round() / 1000.0;
        let mut seed = current.seed;
        let mut changes: Vec<(TumourChange, bool)> = Vec::new();
        // 这一帧有没有数字框 / 滑块刚刚开始拖（新的一次手势 = 新的一步撤销）
        let mut fresh = false;
        let mut reroll = false;
        egui::CollapsingHeader::new(rust_i18n::t!("panel.tumour.title"))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let mut on_edit = on;
                    if ui
                        .checkbox(&mut on_edit, rust_i18n::t!("panel.tumour.on"))
                        .changed()
                    {
                        changes.push((TumourChange::On(on_edit), true));
                    }
                    ui.label(rust_i18n::t!("panel.tumour.shape"));
                    let mut shape_edit = shape;
                    egui::ComboBox::from_id_salt("tumour_shape")
                        .selected_text(tumour_shape_name(shape_edit))
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for s in [
                                TumourShape::Triangle,
                                TumourShape::Square,
                                TumourShape::Circle,
                                TumourShape::Parabola,
                            ] {
                                if ui
                                    .selectable_label(shape_edit == s, tumour_shape_name(s))
                                    .clicked()
                                {
                                    shape_edit = s;
                                }
                            }
                        });
                    if shape_edit != shape {
                        changes.push((TumourChange::Shape(shape_edit), true));
                    }
                });
                tumour_num_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.tumour.size").to_string(),
                        unit: rust_i18n::t!("unit.keys").to_string(),
                        value: size,
                        range: 0.0..=1000.0,
                        speed: 0.1,
                    },
                    TumourChange::Size,
                    &mut changes,
                    &mut fresh,
                );
                tumour_num_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.tumour.length").to_string(),
                        unit: rust_i18n::t!("unit.ticks").to_string(),
                        value: length,
                        range: 0.0..=10_000_000.0,
                        speed: 1.0,
                    },
                    |v| TumourChange::Length(v / ppq),
                    &mut changes,
                    &mut fresh,
                );
                tumour_num_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.tumour.distance").to_string(),
                        unit: rust_i18n::t!("unit.ticks").to_string(),
                        value: dist,
                        range: 1.0..=10_000_000.0,
                        speed: 1.0,
                    },
                    |v| TumourChange::Dist(v / ppq),
                    &mut changes,
                    &mut fresh,
                );
                tumour_num_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.tumour.lead_in").to_string(),
                        unit: rust_i18n::t!("unit.ticks").to_string(),
                        value: ease,
                        range: 0.0..=10_000_000.0,
                        speed: 1.0,
                    },
                    |v| TumourChange::Ease(v / ppq),
                    &mut changes,
                    &mut fresh,
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(rust_i18n::t!("panel.tumour.side"));
                    let mut side_edit = side;
                    egui::ComboBox::from_id_salt("tumour_side")
                        .selected_text(tumour_side_name(side_edit))
                        .width(105.0)
                        .show_ui(ui, |ui| {
                            for s in [
                                TumourSide::Alt,
                                TumourSide::Left,
                                TumourSide::Right,
                                TumourSide::Random,
                            ] {
                                if ui
                                    .selectable_label(side_edit == s, tumour_side_name(s))
                                    .clicked()
                                {
                                    side_edit = s;
                                }
                            }
                        });
                    if side_edit != side {
                        changes.push((TumourChange::Side(side_edit), true));
                    }
                    ui.label(rust_i18n::t!("panel.tumour.wrap"));
                    let mut wrap_edit = wrap;
                    egui::ComboBox::from_id_salt("tumour_wrap")
                        .selected_text(tumour_wrap_name(wrap_edit))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for w in [TumourWrap::Simple, TumourWrap::Wrap] {
                                if ui
                                    .selectable_label(wrap_edit == w, tumour_wrap_name(w))
                                    .clicked()
                                {
                                    wrap_edit = w;
                                }
                            }
                        });
                    if wrap_edit != wrap {
                        changes.push((TumourChange::Wrap(wrap_edit), true));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(rust_i18n::t!("panel.tumour.range"));
                    let mut start_edit = start;
                    let resp = ui.add(
                        egui::DragValue::new(&mut start_edit)
                            .speed(1.0)
                            .range(0.0..=100.0)
                            .max_decimals(3),
                    );
                    fresh |= resp.drag_started();
                    if resp.changed() {
                        changes.push((TumourChange::Start(start_edit / 100.0), fresh));
                    }
                    ui.label(rust_i18n::t!("panel.tumour.pct_to"));
                    let mut end_edit = end;
                    let resp = ui.add(
                        egui::DragValue::new(&mut end_edit)
                            .speed(1.0)
                            .range(0.0..=100.0)
                            .max_decimals(3),
                    );
                    fresh |= resp.drag_started();
                    if resp.changed() {
                        changes.push((TumourChange::End(end_edit / 100.0), fresh));
                    }
                    ui.label(rust_i18n::t!("panel.tumour.pct"));
                    let mut fit_edit = fit;
                    if ui
                        .checkbox(&mut fit_edit, rust_i18n::t!("panel.tumour.fit"))
                        .changed()
                    {
                        changes.push((TumourChange::Fit(fit_edit), true));
                    }
                    // "New random" 只在方向是 Random 且开着时能按（原版 reroll 按钮）
                    let can_reroll = on && side == TumourSide::Random;
                    if ui
                        .add_enabled(
                            can_reroll,
                            egui::Button::new(rust_i18n::t!("panel.tumour.new_random")),
                        )
                        .on_hover_text(rust_i18n::t!("panel.tumour.reroll_tip"))
                        .clicked()
                    {
                        reroll = true;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(rust_i18n::t!("panel.tumour.seed"));
                    let resp = ui.add(
                        egui::DragValue::new(&mut seed)
                            .speed(1.0)
                            .range(0..=999_999_999),
                    );
                    fresh |= resp.drag_started();
                    if resp.changed() {
                        changes.push((TumourChange::Seed(seed), fresh));
                    }
                    let mut mirror_edit = mirror;
                    if ui
                        .checkbox(&mut mirror_edit, rust_i18n::t!("panel.tumour.mirror"))
                        .changed()
                    {
                        changes.push((TumourChange::Mirror(mirror_edit), true));
                    }
                });
                let info = if !on {
                    rust_i18n::t!("panel.tumour.info_off").to_string()
                } else if placed {
                    rust_i18n::t!("panel.tumour.info_on").to_string()
                } else {
                    String::new()
                };
                ui.label(egui::RichText::new(info).weak().size(10.0));
            });
        for (change, fresh) in changes {
            self.apply_tumour_setting(change, &targets, fresh);
        }
        if reroll {
            let seed = random_seed();
            self.apply_tumour_setting(TumourChange::Seed(seed), &targets, true);
        }
    }

    fn line_tool(&self) -> bool {
        matches!(
            self.tool,
            Tool::Line | Tool::Poly | Tool::Free | Tool::Curve | Tool::Arc
        )
    }

    /// 改一个肿瘤设置：有目标就写进各形状的 `tumour`（没有就建一个），
    /// 没目标就写进新形状默认设置。一次手势（同一个设置连续改）只压一次撤销步。
    fn apply_tumour_setting(&mut self, change: TumourChange, targets: &[usize], fresh: bool) {
        let k = self.tumour_k();
        if targets.is_empty() {
            let tm = self.defaults.tumour.get_or_insert_with(Tumour::default);
            change.apply(tm);
            tm.k = k;
            self.schedule_autosave();
            return;
        }
        if fresh {
            self.edit_key = None; // 新一次拖动 / 点击 = 新的一步撤销
        }
        let key = format!("tumour:{}:{:?}", change.key(), self.sels);
        if self.edit_key.as_deref() != Some(key.as_str()) {
            self.push_undo();
            self.edit_key = Some(key);
        }
        for &i in targets {
            if let Some(sh) = self.shapes.get_mut(i) {
                let tm = sh.tumour.get_or_insert_with(Tumour::default);
                change.apply(tm);
                tm.k = k; // 尺寸按现在的卷帘样子算（原版 set_tumour）
            }
        }
        self.shapes_changed();
    }

    // ------------------------------------------------------------ 自定义形状面板

    /// 面板要改的自定义形状：选中的那些；没选中时是"新形状的默认设置"（原版 custom_targets）。
    fn custom_targets(&self) -> Vec<usize> {
        self.sels
            .iter()
            .copied()
            .filter(|&i| {
                self.shapes
                    .get(i)
                    .is_some_and(|s| s.kind == Kind::Custom && s.text.is_none())
            })
            .collect()
    }

    /// 选中变化时把 gate 文本刷成当前形状的 ticks（原版 sync_custom）。
    fn refresh_custom_gate_text(&mut self) {
        let gate = self
            .custom_targets()
            .first()
            .and_then(|&i| self.shapes.get(i))
            .map(|s| s.gate)
            .unwrap_or(self.custom_defaults.gate);
        self.custom_gate_text = fmt_num((gate * self.ppq as f64 * 1000.0).round() / 1000.0);
    }

    fn custom_section(&mut self, ui: &mut egui::Ui) {
        let placed = self.custom_targets();
        if !self.sels.is_empty() && placed.is_empty() {
            return; // 选中的都不是自定义形状
        }
        let title = if placed.is_empty() {
            rust_i18n::t!("panel.custom.title_new").to_string()
        } else if placed.len() > 1 {
            rust_i18n::t!("panel.custom.title_many", n = placed.len().to_string()).to_string()
        } else {
            rust_i18n::t!("panel.custom.title").to_string()
        };
        egui::CollapsingHeader::new(title)
            .default_open(true)
            .show(ui, |ui| self.custom_body_ui(ui, &placed));
    }

    fn custom_body_ui(&mut self, ui: &mut egui::Ui, placed: &[usize]) {
        let placed_mode = !placed.is_empty();
        let (fill, align, name) = if placed_mode {
            match self.shapes.get(placed[0]) {
                Some(sh) => (sh.fill, sh.align, sh.name.clone()),
                None => return,
            }
        } else {
            (
                self.custom_defaults.fill,
                self.custom_defaults.align,
                self.custom_defaults.shape.clone(),
            )
        };
        let pasted = placed_mode
            && placed
                .iter()
                .any(|&i| self.shapes.get(i).is_some_and(|s| s.notes.is_some()));
        let gaps = if placed_mode {
            placed
                .iter()
                .filter_map(|&i| self.shapes.get(i))
                .map(|s| spiderweb_core::custom::open_paths(&s.strokes).len())
                .max()
                .unwrap_or(0)
        } else {
            crate::roll_live::builtin_template(&self.library_dir, &name)
                .map(|(st, _)| spiderweb_core::custom::open_paths(&st).len())
                .unwrap_or(2)
        };
        let fillable = gaps <= 1;
        let spam = matches!(fill, Fill::Spam | Fill::OutlineSpam)
            && (fillable || fill == Fill::OutlineSpam);
        // 缺口太多时面板显示 Empty（原版 fill_var.set 的兜底）
        let shown_fill = if fillable || fill == Fill::OutlineSpam {
            fill
        } else {
            Fill::Empty
        };

        let mut pick: Option<String> = None;
        let mut new_fill: Option<Fill> = None;
        let mut new_align: Option<Align> = None;
        let mut apply_gate = false;
        let lib_names = crate::drawer::library_names(&self.library_dir);

        ui.horizontal(|ui| {
            ui.label(rust_i18n::t!("panel.custom.shape"));
            egui::ComboBox::from_id_salt("custom_shape")
                .selected_text(name.clone())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for n in &lib_names {
                        if ui.selectable_label(name == *n, n).clicked() && name != *n {
                            pick = Some(n.clone());
                        }
                    }
                });
            if ui.button(rust_i18n::t!("panel.custom.drawer")).clicked() {
                self.open_drawer();
            }
        });
        if pasted {
            let total: i64 = placed
                .iter()
                .filter_map(|&i| self.shapes.get(i))
                .map(|s| self.note_count(s))
                .sum();
            ui.label(rust_i18n::t!(
                "panel.custom.info_pasted",
                total = total.to_string()
            ));
        } else {
            ui.label(rust_i18n::t!("panel.custom.inside"));
            for (label, value) in [
                (rust_i18n::t!("panel.custom.empty"), Fill::Empty),
                (rust_i18n::t!("panel.custom.fill"), Fill::Fill),
                (rust_i18n::t!("panel.custom.spam"), Fill::Spam),
                (
                    rust_i18n::t!("panel.custom.outline_spam"),
                    Fill::OutlineSpam,
                ),
            ] {
                let enabled = fillable || matches!(value, Fill::Empty | Fill::OutlineSpam);
                if ui
                    .add_enabled(enabled, egui::RadioButton::new(shown_fill == value, label))
                    .clicked()
                {
                    new_fill = Some(value);
                }
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(spam, |ui| {
                    ui.label(rust_i18n::t!("panel.custom.gate"));
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.custom_gate_text).desired_width(70.0),
                    );
                    if resp.lost_focus() {
                        apply_gate = true; // Enter 或点到别处都应用（原版 Return / FocusOut）
                    }
                    ui.label(rust_i18n::t!("panel.custom.gate_hint"));
                });
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(spam, |ui| {
                    ui.label(rust_i18n::t!("panel.custom.start"));
                    if ui
                        .add_enabled(
                            align != Align::Auto,
                            egui::RadioButton::new(
                                align == Align::Auto,
                                rust_i18n::t!("panel.custom.auto"),
                            ),
                        )
                        .clicked()
                    {
                        new_align = Some(Align::Auto);
                    }
                    if ui
                        .add_enabled(
                            align != Align::Aligned,
                            egui::RadioButton::new(
                                align == Align::Aligned,
                                rust_i18n::t!("panel.custom.aligned"),
                            ),
                        )
                        .clicked()
                    {
                        new_align = Some(Align::Aligned);
                    }
                });
            });
        }

        let info = if pasted {
            String::new()
        } else if placed_mode {
            let total: i64 = placed
                .iter()
                .filter_map(|&i| self.shapes.get(i))
                .map(|s| self.note_count(s))
                .sum();
            let mut info =
                rust_i18n::t!("panel.custom.info_notes", total = total.to_string()).to_string();
            if gaps == 1 && matches!(fill, Fill::Fill | Fill::Spam) {
                info += &format!("  {}", rust_i18n::t!("panel.custom.info_one_gap"));
            } else if gaps > 1 {
                info += &format!(
                    "  {}",
                    rust_i18n::t!("panel.custom.info_gaps", gaps = gaps.to_string())
                );
            }
            if let Some(k) = self.stroke
                && placed.len() == 1
                && let Some(sh) = self.shapes.get(placed[0])
            {
                info += &format!(
                    "  {}",
                    rust_i18n::t!(
                        "panel.custom.info_picked",
                        k = (k + 1).to_string(),
                        n = sh.strokes.len().to_string()
                    )
                );
            }
            info
        } else if self.live && crate::roll_live::is_stroke_tool(self.tool) {
            rust_i18n::t!("panel.custom.info_live").to_string()
        } else if crate::roll_live::builtin_template(&self.library_dir, &name).is_none() {
            rust_i18n::t!("panel.custom.info_pick").to_string()
        } else if self.tool.is_box() {
            rust_i18n::t!(
                "panel.custom.info_drag_perfect",
                tool = self.tool.ui_label()
            )
            .to_string()
        } else {
            rust_i18n::t!("panel.custom.info_drag_place").to_string()
        };
        if !info.is_empty() {
            ui.label(
                egui::RichText::new(info)
                    .small()
                    .color(egui::Color32::from_gray(120)),
            );
        }

        if let Some(pick) = pick {
            self.pick_custom_template(&pick, placed);
        }
        if let Some(fill) = new_fill {
            self.set_custom_fill(fill, placed);
        }
        if let Some(align) = new_align {
            self.set_custom_align(align, placed);
        }
        if apply_gate {
            self.apply_custom_gate(placed);
        }
    }

    /// 面板里选了一个模板（内置或图形库里的）：新形状用它，选中的自定义形状也换成它。
    fn pick_custom_template(&mut self, name: &str, placed: &[usize]) {
        let Some((strokes, _)) = crate::roll_live::builtin_template(&self.library_dir, name) else {
            self.status = rust_i18n::t!("panel.custom.error_read", name = name).to_string();
            return;
        };
        self.custom_defaults.shape = name.to_string();
        if placed.is_empty() {
            self.schedule_autosave();
            return;
        }
        self.push_undo();
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.name = name.to_string();
                sh.strokes = strokes.clone();
            }
        }
        self.shapes_changed();
    }

    fn set_custom_fill(&mut self, fill: Fill, placed: &[usize]) {
        if placed.is_empty() {
            self.custom_defaults.fill = fill;
            self.schedule_autosave();
            return;
        }
        self.push_undo();
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.fill = fill;
            }
        }
        self.shapes_changed();
    }

    fn set_custom_align(&mut self, align: Align, placed: &[usize]) {
        if placed.is_empty() {
            self.custom_defaults.align = align;
            self.schedule_autosave();
            return;
        }
        self.push_undo();
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.align = align;
            }
        }
        self.shapes_changed();
    }

    /// gate 文本框（ticks，mathexpr 表达式）应用成形状的拍数（原版 on_gate）。
    fn apply_custom_gate(&mut self, placed: &[usize]) {
        // 输入不合法：原版只把输入框标红（Bad.TEntry），没有提示文字
        let Ok(ticks) =
            spiderweb_io::mathexpr::calc_int(&self.custom_gate_text, Some(1), Some(10_000_000))
        else {
            return;
        };
        let gate = ticks as f64 / self.ppq as f64;
        if placed.is_empty() {
            self.custom_defaults.gate = gate;
            self.schedule_autosave();
            return;
        }
        self.push_undo();
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.gate = gate;
            }
        }
        self.shapes_changed();
    }

    fn project_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(rust_i18n::t!("panel.project.title"))
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("project_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(rust_i18n::t!("panel.project.ppq"));
                        if ui
                            .add(egui::TextEdit::singleline(&mut self.pvar.ppq).desired_width(90.0))
                            .changed()
                        {
                            self.on_project_change();
                        }
                        ui.end_row();
                        ui.label(rust_i18n::t!("panel.project.bpm"));
                        if ui
                            .add(egui::TextEdit::singleline(&mut self.pvar.bpm).desired_width(90.0))
                            .changed()
                        {
                            self.on_project_change();
                        }
                        ui.end_row();
                        ui.label(rust_i18n::t!("panel.project.beats"));
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.pvar.beats)
                                    .desired_width(90.0),
                            )
                            .changed()
                        {
                            self.on_project_change();
                        }
                        ui.end_row();
                        ui.label(rust_i18n::t!("panel.project.output"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.pvar.output).desired_width(180.0),
                        );
                        ui.end_row();
                    });
                if self.ppq >= 32767 {
                    ui.colored_label(
                        egui::Color32::from_rgb(0xd0, 0x00, 0x00),
                        rust_i18n::t!("panel.project.ppq_warning"),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button(rust_i18n::t!("panel.project.open")).clicked() {
                        self.open_project();
                    }
                    if ui.button(rust_i18n::t!("panel.project.save")).clicked() {
                        self.save_project_as();
                    }
                    if ui.button(rust_i18n::t!("panel.project.generate")).clicked() {
                        self.generate_midi();
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .button(rust_i18n::t!("panel.project.paste_domino"))
                        .clicked()
                    {
                        self.paste_from_domino();
                    }
                    if ui
                        .button(rust_i18n::t!("panel.project.copy_domino"))
                        .clicked()
                    {
                        self.copy_to_domino();
                    }
                });
                ui.label(rust_i18n::t!("panel.project.channels"));
                ui.radio_value(
                    &mut self.channel_mode,
                    ChannelMode::Raw,
                    rust_i18n::t!("panel.project.mode_raw"),
                );
                ui.radio_value(
                    &mut self.channel_mode,
                    ChannelMode::Single,
                    rust_i18n::t!("panel.project.mode_single"),
                );
                ui.radio_value(
                    &mut self.channel_mode,
                    ChannelMode::Auto,
                    rust_i18n::t!("panel.project.mode_auto"),
                );
                if self.channel_mode == ChannelMode::Auto {
                    ui.horizontal(|ui| {
                        ui.label(rust_i18n::t!("panel.project.split"));
                        ui.radio_value(
                            &mut self.channel_split,
                            ChannelSplit::Key,
                            rust_i18n::t!("panel.project.split_key"),
                        );
                        ui.radio_value(
                            &mut self.channel_split,
                            ChannelSplit::Time,
                            rust_i18n::t!("panel.project.split_time"),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.project.midi_out"));
                    egui::ComboBox::from_id_salt("midi_out")
                        .selected_text(if self.midi_device.is_empty() {
                            "(default)"
                        } else {
                            &self.midi_device
                        })
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for d in crate::playback::devices() {
                                if ui.selectable_label(self.midi_device == d, &d).clicked() {
                                    self.stop_play();
                                    self.player.close();
                                    self.midi_device = d;
                                }
                            }
                        });
                });
            });
    }

    fn shapes_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(rust_i18n::t!("panel.shapes.title"))
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .id_salt("shapes_list")
                    .show(ui, |ui| {
                        let labels: Vec<(usize, String)> = self
                            .shapes
                            .iter()
                            .enumerate()
                            .map(|(i, sh)| {
                                let count = self.note_counts.get(i).copied().unwrap_or(0);
                                (
                                    i,
                                    rust_i18n::t!(
                                        "panel.shapes.item",
                                        i = (i + 1).to_string(),
                                        label = self.shape_label(sh),
                                        notes = count.to_string()
                                    )
                                    .to_string(),
                                )
                            })
                            .collect();
                        let mut clicked: Option<usize> = None;
                        let mut toggle = false;
                        for (i, label) in labels {
                            let selected = self.sels.contains(&i);
                            let resp = ui.selectable_label(selected, label);
                            if resp.clicked() {
                                clicked = Some(i);
                                toggle =
                                    ui.input(|inp| inp.modifiers.command || inp.modifiers.ctrl);
                            }
                        }
                        if let Some(i) = clicked {
                            self.select(Some(i), toggle);
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button(rust_i18n::t!("panel.shapes.duplicate")).clicked() {
                        self.duplicate();
                    }
                    if ui.button(rust_i18n::t!("panel.shapes.delete")).clicked() {
                        self.delete_selected();
                    }
                    if ui
                        .button(rust_i18n::t!("panel.shapes.delete_all"))
                        .clicked()
                    {
                        self.delete_all();
                    }
                });
            });
    }

    fn defaults_section(&mut self, ui: &mut egui::Ui) {
        let title = match self.selected() {
            Some(sh) => {
                let extra = if self.sels.len() > 1 {
                    rust_i18n::t!("panel.defaults.more", n = (self.sels.len() - 1).to_string())
                        .to_string()
                } else {
                    String::new()
                };
                rust_i18n::t!(
                    "panel.defaults.title",
                    i = (self.sel.unwrap_or(0) + 1).to_string(),
                    label = self.shape_label(sh),
                    extra = extra
                )
                .to_string()
            }
            None => rust_i18n::t!("panel.defaults.new").to_string(),
        };
        egui::CollapsingHeader::new(title)
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.defaults.velocity"));
                    let r0 = ui
                        .add(egui::TextEdit::singleline(&mut self.vel_text[0]).desired_width(50.0));
                    ui.label("→");
                    let r1 = ui
                        .add(egui::TextEdit::singleline(&mut self.vel_text[1]).desired_width(50.0));
                    if r0.changed() || r1.changed() {
                        self.apply_velocity_text();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.defaults.last_note"));
                    let mut end_dot = self.target_end_dot();
                    let r0 = ui.radio_value(
                        &mut end_dot,
                        false,
                        rust_i18n::t!("panel.defaults.ends_on"),
                    );
                    let r1 = ui.radio_value(
                        &mut end_dot,
                        true,
                        rust_i18n::t!("panel.defaults.starts_on"),
                    );
                    if r0.changed() || r1.changed() {
                        self.set_end_dot(end_dot);
                    }
                });
            });
    }

    fn target_end_dot(&self) -> bool {
        self.selected()
            .or(Some(&self.defaults))
            .map(|t| t.end_dot)
            .unwrap_or(false)
    }

    pub fn apply_velocity_text(&mut self) {
        let v0 = parse_int(&self.vel_text[0], 1, 127);
        let v1 = parse_int(&self.vel_text[1], 1, 127);
        let (Some(v0), Some(v1)) = (v0, v1) else {
            return;
        };
        let idx: Vec<usize> = if self.sels.is_empty() {
            Vec::new()
        } else {
            self.sels.iter().copied().collect()
        };
        self.push_undo();
        if idx.is_empty() {
            self.defaults.vel0 = v0 as f64;
            self.defaults.vel1 = v1 as f64;
            self.defaults.vel_env.clear();
        } else {
            for i in idx {
                if let Some(sh) = self.shapes.get_mut(i) {
                    sh.vel0 = v0 as f64;
                    sh.vel1 = v1 as f64;
                    sh.vel_env.clear();
                    sh.own_vel = false;
                }
            }
        }
        self.shapes_changed();
    }

    pub fn set_end_dot(&mut self, value: bool) {
        if self.sels.is_empty() {
            self.defaults.end_dot = value;
            return;
        }
        self.push_undo();
        for i in self.sels.iter().copied().collect::<Vec<_>>() {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.end_dot = value;
            }
        }
        self.shapes_changed();
    }

    // ------------------------------------------------------------ 文本面板

    /// 选中的文本形状（按下标排序）（原版 text_shapes）。
    pub fn text_shapes(&self) -> Vec<usize> {
        self.sels
            .iter()
            .copied()
            .filter(|&i| {
                self.shapes
                    .get(i)
                    .map(|sh| sh.text.is_some())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// 面板显示的（设置, size 框的数字）：正在输入的、选中的第一段文本，或新文本的默认值
    /// （原版 text_current；size 跟着轴走，见 shown_size）。
    fn text_current(&self) -> (TextSettings, f64) {
        if let Some((tx, axes)) = crate::roll_text::typing_state(self) {
            let cap = text::text_font(&tx).cap;
            return (tx.clone(), text::shown_size(&tx, axes, cap));
        }
        if let Some(&i) = self.text_shapes().first()
            && let Some(sh) = self.shapes.get(i)
        {
            let tx = sh.text.clone().unwrap_or_default();
            if let Some(axes) = text::text_axes(sh) {
                let cap = text::text_font(&tx).cap;
                let size = text::shown_size(&tx, axes, cap);
                return (tx, size);
            }
            let size = tx.size;
            return (tx, size);
        }
        (self.text_defaults.clone(), self.text_defaults.size)
    }

    /// 文本面板（原版 panel_text）：字体 / 字号 / 字重 / 字距 / 行距 / 对齐 / 阈值 / 加粗。
    fn text_section(&mut self, ui: &mut egui::Ui) {
        let text_shapes = self.text_shapes();
        if self.typing.is_none() && self.tool != Tool::Text && text_shapes.is_empty() {
            return;
        }
        if self.font_families.is_empty() {
            self.font_families = spiderweb_core::fonts::font_families();
        }
        let (tx, size) = self.text_current();
        let font = text::text_font(&tx);
        let font_found = font.found();
        let mut changes = TextChange::default();
        let mut any = false;
        egui::CollapsingHeader::new(rust_i18n::t!("panel.text.title"))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.text.font"));
                    egui::ComboBox::from_id_salt("text_font")
                        .selected_text(tx.font.clone())
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for f in &self.font_families {
                                if ui.selectable_label(tx.font == *f, f).clicked() {
                                    changes.font = Some(f.clone());
                                    any = true;
                                }
                            }
                        });
                    if ui
                        .button(
                            rust_i18n::t!("panel.text.font_button", font = tx.font.clone())
                                .to_string(),
                        )
                        .on_hover_text(rust_i18n::t!("panel.text.font_tip"))
                        .clicked()
                    {
                        crate::text_dialog::open_font_dialog(self, &tx);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.text.size"));
                    let mut v = size;
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .speed(1.0)
                                .range(0.01..=2000.0)
                                .max_decimals(4),
                        )
                        .changed()
                    {
                        changes.size = Some(v);
                        any = true;
                    }
                    if ui
                        .selectable_label(
                            tx.unit == TextUnit::Font,
                            rust_i18n::t!("panel.text.unit_font"),
                        )
                        .clicked()
                    {
                        changes.unit = Some(TextUnit::Font);
                        any = true;
                    }
                    if ui
                        .selectable_label(
                            tx.unit == TextUnit::Rows,
                            rust_i18n::t!("panel.text.unit_rows"),
                        )
                        .clicked()
                    {
                        changes.unit = Some(TextUnit::Rows);
                        any = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.text.weight"));
                    let current = weight_name(tx.weight);
                    egui::ComboBox::from_id_salt("text_weight")
                        .selected_text(current)
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for (w, n) in WEIGHTS {
                                if ui.selectable_label(current == n, n).clicked() {
                                    changes.weight = Some(w);
                                    any = true;
                                }
                            }
                        });
                    let mut italic = tx.italic;
                    if ui
                        .checkbox(&mut italic, rust_i18n::t!("panel.text.italic"))
                        .changed()
                    {
                        changes.italic = Some(italic);
                        any = true;
                    }
                });
                number_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.text.letter_spacing").to_string(),
                        unit: "/1000 em".to_string(),
                        value: tx.tracking,
                        range: -1000.0..=10000.0,
                        speed: 10.0,
                    },
                    &mut changes,
                    &mut any,
                    |c, v| c.tracking = Some(v),
                );
                number_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.text.line_spacing").to_string(),
                        unit: "%".to_string(),
                        value: tx.leading,
                        range: 1.0..=1000.0,
                        speed: 5.0,
                    },
                    &mut changes,
                    &mut any,
                    |c, v| c.leading = Some(v),
                );
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.text.align"));
                    for (align, label) in [
                        (TextAlign::Left, rust_i18n::t!("panel.text.align_left")),
                        (TextAlign::Center, rust_i18n::t!("panel.text.align_center")),
                        (TextAlign::Right, rust_i18n::t!("panel.text.align_right")),
                    ] {
                        if ui.selectable_label(tx.align == align, label).clicked() {
                            changes.align = Some(align);
                            any = true;
                        }
                    }
                });
                number_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.text.threshold").to_string(),
                        unit: "%".to_string(),
                        value: tx.threshold,
                        range: 0.0..=100.0,
                        speed: 1.0,
                    },
                    &mut changes,
                    &mut any,
                    |c, v| c.threshold = Some(v),
                );
                number_row(
                    ui,
                    NumberRow {
                        label: rust_i18n::t!("panel.text.grow").to_string(),
                        unit: rust_i18n::t!("unit.keys").to_string(),
                        value: tx.grow,
                        range: -100.0..=100.0,
                        speed: 0.1,
                    },
                    &mut changes,
                    &mut any,
                    |c, v| c.grow = Some(v),
                );
                let info = if !font_found {
                    rust_i18n::t!(
                        "panel.text.not_installed",
                        font = tx.font.clone(),
                        face = font.face.clone()
                    )
                    .to_string()
                } else if self.typing.is_some() {
                    rust_i18n::t!("panel.text.typing").to_string()
                } else if self.tool == Tool::Text {
                    rust_i18n::t!("panel.text.click_type").to_string()
                } else {
                    rust_i18n::t!("panel.text.dbl_click").to_string()
                };
                let info = if text_shapes.len() > 1 && self.typing.is_none() {
                    rust_i18n::t!(
                        "panel.text.all_texts",
                        info = info,
                        n = text_shapes.len().to_string()
                    )
                    .to_string()
                } else {
                    info
                };
                ui.label(egui::RichText::new(info).weak().size(10.0));
            });
        if any {
            crate::roll_text::set_text_setting(self, &changes);
        }
    }

    fn points_section(&mut self, ui: &mut egui::Ui) {
        let Some(sh) = self.selected().cloned() else {
            return;
        };
        if self.sels.len() != 1
            || sh.pts.len() > 300
            || sh.kind == Kind::Custom
            || sh.kind == Kind::Curve
        {
            return;
        }
        let names = spiderweb_core::engine::point_names(&sh);
        let ppq = self.ppq as f64;
        let mut pts = sh.pts.clone();
        let mut changed = false;
        egui::CollapsingHeader::new(rust_i18n::t!("panel.points.title"))
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("points_grid")
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label(rust_i18n::t!("panel.points.point"));
                        ui.label(rust_i18n::t!("panel.points.tick"));
                        ui.label(rust_i18n::t!("panel.points.pitch"));
                        ui.end_row();
                        for (i, pt) in pts.iter_mut().enumerate() {
                            let name = names
                                .as_ref()
                                .and_then(|n| n.get(i).cloned())
                                .unwrap_or_else(|| format!("{i}"));
                            ui.label(name);
                            let mut tick = (pt[0] * ppq).round() as i64;
                            let mut pitch = pt[1];
                            if ui.add(egui::DragValue::new(&mut tick).speed(1.0)).changed() {
                                pt[0] = tick as f64 / ppq;
                                changed = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(&mut pitch)
                                        .speed(0.1)
                                        .range(0.0..=127.0),
                                )
                                .changed()
                            {
                                pt[1] = pitch;
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });
        if changed {
            if let Some(target) = self.sel.and_then(|i| self.shapes.get_mut(i)) {
                target.pts = pts;
            }
            self.shapes_changed();
        }
    }

    // ------------------------------------------------------------ 漏斗

    /// 漏斗面板要作用的形状：选中的漏斗（原版 funnel_targets）。
    fn funnel_target_indices(&self) -> Vec<usize> {
        self.sels
            .iter()
            .copied()
            .filter(|&i| {
                self.shapes
                    .get(i)
                    .map(|s| s.kind == Kind::Funnel)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// gate 文本框跟着选中的漏斗（或默认设置）刷新（原版 sync_funnel 的文本部分）。
    pub fn sync_funnel_text(&mut self) {
        let cur = self
            .selected()
            .filter(|sh| sh.kind == Kind::Funnel)
            .map(roll_funnel::settings_of_shape)
            .unwrap_or_else(|| roll_funnel::settings_of_defaults(&self.funnel_defaults));
        let ppq = self.ppq.max(1) as f64;
        self.funnel_text = [fmt_ticks(cur.gate0, ppq), fmt_ticks(cur.gate1, ppq)];
    }

    /// 漏斗设置面板（原版 panel_funnel._build_funnel / sync_funnel）：填充、墙、gate、
    /// 变化方式与跟随、曲线预设 / 公式、翻里翻外与首尾对调。
    fn funnel_section(&mut self, ui: &mut egui::Ui) {
        let targets = self.funnel_target_indices();
        if targets.is_empty() && !self.sels.is_empty() {
            return;
        }
        if targets.is_empty() && self.tool != Tool::Funnel {
            return; // 没选东西：只有 Funnel 工具下才给新漏斗调设置
        }
        let cur = targets
            .first()
            .and_then(|&i| self.shapes.get(i))
            .map(roll_funnel::settings_of_shape)
            .unwrap_or(funnel::FUNNEL_DEFAULTS);
        let reversed = targets
            .first()
            .and_then(|&i| self.shapes.get(i))
            .is_some_and(funnel::funnel_reversed);
        let placed = !targets.is_empty();
        let note_total: i64 = targets
            .iter()
            .filter_map(|&i| self.shapes.get(i))
            .map(|sh| self.note_count(sh))
            .sum();
        let parts_text = self.parts_text();
        let waiting_wall = self
            .draft
            .as_ref()
            .is_some_and(|d| d.kind == Kind::Funnel && d.pts.len() == 2);
        let info = if waiting_wall {
            rust_i18n::t!("panel.funnel.info_waiting_wall").to_string()
        } else if placed {
            let mut s =
                rust_i18n::t!("panel.funnel.info_notes", n = note_total.to_string()).to_string();
            if !parts_text.is_empty() {
                s += rust_i18n::t!("panel.funnel.info_highlighted", parts = parts_text).as_ref();
            } else if targets.len() == 1 {
                s += rust_i18n::t!("panel.funnel.info_start_curve").as_ref();
            }
            s
        } else {
            rust_i18n::t!("panel.funnel.info_draw").to_string()
        };

        let mut new = cur;
        let mut text = self.funnel_text.clone();
        let mut text_changed = false;
        let mut preset_pick: Option<usize> = None;
        let mut inside_out = false;
        let mut turn = false;
        let mut apply_formula = false;
        let mut formula_text = self.funnel_formula.clone();
        egui::CollapsingHeader::new(rust_i18n::t!("panel.funnel.title"))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(rust_i18n::t!("panel.funnel.inside"));
                    ui.radio_value(
                        &mut new.fill,
                        FunnelFill::Spam,
                        rust_i18n::t!("panel.funnel.spam"),
                    );
                    ui.radio_value(
                        &mut new.fill,
                        FunnelFill::Long,
                        rust_i18n::t!("panel.funnel.long_notes"),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(rust_i18n::t!("panel.funnel.wall"));
                    // 反向漏斗的墙在前头：两种说法跟着换（原版 sync_funnel）
                    let (end_text, start_text) = if reversed {
                        (
                            rust_i18n::t!("panel.funnel.notes_start_on_it"),
                            rust_i18n::t!("panel.funnel.notes_end_on_it"),
                        )
                    } else {
                        (
                            rust_i18n::t!("panel.funnel.notes_end_on_it"),
                            rust_i18n::t!("panel.funnel.notes_start_on_it"),
                        )
                    };
                    ui.radio_value(&mut new.wall, WallMode::In, end_text);
                    ui.radio_value(&mut new.wall, WallMode::Past, start_text);
                });
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.funnel.gate"));
                    let gate_on = new.fill == FunnelFill::Spam || new.wall == WallMode::Past;
                    ui.add_enabled_ui(gate_on, |ui| {
                        if ui
                            .add(egui::TextEdit::singleline(&mut text[0]).desired_width(52.0))
                            .changed()
                        {
                            text_changed = true;
                        }
                    });
                    if new.vary {
                        ui.label("→");
                        if ui
                            .add(egui::TextEdit::singleline(&mut text[1]).desired_width(52.0))
                            .changed()
                        {
                            text_changed = true;
                        }
                    }
                    ui.label(rust_i18n::t!("unit.ticks"));
                });
                if ui
                    .checkbox(&mut new.vary, rust_i18n::t!("panel.funnel.vary"))
                    .changed()
                    && new.vary
                {
                    text[1] = text[0].clone();
                }
                let gates_on = new.fill == FunnelFill::Spam && new.vary;
                ui.add_enabled_ui(gates_on, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(rust_i18n::t!("panel.funnel.change"));
                        ui.radio_value(
                            &mut new.change,
                            GateChange::Steps,
                            rust_i18n::t!("panel.funnel.steps"),
                        );
                        ui.radio_value(
                            &mut new.change,
                            GateChange::Smooth,
                            rust_i18n::t!("panel.funnel.smooth"),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label(rust_i18n::t!("panel.funnel.follow"));
                        ui.radio_value(
                            &mut new.follow,
                            GateFollow::Time,
                            rust_i18n::t!("panel.funnel.evenly"),
                        );
                        ui.radio_value(
                            &mut new.follow,
                            GateFollow::Curve,
                            rust_i18n::t!("panel.funnel.with_curve"),
                        );
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(rust_i18n::t!("panel.funnel.curve"));
                    egui::ComboBox::from_id_salt("funnel_curve")
                        .selected_text(rust_i18n::t!("panel.funnel.preset"))
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for (i, (name, _)) in CURVE_PRESETS.iter().enumerate() {
                                if ui.selectable_label(false, *name).clicked() {
                                    preset_pick = Some(i);
                                }
                            }
                        });
                    if ui
                        .button(rust_i18n::t!("panel.funnel.inside_out"))
                        .clicked()
                    {
                        inside_out = true;
                    }
                    if ui.button(rust_i18n::t!("panel.funnel.turn")).clicked() {
                        turn = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("panel.funnel.formula"));
                    ui.add(
                        egui::TextEdit::singleline(&mut formula_text)
                            .desired_width(130.0)
                            .hint_text("x^2"),
                    );
                    if ui.button(rust_i18n::t!("panel.funnel.apply")).clicked() {
                        apply_formula = true;
                    }
                });
                ui.label(egui::RichText::new(info).weak());
            });

        if text_changed
            && let (Some(t0), Some(t1)) = (
                parse_int(&text[0], 1, 10_000_000),
                parse_int(&text[1], 1, 10_000_000),
            )
        {
            let ppq = self.ppq.max(1) as f64;
            new.gate0 = t0 as f64 / ppq;
            new.gate1 = t1 as f64 / ppq;
        }
        if !new.vary {
            new.gate1 = new.gate0;
        }
        if new != cur {
            self.apply_funnel_settings(new);
        }
        if let Some(pi) = preset_pick {
            self.apply_funnel_preset(pi);
        }
        if apply_formula {
            self.apply_funnel_formula(&formula_text);
        }
        if inside_out {
            self.set_funnel_curves(&|c| funnel::inside_out(c), true);
        }
        if turn {
            self.set_funnel_curves(&|c| funnel::turned_curve(c, true), true);
        }
        let ppq = self.ppq.max(1) as f64;
        self.funnel_text = if text_changed {
            text
        } else {
            [fmt_ticks(new.gate0, ppq), fmt_ticks(new.gate1, ppq)]
        };
        self.funnel_formula = formula_text;
    }

    /// 改选中漏斗（或没选东西时的默认设置）的漏斗设置（原版 set_funnel）。
    fn apply_funnel_settings(&mut self, s: FunnelSettings) {
        let targets = self.funnel_target_indices();
        if !targets.is_empty() {
            self.push_undo();
        }
        if targets.is_empty() {
            self.funnel_defaults = FunnelDefaults {
                fill: s.fill,
                gate0: s.gate0,
                gate1: s.gate1,
                vary: s.vary,
                change: s.change,
                follow: s.follow,
                wall: s.wall,
            };
        } else {
            for i in targets {
                if let Some(sh) = self.shapes.get_mut(i) {
                    roll_funnel::apply_settings(sh, s);
                }
            }
        }
        self.shapes_changed();
    }

    /// 预设曲线（CURVE_PRESETS）给高亮的曲线（原版 apply_formula 的预设部分）。
    fn apply_funnel_preset(&mut self, i: usize) {
        let Some((_, text)) = CURVE_PRESETS.get(i) else {
            return;
        };
        let formula_error = |status: &mut String, text: &str, e: String| {
            *status = rust_i18n::t!("panel.funnel.error_formula", text = text.to_string(), e = e)
                .to_string();
        };
        let shape = match text {
            None => {
                // 默认曲线不会算不出来（原版 preset_curve(None) 同样不报错）
                let Ok(c) = funnel::preset_curve(None) else {
                    return;
                };
                c
            }
            Some(t) => match spiderweb_io::mathexpr::formula(t) {
                Ok(f) => {
                    match funnel::preset_curve(Some(&|x| f.eval(x).map_err(|e| format!("{e:?}")))) {
                        Ok(c) => c,
                        Err(e) => {
                            formula_error(&mut self.status, t, e);
                            return;
                        }
                    }
                }
                Err(e) => {
                    formula_error(&mut self.status, t, format!("{e:?}"));
                    return;
                }
            },
        };
        self.apply_curve_shape(shape);
    }

    /// 自己写的公式给高亮的曲线（原版 apply_formula 的公式部分）。
    fn apply_funnel_formula(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        match spiderweb_io::mathexpr::formula(text) {
            Ok(f) => {
                match funnel::preset_curve(Some(&|x| f.eval(x).map_err(|e| format!("{e:?}")))) {
                    Ok(shape) => self.apply_curve_shape(shape),
                    Err(e) => {
                        self.status = rust_i18n::t!(
                            "panel.funnel.error_formula",
                            text = text.to_string(),
                            e = e
                        )
                        .to_string();
                    }
                }
            }
            Err(e) => {
                self.status = rust_i18n::t!(
                    "panel.funnel.error_formula",
                    text = text.to_string(),
                    e = format!("{e:?}")
                )
                .to_string();
            }
        }
    }

    /// 把一条曲线形状给高亮的曲线（原版 set_curves：没高亮时什么都不做，也不提示）。
    fn apply_curve_shape(&mut self, shape: FunnelCurve) {
        if self
            .funnel_parts()
            .map(|(_, _, c)| c.is_empty())
            .unwrap_or(true)
        {
            return;
        }
        self.set_funnel_curves(&|_| shape.clone(), true);
    }
}

fn fmt_num(v: f64) -> String {
    spiderweb_io::mathexpr::fmt(v)
}

/// gate 以 tick 显示（原版 fmt(round(t[key] * ppq, 3))）。
fn fmt_ticks(gate: f64, ppq: f64) -> String {
    spiderweb_io::mathexpr::fmt((gate * ppq * 1000.0).round() / 1000.0)
}
