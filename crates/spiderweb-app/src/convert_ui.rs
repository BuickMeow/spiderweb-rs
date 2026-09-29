//! "Turn into live shape" UI (upstream window/join_split.py's turn_into_live, roll_menu.py and
//! window/app.py wiring): the menu item, the Ctrl+L shortcut, the Custom panel button and the
//! warning dialog when something is lost. The maths is in spiderweb_core::convert.

use std::collections::BTreeSet;

use eframe::egui;

use spiderweb_core::convert as Cv;
use spiderweb_core::shape::{Kind, Shape};

use crate::app::App;

/// The warning dialog's pending shapes: the selection when "Turn into live shape" was asked for,
/// with the loss lines to show. Confirming keeps working on these, even if the selection moves on.
pub struct PendingTurnLive {
    pub order: Vec<usize>,
    pub olds: Vec<Shape>,
    pub lost: Vec<String>,
}

/// The kinds a live shape can be made of, as the message lists them (join_split.JOIN_KINDS).
fn join_kinds() -> String {
    rust_i18n::t!("join_split.lines_polylines_freehand_strokes_curves").to_string()
}

/// A single loss as the sentence the warning shows (convert.losses).
fn loss_text(loss: Cv::Loss) -> String {
    match loss {
        Cv::Loss::Tumours => {
            rust_i18n::t!("convert.tumours_become_fixed_points_they_can").to_string()
        }
        Cv::Loss::EndDot => rust_i18n::t!("convert.last_note_starts_on_it_is").to_string(),
    }
}

/// Why the selection can't be turned into a live shape (None = it can) (join_split.live_problem).
pub fn live_problem(app: &App) -> Option<String> {
    let sels: Vec<usize> = app
        .sels
        .iter()
        .copied()
        .filter(|&i| i < app.shapes.len())
        .collect();
    if sels.is_empty() {
        return Some(
            rust_i18n::t!(
                "join_split.select_the_shapes_to_turn_into",
                JOIN_KINDS = join_kinds()
            )
            .to_string(),
        );
    }
    let shapes: Vec<&Shape> = sels.iter().map(|&i| &app.shapes[i]).collect();
    let mut other: BTreeSet<String> = BTreeSet::new();
    for sh in &shapes {
        if !Cv::CAN_TURN.contains(&sh.kind) || sh.text.is_some() || sh.notes.is_some() {
            let label = app.shape_label(sh);
            let head = label.split(':').next().unwrap_or("").to_string();
            other.insert(head);
        }
    }
    if !other.is_empty() {
        let and = rust_i18n::t!("roll_funnel.and");
        let join = other
            .into_iter()
            .collect::<Vec<_>>()
            .join(&and)
            .to_lowercase();
        return Some(
            rust_i18n::t!(
                "join_split.only_and_custom_shapes_can_be",
                JOIN_KINDS = join_kinds(),
                join = join
            )
            .to_string(),
        );
    }
    if shapes.len() == 1 && shapes[0].kind == Kind::Custom {
        return Some(rust_i18n::t!("join_split.it_s_a_custom_shape_already").to_string());
    }
    None
}

/// The selected shapes -> one live shape (join_split.turn_into_live). With a selection that can't
/// be turned the status line says why; when something is lost the warning dialog asks first.
pub fn turn_into_live(app: &mut App) {
    if let Some(problem) = live_problem(app) {
        app.status = problem; // (the shortcut: say why)
        return;
    }
    let order: Vec<usize> = app.sels.iter().copied().collect();
    let olds: Vec<Shape> = order.iter().map(|&i| app.shapes[i].clone()).collect();
    let lost: Vec<String> = Cv::losses(&olds).into_iter().map(loss_text).collect();
    if lost.is_empty() {
        turn_live_now(app, &order, &olds);
    } else {
        app.pending_turn_live = Some(PendingTurnLive { order, olds, lost });
    }
}

/// The conversion itself, after the warning (if any): the selected shapes are replaced, where the
/// first of them was, by the new live shape.
fn turn_live_now(app: &mut App, order: &[usize], olds: &[Shape]) {
    let paths = Cv::paths_of(olds);
    let cd = crate::roll_live::core_custom_defaults(app);
    let new = Cv::to_live(olds, &paths, &app.defaults, &cd);
    app.cancel_draft();
    app.push_undo(&rust_i18n::t!("join_split.turn_into_live_shape"));
    for &i in order.iter().rev() {
        if i < app.shapes.len() {
            app.shapes.remove(i);
        }
    }
    let at = order.first().copied().unwrap_or(0).min(app.shapes.len());
    app.shapes.insert(at, new);
    app.select(Some(at), false);
    app.shapes_changed();
    let n = olds.len();
    app.status = if n == 1 {
        rust_i18n::t!("join_split.turned_the_shape_into_a_live").to_string()
    } else {
        rust_i18n::t!(
            "join_split.turned_shapes_into_a_live_shape",
            n = n.to_string()
        )
        .to_string()
    };
    app.tips.show_waiting("turn_live");
}

/// The Custom panel's "Turn into live shape" button (upstream: the button under the shape list).
/// Disabled with the reason in its tooltip while the selection can't be turned.
pub fn live_button_ui(app: &mut App, ui: &mut egui::Ui) {
    let problem = live_problem(app);
    let label = rust_i18n::t!("join_split.turn_into_live_shape").to_string();
    let resp = ui.add_enabled(problem.is_none(), egui::Button::new(label));
    let tip = rust_i18n::t!("join_split.turns_the_selected_shapes_into_one").to_string();
    let tip = match problem {
        Some(problem) => format!("{tip}\n\n{problem}"),
        None => tip,
    };
    if resp.on_hover_text(tip).clicked() {
        turn_into_live(app);
    }
}

/// The warning dialog ("Turning these into a live shape changes this: ..."), upstream's
/// messagebox.askokcancel with the warning icon.
pub fn dialog_ui(app: &mut App, ctx: &egui::Context) {
    let Some(pending) = app.pending_turn_live.take() else {
        return;
    };
    let mut go = false;
    let mut cancel = false;
    egui::Window::new(rust_i18n::t!("join_split.spiderweb").to_string())
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let text = format!(
                "{}{}{}",
                rust_i18n::t!("join_split.turning_these_into_a_live_shape"),
                pending.lost.join("\n• "),
                rust_i18n::t!("join_split.split_into_separate_shapes_or_ctrl")
            );
            ui.label(text);
            ui.horizontal(|ui| {
                if ui.button(rust_i18n::t!("common.yes").to_string()).clicked() {
                    go = true;
                }
                if ui.button(rust_i18n::t!("common.no").to_string()).clicked() {
                    cancel = true;
                }
            });
        });
    if go {
        turn_live_now(app, &pending.order, &pending.olds);
    } else if !cancel {
        app.pending_turn_live = Some(pending);
    }
}
