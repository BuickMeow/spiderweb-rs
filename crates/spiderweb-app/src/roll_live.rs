//! Live 绘制与自定义形状的笔画（原版 roll/roll_live.py）。
//!
//! Live 开着时，线 / 折线 / 自由笔 / 曲线 / 弧 / 方 / 圆 / 三角画下的东西成为同一个自定义形状的笔画
//! （选中的那个，否则新建一个）；方 / 圆 / 三角无论 Live 开关都生成自己的自定义形状。还负责：
//! 拾取自定义形状的一条笔画（[`stroke_at`]）、删掉它（[`delete_stroke`]）、
//! 弯曲被拾取的曲线笔画（锚点 / 手柄，逻辑在 [`spiderweb_core::bezier`]）。
//! 笔画存在形状自己的框里（核心 custom 的 `add_stroke` / `refit`）。

use eframe::egui;
use egui::{Color32, Pos2, Rect, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::arc::arc_bezier;
use spiderweb_core::bezier::{self, HandleKind};
use spiderweb_core::custom::{
    CustomDefaults as CoreCustomDefaults, add_stroke, box_frame, frame_to_bp, frame_to_uv,
    new_live_shape, refit, stroke_ends,
};
use spiderweb_core::engine;
use spiderweb_core::shape::{Kind, Shape, Stroke as PathStroke};

use crate::app::{App, Tool};

/// Live 时会画成笔画的工具（roll_live.STROKE_TOOLS）。
pub const STROKE_TOOLS: [Tool; 8] = [
    Tool::Line,
    Tool::Poly,
    Tool::Free,
    Tool::Curve,
    Tool::Arc,
    Tool::Square,
    Tool::Circle,
    Tool::Triangle,
];

/// 方 / 圆 / 三角：总是生成自定义形状（自己的，或 Live 形状的一条笔画）。
pub const BOX_TOOLS: [Tool; 3] = [Tool::Square, Tool::Circle, Tool::Triangle];

/// 内置方形的笔画点（roll_live.SQUARE）。
pub const SQUARE: [Pt; 5] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
/// 内置三角的笔画点（roll_live.TRIANGLE，同 Drawer 内置 Triangle）。
pub const TRIANGLE: [Pt; 4] = [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0], [0.0, 0.0]];
/// 长自由笔画的点数门槛：超过只在该笔画被拾取时显示点（roll_live.LONG_STROKE）。
pub const LONG_STROKE: usize = 64;

/// 被拾取笔画的紫色（roll_draw.STROKE_POINT_COLOR / roll_live.draw_picked_stroke）。
const PICK_COLOR: Color32 = Color32::from_rgb(0x7a, 0x1f, 0xe0);
/// 曲线锚点 / 手柄的蓝色（roll_curve.HANDLE_COLOR）。
const HANDLE_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);

/// Ctrl 时方 / 圆 / 三角要保持的宽 / 高（roll_live.BOX_ASPECT）。
pub fn box_aspect(tool: Tool) -> f64 {
    match tool {
        Tool::Triangle => 2.0 / 3.0_f64.sqrt(),
        _ => 1.0,
    }
}

pub fn is_box_tool(tool: Tool) -> bool {
    BOX_TOOLS.contains(&tool)
}

pub fn is_stroke_tool(tool: Tool) -> bool {
    STROKE_TOOLS.contains(&tool)
}

/// 工程里的自定义默认设置 -> 核心 custom 的默认设置。
pub fn core_custom_defaults(app: &App) -> CoreCustomDefaults {
    CoreCustomDefaults {
        fill: app.custom_defaults.fill,
        gate: app.custom_defaults.gate,
        align: app.custom_defaults.align,
    }
}

/// 内置模板（Circle / Square / Triangle）的笔画与宽 / 高。
pub fn builtin_shape(name: &str) -> Option<(Vec<PathStroke>, f64)> {
    let poly = |pts: &[Pt]| PathStroke::Poly {
        pts: pts.to_vec(),
        free: false,
        smooth: 0,
        k: 1.0,
    };
    match name {
        "Circle" => Some((
            vec![PathStroke::Ellipse {
                box_: [0.0, 0.0, 1.0, 1.0],
            }],
            1.0,
        )),
        "Square" => Some((vec![poly(&SQUARE)], 1.0)),
        "Triangle" => Some((vec![poly(&TRIANGLE)], 1.0)),
        _ => None,
    }
}

/// 形状模板：先查图形库 `shapes/*.json` 里的同名形状（归一化），没有就退回内置模板（原版 custom_template）。
pub fn builtin_template(dir: &std::path::Path, name: &str) -> Option<(Vec<PathStroke>, f64)> {
    crate::drawer::library_template(dir, name).or_else(|| builtin_shape(name))
}

/// 方 / 圆 / 三角拖出来的自定义形状（roll_live.box_draft 的纯逻辑）。
pub fn box_draft_parts(
    defaults: &Shape,
    cd: &CoreCustomDefaults,
    tool: Tool,
    a: Pt,
    b: Pt,
) -> Shape {
    let poly = |pts: &[Pt]| PathStroke::Poly {
        pts: pts.to_vec(),
        free: false,
        smooth: 0,
        k: 1.0,
    };
    let strokes = match tool {
        Tool::Circle => vec![PathStroke::Ellipse {
            box_: [0.0, 0.0, 1.0, 1.0],
        }],
        Tool::Triangle => vec![poly(&TRIANGLE)],
        _ => vec![poly(&SQUARE)],
    };
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = tool.label().to_string();
    sh.strokes = strokes;
    sh.fill = cd.fill;
    sh.gate = cd.gate;
    sh.align = cd.align;
    sh.pts = box_frame(a[0], a[1], b[0], b[1]).to_vec();
    sh
}

/// 面板里选的内置模板放进框里（原版 new_custom）。
pub fn new_custom_parts(
    defaults: &Shape,
    cd: &CoreCustomDefaults,
    name: &str,
    strokes: &[PathStroke],
    a: Pt,
    b: Pt,
) -> Shape {
    let mut sh = defaults.clone();
    sh.kind = Kind::Custom;
    sh.name = name.to_string();
    sh.strokes = strokes.to_vec();
    sh.fill = cd.fill;
    sh.gate = cd.gate;
    sh.align = cd.align;
    sh.pts = box_frame(a[0], a[1], b[0], b[1]).to_vec();
    sh
}

/// 方 / 圆 / 三角拖出来的自定义形状。
pub fn box_draft(app: &App, tool: Tool, a: Pt, b: Pt) -> Shape {
    box_draft_parts(
        &app.defaults.clone(),
        &core_custom_defaults(app),
        tool,
        a,
        b,
    )
}

/// 从 (start) 到 (pt) 的框，按 drawing 的宽 / 高（Ctrl）修正 pt（roll_custom.keep_aspect）。
pub fn keep_aspect_xy(start: Pt, pt: Pt, aspect: f64, sx: f64, sy: f64) -> Pt {
    let mut dx = (pt[0] - start[0]) * sx;
    let mut dy = (pt[1] - start[1]) * sy;
    if dx.abs() > dy.abs() * aspect {
        dy = (dx.abs() / aspect).copysign(if dy == 0.0 { 1.0 } else { dy });
    } else {
        dx = (dy.abs() * aspect).copysign(if dx == 0.0 { 1.0 } else { dx });
    }
    [
        (start[0] + dx / sx).max(0.0),
        (start[1] + dy / sy).clamp(0.0, 127.0),
    ]
}

/// Ctrl 的 box 比例修正。
pub fn keep_aspect(app: &App, start: Pt, pt: Pt, aspect: f64) -> Pt {
    keep_aspect_xy(start, pt, aspect, app.view.sx, app.view.sy)
}

// ---------------------------------------------------------------- Live 绘制

/// Live 开着且当前工具会画笔画（roll_live.live_drawing）。
pub fn live_drawing(app: &App) -> bool {
    app.live && is_stroke_tool(app.tool)
}

/// 新笔画画进的自定义形状（Live 开且只选了一个自定义形状）：返回下标。
pub fn live_target(app: &App) -> Option<usize> {
    let i = app.sel?;
    let sh = app.shapes.get(i)?;
    (app.live
        && sh.kind == Kind::Custom
        && sh.text.is_none()
        && sh.notes.is_none()
        && app.sels.len() == 1)
        .then_some(i)
}

/// event_pt，且在 Live 绘制时：离画进去的形状的笔画端（或正在画的折线起点）够近就精确落上去（roll_live.draw_pt）。
pub fn draw_pt(app: &App, pos: Pos2, snap: bool, shift: bool) -> Pt {
    let pt = crate::roll::event_pt(app, pos, snap, shift);
    if !live_drawing(app) {
        return pt;
    }
    let mut near: Vec<Pt> = Vec::new();
    if let Some(i) = live_target(app)
        && let Some(to_bp) = frame_to_bp(&app.shapes[i].pts)
    {
        near.extend(
            stroke_ends(&app.shapes[i].strokes)
                .into_iter()
                .map(|q| to_bp(q[0], q[1])),
        );
    }
    if let Some(d) = &app.draft
        && d.kind == Kind::Poly
        && d.pts.len() >= 3
    {
        near.push(d.pts[0]);
    }
    let mut best: Option<Pt> = None;
    let mut reach = 8.0 * app.scale() as f64;
    for q in near {
        let d = ((app.view.x_of(q[0]) - pos.x).powi(2) + (app.view.y_of(q[1]) - pos.y).powi(2))
            .sqrt() as f64;
        if d < reach {
            best = Some(q);
            reach = d;
        }
    }
    best.unwrap_or(pt)
}

/// 画完的草稿 -> 一条笔画（弧变成曲线，好跟着曲线那样弯）（roll_live.draft_stroke）。
pub fn draft_stroke(sh: &Shape, draw: Option<Tool>) -> Option<PathStroke> {
    if let Some(tool) = draw
        && is_box_tool(tool)
    {
        let a = *sh.pts.first()?;
        let b = *sh.pts.get(1)?;
        let c = *sh.pts.get(2)?;
        let (b0, p0, b1, p1) = (a[0], a[1], b[0], c[1]);
        let poly = |pts: Vec<Pt>| PathStroke::Poly {
            pts,
            free: false,
            smooth: 0,
            k: 1.0,
        };
        return Some(match tool {
            Tool::Circle => PathStroke::Ellipse {
                box_: [b0, p0, b1, p1],
            },
            Tool::Triangle => poly(vec![[b0, p0], [b1, p0], [(b0 + b1) / 2.0, p1], [b0, p0]]),
            _ => poly(vec![[b0, p0], [b1, p0], [b1, p1], [b0, p1], [b0, p0]]),
        });
    }
    match sh.kind {
        Kind::Curve => Some(PathStroke::Curve {
            pts: sh.pts.clone(),
            sharp: sh.sharp.clone(),
            sym: sh.sym,
        }),
        Kind::Arc => Some(PathStroke::Curve {
            pts: arc_bezier(&sh.pts, sh.k),
            sharp: Vec::new(),
            sym: None,
        }),
        Kind::Free => Some(PathStroke::Poly {
            pts: sh.pts.clone(),
            free: true,
            smooth: sh.smooth,
            k: sh.k,
        }),
        _ => Some(PathStroke::Poly {
            pts: sh.pts.clone(),
            free: false,
            smooth: 0,
            k: 1.0,
        }),
    }
}

/// 画完的草稿进 Live 形状（或新建一个）当一条笔画；没开 Live 的方 / 圆 / 三角成为独立的自定义形状。
/// true = 已处理（调用方不要再加形状），false = 普通形状（调用方自己加）（roll_live.live_commit）。
pub fn live_commit(app: &mut App, sh: &Shape) -> bool {
    let draw = app.draft_draw.take();
    let is_box = draw.is_some_and(is_box_tool);
    if !is_box {
        let kind_tool = draw.or(match sh.kind {
            Kind::Line => Some(Tool::Line),
            Kind::Poly => Some(Tool::Poly),
            Kind::Free => Some(Tool::Free),
            Kind::Curve => Some(Tool::Curve),
            Kind::Arc => Some(Tool::Arc),
            _ => None,
        });
        if !(app.live && kind_tool.is_some_and(is_stroke_tool)) {
            return false;
        }
    }
    let Some(st) = draft_stroke(sh, draw) else {
        return true; // 框不够点：丢掉
    };
    if is_box {
        let (Some(a), Some(b), Some(c)) = (sh.pts.first(), sh.pts.get(1), sh.pts.get(2)) else {
            return true;
        };
        if a[0] == b[0] || a[1] == c[1] {
            return true; // 没有宽或高：丢掉
        }
    } else if let PathStroke::Poly { pts, .. }
    | PathStroke::Curve { pts, .. }
    | PathStroke::Arc { pts, .. } = &st
        && pts.first().is_none_or(|p| pts.iter().all(|q| q == p))
    {
        return true; // 所有点重合：丢掉
    }
    let is_curve_stroke =
        |target: &Shape, k: usize| matches!(target.strokes.get(k), Some(PathStroke::Curve { .. }));
    match live_target(app) {
        None if !app.live => {
            app.add_shape(sh.clone());
            true
        }
        None => {
            let defaults = app.defaults.clone();
            let cd = core_custom_defaults(app);
            let mut target = new_live_shape(&defaults, &cd);
            let Some(k) = add_stroke(&mut target, &st) else {
                return true;
            };
            let curve = is_curve_stroke(&target, k);
            app.add_shape(target);
            app.set_stroke(curve.then_some(k));
            true
        }
        Some(i) => {
            app.push_undo();
            let k = app
                .shapes
                .get_mut(i)
                .and_then(|target| add_stroke(target, &st));
            if let Some(k) = k {
                let curve = app
                    .shapes
                    .get(i)
                    .is_some_and(|target| is_curve_stroke(target, k));
                app.set_stroke(curve.then_some(k));
            }
            app.shapes_changed();
            true
        }
    }
}

// ---------------------------------------------------------------- 笔画

/// 屏幕点到线段的距离（roll_funnel.seg_dist）。
fn seg_dist(x: f32, y: f32, a: Pos2, b: Pos2) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let ll = dx * dx + dy * dy;
    let u = if ll == 0.0 {
        0.0
    } else {
        (((x - a.x) * dx + (y - a.y) * dy) / ll).clamp(0.0, 1.0)
    };
    ((x - a.x - u * dx).powi(2) + (y - a.y - u * dy).powi(2)).sqrt()
}

/// 形状笔画在屏幕上的路径。
fn screen_path(app: &App, path: &[Pt]) -> Vec<Pos2> {
    path.iter()
        .map(|p| Pos2::new(app.view.x_of(p[0]), app.view.y_of(p[1])))
        .collect()
}

/// 屏幕上 (x, y) 下的笔画号（roll_live.stroke_at）。
pub fn stroke_at(app: &App, sh: &Shape, pos: Pos2, near: f32) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (k, path) in engine::shape_strokes(sh).iter().enumerate() {
        let pts = screen_path(app, path);
        let mut d = f32::INFINITY;
        for w in pts.windows(2) {
            d = d.min(seg_dist(pos.x, pos.y, w[0], w[1]));
        }
        if d <= near && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

/// 被拾取的笔画号（roll_live.picked_stroke）。
pub fn picked_stroke(app: &App, sh: &Shape) -> Option<usize> {
    let k = app.stroke?;
    (sh.kind == Kind::Custom && k < sh.strokes.len()).then_some(k)
}

/// 笔画列里被拾取的是曲线时返回它（roll_live.stroke_curve）。
fn picked_curve_in(app: &App, strokes: &[PathStroke]) -> Option<usize> {
    let k = app.stroke?;
    matches!(strokes.get(k), Some(PathStroke::Curve { .. })).then_some(k)
}

/// 被拾取的笔画是曲线时返回它（roll_live.stroke_curve）。
fn picked_curve_stroke(app: &App, sh: &Shape) -> Option<usize> {
    picked_stroke(app, sh)?;
    picked_curve_in(app, &sh.strokes)
}

/// 一条笔画的原始点数（椭圆没有原始点）。
fn stroke_raw_len(st: &PathStroke) -> usize {
    match st {
        PathStroke::Poly { pts, .. }
        | PathStroke::Curve { pts, .. }
        | PathStroke::Arc { pts, .. } => pts.len(),
        PathStroke::Ellipse { .. } => usize::MAX,
    }
}

/// 会显示点（并跟着 Select 工具拖动）的笔画：Live 开且只选一个时是全部
/// （长自由笔画只在被拾取时），否则只是被拾取的那条（roll_live.point_strokes）。
pub fn point_strokes(app: &App, sh: &Shape) -> Vec<usize> {
    if sh.text.is_some() || sh.notes.is_some() {
        return Vec::new();
    }
    let k = picked_stroke(app, sh);
    if app.live && app.sels.len() == 1 {
        sh.strokes
            .iter()
            .enumerate()
            .filter(|(i, st)| Some(*i) == k || stroke_raw_len(st) <= LONG_STROKE)
            .map(|(i, _)| i)
            .collect()
    } else {
        k.into_iter().collect()
    }
}

/// 一条笔画能拖的点 (点号, (u, v))：折线的点、曲线的两端、椭圆的左 / 右 / 下 / 上（点号 0-3）
/// （roll_live.stroke_spots）。
pub fn stroke_spots(st: &PathStroke) -> Vec<(usize, Pt)> {
    match st {
        PathStroke::Ellipse { box_ } => {
            let (u0, v0, u1, v1) = (box_[0], box_[1], box_[2], box_[3]);
            let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
            vec![(0, [u0, cv]), (1, [u1, cv]), (2, [cu, v0]), (3, [cu, v1])]
        }
        PathStroke::Curve { pts, .. } => {
            if pts.is_empty() {
                Vec::new()
            } else {
                vec![(0, pts[0]), (pts.len() - 1, pts[pts.len() - 1])]
            }
        }
        PathStroke::Poly { pts, .. } | PathStroke::Arc { pts, .. } => {
            pts.iter().copied().enumerate().collect()
        }
    }
}

/// 自定义形状的一个笔画把手。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeHandleId {
    /// 笔画的一个点（Select 工具）：笔画号与点号
    Pt { k: usize, j: usize },
    /// 被拾取曲线笔画的锚点 / 手柄：ctrl 区分手柄，j 是点号
    Curve { ctrl: bool, j: usize },
}

/// 选中自定义形状要显示的笔画把手 `(beat, pitch, 把手, 任何工具可用)`
/// （roll_live.stroke_handles）。
pub fn stroke_handles(app: &App, sh: &Shape) -> Vec<(Pt, StrokeHandleId, bool)> {
    let Some(to_bp) = frame_to_bp(&sh.pts) else {
        return Vec::new();
    };
    let mut out: Vec<(Pt, StrokeHandleId, bool)> = Vec::new();
    for k in point_strokes(app, sh) {
        if let Some(st) = sh.strokes.get(k) {
            for (j, p) in stroke_spots(st) {
                out.push((to_bp(p[0], p[1]), StrokeHandleId::Pt { k, j }, false));
            }
        }
    }
    if let Some(k) = picked_curve_stroke(app, sh)
        && let Some(PathStroke::Curve { pts, .. }) = sh.strokes.get(k)
    {
        for (j, kind) in bezier::pen_handles(pts, true) {
            if kind == HandleKind::End {
                continue;
            }
            if let Some(p) = pts.get(j) {
                out.push((
                    to_bp(p[0], p[1]),
                    StrokeHandleId::Curve {
                        ctrl: kind == HandleKind::Ctrl,
                        j,
                    },
                    true,
                ));
            }
        }
    }
    out
}

/// 屏幕上命中笔画把手；`any` 为 true（Select 工具）时笔画点也可命中。
pub fn hit_stroke_handle(app: &App, sh: &Shape, pos: Pos2, any: bool) -> Option<StrokeHandleId> {
    let near = 7.0_f32.max(8.0 * app.scale());
    for (p, id, free) in stroke_handles(app, sh).into_iter().rev() {
        if !(free || any) {
            continue;
        }
        let x = app.view.x_of(p[0]);
        let y = app.view.y_of(p[1]);
        if (x - pos.x).abs() <= near && (y - pos.y).abs() <= near {
            return Some(id);
        }
    }
    None
}

/// 一次笔画拖动要改的形状现场：(u, v) 映射与框的点。
struct StrokeFrame {
    frame: Vec<Pt>,
    strokes: Vec<PathStroke>,
}

fn stroke_frame(app: &App) -> Option<(usize, StrokeFrame)> {
    let i = app.sel?;
    let sh = app.shapes.get(i)?;
    if sh.kind != Kind::Custom {
        return None;
    }
    Some((
        i,
        StrokeFrame {
            frame: sh.pts.clone(),
            strokes: sh.strokes.clone(),
        },
    ))
}

/// 把改动写回形状并重新套框。
fn commit_strokes(app: &mut App, i: usize, frame: Vec<Pt>, strokes: Vec<PathStroke>) {
    if let Some(target) = app.shapes.get_mut(i) {
        target.pts = frame;
        target.strokes = strokes;
        refit(target);
    }
}

/// 拖一条笔画的点（"pt"）：吸附到网格（Shift 自由），离别的笔画点几像素内就落上去（轮廓能接上）。
/// 同一位置的所有笔画点一起走；曲线端点带着手柄，椭圆边点改那一边。
/// 返回要继续用的把手（椭圆边拖过对面会变成那一边）（roll_live.drag_stroke_point）。
pub fn drag_stroke_point(
    app: &mut App,
    hid: StrokeHandleId,
    pos: Pos2,
    shift: bool,
) -> StrokeHandleId {
    let StrokeHandleId::Pt { k, j } = hid else {
        return hid;
    };
    let Some((i, fr)) = stroke_frame(app) else {
        return hid;
    };
    let Some(to_uv) = frame_to_uv(&fr.frame) else {
        return hid;
    };
    let Some(st) = fr.strokes.get(k).cloned() else {
        return hid;
    };
    if let PathStroke::Ellipse { box_ } = st {
        let mut box_ = box_;
        let pt = crate::roll::event_pt(app, pos, true, shift);
        let uv = to_uv(pt[0], pt[1]);
        let idx = [0, 2, 1, 3][j.min(3)];
        box_[idx] = if j < 2 { uv[0] } else { uv[1] };
        let flipped = if j < 2 {
            box_[0] > box_[2]
        } else {
            box_[1] > box_[3]
        };
        if let Some(target) = app.shapes.get_mut(i) {
            if let Some(PathStroke::Ellipse { box_: b }) = target.strokes.get_mut(k) {
                *b = box_;
            }
            refit(target);
        }
        return if flipped {
            StrokeHandleId::Pt { k, j: j ^ 1 }
        } else {
            hid
        };
    }
    let Some(old) = stroke_spots(&st)
        .into_iter()
        .find(|(n, _)| *n == j)
        .map(|(_, p)| p)
    else {
        return hid;
    };
    let near_old = |p: Pt| ((p[0] - old[0]).powi(2) + (p[1] - old[1]).powi(2)).sqrt() < 1e-7;
    let mut joined: Vec<(usize, usize)> = Vec::new();
    for (ii, other) in fr.strokes.iter().enumerate() {
        if matches!(other, PathStroke::Ellipse { .. }) {
            continue;
        }
        for (n, p) in stroke_spots(other) {
            if near_old(p) {
                joined.push((ii, n));
            }
        }
    }
    if joined.is_empty() {
        joined.push((k, j));
    }
    let Some(to_bp) = frame_to_bp(&fr.frame) else {
        return hid;
    };
    let pt = crate::roll::event_pt(app, pos, true, shift);
    let mut best: Option<Pt> = None;
    let mut reach = 8.0 * app.scale() as f64;
    for (ii, other) in fr.strokes.iter().enumerate() {
        if matches!(other, PathStroke::Ellipse { .. }) {
            continue;
        }
        for (n, p) in stroke_spots(other) {
            if joined.contains(&(ii, n)) {
                continue;
            }
            let bp = to_bp(p[0], p[1]);
            let d = ((app.view.x_of(bp[0]) - pos.x).powi(2)
                + (app.view.y_of(bp[1]) - pos.y).powi(2))
            .sqrt() as f64;
            if d < reach {
                best = Some(bp);
                reach = d;
            }
        }
    }
    let new = to_uv(best.unwrap_or(pt)[0], best.unwrap_or(pt)[1]);
    let mut strokes = fr.strokes;
    for (ii, n) in joined {
        let curve = matches!(strokes.get(ii), Some(PathStroke::Curve { .. }));
        let pts = match strokes.get_mut(ii) {
            Some(
                PathStroke::Poly { pts, .. }
                | PathStroke::Curve { pts, .. }
                | PathStroke::Arc { pts, .. },
            ) => pts,
            _ => continue,
        };
        if n >= pts.len() {
            continue;
        }
        let du = new[0] - pts[n][0];
        let dv = new[1] - pts[n][1];
        pts[n] = new;
        if curve {
            let h = if n == 0 { 1 } else { n - 1 };
            if h < pts.len() {
                pts[h][0] += du;
                pts[h][1] += dv;
            }
        }
    }
    commit_strokes(app, i, fr.frame, strokes);
    hid
}

/// (u, v) <-> 屏幕的映射（roll_live.stroke_maps）。
#[allow(clippy::type_complexity)] // 两个闭包的类型没法起别名
fn stroke_maps(
    app: &App,
    frame: &[Pt],
) -> Option<(
    impl Fn(Pt) -> [f64; 2] + use<>,
    impl Fn(f64, f64) -> Pt + use<>,
)> {
    let to_bp = frame_to_bp(frame)?;
    let to_uv = frame_to_uv(frame)?;
    let view = app.view.clone();
    let view2 = app.view.clone();
    Some((
        move |p: Pt| {
            let bp = to_bp(p[0], p[1]);
            [view.x_of(bp[0]) as f64, view.y_of(bp[1]) as f64]
        },
        move |x: f64, y: f64| to_uv(view2.b_of(x as f32), view2.p_of(y as f32)),
    ))
}

/// 拖一个笔画把手：笔画点是 [`drag_stroke_point`]，被拾取曲线的锚点 / 手柄走 bezier
/// （吸附，Alt 同 bezier.drag_point）。返回要继续用的把手（roll_live.drag_stroke）。
pub fn drag_stroke_handle(
    app: &mut App,
    hid: StrokeHandleId,
    pos: Pos2,
    shift: bool,
    alt: bool,
) -> StrokeHandleId {
    match hid {
        StrokeHandleId::Pt { .. } => drag_stroke_point(app, hid, pos, shift),
        StrokeHandleId::Curve { j, .. } => {
            let Some((i, fr)) = stroke_frame(app) else {
                return hid;
            };
            let Some(k) = picked_curve_in(app, &fr.strokes) else {
                return hid;
            };
            let Some(PathStroke::Curve { pts, sharp, sym }) = fr.strokes.get(k) else {
                return hid;
            };
            let mut c = bezier::Curve {
                pts: pts.clone(),
                sharp: sharp.clone(),
                sym: *sym,
            };
            let Some((to_screen, from_screen)) = stroke_maps(app, &fr.frame) else {
                return hid;
            };
            let Some(to_uv) = frame_to_uv(&fr.frame) else {
                return hid;
            };
            let pt = crate::roll::event_pt(app, pos, true, shift);
            let uv = to_uv(pt[0], pt[1]);
            bezier::drag_point(&mut c, j, uv, alt, &to_screen, &from_screen, false);
            let mut strokes = fr.strokes;
            if let Some(PathStroke::Curve { pts, sharp, sym }) = strokes.get_mut(k) {
                *pts = c.pts;
                *sharp = c.sharp;
                *sym = c.sym;
            }
            commit_strokes(app, i, fr.frame, strokes);
            hid
        }
    }
}

/// 右键被拾取曲线笔画的一个点：锚点删掉，手柄收回去（roll_live.delete_stroke_handle）。
pub fn delete_stroke_handle(app: &mut App, hid: StrokeHandleId) {
    let StrokeHandleId::Curve { j, .. } = hid else {
        return; // 笔画点：右键当在笔画上（出菜单 / 取消选择）
    };
    let Some((i, fr)) = stroke_frame(app) else {
        return;
    };
    let Some(k) = picked_curve_in(app, &fr.strokes) else {
        return;
    };
    let Some(PathStroke::Curve { pts, sharp, sym }) = fr.strokes.get(k) else {
        return;
    };
    let mut c = bezier::Curve {
        pts: pts.clone(),
        sharp: sharp.clone(),
        sym: *sym,
    };
    match bezier::can_delete(&c, j) {
        None => {}
        Some(bezier::CanDelete::Middle) => {
            app.status = "对称曲线的中间锚点会保留（关掉对称再删）".to_string();
        }
        Some(_) => {
            let Some((to_screen, _)) = stroke_maps(app, &fr.frame) else {
                return;
            };
            app.push_undo();
            bezier::delete_point(&mut c, j, &to_screen, false);
            let mut strokes = fr.strokes;
            if let Some(PathStroke::Curve { pts, sharp, sym }) = strokes.get_mut(k) {
                *pts = c.pts;
                *sharp = c.sharp;
                *sym = c.sym;
            }
            commit_strokes(app, i, fr.frame, strokes);
            app.shapes_changed();
        }
    }
}

/// 被拾取曲线笔画上离鼠标最近的地方加一个锚点并移到鼠标；`near` 内才加。
/// true = 加上了（roll_live.stroke_click）。
pub fn stroke_click(app: &mut App, pos: Pos2, near: Option<f64>, shift: bool) -> bool {
    let Some((i, fr)) = stroke_frame(app) else {
        return false;
    };
    let Some(k) = picked_curve_in(app, &fr.strokes) else {
        return false;
    };
    let Some(PathStroke::Curve { pts, sharp, sym }) = fr.strokes.get(k) else {
        return false;
    };
    let mut c = bezier::Curve {
        pts: pts.clone(),
        sharp: sharp.clone(),
        sym: *sym,
    };
    let Some((to_screen, _)) = stroke_maps(app, &fr.frame) else {
        return false;
    };
    let Some((seg, t, d)) = bezier::nearest(&c.pts, &to_screen, pos.x as f64, pos.y as f64, 64)
    else {
        return false;
    };
    if near.is_some_and(|n| d > n) {
        return false;
    }
    let Some(to_uv) = frame_to_uv(&fr.frame) else {
        return false;
    };
    let pt = crate::roll::event_pt(app, pos, true, shift);
    let uv = to_uv(pt[0], pt[1]);
    if !bezier::add_anchor(&mut c, seg, t, uv, &to_screen, false) {
        return false;
    }
    app.push_undo();
    let mut strokes = fr.strokes;
    if let Some(PathStroke::Curve { pts, sharp, sym }) = strokes.get_mut(k) {
        *pts = c.pts.clone();
        *sharp = c.sharp.clone();
        *sym = c.sym;
    }
    commit_strokes(app, i, fr.frame, strokes);
    app.shapes_changed();
    true
}

/// 把笔画 k 从自定义形状里删掉（最后一条：整个形状删掉）（roll_live.delete_stroke）。
pub fn delete_stroke(app: &mut App, i: usize, k: usize) {
    let n = app.shapes.get(i).map(|s| s.strokes.len()).unwrap_or(0);
    if n <= 1 {
        app.delete_selected();
        return;
    }
    if k >= n {
        return;
    }
    app.push_undo();
    if let Some(target) = app.shapes.get_mut(i) {
        target.strokes.remove(k);
        refit(target);
    }
    app.set_stroke(None);
    app.shapes_changed();
}

/// (b, p) 在屏幕上的位置。
fn at(app: &App, rect: Rect, p: Pt) -> Pos2 {
    Pos2::new(
        rect.min.x + app.view.x_of(p[0]),
        rect.min.y + app.view.y_of(p[1]),
    )
}

// ---------------------------------------------------------------- 绘制

/// 被拾取的那条笔画：粗紫线，压在把手下面（roll_live.draw_picked_stroke）。
pub fn paint_picked_stroke(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let Some(k) = picked_stroke(app, sh) else {
        return;
    };
    let paths = engine::shape_strokes(sh);
    let Some(path) = paths.get(k) else {
        return;
    };
    if path.len() < 2 {
        return;
    }
    let pts: Vec<Pos2> = path.iter().map(|p| at(app, rect, *p)).collect();
    painter.add(egui::Shape::line(
        pts,
        egui::Stroke::new((3.0 * app.scale()).max(3.0), PICK_COLOR),
    ));
}

/// 选中自定义形状的笔画点与被拾取曲线笔画的锚点 / 手柄（roll_draw.draw_handles 的 custom 分支）。
pub fn paint_custom_handles(app: &App, painter: &egui::Painter, rect: Rect, sh: &Shape) {
    let s = app.scale();
    if let Some(k) = picked_curve_stroke(app, sh)
        && let Some(PathStroke::Curve { pts, .. }) = sh.strokes.get(k)
        && let Some(to_bp) = frame_to_bp(&sh.pts)
    {
        for (a, h) in bezier::handle_lines(pts) {
            let pa = at(app, rect, to_bp(a[0], a[1]));
            let ph = at(app, rect, to_bp(h[0], h[1]));
            painter.line_segment(
                [pa, ph],
                egui::Stroke::new((3.5 * s).max(3.0), Color32::WHITE),
            );
            painter.line_segment(
                [pa, ph],
                egui::Stroke::new((1.5 * s).max(1.0), HANDLE_COLOR),
            );
        }
    }
    for (p, id, _) in stroke_handles(app, sh) {
        let q = at(app, rect, p);
        match id {
            StrokeHandleId::Pt { .. } => {
                let r = 3.0 * s;
                let box_ = Rect::from_center_size(q, Vec2::splat(r * 2.0));
                painter.rect_filled(box_, 0.0, Color32::WHITE);
                painter.rect_stroke(
                    box_,
                    0.0,
                    egui::Stroke::new(2.0, PICK_COLOR),
                    egui::StrokeKind::Inside,
                );
            }
            StrokeHandleId::Curve { ctrl: true, .. } => {
                let r = 4.5 * s;
                painter.circle_filled(q, r, HANDLE_COLOR);
                painter.circle_stroke(q, r, egui::Stroke::new(1.0 * s, Color32::WHITE));
            }
            StrokeHandleId::Curve { ctrl: false, .. } => {
                let r = 5.5 * s;
                painter.circle_filled(q, r, Color32::WHITE);
                painter.circle_stroke(q, r, egui::Stroke::new((2.0 * s).max(2.0), HANDLE_COLOR));
            }
        }
    }
}

/// 偶奇规则：从 (b, p) 向右的线穿过轮廓奇数次就在里面（roll_custom.inside_strokes）。
pub fn inside_strokes(strokes: &[Vec<Pt>], b: f64, p: f64) -> bool {
    let mut inside = false;
    for poly in strokes {
        for w in poly.windows(2) {
            let (xa, ya) = (w[0][0], w[0][1]);
            let (xb, yb) = (w[1][0], w[1][1]);
            if (ya <= p) != (yb <= p) && b < xa + (xb - xa) * (p - ya) / (yb - ya) {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use spiderweb_core::custom::CustomDefaults;

    fn defaults() -> Shape {
        Shape::default()
    }

    fn cd() -> CustomDefaults {
        CustomDefaults {
            fill: spiderweb_core::shape::Fill::Spam,
            gate: 0.125,
            align: spiderweb_core::shape::Align::Aligned,
        }
    }

    #[test]
    fn box_draft_square_uses_poly_and_box_frame() {
        let sh = box_draft_parts(&defaults(), &cd(), Tool::Square, [1.0, 3.0], [4.0, 8.0]);
        assert_eq!(sh.kind, Kind::Custom);
        assert_eq!(sh.name, "Square");
        assert_eq!(sh.pts, vec![[1.0, 3.0], [4.0, 3.0], [1.0, 8.0]]);
        assert_eq!(sh.fill, spiderweb_core::shape::Fill::Spam);
        assert_eq!(sh.gate, 0.125);
        match &sh.strokes[..] {
            [PathStroke::Poly { pts, .. }] => assert_eq!(pts.as_slice(), &SQUARE),
            other => panic!("期望 poly 笔画，得到 {other:?}"),
        }
    }

    #[test]
    fn box_draft_circle_is_ellipse() {
        let sh = box_draft_parts(&defaults(), &cd(), Tool::Circle, [0.0, 0.0], [2.0, 1.0]);
        assert_eq!(sh.name, "Circle");
        assert_eq!(sh.pts, vec![[0.0, 0.0], [2.0, 0.0], [0.0, 1.0]]);
        assert_eq!(
            sh.strokes,
            vec![PathStroke::Ellipse {
                box_: [0.0, 0.0, 1.0, 1.0]
            }]
        );
    }

    #[test]
    fn box_draft_triangle_uses_builtin_triangle() {
        let sh = box_draft_parts(&defaults(), &cd(), Tool::Triangle, [0.0, 5.0], [4.0, 7.0]);
        assert_eq!(sh.name, "Triangle");
        match &sh.strokes[..] {
            [PathStroke::Poly { pts, .. }] => assert_eq!(pts.as_slice(), &TRIANGLE),
            other => panic!("期望 poly 笔画，得到 {other:?}"),
        }
    }

    #[test]
    fn box_aspect_matches_original() {
        assert_eq!(box_aspect(Tool::Square), 1.0);
        assert_eq!(box_aspect(Tool::Circle), 1.0);
        let t = box_aspect(Tool::Triangle);
        assert!((t - 2.0 / 3.0_f64.sqrt()).abs() < 1e-12);
        assert!((t - 1.1547005383792515).abs() < 1e-12);
    }

    #[test]
    fn keep_aspect_wider_than_tall_fixes_dy() {
        let got = keep_aspect_xy([0.0, 0.0], [4.0, 1.0], 2.0, 10.0, 10.0);
        assert_eq!(got, [4.0, 2.0]);
    }

    #[test]
    fn keep_aspect_taller_than_wide_fixes_dx() {
        // 高 40px、宽 10px，aspect 2 -> 宽要 80px = 8 拍
        let got = keep_aspect_xy([0.0, 0.0], [1.0, 4.0], 2.0, 10.0, 10.0);
        assert_eq!(got, [8.0, 4.0]);
    }

    #[test]
    fn keep_aspect_takes_screen_ratio() {
        // 每拍 20px、每 key 5px：(2 拍, 8 key) 在屏幕上都是 40px，aspect 2 -> 宽 80px = 4 拍
        let got = keep_aspect_xy([0.0, 0.0], [2.0, 8.0], 2.0, 20.0, 5.0);
        assert!((got[0] - 4.0).abs() < 1e-12);
        assert!((got[1] - 8.0).abs() < 1e-12);
    }

    #[test]
    fn keep_aspect_clamps_inside_roll() {
        let got = keep_aspect_xy([0.0, 0.0], [-3.0, 2.0], 1.0, 10.0, 10.0);
        assert_eq!(got, [0.0, 3.0]);
        // 高的那维是 800px：宽被拉到 800px = 80 拍，音高夹在 127
        let got = keep_aspect_xy([0.0, 120.0], [5.0, 200.0], 1.0, 10.0, 10.0);
        assert_eq!(got, [80.0, 127.0]);
    }

    #[test]
    fn builtin_templates_are_known() {
        let dir = std::path::Path::new("/nonexistent-spiderweb-shapes");
        assert!(builtin_template(dir, "Circle").is_some());
        assert!(builtin_template(dir, "Square").is_some());
        assert!(builtin_template(dir, "Triangle").is_some());
        assert!(builtin_template(dir, "Spider").is_none());
        assert!(builtin_shape("Circle").is_some());
        assert!(builtin_shape("Spider").is_none());
    }

    #[test]
    fn inside_strokes_even_odd() {
        let square = vec![SQUARE.to_vec()];
        assert!(inside_strokes(&square, 0.5, 0.5));
        assert!(!inside_strokes(&square, 1.5, 0.5));
        // 洞：一个外框 + 反向内框，中间不算里面
        let hole = vec![
            SQUARE.to_vec(),
            vec![
                [0.25, 0.25],
                [0.25, 0.75],
                [0.75, 0.75],
                [0.75, 0.25],
                [0.25, 0.25],
            ],
        ];
        assert!(!inside_strokes(&hole, 0.5, 0.5));
        assert!(inside_strokes(&hole, 0.1, 0.5));
    }
}
