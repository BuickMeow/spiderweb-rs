//! 工具栏与右侧面板（原版 window/app.py 的 _build / _build_side 及各面板）。

use eframe::egui;

use spiderweb_core::funnel::{self, CURVE_PRESETS, FunnelSettings};
use spiderweb_core::shape::{
    Align, Fill, FunnelCurve, FunnelFill, GateChange, GateFollow, Kind, TextAlign, TextSettings,
    TextUnit, WallMode,
};
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
struct NumberRow<'a> {
    label: &'a str,
    unit: &'a str,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
}

fn number_row(
    ui: &mut egui::Ui,
    row: NumberRow<'_>,
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

impl App {
    pub fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let box_tools_on = self.tool == Tool::Custom || self.tool.is_box();
            for tool in Tool::ALL {
                if tool.is_box() && !box_tools_on {
                    continue;
                }
                let selected = self.tool == tool;
                let text = format!("{} ({})", tool.label(), tool.hotkey().to_uppercase());
                if ui.selectable_label(selected, text).clicked() && self.tool != tool {
                    if tool != Tool::Select {
                        self.draw_tool = tool;
                    }
                    self.tool = tool;
                    self.cancel_draft();
                }
            }
            ui.separator();
            ui.checkbox(&mut self.live, "Live shape (G)");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Snap");
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
            ui.checkbox(&mut self.show_lines, "Show lines").changed();
            ui.checkbox(&mut self.show_notes, "Show notes");
            ui.checkbox(&mut self.show_velocity, "Velocity pane");
            if ui.button("Fit view").clicked() {
                self.view.fit_shapes(&self.shapes, self.beats);
            }
            if ui.button("Undo").clicked() {
                self.undo();
            }
            if ui.button("Redo").clicked() {
                self.redo();
            }
            let play_label = if self.player.running() {
                "■ Stop (Space)"
            } else {
                "▶ Play (Space)"
            };
            if ui.button(play_label).clicked() {
                self.toggle_play();
            }
            if ui.button("Help (F1)").clicked() {
                self.status = "帮助窗口待移植".to_string();
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
        }
        self.project_section(ui);
        self.shapes_section(ui);
        self.defaults_section(ui);
        self.text_section(ui);
        self.custom_section(ui);
        self.points_section(ui);
        self.funnel_section(ui);
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
            "New custom shape".to_string()
        } else if placed.len() > 1 {
            format!("Custom shapes ({})", placed.len())
        } else {
            "Custom shape".to_string()
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
            crate::roll_live::builtin_template(&name)
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

        ui.horizontal(|ui| {
            ui.label("Shape");
            egui::ComboBox::from_id_salt("custom_shape")
                .selected_text(name.clone())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for n in ["Circle", "Square", "Triangle"] {
                        if ui.selectable_label(name == n, n).clicked() && name != n {
                            pick = Some(n.to_string());
                        }
                    }
                });
            if ui.button("Drawer…").clicked() {
                self.status =
                    "Drawer 抽屉窗口待移植：先用内置 Circle / Square / Triangle".to_string();
            }
        });
        if pasted {
            let total: i64 = placed
                .iter()
                .filter_map(|&i| self.shapes.get(i))
                .map(|s| self.note_count(s))
                .sum();
            ui.label(format!(
                "{total} 个粘贴的音符。拖角点 / 边缩放，角外旋转，边中外斜切。"
            ));
        } else {
            ui.label("Inside");
            for (label, value) in [
                ("Empty (outline only)", Fill::Empty),
                ("Fill (one long note per key)", Fill::Fill),
                ("Spam (notes of one gate)", Fill::Spam),
                (
                    "Outline spam (the outline in notes of one gate)",
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
                    ui.label("gate");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.custom_gate_text).desired_width(70.0),
                    );
                    if resp.lost_focus() {
                        apply_gate = true; // Enter 或点到别处都应用（原版 Return / FocusOut）
                    }
                    ui.label("ticks (Enter to apply)");
                });
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(spam, |ui| {
                    ui.label("start");
                    if ui
                        .add_enabled(
                            align != Align::Auto,
                            egui::RadioButton::new(align == Align::Auto, "Auto"),
                        )
                        .clicked()
                    {
                        new_align = Some(Align::Auto);
                    }
                    if ui
                        .add_enabled(
                            align != Align::Aligned,
                            egui::RadioButton::new(align == Align::Aligned, "Aligned"),
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
            let mut info = format!("{total} 个音符。");
            if gaps == 1 && matches!(fill, Fill::Fill | Fill::Spam) {
                info += "  轮廓有一个缺口：按虚线补直接上。";
            } else if gaps > 1 {
                info += &format!("  轮廓有 {gaps} 个缺口：只能 Empty / Outline spam。");
            }
            if let Some(k) = self.stroke
                && placed.len() == 1
                && let Some(sh) = self.shapes.get(placed[0])
            {
                info += &format!(
                    "  已拾取第 {} / {} 条笔画（Del 删除，Esc 取消拾取）。",
                    k + 1,
                    sh.strokes.len()
                );
            }
            info
        } else if self.live && crate::roll_live::is_stroke_tool(self.tool) {
            "Live shape：画下的东西进同一个自定义形状（没有选中的就新建一个）。轮廓要填就先闭合。"
                .to_string()
        } else if crate::roll_live::builtin_template(&name).is_none() {
            "选一个形状，或用 Drawer… 画一个。".to_string()
        } else if self.tool.is_box() {
            format!(
                "在卷帘上拖一个框，或点两个角（Ctrl = 屏幕上正的 {}）。",
                self.tool.label()
            )
        } else {
            "在卷帘上拖一个框，或点两个角放置（Ctrl = 保持比例）。".to_string()
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

    /// 面板里选了一个内置模板：新形状用它，选中的自定义形状也换成它。
    fn pick_custom_template(&mut self, name: &str, placed: &[usize]) {
        let Some((strokes, _)) = crate::roll_live::builtin_template(name) else {
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
        let Ok(ticks) =
            spiderweb_io::mathexpr::calc_int(&self.custom_gate_text, Some(1), Some(10_000_000))
        else {
            self.status = "gate 要是 1..10000000 ticks 的表达式".to_string();
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
        egui::CollapsingHeader::new("Project")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("project_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("PPQ");
                        if ui
                            .add(egui::TextEdit::singleline(&mut self.pvar.ppq).desired_width(90.0))
                            .changed()
                        {
                            self.on_project_change();
                        }
                        ui.end_row();
                        ui.label("BPM");
                        if ui
                            .add(egui::TextEdit::singleline(&mut self.pvar.bpm).desired_width(90.0))
                            .changed()
                        {
                            self.on_project_change();
                        }
                        ui.end_row();
                        ui.label("Beats per bar");
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
                        ui.label("Output file");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.pvar.output).desired_width(180.0),
                        );
                        ui.end_row();
                    });
                if self.ppq >= 32767 {
                    ui.colored_label(
                        egui::Color32::from_rgb(0xd0, 0x00, 0x00),
                        "Many programs can't open this PPQ",
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Open…").clicked() {
                        self.open_project();
                    }
                    if ui.button("Save…").clicked() {
                        self.save_project_as();
                    }
                    if ui.button("Generate MIDI").clicked() {
                        self.generate_midi();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Paste from Domino").clicked() {
                        self.paste_from_domino();
                    }
                    if ui.button("Copy to Domino").clicked() {
                        self.copy_to_domino();
                    }
                });
                ui.label("Channels");
                ui.radio_value(
                    &mut self.channel_mode,
                    ChannelMode::Raw,
                    "As drawn (keep overlaps)",
                );
                ui.radio_value(
                    &mut self.channel_mode,
                    ChannelMode::Single,
                    "Single channel (remove overlaps)",
                );
                ui.radio_value(
                    &mut self.channel_mode,
                    ChannelMode::Auto,
                    "Multi channel (a channel per overlap)",
                );
                if self.channel_mode == ChannelMode::Auto {
                    ui.horizontal(|ui| {
                        ui.label("Split");
                        ui.radio_value(
                            &mut self.channel_split,
                            ChannelSplit::Key,
                            "Same key at the same time",
                        );
                        ui.radio_value(
                            &mut self.channel_split,
                            ChannelSplit::Time,
                            "Any notes at the same time",
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("MIDI out");
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
        egui::CollapsingHeader::new("Shapes")
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
                                    format!(
                                        "{}.  {}  —  {} notes",
                                        i + 1,
                                        self.shape_label(sh),
                                        count
                                    ),
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
                    if ui.button("Duplicate").clicked() {
                        self.duplicate();
                    }
                    if ui.button("Delete").clicked() {
                        self.delete_selected();
                    }
                    if ui.button("Delete all").clicked() {
                        self.delete_all();
                    }
                });
            });
    }

    fn defaults_section(&mut self, ui: &mut egui::Ui) {
        let title = match self.selected() {
            Some(sh) => {
                let extra = if self.sels.len() > 1 {
                    format!("  (+{} more selected)", self.sels.len() - 1)
                } else {
                    String::new()
                };
                format!(
                    "Shape {}: {}{}",
                    self.sel.unwrap_or(0) + 1,
                    self.shape_label(sh),
                    extra
                )
            }
            None => "New shape defaults".to_string(),
        };
        egui::CollapsingHeader::new(title)
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Velocity");
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
                    ui.label("Last note");
                    let mut end_dot = self.target_end_dot();
                    let r0 = ui.radio_value(&mut end_dot, false, "ends on the last point");
                    let r1 = ui.radio_value(&mut end_dot, true, "starts exactly on the last point");
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
        let font_found = text::text_font(&tx).found();
        let mut changes = TextChange::default();
        let mut any = false;
        egui::CollapsingHeader::new("Text")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Font");
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
                        .button("选择字体…")
                        .on_hover_text(
                            "Pick the font (you can type its name in the window that opens).",
                        )
                        .clicked()
                    {
                        crate::text_dialog::open_font_dialog(self, &tx);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Size");
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
                        .selectable_label(tx.unit == TextUnit::Font, "Font size")
                        .clicked()
                    {
                        changes.unit = Some(TextUnit::Font);
                        any = true;
                    }
                    if ui
                        .selectable_label(tx.unit == TextUnit::Rows, "Rows")
                        .clicked()
                    {
                        changes.unit = Some(TextUnit::Rows);
                        any = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Weight");
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
                    if ui.checkbox(&mut italic, "Italic").changed() {
                        changes.italic = Some(italic);
                        any = true;
                    }
                });
                number_row(
                    ui,
                    NumberRow {
                        label: "Letter spacing",
                        unit: "/1000 em",
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
                        label: "Line spacing",
                        unit: "%",
                        value: tx.leading,
                        range: 1.0..=1000.0,
                        speed: 5.0,
                    },
                    &mut changes,
                    &mut any,
                    |c, v| c.leading = Some(v),
                );
                ui.horizontal(|ui| {
                    ui.label("Align");
                    for (align, label) in [
                        (TextAlign::Left, "Left"),
                        (TextAlign::Center, "Centre"),
                        (TextAlign::Right, "Right"),
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
                        label: "Threshold",
                        unit: "%",
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
                        label: "Grow",
                        unit: "keys",
                        value: tx.grow,
                        range: -100.0..=100.0,
                        speed: 0.1,
                    },
                    &mut changes,
                    &mut any,
                    |c, v| c.grow = Some(v),
                );
                let info = if !font_found {
                    format!(
                        "“{}” isn't installed on this PC. The letters stay as they were saved.",
                        tx.font
                    )
                } else if self.typing.is_some() {
                    "Typing: Enter = new line, Esc = done. Click somewhere else for a new text, on a text to retype it.".to_string()
                } else if self.tool == Tool::Text {
                    "Click on the piano roll and type. Click a text to retype it.".to_string()
                } else {
                    "Double-click the text (or right-click → Edit text) to retype it.".to_string()
                };
                let info = if text_shapes.len() > 1 && self.typing.is_none() {
                    format!("{info}  Changes go to all {} selected texts.", text_shapes.len())
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
        egui::CollapsingHeader::new("Points")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("points_grid")
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label("Point");
                        ui.label("Tick");
                        ui.label("Pitch");
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
            "再画墙（Ctrl = 关于线对称，右键 = 取消）。".to_string()
        } else if placed {
            let mut s = format!("{note_total} notes.  ");
            if !parts_text.is_empty() {
                s += &format!(
                    "已高亮：{parts_text}。右键 = 曲线形状，Del = 删除，Esc = 清除，Ctrl+点击 = 加 / 减。"
                );
            } else if targets.len() == 1 {
                s += "中键点线 = 新起点，靠近曲线 = 加锚点；Select 再点曲线 = 高亮。";
            }
            s
        } else {
            "先画漏斗的线，再画墙（拖动或各点两下）。".to_string()
        };

        let mut new = cur;
        let mut text = self.funnel_text.clone();
        let mut text_changed = false;
        let mut preset_pick: Option<usize> = None;
        let mut inside_out = false;
        let mut turn = false;
        let mut apply_formula = false;
        let mut formula_text = self.funnel_formula.clone();
        egui::CollapsingHeader::new("Funnel")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Inside");
                    ui.radio_value(&mut new.fill, FunnelFill::Spam, "Spam");
                    ui.radio_value(&mut new.fill, FunnelFill::Long, "Long notes");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Wall");
                    // 反向漏斗的墙在前头：两种说法跟着换（原版 sync_funnel）
                    let (end_text, start_text) = if reversed {
                        ("Notes start on it", "Notes end on it")
                    } else {
                        ("Notes end on it", "Notes start on it")
                    };
                    ui.radio_value(&mut new.wall, WallMode::In, end_text);
                    ui.radio_value(&mut new.wall, WallMode::Past, start_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Gate");
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
                    ui.label("ticks");
                });
                if ui
                    .checkbox(&mut new.vary, "Different start and wall gate")
                    .changed()
                    && new.vary
                {
                    text[1] = text[0].clone();
                }
                let gates_on = new.fill == FunnelFill::Spam && new.vary;
                ui.add_enabled_ui(gates_on, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Change");
                        ui.radio_value(&mut new.change, GateChange::Steps, "Steps");
                        ui.radio_value(&mut new.change, GateChange::Smooth, "Smooth");
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Follow");
                        ui.radio_value(&mut new.follow, GateFollow::Time, "Evenly");
                        ui.radio_value(&mut new.follow, GateFollow::Curve, "With the curve");
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Curve");
                    egui::ComboBox::from_id_salt("funnel_curve")
                        .selected_text("Preset…")
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for (i, (name, _)) in CURVE_PRESETS.iter().enumerate() {
                                if ui.selectable_label(false, *name).clicked() {
                                    preset_pick = Some(i);
                                }
                            }
                        });
                    if ui.button("Inside out").clicked() {
                        inside_out = true;
                    }
                    if ui.button("Turn").clicked() {
                        turn = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Formula");
                    ui.add(
                        egui::TextEdit::singleline(&mut formula_text)
                            .desired_width(130.0)
                            .hint_text("x^2"),
                    );
                    if ui.button("Apply").clicked() {
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
        let shape = match text {
            None => match funnel::preset_curve(None) {
                Ok(c) => c,
                Err(e) => {
                    self.status = format!("曲线算不出来：{e}");
                    return;
                }
            },
            Some(t) => match spiderweb_io::mathexpr::formula(t) {
                Ok(f) => {
                    match funnel::preset_curve(Some(&|x| f.eval(x).map_err(|e| format!("{e:?}")))) {
                        Ok(c) => c,
                        Err(e) => {
                            self.status = format!("曲线算不出来：{e}");
                            return;
                        }
                    }
                }
                Err(e) => {
                    self.status = format!("公式不对：{e:?}");
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
                    Err(e) => self.status = format!("公式算不出来：{e}"),
                }
            }
            Err(e) => self.status = format!("公式不对：{e:?}"),
        }
    }

    /// 把一条曲线形状给高亮的曲线；没高亮时提示（原版 set_curves）。
    fn apply_curve_shape(&mut self, shape: FunnelCurve) {
        if self
            .funnel_parts()
            .map(|(_, _, c)| c.is_empty())
            .unwrap_or(true)
        {
            self.status = "先高亮要改的曲线（Select 工具再点一次漏斗）".to_string();
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
