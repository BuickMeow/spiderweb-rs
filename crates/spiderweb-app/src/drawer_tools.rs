//! Drawer 画板的工具定义与纯几何（原版 window/drawer.py 的工具部分）。
//!
//! 画板坐标是 0..1 的方框：u 向右、v 向上，与卷帘的拍 / 音高变换无关。
//! 这里只放与界面无关的纯函数：吸附、命中、把手、翻转 / 旋转等，方便单测。

use spiderweb_core::Pt;
use spiderweb_core::bezier::{self, HandleKind};
use spiderweb_core::custom::{map_stroke, stroke_points};
use spiderweb_core::shape::Stroke;

/// 网格（原版 drawer.GRIDS）。
pub const GRIDS: [i64; 8] = [4, 8, 12, 16, 24, 32, 48, 64];

/// 画板工具（原版 TOOLS + Triangle）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawerTool {
    Select,
    Line,
    Poly,
    Free,
    Curve,
    Arc,
    Square,
    Circle,
    Triangle,
    Erase,
}

impl DrawerTool {
    pub const ALL: [DrawerTool; 10] = [
        DrawerTool::Select,
        DrawerTool::Line,
        DrawerTool::Poly,
        DrawerTool::Free,
        DrawerTool::Curve,
        DrawerTool::Arc,
        DrawerTool::Square,
        DrawerTool::Circle,
        DrawerTool::Triangle,
        DrawerTool::Erase,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrawerTool::Select => "Select",
            DrawerTool::Line => "Line",
            DrawerTool::Poly => "Polyline",
            DrawerTool::Free => "Freehand",
            DrawerTool::Curve => "Curve",
            DrawerTool::Arc => "Arc",
            DrawerTool::Square => "Square",
            DrawerTool::Circle => "Circle",
            DrawerTool::Triangle => "Triangle",
            DrawerTool::Erase => "Eraser",
        }
    }

    pub fn hotkey(self) -> &'static str {
        match self {
            DrawerTool::Select => "v",
            DrawerTool::Line => "l",
            DrawerTool::Poly => "p",
            DrawerTool::Free => "f",
            DrawerTool::Curve => "c",
            DrawerTool::Arc => "a",
            DrawerTool::Square => "s",
            DrawerTool::Circle => "o",
            DrawerTool::Triangle => "t",
            DrawerTool::Erase => "e",
        }
    }

    /// 拖动式方框工具（点一下再点一下也行，见 Drawer::follow）。
    pub fn is_box(self) -> bool {
        matches!(
            self,
            DrawerTool::Line
                | DrawerTool::Curve
                | DrawerTool::Square
                | DrawerTool::Circle
                | DrawerTool::Triangle
        )
    }
}

/// `round(x, 5)`（原版 event_pt 的取整）。
pub fn round5(x: f64) -> f64 {
    (x * 1e5).round() / 1e5
}

/// 画板点吸附到网格（原版 event_pt 的吸附部分；Shift 不吸附）。
pub fn snap_uv(u: f64, v: f64, n: i64, shift: bool) -> Pt {
    if shift || n <= 0 {
        return [round5(u), round5(v)];
    }
    let nf = n as f64;
    [round5((u * nf).round() / nf), round5((v * nf).round() / nf)]
}

/// 从 start 到 pt 的框修正成正方形（原版 perfect；Ctrl 时方 / 圆用）。
pub fn perfect(start: Pt, pt: Pt) -> Pt {
    let (du, dv) = (pt[0] - start[0], pt[1] - start[1]);
    let m = du.abs().max(dv.abs());
    [
        start[0] + if du >= 0.0 { m } else { -m },
        start[1] + if dv >= 0.0 { m } else { -m },
    ]
}

/// 方框（原版 square 工具的五点闭合折线）。
pub fn box_pts(start: Pt, pt: Pt) -> Vec<Pt> {
    let (u0, u1) = (start[0].min(pt[0]), start[0].max(pt[0]));
    let (v0, v1) = (start[1].min(pt[1]), start[1].max(pt[1]));
    vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1], [u0, v0]]
}

/// 方框里的三角（顶点朝上，同内置 Triangle）。
pub fn triangle_pts(start: Pt, pt: Pt) -> Vec<Pt> {
    let (u0, u1) = (start[0].min(pt[0]), start[0].max(pt[0]));
    let (v0, v1) = (start[1].min(pt[1]), start[1].max(pt[1]));
    let mid = round5((u0 + u1) / 2.0);
    vec![[u0, v0], [u1, v0], [mid, v1], [u0, v0]]
}

/// 拖动给出的 S 形曲线（原版 curve 工具的起点 / 中点手柄 / 终点）。
pub fn curve_pts(start: Pt, pt: Pt) -> Vec<Pt> {
    let mid = round5((start[0] + pt[0]) / 2.0);
    vec![start, [mid, start[1]], [mid, pt[1]], pt]
}

/// 屏幕点 (x, y) 到一条笔画的距离（点列按屏幕坐标给）。
pub fn path_dist(screen: &[[f64; 2]], x: f64, y: f64) -> f64 {
    if screen.is_empty() {
        return f64::INFINITY;
    }
    if screen.len() == 1 {
        return ((screen[0][0] - x).powi(2) + (screen[0][1] - y).powi(2)).sqrt();
    }
    let mut best = f64::INFINITY;
    for w in screen.windows(2) {
        let [ax, ay] = w[0];
        let [bx, by] = w[1];
        let (dx, dy) = (bx - ax, by - ay);
        let ll = dx * dx + dy * dy;
        let u = if ll == 0.0 {
            0.0
        } else {
            (((x - ax) * dx + (y - ay) * dy) / ll).clamp(0.0, 1.0)
        };
        let d = ((x - ax - u * dx).powi(2) + (y - ay - u * dy).powi(2)).sqrt();
        best = best.min(d);
    }
    best
}

/// 屏幕上 (x, y) 下的笔画号（从最上面一条找起，near = 像素半径）。
pub fn stroke_at(
    strokes: &[Stroke],
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    x: f64,
    y: f64,
    near: f64,
) -> Option<usize> {
    for i in (0..strokes.len()).rev() {
        let screen: Vec<[f64; 2]> = stroke_points(&strokes[i])
            .iter()
            .map(|p| to_screen(*p))
            .collect();
        if path_dist(&screen, x, y) <= near {
            return Some(i);
        }
    }
    None
}

/// 选中笔画上一个能拖的点（原版 handles 的元素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// 折线 / 弧的点、曲线两端、椭圆角点（角点号 0-3）
    Point(usize),
    /// 椭圆角点（0 左下、1 右下、2 右上、3 左上）
    Corner(usize),
    /// 曲线的锚点 / 手柄：j 是点号，ctrl 区分手柄
    Pen { j: usize, ctrl: bool },
}

/// 要显示的把手 `(笔画号, 点, (u, v))`：选中的笔画排前面；长自由笔画只在选中时显示点
/// （原版 Drawer.handles）。
pub fn handles(strokes: &[Stroke], sel: Option<usize>) -> Vec<(usize, Spot, Pt)> {
    let order: Vec<usize> = sel
        .into_iter()
        .chain((0..strokes.len()).rev().filter(|&i| Some(i) != sel))
        .collect();
    let mut out = Vec::new();
    for i in order {
        match &strokes[i] {
            Stroke::Ellipse { box_ } => {
                let [u0, v0, u1, v1] = *box_;
                for (k, p) in [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
                    .into_iter()
                    .enumerate()
                {
                    out.push((i, Spot::Corner(k), p));
                }
            }
            Stroke::Curve { pts, .. } => {
                // 原版把 pen_handles 反过来放：端点的命中优先级最高
                for (j, kind) in bezier::pen_handles(pts, Some(i) == sel).into_iter().rev() {
                    match kind {
                        HandleKind::End => out.push((i, Spot::Point(j), pts[j])),
                        HandleKind::Ctrl => out.push((i, Spot::Pen { j, ctrl: true }, pts[j])),
                        HandleKind::Anchor => out.push((i, Spot::Pen { j, ctrl: false }, pts[j])),
                    }
                }
            }
            Stroke::Poly { pts, .. } | Stroke::Arc { pts, .. } => {
                if pts.len() <= 64 || Some(i) == sel {
                    for (j, p) in pts.iter().enumerate() {
                        out.push((i, Spot::Point(j), *p));
                    }
                }
            }
        }
    }
    out
}

/// 命中的把手（near 像素内，先找选中的笔画）。
pub fn handle_at(
    strokes: &[Stroke],
    sel: Option<usize>,
    to_screen: &dyn Fn(Pt) -> [f64; 2],
    x: f64,
    y: f64,
    near: f64,
) -> Option<(usize, Spot)> {
    for (i, spot, p) in handles(strokes, sel) {
        let s = to_screen(p);
        if (s[0] - x).abs() <= near && (s[1] - y).abs() <= near {
            return Some((i, spot));
        }
    }
    None
}

/// 被选中的曲线笔画里，具体是哪一条（原版 is_pen_point 的入口）。
pub fn selected_curve(strokes: &[Stroke], sel: Option<usize>) -> Option<usize> {
    let i = sel?;
    matches!(strokes.get(i), Some(Stroke::Curve { .. })).then_some(i)
}

/// 一组笔画的所有原始点（椭圆用框的两个角）（原版 middle 的点集）。
fn raw_points(strokes: &[Stroke], idx: &[usize]) -> Vec<Pt> {
    let mut out = Vec::new();
    for &i in idx {
        match strokes.get(i) {
            Some(Stroke::Ellipse { box_ }) => {
                out.push([box_[0], box_[1]]);
                out.push([box_[2], box_[3]]);
            }
            Some(
                Stroke::Poly { pts, .. } | Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. },
            ) => out.extend_from_slice(pts),
            None => {}
        }
    }
    out
}

/// 一组笔画的中间（u, v）；没有点时 None。
pub fn center_uv(strokes: &[Stroke], idx: &[usize]) -> Option<Pt> {
    let pts = raw_points(strokes, idx);
    let ul = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let uh = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let vl = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let vh = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    (ul.is_finite() && uh.is_finite() && vl.is_finite() && vh.is_finite())
        .then_some([(ul + uh) / 2.0, (vl + vh) / 2.0])
}

/// 翻转选中的笔画（原版 flip）：sideways = 左右翻，否则上下翻。
pub fn flip_strokes(strokes: &[Stroke], idx: &[usize], sideways: bool) -> Vec<Stroke> {
    let Some(c) = center_uv(strokes, idx) else {
        return strokes.to_vec();
    };
    strokes
        .iter()
        .enumerate()
        .map(|(i, st)| {
            if !idx.contains(&i) {
                return st.clone();
            }
            map_stroke(
                st,
                |u, v| {
                    if sideways {
                        [2.0 * c[0] - u, v]
                    } else {
                        [u, 2.0 * c[1] - v]
                    }
                },
                1.0,
                1.0,
            )
        })
        .collect()
}

/// 选中的笔画绕中间转 90°（原版 turn）：站在网格上的图形转完还站在网格上。
pub fn turn_strokes(strokes: &[Stroke], idx: &[usize], n: i64, clockwise: bool) -> Vec<Stroke> {
    let pts = raw_points(strokes, idx);
    if pts.is_empty() || n <= 0 {
        return strokes.to_vec();
    }
    let nf = n as f64;
    let us: Vec<f64> = pts.iter().map(|p| p[0] * nf).collect();
    let vs: Vec<f64> = pts.iter().map(|p| p[1] * nf).collect();
    let ul = us.iter().copied().fold(f64::INFINITY, f64::min);
    let uh = us.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let vl = vs.iter().copied().fold(f64::INFINITY, f64::min);
    let vh = vs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let on_grid = us
        .iter()
        .chain(vs.iter())
        .all(|c| (c - c.round()).abs() < 1e-6);
    let (mut cu, mut cv) = ((ul + uh) / 2.0, (vl + vh) / 2.0);
    if on_grid {
        let (a, b) = ((2.0 * cu).round() as i64, (2.0 * cv).round() as i64);
        // 一个中间在线上、一个在线间：转 90° 会落到网格之间，绕半个方格外转
        if (a.rem_euclid(2) + b.rem_euclid(2)) % 2 == 1 {
            let d = if a.rem_euclid(2) == 1 { 0.5 } else { -0.5 };
            if clockwise {
                cu += d;
            } else {
                cv += d;
            }
        }
    }
    let (cu, cv) = (cu / nf, cv / nf);
    strokes
        .iter()
        .enumerate()
        .map(|(i, st)| {
            if !idx.contains(&i) {
                return st.clone();
            }
            map_stroke(
                st,
                |u, v| {
                    if clockwise {
                        [cu + (v - cv), cv - (u - cu)]
                    } else {
                        [cu - (v - cv), cv + (u - cu)]
                    }
                },
                1.0,
                1.0,
            )
        })
        .collect()
}

/// copy / 翻转 / 旋转作用的对象：选中的一条，或（没选时）全部（原版 targets）。
pub fn targets(sel: Option<usize>, len: usize) -> Vec<usize> {
    match sel {
        Some(i) if i < len => vec![i],
        _ => (0..len).collect(),
    }
}

/// 去掉连续重复的点（原版 finish_poly / commit 的过滤）。
pub fn dedupe_points(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last() != Some(p) {
            out.push(*p);
        }
    }
    out
}

/// 正 n 边形（"Sides" 控制用）：中心 center、外接半径 r。
pub fn polygon_pts(center: Pt, r: f64, sides: usize) -> Vec<Pt> {
    let n = sides.clamp(3, 64);
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..n {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        out.push([
            round5(center[0] + r * a.cos()),
            round5(center[1] + r * a.sin()),
        ]);
    }
    out.push(out[0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(pts: Vec<Pt>) -> Stroke {
        Stroke::Poly {
            pts,
            free: false,
            smooth: 0,
            k: 1.0,
        }
    }

    fn identity(p: Pt) -> [f64; 2] {
        p
    }

    #[test]
    fn snap_rounds_to_grid_unless_shift() {
        assert_eq!(snap_uv(0.31, 0.69, 8, false), [0.25, 0.75]);
        assert_eq!(snap_uv(0.31, 0.69, 8, true), [0.31, 0.69]);
    }

    #[test]
    fn perfect_uses_the_larger_side() {
        assert_eq!(perfect([0.0, 0.0], [0.4, 0.9]), [0.9, 0.9]);
        assert_eq!(perfect([0.0, 0.0], [-0.4, 0.9]), [-0.9, 0.9]);
    }

    #[test]
    fn box_and_triangle_fill_the_drag_box() {
        assert_eq!(
            box_pts([1.0, 1.0], [0.0, 0.0]),
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]
        );
        assert_eq!(
            triangle_pts([0.0, 0.0], [1.0, 2.0]),
            vec![[0.0, 0.0], [1.0, 0.0], [0.5, 2.0], [0.0, 0.0]]
        );
    }

    #[test]
    fn curve_starts_with_an_s_shape() {
        assert_eq!(
            curve_pts([0.0, 0.0], [1.0, 2.0]),
            vec![[0.0, 0.0], [0.5, 0.0], [0.5, 2.0], [1.0, 2.0]]
        );
    }

    #[test]
    fn stroke_at_finds_the_topmost() {
        let strokes = vec![
            poly(vec![[0.0, 0.0], [1.0, 0.0]]),
            poly(vec![[0.0, 0.0], [1.0, 0.0]]),
        ];
        assert_eq!(stroke_at(&strokes, &identity, 0.5, 0.01, 0.05), Some(1));
        assert_eq!(stroke_at(&strokes, &identity, 0.5, 0.5, 0.05), None);
    }

    #[test]
    fn flip_sideways_mirrors_around_the_middle() {
        let strokes = vec![poly(vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]])];
        let got = flip_strokes(&strokes, &[0], true);
        match &got[0] {
            Stroke::Poly { pts, .. } => {
                assert_eq!(pts, &vec![[1.0, 0.0], [0.0, 0.0], [0.5, 1.0]]);
            }
            other => panic!("期望 poly，得到 {other:?}"),
        }
    }

    #[test]
    fn turn_clockwise_moves_points_a_quarter() {
        let strokes = vec![poly(vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]])];
        // n = 4：中心 (0.5, 0.5)，全部落在网格上，2cu + 2cv = 2 是偶数，不偏半格
        let got = turn_strokes(&strokes, &[0], 4, true);
        match &got[0] {
            Stroke::Poly { pts, .. } => {
                assert_eq!(pts, &vec![[0.0, 1.0], [0.0, 0.0], [1.0, 0.5]]);
            }
            other => panic!("期望 poly，得到 {other:?}"),
        }
    }

    #[test]
    fn turn_leaves_other_strokes_alone() {
        let strokes = vec![
            poly(vec![[0.0, 0.0], [1.0, 1.0]]),
            poly(vec![[2.0, 2.0], [3.0, 3.0]]),
        ];
        let got = turn_strokes(&strokes, &[0], 4, true);
        assert_eq!(got[1], strokes[1]);
    }

    #[test]
    fn handles_show_ellipse_corners_and_select_first() {
        let strokes = vec![
            Stroke::Ellipse {
                box_: [0.0, 0.0, 1.0, 1.0],
            },
            poly(vec![[0.25, 0.25], [0.5, 0.5]]),
        ];
        let hs = handles(&strokes, Some(1));
        assert_eq!(hs[0].0, 1, "选中的笔画排最前");
        assert_eq!(hs[0].1, Spot::Point(0));
        let corners: Vec<_> = hs.iter().filter(|(i, ..)| *i == 0).collect();
        assert_eq!(corners.len(), 4);
        assert_eq!(corners[0].1, Spot::Corner(0));
    }

    #[test]
    fn handle_at_finds_the_spot() {
        let strokes = vec![poly(vec![[0.0, 0.0], [1.0, 1.0]])];
        assert_eq!(
            handle_at(&strokes, None, &identity, 0.01, 0.01, 0.05),
            Some((0, Spot::Point(0)))
        );
        assert_eq!(handle_at(&strokes, None, &identity, 0.5, 0.5, 0.05), None);
    }

    #[test]
    fn targets_are_sel_or_all() {
        assert_eq!(targets(Some(2), 5), vec![2]);
        assert_eq!(targets(None, 3), vec![0, 1, 2]);
        assert_eq!(targets(Some(9), 3), vec![0, 1, 2]);
    }

    #[test]
    fn dedupe_removes_consecutive_repeats_only() {
        let got = dedupe_points(&[[0.0, 0.0], [0.0, 0.0], [1.0, 0.0], [0.0, 0.0]]);
        assert_eq!(got, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]]);
    }

    #[test]
    fn polygon_is_closed_and_regular() {
        let pts = polygon_pts([0.5, 0.5], 0.5, 4);
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[0], pts[4]);
        assert!((pts[0][0] - 1.0).abs() < 1e-9);
        assert!((pts[0][1] - 0.5).abs() < 1e-9);
    }
}
