//! The Snap dropdown in the toolbar and its "Customised snap" window (port of Python
//! `window/snap_picker.py`): the list with little pictures of each note (dotted ones with a
//! dot, triplets with a 3), drawn with egui shapes — no image files. The choices themselves
//! are in [`spiderweb_io::snap`].

use eframe::egui;

use spiderweb_io::snap::{
    COUNT_RANGE, DIV_RANGE, NOTE_RANGE, SNAP_LIST, custom_parts, custom_snap, py_int,
    whole_note_beats,
};

use crate::app::App;

/// The pictures are drawn in a 16 x 16 box (upstream `SIZE`).
const ICON: f32 = 16.0;
/// The triplet digit: five rows of three pixels (upstream `DIGIT_3`).
const DIGIT_3: [&str; 5] = ["111", "001", "011", "001", "111"];

/// What the toolbar draws for a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapPicture {
    Empty,
    Bar,
    Custom,
    Note {
        note: i64,
        dots: bool,
        triplet: bool,
    },
}

/// The picture of a snap text (custom ones get the wrench, like upstream `SnapPicker.show`).
pub fn snap_picture(snap: &str) -> SnapPicture {
    if snap == "bar" {
        return SnapPicture::Bar;
    }
    if custom_parts(snap).is_some() {
        return SnapPicture::Custom;
    }
    SNAP_LIST
        .iter()
        .find(|(s, _)| *s == snap)
        .and_then(|(_, what)| *what)
        .map_or(SnapPicture::Empty, |n| SnapPicture::Note {
            note: n.note,
            dots: n.dots > 0,
            triplet: n.triplet,
        })
}

/// How the snap is shown (upstream `snap_text`).
pub fn snap_text(snap: &str) -> String {
    match snap {
        "off" => rust_i18n::t!("snap.off").to_string(),
        "bar" => rust_i18n::t!("snap.bar").to_string(),
        "1/1" => rust_i18n::t!("snap.whole").to_string(),
        _ => match custom_parts(snap) {
            None => snap.to_string(),
            Some(p) => {
                let text = if p.count == "." {
                    rust_i18n::t!("snap.dotted", note = p.note.to_string()).to_string()
                } else if p.count == ".." {
                    rust_i18n::t!("snap.double_dotted", note = p.note.to_string()).to_string()
                } else {
                    let count = if p.count.is_empty() { "1" } else { &p.count };
                    format!("{count}/{}", p.note)
                };
                if p.div == 1 {
                    text
                } else {
                    rust_i18n::t!("snap.divided", text = text, div = p.div.to_string()).to_string()
                }
            }
        },
    }
}

/// The snap step in beats for the app's snap text (upstream `App.snap_beats`).
pub fn snap_step(snap: &str, beats: i64) -> Option<f64> {
    spiderweb_io::snap::snap_beats(snap, beats as f64)
}

// ---------------------------------------------------------------- the pictures

/// Draws in the 16 x 16 box's coordinates (y down, like upstream `_note_inside`).
struct Icon<'a> {
    painter: &'a egui::Painter,
    origin: egui::Pos2,
    scale: f32,
    color: egui::Color32,
}

impl Icon<'_> {
    fn at(&self, x: f32, y: f32) -> egui::Pos2 {
        self.origin + egui::vec2(x * self.scale, y * self.scale)
    }

    fn seg(&self, a: (f32, f32), b: (f32, f32), width: f32) {
        self.painter.line_segment(
            [self.at(a.0, a.1), self.at(b.0, b.1)],
            egui::Stroke::new(width * self.scale, self.color),
        );
    }

    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_filled(self.at(x, y), r * self.scale, self.color);
    }

    /// The points of an ellipse turned by `turn` radians (upstream `_ellipse` with a turn).
    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32, turn: f32) -> Vec<egui::Pos2> {
        let (sin_t, cos_t) = turn.sin_cos();
        (0..32)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 32.0;
                let (sin_a, cos_a) = a.sin_cos();
                self.at(
                    cx + rx * cos_a * cos_t - ry * sin_a * sin_t,
                    cy + rx * cos_a * sin_t + ry * sin_a * cos_t,
                )
            })
            .collect()
    }

    fn outline(&self, cx: f32, cy: f32, rx: f32, ry: f32, turn: f32, width: f32) {
        self.painter.add(egui::Shape::closed_line(
            self.ellipse(cx, cy, rx, ry, turn),
            egui::Stroke::new(width * self.scale, self.color),
        ));
    }

    fn filled(&self, cx: f32, cy: f32, rx: f32, ry: f32, turn: f32) {
        self.painter.add(egui::Shape::convex_polygon(
            self.ellipse(cx, cy, rx, ry, turn),
            self.color,
            egui::Stroke::NONE,
        ));
    }

    /// One 1 x 1 pixel of the box (the triplet's little 3).
    fn pixel(&self, x: f32, y: f32) {
        self.painter.rect_filled(
            egui::Rect::from_min_size(self.at(x, y), egui::Vec2::splat(self.scale)),
            0.0,
            self.color,
        );
    }
}

fn paint_note(icon: &Icon, note: i64, dots: bool, triplet: bool) {
    let tilt = (-25.0_f32).to_radians();
    if note == 1 {
        // a whole note: an open oval, no stem (the ring between outer and inner ellipse)
        icon.outline(7.5, 9.0, 3.1, 2.25, 0.0, 1.4);
        return;
    }
    if note == 2 {
        // a half note: open head
        icon.outline(6.2, 12.6, 2.85, 1.65, tilt, 1.3);
    } else {
        icon.filled(6.2, 12.6, 3.3, 2.3, tilt);
    }
    icon.seg((9.2, 12.2), (9.2, 1.6), 1.2);
    let flags = match note {
        8 => 1,
        16 => 2,
        32 => 3,
        _ => 0,
    };
    let gap = if flags == 3 { 2.0 } else { 2.4 };
    for k in 0..flags {
        let y0 = 1.6 + k as f32 * gap;
        icon.seg((9.2, y0), (12.6, y0 + 3.2), 1.3);
    }
    if dots {
        icon.dot(13.3, 12.8, 1.2);
    }
    if triplet {
        for (row, bits) in DIGIT_3.iter().enumerate() {
            for (col, bit) in bits.chars().enumerate() {
                if bit == '1' {
                    icon.pixel(12.2 + col as f32, 10.5 + row as f32);
                }
            }
        }
    }
}

fn paint_bar(icon: &Icon) {
    icon.seg((2.5, 3.0), (2.5, 13.0), 1.3);
    icon.seg((13.5, 3.0), (13.5, 13.0), 1.3);
    for h in [4.5, 8.0, 11.5] {
        icon.seg((2.5, h), (13.5, h), 0.8);
    }
}

fn paint_wrench(icon: &Icon) {
    // a spanner (settings): the jaw's ring and its handle
    icon.outline(11.3, 4.7, 2.65, 2.65, 0.0, 1.3);
    icon.seg((3.2, 12.8), (10.0, 6.0), 2.4);
}

/// Draw one picture inside `rect` (upstream `picture`).
pub fn paint_snap_picture(
    painter: &egui::Painter,
    rect: egui::Rect,
    what: SnapPicture,
    color: egui::Color32,
) {
    let icon = Icon {
        painter,
        origin: rect.min,
        scale: rect.width() / ICON,
        color,
    };
    match what {
        SnapPicture::Empty => {}
        SnapPicture::Bar => paint_bar(&icon),
        SnapPicture::Custom => paint_wrench(&icon),
        SnapPicture::Note {
            note,
            dots,
            triplet,
        } => paint_note(&icon, note, dots, triplet),
    }
}

/// A picture widget (16 x 16 at scale 1).
fn picture_ui(ui: &mut egui::Ui, what: SnapPicture, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_snap_picture(ui.painter(), rect, what, ui.visuals().text_color());
    }
}

// ---------------------------------------------------------------- the dropdown

/// The toolbar's Snap button + list (upstream `SnapPicker`).
pub fn snap_picker_ui(app: &mut App, ui: &mut egui::Ui) {
    let text = snap_text(&app.snap);
    let response = picker_button(ui, snap_picture(&app.snap), &text)
        .on_hover_text(rust_i18n::t!("snap.tip").to_string());
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            let mut choice: Option<&str> = None;
            for (snap, _) in SNAP_LIST {
                ui.horizontal(|ui| {
                    picture_ui(ui, snap_picture(snap), ICON);
                    if ui
                        .selectable_label(app.snap == snap, snap_text(snap))
                        .clicked()
                    {
                        choice = Some(snap);
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                picture_ui(ui, SnapPicture::Custom, ICON);
                if ui
                    .selectable_label(
                        custom_parts(&app.snap).is_some(),
                        rust_i18n::t!("snap.custom"),
                    )
                    .clicked()
                {
                    app.snap_window = Some(CustomSnapWindow::from_snap(&app.snap));
                    ui.close();
                }
            });
            if let Some(snap) = choice {
                app.snap = snap.to_string();
                ui.close();
            }
        });
}

/// The button: the picture, the snap's name and a little arrow.
fn picker_button(ui: &mut egui::Ui, what: SnapPicture, text: &str) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, ui.visuals().text_color());
    let pad = ui.spacing().button_padding;
    let arrow = 10.0;
    let width = pad.x + ICON + pad.x + galley.size().x + pad.x + arrow;
    let height = galley.size().y.max(ICON) + 2.0 * pad.y;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + pad.x, rect.center().y - ICON / 2.0),
            egui::Vec2::splat(ICON),
        );
        paint_snap_picture(ui.painter(), icon_rect, what, visuals.fg_stroke.color);
        ui.painter().galley(
            egui::pos2(
                icon_rect.max.x + pad.x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            visuals.fg_stroke.color,
        );
        let cx = rect.max.x - pad.x - arrow / 2.0;
        let cy = rect.center().y;
        let stroke = egui::Stroke::new(1.0, visuals.fg_stroke.color);
        ui.painter().line_segment(
            [egui::pos2(cx - 3.0, cy - 2.0), egui::pos2(cx, cy + 2.0)],
            stroke,
        );
        ui.painter().line_segment(
            [egui::pos2(cx, cy + 2.0), egui::pos2(cx + 3.0, cy - 2.0)],
            stroke,
        );
    }
    response
}

// ---------------------------------------------------------------- Customised snap

/// The "Customised snap" window's boxes (upstream `CustomSnapWindow`).
#[derive(Clone, Debug)]
pub struct CustomSnapWindow {
    pub count: String,
    /// `""` = one plain note, `"/"` = count notes, `"."` dotted, `".."` double dotted.
    pub kind: String,
    pub note: String,
    pub div: String,
}

impl CustomSnapWindow {
    /// Opens on the snap in use, or 3 / 16 / 1 (upstream `__init__`).
    pub fn from_snap(snap: &str) -> Self {
        let (count, note, div) = custom_parts(snap)
            .map(|p| (p.count, p.note, p.div))
            .unwrap_or(("3".to_string(), 16, 1));
        let dots = count.is_empty() || count == "." || count == "..";
        Self {
            count: if dots { "3".to_string() } else { count.clone() },
            kind: if dots { count } else { "/".to_string() },
            note: note.to_string(),
            div: div.to_string(),
        }
    }

    /// The custom snap the boxes make, or None if a number is missing or out of range.
    pub fn snap(&self) -> Option<String> {
        let note = py_int(&self.note)?;
        let div = py_int(&self.div)?;
        let count = if self.kind == "/" {
            py_int(&self.count)?.to_string()
        } else {
            self.kind.clone()
        };
        let snap = custom_snap(&count, note, div);
        custom_parts(&snap).map(|_| snap)
    }

    /// OK: a plain note that's in the list anyway goes back to its `"1/n"` text.
    pub fn ok_snap(&self) -> Option<String> {
        let snap = self.snap()?;
        let p = custom_parts(&snap)?;
        if p.count.is_empty() && p.div == 1 && matches!(p.note, 1 | 2 | 4 | 8 | 16 | 32) {
            return Some(format!("1/{}", p.note));
        }
        Some(snap)
    }
}

/// The "Customised snap" window (upstream `CustomSnapWindow`); `app.snap_window` is None when
/// it's closed.
pub fn custom_snap_window_ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = app.snap_window.take() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    egui::Window::new(rust_i18n::t!("snap.customised_snap"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let mut enter = false;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(rust_i18n::t!("snap.setup"));
                ui.horizontal(|ui| {
                    let count = ui.add_enabled(
                        w.kind == "/",
                        egui::TextEdit::singleline(&mut w.count).desired_width(50.0),
                    );
                    enter |= count.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    egui::ComboBox::from_id_salt("snap_kind")
                        .selected_text(w.kind.clone())
                        .width(50.0)
                        .show_ui(ui, |ui| {
                            for k in ["", "/", ".", ".."] {
                                ui.selectable_value(&mut w.kind, k.to_string(), k);
                            }
                        })
                        .response
                        .on_hover_text(rust_i18n::t!("snap.kind_tip").to_string());
                    let note = ui.add(egui::TextEdit::singleline(&mut w.note).desired_width(50.0));
                    enter |= note.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.label(rust_i18n::t!("snap.note"));
                });
                ui.horizontal(|ui| {
                    ui.label(rust_i18n::t!("snap.divided_by"));
                    let div = ui
                        .add(egui::TextEdit::singleline(&mut w.div).desired_width(50.0))
                        .on_hover_text(rust_i18n::t!("snap.divided_by_tip").to_string());
                    enter |= div.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
            });
            if enter {
                ok = true;
            }
            match w.snap() {
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(0xd0, 0x00, 0x00),
                        rust_i18n::t!(
                            "snap.out_of_range",
                            COUNT = format!("{}-{}", COUNT_RANGE.0, COUNT_RANGE.1),
                            NOTE = format!("{}-{}", NOTE_RANGE.0, NOTE_RANGE.1),
                            DIV = format!("{}-{}", DIV_RANGE.0, DIV_RANGE.1)
                        )
                        .to_string(),
                    );
                }
                Some(snap) => {
                    let beats = whole_note_beats(&snap).unwrap_or(0.0);
                    let mut beats_text = format!("{beats:.4}");
                    while beats_text.ends_with('0') {
                        beats_text.pop();
                    }
                    beats_text = beats_text.trim_end_matches('.').to_string();
                    let ticks = spiderweb_io::mathexpr::fmt(
                        (beats * app.ppq as f64 * 100.0).round_ties_even() / 100.0,
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(0x77, 0x77, 0x77),
                        rust_i18n::t!(
                            "snap.length",
                            beats = beats_text,
                            ticks = ticks,
                            ppq = app.ppq.to_string()
                        )
                        .to_string(),
                    );
                }
            }
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(rust_i18n::t!("snap.cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(rust_i18n::t!("snap.ok")).clicked() {
                        ok = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });
    if ok
        && !cancel
        && let Some(snap) = w.ok_snap()
    {
        app.snap = snap;
        return;
    }
    if !cancel && open {
        app.snap_window = Some(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's snap step follows every snap form (App.snap_beats delegates here).
    #[test]
    fn snap_step_of_every_form() {
        assert_eq!(snap_step("off", 4), None);
        assert_eq!(snap_step("bar", 4), Some(4.0));
        assert_eq!(snap_step("bar", 7), Some(7.0));
        assert_eq!(snap_step("1/8", 4), Some(0.5));
        assert_eq!(snap_step("3/8", 4), Some(1.5));
        assert_eq!(snap_step("1/3", 4), Some(4.0 / 3.0));
        assert_eq!(snap_step("c:/64/1", 4), Some(0.0625));
        assert_eq!(snap_step("c:./8/3", 4), Some(0.25));
        assert_eq!(snap_step("c:5/16/1", 4), Some(1.25));
        assert_eq!(snap_step("c:../4/2", 4), Some(0.875));
        assert_eq!(snap_step("c:3/16/1", 4), Some(0.75));
        assert_eq!(spiderweb_io::snap::snap_ticks("1/8", 4.0, 960), 480);
        assert_eq!(spiderweb_io::snap::snap_ticks("off", 4.0, 960), 1);
    }

    /// The labels follow every snap form and every picture paints (headless egui).
    #[test]
    fn labels_and_pictures() {
        rust_i18n::set_locale("en");
        assert_eq!(snap_text("off"), "Off");
        assert_eq!(snap_text("bar"), "Bar");
        assert_eq!(snap_text("1/1"), "Whole");
        assert_eq!(snap_text("3/16"), "3/16");
        assert_eq!(snap_text("c:/64/1"), "1/64");
        assert_eq!(snap_text("c:5/16/1"), "5/16");
        assert_eq!(snap_text("c:./8/3"), "dotted 1/8 ÷ 3");
        assert_eq!(snap_text("c:../4/2"), "double-dotted 1/4 ÷ 2");
        assert_eq!(snap_picture("bar"), SnapPicture::Bar);
        assert_eq!(snap_picture("c:./8/3"), SnapPicture::Custom);
        assert_eq!(
            snap_picture("1/16"),
            SnapPicture::Note {
                note: 16,
                dots: false,
                triplet: false
            }
        );
        assert_eq!(
            snap_picture("1/6"),
            SnapPicture::Note {
                note: 4,
                dots: false,
                triplet: true
            }
        );
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for (snap, _) in SNAP_LIST {
                picture_ui(ui, snap_picture(snap), ICON);
            }
            picture_ui(ui, SnapPicture::Custom, ICON);
        });
        out.textures_delta.clear(); // nothing renders here; the deltas would assert on drop
    }

    /// The custom window opens on the snap in use and its boxes make that snap again.
    #[test]
    fn custom_snap_window_round_trip() {
        let w = CustomSnapWindow::from_snap("c:./8/3");
        assert_eq!(
            (
                w.count.as_str(),
                w.kind.as_str(),
                w.note.as_str(),
                w.div.as_str()
            ),
            ("3", ".", "8", "3")
        );
        assert_eq!(w.snap(), Some("c:./8/3".to_string()));
        let w = CustomSnapWindow::from_snap("c:5/16/2");
        assert_eq!(w.snap(), Some("c:5/16/2".to_string()));
        let w = CustomSnapWindow::from_snap("c:/16/1");
        assert_eq!(w.ok_snap(), Some("1/16".to_string()));
        let w = CustomSnapWindow::from_snap("c:../4/2");
        assert_eq!(w.snap(), Some("c:../4/2".to_string()));
        // a list snap opens at 3 / 16 / 1 (kind "/")
        let w = CustomSnapWindow::from_snap("1/16");
        assert_eq!((w.count.as_str(), w.kind.as_str()), ("3", "/"));
        // missing or out of range numbers make no snap
        let mut w = CustomSnapWindow::from_snap("1/16");
        w.note = "129".to_string();
        assert_eq!(w.snap(), None);
        w.note = "abc".to_string();
        assert_eq!(w.snap(), None);
        w.note = "16".to_string();
        w.div = "0".to_string();
        assert_eq!(w.snap(), None);
        let mut w = CustomSnapWindow::from_snap("1/16");
        w.kind = ".".to_string();
        w.note = "8".to_string();
        w.div = "3".to_string();
        assert_eq!(w.snap(), Some("c:./8/3".to_string()));
    }
}
