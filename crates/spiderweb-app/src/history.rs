//! The History panel (upstream window/history.py): every undo step by name, oldest at the top;
//! click one to go back (or forward) to it. The toolbar's History box shows / hides it (off on a
//! fresh start). It sits in the side panel between Project and Shapes; Undock puts it in a window
//! of its own (Dock, or closing that window, puts it back). The steps themselves are
//! App.undo_stack / redo_stack: (shapes as JSON, name) pairs.

use eframe::egui;

use crate::app::App;

/// An undo step: the shapes as JSON, then what the step does (upstream `(before, name)`).
pub type Step = (String, String);

/// Steps kept; older ones are dropped (upstream `del undo_stack[:-300]`).
pub const LIMIT: usize = 300;

/// The undone steps are greyed out (upstream `FUTURE`).
const FUTURE: egui::Color32 = egui::Color32::from_gray(160);

/// Push a named step; a new change drops the steps undone so far (upstream `push_undo`).
pub fn push(undo: &mut Vec<Step>, redo: &mut Vec<Step>, state: String, name: &str) {
    undo.push((state, name.to_string()));
    if undo.len() > LIMIT {
        let cut = undo.len() - LIMIT;
        undo.drain(..cut);
    }
    redo.clear();
}

/// One step from `src` to `dst`: `dst` remembers `current` under the same name (upstream `_restore`).
pub fn take(src: &mut Vec<Step>, dst: &mut Vec<Step>, current: String) -> Option<Step> {
    let (state, name) = src.pop()?;
    dst.push((current, name.clone()));
    Some((state, name))
}

/// The list rows and the current one: Start, the steps done, then the ones undone
/// (upstream `history_rows`).
pub fn rows(undo: &[Step], redo: &[Step]) -> (Vec<String>, usize) {
    let mut names = vec![rust_i18n::t!("history.start").to_string()];
    names.extend(undo.iter().map(|(_, name)| name.clone()));
    names.extend(redo.iter().rev().map(|(_, name)| name.clone()));
    (names, undo.len())
}

/// What a jump reads and writes: the shapes themselves (the App, or a fake in tests).
pub trait HistoryHost {
    /// The current shapes as JSON.
    fn snapshot(&mut self) -> String;
    /// Show one step's shapes.
    fn apply(&mut self, state: &str);
}

/// Undo / redo until `row` is the current step (upstream `history_jump`); the steps on the way
/// keep their names in the other stack.
pub fn jump(undo: &mut Vec<Step>, redo: &mut Vec<Step>, row: usize, host: &mut impl HistoryHost) {
    while undo.len() > row && !undo.is_empty() {
        let current = host.snapshot();
        let Some((state, _)) = take(undo, redo, current) else {
            break;
        };
        host.apply(&state);
    }
    while undo.len() < row && !redo.is_empty() {
        let current = host.snapshot();
        let Some((state, _)) = take(redo, undo, current) else {
            break;
        };
        host.apply(&state);
    }
}

// ------------------------------------------------------------ the panel

/// The side panel's History section (between Project and Shapes); hidden while undocked
/// (upstream `toggle_history`'s docked branch).
pub fn history_section(app: &mut App, ui: &mut egui::Ui) {
    if !app.show_history || app.history_undocked {
        return;
    }
    egui::CollapsingHeader::new(rust_i18n::t!("history.history"))
        .default_open(true)
        .show(ui, |ui| body(app, ui, false));
}

/// The History window while it is undocked; closing it docks it again
/// (upstream `_history_window` + `WM_DELETE_WINDOW`).
pub fn history_window_ui(app: &mut App, ctx: &egui::Context) {
    if !app.show_history || !app.history_undocked {
        return;
    }
    let mut open = true;
    egui::Window::new(rust_i18n::t!("history.history"))
        .open(&mut open)
        .collapsible(false)
        .default_size([260.0, 360.0])
        .show(ctx, |ui| body(app, ui, true));
    if !open {
        app.history_undocked = false; // closing the window docks it again
    }
}

/// The list and its Undock / Dock button (upstream `_history_list`).
fn body(app: &mut App, ui: &mut egui::Ui, undocked: bool) {
    let (names, now) = rows(&app.undo_stack, &app.redo_stack);
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt(if undocked {
            "history_list_window"
        } else {
            "history_list_docked"
        })
        .max_height(if undocked { 300.0 } else { 110.0 })
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, name) in names.iter().enumerate() {
                let text = if i > now {
                    egui::RichText::new(name).color(FUTURE)
                } else {
                    egui::RichText::new(name)
                };
                if ui.selectable_label(i == now, text).clicked() {
                    clicked = Some(i);
                }
            }
        });
    ui.horizontal(|ui| {
        ui.weak(rust_i18n::t!("history.click_a_step_to_go_back"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if undocked {
                rust_i18n::t!("history.dock")
            } else {
                rust_i18n::t!("history.undock")
            };
            if ui
                .button(label)
                .on_hover_text(rust_i18n::t!("history.dock_tip"))
                .clicked()
            {
                app.history_undocked = !undocked;
            }
        });
    });
    if let Some(row) = clicked {
        app.history_jump(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(state: &str, name: &str) -> Step {
        (state.to_string(), name.to_string())
    }

    /// The shapes as a single string, standing in for the App in jump tests.
    struct Fake {
        state: String,
    }

    impl HistoryHost for Fake {
        fn snapshot(&mut self) -> String {
            self.state.clone()
        }
        fn apply(&mut self, state: &str) {
            self.state = state.to_string();
        }
    }

    #[test]
    fn push_names_steps_and_new_edit_clears_redo() {
        let mut undo = Vec::new();
        let mut redo = vec![step("old", "Delete")];
        push(&mut undo, &mut redo, "s0".into(), "Draw: Line");
        push(&mut undo, &mut redo, "s1".into(), "Delete");
        assert_eq!(undo, vec![step("s0", "Draw: Line"), step("s1", "Delete")]);
        assert!(redo.is_empty(), "a new edit drops the steps undone so far");
    }

    #[test]
    fn push_keeps_the_last_steps_only() {
        let mut undo = Vec::new();
        let mut redo = Vec::new();
        for i in 0..LIMIT + 5 {
            push(&mut undo, &mut redo, format!("s{i}"), "Draw: Line");
        }
        assert_eq!(undo.len(), LIMIT);
        assert_eq!(undo[0].0, "s5");
        assert_eq!(undo[LIMIT - 1].0, format!("s{}", LIMIT + 4));
    }

    #[test]
    fn rows_show_start_done_then_undone() {
        let undo = vec![step("s0", "Draw: Line")];
        let redo = vec![step("s1", "Delete"), step("s2", "Move")];
        let (names, now) = rows(&undo, &redo);
        let mut expected = vec![rust_i18n::t!("history.start").to_string()];
        expected.extend(["Draw: Line", "Move", "Delete"].map(str::to_string));
        assert_eq!(names, expected);
        assert_eq!(now, 1);
    }

    #[test]
    fn jump_walks_back_and_forward_with_names() {
        let mut undo = vec![step("s0", "Draw: Line"), step("s1", "Delete")];
        let mut redo = Vec::new();
        let mut fake = Fake { state: "s2".into() };
        jump(&mut undo, &mut redo, 0, &mut fake);
        assert_eq!(fake.state, "s0");
        assert!(undo.is_empty());
        assert_eq!(redo, vec![step("s2", "Delete"), step("s1", "Draw: Line")]);
        jump(&mut undo, &mut redo, 2, &mut fake);
        assert_eq!(fake.state, "s2");
        assert_eq!(undo, vec![step("s0", "Draw: Line"), step("s1", "Delete")]);
        assert!(redo.is_empty());
    }
}
