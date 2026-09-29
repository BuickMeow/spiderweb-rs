//! 文本工具（原版 roll/roll_text.py）：在卷帘上点击放文本、打字输入、光标与选区。
//!
//! 文本形状是自定义形状：`sh.text` 存设置，`sh.strokes` 是字形轮廓，`sh.pts` 是三个框点。
//! 每次编辑都走 [`spiderweb_core::text::build`] 重排；只剩空格的文本没有形状，设置与轴留在
//! [`Typing`] 里，等下一个可见字符再长出来。键盘事件由 `App::ui` 最先交给 [`text_keyboard`]
//! （原版 `on_key` 的 typing 优先级：打字时全局快捷键让路）。

use std::time::Duration;

use eframe::egui;
use egui::{Color32, Pos2, Rect, Stroke};

use spiderweb_core::Pt;
use spiderweb_core::shape::{Kind, Shape, TextSettings};
use spiderweb_core::text::{self, Axes, TextChange};

use crate::app::{App, Tool};
use crate::roll::Drag;

/// 光标闪烁半周期，毫秒（原版 BLINK_MS）。
pub const BLINK_MS: u64 = 530;

/// 正在输入的文本（原版 self.typing 字典）。
#[derive(Clone, Debug)]
pub struct Typing {
    /// 正在输入的形状序号；还没有可见字符时为 None
    pub i: Option<usize>,
    /// 光标位置（第几个字符前）
    pub caret: usize,
    /// 选区的另一端（== caret 时没有选区）
    pub anchor: usize,
    /// 还没有形状时的设置与轴
    pub tx: Option<TextSettings>,
    pub axes: Option<Axes>,
    /// 这个形状的输入是否已经压过撤销步
    pub undo: bool,
}

// ---------------------------------------------------------------- 纯逻辑（可单测）

/// 在字符区间 s0..s1 里插入 s，返回新文本与新光标位置（原版 insert_text 的字符串部分）。
pub fn insert_str(text: &str, s0: usize, s1: usize, s: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let s0 = s0.min(n);
    let s1 = s1.clamp(s0, n);
    let mut out: String = chars[..s0].iter().collect();
    out.push_str(s);
    out.extend(chars[s1..].iter());
    (out, s0 + s.chars().count())
}

/// Backspace：删掉光标前一个字符（原版 type_key 的 BackSpace 分支）。返回新文本与新光标。
pub fn backspace(text: &str, caret: usize) -> (String, usize) {
    insert_str(text, caret.saturating_sub(1), caret, "")
}

/// Delete：删掉光标处的字符（原版 type_key 的 Delete 分支）。返回新文本与新光标。
pub fn delete_at(text: &str, caret: usize) -> (String, usize) {
    insert_str(text, caret, caret.saturating_add(1), "")
}

/// 光标移动方向（原版 type_key 的 Left / Right / Home / End / Up / Down）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaretDir {
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
}

/// 第 line 行首在字符数组里的下标（原版 rfind("\n") + 1）。
fn line_start(chars: &[char], line: usize) -> usize {
    let mut at = 0;
    let mut n = 0;
    for (i, &c) in chars.iter().enumerate() {
        if n == line {
            break;
        }
        if c == '\n' {
            n += 1;
            at = i + 1;
        }
    }
    at
}

/// 从 start 起这一行的行尾下标（不含换行符）。
fn line_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&c| c == '\n')
        .map(|p| p + start)
        .unwrap_or(chars.len())
}

/// 移动光标（原版 type_key 的光标分支）：`anchor` 是选区的另一端，`shift` = 扩选。
pub fn move_caret(text: &str, caret: usize, anchor: usize, dir: CaretDir, shift: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let caret = caret.min(n);
    let (lo, hi) = (caret.min(anchor), caret.max(anchor));
    match dir {
        CaretDir::Left | CaretDir::Right => {
            if lo != hi && !shift {
                // 有选区：左到选区头、右到选区尾
                if dir == CaretDir::Left { lo } else { hi }
            } else if dir == CaretDir::Left {
                caret.saturating_sub(1)
            } else {
                (caret + 1).min(n)
            }
        }
        CaretDir::Home | CaretDir::End => {
            let line = chars[..caret].iter().filter(|&&c| c == '\n').count();
            let start = line_start(&chars, line);
            if dir == CaretDir::Home {
                start
            } else {
                line_end(&chars, start)
            }
        }
        CaretDir::Up | CaretDir::Down => {
            let line = chars[..caret].iter().filter(|&&c| c == '\n').count();
            let start = line_start(&chars, line);
            let col = caret - start;
            let newlines = chars.iter().filter(|&&c| c == '\n').count();
            let target = if dir == CaretDir::Up {
                line.checked_sub(1)
            } else {
                (line < newlines).then_some(line + 1)
            };
            match target {
                Some(m) => {
                    let s = line_start(&chars, m);
                    s + col.min(line_end(&chars, s) - s)
                }
                None => caret,
            }
        }
    }
}

// ---------------------------------------------------------------- 命中与坐标

/// 文本框的四个角（原版 custom_corners）：u0v0, u1v0, u1v1, u0v1，拍 / 音高。
fn corners(sh: &Shape) -> Option<[Pt; 4]> {
    if sh.pts.len() < 3 {
        return None;
    }
    let ([b0, p0], [b1, p1], [b2, p2]) = (sh.pts[0], sh.pts[1], sh.pts[2]);
    Some([[b0, p0], [b1, p1], [b1 + b2 - b0, p1 + p2 - p0], [b2, p2]])
}

/// 屏幕点 (x, y) 在不在这个平行四边形框里（原版 inside_box）。
fn inside_box(c: &[Pt; 4], v: &crate::roll::View, x: f32, y: f32) -> bool {
    let (ax, ay) = (v.x_of(c[0][0]) as f64, v.y_of(c[0][1]) as f64);
    let (bx, by) = (v.x_of(c[1][0]) as f64, v.y_of(c[1][1]) as f64);
    let (dx, dy) = (v.x_of(c[3][0]) as f64, v.y_of(c[3][1]) as f64);
    let (ux, uy, vx, vy) = (bx - ax, by - ay, dx - ax, dy - ay);
    let det = ux * vy - uy * vx;
    if det.abs() < 1e-9 {
        return false;
    }
    let u = ((x as f64 - ax) * vy - (y as f64 - ay) * vx) / det;
    let w = (ux * (y as f64 - ay) - uy * (x as f64 - ax)) / det;
    (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&w)
}

/// 屏幕点下面的文本形状（从新到旧）（原版 text_at）。
pub fn text_at(app: &App, pos: Pos2) -> Option<usize> {
    for i in (0..app.shapes.len()).rev() {
        let sh = &app.shapes[i];
        if sh.text.is_none() {
            continue;
        }
        if let Some(c) = corners(sh)
            && inside_box(&c, &app.view, pos.x, pos.y)
        {
            return Some(i);
        }
    }
    None
}

/// 屏幕位置 -> 卷帘 (beat, pitch)（与 roll::event_pt 同一套夹取）。
fn roll_pt(app: &App, pos: Pos2) -> Pt {
    let v = &app.view;
    let x = pos.x.clamp(v.kb_w, v.w);
    let y = pos.y.clamp(v.ruler_h, v.h);
    [v.b_of(x), v.p_of(y)]
}

/// 正在输入的（设置, 轴）（原版 typing_state）。
pub fn typing_state(app: &App) -> Option<(TextSettings, Axes)> {
    let ty = app.typing.as_ref()?;
    match ty.i {
        Some(i) => {
            let sh = app.shapes.get(i)?;
            Some((sh.text.clone()?, text::text_axes(sh)?))
        }
        None => Some((ty.tx.clone()?, ty.axes?)),
    }
}

/// 输入中文本的内容。
fn typing_text(app: &App) -> Option<String> {
    typing_state(app).map(|(tx, _)| tx.text)
}

/// 选区的 (起点, 终点)；相等 = 没有选区（原版 text_selection）。
pub fn selection(app: &App) -> (usize, usize) {
    app.typing
        .as_ref()
        .map(|t| (t.anchor.min(t.caret), t.anchor.max(t.caret)))
        .unwrap_or((0, 0))
}

/// 鼠标最近的光标位置（原版 caret_at）：先按行高选行，再选行里 x 最近的位置。
pub fn caret_at(app: &App, pos: Pos2) -> usize {
    let Some((tx, axes)) = typing_state(app) else {
        return 0;
    };
    let chars: Vec<char> = tx.text.chars().collect();
    let [b, p] = roll_pt(app, pos);
    let Some(em) = text::from_roll(axes, b, p) else {
        return chars.len();
    };
    let font = text::text_font(&tx);
    let (_, spots) = text::layout(&tx, &font);
    if spots.is_empty() {
        return chars.len();
    }
    let mid = (font.ascent - font.descent) / 2.0;
    let line_of: Vec<usize> = (0..=chars.len())
        .map(|j| chars[..j].iter().filter(|&&c| c == '\n').count())
        .collect();
    // 每行的基线 y（同一行都一样，后写的覆盖前写的）
    let mut base: Vec<(usize, f64)> = Vec::new();
    for (j, p) in spots.iter().enumerate() {
        let line = line_of.get(j).copied().unwrap_or(0);
        match base.iter_mut().find(|(n, _)| *n == line) {
            Some((_, by)) => *by = p[1],
            None => base.push((line, p[1])),
        }
    }
    let line = base
        .iter()
        .min_by(|(_, a), (_, b)| ((em[1] - a - mid).abs()).total_cmp(&(em[1] - b - mid).abs()))
        .map(|(n, _)| *n)
        .unwrap_or(0);
    (0..spots.len())
        .filter(|&j| line_of.get(j).copied() == Some(line))
        .min_by(|&a, &b| {
            (em[0] - spots[a][0])
                .abs()
                .total_cmp(&(em[0] - spots[b][0]).abs())
        })
        .unwrap_or(chars.len())
}

// ---------------------------------------------------------------- 点击 / 拖动 / 双击

/// 一次点击（原版 text_click）：点在正在输入的文本上 = 移动光标 / 扩选；否则结束输入，
/// 点已有文本 = 在那里接着打，点空白 = 放一段新文本。
fn press(app: &mut App, pos: Pos2, pt: Pt, shift: bool) {
    app.drag = Some(Drag::TextSel);
    let i = text_at(app, pos);
    if let Some(ty) = app.typing.as_ref()
        && let (Some(ti), Some(i)) = (ty.i, i)
        && ti == i
    {
        let caret = caret_at(app, pos);
        if let Some(ty) = app.typing.as_mut() {
            ty.caret = caret;
            if !shift {
                ty.anchor = caret;
            }
        }
        return;
    }
    end_typing(app);
    match i {
        Some(i) => {
            app.select(Some(i), false);
            app.typing = Some(Typing {
                i: Some(i),
                caret: 0,
                anchor: 0,
                tx: None,
                axes: None,
                undo: false,
            });
            let caret = caret_at(app, pos);
            if let Some(ty) = app.typing.as_mut() {
                ty.caret = caret;
                ty.anchor = caret;
            }
        }
        None => {
            app.select(None, false);
            let mut tx = app.text_defaults.clone();
            tx.text.clear();
            tx.k = app.view.sy / app.view.sx;
            tx.cap = text::text_font(&tx).cap;
            let axes = text::new_axes(&tx, pt[0], pt[1], tx.k, tx.cap);
            app.typing = Some(Typing {
                i: None,
                caret: 0,
                anchor: 0,
                tx: Some(tx),
                axes: Some(axes),
                undo: false,
            });
        }
    }
}

/// 文本工具点击（原版 text_click）。
pub fn text_press(app: &mut App, pos: Pos2, pt: Pt) {
    press(app, pos, pt, false);
}

/// 文本工具点击，Shift = 从原来那头扩选。
pub fn text_press_shift(app: &mut App, pos: Pos2, pt: Pt, shift: bool) {
    press(app, pos, pt, shift);
}

/// 点击后拖动：从按下处选到鼠标（原版 text_drag）。
pub fn text_drag(app: &mut App, pos: Pos2) {
    if app.typing.is_none() {
        return;
    }
    let caret = caret_at(app, pos);
    if let Some(ty) = app.typing.as_mut() {
        ty.caret = caret;
    }
}

/// Select 工具双击文本：切到文本工具并接着打（原版 edit_text）。
pub fn text_edit(app: &mut App, pos: Pos2) {
    app.tool = Tool::Text;
    app.draw_tool = Tool::Text;
    let pt = crate::roll::event_pt(app, pos, true, false);
    text_press(app, pos, pt);
}

/// 文本工具双击：选中鼠标下的一个词（原版 text_double）。
pub fn text_double(app: &mut App, pos: Pos2) {
    let same = app
        .typing
        .as_ref()
        .and_then(|t| t.i)
        .map(|i| Some(i) == text_at(app, pos))
        .unwrap_or(false);
    if !same {
        let pt = crate::roll::event_pt(app, pos, true, false);
        text_press(app, pos, pt);
        return;
    }
    let Some((tx, _)) = typing_state(app) else {
        return;
    };
    let chars: Vec<char> = tx.text.chars().collect();
    let c = caret_at(app, pos);
    let word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let kind = if c < chars.len() {
        word(chars[c])
    } else if c > 0 {
        word(chars[c - 1])
    } else {
        false
    };
    let mut a = c;
    while a > 0 && chars[a - 1] != '\n' && word(chars[a - 1]) == kind {
        a -= 1;
    }
    let mut b = c;
    while b < chars.len() && chars[b] != '\n' && word(chars[b]) == kind {
        b += 1;
    }
    if let Some(ty) = app.typing.as_mut() {
        ty.anchor = a;
        ty.caret = b;
    }
}

/// 结束输入（原版 end_typing）。
pub fn end_typing(app: &mut App) {
    app.typing = None;
}

// ---------------------------------------------------------------- 文本编辑

/// 把文本写进正在输入的东西（原版 set_text）：有形状就地重排，只有空格时把形状删掉；
/// 还没有形状时，第一个看得见的字符让形状长出来（同一次输入只压一次撤销步）。
pub fn set_text(app: &mut App, text: &str, caret: usize) {
    let Some((mut tx, axes)) = typing_state(app) else {
        return;
    };
    tx.text = text.to_string();
    if let Some(ty) = app.typing.as_mut() {
        ty.caret = caret;
        ty.anchor = caret;
    }
    let font = text::text_font(&tx);
    match app.typing.as_ref().and_then(|t| t.i) {
        Some(i) => {
            // 这次输入的第一次改动才压撤销步（原版 ty["undo"]）
            if !app.typing.as_ref().map(|t| t.undo).unwrap_or(false) {
                app.push_undo();
                if let Some(ty) = app.typing.as_mut() {
                    ty.undo = true;
                }
            }
            let Some(sh) = app.shapes.get_mut(i) else {
                return;
            };
            if text::build(sh, &tx, &font, axes) {
                app.shapes_changed();
            } else {
                // 看不见了：形状收起来，输入状态留着
                app.shapes.remove(i);
                if let Some(ty) = app.typing.as_mut() {
                    ty.i = None;
                    ty.tx = Some(tx);
                    ty.axes = Some(axes);
                }
                app.select(None, false);
                app.shapes_changed();
            }
        }
        None => {
            let mut sh = app.defaults.clone();
            sh.kind = Kind::Custom;
            crate::roll_live::core_custom_defaults(app).apply(&mut sh);
            if text::build(&mut sh, &tx, &font, axes) {
                let idx = app.shapes.len();
                let undo_done = app.typing.as_ref().map(|t| t.undo).unwrap_or(false);
                if let Some(ty) = app.typing.as_mut() {
                    ty.i = Some(idx);
                }
                if undo_done {
                    // 这次输入已经压过撤销步：形状可能删过又长出来，直接接回列表
                    app.shapes.push(sh);
                    app.select(Some(idx), false);
                    app.shapes_changed();
                } else {
                    app.add_shape(sh);
                    if let Some(ty) = app.typing.as_mut() {
                        ty.undo = true;
                    }
                }
            } else if let Some(ty) = app.typing.as_mut() {
                ty.tx = Some(tx);
                ty.axes = Some(axes);
            }
        }
    }
}

/// 在光标处插入一段字符，替换选区（原版 insert_text）。
pub fn insert_text(app: &mut App, s: &str) {
    let Some(text) = typing_text(app) else {
        return;
    };
    let (s0, s1) = selection(app);
    let (new, caret) = insert_str(&text, s0, s1, s);
    set_text(app, &new, caret);
}

/// 面板改了设置后的文字输入现场（原版 retype）：设置与轴换了，文本就地重排或长出来。
pub fn retype(app: &mut App, tx: TextSettings, axes: Axes) {
    match app.typing.as_ref().and_then(|t| t.i) {
        None => {
            if let Some(ty) = app.typing.as_mut() {
                ty.tx = Some(tx.clone());
                ty.axes = Some(axes);
            }
            let caret = app.typing.as_ref().map(|t| t.caret).unwrap_or(0);
            set_text(app, &tx.text, caret);
        }
        Some(i) => {
            let font = text::text_font(&tx);
            if let Some(sh) = app.shapes.get_mut(i) {
                text::build(sh, &tx, &font, axes);
            }
            // 面板自己压过撤销步：下一次按键重新开始一步
            if let Some(ty) = app.typing.as_mut() {
                ty.undo = false;
            }
        }
    }
}

/// 面板改了一次文本设置（原版 set_text_setting）：改默认值、正在输入的文本或选中的文本们，
/// 第一行起点不动（restyle 缩放轴），然后重算音符。
pub fn set_text_setting(app: &mut App, changes: &TextChange) {
    app.text_defaults = changes.apply(&app.text_defaults);
    if app.typing.is_some() {
        let Some((tx, axes)) = typing_state(app) else {
            return;
        };
        let old_cap = text::text_font(&tx).cap;
        let new_cap = text::text_font(&changes.apply(&tx)).cap;
        if app.typing.as_ref().and_then(|t| t.i).is_some() {
            app.push_undo();
        }
        let (new_tx, new_axes) = text::restyle(&tx, axes, changes, old_cap, new_cap);
        retype(app, new_tx, new_axes);
        app.shapes_changed();
    } else {
        let idxs: Vec<usize> = app
            .sels
            .iter()
            .copied()
            .filter(|&i| {
                app.shapes
                    .get(i)
                    .map(|sh| sh.text.is_some())
                    .unwrap_or(false)
            })
            .collect();
        if !idxs.is_empty() {
            app.push_undo();
        }
        for i in idxs {
            let (tx, axes) = {
                let Some(sh) = app.shapes.get(i) else {
                    continue;
                };
                let Some(tx) = sh.text.clone() else {
                    continue;
                };
                let Some(axes) = text::text_axes(sh) else {
                    continue;
                };
                (tx, axes)
            };
            let old_cap = text::text_font(&tx).cap;
            let new_cap = text::text_font(&changes.apply(&tx)).cap;
            let (new_tx, new_axes) = text::restyle(&tx, axes, changes, old_cap, new_cap);
            let font = text::text_font(&new_tx);
            if let Some(sh) = app.shapes.get_mut(i) {
                text::build(sh, &new_tx, &font, new_axes);
            }
        }
        app.shapes_changed();
    }
    app.panel_sel = None;
}

// ---------------------------------------------------------------- 键盘

/// 处理一个输入事件；true = 文本输入消费了它（原版 type_key 的 "break"）。
pub fn text_key(app: &mut App, event: &egui::Event) -> bool {
    match event {
        egui::Event::Copy => {
            copy_selection(app, false);
            true
        }
        egui::Event::Cut => {
            copy_selection(app, true);
            true
        }
        egui::Event::Paste(s) => {
            let s = s
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\t', " ");
            insert_text(app, &s);
            true
        }
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => key_event(app, *key, *modifiers),
        egui::Event::Text(s) => {
            let clean: String = s.chars().filter(|&c| c >= ' ' && c != '\u{7f}').collect();
            if !clean.is_empty() {
                insert_text(app, &clean);
            }
            true
        }
        _ => false,
    }
}

/// 打字时的键盘入口：`typing` 激活时最先消费事件，全局快捷键让路（原版 on_key 的优先级）。
pub fn text_keyboard(app: &mut App, ctx: &egui::Context) {
    if app.typing.is_none() || ctx.egui_wants_keyboard_input() {
        return;
    }
    let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
    let mut leftover = Vec::new();
    for ev in events {
        if !text_key(app, &ev) {
            leftover.push(ev);
        }
    }
    ctx.input_mut(|i| {
        let rest = std::mem::take(&mut i.events);
        i.events = leftover.into_iter().chain(rest).collect();
    });
    if let Some(s) = app.text_clipboard.take() {
        ctx.copy_text(s);
    }
}

/// 一次按键（原版 type_key）：Ctrl 组合走复制 / 撤销等，其余是编辑与光标移动。
fn key_event(app: &mut App, key: egui::Key, modifiers: egui::Modifiers) -> bool {
    use egui::Key;
    if modifiers.command || modifiers.ctrl {
        match key {
            Key::Z => {
                end_typing(app);
                app.undo();
            }
            Key::Y => {
                end_typing(app);
                app.redo();
            }
            Key::A => select_all(app),
            Key::C => copy_selection(app, false),
            Key::X => copy_selection(app, true),
            // Ctrl+V 由 Event::Paste 处理；其余 Ctrl+键在打字时都不触发全局快捷键
            _ => {}
        }
        return true;
    }
    match key {
        Key::Escape => end_typing(app),
        Key::Enter => insert_text(app, "\n"),
        Key::Backspace => edit_backspace(app),
        Key::Delete => edit_delete(app),
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => {
            let dir = match key {
                Key::ArrowLeft => CaretDir::Left,
                Key::ArrowRight => CaretDir::Right,
                Key::ArrowUp => CaretDir::Up,
                Key::ArrowDown => CaretDir::Down,
                Key::Home => CaretDir::Home,
                _ => CaretDir::End,
            };
            move_caret_key(app, dir, modifiers.shift);
        }
        _ => {}
    }
    true
}

/// Backspace：有选区删选区，否则删光标前一个字符。
fn edit_backspace(app: &mut App) {
    let (s0, s1) = selection(app);
    if s0 != s1 {
        insert_text(app, "");
        return;
    }
    let Some(text) = typing_text(app) else {
        return;
    };
    let (new, caret) = backspace(&text, s0);
    set_text(app, &new, caret);
}

/// Delete：有选区删选区，否则删光标处的字符。
fn edit_delete(app: &mut App) {
    let (s0, s1) = selection(app);
    if s0 != s1 {
        insert_text(app, "");
        return;
    }
    let Some(text) = typing_text(app) else {
        return;
    };
    let (new, caret) = delete_at(&text, s0);
    set_text(app, &new, caret);
}

/// 方向键：移动光标；Shift 时保留选区另一端。
fn move_caret_key(app: &mut App, dir: CaretDir, shift: bool) {
    let Some(text) = typing_text(app) else {
        return;
    };
    let Some((caret, anchor)) = app.typing.as_ref().map(|t| (t.caret, t.anchor)) else {
        return;
    };
    let c = move_caret(&text, caret, anchor, dir, shift);
    if let Some(ty) = app.typing.as_mut() {
        ty.caret = c;
        if !shift {
            ty.anchor = c;
        }
    }
}

/// 全选正在输入的这段文本（不是所有形状）。
fn select_all(app: &mut App) {
    let Some(text) = typing_text(app) else {
        return;
    };
    if let Some(ty) = app.typing.as_mut() {
        ty.anchor = 0;
        ty.caret = text.chars().count();
    }
}

/// Ctrl+C / Ctrl+X：有选区复制（剪切再删）；没选区时 Ctrl+C 复制选中的形状（原版行为）。
fn copy_selection(app: &mut App, cut: bool) {
    let (s0, s1) = selection(app);
    if s0 == s1 {
        if !cut {
            app.copy_selected();
        }
        return;
    }
    let Some(text) = typing_text(app) else {
        return;
    };
    let chars: Vec<char> = text.chars().collect();
    let s: String = chars[s0..s1].iter().collect();
    app.text_clipboard = Some(s);
    if cut {
        insert_text(app, "");
    }
}

// ---------------------------------------------------------------- 绘制

/// 画正在输入的文本：虚线框、选中字符的高亮、闪烁的光标（原版 draw_typing）。
pub fn paint_text_caret(app: &App, painter: &egui::Painter, rect: Rect) {
    let Some(ty) = app.typing.as_ref() else {
        return;
    };
    let Some((tx, axes)) = typing_state(app) else {
        return;
    };
    let font = text::text_font(&tx);
    let (_, spots) = text::layout(&tx, &font);
    let Some(&first) = spots.first() else {
        return;
    };
    // em 单位 -> 屏幕
    let at = |x: f64, y: f64| -> Pos2 {
        let p = text::to_roll(axes, x, y);
        Pos2::new(
            rect.min.x + app.view.x_of(p[0]),
            rect.min.y + app.view.y_of(p[1]),
        )
    };
    let (mut xlo, mut xhi, mut ylo, mut yhi) = (first[0], first[0], first[1], first[1]);
    for p in &spots {
        xlo = xlo.min(p[0]);
        xhi = xhi.max(p[0]);
        ylo = ylo.min(p[1]);
        yhi = yhi.max(p[1]);
    }
    let blue = Color32::from_rgb(0x3a, 0x7b, 0xd5);
    let lw = app.scale().max(1.0);
    let pad = 0.08;
    // 虚线框（跟着文字转 / 斜）
    let corners = [
        at(xlo - pad, ylo - font.descent - pad),
        at(xhi + pad, ylo - font.descent - pad),
        at(xhi + pad, yhi + font.ascent + pad),
        at(xlo - pad, yhi + font.ascent + pad),
        at(xlo - pad, ylo - font.descent - pad),
    ];
    for w in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(
            &[w[0], w[1]],
            Stroke::new(lw, blue),
            4.0 * lw,
            3.0 * lw,
        ));
    }
    // 选中字符
    let chars: Vec<char> = tx.text.chars().collect();
    let (s0, s1) = (ty.anchor.min(ty.caret), ty.anchor.max(ty.caret));
    let highlight = Color32::from_rgba_unmultiplied(0x3a, 0x7b, 0xd5, 0x70);
    for j in s0..s1 {
        let Some(&p) = spots.get(j) else {
            break;
        };
        let (x0, y) = (p[0], p[1]);
        let x1 = match (chars.get(j), spots.get(j + 1)) {
            // 选中的换行符：画一小段
            (Some('\n'), _) => x0 + 0.25,
            (_, Some(s)) => s[0],
            _ => x0,
        };
        let q = vec![
            at(x0, y - font.descent),
            at(x1, y - font.descent),
            at(x1, y + font.ascent),
            at(x0, y + font.ascent),
        ];
        painter.add(egui::Shape::convex_polygon(q, highlight, Stroke::NONE));
    }
    // 光标（闪烁）
    if s0 == s1 {
        let phase = painter.ctx().input(|i| i.time) * 1000.0 / BLINK_MS as f64;
        if (phase as u64).is_multiple_of(2) {
            let j = ty.caret.min(spots.len().saturating_sub(1));
            let [x, y] = spots[j];
            painter.line_segment(
                [at(x, y - font.descent), at(x, y + font.ascent)],
                Stroke::new((2.0 * lw).max(2.0), Color32::BLACK),
            );
        }
    }
    painter
        .ctx()
        .request_repaint_after(Duration::from_millis(BLINK_MS));
}

#[cfg(test)]
mod tests {
    use super::*;
    use spiderweb_core::shape::{TextAlign, TextUnit};

    /// 打字编辑：在光标处插入（字符下标，中文也一样）并写进 Shape.text。
    #[test]
    fn typing_edits_shape_text() {
        let mut sh = Shape::new(Kind::Custom, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        sh.text = Some(TextSettings {
            text: "heo".to_string(),
            ..Default::default()
        });
        // 在 e 后插入 "ll"：heo -> hello
        let text0 = sh.text.as_ref().map(|t| t.text.clone()).unwrap_or_default();
        let (text, caret) = insert_str(&text0, 2, 2, "ll");
        if let Some(tx) = sh.text.as_mut() {
            tx.text = text;
        }
        assert_eq!(sh.text.as_ref().map(|t| t.text.as_str()), Some("hello"));
        assert_eq!(caret, 4);
        // 中文按字符而不是字节：删掉 "l"，再在 "中" 前插一个字符
        let (text, _) = delete_at("hello", 2);
        assert_eq!(text, "helo");
        let (text, caret) = insert_str("a中b", 1, 1, "X");
        assert_eq!(text, "aX中b");
        assert_eq!(caret, 2);
        if let Some(tx) = sh.text.as_mut() {
            tx.text = text;
        }
        assert_eq!(sh.text.as_ref().map(|t| t.text.as_str()), Some("aX中b"));
    }

    /// 选区替换：选中的字符被输入替换，Backspace / Delete 按光标走。
    #[test]
    fn insert_replaces_selection_and_deletes() {
        // 选区 1..3（"el"）换成 "Z"
        assert_eq!(insert_str("hello", 1, 3, "Z"), ("hZlo".to_string(), 2));
        // Backspace 删光标前一个；光标 0 不动
        assert_eq!(backspace("ab", 1), ("b".to_string(), 0));
        assert_eq!(backspace("ab", 0), ("ab".to_string(), 0));
        // Delete 删光标处；末尾不动
        assert_eq!(delete_at("ab", 0), ("b".to_string(), 0));
        assert_eq!(delete_at("ab", 2), ("ab".to_string(), 2));
    }

    /// 光标移动：Left/Right 遇选区跳到选区头尾，Home/End 在行内，Up/Down 保持列。
    #[test]
    fn caret_moves_like_the_original() {
        let text = "ab\ncdef\ngh";
        // 选区 2..6（"b\ncde"）：左到其头、右到其尾
        assert_eq!(move_caret(text, 2, 6, CaretDir::Left, false), 2);
        assert_eq!(move_caret(text, 2, 6, CaretDir::Right, false), 6);
        // Shift 时逐字符走
        assert_eq!(move_caret(text, 2, 6, CaretDir::Right, true), 3);
        assert_eq!(move_caret(text, 0, 0, CaretDir::Left, false), 0);
        assert_eq!(move_caret(text, 10, 10, CaretDir::Right, false), 10);
        // Home/End：第 2 行 "cdef" 是 3..7
        assert_eq!(move_caret(text, 5, 5, CaretDir::Home, false), 3);
        assert_eq!(move_caret(text, 5, 5, CaretDir::End, false), 7);
        // Up/Down：从第 2 行第 2 列 (4) 往下到第 3 行第 2 列 = 9；短行收在行尾
        assert_eq!(move_caret(text, 4, 4, CaretDir::Down, false), 9);
        assert_eq!(move_caret(text, 8, 8, CaretDir::Up, false), 3);
        // 行尾的换行处往下：列数收在下一行行尾
        assert_eq!(move_caret(text, 7, 7, CaretDir::Down, false), 10);
        assert_eq!(move_caret(text, 2, 2, CaretDir::Down, false), 5);
        // 第一行往上、最后一行往下都不动
        assert_eq!(move_caret(text, 1, 1, CaretDir::Up, false), 1);
        assert_eq!(move_caret(text, 9, 9, CaretDir::Down, false), 9);
    }

    /// restyle：字号翻倍轴翻倍、原点不动；只改行距轴不动；换单位让字母变大而 size 数字不变。
    #[test]
    fn restyle_scales_axes_and_keeps_the_start() {
        let tx = TextSettings {
            size: 2.0,
            cap: 0.5,
            k: 1.0,
            unit: TextUnit::Font,
            ..Default::default()
        };
        let axes: Axes = ([4.0, 60.0], [2.0, 0.0], [0.0, 2.0]);

        let (new, a) = text::restyle(
            &tx,
            axes,
            &TextChange {
                size: Some(4.0),
                ..Default::default()
            },
            0.5,
            0.5,
        );
        assert_eq!(new.size, 4.0);
        assert_eq!(a.0, axes.0, "第一行起点不动");
        assert_eq!(a.2, [0.0, 4.0]);

        // 只改行距：size 框的数字保持（shown_size），轴不动
        let (new, a) = text::restyle(
            &tx,
            axes,
            &TextChange {
                leading: Some(150.0),
                ..Default::default()
            },
            0.5,
            0.5,
        );
        assert_eq!(new.size, 2.0);
        assert_eq!(a, axes);

        // Font -> Rows：数字 2 不变，但 em = size / cap = 4，轴放大一倍
        let (new, a) = text::restyle(
            &tx,
            axes,
            &TextChange {
                unit: Some(TextUnit::Rows),
                ..Default::default()
            },
            0.5,
            0.5,
        );
        assert_eq!(new.unit, TextUnit::Rows);
        assert_eq!(new.size, 2.0);
        assert!((a.2[1] - 4.0).abs() < 1e-9);
        assert_eq!(a.0, axes.0);

        // 改对齐不动轴
        let (new, a) = text::restyle(
            &tx,
            axes,
            &TextChange {
                align: Some(TextAlign::Center),
                ..Default::default()
            },
            0.5,
            0.5,
        );
        assert_eq!(new.align, TextAlign::Center);
        assert_eq!(a, axes);
    }

    /// 文本框命中（原版 inside_box）：斜框里外与退化的框。
    #[test]
    fn text_box_hit_test() {
        let v = crate::roll::View::default();
        // 框：beat 0..1、pitch 60..61 的斜平行四边形
        let c: [Pt; 4] = [
            [0.0, 60.0],
            [1.0, 60.0],
            [1.0 + 0.25, 60.0 + 1.0],
            [0.25, 61.0],
        ];
        // 屏幕坐标由 View 换算：kb_w=56, ruler_h=20, sx=60, sy=6, top=127.5
        let at = |b: f64, p: f64| (v.x_of(b), v.y_of(p));
        let (x, y) = at(0.5, 60.5);
        assert!(inside_box(&c, &v, x, y));
        let (x, y) = at(1.5, 60.5); // u 在框外
        assert!(!inside_box(&c, &v, x, y));
        // 退化的框（三点一线）不算命中
        let flat: [Pt; 4] = [[0.0, 60.0], [1.0, 60.0], [2.0, 60.0], [1.0, 60.0]];
        let (x, y) = at(0.5, 60.0);
        assert!(!inside_box(&flat, &v, x, y));
    }
}
