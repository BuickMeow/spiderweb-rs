//! Curve tool and curve editing (pen): handle enumeration, hit testing, point dragging,
//! anchor add/remove, symmetric following and handle drawing.
//! Corresponds to upstream roll/roll_curve.py's CurveEditing and the curve branch of
//! roll_draw.py's draw_handles; editing logic is delegated to spiderweb_core::bezier
//! (the same set as Python's notes/bezier.py).

use eframe::egui;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::bezier::{self, CanDelete, Curve, HandleKind};
use spiderweb_core::shape::{Kind, Shape};

use crate::app::App;
use crate::roll::View;

/// Curve handle kind (anchor / control handle / end), an alias for core HandleKind.
pub type CurveHandle = HandleKind;

/// Color of handle lines and handles (upstream HANDLE_COLOR).
const HANDLE_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);
/// Outline of the squares at the curve's ends (upstream #c00000).
const END_COLOR: Color32 = Color32::from_rgb(0xc0, 0x00, 0x00);

/// The View's (to_screen, from_screen) closures (upstream to_xy / from_xy).
fn view_maps(
    view: &View,
) -> (
    impl Fn(Pt) -> [f64; 2] + use<>,
    impl Fn(f64, f64) -> Pt + use<>,
) {
    let a = view.clone();
    let b = view.clone();
    (
        move |p: Pt| [a.x_of(p[0]) as f64, a.y_of(p[1]) as f64],
        move |x: f64, y: f64| [b.b_of(x as f32), b.p_of(y as f32)],
    )
}

/// The curve's handle points `(point number, point, kind)` in drawing order: pulled-out handles,
/// middle anchors, piece ends (core pen_handles).
fn curve_point_handles(sh: &Shape) -> Vec<(usize, Pt, CurveHandle)> {
    if sh.kind != Kind::Curve {
        return Vec::new();
    }
    bezier::pen_handles(&sh.pts, true, &sh.gaps)
        .into_iter()
        .filter_map(|(i, kind)| sh.pts.get(i).map(|p| (i, *p, kind)))
        .collect()
}

/// Handles to display as `(point, kind)` (upstream pen_handles).
fn curve_handle_points(sh: &Shape) -> Vec<(Pt, CurveHandle)> {
    curve_point_handles(sh)
        .into_iter()
        .map(|(_, p, kind)| (p, kind))
        .collect()
}

/// Screen hit test: point number of the closest handle within `near` pixels of (x, y).
/// `any` = Select tool: ends can be dragged too; otherwise ends are left for "start a new
/// curve from an endpoint" (the free flag of upstream curve_handles).
fn hit_curve_handle(
    sh: &Shape,
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    x: f64,
    y: f64,
    near: f64,
    any: bool,
) -> Option<usize> {
    for (i, p, kind) in curve_point_handles(sh).into_iter().rev() {
        if !any && kind == CurveHandle::End {
            continue;
        }
        let s = to_screen(p);
        if (s[0] - x).abs() <= near && (s[1] - y).abs() <= near {
            return Some(i);
        }
    }
    None
}

/// Shape -> curve (curve fields only).
fn curve_of(sh: &Shape) -> Curve {
    Curve {
        pts: sh.pts.clone(),
        sharp: sh.sharp.clone(),
        sym: sh.sym,
        gaps: sh.gaps.clone(),
        splits: sh.splits.clone(),
    }
}

/// Write the curve's points, corners, gaps and section marks back to the shape (sym unchanged).
fn apply_curve(sh: &mut Shape, c: Curve) {
    sh.pts = c.pts;
    sh.sharp = c.sharp;
    sh.sym = c.sym;
    sh.gaps = c.gaps;
    sh.splits = c.splits;
}

/// What happens when right-clicking point i (core can_delete).
fn curve_can_delete(sh: &Shape, i: usize) -> Option<CanDelete> {
    bezier::can_delete(&curve_of(sh), i)
}

impl App {
    /// Handles to display for the selected curve `(point, kind)`: pulled-out handles, middle anchors, ends.
    pub fn curve_handles(&self, sh: &Shape) -> Vec<(Pt, CurveHandle)> {
        curve_handle_points(sh)
    }

    /// Curve handles `(point, point number)` in drawing order (used by the roll's handles).
    pub fn curve_handle_indices(&self, sh: &Shape) -> Vec<(Pt, usize)> {
        curve_point_handles(sh)
            .into_iter()
            .map(|(i, p, _)| (p, i))
            .collect()
    }

    /// Screen hit test for curve handles, returns the point number; ends are also hit testable when `any` is true (Select tool).
    pub fn curve_hit_handle(&self, sh: &Shape, pos: Pos2, any: bool) -> Option<usize> {
        let (to_screen, _) = view_maps(&self.view);
        let near = 7.0_f32.max(8.0 * self.scale()) as f64;
        hit_curve_handle(sh, &to_screen, pos.x as f64, pos.y as f64, near, any)
    }

    /// Drags curve point i to pt (already snapped): the anchor carries its handles, Alt pulls
    /// out / breaks handles, the other handle of a smooth anchor rotates along, and the other
    /// half of a symmetric curve follows (bezier.drag_point, exact=false).
    pub fn curve_drag(&mut self, i: usize, pt: Pt, alt: bool) {
        let (to_screen, from_screen) = view_maps(&self.view);
        let Some(idx) = self.sel else {
            return;
        };
        let Some(sh) = self.shapes.get(idx) else {
            return;
        };
        if sh.kind != Kind::Curve {
            return;
        }
        let mut c = curve_of(sh);
        bezier::drag_point(&mut c, i, pt, alt, &to_screen, &from_screen, false);
        if let Some(sh) = self.shapes.get_mut(idx) {
            apply_curve(sh, c);
        }
    }

    /// Adds an anchor at t on segment seg of the selected curve and moves it to pt (the curve
    /// passes through there); a matching one is added to the symmetric half. true = added.
    pub fn curve_add_anchor(&mut self, seg: usize, t: f64, pt: Pt) -> bool {
        if !(0.0..1.0).contains(&t) {
            return false;
        }
        let (to_screen, _) = view_maps(&self.view);
        let Some(idx) = self.sel else {
            return false;
        };
        let Some(sh) = self.shapes.get(idx) else {
            return false;
        };
        if sh.kind != Kind::Curve || seg >= bezier::segments(&sh.pts).len() {
            return false;
        }
        let mut c = curve_of(sh);
        if !bezier::add_anchor(&mut c, seg, t, pt, &to_screen, false) {
            return false;
        }
        self.push_undo(&rust_i18n::t!("roll_curve.add_an_anchor"));
        if let Some(sh) = self.shapes.get_mut(idx) {
            apply_curve(sh, c);
        }
        true
    }

    /// Middle-click / double-click adds an anchor on the selected curve: uses bezier.nearest
    /// to find the closest segment and t, and only adds it when the curve is within `near`
    /// pixels of the mouse (None = no limit). true = added.
    pub fn curve_click(&mut self, pos: Pos2, pt: Pt, near: Option<f64>) -> bool {
        let (to_screen, _) = view_maps(&self.view);
        let Some(sh) = self.selected() else {
            return false;
        };
        if sh.kind != Kind::Curve {
            return false;
        }
        let Some((seg, t, d)) = bezier::nearest(
            &sh.pts,
            &to_screen,
            pos.x as f64,
            pos.y as f64,
            64,
            &sh.gaps,
        ) else {
            return false;
        };
        if let Some(near) = near
            && d > near
        {
            return false;
        }
        if !self.curve_add_anchor(seg, t, pt) {
            return false;
        }
        self.shapes_changed();
        true
    }

    /// Right-click on curve point i: an anchor is deleted and its handles retract into it; the middle anchor of a symmetric curve is kept. true = handled.
    pub fn curve_delete_handle(&mut self, i: usize) -> bool {
        let Some(idx) = self.sel else {
            return false;
        };
        let Some(sh) = self.shapes.get(idx) else {
            return false;
        };
        if sh.kind != Kind::Curve {
            return false;
        }
        let what = curve_can_delete(sh, i);
        let mut c = curve_of(sh);
        match what {
            None => return false,
            Some(CanDelete::Middle) => {
                self.status = rust_i18n::t!("status.middle_anchor").to_string();
                return true;
            }
            Some(_) => {}
        }
        let (to_screen, _) = view_maps(&self.view);
        self.push_undo(&rust_i18n::t!("roll_curve.remove_a_point"));
        bezier::delete_point(&mut c, i, &to_screen, false);
        if let Some(sh) = self.shapes.get_mut(idx) {
            apply_curve(sh, c);
        }
        self.shapes_changed();
        true
    }

    /// The other half of a symmetric curve follows the half containing point i; returns true
    /// when the curve is symmetric. Called after the panel changes a point's coordinates
    /// (upstream PianoRoll.keep_symmetric, i=0).
    #[allow(dead_code)] // Curve point box yet to be wired up (the panels Points table does not include curves)
    pub fn curve_keep_symmetric(&mut self, i: usize) -> bool {
        let (to_screen, _) = view_maps(&self.view);
        let Some(idx) = self.sel else {
            return false;
        };
        let Some(sh) = self.shapes.get_mut(idx) else {
            return false;
        };
        if sh.kind != Kind::Curve {
            return false;
        }
        let mut c = curve_of(sh);
        let done = bezier::keep_symmetric(&mut c, i, &to_screen, false);
        if done {
            apply_curve(sh, c);
        }
        done
    }
}

/// Draws the handles of the selected curve (the curve branch of upstream draw_handles):
/// handle lines are blue with a white edge, anchors are white circles with a blue edge,
/// handles are solid blue circles with a white edge, ends are white squares with a red edge.
pub fn paint_curve_handles(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let s = app.scale();
    let at = |p: Pt| {
        Pos2::new(
            rect.min.x + app.view.x_of(p[0]),
            rect.min.y + app.view.y_of(p[1]),
        )
    };
    for (a, h) in bezier::handle_lines(&sh.pts, &sh.gaps) {
        painter.line_segment(
            [at(a), at(h)],
            Stroke::new((3.5 * s).max(3.0), Color32::WHITE),
        );
        painter.line_segment(
            [at(a), at(h)],
            Stroke::new((1.5 * s).max(1.0), HANDLE_COLOR),
        );
    }
    let r = 4.0 * s;
    for (pt, kind) in app.curve_handles(sh) {
        let p = at(pt);
        match kind {
            CurveHandle::Ctrl => {
                let q = r + 0.5 * s;
                painter.circle_filled(p, q, HANDLE_COLOR);
                painter.circle_stroke(p, q, Stroke::new((1.0 * s).max(1.0), Color32::WHITE));
            }
            CurveHandle::Anchor => {
                let q = r + 1.5 * s;
                painter.circle_filled(p, q, Color32::WHITE);
                painter.circle_stroke(p, q, Stroke::new((2.0 * s).max(2.0), HANDLE_COLOR));
            }
            CurveHandle::End => {
                let q = Rect::from_center_size(p, Vec2::splat(r * 2.0));
                painter.rect_filled(q, 0.0, Color32::WHITE);
                painter.rect_stroke(
                    q,
                    0.0,
                    Stroke::new(2.0, END_COLOR),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spiderweb_core::shape::Sym;

    fn curve4() -> Shape {
        Shape::new(
            Kind::Curve,
            vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]],
        )
    }

    fn curve7() -> Shape {
        Shape::new(
            Kind::Curve,
            vec![
                [0.0, 0.0],
                [0.5, 0.5],
                [1.5, 0.5],
                [2.0, 0.0],
                [2.5, -0.5],
                [3.5, -0.5],
                [4.0, 0.0],
            ],
        )
    }

    #[test]
    fn handles_order_is_ctrl_then_anchor_then_ends() {
        let got: Vec<(usize, CurveHandle)> = curve_point_handles(&curve4())
            .into_iter()
            .map(|(i, _, k)| (i, k))
            .collect();
        assert_eq!(
            got,
            vec![
                (1, CurveHandle::Ctrl),
                (2, CurveHandle::Ctrl),
                (0, CurveHandle::End),
                (3, CurveHandle::End),
            ]
        );
    }

    #[test]
    fn middle_anchor_is_anchor() {
        let got: Vec<(usize, CurveHandle)> = curve_point_handles(&curve7())
            .into_iter()
            .map(|(i, _, k)| (i, k))
            .collect();
        assert_eq!(
            got,
            vec![
                (1, CurveHandle::Ctrl),
                (2, CurveHandle::Ctrl),
                (4, CurveHandle::Ctrl),
                (5, CurveHandle::Ctrl),
                (3, CurveHandle::Anchor),
                (0, CurveHandle::End),
                (6, CurveHandle::End),
            ]
        );
    }

    #[test]
    fn end_wins_over_nearby_control_with_select() {
        let sh = curve4();
        let screen = |p: Pt| [p[0], p[1]];
        // Cursor at the end (3,0): Select (any=true) hits the end first; the pen (any=false) can only hit handles
        assert_eq!(hit_curve_handle(&sh, &screen, 3.0, 0.0, 2.0, true), Some(3));
        assert_eq!(
            hit_curve_handle(&sh, &screen, 3.0, 0.0, 2.0, false),
            Some(2)
        );
    }

    #[test]
    fn nothing_within_reach() {
        let sh = curve4();
        let screen = |p: Pt| [p[0], p[1]];
        assert_eq!(hit_curve_handle(&sh, &screen, 3.0, 5.0, 2.0, true), None);
    }

    #[test]
    fn delete_rules_keep_ends_and_their_handles() {
        let sh = curve4();
        assert_eq!(curve_can_delete(&sh, 0), None);
        assert_eq!(curve_can_delete(&sh, 3), None);
        assert_eq!(curve_can_delete(&sh, 1), None);
        assert_eq!(curve_can_delete(&sh, 2), None);
    }

    #[test]
    fn delete_rules_anchor_handle_and_middle() {
        let mut sh = curve7();
        assert_eq!(curve_can_delete(&sh, 1), None);
        assert_eq!(curve_can_delete(&sh, 2), Some(CanDelete::Handle));
        assert_eq!(curve_can_delete(&sh, 3), Some(CanDelete::Anchor));
        assert_eq!(curve_can_delete(&sh, 4), Some(CanDelete::Handle));
        assert_eq!(curve_can_delete(&sh, 5), None); // handle of the end anchor
        sh.sym = Some(Sym::Turn);
        assert_eq!(curve_can_delete(&sh, 3), Some(CanDelete::Middle));
        assert_eq!(curve_can_delete(&sh, 6), None);
    }
}
