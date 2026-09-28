//! 选中自定义形状的盒子（原版 roll/roll_custom.py）：角点缩放、边中缩放、盒外旋转、斜切。
//!
//! 都在屏幕单位里算（拍 * sx、音高 * sy），所以转动在屏幕上看着是对的。
//! 命中 [`custom_hit`]、鼠标形状 [`custom_cursor`]、拖动换算 [`resize_custom`] / [`skew_custom`] /
//! [`turn_custom`] 与盒子绘制 [`paint_custom_box`]。

use eframe::egui;
use egui::{Color32, CursorIcon, Pos2, Rect, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::shape::{Kind, Shape};

use crate::app::{App, Tool};

/// 盒子的蓝（roll_custom.draw_custom_box）。
const BOX_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);

/// 命中盒子的哪一部分（roll_custom.custom_hit）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomHit {
    /// 角点方块（0-3，绕一圈）
    Corner(usize),
    /// 边中（0 下、1 右、2 上、3 左，按画的方向）
    Side(usize),
    /// 边中外面一点：斜切
    Skew(usize),
    /// 角点外面一点：旋转
    Turn(usize),
    /// 盒子里面
    Inside,
}

/// 自定义形状盒子的四个角（拍 / 音高，绕一圈）：u0v0, u1v0, u1v1, u0v1。
pub fn custom_corners(sh: &Shape) -> Option<[Pt; 4]> {
    let (a, b, c) = (*sh.pts.first()?, *sh.pts.get(1)?, *sh.pts.get(2)?);
    Some([
        [a[0], a[1]],
        [b[0], b[1]],
        [b[0] + c[0] - a[0], b[1] + c[1] - a[1]],
        [c[0], c[1]],
    ])
}

/// 屏幕点是否在（可以斜的）盒子里面（roll_custom.inside_box）。
fn inside_box(corners: &[Pos2; 4], x: f32, y: f32) -> bool {
    let (ax, ay) = (corners[0].x, corners[0].y);
    let (ux, uy) = (corners[1].x - ax, corners[1].y - ay);
    let (vx, vy) = (corners[3].x - ax, corners[3].y - ay);
    let det = ux * vy - uy * vx;
    if det.abs() < 1e-9 {
        return false;
    }
    let u = ((x - ax) * vy - (y - ay) * vx) / det;
    let v = (ux * (y - ay) - uy * (x - ax)) / det;
    (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
}

/// 盒子四个角的屏幕位置。
fn screen_corners(app: &App, sh: &Shape) -> Option<[Pos2; 4]> {
    let c = custom_corners(sh)?;
    Some(c.map(|p| Pos2::new(app.view.x_of(p[0]), app.view.y_of(p[1]))))
}

/// 屏幕命中：角点 / 边中 / 斜切 / 旋转 / 盒内（roll_custom.custom_hit）。
/// 只有 Select 与 Custom shape 工具有，且没在画草稿时。
pub fn custom_hit(app: &App, pos: Pos2) -> Option<CustomHit> {
    let i = app.sel?;
    let sh = app.shapes.get(i)?;
    if sh.kind != Kind::Custom || app.draft.is_some() {
        return None;
    }
    if !matches!(app.tool, Tool::Select | Tool::Custom) {
        return None;
    }
    let corners = screen_corners(app, sh)?;
    let s = app.scale();
    let hypot = |a: Pos2, b: Pos2| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let k = (0..4)
        .min_by(|&a, &b| hypot(corners[a], pos).total_cmp(&hypot(corners[b], pos)))
        .unwrap_or(0);
    let d = hypot(corners[k], pos);
    if d <= 7.0 * s {
        return Some(CustomHit::Corner(k));
    }
    for side in 0..4 {
        let (a, b) = (corners[side], corners[(side + 1) % 4]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let ll = dx * dx + dy * dy;
        let u = if ll == 0.0 {
            0.0
        } else {
            (((pos.x - a.x) * dx + (pos.y - a.y) * dy) / ll).clamp(0.0, 1.0)
        };
        if ((pos.x - a.x - u * dx).powi(2) + (pos.y - a.y - u * dy).powi(2)).sqrt() <= 5.0 * s {
            return Some(CustomHit::Side(side));
        }
    }
    let inside = inside_box(&corners, pos.x, pos.y);
    if !inside {
        let mids: [Pos2; 4] = std::array::from_fn(|i| {
            Pos2::new(
                (corners[i].x + corners[(i + 1) % 4].x) / 2.0,
                (corners[i].y + corners[(i + 1) % 4].y) / 2.0,
            )
        });
        let m = (0..4)
            .min_by(|&a, &b| hypot(mids[a], pos).total_cmp(&hypot(mids[b], pos)))
            .unwrap_or(0);
        let dm = hypot(mids[m], pos);
        if dm <= 20.0 * s && dm < d {
            return Some(CustomHit::Skew(m));
        }
    }
    if d <= 24.0 * s && !inside {
        return Some(CustomHit::Turn(k));
    }
    inside.then_some(CustomHit::Inside)
}

/// 屏幕方向 (dx, dy) 最接近的双箭头（roll_custom.arrow_cursor）：- / | \。
fn arrow_cursor(dx: f32, dy: f32) -> CursorIcon {
    let a = (-dy as f64).atan2(dx as f64).to_degrees().rem_euclid(180.0);
    let idx = ((a + 22.5) / 45.0) as usize % 4;
    [
        CursorIcon::ResizeHorizontal,
        CursorIcon::ResizeNeSw,
        CursorIcon::ResizeVertical,
        CursorIcon::ResizeNwSe,
    ][idx]
}

/// 盒子命中时的鼠标形状（roll_custom.custom_cursor）。
pub fn custom_cursor(app: &App, hit: Option<CustomHit>) -> CursorIcon {
    let Some(hit) = hit else {
        return match app.tool {
            Tool::Select => CursorIcon::Default,
            Tool::Text => CursorIcon::Text,
            _ => CursorIcon::Crosshair,
        };
    };
    match hit {
        CustomHit::Turn(_) => CursorIcon::Move,
        CustomHit::Inside => {
            if app.tool == Tool::Select {
                CursorIcon::Move
            } else {
                CursorIcon::Crosshair
            }
        }
        CustomHit::Corner(k) | CustomHit::Side(k) | CustomHit::Skew(k) => {
            let Some(i) = app.sel else {
                return CursorIcon::Crosshair;
            };
            let Some(sh) = app.shapes.get(i) else {
                return CursorIcon::Crosshair;
            };
            let Some(c) = screen_corners(app, sh) else {
                return CursorIcon::Crosshair;
            };
            match hit {
                CustomHit::Corner(_) => {
                    // 角点两个边的方向平均，朝外（盒子很长时也是对角）
                    let (mut dx, mut dy) = (0.0_f32, 0.0_f32);
                    for n in [c[(k + 3) % 4], c[(k + 1) % 4]] {
                        let ll = ((c[k].x - n.x).powi(2) + (c[k].y - n.y).powi(2))
                            .sqrt()
                            .max(1.0);
                        dx += (c[k].x - n.x) / ll;
                        dy += (c[k].y - n.y) / ll;
                    }
                    arrow_cursor(dx, dy)
                }
                CustomHit::Skew(_) => {
                    let (a, b) = (c[k], c[(k + 1) % 4]);
                    arrow_cursor(b.x - a.x, b.y - a.y)
                }
                _ => {
                    // 边沿相邻两边移动
                    let (a, b) = (c[(k + 1) % 4], c[(k + 2) % 4]);
                    arrow_cursor(b.x - a.x, b.y - a.y)
                }
            }
        }
    }
}

/// 盒子的边顺着时间与音高（没转动 / 斜切），角点可以放在网格上（roll_custom.grid_aligned）。
pub fn grid_aligned(orig: &[Pt]) -> bool {
    let (Some(a), Some(b), Some(c)) = (orig.first(), orig.get(1), orig.get(2)) else {
        return false;
    };
    let flat = |db: f64, dp: f64| db.abs() < 1e-9 || dp.abs() < 1e-9;
    flat(b[0] - a[0], b[1] - a[1]) && flat(c[0] - a[0], c[1] - a[1])
}

/// 鼠标自 start 走了多少（拍 / key）：没有 Shift 时按整网格步 / 整 key（roll_custom.drag_steps 的纯逻辑）。
pub fn drag_steps_xy(start: Pt, pt: Pt, sb: Option<f64>, shift: bool) -> (f64, f64) {
    let mut db = pt[0] - start[0];
    let mut dp = pt[1] - start[1];
    if let Some(sb) = sb
        && !shift
    {
        db = (db / sb).round() * sb;
        dp = dp.round();
    }
    (db, dp)
}

/// 鼠标自 start 走了多少（拍 / key），吸附（roll_custom.drag_steps）。
pub fn drag_steps(app: &App, start: Pt, pos: Pos2, shift: bool) -> (f64, f64) {
    let pt = crate::roll::event_pt(app, pos, false, shift);
    drag_steps_xy(start, pt, app.snap_beats(), shift)
}

/// 被拖的角点 / 边中要去哪（拍 / 音高）：网格对齐的盒子吸到网格上；
/// 转动 / 斜切过的按鼠标整步移动（roll_custom.resize_point）。
pub fn resize_point(
    app: &App,
    orig: &[Pt],
    k: usize,
    side: bool,
    start: Pt,
    pos: Pos2,
    shift: bool,
) -> Pt {
    if grid_aligned(orig) {
        return crate::roll::event_pt(app, pos, true, shift);
    }
    let Some(c) = custom_corners(&Shape {
        pts: orig.to_vec(),
        ..Shape::default()
    }) else {
        return start;
    };
    let (b, p) = if side {
        (
            (c[k][0] + c[(k + 1) % 4][0]) / 2.0,
            (c[k][1] + c[(k + 1) % 4][1]) / 2.0,
        )
    } else {
        (c[k][0], c[k][1])
    };
    let (db, dp) = drag_steps(app, start, pos, shift);
    [b + db, p + dp]
}

/// 锁住哪一维（对应 Python _resize 的 lock 字符串）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResizeLock {
    U,
    V,
    None,
}

/// 盒子角点 k / 边 k 拖到 pt 后，重新算框的三个点（roll_custom._resize 的纯逻辑，屏幕单位）。
pub fn resize_custom_xy(
    orig: &[Pt],
    k: usize,
    pt: Pt,
    keep_shape: bool,
    side: bool,
    sx: f64,
    sy: f64,
) -> Vec<Pt> {
    let (Some(a), Some(b), Some(c)) = (orig.first(), orig.get(1), orig.get(2)) else {
        return orig.to_vec();
    };
    let (cu, cv, lock) = if side {
        match k {
            0 => (0.0, 0.0, ResizeLock::U),
            1 => (1.0, 0.0, ResizeLock::V),
            2 => (0.0, 1.0, ResizeLock::U),
            _ => (0.0, 0.0, ResizeLock::V),
        }
    } else {
        (
            [0.0, 1.0, 1.0, 0.0][k.min(3)],
            [0.0, 0.0, 1.0, 1.0][k.min(3)],
            ResizeLock::None,
        )
    };
    let p0 = [a[0] * sx, a[1] * sy];
    let pu = [b[0] * sx, b[1] * sy];
    let pv = [c[0] * sx, c[1] * sy];
    let u = (pu[0] - p0[0], pu[1] - p0[1]);
    let v = (pv[0] - p0[0], pv[1] - p0[1]);
    let lu = (u.0 * u.0 + u.1 * u.1).sqrt();
    let lv = (v.0 * v.0 + v.1 * v.1).sqrt();
    let eu = if lu > 1e-9 {
        (u.0 / lu, u.1 / lu)
    } else {
        (1.0, 0.0)
    };
    let ev = if lv > 1e-9 {
        (v.0 / lv, v.1 / lv)
    } else {
        (-eu.1, eu.0)
    };
    let ou = 1.0 - cu;
    let ov = 1.0 - cv;
    let opp = (p0[0] + ou * u.0 + ov * v.0, p0[1] + ou * u.1 + ov * v.1);
    let m = [pt[0] * sx, pt[1] * sy];
    let d = (m[0] - opp.0, m[1] - opp.1);
    let det = eu.0 * ev.1 - eu.1 * ev.0;
    if det.abs() < 1e-9 {
        return orig.to_vec();
    }
    let mut a_len = (d.0 * ev.1 - d.1 * ev.0) / det * (cu - ou);
    let mut b_len = (eu.0 * d.1 - eu.1 * d.0) / det * (cv - ov);
    match lock {
        ResizeLock::U => a_len = lu,
        ResizeLock::V => b_len = lv,
        ResizeLock::None => {}
    }
    if keep_shape && lu > 1e-9 && lv > 1e-9 {
        let ratio = lu / lv;
        if a_len.abs() > b_len.abs() * ratio {
            b_len = (a_len.abs() / ratio).copysign(if b_len == 0.0 { 1.0 } else { b_len });
        } else {
            a_len = (b_len.abs() * ratio).copysign(if a_len == 0.0 { 1.0 } else { a_len });
        }
    }
    let nu = (eu.0 * a_len, eu.1 * a_len);
    let nv = (ev.0 * b_len, ev.1 * b_len);
    let n0 = (opp.0 - ou * nu.0 - ov * nv.0, opp.1 - ou * nu.1 - ov * nv.1);
    vec![
        [n0.0 / sx, n0.1 / sy],
        [(n0.0 + nu.0) / sx, (n0.1 + nu.1) / sy],
        [(n0.0 + nv.0) / sx, (n0.1 + nv.1) / sy],
    ]
}

/// 角点 k 拖到 pt 后的框（roll_custom.resize_custom）。`side`：k 是边中点，只动那一边。
pub fn resize_custom(
    app: &App,
    orig: &[Pt],
    k: usize,
    pt: Pt,
    keep_shape: bool,
    side: bool,
) -> Vec<Pt> {
    resize_custom_xy(orig, k, pt, keep_shape, side, app.view.sx, app.view.sy)
}

/// 边 k（0 下、1 右、2 上、3 左）沿自己滑动 (db, dp) 后框的三个点（roll_custom.skew_custom 的纯逻辑）。
pub fn skew_custom_xy(orig: &[Pt], k: usize, db: f64, dp: f64, sx: f64, sy: f64) -> Vec<Pt> {
    let Some(c) = custom_corners(&Shape {
        pts: orig.to_vec(),
        ..Shape::default()
    }) else {
        return orig.to_vec();
    };
    let corners: [Pt; 4] = c.map(|q| [q[0] * sx, q[1] * sy]);
    let (ax, ay) = (corners[k % 4][0], corners[k % 4][1]);
    let (bx, by) = (corners[(k + 1) % 4][0], corners[(k + 1) % 4][1]);
    let ll = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
    if ll < 1e-9 {
        return orig.to_vec();
    }
    let (mx, my) = (db * sx, dp * sy);
    let t = (mx * (bx - ax) + my * (by - ay)) / ll / ll;
    let d = [t * (bx - ax) / sx, t * (by - ay) / sy];
    let moved: &[usize] = match k % 4 {
        0 => &[0, 1],
        1 => &[1],
        2 => &[2],
        _ => &[0, 2],
    };
    orig.iter()
        .enumerate()
        .map(|(i, p)| {
            if moved.contains(&i) {
                [p[0] + d[0], p[1] + d[1]]
            } else {
                *p
            }
        })
        .collect()
}

/// 边 k 沿自己滑动 (db, dp)（roll_custom.skew_custom）。
pub fn skew_custom(app: &App, orig: &[Pt], k: usize, db: f64, dp: f64) -> Vec<Pt> {
    skew_custom_xy(orig, k, db, dp, app.view.sx, app.view.sy)
}

/// 框的三个点绕盒子中间转 angle（弧度，屏幕上顺时针）（roll_custom.turn_custom 的纯逻辑）。
pub fn turn_custom_xy(orig: &[Pt], angle: f64, sx: f64, sy: f64) -> Vec<Pt> {
    if custom_corners(&Shape {
        pts: orig.to_vec(),
        ..Shape::default()
    })
    .is_none()
    {
        return orig.to_vec();
    }
    let pts: [Pt; 3] = [
        [orig[0][0] * sx, orig[0][1] * sy],
        [orig[1][0] * sx, orig[1][1] * sy],
        [orig[2][0] * sx, orig[2][1] * sy],
    ];
    let (x1, y1) = (pts[1][0], pts[1][1]);
    let (x2, y2) = (pts[2][0], pts[2][1]);
    let (cx, cy) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
    let (c, s) = ((-angle).cos(), (-angle).sin());
    (0..3)
        .map(|i| {
            let (x, y) = (pts[i][0], pts[i][1]);
            [
                (cx + (x - cx) * c - (y - cy) * s) / sx,
                (cy + (x - cx) * s + (y - cy) * c) / sy,
            ]
        })
        .collect()
}

/// 框的三个点绕盒子中间转 angle（roll_custom.turn_custom）。
pub fn turn_custom(app: &App, orig: &[Pt], angle: f64) -> Vec<Pt> {
    turn_custom_xy(orig, angle, app.view.sx, app.view.sy)
}

/// 鼠标绕盒子中间的角度（屏幕上顺时针）（roll_custom.screen_angle）。
pub fn screen_angle(app: &App, sh_pts: &[Pt], pos: Pos2) -> f64 {
    let (Some(b1), Some(b2)) = (sh_pts.get(1), sh_pts.get(2)) else {
        return 0.0;
    };
    let cx = app.view.x_of((b1[0] + b2[0]) / 2.0) as f64;
    let cy = app.view.y_of((b1[1] + b2[1]) / 2.0) as f64;
    (pos.y as f64 - cy).atan2(pos.x as f64 - cx)
}

/// 选中自定义形状的盒子：虚线轮廓、边中缩放方块、角点缩放方块（roll_custom.draw_custom_box）。
pub fn paint_custom_box(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let Some(corners) = screen_corners(app, sh) else {
        return;
    };
    let s = app.scale();
    let mut path: Vec<Pos2> = corners.to_vec();
    path.push(corners[0]);
    for seg in egui::Shape::dashed_line(&path, egui::Stroke::new(1.0, BOX_COLOR), 4.0, 3.0) {
        painter.add(seg);
    }
    let at = |p: Pos2| Pos2::new(rect.min.x + p.x, rect.min.y + p.y);
    let m = 3.0 * s;
    let r = 4.0 * s;
    for k in 0..4 {
        let mid = Pos2::new(
            (corners[k].x + corners[(k + 1) % 4].x) / 2.0,
            (corners[k].y + corners[(k + 1) % 4].y) / 2.0,
        );
        let q = Rect::from_center_size(at(mid), Vec2::splat(m * 2.0));
        painter.rect_filled(q, 0.0, Color32::WHITE);
        painter.rect_stroke(
            q,
            0.0,
            egui::Stroke::new(1.0, BOX_COLOR),
            egui::StrokeKind::Inside,
        );
    }
    for c in corners {
        let q = Rect::from_center_size(at(c), Vec2::splat(r * 2.0));
        painter.rect_filled(q, 0.0, Color32::WHITE);
        painter.rect_stroke(
            q,
            0.0,
            egui::Stroke::new(2.0, BOX_COLOR),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1x1 的正方形框（拍 / 音高）。
    fn square() -> Vec<Pt> {
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    }

    #[test]
    fn corners_go_round() {
        let Some(c) = custom_corners(&Shape {
            pts: square(),
            ..Shape::default()
        }) else {
            panic!("框应有三个点");
        };
        assert_eq!(c, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    }

    #[test]
    fn grid_aligned_only_for_flat_boxes() {
        assert!(grid_aligned(&square()));
        // 转过的盒子：u 不再沿时间
        let r = std::f64::consts::FRAC_1_SQRT_2;
        assert!(!grid_aligned(&[[0.0, 0.0], [r, r], [-r, r]]));
    }

    #[test]
    fn drag_steps_snap_to_grid_and_keys() {
        // 吸附 1/4 拍：db 取整到 0.25，dp 取整到 key
        assert_eq!(
            drag_steps_xy([0.0, 0.0], [0.4, 2.6], Some(0.25), false),
            (0.5, 3.0)
        );
        // Shift：自由
        assert_eq!(
            drag_steps_xy([0.0, 0.0], [0.4, 2.6], Some(0.25), true),
            (0.4, 2.6)
        );
        // 没有吸附：自由
        assert_eq!(
            drag_steps_xy([0.0, 0.0], [0.4, 2.6], None, false),
            (0.4, 2.6)
        );
    }

    #[test]
    fn skew_bottom_side_moves_bottom_two_points() {
        // 边 0（下）沿 +x 滑 0.5 拍（屏幕单位与拍相同）
        let got = skew_custom_xy(&square(), 0, 0.5, 0.0, 1.0, 1.0);
        assert_eq!(got, vec![[0.5, 0.0], [1.5, 0.0], [0.0, 1.0]]);
    }

    #[test]
    fn skew_right_side_moves_top_right_only() {
        let got = skew_custom_xy(&square(), 1, 0.0, 2.0, 1.0, 1.0);
        assert_eq!(got, vec![[0.0, 0.0], [1.0, 2.0], [0.0, 1.0]]);
    }

    #[test]
    fn skew_uses_screen_direction() {
        // 每拍 10px、每 key 5px：鼠标移 (5px, 10px) = (0.5 拍, 2 key)
        // 边 0 在屏幕上长 10px，鼠标位移在它上面的投影是 5px = 0.5 个边
        let got = skew_custom_xy(&square(), 0, 0.5, 2.0, 10.0, 5.0);
        assert_eq!(got, vec![[0.5, 0.0], [1.5, 0.0], [0.0, 1.0]]);
    }

    #[test]
    fn skew_degenerate_side_keeps_box() {
        let orig = vec![[0.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
        let got = skew_custom_xy(&orig, 0, 0.5, 0.0, 1.0, 1.0);
        assert_eq!(got, orig);
    }

    #[test]
    fn resize_corner_opposite_stays() {
        // 角 (u1,v0)（k=1）拖到 (3, -2)：对角 (u0,v1) = pts[2] 不动
        let got = resize_custom_xy(&square(), 1, [3.0, -2.0], false, false, 1.0, 1.0);
        assert_eq!(got[2], [0.0, 1.0]);
        // 新框：p0 = (0,-2)，u 到 (3,-2)，v 到 (0,1)
        assert_eq!(got[0], [0.0, -2.0]);
        assert_eq!(got[1], [3.0, -2.0]);
    }

    #[test]
    fn resize_side_locks_the_other_size() {
        // 右边 k=1 拖到 3 拍：左边（u 尺寸）不动
        let got = resize_custom_xy(&square(), 1, [3.0, 0.0], false, true, 1.0, 1.0);
        assert_eq!(got, vec![[0.0, 0.0], [3.0, 0.0], [0.0, 1.0]]);
    }

    #[test]
    fn turn_quarter_around_box_middle() {
        // 绕 (0.5, 0.5) 转 +90°：p1(1,0) -> (0,0)、p0(0,0) -> (0,1)、p2(0,1) -> (1,1)
        let got = turn_custom_xy(&square(), std::f64::consts::FRAC_PI_2, 1.0, 1.0);
        let eps = 1e-12;
        let close = |a: Pt, b: Pt| (a[0] - b[0]).abs() < eps && (a[1] - b[1]).abs() < eps;
        assert!(close(got[0], [0.0, 1.0]), "got {:?}", got);
        assert!(close(got[1], [0.0, 0.0]), "got {:?}", got);
        assert!(close(got[2], [1.0, 1.0]), "got {:?}", got);
    }
}
