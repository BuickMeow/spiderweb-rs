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
    /// (topic, force) shown one after another once the open tip is closed (upstream waiting).
    pub waiting: Vec<(String, bool)>,
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
            waiting: Vec::new(),
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

    /// 弹这个主题的 tip（看过了 / 关掉了就不弹）。正弹着别的主题时直接换掉（被换掉的稍后补弹）。
    pub fn show(&mut self, topic: &str) {
        self.show_ex(topic, false, false, false);
    }

    /// 同 [`Tips::show`]，但正弹着别的主题时排队等它关掉（原版 wait=True）。
    pub fn show_waiting(&mut self, topic: &str) {
        self.show_ex(topic, true, false, false);
    }

    fn show_ex(&mut self, topic: &str, wait: bool, force: bool, done: bool) {
        if help_texts::by_id(topic).is_none() || (!force && (!self.on || self.seen.contains(topic)))
        {
            return;
        }
        let open_now = self.popup.is_some();
        if wait && open_now && self.popup.as_deref() != Some(topic) {
            if !self.waiting.iter().any(|(t, _)| t == topic) {
                self.waiting.push((topic.to_string(), force));
            }
            return;
        }
        self.waiting.retain(|(t, _)| t != topic);
        if open_now
            && !done
            && self.popup.as_deref() != Some(topic)
            && !is_tool_tip(self.popup.as_deref().unwrap_or(""))
        {
            // A tip pushed aside (by a tool's tip, say) comes back next; one tool's tip replacing
            // another's doesn't.
            if let Some(old) = self.popup.clone() {
                self.waiting.insert(0, (old, true));
            }
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
            Some(n) if self.on && !self.seen.contains(&n) => self.show_ex(&n, false, false, true),
            _ => self.close(),
        }
    }

    /// 关掉当前 tip（窗口 X / Esc），排队的接着来（看过的不再弹）。
    pub fn close(&mut self) {
        self.popup = None;
        while self.popup.is_none() && !self.waiting.is_empty() {
            let (topic, force) = self.waiting.remove(0);
            self.show_ex(&topic, false, force, false);
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

/// The topics a tool's tip uses (upstream TOOL_TIPS: TOOL_TOPICS + DRAWER_TOOL_TOPICS). A tool tip
/// replacing another tip doesn't come back later.
fn is_tool_tip(id: &str) -> bool {
    help_texts::TOOL_TOPICS.iter().any(|(_, t)| *t == id)
        || matches!(
            id,
            "drawer_select"
                | "drawer_line"
                | "drawer_poly"
                | "drawer_free"
                | "drawer_curve"
                | "drawer_arc"
                | "drawer_square"
                | "drawer_circle"
                | "drawer_erase"
        )
}

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
        Some(t) => rust_i18n::t!("help.side", title = t.title, text = t.text).to_string(),
        None => String::new(),
    }
}

/// 侧栏底部一块：当前工具的帮助 + 打开帮助窗口的按钮。
pub fn side_help_ui(app: &mut App, ui: &mut egui::Ui) {
    if help_texts::by_id(tool_topic(app.tool)).is_none() {
        return;
    }
    let mut open = false;
    egui::CollapsingHeader::new(rust_i18n::t!("help.side_title"))
        .default_open(true)
        .id_salt("side_help")
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(side_help_text(app))
                    .small()
                    .color(egui::Color32::from_gray(120)),
            );
            open = ui.button(rust_i18n::t!("help.button")).clicked();
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

/// Typing anywhere in the Help window types into the search box (upstream HelpWindow.type_to_search):
/// printable characters go in, Backspace takes the last one off.
fn type_query(query: &mut String, events: &[egui::Event]) {
    for ev in events {
        match ev {
            egui::Event::Text(t) if !t.chars().any(char::is_control) => query.push_str(t),
            egui::Event::Key {
                key: egui::Key::Backspace,
                pressed: true,
                ..
            } => {
                query.pop();
            }
            _ => {}
        }
    }
}

/// 帮助窗口的界面（每帧调用；没打开就什么都不做）。
pub fn help_ui(app: &mut App, ctx: &egui::Context) {
    if !app.help.open {
        return;
    }
    // No box has the keyboard: whatever is typed is a search (upstream type_to_search).
    if ctx.memory(|m| m.focused().is_none()) {
        let events = ctx.input(|i| i.events.clone());
        type_query(&mut app.help.query, &events);
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

    // 主体至少这么高：egui 的窗口会缩到内容高度，不设的话会比原版的 900x620 矮很多
    let body_h = (ctx.viewport_rect().height() * 0.72).clamp(520.0, 900.0);
    egui::Window::new(rust_i18n::t!("help.title", version = VERSION))
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
                            .hint_text(rust_i18n::t!("help.search_hint"))
                            .desired_width(ui.available_width()),
                    );
                    ui.label(
                        egui::RichText::new(rust_i18n::t!("help.search_note"))
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
                            show_topic(ui, cur.as_deref(), &mut selected, &mut link);
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut on, rust_i18n::t!("help.show_first_tip"));
                if ui.button(rust_i18n::t!("help.show_all")).clicked() {
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
    selected: &mut Option<String>,
    link: &mut Option<&'static str>,
) {
    let Some(t) = id.and_then(help_texts::by_id) else {
        ui.label(
            egui::RichText::new(rust_i18n::t!("help.nothing_found"))
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
        if ui.button(rust_i18n::t!("help.website")).clicked() {
            *link = Some(WEBSITE);
        }
    }
    let see = help_texts::see(t.id);
    if !see.is_empty() {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.weak(rust_i18n::t!("help.see_also"));
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
    egui::Window::new(rust_i18n::t!("tip.title"))
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
                if ui.checkbox(&mut on, rust_i18n::t!("tip.show")).changed() {
                    on_changed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(rust_i18n::t!("tip.got_it")).clicked() {
                        got_it = true;
                    }
                    if ui.button(rust_i18n::t!("tip.more")).clicked() {
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
        app.tips.close(); // (the next waiting tip comes up, like upstream closed())
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
    fn typing_anywhere_fills_the_search_box() {
        let mut query = String::new();
        type_query(
            &mut query,
            &[
                egui::Event::Text("fur".to_string()),
                egui::Event::Text(" ".to_string()),
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Text("n".to_string()),
            ],
        );
        assert_eq!(query, "furn");
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
        assert_eq!(tips.waiting, vec![("funnel".to_string(), false)]);
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

    /// Upstream 1.2.0 tips queue: a tip pushed aside comes back next (a tool's tip doesn't),
    /// and "Got it" going on to the next one doesn't queue the old one again.
    #[test]
    fn tips_queue_comes_back_and_got_it_goes_on() {
        let tmp = crate::test_support::TempDir::new("tips-queue");
        let mut tips = Tips::new(tmp.path());

        // A non-tool tip pushed aside by another comes back next.
        tips.show("view");
        tips.show("history");
        assert_eq!(tips.popup.as_deref(), Some("history"));
        assert_eq!(
            tips.waiting,
            vec![("view".to_string(), true)],
            "the pushed-aside tip comes back"
        );
        tips.close();
        assert_eq!(tips.popup.as_deref(), Some("view"));
        tips.close();
        assert!(tips.popup.is_none());

        // A tool tip replaced by another doesn't come back.
        tips.show("funnel"); // a tool tip (Funnel)
        tips.show("undo");
        assert_eq!(tips.popup.as_deref(), Some("undo"));
        assert!(
            !tips.waiting.iter().any(|(t, _)| t == "funnel"),
            "a replaced tool tip stays gone"
        );

        // "Got it" on welcome goes to view (NEXT) without queueing welcome again.
        let tmp2 = crate::test_support::TempDir::new("tips-next2");
        let mut t2 = Tips::new(tmp2.path());
        t2.show("welcome");
        t2.got_it();
        assert_eq!(t2.popup.as_deref(), Some("view"));
        assert!(t2.waiting.is_empty(), "welcome isn't queued back");
    }
}
