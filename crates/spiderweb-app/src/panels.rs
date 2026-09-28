//! 工具栏与右侧面板（原版 window/app.py 的 _build / _build_side 及各面板）。

use eframe::egui;

use spiderweb_core::shape::Kind;
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
        }
        self.project_section(ui);
        self.shapes_section(ui);
        self.defaults_section(ui);
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
