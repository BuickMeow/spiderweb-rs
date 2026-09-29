//! 曲线工具与曲线编辑（钢笔）：把手枚举、命中、拖点、增删锚点、对称跟随与把手绘制。
//! 对应原版 roll/roll_curve.py 的 CurveEditing 与 roll_draw.py 的 draw_handles 曲线分支；
//! 编辑逻辑委托给 spiderweb_core::bezier（与 Python 的 notes/bezier.py 同一套）。

use eframe::egui;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::bezier::{self, CanDelete, Curve, HandleKind};
use spiderweb_core::shape::{Kind, Shape};

use crate::app::App;
use crate::roll::View;

/// 曲线把手类型（锚点 / 手柄 / 两端），核心 HandleKind 的别名。
pub type CurveHandle = HandleKind;

/// 手柄线与手柄的颜色（原版 HANDLE_COLOR）。
const HANDLE_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);
/// 曲线两端的方块描边（原版 #c00000）。
const END_COLOR: Color32 = Color32::from_rgb(0xc0, 0x00, 0x00);

/// View 的 (to_screen, from_screen) 闭包（原版 to_xy / from_xy）。
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

/// 要显示的把手 `(点, 种类)`（原版 pen_handles）。
fn curve_handle_points(sh: &Shape) -> Vec<(Pt, CurveHandle)> {
    curve_point_handles(sh)
        .into_iter()
        .map(|(_, p, kind)| (p, kind))
        .collect()
}

/// 屏幕命中：`near` 像素内离 (x, y) 最近的把手点号。
/// `any` = Select 工具：两端也能拖；否则两端留给"从端点起新建曲线"（原版 curve_handles 的 free）。
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

/// 形状 -> 曲线（只带曲线字段）。
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

/// 右键点 i 会发生什么（核心 can_delete）。
fn curve_can_delete(sh: &Shape, i: usize) -> Option<CanDelete> {
    bezier::can_delete(&curve_of(sh), i)
}

impl App {
    /// 选中曲线要显示的把手 `(点, 种类)`：拉出的手柄、中间锚点、两端。
    pub fn curve_handles(&self, sh: &Shape) -> Vec<(Pt, CurveHandle)> {
        curve_handle_points(sh)
    }

    /// 曲线把手 `(点, 点号)`，按绘制顺序（卷帘 handles 用）。
    pub fn curve_handle_indices(&self, sh: &Shape) -> Vec<(Pt, usize)> {
        curve_point_handles(sh)
            .into_iter()
            .map(|(i, p, _)| (p, i))
            .collect()
    }

    /// 屏幕命中曲线把手，返回点号；`any` 为 true（Select 工具）时两端也可命中。
    pub fn curve_hit_handle(&self, sh: &Shape, pos: Pos2, any: bool) -> Option<usize> {
        let (to_screen, _) = view_maps(&self.view);
        let near = 7.0_f32.max(8.0 * self.scale()) as f64;
        hit_curve_handle(sh, &to_screen, pos.x as f64, pos.y as f64, near, any)
    }

    /// 把曲线点 i 拖到 pt（已吸附）：锚点带着手柄走，Alt 拉出 / 断开手柄，平滑锚点另一手柄跟着转，
    /// 对称曲线的另一半跟随（bezier.drag_point，exact=false）。
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

    /// 在选中曲线的第 seg 段的 t 处加锚点并移到 pt（曲线经过那里）；对称的另一半也加一个。
    /// true = 加上了。
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
        self.push_undo();
        if let Some(sh) = self.shapes.get_mut(idx) {
            apply_curve(sh, c);
        }
        true
    }

    /// 中键 / 双击在选中的曲线上加锚点：用 bezier.nearest 找最近段与 t，
    /// 只当曲线离鼠标不超过 `near` 像素（None = 不限）时才加。true = 加上了。
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

    /// 右键曲线点 i：锚点删掉，手柄收回到锚点；对称曲线的中间锚点保留。true = 处理了。
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
        self.push_undo();
        bezier::delete_point(&mut c, i, &to_screen, false);
        if let Some(sh) = self.shapes.get_mut(idx) {
            apply_curve(sh, c);
        }
        self.shapes_changed();
        true
    }

    /// 对称曲线的另一半跟着点 i 所在的一半；是对称曲线时返回 true。
    /// 面板改了点的坐标后调用（原版 PianoRoll.keep_symmetric，i=0）。
    #[allow(dead_code)] // 曲线点框待接上（panels 的 Points 表暂不含曲线）
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

/// 画选中曲线的把手（原版 draw_handles 的 curve 分支）：
/// 手柄线白边蓝芯，锚点白圆蓝边，手柄实心蓝圆白边，两端白方块红边。
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
        // 光标在末端 (3,0)：Select（any=true）优先命中端点；钢笔（any=false）只能命中手柄
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
        assert_eq!(curve_can_delete(&sh, 5), None); // 末端锚点的手柄
        sh.sym = Some(Sym::Turn);
        assert_eq!(curve_can_delete(&sh, 3), Some(CanDelete::Middle));
        assert_eq!(curve_can_delete(&sh, 6), None);
    }
}
