//! 右键菜单（原版 roll/roll_menu.py）：命中形状时弹出编辑菜单；双击右键切工具。
//!
//! 菜单项先算成 [`MenuEntry`]（纯逻辑、可单测），弹出层里点中的命令记成 [`MenuAction`]，
//! 等弹出层关掉后再改 App，避免在菜单闭包里同时借用。

use eframe::egui;
use egui::Pos2;

use spiderweb_core::shape::{Kind, Stroke};

use crate::app::App;

/// 菜单里能做的事（原版各菜单项；没移植的项以禁用显示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    AddPolyPoint,
    AddCurveAnchor,
    EditText,
    DeleteStroke,
    SaveToLibrary,
    Delete,
    Duplicate,
    Copy,
    Paste,
    FlipSideways,
    FlipUpsideDown,
    TurnLeft,
    TurnRight,
}

/// 一个菜单项：分隔线或可点的命令。
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Separator,
    Item {
        label: String,
        enabled: bool,
        action: MenuAction,
    },
}

/// 打开着的菜单（原版 show_menu 的现场）：命中的形状、右键位置（卷帘局部坐标）与 Shift。
#[derive(Clone, Debug)]
pub struct MenuState {
    pub target: usize,
    pub pos: Pos2,
    pub shift: bool,
    /// 刚打开：下一帧在 egui 里把 Popup 打开
    pub pending: bool,
}

/// 菜单项要看的东西（从 App 里摘出来，方便单测）。
#[derive(Clone, Debug)]
pub struct MenuTarget {
    pub kind: Kind,
    pub has_text: bool,
    pub has_notes: bool,
    /// 选中数量（Delete shape / N shapes 的措辞）
    pub count: usize,
    pub clipboard: bool,
    /// 被拾取的笔画（自定义形状）
    pub picked_stroke: Option<usize>,
    /// 被拾取的笔画正好是曲线（可以加锚点）
    pub picked_curve: Option<usize>,
}

fn item(label: impl Into<String>, action: MenuAction, enabled: bool) -> MenuEntry {
    MenuEntry::Item {
        label: label.into(),
        enabled,
        action,
    }
}

/// 命中形状 i 时菜单长什么样（原版 show_menu 的菜单项，顺序一致）。
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
    })
}

/// 纯逻辑版的 [`menu_entries`]。
pub fn menu_entries_for(t: &MenuTarget) -> Vec<MenuEntry> {
    let one = t.count == 1;
    let mut out: Vec<MenuEntry> = Vec::new();
    if one && t.kind == Kind::Poly {
        out.push(item("Add point here", MenuAction::AddPolyPoint, true));
    }
    if one && t.kind == Kind::Curve {
        out.push(item("Add anchor here", MenuAction::AddCurveAnchor, true));
    }
    if one && t.has_text {
        out.push(item("Edit text", MenuAction::EditText, true));
    }
    if one && t.kind == Kind::Custom && !t.has_notes && !t.has_text {
        if t.picked_curve.is_some() {
            out.push(item("Add anchor here", MenuAction::AddCurveAnchor, true));
        }
        if t.picked_stroke.is_some() {
            out.push(item("Delete this stroke", MenuAction::DeleteStroke, true));
        }
        out.push(item(
            "Save drawing to the shape library…",
            MenuAction::SaveToLibrary,
            false,
        ));
    }
    if !out.is_empty() {
        out.push(MenuEntry::Separator);
    }
    let shapes = if t.count == 1 {
        "shape".to_string()
    } else {
        format!("{} shapes", t.count)
    };
    out.push(item(format!("Delete {shapes}"), MenuAction::Delete, true));
    out.push(item(
        format!("Duplicate {shapes}"),
        MenuAction::Duplicate,
        true,
    ));
    out.push(item(format!("Copy {shapes}"), MenuAction::Copy, true));
    out.push(item(
        "Paste at the play line",
        MenuAction::Paste,
        t.clipboard,
    ));
    out.push(MenuEntry::Separator);
    out.push(item("Flip sideways", MenuAction::FlipSideways, true));
    out.push(item("Flip upside down", MenuAction::FlipUpsideDown, true));
    out.push(item("Turn 90° left", MenuAction::TurnLeft, true));
    out.push(item("Turn 90° right", MenuAction::TurnRight, true));
    out
}

/// 右键命中形状 i：选中它（还没选中的话）、自定义形状先拾取鼠标下的笔画，然后开菜单。
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

/// 有弹出层（右键菜单 / 下拉框）开着：卷帘输入让路（原版 tk 菜单会抓走事件）。
pub fn is_popup_open(ctx: &egui::Context) -> bool {
    egui::Popup::is_any_open(ctx)
}

/// 每帧画菜单（放在卷帘绘制之后）。菜单外点击 / Esc 由 egui 的 Popup 关掉。
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
    // 右键位置是卷帘局部坐标：换成屏幕坐标挂 Popup
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

/// "Add anchor here"：曲线形状加在曲线上，自定义形状加在被拾取的曲线笔画上。
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

/// "Add point here"：在折线上加一个吸附好的点，放在最不拉长折线的地方（原版 insert_poly_point）。
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
    app.push_undo();
    if let Some(sh) = app.shapes.get_mut(i) {
        sh.pts.insert(k, pt);
    }
    app.shapes_changed();
}

/// 折线里插入 q 后多出来的屏幕长度（原版 extra_length）。
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

/// 加点后折线最短的位置（屏幕坐标；原版 insert_poly_point 取 extra_length 最小）。
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
            assert!(enabled(&got, a), "缺少可用项：{a:?}");
        }
        // 剪贴板空：粘贴项在但禁用
        assert!(find(&got, MenuAction::Paste).is_some());
        assert!(!enabled(&got, MenuAction::Paste));
        // 直线没有加点 / 加锚点 / 删笔画
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
    fn custom_with_picked_curve_can_add_anchor_and_delete_stroke() {
        let mut t = target(Kind::Custom);
        t.picked_stroke = Some(1);
        t.picked_curve = Some(1);
        let got = menu_entries_for(&t);
        assert!(enabled(&got, MenuAction::AddCurveAnchor));
        assert!(enabled(&got, MenuAction::DeleteStroke));
        // 图形库没移植：项在但禁用
        assert!(find(&got, MenuAction::SaveToLibrary).is_some());
        assert!(!enabled(&got, MenuAction::SaveToLibrary));
        // 拾取的是普通折线：不能加锚点，但能删
        t.picked_curve = None;
        let got = menu_entries_for(&t);
        assert!(find(&got, MenuAction::AddCurveAnchor).is_none());
        assert!(enabled(&got, MenuAction::DeleteStroke));
    }

    #[test]
    fn poly_point_goes_where_it_stretches_least() {
        // 横着一排点 (0,0)-(10,0)-(20,0)：点在 (15,1) 最近的是最后两点之间（下标 2）
        let screen = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
        assert_eq!(poly_insert_index(&screen, [15.0, 1.0]), 2);
        // 点在开头左边：插在最前
        assert_eq!(poly_insert_index(&screen, [-5.0, 0.0]), 0);
        // 点在末尾右边：插在最后
        assert_eq!(poly_insert_index(&screen, [30.0, 0.0]), 3);
        // 空折线：只有 0
        assert_eq!(poly_insert_index(&[], [1.0, 1.0]), 0);
    }
}
