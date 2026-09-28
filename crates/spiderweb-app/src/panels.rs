//! 工具栏与右侧面板（原版 window/app.py 的 _build / _build_side 及各面板）。

use eframe::egui;

use spiderweb_core::shape::{Align, Fill, Kind};
use spiderweb_io::project::{ChannelMode, ChannelSplit};

use crate::app::{App, Tool};

const SNAPS: [&str; 7] = ["Off", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64"];

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
        }
        self.project_section(ui);
        self.shapes_section(ui);
        self.defaults_section(ui);
        self.custom_section(ui);
        self.points_section(ui);
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
