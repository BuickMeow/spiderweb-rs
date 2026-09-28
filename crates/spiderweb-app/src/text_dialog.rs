//! 字体选择窗口（原版 window/font_dialog.py）：输入字体名（列表跟着过滤）或从列表里挑，
//! 下面是样文预览与 OK / Cancel。
//!
//! 与原版的差异：预览用 egui 自带字体画，不按所选字体渲染（egui 还没接系统字体）；列表的
//! 上下键移动也交给搜索框本身的文本光标，不做。

use eframe::egui;

use spiderweb_core::shape::TextSettings;
use spiderweb_core::text::TextChange;

use crate::app::App;

/// 没有可预览的文本时用的样文（原版 SAMPLE）。
const SAMPLE: &str = "AaBbCc 0123";

/// 字体窗口的状态（原版 FontDialog 的 tk 变量）。
pub struct FontDialog {
    /// 搜索框里的字（原版 self.name）
    pub search: String,
    /// 已安装字体（打开时取一次，原版 font_families）
    pub families: Vec<String>,
    /// 列表里高亮的那一个
    pub selected: Option<String>,
    /// 样文
    pub sample: String,
    /// 列表当前的内容（原版 listbox）
    pub shown: Vec<String>,
    /// 列表是按哪个输入过滤出来的；None = 刚打开，整张显示（原版 filter(select=current)）
    listed_for: Option<String>,
}

impl FontDialog {
    /// 打开窗口（current = 当前字体，sample = 正在编辑的文本，families = 已安装字体）。
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

    /// 输入变了就重算列表；刚打开时整张显示（原版 filter）。
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

/// 列表：名字以输入开头的在前、其次包含的（原版 filter）。
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

/// 确定按钮会用的字体：输入的名字正好是某个字体就用它，否则用列表里高亮的那个（原版 chosen）。
fn chosen(dlg: &FontDialog, shown: &[String]) -> Option<String> {
    let typed = dlg.search.trim().to_lowercase();
    if let Some(f) = dlg.families.iter().find(|f| f.to_lowercase() == typed) {
        return Some(f.clone());
    }
    dlg.selected
        .clone()
        .filter(|s| shown.iter().any(|f| f == s))
}

/// 字体窗口：搜索 + 列表 + 预览 + OK / Cancel；选中后把新字体套到文本上。
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
            // 过滤后高亮的那个不在了：选列表第一个（原版 filter 的 selection_set）
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

/// 开一个字体窗口（面板的按钮用；字体表复用 app 里缓存的）。
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
        // 大小写不敏感
        assert_eq!(filtered(&f, "TIMES"), vec!["Times New Roman".to_string()]);
    }
}
