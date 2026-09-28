//! 漏斗工具与编辑（原版 roll/roll_funnel.py，曲线形状在 panel_funnel / roll_menu）：
//! 两次点击画线 + 墙、在线上加起点、靠近曲线加锚点、拖曲线把手 / 起点、右键删点、
//! 高亮线与联动曲线（主 part 紫、联动的青）。

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::bezier;
use spiderweb_core::engine;
use spiderweb_core::funnel::{self, FunnelSettings, HandleId};
use spiderweb_core::shape::{FunnelCurve, Kind, Shape};
use spiderweb_io::project::FunnelDefaults;

use crate::app::{App, PartId};
use crate::roll::{Drag, event_pt};

/// 高亮的线 / 点中的曲线（原版 roll_draw.PART_COLOR）。
const PART_COLOR: Color32 = Color32::from_rgb(0x7a, 0x1f, 0xe0);
/// 与点中的曲线联动的曲线（原版 roll_draw.TWIN_COLOR）。
const TWIN_COLOR: Color32 = Color32::from_rgb(0x00, 0xa3, 0x9a);
/// 把手线与手柄的颜色（原版 HANDLE_COLOR）。
const HANDLE_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);
/// 普通点的方块描边（原版 #c00000）。
const POINT_COLOR: Color32 = Color32::from_rgb(0xc0, 0x00, 0x00);

// ---------------------------------------------------------------- 纯几何

/// 屏幕方向：x 沿 beat、y 沿 pitch（原版 FunnelEditing.screen_dir）。
fn screen_dir(sx: f64, sy: f64, a: Pt, b: Pt) -> [f64; 2] {
    [(b[0] - a[0]) * sx, (a[1] - b[1]) * sy]
}

/// 点到无限长直线（屏幕）的距离（原版 wall_dist）。
fn line_dist(dir: [f64; 2], origin: Pt, pt: Pt, sx: f64, sy: f64) -> f64 {
    let p = screen_dir(sx, sy, origin, pt);
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if len == 0.0 {
        0.0
    } else {
        (dir[0] * p[1] - dir[1] * p[0]).abs() / len
    }
}

/// 点到屏幕上线段的距离（原版 roll_funnel.seg_dist）。
fn seg_dist(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (ax, ay) = a;
    let (bx, by) = b;
    let (dx, dy) = (bx - ax, by - ay);
    let ll = dx * dx + dy * dy;
    let u = if ll == 0.0 {
        0.0
    } else {
        (((x - ax) * dx + (y - ay) * dy) / ll).clamp(0.0, 1.0)
    };
    ((x - ax - u * dx).powi(2) + (y - ay - u * dy).powi(2)).sqrt()
}

/// 画下的 [线起点, 线终点, 墙1, 墙2] 整理成 [线起, 线终, 墙1, 墙2]：先画的不算数，
/// 更立的那条是墙，线的起点在离墙远的一头；两条方向太近（小于约 12°）返回 None
/// （原版 FunnelEditing.arrange_funnel）。
fn arrange_funnel(sx: f64, sy: f64, pts: &[Pt]) -> Option<[Pt; 4]> {
    let p0 = *pts.first()?;
    let p1 = *pts.get(1)?;
    let p2 = *pts.get(2)?;
    let p3 = *pts.get(3)?;
    let a = screen_dir(sx, sy, p0, p1);
    let b = screen_dir(sx, sy, p2, p3);
    let la = (a[0] * a[0] + a[1] * a[1]).sqrt();
    let lb = (b[0] * b[0] + b[1] * b[1]).sqrt();
    if la == 0.0 || lb == 0.0 || (a[0] * b[1] - a[1] * b[0]).abs() < 0.2 * la * lb {
        return None;
    }
    let mut line = [p0, p1];
    let mut wall = [p2, p3];
    if a[1].abs() / la > b[1].abs() / lb {
        std::mem::swap(&mut line, &mut wall);
    }
    let dir = screen_dir(sx, sy, wall[0], wall[1]);
    if line_dist(dir, wall[0], line[0], sx, sy) < line_dist(dir, wall[0], line[1], sx, sy) {
        line.reverse();
    }
    Some([line[0], line[1], wall[0], wall[1]])
}

/// 两条无限长直线（beat, pitch）的交点（原版 FunnelEditing.crossing）。
fn crossing(line: [Pt; 2], wall: [Pt; 2]) -> Option<Pt> {
    let d1 = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
    let d2 = [wall[1][0] - wall[0][0], wall[1][1] - wall[0][1]];
    let det = d1[0] * d2[1] - d1[1] * d2[0];
    if det.abs() < 1e-12 {
        return None;
    }
    let t = ((wall[0][0] - line[0][0]) * d2[1] - (wall[0][1] - line[0][1]) * d2[0]) / det;
    Some([line[0][0] + t * d1[0], line[0][1] + t * d1[1]])
}

/// 拖墙端 `pt` 时，Ctrl 让墙另一端关于线与墙的交点对称（原版拖 2 / 3 点的做法）。
fn wall_mirror(line: [Pt; 2], fixed: Pt, pt: Pt) -> Option<Pt> {
    let mid = crossing(line, [fixed, pt])?;
    Some([2.0 * mid[0] - pt[0], 2.0 * mid[1] - pt[1]])
}

// ---------------------------------------------------------------- 设置转换

/// 形状的漏斗设置。
pub(crate) fn settings_of_shape(sh: &Shape) -> FunnelSettings {
    FunnelSettings {
        fill: sh.funnel_fill,
        gate0: sh.gate0,
        gate1: sh.gate1,
        vary: sh.vary,
        change: sh.change,
        follow: sh.follow,
        wall: sh.wall,
    }
}

/// 新漏斗的默认设置。
pub(crate) fn settings_of_defaults(d: &FunnelDefaults) -> FunnelSettings {
    FunnelSettings {
        fill: d.fill,
        gate0: d.gate0,
        gate1: d.gate1,
        vary: d.vary,
        change: d.change,
        follow: d.follow,
        wall: d.wall,
    }
}

/// 把漏斗设置写进形状。
pub(crate) fn apply_settings(sh: &mut Shape, s: FunnelSettings) {
    sh.funnel_fill = s.fill;
    sh.gate0 = s.gate0;
    sh.gate1 = s.gate1;
    sh.vary = s.vary;
    sh.change = s.change;
    sh.follow = s.follow;
    sh.wall = s.wall;
}

/// 新漏斗的默认：形状默认值 + 漏斗面板设置（原版 new_defaults("funnel")）。
pub(crate) fn new_funnel_defaults(app: &App) -> Shape {
    let mut d = app.defaults.clone();
    apply_settings(&mut d, settings_of_defaults(&app.funnel_defaults));
    d
}

// ---------------------------------------------------------------- 小工具

/// 曲线（起点 k 通向墙端 end）的只读引用。
fn curve_ref(sh: &Shape, k: usize, end: usize) -> Option<&FunnelCurve> {
    sh.starts
        .get(k)
        .and_then(|st| st.ends.get(end))
        .and_then(|c| c.as_ref())
}

/// 曲线（起点 k 通向墙端 end）的可变引用。
fn curve_mut(sh: &mut Shape, k: usize, end: usize) -> Option<&mut FunnelCurve> {
    sh.starts
        .get_mut(k)
        .and_then(|st| st.ends.get_mut(end))
        .and_then(|c| c.as_mut())
}

/// 在曲线第 seg 段的 t 处加锚点，形状不变（原版 FunnelEditing.add_anchor）。
fn add_anchor(c: &mut FunnelCurve, seg: usize, t: f64) {
    c.pts = bezier::split(&c.pts, seg, t);
    c.sharp = c
        .sharp
        .iter()
        .map(|&a| if a > seg { a + 1 } else { a })
        .collect();
}

/// 去掉曲线的锚点 a（原版 FunnelEditing.drop_anchor）。
fn drop_anchor(c: &mut FunnelCurve, a: usize) {
    c.pts = bezier::remove_anchor(&c.pts, a);
    c.sharp = c
        .sharp
        .iter()
        .filter_map(|&b| {
            if b == a {
                None
            } else if b > a {
                Some(b - 1)
            } else {
                Some(b)
            }
        })
        .collect();
}

/// 联动曲线跟上刚改过的点（原版 sync_linked）：Ctrl 时不动联动曲线；
/// 点号按 flip 首尾映射，尖角跟着走。
fn sync_linked(
    sh: &mut Shape,
    k: usize,
    end: usize,
    before: &[Pt],
    sharp_before: &[usize],
    ctrl: bool,
) {
    if ctrl {
        return;
    }
    let Some(c) = curve_ref(sh, k, end).cloned() else {
        return;
    };
    let n = c.pts.len();
    let changed: Vec<usize> = (0..n.min(before.len()))
        .filter(|&j| before[j] != c.pts[j])
        .collect();
    let sharp_changed = c.sharp != sharp_before;
    for (k2, e2, flip) in funnel::partners(sh, k, end) {
        let mine = funnel::turned_curve(&c, flip);
        let Some(c2) = curve_mut(sh, k2, e2) else {
            continue;
        };
        if c2.pts.len() != n {
            continue; // Ctrl 弄成不一样的：别动
        }
        for &j in &changed {
            let jj = if flip { n - 1 - j } else { j };
            if let Some(p) = mine.pts.get(jj) {
                c2.pts[jj] = *p;
            }
        }
        if sharp_changed {
            c2.sharp = mine.sharp.clone();
        }
    }
}

/// 线上离鼠标最近的位置 0..1（吸附后投影到线上）；`near` 给定时离线段太远返回 None
/// （原版 line_at）。
fn line_at(
    app: &App,
    sh: &Shape,
    line: usize,
    pos: Pos2,
    shift: bool,
    near: Option<f32>,
) -> Option<f64> {
    let i = funnel::line_index(line);
    let a = *sh.pts.get(i)?;
    let b = *sh.pts.get(i + 1)?;
    let (xa, ya) = (app.view.x_of(a[0]), app.view.y_of(a[1]));
    let (xb, yb) = (app.view.x_of(b[0]), app.view.y_of(b[1]));
    let (dx, dy) = (xb - xa, yb - ya);
    let ll = dx * dx + dy * dy;
    if ll == 0.0 {
        return None;
    }
    let along = |x: f32, y: f32| (((x - xa) * dx + (y - ya) * dy) / ll).clamp(0.0, 1.0);
    let u = along(pos.x, pos.y);
    if let Some(near) = near
        && ((pos.x - xa - u * dx).powi(2) + (pos.y - ya - u * dy).powi(2)).sqrt() > near
    {
        return None;
    }
    let pt = event_pt(app, pos, true, shift);
    Some(along(app.view.x_of(pt[0]), app.view.y_of(pt[1])) as f64)
}

/// (beat, pitch) 已映射到屏幕的点是否落在漏斗的墙上（原版 near_wall）。
fn near_wall(app: &App, sh: &Shape, pt: Pt) -> bool {
    let (Some(w0), Some(w1)) = (sh.pts.get(2), sh.pts.get(3)) else {
        return false;
    };
    seg_dist(
        app.view.x_of(pt[0]),
        app.view.y_of(pt[1]),
        (app.view.x_of(w0[0]), app.view.y_of(w0[1])),
        (app.view.x_of(w1[0]), app.view.y_of(w1[1])),
    ) <= 8.0
}

// ---------------------------------------------------------------- 画线画墙

/// Funnel 工具按下（原版 on_press）：还没有线就起一条线；线画好（两个点）就接着画墙。
pub fn funnel_press(app: &mut App, pos: Pos2, shift: bool) {
    let pt = event_pt(app, pos, true, shift);
    let waiting_wall = app
        .draft
        .as_ref()
        .is_some_and(|d| d.kind == Kind::Funnel && d.pts.len() == 2);
    if waiting_wall {
        if let Some(d) = app.draft.as_mut() {
            d.pts.push(pt);
            d.pts.push(pt);
        }
        app.drag = Some(Drag::Wall { screen: pos });
        return;
    }
    let defaults = new_funnel_defaults(app);
    app.draft = Some(engine::make_shape(Kind::Funnel, &[pt, pt], &defaults));
    app.drag = Some(Drag::Create {
        start: pt,
        screen: pos,
    });
}

/// 拖动正在画的墙：更新第二个墙端；Ctrl 让第一个墙端关于线对称（原版 wall 拖动）。
pub fn funnel_drag_wall(app: &mut App, pt: Pt, ctrl: bool) {
    let Some(d) = app.draft.as_mut() else {
        return;
    };
    if d.kind != Kind::Funnel || d.pts.len() < 4 {
        return;
    }
    d.pts[3] = pt;
    if ctrl && let Some(other) = wall_mirror([d.pts[0], d.pts[1]], d.pts[2], pt) {
        d.pts[2] = other;
    }
}

/// 松开正在画的墙（原版 on_release 的 wall 分支）：原地单击 / 墙不能与线交叉时退回等墙；
/// 否则整理顺序，大漏斗先确认再提交。
pub fn funnel_finish_wall(app: &mut App, still: bool) {
    let Some(d) = app.draft.as_ref() else {
        return;
    };
    if d.kind != Kind::Funnel || d.pts.len() < 4 {
        return;
    }
    let pts = d.pts.clone();
    if still {
        truncate_wall(app);
        return;
    }
    let Some(arr) = arrange_funnel(app.view.sx, app.view.sy, &pts) else {
        truncate_wall(app);
        app.status = rust_i18n::t!("status.wall_must_cross").to_string();
        return;
    };
    if let Some(d) = app.draft.as_mut() {
        d.pts = arr.to_vec();
    }
    if app.confirm_big_draft() {
        app.commit_draft();
    }
}

/// 退回"只差墙"的状态（删掉 draft 里的两个墙端点）。
fn truncate_wall(app: &mut App) {
    if let Some(d) = app.draft.as_mut() {
        d.pts.truncate(2);
    }
}

/// 松开 Funnel 的线：画到选中漏斗的墙上就变成它的一条新线，否则留着 draft 等墙
/// （原版 on_release 的 create 分支）。
pub fn funnel_finish_line(app: &mut App) {
    let _ = add_funnel_line(app);
}

/// 刚画的线落在选中漏斗的墙上：加为那个漏斗的一条新线（原版 add_funnel_line）。
fn add_funnel_line(app: &mut App) -> bool {
    let Some(sel) = app.sel else {
        return false;
    };
    let Some(sh) = app.shapes.get(sel) else {
        return false;
    };
    if sh.kind != Kind::Funnel || sh.pts.len() < 4 {
        return false;
    }
    let pts = app
        .draft
        .as_ref()
        .map(|d| d.pts.clone())
        .unwrap_or_default();
    if pts.len() != 2 {
        return false;
    }
    let (a, b) = (pts[0], pts[1]);
    if !near_wall(app, sh, a) && !near_wall(app, sh, b) {
        return false;
    }
    let wall = [sh.pts[2], sh.pts[3]];
    let Some(arr) = arrange_funnel(app.view.sx, app.view.sy, &[a, b, wall[0], wall[1]]) else {
        return false;
    };
    if arr[2] != wall[0] || arr[3] != wall[1] {
        return false; // 画出来的墙跟原来的不一样：不算同一条线
    }
    app.cancel_draft();
    app.push_undo();
    if let Some(sh) = app.shapes.get_mut(sel) {
        sh.pts.push(arr[0]);
        sh.pts.push(arr[1]);
    }
    app.shapes_changed();
    true
}

// ---------------------------------------------------------------- 拖把手

/// 一次曲线点拖动需要的一切。
struct CurveDrag {
    k: usize,
    end: usize,
    pi: usize,
    anchor: bool,
    new: Pt,
    alt: bool,
    ctrl: bool,
    box_: (Pt, Pt, Pt),
}

/// 拖一个曲线锚点或手柄（原版 drag_funnel 的 anchor / ctrl 分支）。
fn drag_curve_point(app: &App, sh: &mut Shape, d: &CurveDrag) {
    let Some(c) = curve_mut(sh, d.k, d.end) else {
        return;
    };
    let before = c.pts.clone();
    let sharp_before = c.sharp.clone();
    let uf_screen = |p: Pt| {
        let q = funnel::box_point(&d.box_, p[0], p[1]);
        (app.view.x_of(q[0]), app.view.y_of(q[1]))
    };
    if d.anchor {
        if d.alt {
            // Alt：从锚点两侧拉出新手柄，锚点变成平滑的
            if d.pi > 0 && d.pi + 1 < c.pts.len() {
                let a = c.pts[d.pi];
                let (dx, dy) = (d.new[0] - a[0], d.new[1] - a[1]);
                c.pts[d.pi + 1] = [a[0] + dx, a[1] + dy];
                c.pts[d.pi - 1] = [a[0] - dx, a[1] - dy];
                c.sharp.retain(|&a| a != d.pi / 3);
            }
        } else {
            // 锚点带着两侧手柄一起走
            let a = c.pts[d.pi];
            let (dx, dy) = (d.new[0] - a[0], d.new[1] - a[1]);
            for j in d.pi.saturating_sub(1)..=(d.pi + 1) {
                if let Some(p) = c.pts.get_mut(j) {
                    *p = [p[0] + dx, p[1] + dy];
                }
            }
        }
    } else {
        c.pts[d.pi] = d.new;
        let a = bezier::handle_anchor(d.pi);
        let middle = a > 0 && a + 1 < c.pts.len();
        if middle && !c.sharp.contains(&(a / 3)) {
            if d.alt {
                // Alt：只动这一根，锚点变尖角
                c.sharp.push(a / 3);
                c.sharp.sort_unstable();
            } else {
                // 平滑锚点：另一根手柄反方向、保持屏幕长度
                let other = 2 * a - d.pi;
                if let (Some(ap), Some(hp), Some(op)) = (
                    c.pts.get(a).copied(),
                    c.pts.get(d.pi).copied(),
                    c.pts.get(other).copied(),
                ) {
                    let (ax, ay) = uf_screen(ap);
                    let (hx, hy) = uf_screen(hp);
                    let (ox, oy) = uf_screen(op);
                    let dist = ((hx - ax).powi(2) + (hy - ay).powi(2)).sqrt();
                    let length = ((ox - ax).powi(2) + (oy - ay).powi(2)).sqrt();
                    if dist > 0.0 && length > 0.0 {
                        let tx = ax - (hx - ax) / dist * length;
                        let ty = ay - (hy - ay) / dist * length;
                        if let Some(p) = c.pts.get_mut(other) {
                            *p = funnel::box_uf(&d.box_, app.view.b_of(tx), app.view.p_of(ty));
                        }
                    }
                }
            }
        }
    }
    sync_linked(sh, d.k, d.end, &before, &sharp_before, d.ctrl);
}

/// 拖动漏斗把手（原版 drag_funnel）：
/// `i < pts.len()` 是普通点（线 / 墙端；墙端 2 / 3 时 Ctrl 关于线对称）；
/// 否则是曲线把手 / 起点，`i - pts.len()` 索引 [`funnel::funnel_handles`]。
pub fn funnel_drag_handle(app: &mut App, i: usize, pos: Pos2, shift: bool, alt: bool, ctrl: bool) {
    let Some(sel) = app.sel else {
        return;
    };
    let Some(sh0) = app.shapes.get(sel) else {
        return;
    };
    if sh0.kind != Kind::Funnel {
        return;
    }
    let n = sh0.pts.len();
    let mut sh = sh0.clone();
    if i < n {
        let pt = event_pt(app, pos, true, shift);
        if i >= sh.pts.len() {
            return;
        }
        sh.pts[i] = pt;
        if (i == 2 || i == 3) && ctrl {
            let other = 5 - i;
            if let Some(mirror) = wall_mirror([sh.pts[0], sh.pts[1]], sh.pts[other], pt) {
                sh.pts[other] = mirror;
            }
        }
    } else {
        let Some(hid) = funnel::funnel_handles(&sh)
            .into_iter()
            .nth(i - n)
            .map(|(_, id)| id)
        else {
            return;
        };
        match hid {
            HandleId::Start(k) => {
                let Some(st) = sh.starts.get(k) else {
                    return;
                };
                let line = st.line;
                let Some(at) = line_at(app, &sh, line, pos, shift, None) else {
                    return;
                };
                if let Some(st) = sh.starts.get_mut(k) {
                    st.at = at;
                }
            }
            HandleId::Ctrl(k, end, pi) | HandleId::Anchor(k, end, pi) => {
                let anchor = matches!(hid, HandleId::Anchor(..));
                let Some(st) = sh.starts.get(k) else {
                    return;
                };
                let Some(box_) = funnel::curve_box(&sh, st.at, end, st.line) else {
                    return;
                };
                let new = funnel::box_uf(&box_, app.view.b_of(pos.x), app.view.p_of(pos.y));
                let d = CurveDrag {
                    k,
                    end,
                    pi,
                    anchor,
                    new,
                    alt,
                    ctrl,
                    box_,
                };
                drag_curve_point(app, &mut sh, &d);
            }
        }
    }
    if let Some(dst) = app.shapes.get_mut(sel) {
        *dst = sh;
    }
    app.shapes_changed();
}

/// 选中漏斗的把手命中：曲线把手 / 起点（任何工具）与普通点（只有 Select 工具）相比，
/// 返回 [`funnel_drag_handle`] 用的合并编号（原版 hit_handle + handles）。
pub fn hit_funnel_handle(app: &App, sh: &Shape, p: Pos2, any: bool) -> Option<usize> {
    if sh.kind != Kind::Funnel {
        return None;
    }
    let near = 7.0_f32.max(8.0 * app.scale());
    let hs = funnel::funnel_handles(sh);
    for (idx, (pt, _)) in hs.iter().enumerate().rev() {
        let x = app.view.x_of(pt[0]);
        let y = app.view.y_of(pt[1]);
        if (x - p.x).abs() <= near && (y - p.y).abs() <= near {
            return Some(sh.pts.len() + idx);
        }
    }
    if any {
        for (i, pt) in sh.pts.iter().enumerate().rev() {
            let x = app.view.x_of(pt[0]);
            let y = app.view.y_of(pt[1]);
            if (x - p.x).abs() <= near && (y - p.y).abs() <= near {
                return Some(i);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- 加起点 / 锚点

/// 中键 / 面板在选中的漏斗上操作（原版 funnel_click）：点在线上 = 那里加一个起点；
/// 否则在最近的曲线上加一个锚点并拖到点击处（联动曲线也加）。true = 加上了。
pub fn funnel_click(app: &mut App, pos: Pos2, shift: bool, ctrl: bool) -> bool {
    let Some(sel) = app.sel else {
        return false;
    };
    let Some(mut sh) = app.shapes.get(sel).cloned() else {
        return false;
    };
    if sh.kind != Kind::Funnel || sh.pts.len() < 4 {
        return false;
    }

    // 1) 在线上：新起点
    for line in 0..funnel::funnel_lines(&sh).len() {
        if let Some(at) = line_at(app, &sh, line, pos, shift, Some(6.0)) {
            let Some(st) = funnel::new_start(&sh, at, line) else {
                return false;
            };
            app.push_undo();
            if let Some(shm) = app.shapes.get_mut(sel) {
                shm.starts.push(st);
            }
            app.shapes_changed();
            // 原版这里弹 "funnel_links" 的 tip（help.rs 的 tips），没有状态栏文案
            return true;
        }
    }

    // 2) 靠近某条曲线：加锚点
    let curves = funnel::funnel_curves(&sh, false);
    if curves.is_empty() {
        return false;
    }
    let pad = 40.0_f32;
    let (mut lo_x, mut hi_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut lo_y, mut hi_y) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut grow = |p: Pt| {
        let (x, y) = (app.view.x_of(p[0]), app.view.y_of(p[1]));
        lo_x = lo_x.min(x);
        hi_x = hi_x.max(x);
        lo_y = lo_y.min(y);
        hi_y = hi_y.max(y);
    };
    for p in &sh.pts {
        grow(*p);
    }
    for (_, _, c, _) in &curves {
        for p in c {
            grow(*p);
        }
    }
    if pos.x < lo_x - pad || pos.x > hi_x + pad || pos.y < lo_y - pad || pos.y > hi_y + pad {
        return false;
    }

    let mut best: Option<(f64, PartId, usize, f64)> = None;
    for &(k, end, ref c, _) in &curves {
        let st = &sh.starts[k];
        let Some(box_) = funnel::curve_box(&sh, st.at, end, st.line) else {
            continue;
        };
        let to_screen = |p: Pt| {
            let q = funnel::box_point(&box_, p[0], p[1]);
            [app.view.x_of(q[0]) as f64, app.view.y_of(q[1]) as f64]
        };
        let Some((seg, t, d)) = bezier::nearest(c, &to_screen, pos.x as f64, pos.y as f64, 64)
        else {
            continue;
        };
        if best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, PartId::Curve(k, end), seg, t));
        }
    }
    let Some((_, PartId::Curve(k, end), seg, t)) = best else {
        return false;
    };

    let st = sh.starts[k].clone();
    let Some(box_) = funnel::curve_box(&sh, st.at, end, st.line) else {
        return false;
    };
    let n_before = curve_ref(&sh, k, end).map(|c| c.pts.len()).unwrap_or(0);
    let i = 3 * (seg + 1);
    {
        let Some(c) = curve_mut(&mut sh, k, end) else {
            return false;
        };
        add_anchor(c, seg, t);
        let target = funnel::box_uf(&box_, app.view.b_of(pos.x), app.view.p_of(pos.y));
        if i < c.pts.len() {
            let (dx, dy) = (target[0] - c.pts[i][0], target[1] - c.pts[i][1]);
            for j in i.saturating_sub(1)..=(i + 1) {
                if let Some(p) = c.pts.get_mut(j) {
                    *p = [p[0] + dx, p[1] + dy];
                }
            }
        }
    }
    let Some(updated) = curve_ref(&sh, k, end).cloned() else {
        return false;
    };
    if !ctrl {
        for (k2, e2, flip) in funnel::partners(&sh, k, end) {
            let Some(c2) = curve_mut(&mut sh, k2, e2) else {
                continue;
            };
            if c2.pts.len() != n_before {
                continue;
            }
            let nseg = bezier::segments(&c2.pts).len();
            add_anchor(
                c2,
                if flip { nseg - 1 - seg } else { seg },
                if flip { 1.0 - t } else { t },
            );
            let mine = funnel::turned_curve(&updated, flip);
            let ii = if flip { c2.pts.len() - 1 - i } else { i };
            if ii >= 1 && ii + 1 < c2.pts.len() {
                for j in ii - 1..=ii + 1 {
                    if let Some(p) = mine.pts.get(j) {
                        c2.pts[j] = *p;
                    }
                }
            }
        }
    }
    app.push_undo();
    app.shapes[sel] = sh;
    app.shapes_changed();
    true
}

// ---------------------------------------------------------------- 高亮 part

/// 屏幕上命中漏斗的线或曲线（原版 part_at）：先线后曲线，取距离最小。
fn part_at_screen(
    sh: &Shape,
    to_screen: &dyn Fn(Pt) -> (f32, f32),
    x: f32,
    y: f32,
    near: f32,
) -> Option<PartId> {
    if sh.pts.len() < 4 {
        return None;
    }
    let mut best: Option<PartId> = None;
    let mut best_d = near;
    for (n, seg) in funnel::funnel_lines(sh).iter().enumerate() {
        let d = seg_dist(x, y, to_screen(seg[0]), to_screen(seg[1]));
        if d < best_d {
            best = Some(PartId::Line(n));
            best_d = d;
        }
    }
    for (k, end, curve, _) in funnel::funnel_curves(sh, false) {
        let d = curve
            .windows(2)
            .map(|w| seg_dist(x, y, to_screen(w[0]), to_screen(w[1])))
            .fold(f32::INFINITY, f32::min);
        if d < best_d {
            best = Some(PartId::Curve(k, end));
            best_d = d;
        }
    }
    best
}

/// 选中的漏斗里，鼠标下（near 像素内）的线或曲线，用于再次点击时高亮。
pub fn part_at(app: &App, sh: &Shape, p: Pos2, near: f32) -> Option<PartId> {
    let to_screen = |q: Pt| (app.view.x_of(q[0]), app.view.y_of(q[1]));
    part_at_screen(sh, &to_screen, p.x, p.y, near)
}

/// 点中的 part 会带上的联动曲线（原版 part_group）。
pub fn part_group(sh: &Shape, part: PartId) -> BTreeSet<PartId> {
    let mut out = BTreeSet::new();
    out.insert(part);
    if let PartId::Curve(k, end) = part {
        for (k2, e2, _) in funnel::partners(sh, k, end) {
            out.insert(PartId::Curve(k2, e2));
        }
    }
    out
}

// ---------------------------------------------------------------- 删点

/// 右键漏斗的曲线起点 / 锚点 / 手柄（原版 delete_funnel_handle）：
/// 起点连同它的曲线一起删；锚点删掉；手柄收回锚点（那里变成尖角）；联动曲线同样处理，
/// Ctrl 时不动它们。`i` 是 [`hit_funnel_handle`] / [`funnel_drag_handle`] 的合并编号。
pub fn funnel_delete_handle(app: &mut App, i: usize, ctrl: bool) -> bool {
    let Some(sel) = app.sel else {
        return false;
    };
    let Some(sh0) = app.shapes.get(sel).cloned() else {
        return false;
    };
    if sh0.kind != Kind::Funnel {
        return false;
    }
    let n = sh0.pts.len();
    if i < n {
        return false;
    }
    let Some(hid) = funnel::funnel_handles(&sh0)
        .into_iter()
        .nth(i - n)
        .map(|(_, id)| id)
    else {
        return false;
    };

    if let HandleId::Start(k) = hid {
        if k >= sh0.starts.len() {
            return false;
        }
        app.push_undo();
        if let Some(sh) = app.shapes.get_mut(sel) {
            sh.starts.remove(k);
        }
        app.parts.clear();
        app.part_main = None;
        app.shapes_changed();
        return true;
    }

    let mut sh = sh0;
    match hid {
        HandleId::Ctrl(k, end, pi) => {
            let a = bezier::handle_anchor(pi);
            let Some(c) = curve_ref(&sh, k, end) else {
                return false;
            };
            if a == 0 || a + 1 == c.pts.len() {
                return true; // 两端的手柄留着（原版直接返回）
            }
            let before = c.pts.clone();
            let sharp_before = c.sharp.clone();
            if let Some(c) = curve_mut(&mut sh, k, end) {
                c.pts[pi] = c.pts[a];
                if !c.sharp.contains(&(a / 3)) {
                    c.sharp.push(a / 3);
                    c.sharp.sort_unstable();
                }
            }
            sync_linked(&mut sh, k, end, &before, &sharp_before, ctrl);
        }
        HandleId::Anchor(k, end, pi) => {
            let Some(c) = curve_ref(&sh, k, end) else {
                return false;
            };
            let na = bezier::anchor_count(&c.pts);
            let ai = pi / 3;
            if ai == 0 || ai + 1 >= na {
                return true; // 两端的锚点不删（原版直接返回）
            }
            let clen = c.pts.len();
            if !ctrl {
                for (k2, e2, flip) in funnel::partners(&sh, k, end) {
                    let Some(c2) = curve_mut(&mut sh, k2, e2) else {
                        continue;
                    };
                    if c2.pts.len() == clen {
                        drop_anchor(c2, if flip { na - 1 - ai } else { ai });
                    }
                }
            }
            if let Some(c) = curve_mut(&mut sh, k, end) {
                drop_anchor(c, ai);
            }
        }
        HandleId::Start(_) => return false,
    }
    app.push_undo();
    if let Some(dst) = app.shapes.get_mut(sel) {
        *dst = sh;
    }
    app.shapes_changed();
    true
}

// ---------------------------------------------------------------- 高亮 part 的 App 状态

/// 高亮的漏斗 part：`(形状下标, 线的编号, 曲线 (起点号, 墙端))`。
pub type FunnelParts = (usize, BTreeSet<usize>, BTreeSet<(usize, usize)>);

impl App {
    /// 高亮的漏斗线与曲线（原版 funnel_parts）。
    pub fn funnel_parts(&self) -> Option<FunnelParts> {
        if self.parts.is_empty() {
            return None;
        }
        let sel = self.sel?;
        let sh = self.shapes.get(sel)?;
        if sh.kind != Kind::Funnel || self.sels.len() != 1 {
            return None;
        }
        let nlines = funnel::funnel_lines(sh).len();
        let mut lines = BTreeSet::new();
        let mut curves = BTreeSet::new();
        for part in &self.parts {
            match *part {
                PartId::Line(n) if n < nlines => {
                    lines.insert(n);
                }
                PartId::Curve(k, end) if curve_ref(sh, k, end).is_some() => {
                    curves.insert((k, end));
                }
                _ => {}
            }
        }
        if lines.is_empty() && curves.is_empty() {
            None
        } else {
            Some((sel, lines, curves))
        }
    }

    /// 高亮这些线 / 曲线（原版 set_parts）：main = 点中的那个，联动的换颜色；
    /// main 为 None 且 parts 为空时连 main 一起清掉。
    pub fn set_parts(&mut self, parts: BTreeSet<PartId>, main: Option<PartId>) {
        if main.is_some() || parts.is_empty() {
            self.part_main = main;
        }
        if parts != self.parts {
            self.parts = parts;
        }
    }

    /// Ctrl+点击：加上或去掉一个 part（原版 `app.parts ^ {part}`）。
    pub fn toggle_part(&mut self, part: PartId) {
        let mut parts = self.parts.clone();
        if !parts.remove(&part) {
            parts.insert(part);
        }
        self.parts = parts;
        if self.parts.is_empty() {
            self.part_main = None;
        }
    }

    /// 高亮内容的文字："2 curves and 1 line"（原版 parts_text）。
    pub fn parts_text(&self) -> String {
        let Some((_, lines, curves)) = self.funnel_parts() else {
            return String::new();
        };
        let mut words: Vec<String> = Vec::new();
        if !curves.is_empty() {
            words.push(format!(
                "{} curve{}",
                curves.len(),
                if curves.len() > 1 { "s" } else { "" }
            ));
        }
        if !lines.is_empty() {
            words.push(format!(
                "{} line{}",
                lines.len(),
                if lines.len() > 1 { "s" } else { "" }
            ));
        }
        words.join(" and ")
    }

    /// Delete：删掉高亮的线与曲线；所有线都在时整个漏斗删掉（原版 delete_parts）。
    /// true = 处理了（Delete 不再删形状）。
    pub fn delete_funnel_parts(&mut self) -> bool {
        let Some((sel, lines, curves)) = self.funnel_parts() else {
            return false;
        };
        let Some(sh) = self.shapes.get(sel) else {
            return false;
        };
        if lines.len() >= funnel::funnel_lines(sh).len() {
            self.delete_selected();
            return true;
        }
        let lines: Vec<usize> = lines.into_iter().collect();
        let curves: Vec<(usize, usize)> = curves.into_iter().collect();
        self.push_undo();
        if let Some(sh) = self.shapes.get_mut(sel) {
            funnel::remove_funnel_parts(sh, &lines, &curves);
        }
        self.parts.clear();
        self.part_main = None;
        self.shapes_changed();
        true
    }

    /// 给高亮的曲线换形状（原版 set_curves）：联动曲线按 link 同形或首尾对调，只换一遍。
    pub fn set_funnel_curves(
        &mut self,
        shape_of: &dyn Fn(&FunnelCurve) -> FunnelCurve,
        undo: bool,
    ) {
        let Some((sel, _, curves)) = self.funnel_parts() else {
            return;
        };
        if curves.is_empty() {
            return;
        }
        if undo {
            self.push_undo();
        }
        let Some(mut sh) = self.shapes.get(sel).cloned() else {
            return;
        };
        let mut done: BTreeSet<(usize, usize)> = BTreeSet::new();
        for &(k, end) in &curves {
            if done.contains(&(k, end)) {
                continue;
            }
            let Some(new) = curve_ref(&sh, k, end).map(shape_of) else {
                continue;
            };
            if let Some(c) = curve_mut(&mut sh, k, end) {
                funnel::set_shape(c, &new);
            }
            done.insert((k, end));
            for (k2, e2, flip) in funnel::partners(&sh, k, end) {
                if curves.contains(&(k2, e2)) && !done.contains(&(k2, e2)) {
                    let turned = funnel::turned_curve(&new, flip);
                    if let Some(c2) = curve_mut(&mut sh, k2, e2) {
                        funnel::set_shape(c2, &turned);
                    }
                    done.insert((k2, e2));
                }
            }
        }
        self.shapes[sel] = sh;
        self.shapes_changed();
    }
}

// ---------------------------------------------------------------- 绘制

/// 高亮曲线到颜色的映射：点中的那条紫、联动的青（原版 part_colors）。
fn part_colors(app: &App, curves: &BTreeSet<(usize, usize)>) -> BTreeMap<(usize, usize), Color32> {
    let main = match app.part_main {
        Some(PartId::Curve(k, e)) if curves.contains(&(k, e)) => Some((k, e)),
        _ => None,
    };
    curves
        .iter()
        .map(|&c| {
            (
                c,
                if main.is_none() || main == Some(c) {
                    PART_COLOR
                } else {
                    TWIN_COLOR
                },
            )
        })
        .collect()
}

/// 画高亮的线与曲线：粗紫 / 青（原版 draw_parts），在把手下面。
pub fn paint_parts(app: &App, painter: &egui::Painter, rect: Rect) {
    let Some((sel, lines, curves)) = app.funnel_parts() else {
        return;
    };
    let Some(sh) = app.shapes.get(sel) else {
        return;
    };
    let colors = part_colors(app, &curves);
    let width = 4.0_f32.max((4.0 * app.scale()).round());
    let at = |p: Pt| {
        Pos2::new(
            rect.min.x + app.view.x_of(p[0]),
            rect.min.y + app.view.y_of(p[1]),
        )
    };
    for n in &lines {
        if let Some(seg) = funnel::funnel_lines(sh).get(*n) {
            painter.line_segment([at(seg[0]), at(seg[1])], Stroke::new(width, PART_COLOR));
        }
    }
    for (k, end, path, _) in funnel::funnel_curves(sh, false) {
        if let Some(color) = colors.get(&(k, end)) {
            let pts: Vec<Pos2> = path.iter().map(|&p| at(p)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(width, *color)));
        }
    }
}

/// 画选中漏斗的把手（原版 draw_handles 的 funnel 分支）：把手线白边彩芯、普通点白方块红边、
/// 起点白菱形蓝边、锚点白圆、手柄实心圆（高亮曲线的手柄用紫 / 青）。
pub fn paint_funnel_handles(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let s = app.scale();
    let at = |p: Pt| {
        Pos2::new(
            rect.min.x + app.view.x_of(p[0]),
            rect.min.y + app.view.y_of(p[1]),
        )
    };
    let curves = app.funnel_parts().map(|(_, _, c)| c).unwrap_or_default();
    let colors = part_colors(app, &curves);
    // 把手线：白的先画，高亮的彩芯压在上面
    let mut lines: Vec<(Pt, Pt, Color32)> = funnel::funnel_handle_lines(sh)
        .into_iter()
        .map(|(a, h, c)| (a, h, colors.get(&c).copied().unwrap_or(HANDLE_COLOR)))
        .collect();
    lines.sort_by_key(|l| l.2 != HANDLE_COLOR);
    for (a, h, _) in &lines {
        painter.line_segment(
            [at(*a), at(*h)],
            Stroke::new((3.5 * s).max(3.0), Color32::WHITE),
        );
    }
    for (a, h, c) in &lines {
        painter.line_segment([at(*a), at(*h)], Stroke::new((1.5 * s).max(1.0), *c));
    }
    let r = 4.0 * s;
    for p in &sh.pts {
        let q = Rect::from_center_size(at(*p), Vec2::splat(r * 2.0));
        painter.rect_filled(q, 0.0, Color32::WHITE);
        painter.rect_stroke(
            q,
            0.0,
            Stroke::new(2.0, POINT_COLOR),
            egui::StrokeKind::Inside,
        );
    }
    let mut handles = funnel::funnel_handles(sh);
    handles.sort_by_key(|(_, id)| match id {
        // 高亮曲线的手柄画在最后（压在别的上面）
        HandleId::Ctrl(k, e, _) | HandleId::Anchor(k, e, _) => colors.contains_key(&(*k, *e)),
        HandleId::Start(_) => false,
    });
    for (p, id) in handles {
        let pos = at(p);
        let color = match &id {
            HandleId::Ctrl(k, e, _) | HandleId::Anchor(k, e, _) => {
                colors.get(&(*k, *e)).copied().unwrap_or(HANDLE_COLOR)
            }
            HandleId::Start(_) => HANDLE_COLOR,
        };
        match id {
            HandleId::Start(_) => {
                let d = r + 2.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(pos.x, pos.y - d),
                        Pos2::new(pos.x + d, pos.y),
                        Pos2::new(pos.x, pos.y + d),
                        Pos2::new(pos.x - d, pos.y),
                    ],
                    Color32::WHITE,
                    Stroke::new(2.0, HANDLE_COLOR),
                ));
            }
            HandleId::Anchor(..) => {
                let q = r + 1.5 * s;
                painter.circle_filled(pos, q, Color32::WHITE);
                painter.circle_stroke(pos, q, Stroke::new(2.0, color));
            }
            HandleId::Ctrl(..) => {
                let q = r + 0.5 * s;
                painter.circle_filled(pos, q, color);
                painter.circle_stroke(pos, q, Stroke::new((1.0 * s).max(1.0), Color32::WHITE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spiderweb_core::shape::FunnelStart;

    /// 一条横线 + 一堵竖墙的漏斗（起点在线上，向墙端 0 张开默认曲线）。
    fn sample_funnel() -> Shape {
        let mut sh = Shape::new(
            Kind::Funnel,
            vec![[0.0, 60.0], [4.0, 60.0], [4.0, 70.0], [4.0, 50.0]],
        );
        sh.starts.push(FunnelStart {
            line: 0,
            at: 0.5,
            ends: [Some(funnel::new_curve(None)), None],
        });
        sh
    }

    #[test]
    fn arrange_swaps_when_the_wall_was_drawn_first() {
        // 先画的竖线其实是墙：换成 [线, 线, 墙, 墙]，线起点在离墙远的一头
        let pts = [[2.0, 64.0], [2.0, 56.0], [0.5, 62.0], [4.0, 62.0]];
        let arr = arrange_funnel(1.0, 1.0, &pts).expect("墙与线交叉");
        assert_eq!(arr, [[4.0, 62.0], [0.5, 62.0], [2.0, 64.0], [2.0, 56.0]]);
    }

    #[test]
    fn arrange_keeps_the_line_drawn_first() {
        let pts = [[0.5, 62.0], [4.0, 62.0], [2.0, 64.0], [2.0, 56.0]];
        let arr = arrange_funnel(1.0, 1.0, &pts).expect("墙与线交叉");
        assert_eq!(arr, [[4.0, 62.0], [0.5, 62.0], [2.0, 64.0], [2.0, 56.0]]);
    }

    #[test]
    fn arrange_rejects_lines_running_side_by_side() {
        assert_eq!(
            arrange_funnel(1.0, 1.0, &[[0.0, 0.0], [4.0, 0.0], [0.0, 4.0], [4.0, 4.0]]),
            None
        );
    }

    #[test]
    fn ctrl_wall_keeps_both_ends_symmetric() {
        let line = [[0.0, 0.0], [4.0, 0.0]];
        let mirrored = wall_mirror(line, [2.0, 2.0], [2.0, 0.5]).expect("墙与线交叉");
        assert!((mirrored[0] - 2.0).abs() < 1e-9);
        assert!((mirrored[1] + 0.5).abs() < 1e-9);
        // 墙与线平行：没法对称
        assert_eq!(wall_mirror(line, [0.0, 1.0], [4.0, 1.0]), None);
    }

    #[test]
    fn part_at_hits_line_and_curve() {
        let sh = sample_funnel();
        let project = |p: Pt| ((p[0] * 10.0) as f32, p[1] as f32);
        // 线上的点（曲线还贴着线走，但近处只有线）
        assert_eq!(
            part_at_screen(&sh, &project, 5.0, 60.0, 6.0),
            Some(PartId::Line(0))
        );
        // 曲线到墙端 (4, 70)
        assert_eq!(
            part_at_screen(&sh, &project, 40.0, 70.0, 6.0),
            Some(PartId::Curve(0, 0))
        );
        // 离得太远：什么都没有
        assert_eq!(part_at_screen(&sh, &project, 5.0, 80.0, 6.0), None);
    }

    #[test]
    fn part_group_brings_the_linked_curves() {
        let mut sh = sample_funnel();
        let mut c0 = funnel::new_curve(None);
        c0.link = Some(1);
        let mut c1 = funnel::new_curve(None);
        c1.link = Some(1);
        sh.starts[0].ends = [Some(c0), Some(c1)];
        assert_eq!(
            part_group(&sh, PartId::Curve(0, 0)),
            BTreeSet::from([PartId::Curve(0, 0), PartId::Curve(0, 1)])
        );
        // 线没有联动
        assert_eq!(
            part_group(&sh, PartId::Line(0)),
            BTreeSet::from([PartId::Line(0)])
        );
    }
}
