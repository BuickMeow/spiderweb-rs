//! Context menu (upstream roll/roll_menu.py): right-clicking a shape opens its edit menu;
//! double right-click switches tools.
//!
//! Menu entries are computed first as [`MenuEntry`] (pure logic, unit-testable), the command
//! clicked in the popup is recorded as a [`MenuAction`], and App is only modified after the
//! popup closes, avoiding simultaneous borrows inside the menu closure.

use eframe::egui;
use egui::Pos2;

use spiderweb_core::shape::{Kind, Stroke};

use crate::app::App;

/// Things the menu can do (upstream menu items; items not yet ported show as disabled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    AddPolyPoint,
    AddCurveAnchor,
    EditText,
    DeleteStroke,
    SaveToLibrary,
    Tumours,
    Join,
    SplitHere,
    TurnIntoLive,
    SplitPieces,
    Delete,
    Duplicate,
    Copy,
    Paste,
    FlipSideways,
    FlipUpsideDown,
    TurnLeft,
    TurnRight,
}

/// A menu entry: a separator or a clickable command.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Separator,
    Item {
        label: String,
        enabled: bool,
        action: MenuAction,
    },
}

/// Open menu (the live state of upstream show_menu): hit shape, right-click position (roll-local coordinates) and Shift.
#[derive(Clone, Debug)]
pub struct MenuState {
    pub target: usize,
    pub pos: Pos2,
    pub shift: bool,
    /// Just opened: open the Popup in egui on the next frame
    pub pending: bool,
}

/// What menu entries depend on (extracted from App for easy unit testing).
#[derive(Clone, Debug)]
pub struct MenuTarget {
    pub kind: Kind,
    pub has_text: bool,
    pub has_notes: bool,
    /// Number of selected shapes (wording of Delete shape / N shapes)
    pub count: usize,
    pub clipboard: bool,
    /// The picked stroke (custom shapes)
    pub picked_stroke: Option<usize>,
    /// The picked stroke is a curve (an anchor can be added)
    pub picked_curve: Option<usize>,
    /// The hit shape is a line kind (it can be cut in two here)
    pub line_kind: bool,
    /// The selection can carry tumours (the tumour window opens with "Tumours…")
    pub tumour: bool,
    /// The selected shapes can be joined
    pub can_join: bool,
    /// The selection can be turned into a live shape (no problem at all).
    pub live: bool,
    /// The hit shape can be split into separate shapes
    pub can_split: bool,
    /// The hit shape can go back to the shapes it was made of ("Split back into the old shapes")
    pub split_back: bool,
}

fn item(label: impl Into<String>, action: MenuAction, enabled: bool) -> MenuEntry {
    MenuEntry::Item {
        label: label.into(),
        enabled,
        action,
    }
}

/// What the menu looks like when shape i is hit (upstream show_menu's items, same order).
pub fn menu_entries(app: &App, i: usize) -> Vec<MenuEntry> {
    let Some(sh) = app.shapes.get(i) else {
        return Vec::new();
    };
    let picked = crate::roll_live::picked_stroke(app, sh);
    let picked_curve = picked.filter(|&k| matches!(sh.strokes.get(k), Some(Stroke::Curve { .. })));
    menu_entries_for(&MenuTarget {
        kind: sh.kind,
        has_text: sh.text.is_some(),
        has_notes: sh.notes.is_some(),
        count: app.sels.len(),
        clipboard: !app.clipboard.is_empty(),
        picked_stroke: picked,
        picked_curve,
        line_kind: spiderweb_core::joined::LINE_KINDS.contains(&sh.kind),
        tumour: !app.tumour_targets().is_empty(),
        can_join: app.can_join(),
        live: crate::convert_ui::live_problem(app).is_none(),
        can_split: app.can_split_pieces(sh),
        split_back: sh.kind == Kind::Custom && spiderweb_core::convert::originals(sh).is_some(),
    })
}

/// Pure-logic version of [`menu_entries`].
pub fn menu_entries_for(t: &MenuTarget) -> Vec<MenuEntry> {
    let one = t.count == 1;
    let mut out: Vec<MenuEntry> = Vec::new();
    if one && t.kind == Kind::Poly {
        out.push(item(
            rust_i18n::t!("menu.add_point"),
            MenuAction::AddPolyPoint,
            true,
        ));
    }
    if one && t.kind == Kind::Curve {
        out.push(item(
            rust_i18n::t!("menu.add_anchor"),
            MenuAction::AddCurveAnchor,
            true,
        ));
    }
    if one && t.has_text {
        out.push(item(
            rust_i18n::t!("menu.edit_text"),
            MenuAction::EditText,
            true,
        ));
    }
    if one && t.kind == Kind::Custom && !t.has_notes && !t.has_text {
        if t.picked_curve.is_some() {
            out.push(item(
                rust_i18n::t!("menu.add_anchor"),
                MenuAction::AddCurveAnchor,
                true,
            ));
        }
        if t.picked_stroke.is_some() {
            out.push(item(
                rust_i18n::t!("menu.delete_stroke"),
                MenuAction::DeleteStroke,
                true,
            ));
        }
        out.push(item(
            rust_i18n::t!("menu.save_library"),
            MenuAction::SaveToLibrary,
            false,
        ));
    }
    if t.tumour {
        out.push(item(
            rust_i18n::t!("menu.tumours"),
            MenuAction::Tumours,
            true,
        ));
    }
    if t.count >= 2 {
        out.push(item(
            if t.can_join {
                rust_i18n::t!("menu.join_shapes")
            } else {
                rust_i18n::t!("menu.join_shapes_only")
            },
            MenuAction::Join,
            t.can_join,
        ));
    }
    if one && t.line_kind {
        out.push(item(
            rust_i18n::t!("menu.split_here"),
            MenuAction::SplitHere,
            true,
        ));
    }
    // "Turn into live shape" (convert.py)
    if t.live {
        out.push(item(
            rust_i18n::t!("menu.turn_into_live_shape"),
            MenuAction::TurnIntoLive,
            true,
        ));
    }
    if one && t.can_split {
        out.push(item(
            if t.split_back {
                rust_i18n::t!("menu.split_back")
            } else {
                rust_i18n::t!("menu.split_separate")
            },
            MenuAction::SplitPieces,
            true,
        ));
    }
    if !out.is_empty() {
        out.push(MenuEntry::Separator);
    }
    let shapes = if t.count == 1 {
        rust_i18n::t!("menu.shape").to_string()
    } else {
        rust_i18n::t!("menu.n_shapes", n = t.count.to_string()).to_string()
    };
    out.push(item(
        rust_i18n::t!("menu.delete", shapes = shapes.clone()),
        MenuAction::Delete,
        true,
    ));
    out.push(item(
        rust_i18n::t!("menu.duplicate", shapes = shapes.clone()),
        MenuAction::Duplicate,
        true,
    ));
    out.push(item(
        rust_i18n::t!("menu.copy", shapes = shapes),
        MenuAction::Copy,
        true,
    ));
    out.push(item(
        rust_i18n::t!("menu.paste"),
        MenuAction::Paste,
        t.clipboard,
    ));
    out.push(MenuEntry::Separator);
    out.push(item(
        rust_i18n::t!("menu.flip_sideways"),
        MenuAction::FlipSideways,
        true,
    ));
    out.push(item(
        rust_i18n::t!("menu.flip_upside_down"),
        MenuAction::FlipUpsideDown,
        true,
    ));
    out.push(item(
        rust_i18n::t!("menu.turn_left"),
        MenuAction::TurnLeft,
        true,
    ));
    out.push(item(
        rust_i18n::t!("menu.turn_right"),
        MenuAction::TurnRight,
        true,
    ));
    out
}

/// Right-click hit on shape i: select it (if not selected), pick the stroke under the mouse for custom shapes, then open the menu.
pub fn open_menu(app: &mut App, i: usize, pos: Pos2, shift: bool) {
    if app.shapes.get(i).is_none() {
        return;
    }
    if !app.sels.contains(&i) {
        app.select(Some(i), false);
    }
    if let Some(sh) = app.shapes.get(i).cloned()
        && sh.kind == Kind::Custom
        && sh.text.is_none()
        && let Some(k) = crate::roll_live::stroke_at(app, &sh, pos, 6.0)
    {
        app.set_stroke(Some(k));
    }
    app.shape_menu = Some(MenuState {
        target: i,
        pos,
        shift,
        pending: true,
    });
}

/// A popup (context menu / dropdown) is open: the roll's input steps aside (upstream tk menus grab events).
pub fn is_popup_open(ctx: &egui::Context) -> bool {
    egui::Popup::is_any_open(ctx)
}

/// Draws the menu each frame (after the roll is painted). Clicks outside the menu / Esc close it via egui's Popup.
pub fn menu_ui(app: &mut App, ui: &egui::Ui) {
    let Some(state) = app.shape_menu.clone() else {
        return;
    };
    let ctx = ui.ctx().clone();
    let id = egui::Id::new(("shape_menu", state.target));
    if state.pending {
        egui::Popup::open_id(&ctx, id);
        if let Some(s) = app.shape_menu.as_mut() {
            s.pending = false;
        }
    }
    if !egui::Popup::is_id_open(&ctx, id) {
        app.shape_menu = None;
        return;
    }
    let entries = menu_entries(app, state.target);
    // The right-click position is roll-local: convert to screen coordinates to anchor the Popup
    let at = state.pos + ui.max_rect().min.to_vec2();
    let mut action: Option<MenuAction> = None;
    let layer = egui::LayerId::new(egui::Order::Foreground, id);
    egui::Popup::new(id, ctx, egui::PopupAnchor::Position(at), layer)
        .kind(egui::PopupKind::Menu)
        .open_memory(None)
        .show(|ui| {
            ui.set_min_width(180.0);
            for e in &entries {
                match e {
                    MenuEntry::Separator => {
                        ui.separator();
                    }
                    MenuEntry::Item {
                        label,
                        enabled,
                        action: a,
                    } => {
                        if ui.add_enabled(*enabled, egui::Button::new(label)).clicked() {
                            action = Some(*a);
                        }
                    }
                }
            }
        });
    if let Some(a) = action {
        app.shape_menu = None;
        apply_action(app, a, state.target, state.pos, state.shift);
    }
}

fn apply_action(app: &mut App, action: MenuAction, i: usize, pos: Pos2, shift: bool) {
    match action {
        MenuAction::AddPolyPoint => add_poly_point(app, i, pos, shift),
        MenuAction::AddCurveAnchor => add_anchor(app, i, pos, shift),
        MenuAction::EditText => crate::roll_text::text_edit(app, pos),
        MenuAction::DeleteStroke => {
            if let Some(k) = app.stroke {
                crate::roll_live::delete_stroke(app, i, k);
            }
        }
        MenuAction::SaveToLibrary => {}
        MenuAction::Tumours => crate::tumour_window::open_tumour_window(app),
        MenuAction::Join => app.join_selected(),
        MenuAction::SplitHere => app.split_here(i, pos),
        MenuAction::TurnIntoLive => crate::convert_ui::turn_into_live(app),
        MenuAction::SplitPieces => app.split_pieces_shape(i),
        MenuAction::Delete => app.delete_selected(),
        MenuAction::Duplicate => app.duplicate(),
        MenuAction::Copy => app.copy_selected(),
        MenuAction::Paste => app.paste(),
        MenuAction::FlipSideways => app.flip(true),
        MenuAction::FlipUpsideDown => app.flip(false),
        MenuAction::TurnLeft => app.rotate(false),
        MenuAction::TurnRight => app.rotate(true),
    }
}

/// "Add anchor here": for curve shapes add it on the curve; for custom shapes add it on the picked curve stroke.
fn add_anchor(app: &mut App, i: usize, pos: Pos2, shift: bool) {
    let kind = app.shapes.get(i).map(|sh| sh.kind);
    let pt = crate::roll::event_pt(app, pos, true, shift);
    let near = Some((12.0 * app.scale()) as f64);
    match kind {
        Some(Kind::Curve) => {
            app.curve_click(pos, pt, near);
        }
        Some(Kind::Custom) => {
            crate::roll_live::stroke_click(app, pos, near, shift);
        }
        _ => {}
    }
}

/// "Add point here": adds a snapped point to the polyline at the spot that stretches it least (upstream insert_poly_point).
fn add_poly_point(app: &mut App, i: usize, pos: Pos2, shift: bool) {
    let pt = crate::roll::event_pt(app, pos, true, shift);
    let view = &app.view;
    let Some(sh) = app.shapes.get(i) else {
        return;
    };
    if sh.pts.contains(&pt) {
        return;
    }
    let screen: Vec<[f32; 2]> = sh
        .pts
        .iter()
        .map(|p| [view.x_of(p[0]), view.y_of(p[1])])
        .collect();
    let q = [view.x_of(pt[0]), view.y_of(pt[1])];
    let k = poly_insert_index(&screen, q);
    app.push_undo(&rust_i18n::t!("pianoroll.add_a_point"));
    if let Some(sh) = app.shapes.get_mut(i) {
        sh.pts.insert(k, pt);
    }
    app.shapes_changed();
}

/// Extra screen length caused by inserting q into the polyline (upstream extra_length).
fn poly_extra(screen: &[[f32; 2]], i: usize, q: [f32; 2]) -> f64 {
    if i == 0 {
        return screen.first().map(|a| seg_len(*a, q)).unwrap_or(0.0);
    }
    if i >= screen.len() {
        return screen.last().map(|a| seg_len(*a, q)).unwrap_or(0.0);
    }
    let (a, b) = (screen[i - 1], screen[i]);
    seg_len(a, q) + seg_len(q, b) - seg_len(a, b)
}

/// Position that keeps the polyline shortest after adding the point (screen coordinates; upstream insert_poly_point takes the minimum extra_length).
pub fn poly_insert_index(screen: &[[f32; 2]], q: [f32; 2]) -> usize {
    (0..=screen.len())
        .min_by(|&a, &b| poly_extra(screen, a, q).total_cmp(&poly_extra(screen, b, q)))
        .unwrap_or(0)
}

fn seg_len(a: [f32; 2], b: [f32; 2]) -> f64 {
    (((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)) as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(kind: Kind) -> MenuTarget {
        MenuTarget {
            kind,
            has_text: false,
            has_notes: false,
            count: 1,
            clipboard: false,
            picked_stroke: None,
            picked_curve: None,
            line_kind: spiderweb_core::joined::LINE_KINDS.contains(&kind),
            tumour: spiderweb_core::joined::LINE_KINDS.contains(&kind),
            can_join: false,
            live: false,
            can_split: false,
            split_back: false,
        }
    }

    fn find(entries: &[MenuEntry], action: MenuAction) -> Option<&MenuEntry> {
        entries
            .iter()
            .find(|e| matches!(e, MenuEntry::Item { action: a, .. } if *a == action))
    }

    fn enabled(entries: &[MenuEntry], action: MenuAction) -> bool {
        matches!(
            find(entries, action),
            Some(MenuEntry::Item { enabled: true, .. })
        )
    }

    #[test]
    fn plain_line_has_the_general_items() {
        let got = menu_entries_for(&target(Kind::Line));
        for a in [
            MenuAction::Delete,
            MenuAction::Duplicate,
            MenuAction::Copy,
            MenuAction::FlipSideways,
            MenuAction::FlipUpsideDown,
            MenuAction::TurnLeft,
            MenuAction::TurnRight,
        ] {
            assert!(enabled(&got, a), "missing enabled item: {a:?}");
        }
        // Clipboard empty: the paste item is present but disabled
        assert!(find(&got, MenuAction::Paste).is_some());
        assert!(!enabled(&got, MenuAction::Paste));
        // A line has no add point / add anchor / delete stroke
        assert!(find(&got, MenuAction::AddPolyPoint).is_none());
        assert!(find(&got, MenuAction::AddCurveAnchor).is_none());
    }

    #[test]
    fn clipboard_and_single_selection_control_items() {
        let mut t = target(Kind::Line);
        t.clipboard = true;
        assert!(enabled(&menu_entries_for(&t), MenuAction::Paste));
        t.count = 3;
        let got = menu_entries_for(&t);
        assert!(matches!(
            find(&got, MenuAction::Delete),
            Some(MenuEntry::Item { label, .. }) if label == "Delete 3 shapes"
        ));
        assert!(find(&got, MenuAction::AddPolyPoint).is_none());
    }

    #[test]
    fn poly_curve_and_text_get_their_own_items() {
        let poly = menu_entries_for(&target(Kind::Poly));
        assert!(enabled(&poly, MenuAction::AddPolyPoint));
        let curve = menu_entries_for(&target(Kind::Curve));
        assert!(enabled(&curve, MenuAction::AddCurveAnchor));
        let mut text = target(Kind::Custom);
        text.has_text = true;
        assert!(enabled(&menu_entries_for(&text), MenuAction::EditText));
    }

    #[test]
    fn live_item_shows_only_when_the_selection_can_be_turned() {
        let mut t = target(Kind::Line);
        assert!(find(&menu_entries_for(&t), MenuAction::TurnIntoLive).is_none());
        t.live = true;
        assert!(enabled(&menu_entries_for(&t), MenuAction::TurnIntoLive));
    }

    #[test]
    fn custom_with_picked_curve_can_add_anchor_and_delete_stroke() {
        let mut t = target(Kind::Custom);
        t.picked_stroke = Some(1);
        t.picked_curve = Some(1);
        let got = menu_entries_for(&t);
        assert!(enabled(&got, MenuAction::AddCurveAnchor));
        assert!(enabled(&got, MenuAction::DeleteStroke));
        // Shape library not ported: the item is present but disabled
        assert!(find(&got, MenuAction::SaveToLibrary).is_some());
        assert!(!enabled(&got, MenuAction::SaveToLibrary));
        // A plain polyline was picked: no anchor can be added, but it can be deleted
        t.picked_curve = None;
        let got = menu_entries_for(&t);
        assert!(find(&got, MenuAction::AddCurveAnchor).is_none());
        assert!(enabled(&got, MenuAction::DeleteStroke));
    }

    #[test]
    fn join_and_split_items_follow_the_selection() {
        let t = target(Kind::Line);
        assert!(find(&menu_entries_for(&t), MenuAction::SplitHere).is_some());
        let mut t = target(Kind::Line);
        t.count = 2;
        t.can_join = true;
        assert!(enabled(&menu_entries_for(&t), MenuAction::Join));
        // a shape that can't be joined: the item is there but greyed out
        t.can_join = false;
        assert!(find(&menu_entries_for(&t), MenuAction::Join).is_some());
        assert!(!enabled(&menu_entries_for(&t), MenuAction::Join));
        // a custom shape isn't a line kind: no Split here
        let custom = target(Kind::Custom);
        assert!(find(&menu_entries_for(&custom), MenuAction::SplitHere).is_none());
        // a joined curve with more than one piece: Split into separate shapes
        let mut curve = target(Kind::Curve);
        curve.can_split = true;
        assert!(enabled(&menu_entries_for(&curve), MenuAction::SplitPieces));
        assert!(matches!(
            find(&menu_entries_for(&curve), MenuAction::SplitPieces),
            Some(MenuEntry::Item { label, .. }) if label == "Split into separate shapes"
        ));
        // a live shape that still matches its "from": Split back into the old shapes
        let mut live = target(Kind::Custom);
        live.can_split = true;
        live.split_back = true;
        assert!(matches!(
            find(&menu_entries_for(&live), MenuAction::SplitPieces),
            Some(MenuEntry::Item { label, .. }) if label == "Split back into the old shapes"
        ));
    }

    #[test]
    fn poly_point_goes_where_it_stretches_least() {
        // A row of points (0,0)-(10,0)-(20,0): the point (15,1) is closest between the last two points (index 2)
        let screen = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
        assert_eq!(poly_insert_index(&screen, [15.0, 1.0]), 2);
        // The point is left of the start: insert at the very front
        assert_eq!(poly_insert_index(&screen, [-5.0, 0.0]), 0);
        // The point is right of the end: insert at the very back
        assert_eq!(poly_insert_index(&screen, [30.0, 0.0]), 3);
        // Empty polyline: only 0
        assert_eq!(poly_insert_index(&[], [1.0, 1.0]), 0);
    }
}
