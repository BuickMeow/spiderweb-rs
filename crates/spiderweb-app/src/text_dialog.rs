//! Font picker window (upstream window/font_dialog.py): type a font name (the list filters
//! as you type) or pick from the list, with a sample preview and OK / Cancel below.
//!
//! Differences from upstream: the preview is drawn with egui's built-in font, not the
//! selected font (egui has no system fonts yet); Up / Down list navigation is left to the
//! search box's own text cursor and not implemented.

use eframe::egui;

use spiderweb_core::shape::TextSettings;
use spiderweb_core::text::TextChange;

use crate::app::App;

/// Sample text used when there is no text to preview (upstream SAMPLE).
const SAMPLE: &str = "AaBbCc 0123";

/// Font dialog state (the tk variables of upstream FontDialog).
pub struct FontDialog {
    /// Text in the search box (upstream self.name)
    pub search: String,
    /// Installed fonts (fetched once on open; upstream font_families)
    pub families: Vec<String>,
    /// The highlighted entry in the list
    pub selected: Option<String>,
    /// Sample text
    pub sample: String,
    /// Current contents of the list (upstream listbox)
    pub shown: Vec<String>,
    /// The input the list was filtered for; None = just opened, show the whole list (upstream filter(select=current))
    listed_for: Option<String>,
}

impl FontDialog {
    /// Opens the dialog (current = current font, sample = the text being edited, families = installed fonts).
    pub fn new(current: String, sample: String, families: Vec<String>) -> Self {
        let one = sample.split_whitespace().collect::<Vec<_>>().join(" ");
        let sample = if one.is_empty() {
            SAMPLE.to_string()
        } else {
            one.chars().take(40).collect()
        };
        Self {
            search: current.clone(),
            families,
            selected: Some(current),
            sample,
            shown: Vec::new(),
            listed_for: None,
        }
    }

    /// Recomputes the list when the input changes; shows the whole list on first open (upstream filter).
    fn refresh(&mut self) {
        match &self.listed_for {
            None => {
                self.shown = self.families.clone();
                self.listed_for = Some(self.search.clone());
            }
            Some(prev) if *prev != self.search => {
                self.shown = filtered(&self.families, &self.search);
                self.listed_for = Some(self.search.clone());
            }
            Some(_) => {}
        }
    }
}

/// List: names starting with the input first, then names containing it (upstream filter).
pub fn filtered(families: &[String], typed: &str) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return families.to_vec();
    }
    let mut out: Vec<String> = Vec::new();
    for f in families {
        if f.to_lowercase().starts_with(&typed) {
            out.push(f.clone());
        }
    }
    for f in families {
        let l = f.to_lowercase();
        if !l.starts_with(&typed) && l.contains(&typed) {
            out.push(f.clone());
        }
    }
    out
}

/// Font the OK button will use: if the typed name exactly matches a font use it, otherwise the highlighted entry in the list (upstream chosen).
fn chosen(dlg: &FontDialog, shown: &[String]) -> Option<String> {
    let typed = dlg.search.trim().to_lowercase();
    if let Some(f) = dlg.families.iter().find(|f| f.to_lowercase() == typed) {
        return Some(f.clone());
    }
    dlg.selected
        .clone()
        .filter(|s| shown.iter().any(|f| f == s))
}

/// Font dialog: search + list + preview + OK / Cancel; on pick, applies the new font to the text.
pub fn font_dialog_ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut dlg) = app.font_dialog.take() else {
        return;
    };
    let mut pick: Option<String> = None;
    let mut open = true;
    let mut close = false;
    egui::Window::new(rust_i18n::t!("font.title"))
        .collapsible(false)
        .resizable(true)
        .default_size([420.0, 460.0])
        .min_size([300.0, 300.0])
        .open(&mut open)
        .show(ctx, |ui| {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            ui.label(rust_i18n::t!("font.type"));
            let first = dlg.listed_for.is_none();
            let resp =
                ui.add(egui::TextEdit::singleline(&mut dlg.search).desired_width(f32::INFINITY));
            if first {
                resp.request_focus();
            }
            dlg.refresh();
            // The highlighted entry is gone after filtering: pick the first in the list (upstream filter's selection_set)
            if !dlg
                .selected
                .as_ref()
                .map(|s| dlg.shown.contains(s))
                .unwrap_or(false)
                && let Some(f) = dlg.shown.first()
            {
                dlg.selected = Some(f.clone());
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let now = chosen(&dlg, &dlg.shown);
                pick = now;
            }
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .id_salt("font_list")
                .show(ui, |ui| {
                    for k in 0..dlg.shown.len() {
                        let f = dlg.shown[k].clone();
                        let r =
                            ui.selectable_label(dlg.selected.as_deref() == Some(f.as_str()), &f);
                        if r.clicked() {
                            dlg.selected = Some(f.clone());
                        }
                        if r.double_clicked() {
                            pick = Some(f.clone());
                        }
                    }
                });
            let now = chosen(&dlg, &dlg.shown);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&dlg.sample).size(22.0));
            ui.label(
                egui::RichText::new(match &now {
                    Some(f) => f.clone(),
                    None => rust_i18n::t!("font.none").to_string(),
                })
                .weak()
                .size(11.0),
            );
            ui.horizontal(|ui| {
                if ui.button(rust_i18n::t!("font.ok")).clicked() {
                    pick = now.clone();
                }
                if ui.button(rust_i18n::t!("font.cancel")).clicked() {
                    close = true;
                }
            });
        });
    if let Some(f) = pick {
        crate::roll_text::set_text_setting(
            app,
            &TextChange {
                font: Some(f),
                ..Default::default()
            },
        );
    } else if open && !close {
        app.font_dialog = Some(dlg);
    }
}

/// Opens a font dialog (used by the panel button; reuses the font list cached in App).
pub fn open_font_dialog(app: &mut App, tx: &TextSettings) {
    if app.font_families.is_empty() {
        app.font_families = spiderweb_core::fonts::font_families();
    }
    let families = app.font_families.clone();
    app.font_dialog = Some(FontDialog::new(tx.font.clone(), tx.text.clone(), families));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn families() -> Vec<String> {
        [
            "Arial",
            "Arial Black",
            "DejaVu Sans",
            "Noto Sans CJK",
            "Times New Roman",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn filter_puts_prefix_matches_first() {
        let f = families();
        assert_eq!(filtered(&f, "aria").len(), 2);
        assert_eq!(filtered(&f, "aria")[0], "Arial");
        let got = filtered(&f, "sans");
        assert_eq!(
            got,
            vec!["DejaVu Sans".to_string(), "Noto Sans CJK".to_string()]
        );
        assert_eq!(filtered(&f, "").len(), f.len());
        // Case-insensitive
        assert_eq!(filtered(&f, "TIMES"), vec!["Times New Roman".to_string()]);
    }
}
