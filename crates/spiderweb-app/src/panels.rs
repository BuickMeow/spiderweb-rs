//! Toolbar and right side panel (upstream window/app.py's _build / _build_side and the panels).

use eframe::egui;

use spiderweb_core::funnel::{self, CURVE_PRESETS, FunnelSettings};
use spiderweb_core::shape::{
    Align, Ends, Fill, FunnelCurve, FunnelFill, GateChange, GateFollow, Kind, Stroke, TextAlign,
    TextSettings, TextUnit, Tumour, WallMode,
};
use spiderweb_core::smooth::clean_level;
use spiderweb_core::text::{self, TextChange};
use spiderweb_domino::DominoStart;
use spiderweb_io::project::{ChannelMode, ChannelSplit, FunnelDefaults};

use crate::app::{App, Tool};
use crate::roll_funnel;

/// The order of the Project -> Domino start dropdown (upstream `DOMINO_STARTS`).
const DOMINO_STARTS: [DominoStart; 2] = [DominoStart::Note, DominoStart::Bar];

/// A shape spread over more channels than this is shown orange in the shape list (upstream MANY_CHANNELS).
const MANY_CHANNELS: usize = 15;
/// The orange for that (upstream panel_custom.GAP_COLOR).
const GAP_COLOR: egui::Color32 = egui::Color32::from_rgb(0xc0, 0x60, 0x00);

/// The dropdown labels (the tr texts of upstream `DOMINO_STARTS`).
fn domino_start_text(start: DominoStart) -> String {
    match start {
        DominoStart::Note => rust_i18n::t!("domino_clip.first_note_at_tick_0"),
        DominoStart::Bar => rust_i18n::t!("domino_clip.from_the_bar_line"),
    }
    .to_string()
}

/// Font weight choices (fonts.WEIGHTS).
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

/// Closest weight name (the min(WEIGHTS, ...) of upstream sync_text).
fn weight_name(w: i32) -> &'static str {
    WEIGHTS
        .iter()
        .min_by_key(|(v, _)| (v - w).abs())
        .map(|(_, n)| *n)
        .unwrap_or("Regular")
}

/// A number row in the panel (label / unit / range / scrub step of upstream ENTRIES).
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

/// What the freehand panel acts on: a free shape, or the picked free stroke of the only selected custom shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FreeTarget {
    Shape(usize),
    Stroke(usize, usize),
}

/// Shapes that can carry tumours (upstream LINE_KINDS).
fn tumour_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Line | Kind::Poly | Kind::Free | Kind::Curve | Kind::Arc
    )
}

/// A topic's tip text (tooltip of toolbar buttons; empty when not found).
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
                // Show this tool's tip on the button (upstream widgets.Tooltip)
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
                    // Pop this tool's tip when switching tools (seen ones don't pop again)
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
            crate::snap_picker::snap_picker_ui(self, ui);
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
            ui.checkbox(&mut self.show_history, rust_i18n::t!("toolbar.history"))
                .on_hover_text(rust_i18n::t!("toolbar.history_tip"));
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
            // First time a kind of shape is selected: show how to edit it (upstream sync_panel)
            self.show_kind_tip();
        }
        self.project_section(ui);
        crate::history::history_section(self, ui);
        self.shapes_section(ui);
        self.defaults_section(ui);
        self.freehand_section(ui);
        self.tumour_section(ui);
        self.text_section(ui);
        self.custom_section(ui);
        self.points_section(ui);
        self.funnel_section(ui);
        // Bottom of the side panel: help for the current tool
        crate::help::side_help_ui(self, ui);
    }

    /// The tip for the selected shapes' kind (one each for custom / funnel / free).
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

    // ------------------------------------------------------------ freehand

    /// What "straighten" in the panel acts on: selected free shapes; if none, the picked
    /// free stroke of the only selected custom shape (upstream free_targets).
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

    /// Freehand panel (upstream panel_freehand): Straighten sensitivity, 0 = leave as is.
    fn freehand_section(&mut self, ui: &mut egui::Ui) {
        let targets = self.free_targets();
        if targets.is_empty() && !self.sels.is_empty() {
            return; // nothing selected is freehand
        }
        if targets.is_empty() && self.tool != Tool::Free {
            return; // nothing selected: sensitivity for new strokes is only adjustable with the Freehand tool
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

    /// Straighten changed: new strokes use this value; selected targets are written back (free shapes / free strokes of custom shapes).
    fn set_free_smooth(&mut self, value: i64, targets: &[FreeTarget], fresh: bool) {
        self.free_smooth = value; // new free strokes use it too (upstream free_smooth)
        if targets.is_empty() {
            self.schedule_autosave();
            return;
        }
        if fresh {
            self.edit_key = None; // a fresh drag = a new undo step
        }
        let key = format!("smooth:{:?}", self.sels);
        if self.edit_key.as_deref() != Some(key.as_str()) {
            self.push_undo(&rust_i18n::t!("panel.freehand.straighten"));
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

    // ------------------------------------------------------------ tumours

    /// Current screen ratio: how many beats correspond to one key (upstream `roll.sy / roll.sx`, fallback 0.25).
    pub(crate) fn tumour_k(&self) -> f64 {
        if self.view.sx != 0.0 {
            self.view.sy / self.view.sx
        } else {
            0.25
        }
    }

    /// Tumour shapes the panel acts on: selected line / poly / free / curve / arc (upstream tumour_targets).
    pub(crate) fn tumour_targets(&self) -> Vec<usize> {
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

    /// The side panel's tumour line (upstream panel_tumour): a short summary and a button that
    /// opens the tumour window (tumour_window.rs), where the settings are.
    fn tumour_section(&mut self, ui: &mut egui::Ui) {
        let targets = self.tumour_targets();
        if targets.is_empty() {
            return; // nothing selected that can have tumours: the row isn't shown
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(rust_i18n::t!("panel_tumour.tumours"))
                .on_hover_text(rust_i18n::t!("panel_tumour.bumps_along_the_line_opens_the"))
                .clicked()
            {
                crate::tumour_window::open_tumour_window(self);
            }
            ui.label(egui::RichText::new(self.tumour_summary()).weak());
        });
    }

    /// The summary line next to the button (upstream sync_tumour_summary).
    fn tumour_summary(&self) -> String {
        let tgts = self.tumour_targets();
        if tgts.is_empty() {
            return String::new();
        }
        if tgts
            .iter()
            .any(|&i| self.shapes.get(i).is_some_and(|s| !s.tumours.is_empty()))
        {
            return rust_i18n::t!("panel_tumour.each_joined_shape_has_its_own").to_string();
        }
        let on: Vec<&Tumour> = tgts
            .iter()
            .filter_map(|&i| spiderweb_core::joined::shown_tumour(&self.shapes[i]))
            .filter(|t| t.on)
            .collect();
        if on.is_empty() {
            return rust_i18n::t!("panel_tumour.no_tumours").to_string();
        }
        if tgts.len() > 1 {
            return rust_i18n::t!(
                "panel_tumour.tumours_on_of",
                n = on.len().to_string(),
                n2 = tgts.len().to_string()
            )
            .to_string();
        }
        let tm = on[0];
        let mut text = rust_i18n::t!(
            "panel_tumour.keys",
            dict = crate::tumour_window::tumour_shape_name(tm.shape),
            size = spiderweb_io::mathexpr::fmt((tm.size * 100.0).round() / 100.0)
        )
        .to_string();
        if !tm.graphs.is_empty() {
            text.push_str(&rust_i18n::t!("panel_tumour.with_graphs"));
        }
        text
    }

    // ------------------------------------------------------------ custom shape panel

    /// Custom shapes the panel acts on: the selected ones; with nothing selected it is "default settings for new shapes" (upstream custom_targets).
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

    /// Refreshes the gate text to the current shape's ticks when the selection changes (upstream sync_custom).
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
        // A selection that can become one live shape still shows this panel, for the button.
        let can_turn = crate::convert_ui::live_problem(self).is_none();
        if !self.sels.is_empty() && placed.is_empty() && !can_turn {
            return; // the selection has no custom shapes and can't become one live shape
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
            .show(ui, |ui| {
                crate::convert_ui::live_button_ui(self, ui);
                self.custom_body_ui(ui, &placed);
            });
    }

    fn custom_ends_label(ends: Ends) -> String {
        match ends {
            Ends::Round => rust_i18n::t!("panel.custom.ends_round"),
            Ends::Keep => rust_i18n::t!("panel.custom.ends_keep"),
            Ends::Drop => rust_i18n::t!("panel.custom.ends_drop"),
            Ends::Min => rust_i18n::t!("panel.custom.ends_min"),
            Ends::Stretch => rust_i18n::t!("panel.custom.ends_stretch"),
        }
        .to_string()
    }

    fn custom_body_ui(&mut self, ui: &mut egui::Ui, placed: &[usize]) {
        let placed_mode = !placed.is_empty();
        let (fill, align, ends, union, apart, name) = if placed_mode {
            match self.shapes.get(placed[0]) {
                Some(sh) => (
                    sh.fill,
                    sh.align,
                    sh.ends,
                    sh.union,
                    sh.apart,
                    sh.name.clone(),
                ),
                None => return,
            }
        } else {
            (
                self.custom_defaults.fill,
                self.custom_defaults.align,
                self.custom_defaults.ends,
                self.custom_defaults.union,
                self.custom_defaults.apart,
                self.custom_defaults.shape.clone(),
            )
        };
        let pasted = placed_mode
            && placed
                .iter()
                .any(|&i| self.shapes.get(i).is_some_and(|s| s.notes.is_some()));
        // Number of gaps in the outline (1.2.0: Fill / Spam closes them all with straight lines; only used as a hint here)
        let gaps = if placed_mode {
            placed
                .iter()
                .filter_map(|&i| self.shapes.get(i))
                .map(|s| spiderweb_core::custom::gap_lines(s).len())
                .max()
                .unwrap_or(0)
        } else if self.tool != Tool::Custom {
            0
        } else {
            crate::roll_live::builtin_template(&self.library_dir, &name)
                .map(|(st, _)| spiderweb_core::custom::open_paths(&st).len())
                .unwrap_or(0)
        };
        let spam = matches!(fill, Fill::Spam | Fill::OutlineSpam);
        let alignable = spam && ends != Ends::Stretch; // the Stretch gate exactly fills each piece: the start doesn't matter

        let mut pick: Option<String> = None;
        let mut new_fill: Option<Fill> = None;
        let mut new_align: Option<Align> = None;
        let mut new_ends: Option<Ends> = None;
        let mut new_union: Option<bool> = None;
        let mut new_apart: Option<bool> = None;
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
            let own = placed
                .iter()
                .all(|&i| self.shapes.get(i).is_some_and(|s| s.own_vel));
            let mut info = rust_i18n::t!(
                "panel.custom.info_pasted",
                total = crate::app::fmt_int(total)
            )
            .to_string();
            if own {
                info += rust_i18n::t!("panel.custom.they_keep_their_own_velocities").as_ref();
            }
            info += rust_i18n::t!("panel.custom.info_pasted_drag").as_ref();
            ui.label(info);
        } else {
            ui.label(rust_i18n::t!("panel.custom.inside"));
            for (label, tip, value) in [
                (
                    rust_i18n::t!("panel.custom.empty"),
                    rust_i18n::t!("panel.custom.empty_tip"),
                    Fill::Empty,
                ),
                (
                    rust_i18n::t!("panel.custom.fill"),
                    rust_i18n::t!("panel.custom.fill_tip"),
                    Fill::Fill,
                ),
                (
                    rust_i18n::t!("panel.custom.spam"),
                    rust_i18n::t!("panel.custom.spam_tip"),
                    Fill::Spam,
                ),
                (
                    rust_i18n::t!("panel.custom.outline_spam"),
                    rust_i18n::t!("panel.custom.outline_spam_tip"),
                    Fill::OutlineSpam,
                ),
            ] {
                if ui.radio(fill == value, label).on_hover_text(tip).clicked() {
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
                        apply_gate = true; // Enter or clicking elsewhere applies it (upstream Return / FocusOut)
                    }
                    ui.label(rust_i18n::t!("panel.custom.gate_hint"));
                });
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(spam, |ui| {
                    ui.label(rust_i18n::t!("panel.custom.ends"));
                    egui::ComboBox::from_id_salt("custom_ends")
                        .selected_text(Self::custom_ends_label(ends))
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for e in spiderweb_core::custom::ENDS {
                                if ui
                                    .selectable_label(ends == e, Self::custom_ends_label(e))
                                    .clicked()
                                {
                                    new_ends = Some(e);
                                }
                            }
                        });
                })
                .response
                .on_hover_text(rust_i18n::t!("panel.custom.ends_tip"));
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(alignable, |ui| {
                    ui.label(rust_i18n::t!("panel.custom.start"));
                    for (label, value, hover) in [
                        (
                            rust_i18n::t!("panel.custom.auto"),
                            Align::Auto,
                            rust_i18n::t!("panel.custom.auto_tip"),
                        ),
                        (
                            rust_i18n::t!("panel.custom.aligned"),
                            Align::Aligned,
                            rust_i18n::t!("panel.custom.aligned_tip"),
                        ),
                        (
                            rust_i18n::t!("panel.custom.centred"),
                            Align::Centred,
                            rust_i18n::t!("panel.custom.centred_tip"),
                        ),
                    ] {
                        if ui
                            .radio(align == value, label)
                            .on_hover_text(hover)
                            .clicked()
                        {
                            new_align = Some(value);
                        }
                    }
                });
            });
            let mut cancel = !union;
            let resp = ui.add_enabled(
                matches!(fill, Fill::Fill | Fill::Spam),
                egui::Checkbox::new(
                    &mut cancel,
                    rust_i18n::t!("panel.custom.overlaps_cancel_out"),
                ),
            );
            if resp.changed() {
                new_union = Some(!cancel);
            }
            resp.on_hover_text(rust_i18n::t!(
                "panel.custom.fill_and_spam_where_outlines_overlap"
            ));
            ui.horizontal(|ui| {
                ui.add_enabled_ui(matches!(fill, Fill::Fill | Fill::Spam), |ui| {
                    ui.label(rust_i18n::t!("panel.custom.normal_outline"));
                    egui::ComboBox::from_id_salt("custom_apart")
                        .selected_text(if apart {
                            rust_i18n::t!("panel.custom.outline")
                        } else {
                            rust_i18n::t!("panel.custom.normal")
                        })
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(!apart, rust_i18n::t!("panel.custom.normal"))
                                .clicked()
                            {
                                new_apart = Some(false);
                            }
                            if ui
                                .selectable_label(apart, rust_i18n::t!("panel.custom.outline"))
                                .clicked()
                            {
                                new_apart = Some(true);
                            }
                        });
                })
                .response
                .on_hover_text(rust_i18n::t!(
                    "panel.custom.normal_all_its_notes_together_outline"
                ));
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
            let mut info = rust_i18n::t!(
                "panel.custom.info_notes",
                total = crate::app::fmt_int(total)
            )
            .to_string();
            if gaps > 0 && matches!(fill, Fill::Fill | Fill::Spam) {
                if gaps == 1 {
                    info += &format!("  {}", rust_i18n::t!("panel.custom.info_one_gap"));
                } else {
                    info += &format!(
                        "  {}",
                        rust_i18n::t!("panel.custom.info_gaps", gaps = gaps.to_string())
                    );
                }
                info += rust_i18n::t!("panel.custom.ends_that_nearly_touch_1_64").as_ref();
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
        if let Some(ends) = new_ends {
            self.set_custom_ends(ends, placed);
        }
        if let Some(union) = new_union {
            self.set_custom_union(union, placed);
        }
        if let Some(apart) = new_apart {
            self.set_custom_apart(apart, placed);
        }
        if apply_gate {
            self.apply_custom_gate(placed);
        }
    }

    /// A template (built-in or from the shape library) was picked in the panel: new shapes use it and selected custom shapes are switched to it.
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
        self.push_undo(&rust_i18n::t!("tool.custom"));
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
        self.push_undo(&rust_i18n::t!("panel.custom.inside_fill"));
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
        self.push_undo(&rust_i18n::t!("panel.custom.spam_start"));
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.align = align;
            }
        }
        self.shapes_changed();
    }

    fn set_custom_ends(&mut self, ends: Ends, placed: &[usize]) {
        if placed.is_empty() {
            self.custom_defaults.ends = ends;
            self.schedule_autosave();
            return;
        }
        self.push_undo(&rust_i18n::t!("panel.custom.spam_ends"));
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.ends = ends;
            }
        }
        self.shapes_changed();
    }

    /// "Overlaps cancel out": checked = union off (overlaps cancel, even-odd).
    fn set_custom_union(&mut self, union: bool, placed: &[usize]) {
        if placed.is_empty() {
            self.custom_defaults.union = union;
            self.schedule_autosave();
            return;
        }
        self.push_undo(&rust_i18n::t!("panel.custom.overlaps_cancel_out"));
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.union = union;
            }
        }
        self.shapes_changed();
    }

    fn set_custom_apart(&mut self, apart: bool, placed: &[usize]) {
        if placed.is_empty() {
            self.custom_defaults.apart = apart;
            self.schedule_autosave();
            return;
        }
        self.push_undo(&rust_i18n::t!("panel.custom.normal_outline"));
        for &i in placed {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.apart = apart;
            }
        }
        self.shapes_changed();
    }

    /// Applies the gate text box (ticks, a mathexpr expression) as the shape's beat count (upstream on_gate).
    fn apply_custom_gate(&mut self, placed: &[usize]) {
        // Invalid input: upstream just marks the box red (Bad.TEntry) with no message
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
        self.push_undo(&rust_i18n::t!("panel.custom.spam_gate"));
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
                        // 128 / 256 keys (Project -> Keys in 1.2.0)
                        ui.label(rust_i18n::t!("panel.project.keys"));
                        let keys_before = self.keys;
                        egui::ComboBox::from_id_salt("keys")
                            .selected_text(self.keys.to_string())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for k in spiderweb_core::paths::KEYS {
                                    ui.selectable_value(&mut self.keys, k, k.to_string());
                                }
                            })
                            .response
                            .on_hover_text(rust_i18n::t!("panel.project.keys_tip"));
                        if self.keys != keys_before {
                            self.on_keys_change();
                        }
                        ui.end_row();
                        ui.label(rust_i18n::t!("panel.project.output"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.pvar.output).desired_width(180.0),
                        );
                        ui.end_row();
                        // 1.2.0's Domino start: where copying / pasting starts (upstream Project -> Domino start)
                        ui.label(rust_i18n::t!("panel.project.domino_start"));
                        let domino_before = self.domino_start;
                        egui::ComboBox::from_id_salt("domino_start")
                            .selected_text(domino_start_text(self.domino_start))
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                for start in DOMINO_STARTS {
                                    ui.selectable_value(
                                        &mut self.domino_start,
                                        start,
                                        domino_start_text(start),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(
                                rust_i18n::t!("panel.project.domino_start_tip").to_string(),
                            );
                        // upstream `domino_box.bind(<<ComboboxSelected>>, schedule_autosave)`
                        if self.domino_start != domino_before {
                            self.schedule_autosave();
                        }
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
                for (value, label, tip) in [
                    (
                        ChannelMode::Raw,
                        rust_i18n::t!("panel.project.mode_raw"),
                        rust_i18n::t!("panel.project.mode_raw_tip"),
                    ),
                    (
                        ChannelMode::Single,
                        rust_i18n::t!("panel.project.mode_single"),
                        rust_i18n::t!("panel.project.mode_single_tip"),
                    ),
                    (
                        ChannelMode::Auto,
                        rust_i18n::t!("panel.project.mode_auto"),
                        rust_i18n::t!("panel.project.mode_auto_tip"),
                    ),
                ] {
                    ui.radio_value(&mut self.channel_mode, value, label)
                        .on_hover_text(tip);
                }
                if self.channel_mode == ChannelMode::Auto {
                    ui.horizontal(|ui| {
                        ui.label(rust_i18n::t!("panel.project.split"));
                        ui.radio_value(
                            &mut self.channel_split,
                            ChannelSplit::Key,
                            rust_i18n::t!("panel.project.split_key"),
                        )
                        .on_hover_text(rust_i18n::t!("panel.project.split_tip"));
                        ui.radio_value(
                            &mut self.channel_split,
                            ChannelSplit::Time,
                            rust_i18n::t!("panel.project.split_time"),
                        )
                        .on_hover_text(rust_i18n::t!("panel.project.split_tip"));
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
                        let labels: Vec<(usize, String, bool)> = self
                            .shapes
                            .iter()
                            .enumerate()
                            .map(|(i, sh)| {
                                let count = self.note_counts.get(i).copied().unwrap_or(0);
                                let chans = self.shape_channels.get(i).copied().unwrap_or(1);
                                let uses = if chans > 1 {
                                    rust_i18n::t!(
                                        "panel.shapes.channels",
                                        chans = chans.to_string()
                                    )
                                    .to_string()
                                } else {
                                    String::new()
                                };
                                (
                                    i,
                                    rust_i18n::t!(
                                        "panel.shapes.item",
                                        i = (i + 1).to_string(),
                                        label = self.shape_label(sh),
                                        notes = crate::app::fmt_int(count as i64),
                                        uses = uses
                                    )
                                    .to_string(),
                                    chans > MANY_CHANNELS,
                                )
                            })
                            .collect();
                        let mut clicked: Option<usize> = None;
                        let mut toggle = false;
                        for (i, label, many) in labels {
                            let selected = self.sels.contains(&i);
                            // Past 15 channels the note colours and channel numbers repeat: orange.
                            let text = if many {
                                egui::RichText::new(label).color(GAP_COLOR)
                            } else {
                                egui::RichText::new(label)
                            };
                            let resp = ui.selectable_label(selected, text);
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
                // Join / Split (upstream window/join_split.py's buttons under the shape list)
                let base_join = rust_i18n::t!("join.join_tip").to_string();
                let problem = self.join_problem();
                let tip = match &problem {
                    Some(p) => format!("{base_join}\n\n{p}"),
                    None => base_join,
                };
                ui.horizontal(|ui| {
                    let resp = ui.add_enabled(
                        problem.is_none(),
                        egui::Button::new(rust_i18n::t!("join.button_join")),
                    );
                    if resp.clicked() {
                        self.join_selected();
                    }
                    if problem.is_none() {
                        resp.on_hover_text(tip);
                    } else {
                        resp.on_disabled_hover_text(tip);
                    }
                    let base_split = format!(
                        "{}\n{}",
                        rust_i18n::t!("join.split_tip"),
                        rust_i18n::t!("join.split_here_tip")
                    );
                    let problem = self.split_problem();
                    let tip = match &problem {
                        Some(p) => format!("{base_split}\n\n{p}"),
                        None => base_split,
                    };
                    let resp = ui.add_enabled(
                        problem.is_none(),
                        egui::Button::new(rust_i18n::t!("join.button_split")),
                    );
                    if resp.clicked() {
                        self.split_selected();
                    }
                    if problem.is_none() {
                        resp.on_hover_text(tip);
                    } else {
                        resp.on_disabled_hover_text(tip);
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
                    ui.label("->");
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
        self.push_undo(&rust_i18n::t!("panel.defaults.velocity"));
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
        self.push_undo(&rust_i18n::t!("panel.defaults.last_note"));
        for i in self.sels.iter().copied().collect::<Vec<_>>() {
            if let Some(sh) = self.shapes.get_mut(i) {
                sh.end_dot = value;
            }
        }
        self.shapes_changed();
    }

    // ------------------------------------------------------------ text panel

    /// Selected text shapes (sorted by index) (upstream text_shapes).
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

    /// What the panel shows (settings, size box number): the text being typed, the first
    /// selected text, or the defaults for new text (upstream text_current; size follows the
    /// axes, see shown_size).
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

    /// Text panel (upstream panel_text): font / size / weight / letter spacing / line spacing / align / threshold / bold.
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
        let max_pitch = (self.keys - 1) as f64;
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
                                        .range(0.0..=max_pitch),
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

    // ------------------------------------------------------------ funnel

    /// Shapes the funnel panel acts on: selected funnels (upstream funnel_targets).
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

    /// Refreshes the gate text boxes to the selected funnel (or the defaults) (the text part of upstream sync_funnel).
    pub fn sync_funnel_text(&mut self) {
        let cur = self
            .selected()
            .filter(|sh| sh.kind == Kind::Funnel)
            .map(roll_funnel::settings_of_shape)
            .unwrap_or_else(|| roll_funnel::settings_of_defaults(&self.funnel_defaults));
        let ppq = self.ppq.max(1) as f64;
        self.funnel_text = [fmt_ticks(cur.gate0, ppq), fmt_ticks(cur.gate1, ppq)];
    }

    /// Funnel settings panel (upstream panel_funnel._build_funnel / sync_funnel): fill, wall,
    /// gate, variation and follow, curve presets / formulas, inside-out and swapping the ends.
    fn funnel_section(&mut self, ui: &mut egui::Ui) {
        let targets = self.funnel_target_indices();
        if targets.is_empty() && !self.sels.is_empty() {
            return;
        }
        if targets.is_empty() && self.tool != Tool::Funnel {
            return; // nothing selected: settings for new funnels are only adjustable with the Funnel tool
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
            let mut s = rust_i18n::t!(
                "panel.funnel.info_notes",
                n = crate::app::fmt_int(note_total)
            )
            .to_string();
            if !parts_text.is_empty() {
                s += rust_i18n::t!("panel.funnel.info_highlighted", parts = parts_text).as_ref();
            } else if targets.len() == 1 {
                // Upstream: the first hint (no curve starts yet) explains that the
                // start is placed where you middle-click; the other one is the reminder.
                let no_starts = self
                    .shapes
                    .get(targets[0])
                    .map(|sh| sh.starts.is_empty())
                    .unwrap_or(false);
                let hint = if no_starts {
                    rust_i18n::t!("panel.funnel.info_no_starts")
                } else {
                    rust_i18n::t!("panel.funnel.info_start_curve")
                };
                s += hint.as_ref();
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
                    // A reversed funnel's wall is at the front: swap the two wordings accordingly (upstream sync_funnel)
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
                        ui.label("->");
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

    /// Changes funnel settings for the selected funnels (or the defaults when nothing is selected) (upstream set_funnel).
    fn apply_funnel_settings(&mut self, s: FunnelSettings) {
        let targets = self.funnel_target_indices();
        if !targets.is_empty() {
            self.push_undo(&rust_i18n::t!("panel.funnel.funnel_setting"));
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

    /// Applies a preset curve (CURVE_PRESETS) to the highlighted curve (the preset part of upstream apply_formula).
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
                // The default curve cannot fail to compute (upstream preset_curve(None) also never errors)
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

    /// Applies a hand-written formula to the highlighted curve (the formula part of upstream apply_formula).
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

    /// Gives a curve shape to the highlighted curve (upstream set_curves: does nothing and shows nothing when nothing is highlighted).
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

/// Gate shown in ticks (upstream fmt(round(t[key] * ppq, 3))).
fn fmt_ticks(gate: f64, ppq: f64) -> String {
    spiderweb_io::mathexpr::fmt((gate * ppq * 1000.0).round() / 1000.0)
}
