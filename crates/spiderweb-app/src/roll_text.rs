//! Text tool (upstream roll/roll_text.py): click the roll to place text, type it in, caret
//! and selection.
//!
//! A text shape is a custom shape: `sh.text` holds the settings, `sh.strokes` the glyph
//! outlines, `sh.pts` the three frame points. Every edit re-lays out through
//! [`spiderweb_core::text::build`]; whitespace-only text has no shape, its settings and axes
//! stay in [`Typing`] until the next visible character grows it back. Keyboard events are
//! handed to [`text_keyboard`] first by `App::ui` (the typing priority of upstream `on_key`:
//! global shortcuts step aside while typing).

use std::time::Duration;

use eframe::egui;
use egui::{Color32, Pos2, Rect, Stroke};

use spiderweb_core::Pt;
use spiderweb_core::shape::{Kind, Shape, TextSettings};
use spiderweb_core::text::{self, Axes, TextChange};

use crate::app::{App, Tool};
use crate::roll::Drag;

/// Caret blink half-period in milliseconds (upstream BLINK_MS).
pub const BLINK_MS: u64 = 530;

/// The text being typed (upstream self.typing dict).
#[derive(Clone, Debug)]
pub struct Typing {
    /// Index of the shape being typed in; None while there is no visible character yet
    pub i: Option<usize>,
    /// Caret position (before which character)
    pub caret: usize,
    /// The other end of the selection (== caret means no selection)
    pub anchor: usize,
    /// Settings and axes while there is no shape yet
    pub tx: Option<TextSettings>,
    pub axes: Option<Axes>,
    /// Whether this typing already pushed an undo step for the shape
    pub undo: bool,
    /// What the text was when typing started (upstream ty["was"]), for the Erase text: name.
    pub was: String,
    /// The shapes snapshot of this typing's undo step, so its name can follow the text
    /// (upstream ty["step"]; None = no step of its own yet).
    pub step: Option<String>,
    /// This typing made the shape (Draw:), rather than editing an existing one (upstream ty["new"]).
    pub new: bool,
}

// ---------------------------------------------------------------- pure logic (unit-testable)

/// Inserts s into the character range s0..s1 and returns the new text with the new caret position (the string part of upstream insert_text).
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

/// Backspace: deletes the character before the caret (the BackSpace branch of upstream type_key). Returns the new text and caret.
pub fn backspace(text: &str, caret: usize) -> (String, usize) {
    insert_str(text, caret.saturating_sub(1), caret, "")
}

/// Delete: deletes the character at the caret (the Delete branch of upstream type_key). Returns the new text and caret.
pub fn delete_at(text: &str, caret: usize) -> (String, usize) {
    insert_str(text, caret, caret.saturating_add(1), "")
}

/// Caret movement direction (Left / Right / Home / End / Up / Down of upstream type_key).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaretDir {
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
}

/// Index of the start of line `line` in the character array (upstream rfind("\n") + 1).
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

/// Index of the end of the line starting at start (excluding the newline).
fn line_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&c| c == '\n')
        .map(|p| p + start)
        .unwrap_or(chars.len())
}

/// Moves the caret (the caret branch of upstream type_key): `anchor` is the other end of the selection, `shift` = extend it.
pub fn move_caret(text: &str, caret: usize, anchor: usize, dir: CaretDir, shift: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let caret = caret.min(n);
    let (lo, hi) = (caret.min(anchor), caret.max(anchor));
    match dir {
        CaretDir::Left | CaretDir::Right => {
            if lo != hi && !shift {
                // There is a selection: left goes to its head, right to its tail
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

// ---------------------------------------------------------------- hit testing and coordinates

/// The text box's four corners (upstream custom_corners): u0v0, u1v0, u1v1, u0v1, beat / pitch.
fn corners(sh: &Shape) -> Option<[Pt; 4]> {
    if sh.pts.len() < 3 {
        return None;
    }
    let ([b0, p0], [b1, p1], [b2, p2]) = (sh.pts[0], sh.pts[1], sh.pts[2]);
    Some([[b0, p0], [b1, p1], [b1 + b2 - b0, p1 + p2 - p0], [b2, p2]])
}

/// Whether screen point (x, y) is inside this parallelogram box (upstream inside_box).
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

/// Text shape under a screen point (newest to oldest) (upstream text_at).
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

/// Screen position -> roll (beat, pitch) (the same clamping as roll::event_pt).
fn roll_pt(app: &App, pos: Pos2) -> Pt {
    let v = &app.view;
    let x = pos.x.clamp(v.kb_w, v.w);
    let y = pos.y.clamp(v.ruler_h, v.h);
    [v.b_of(x), v.p_of(y)]
}

/// The typing state as (settings, axes) (upstream typing_state).
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

/// Contents of the text being typed.
fn typing_text(app: &App) -> Option<String> {
    typing_state(app).map(|(tx, _)| tx.text)
}

/// The selection as (start, end); equal means no selection (upstream text_selection).
pub fn selection(app: &App) -> (usize, usize) {
    app.typing
        .as_ref()
        .map(|t| (t.anchor.min(t.caret), t.anchor.max(t.caret)))
        .unwrap_or((0, 0))
}

/// Caret position closest to the mouse (upstream caret_at): pick the line by row height first, then the closest x within the line.
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
    // Baseline y of each line (same for the whole line; later writes overwrite earlier ones)
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

// ---------------------------------------------------------------- click / drag / double-click

/// A click (upstream text_click): on the text being typed = move the caret / extend the
/// selection; otherwise end typing: on existing text = keep typing there, on empty space =
/// place new text.
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
                was: String::new(),
                step: None,
                new: false,
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
                was: String::new(),
                step: None,
                new: false,
            });
        }
    }
}

/// Text tool click (upstream text_click).
pub fn text_press(app: &mut App, pos: Pos2, pt: Pt) {
    press(app, pos, pt, false);
}

/// Text tool click; Shift = extend the selection from the original end.
pub fn text_press_shift(app: &mut App, pos: Pos2, pt: Pt, shift: bool) {
    press(app, pos, pt, shift);
}

/// Drag after a click: select from the press position to the mouse (upstream text_drag).
pub fn text_drag(app: &mut App, pos: Pos2) {
    if app.typing.is_none() {
        return;
    }
    let caret = caret_at(app, pos);
    if let Some(ty) = app.typing.as_mut() {
        ty.caret = caret;
    }
}

/// Double-click on text with the Select tool: switch to the text tool and keep typing (upstream edit_text).
pub fn text_edit(app: &mut App, pos: Pos2) {
    app.tool = Tool::Text;
    app.draw_tool = Tool::Text;
    let pt = crate::roll::event_pt(app, pos, true, false);
    text_press(app, pos, pt);
}

/// Text tool double-click: selects the word under the mouse (upstream text_double).
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

/// Ends typing (upstream end_typing).
pub fn end_typing(app: &mut App) {
    app.typing = None;
}

// ---------------------------------------------------------------- text editing

/// Writes the text into what is being typed (upstream set_text): with a shape, re-lay it out
/// in place; whitespace-only removes the shape; without one, the first visible character grows
/// it (one undo step per typing session).
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
            // The undo step is pushed on the first change of this typing (upstream ty["undo"])
            if !app.typing.as_ref().map(|t| t.undo).unwrap_or(false) {
                // the History name: what the text was (upstream ty["was"])
                let was = app
                    .shapes
                    .get(i)
                    .map(|sh| sh.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "?".to_string());
                app.push_undo(&rust_i18n::t!("roll_text.type", text = was.clone()));
                let step = app.undo_stack.last().map(|(state, _)| state.clone());
                if let Some(ty) = app.typing.as_mut() {
                    ty.undo = true;
                    ty.was = was;
                    ty.step = step;
                    ty.new = false;
                }
            }
            let Some(sh) = app.shapes.get_mut(i) else {
                return;
            };
            if text::build(sh, &tx, &font, axes) {
                app.shapes_changed();
            } else {
                // Nothing visible any more: fold the shape away and keep the typing state
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
                    // This typing already pushed an undo step: the shape may have been deleted and grown back, so splice it back into the list directly
                    app.shapes.push(sh);
                    app.select(Some(idx), false);
                    app.shapes_changed();
                } else {
                    app.add_shape(sh);
                    let step = app.undo_stack.last().map(|(state, _)| state.clone());
                    if let Some(ty) = app.typing.as_mut() {
                        ty.undo = true;
                        ty.step = step;
                        ty.new = true;
                    }
                }
            } else if let Some(ty) = app.typing.as_mut() {
                ty.tx = Some(tx);
                ty.axes = Some(axes);
            }
        }
    }
    name_typing_step(app);
}

/// The History name of this typing's undo step follows the text (upstream name_typing_step):
/// a new text is "Draw: ...", editing is "Type: ..." and erasing it all is "Erase text: ...".
fn name_typing_step(app: &mut App) {
    let Some(ty) = app.typing.as_ref() else {
        return;
    };
    let (Some(step), i, new, was) = (ty.step.clone(), ty.i, ty.new, ty.was.clone()) else {
        return;
    };
    let Some((state, _)) = app.undo_stack.last().cloned() else {
        return;
    };
    if state != step {
        return; // (another step came after it)
    }
    let (text_name, label) = match i.and_then(|i| app.shapes.get(i)) {
        Some(sh) => (
            if sh.name.is_empty() {
                "?".to_string()
            } else {
                sh.name.clone()
            },
            app.shape_label(sh),
        ),
        None if i.is_some() => return,
        None => (String::new(), String::new()),
    };
    // (a new text typed and erased again: the step changes nothing)
    let Some(name) = typing_step_name(i, new, &was, &text_name, label) else {
        return;
    };
    if let Some(top) = app.undo_stack.last_mut() {
        top.1 = name;
    }
}

/// The History name this typing's step should have: "Draw: ..." for a text this typing made,
/// "Type: ..." when editing one, "Erase text: ..." when it was erased (upstream name_typing_step).
/// None = the step stays as it is.
pub fn typing_step_name(
    i: Option<usize>,
    new: bool,
    was: &str,
    text_name: &str,
    label: String,
) -> Option<String> {
    if i.is_some() {
        Some(if new {
            label
        } else {
            rust_i18n::t!("roll_text.type", text = text_name).to_string()
        })
    } else if new {
        None
    } else {
        Some(rust_i18n::t!("roll_text.erase", text = was).to_string())
    }
}

/// Inserts a run of characters at the caret, replacing the selection (upstream insert_text).
pub fn insert_text(app: &mut App, s: &str) {
    let Some(text) = typing_text(app) else {
        return;
    };
    let (s0, s1) = selection(app);
    let (new, caret) = insert_str(&text, s0, s1, s);
    set_text(app, &new, caret);
}

/// The typing state after the panel changed settings (upstream retype): settings and axes are replaced, the text re-lays out in place or grows anew.
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
            // The panel pushed its own undo step: the next keystroke starts a fresh one
            if let Some(ty) = app.typing.as_mut() {
                ty.undo = false;
            }
        }
    }
}

/// The panel changed a text setting (upstream set_text_setting): applies to the defaults, the
/// text being typed or the selected texts; the first line's start stays put (restyle scales
/// the axes), then notes are recomputed.
pub fn set_text_setting(app: &mut App, changes: &TextChange) {
    app.text_defaults = changes.apply(&app.text_defaults);
    if app.typing.is_some() {
        let Some((tx, axes)) = typing_state(app) else {
            return;
        };
        let old_cap = text::text_font(&tx).cap;
        let new_cap = text::text_font(&changes.apply(&tx)).cap;
        if app.typing.as_ref().and_then(|t| t.i).is_some() {
            app.push_undo(&rust_i18n::t!("panel.text.text_setting"));
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
            app.push_undo(&rust_i18n::t!("panel.text.text_setting"));
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

// ---------------------------------------------------------------- keyboard

/// Handles one input event; true = the text input consumed it (upstream type_key's "break").
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

/// Keyboard entry point while typing: events are consumed first when `typing` is active, global shortcuts step aside (the priority of upstream on_key).
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

/// One key press (upstream type_key): Ctrl combinations go to copy / undo etc., the rest are editing and caret movement.
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
            // Ctrl+V is handled by Event::Paste; other Ctrl+keys never trigger global shortcuts while typing
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

/// Backspace: deletes the selection if there is one, otherwise the character before the caret.
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

/// Delete: deletes the selection if there is one, otherwise the character at the caret.
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

/// Arrow keys: move the caret; with Shift the other end of the selection is kept.
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

/// Selects all of the text being typed (not all shapes).
fn select_all(app: &mut App) {
    let Some(text) = typing_text(app) else {
        return;
    };
    if let Some(ty) = app.typing.as_mut() {
        ty.anchor = 0;
        ty.caret = text.chars().count();
    }
}

/// Ctrl+C / Ctrl+X: with a selection, copy it (cut copies then deletes); without one, Ctrl+C copies the selected shapes (upstream behaviour).
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

// ---------------------------------------------------------------- painting

/// Paints the text being typed: dashed box, highlights of selected characters, blinking caret (upstream draw_typing).
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
    // em units -> screen
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
    // Dashed box (turns / skews with the text)
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
    // Selected characters
    let chars: Vec<char> = tx.text.chars().collect();
    let (s0, s1) = (ty.anchor.min(ty.caret), ty.anchor.max(ty.caret));
    let highlight = Color32::from_rgba_unmultiplied(0x3a, 0x7b, 0xd5, 0x70);
    for j in s0..s1 {
        let Some(&p) = spots.get(j) else {
            break;
        };
        let (x0, y) = (p[0], p[1]);
        let x1 = match (chars.get(j), spots.get(j + 1)) {
            // A selected newline: draw a small sliver
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
    // Caret (blinking)
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

    /// The History step names of typing (upstream roll_text.name_typing_step).
    #[test]
    fn typing_names_its_history_step() {
        // a text this typing made: Draw: <label>
        assert_eq!(
            typing_step_name(Some(0), true, "?", "hi", "Draw: Text: hi".to_string()).as_deref(),
            Some("Draw: Text: hi")
        );
        // editing an old text: Type: <what it was>
        assert_eq!(
            typing_step_name(Some(0), false, "old", "new", String::new()).as_deref(),
            Some("Type: new")
        );
        // erased again (not new): Erase text: <what it was>
        assert_eq!(
            typing_step_name(None, false, "old", "", String::new()).as_deref(),
            Some("Erase text: old")
        );
        // a new text typed and erased again: the step changes nothing
        assert_eq!(typing_step_name(None, true, "?", "", String::new()), None);
    }

    /// Typing edit: inserts at the caret (character indices, CJK included) and writes into Shape.text.
    #[test]
    fn typing_edits_shape_text() {
        let mut sh = Shape::new(Kind::Custom, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        sh.text = Some(TextSettings {
            text: "heo".to_string(),
            ..Default::default()
        });
        // Insert "ll" after the e: heo -> hello
        let text0 = sh.text.as_ref().map(|t| t.text.clone()).unwrap_or_default();
        let (text, caret) = insert_str(&text0, 2, 2, "ll");
        if let Some(tx) = sh.text.as_mut() {
            tx.text = text;
        }
        assert_eq!(sh.text.as_ref().map(|t| t.text.as_str()), Some("hello"));
        assert_eq!(caret, 4);
        // CJK counts by character, not byte: delete the "l", then insert a character before the CJK one
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

    /// Selection replacement: typed characters replace the selection, Backspace / Delete follow the caret.
    #[test]
    fn insert_replaces_selection_and_deletes() {
        // Selection 1..3 ("el") replaced with "Z"
        assert_eq!(insert_str("hello", 1, 3, "Z"), ("hZlo".to_string(), 2));
        // Backspace deletes before the caret; caret 0 stays put
        assert_eq!(backspace("ab", 1), ("b".to_string(), 0));
        assert_eq!(backspace("ab", 0), ("ab".to_string(), 0));
        // Delete deletes at the caret; the end stays put
        assert_eq!(delete_at("ab", 0), ("b".to_string(), 0));
        assert_eq!(delete_at("ab", 2), ("ab".to_string(), 2));
    }

    /// Caret movement: Left/Right jump to the selection ends when there is a selection, Home/End stay within the line, Up/Down keep the column.
    #[test]
    fn caret_moves_like_the_original() {
        let text = "ab\ncdef\ngh";
        // Selection 2..6 ("b\ncde"): left goes to its head, right to its tail
        assert_eq!(move_caret(text, 2, 6, CaretDir::Left, false), 2);
        assert_eq!(move_caret(text, 2, 6, CaretDir::Right, false), 6);
        // With Shift it moves character by character
        assert_eq!(move_caret(text, 2, 6, CaretDir::Right, true), 3);
        assert_eq!(move_caret(text, 0, 0, CaretDir::Left, false), 0);
        assert_eq!(move_caret(text, 10, 10, CaretDir::Right, false), 10);
        // Home/End: line 2 "cdef" is 3..7
        assert_eq!(move_caret(text, 5, 5, CaretDir::Home, false), 3);
        assert_eq!(move_caret(text, 5, 5, CaretDir::End, false), 7);
        // Up/Down: from line 2 column 2 (4) down to line 3 column 2 = 9; a short line clamps to its end
        assert_eq!(move_caret(text, 4, 4, CaretDir::Down, false), 9);
        assert_eq!(move_caret(text, 8, 8, CaretDir::Up, false), 3);
        // Down from the newline at a line end: the column clamps to the next line's end
        assert_eq!(move_caret(text, 7, 7, CaretDir::Down, false), 10);
        assert_eq!(move_caret(text, 2, 2, CaretDir::Down, false), 5);
        // Up from the first line and down from the last line don't move
        assert_eq!(move_caret(text, 1, 1, CaretDir::Up, false), 1);
        assert_eq!(move_caret(text, 9, 9, CaretDir::Down, false), 9);
    }

    /// restyle: doubling the size doubles the axes with the origin fixed; changing only leading leaves the axes alone; switching units enlarges the letters while the size number stays.
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
        assert_eq!(a.0, axes.0, "the first line's start stays put");
        assert_eq!(a.2, [0.0, 4.0]);

        // Only leading changed: the size box number stays (shown_size) and the axes don't move
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

        // Font -> Rows: the number 2 stays, but em = size / cap = 4, doubling the axes
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

        // Changing the align leaves the axes alone
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

    /// Text box hit test (upstream inside_box): inside and outside a skewed box, and a degenerate box.
    #[test]
    fn text_box_hit_test() {
        let v = crate::roll::View::default();
        // Box: a skewed parallelogram over beat 0..1, pitch 60..61
        let c: [Pt; 4] = [
            [0.0, 60.0],
            [1.0, 60.0],
            [1.0 + 0.25, 60.0 + 1.0],
            [0.25, 61.0],
        ];
        // Screen coordinates are converted by View: kb_w=56, ruler_h=20, sx=60, sy=6, top=127.5
        let at = |b: f64, p: f64| (v.x_of(b), v.y_of(p));
        let (x, y) = at(0.5, 60.5);
        assert!(inside_box(&c, &v, x, y));
        let (x, y) = at(1.5, 60.5); // u is outside the box
        assert!(!inside_box(&c, &v, x, y));
        // A degenerate box (three collinear points) is not a hit
        let flat: [Pt; 4] = [[0.0, 60.0], [1.0, 60.0], [2.0, 60.0], [1.0, 60.0]];
        let (x, y) = at(0.5, 60.0);
        assert!(!inside_box(&flat, &v, x, y));
    }
}
