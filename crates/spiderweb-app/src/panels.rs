//! 工具栏与右侧面板（原版 window/app.py 的 _build / _build_side 及各面板）。

use eframe::egui;

use spiderweb_core::shape::{Kind, TextAlign, TextSettings, TextUnit};
use spiderweb_core::text::{self, TextChange};
use spiderweb_io::project::{ChannelMode, ChannelSplit};

use crate::app::{App, Tool};

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
        }
        self.project_section(ui);
        self.shapes_section(ui);
        self.defaults_section(ui);
        self.text_section(ui);
        self.points_section(ui);
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
}

fn fmt_num(v: f64) -> String {
    spiderweb_io::mathexpr::fmt(v)
}
