//! 帮助与提示（原版 window/help.py + files/about.py 的 VERSION / WEBSITE）：
//! Tips（首次使用的弹窗，看过的主题持久化到程序目录 tips.json）、帮助窗口（F1，可搜索、
//! 按工具定位、可滚动）、侧栏底部的当前工具帮助。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::app::{App, Tool, VERSION};
use crate::help_texts::{self, Topic};

/// 新版 / 源码 / 问题反馈地址（原版 files/about.py 的 WEBSITE）。
pub const WEBSITE: &str = "https://github.com/UnPrioritized/Spiderweb";
/// 看过的 tip 存这里（程序目录，原版存在 autosave 的窗口设置里）。
pub const TIPS_FILE: &str = "tips.json";
/// 启动后延迟弹欢迎 tip（原版 `after(800, ...)`）。
pub const WELCOME_DELAY: Duration = Duration::from_millis(800);

// ---------------------------------------------------------------- 持久化的 tips

/// tips.json 的内容。
#[derive(Serialize, Deserialize)]
struct TipsFile {
    #[serde(default)]
    seen: Vec<String>,
    #[serde(default = "tips_on_default")]
    on: bool,
}

fn tips_on_default() -> bool {
    true
}

impl Default for TipsFile {
    fn default() -> Self {
        Self {
            seen: Vec::new(),
            on: true,
        }
    }
}

/// 哪些 tip 看过、是否显示，以及当前弹窗（原版 help.Tips）。
pub struct Tips {
    /// 看过的主题 id。
    pub seen: BTreeSet<String>,
    /// 首次使用的 tip 开关。
    pub on: bool,
    /// 正在弹的主题 id。
    pub popup: Option<String>,
    /// 等当前弹窗关掉再弹的主题 id（原版 waiting）。
    pub waiting: Option<String>,
    /// 启动后到点弹欢迎 tip（None = 已经试过了）。
    pub welcome_at: Option<Instant>,
    path: PathBuf,
}

impl Tips {
    /// 从程序目录读 tips.json（没有 / 坏了就用默认：全都没看过、显示）。
    pub fn new(base: &Path) -> Self {
        let path = base.join(TIPS_FILE);
        let file = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<TipsFile>(&t).ok())
            .unwrap_or_default();
        Self {
            seen: file.seen.into_iter().collect(),
            on: file.on,
            popup: None,
            waiting: None,
            welcome_at: None,
            path,
        }
    }

    /// tips.json 的完整路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 写回 tips.json（写不了就算了，下次再说）。
    pub fn save(&self) {
        let file = TipsFile {
            seen: self.seen.iter().cloned().collect(),
            on: self.on,
        };
        if let Ok(text) = serde_json::to_string_pretty(&file) {
            let _ = std::fs::write(self.path(), text);
        }
    }

    /// 弹这个主题的 tip（看过了 / 关掉了就不弹）。正弹着别的主题时直接换掉。
    pub fn show(&mut self, topic: &str) {
        self.show_ex(topic, false);
    }

    /// 同 [`Tips::show`]，但正弹着别的主题时排队等它关掉（原版 wait=True）。
    pub fn show_waiting(&mut self, topic: &str) {
        self.show_ex(topic, true);
    }

    fn show_ex(&mut self, topic: &str, wait: bool) {
        if help_texts::by_id(topic).is_none() || !self.on || self.seen.contains(topic) {
            return;
        }
        if wait && self.popup.as_deref().is_some_and(|p| p != topic) {
            self.waiting = Some(topic.to_string());
            return;
        }
        if self.waiting.as_deref() == Some(topic) {
            self.waiting = None;
        }
        self.seen.insert(topic.to_string());
        self.popup = Some(topic.to_string());
        self.save();
    }

    /// “Got it”：还没看过的下一个主题（NEXT）接着弹，否则关掉。
    pub fn got_it(&mut self) {
        let next = self
            .popup
            .as_deref()
            .and_then(|p| help_texts::next(p).map(str::to_string));
        match next {
            Some(n) if self.on && !self.seen.contains(&n) => self.show(&n),
            _ => self.close(),
        }
    }

    /// 关掉当前 tip（窗口 X / Esc），排队的接着来。
    pub fn close(&mut self) {
        self.popup = None;
        if let Some(w) = self.waiting.take() {
            self.show(&w);
        }
    }

    /// “Show all tips again”：全都没看过、重新打开（原版 reset_tips）。
    pub fn reset(&mut self) {
        self.seen.clear();
        self.on = true;
        self.save();
    }
}

// ---------------------------------------------------------------- 搜索

/// 一个主题的全部可搜索文字（原版 topic_words）。
pub fn topic_words(t: &Topic) -> String {
    format!("{} {} {} {}", t.title, t.tip, t.text, t.words).to_lowercase()
}

/// 搜索：每个词都要出现在主题里（原版 HelpWindow.matches）。
pub fn topic_matches(t: &Topic, query: &str) -> bool {
    query
        .split_whitespace()
        .all(|w| topic_words(t).contains(&w.to_lowercase()))
}

/// 过滤后的主题（保持 help_texts::TOPICS 顺序，空搜索 = 全部）。
pub fn filter_topics(query: &str) -> Vec<&'static Topic> {
    help_texts::TOPICS
        .iter()
        .filter(|t| topic_matches(t, query))
        .collect()
}

// ---------------------------------------------------------------- 工具 / 侧栏

/// 工具 -> tip 主题 id（原版 TOOL_TOPICS；Square / Circle / Triangle 共用 box）。
pub fn tool_topic(tool: Tool) -> &'static str {
    let key = match tool {
        Tool::Select => "select",
        Tool::Line => "line",
        Tool::Poly => "poly",
        Tool::Free => "free",
        Tool::Curve => "curve",
        Tool::Arc => "arc",
        Tool::Custom => "custom",
        Tool::Funnel => "funnel",
        Tool::Text => "text",
        Tool::Square => "square",
        Tool::Circle => "circle",
        Tool::Triangle => "triangle",
    };
    help_texts::tool_topic(key)
}

/// 侧栏底部的当前工具帮助（原版 update_side_help）。
pub fn side_help_text(app: &App) -> String {
    match help_texts::by_id(tool_topic(app.tool)) {
        Some(t) => format!(
            "{}\n{}\n\nHelp (F1): every tip, searchable.",
            t.title, t.text
        ),
        None => String::new(),
    }
}

/// 侧栏底部一块：当前工具的帮助 + 打开帮助窗口的按钮。
pub fn side_help_ui(app: &mut App, ui: &mut egui::Ui) {
    if help_texts::by_id(tool_topic(app.tool)).is_none() {
        return;
    }
    let mut open = false;
    egui::CollapsingHeader::new("Help")
        .default_open(true)
        .id_salt("side_help")
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(side_help_text(app))
                    .small()
                    .color(egui::Color32::from_gray(120)),
            );
            open = ui.button("Help (F1)").clicked();
        });
    if open {
        open_help(app, None);
    }
}

/// 帮助窗口的状态。
#[derive(Default)]
pub struct HelpState {
    pub open: bool,
    /// 正在看的主题 id。
    pub topic: Option<String>,
    /// 搜索框。
    pub query: String,
}

/// 打开帮助窗口（原版 open_help）：topic 为 None 时定位到当前工具的主题。
pub fn open_help(app: &mut App, topic: Option<&str>) {
    let id = topic.unwrap_or_else(|| tool_topic(app.tool));
    let Some(_) = help_texts::by_id(id) else {
        return;
    };
    app.help.open = true;
    app.help.topic = Some(id.to_string());
    // 搜索词会把目标主题滤掉的话清空搜索（原版 open_topic）。
    if !app.help.query.is_empty() && !filter_topics(&app.help.query).iter().any(|t| t.id == id) {
        app.help.query.clear();
    }
}

// ---------------------------------------------------------------- 界面

/// 帮助窗口的界面（每帧调用；没打开就什么都不做）。
pub fn help_ui(app: &mut App, ctx: &egui::Context) {
    if !app.help.open {
        return;
    }
    let found = filter_topics(&app.help.query);
    // 当前主题被搜索滤掉 / 还没选过：选第一个（原版 fill_list）。
    let keep = app
        .help
        .topic
        .as_deref()
        .is_some_and(|cur| found.iter().any(|t| t.id == cur));
    if !keep {
        app.help.topic = found.first().map(|t| t.id.to_string());
    }

    let mut open = true;
    let mut query = std::mem::take(&mut app.help.query);
    let mut selected = app.help.topic.clone();
    let mut on = app.tips.on;
    let mut reset = false;
    let mut link: Option<&'static str> = None;
    let base = app
        .autosave_path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    // 主体至少这么高：egui 的窗口会缩到内容高度，不设的话会比原版的 900x620 矮很多
    let body_h = (ctx.screen_rect().height() * 0.72).clamp(520.0, 900.0);
    egui::Window::new(format!("Spiderweb {VERSION} — Help"))
        .open(&mut open)
        .default_size([960.0, 760.0])
        .min_width(600.0)
        .min_height(520.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(body_h);
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("Search")
                            .desired_width(ui.available_width()),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Search: type words (all of them have to be in the topic)",
                        )
                        .small()
                        .weak(),
                    );
                    egui::ScrollArea::vertical()
                        .id_salt("help_list")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for section in help_texts::SECTIONS {
                                let topics: Vec<&Topic> = found
                                    .iter()
                                    .copied()
                                    .filter(|t| t.section == *section)
                                    .collect();
                                if topics.is_empty() {
                                    continue;
                                }
                                egui::CollapsingHeader::new(*section)
                                    .default_open(true)
                                    .id_salt(format!("help_{section}"))
                                    .show(ui, |ui| {
                                        for t in topics {
                                            if ui
                                                .selectable_label(
                                                    selected.as_deref() == Some(t.id),
                                                    t.title,
                                                )
                                                .clicked()
                                            {
                                                selected = Some(t.id.to_string());
                                            }
                                        }
                                    });
                            }
                        });
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("help_text")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let cur = selected.clone();
                            show_topic(ui, cur.as_deref(), &base, &mut selected, &mut link);
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut on, "Show a tip the first time I use something");
                if ui.button("Show all tips again").clicked() {
                    reset = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.link(format!("Spiderweb {VERSION}")).clicked() {
                        selected = Some("about".to_string());
                    }
                });
            });
        });

    app.help.query = query;
    app.help.topic = selected;
    if on != app.tips.on {
        app.tips.on = on;
        app.tips.save();
    }
    if reset {
        app.tips.reset();
    }
    if let Some(url) = link {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    if !open {
        app.help.open = false;
    }
}

/// 帮助窗口右侧：一个小节的标题、正文、See also（原版 HelpWindow.show）。
fn show_topic(
    ui: &mut egui::Ui,
    id: Option<&str>,
    base: &str,
    selected: &mut Option<String>,
    link: &mut Option<&'static str>,
) {
    let Some(t) = id.and_then(help_texts::by_id) else {
        ui.label(
            egui::RichText::new("Nothing found. Try fewer or other words.")
                .weak()
                .size(14.0),
        );
        return;
    };
    ui.label(egui::RichText::new(t.section).small().weak());
    ui.label(egui::RichText::new(t.title).strong().size(18.0));
    ui.add_space(6.0);
    ui.label(t.text);
    if t.id == "about" {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Website").clicked() {
                *link = Some(WEBSITE);
            }
            ui.weak(format!("程序目录：{base}"));
        });
    }
    let see = help_texts::see(t.id);
    if !see.is_empty() {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.weak("See also:");
            for (n, sid) in see.iter().enumerate() {
                if n > 0 {
                    ui.weak("·");
                }
                let sid: &'static str = sid;
                if let Some(st) = help_texts::by_id(sid)
                    && ui.link(st.title).clicked()
                {
                    *selected = Some(sid.to_string());
                }
            }
        });
    }
}

/// tip 弹窗与欢迎 tip 的延迟（每帧调用）。
pub fn tips_ui(app: &mut App, ctx: &egui::Context) {
    // 启动后过一小会儿弹欢迎 tip（原版 after(800)）；到点前安排重绘。
    if let Some(at) = app.tips.welcome_at {
        let elapsed = at.elapsed();
        if elapsed >= WELCOME_DELAY {
            app.tips.welcome_at = None;
            app.tips.show("welcome");
        } else {
            ctx.request_repaint_after(WELCOME_DELAY - elapsed);
        }
    }

    let Some(topic_id) = app.tips.popup.clone() else {
        return;
    };
    let Some(topic) = help_texts::by_id(&topic_id) else {
        app.tips.popup = None;
        return;
    };
    let mut open = true;
    let mut on = app.tips.on;
    let mut on_changed = false;
    let mut got_it = false;
    let mut more = false;
    egui::Window::new("Tip")
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0))
        .collapsible(false)
        .resizable(false)
        .default_width(400.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(topic.title).strong());
            ui.add_space(4.0);
            ui.label(topic.tip);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, "Show tips").changed() {
                    on_changed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Got it").clicked() {
                        got_it = true;
                    }
                    if ui.button("More…").clicked() {
                        more = true;
                    }
                });
            });
        });

    if on_changed {
        app.tips.on = on;
        app.tips.save();
    }
    if more {
        app.tips.popup = None;
        open_help(app, Some(&topic_id));
    } else if got_it {
        app.tips.got_it();
    } else if !open {
        app.tips.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_needs_all_words() {
        assert_eq!(filter_topics("").len(), help_texts::TOPICS.len());
        assert!(filter_topics("zzzzz").is_empty());
        let ids: Vec<&str> = filter_topics("funnel curve").iter().map(|t| t.id).collect();
        assert!(ids.contains(&"funnel_curves"));
        // 搜索词（words 字段）也参与匹配
        assert!(
            filter_topics("unlink")
                .iter()
                .any(|t| t.id == "funnel_links")
        );
        // 所有词都要在：没有主题同时提到 eraser 和 velocity
        assert!(filter_topics("eraser velocity").is_empty());
        let funnel = help_texts::by_id("funnel").expect("funnel 主题");
        assert!(topic_matches(funnel, "FUNNEL"));
        assert!(topic_matches(funnel, "  wall   gate "));
        assert!(!topic_matches(funnel, "wall tunisia"));
    }

    #[test]
    fn every_tool_has_a_topic() {
        for tool in Tool::ALL {
            let id = tool_topic(tool);
            assert!(help_texts::by_id(id).is_some(), "{tool:?} -> {id}");
        }
        assert_eq!(tool_topic(Tool::Square), tool_topic(Tool::Triangle));
    }

    #[test]
    fn side_help_is_the_tool_text() {
        let text = help_texts::by_id(tool_topic(Tool::Funnel)).expect("funnel");
        assert!(text.text.contains("wall"));
    }

    #[test]
    fn tips_persist_and_show_once() {
        let tmp = crate::test_support::TempDir::new("tips");
        let mut tips = Tips::new(tmp.path());
        assert_eq!(tips.path(), tmp.path().join(TIPS_FILE));
        assert!(tips.on);
        assert!(tips.seen.is_empty());

        tips.show("welcome");
        assert_eq!(tips.popup.as_deref(), Some("welcome"));
        assert!(tips.seen.contains("welcome"));
        assert!(std::fs::read_to_string(tips.path()).is_ok());

        // 重开：看过的不再弹
        let mut again = Tips::new(tmp.path());
        assert!(again.seen.contains("welcome"));
        again.show("welcome");
        assert!(again.popup.is_none());
    }

    #[test]
    fn tips_got_it_follows_next_and_waits() {
        let tmp = crate::test_support::TempDir::new("tips-next");
        let mut tips = Tips::new(tmp.path());
        tips.show("welcome");
        tips.got_it();
        assert_eq!(tips.popup.as_deref(), Some("view"));

        // wait：正弹着别的先排队，关掉后接着弹
        tips.close();
        assert!(tips.popup.is_none());
        tips.show("line");
        tips.show_waiting("funnel");
        assert_eq!(tips.popup.as_deref(), Some("line"));
        assert_eq!(tips.waiting.as_deref(), Some("funnel"));
        tips.close();
        assert_eq!(tips.popup.as_deref(), Some("funnel"));

        // 关掉 tips 就不弹、也不记
        let off = crate::test_support::TempDir::new("tips-off");
        let mut tips = Tips::new(off.path());
        tips.on = false;
        tips.show("line");
        assert!(tips.popup.is_none());
        assert!(!tips.seen.contains("line"));

        // reset：全都没看过
        tips.reset();
        assert!(tips.on && tips.seen.is_empty());
    }
}
