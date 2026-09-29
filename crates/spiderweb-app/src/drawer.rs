//! Custom shape drawer (upstream window/drawer.py): the 0..1 drawing board + the shape
//! library shapes/*.json.
//!
//! The board is a u / v = 0..1 square (v upward); draw anywhere, and it is stretched to the
//! shape's own size on the roll. Tools: select / line / polyline / freehand / curve /
//! three-point arc / square / circle / triangle / eraser; editing: whole-stroke drag, point
//! and ellipse-corner drag, flip, turn 90°, curve symmetry, regular polygons; shape library:
//! open / new / delete / rename / save / save as / Rename / Use (as a template for the roll's
//! Custom tool).
//!
//! Differences from upstream (all noted in the implementation):
//! - curve pen anchors / handles, freehand and the three-point arc reuse
//!   [`spiderweb_core::bezier`] and core custom;
//! - the rotate / skew box for custom shapes already exists on the roll (roll_custom.rs), so
//!   the board only does stroke-level flip and 90° turns;
//! - the "Sides" regular polygon is a port addition (upstream has none), generating a regular
//!   n-gon in one go;
//! - unsaved drawings use a system confirmation dialog on switch / close instead of upstream's
//!   custom dialog.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::{Color32, Key, Modifiers, Pos2, Rect, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::bezier::{self, CanDelete, HandleKind};
use spiderweb_core::custom::{
    self, clean_strokes, join_strokes, normalize_strokes, open_ends, open_paths, strokes_closed,
};
use spiderweb_core::shape::{Kind, Shape, Stroke, Sym};
use spiderweb_io::compat::shape_to_json;
use spiderweb_io::safefile;

use crate::app::{App, Tool};
use crate::drawer_tools::{self as dt, DrawerTool, Spot};

/// Built-in shapes (the names of upstream drawer.BUILT_IN): a file with the same name takes priority, and deleting the file brings the built-in back.
pub const BUILT_IN: [&str; 3] = ["Circle", "Square", "Triangle"];
/// Characters not allowed in file names (upstream drawer.BAD_CHARS).
const BAD_CHARS: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

const STROKE_COLOR: Color32 = Color32::from_rgb(0xc0, 0x39, 0x2b);
const SEL_COLOR: Color32 = Color32::from_rgb(0xff, 0x8c, 0x1a);
const DRAFT_COLOR: Color32 = Color32::from_rgb(0x0a, 0x8f, 0x0a);
const HANDLE_COLOR: Color32 = Color32::from_rgb(0x00, 0x50, 0xd0);
const OPEN_END_COLOR: Color32 = Color32::from_rgb(0xff, 0x20, 0x20);
const GRID_MINOR: Color32 = Color32::from_rgb(0xdd, 0xe3, 0xec);
const GRID_MAJOR: Color32 = Color32::from_rgb(0x9a, 0xa4, 0xb4);
const GRID_MID: Color32 = Color32::from_rgb(0x7f, 0x8f, 0xb0);
const BOARD_EDGE: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const SIDES_SPAN: f64 = 0.4;

// ---------------------------------------------------------------- shape library

/// Library directory (shapes/ next to the executable; upstream drawer.LIBRARY).
pub fn library_dir(app: &App) -> PathBuf {
    app.library_dir.clone()
}

/// Shape file names already in the library (without .json).
pub fn saved_names(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() > 5 && name[name.len() - 5..].eq_ignore_ascii_case(".json") {
            out.push(name[..name.len() - 5].to_string());
        }
    }
    out
}

/// When name is a built-in shape, returns its canonical spelling (case-insensitive).
pub fn builtin_name(name: &str) -> Option<&'static str> {
    BUILT_IN
        .iter()
        .find(|b| b.eq_ignore_ascii_case(name))
        .copied()
}

/// Library list: files + built-in shapes not overridden, sorted by lowercase (upstream library_names).
pub fn library_names(dir: &Path) -> Vec<String> {
    let mut names = saved_names(dir);
    let taken: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    for b in BUILT_IN {
        if !taken.iter().any(|t| t == &b.to_lowercase()) {
            names.push(b.to_string());
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// The library file path of a shape.
pub fn shape_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

/// Reads a library shape; falls back to the built-in when the file is missing, returns None when it is broken (upstream load_shape).
pub fn load_shape(dir: &Path, name: &str) -> Option<Vec<Stroke>> {
    match std::fs::read_to_string(shape_file(dir, name)) {
        Ok(text) => {
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            let strokes = value.get("strokes").map(clean_strokes).unwrap_or_default();
            (!strokes.is_empty()).then_some(strokes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let b = builtin_name(name)?;
            crate::roll_live::builtin_shape(b).map(|(st, _)| st)
        }
        Err(_) => None,
    }
}

/// Writes strokes into a library file `{"strokes": [...]}` (the format of upstream save_shape).
pub fn save_shape(dir: &Path, name: &str, strokes: &[Stroke]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let sh = Shape {
        kind: Kind::Custom,
        strokes: strokes.to_vec(),
        ..Shape::default()
    };
    let json = shape_to_json(&sh);
    let doc = serde_json::json!({ "strokes": json.get("strokes").cloned().unwrap_or_default() });
    let text = serde_json::to_string_pretty(&doc).map_err(std::io::Error::other)?;
    safefile::write_text(&shape_file(dir, name), &(text + "\n"))
}

/// File name cleanup (upstream clean_name): strips invalid characters, surrounding whitespace and trailing periods.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !BAD_CHARS.contains(c)).collect();
    cleaned.trim().trim_end_matches('.').to_string()
}

/// A library shape as a template: normalized to 0..1, with its width / height (1.0 when flat) (upstream custom_template).
pub fn library_template(dir: &Path, name: &str) -> Option<(Vec<Stroke>, f64)> {
    let strokes = load_shape(dir, name)?;
    let (out, ratio) = normalize_strokes(&strokes);
    if out.is_empty() {
        None
    } else {
        Some((out, ratio.unwrap_or(1.0)))
    }
}

fn confirm(text: &str) -> bool {
    rfd::MessageDialog::new()
        .set_title("Spiderweb")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

// ---------------------------------------------------------------- state

/// Board view: at zoom = 1 the whole square fits the window; center is the board point at the window center.
#[derive(Clone, Copy, Debug)]
pub struct BoardView {
    pub zoom: f64,
    pub center: Pt,
}

impl Default for BoardView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center: [0.5, 0.5],
        }
    }
}

/// One board drag.
enum BoardDrag {
    /// Empty-space drag / middle button: pan the view
    Pan { start: Pos2, center: Pt },
    /// Box tools: drag from the press position to the mouse
    Box { start: Pt },
    /// Freehand: add points following the mouse
    Free { last: Pos2 },
    /// Polyline cursor point
    Poly,
    /// Drag the whole stroke
    Move { i: usize, start: Pt, orig: Stroke },
    /// Points at the same position move together
    Points {
        group: Vec<(usize, usize)>,
        start: Pt,
    },
    /// Ellipse corner resize (the opposite corner stays put)
    Corner { i: usize, k: usize, box_: [f64; 4] },
    /// Curve anchors / handles
    Pen { i: usize, j: usize },
}

/// Drawer window state (upstream Drawer).
pub struct Drawer {
    pub open: bool,
    /// Keyboard goes to the drawer (upstream in_drawer): after pressing something in the drawer, the roll's shortcuts step aside.
    pub focus: bool,
    pub tool: DrawerTool,
    pub grid_n: i64,
    pub view: BoardView,
    pub strokes: Vec<Stroke>,
    pub draft: Option<Stroke>,
    pub sel: Option<usize>,
    drag: Option<BoardDrag>,
    /// Clicked without dragging: the stroke follows the mouse and the next click fixes it (upstream follow)
    pub follow: Option<(Pt, DrawerTool)>,
    pub undo_stack: Vec<Vec<Stroke>>,
    /// Undone steps, until something new is drawn (upstream redo_stack).
    pub redo_stack: Vec<Vec<Stroke>>,
    /// The redo steps before the last push_undo (a click that moved nothing gets them back).
    pub redo_kept: Vec<Vec<Stroke>>,
    pub clipboard: Option<Vec<Stroke>>,
    pub pastes: i64,
    pub name: String,
    pub saved_name: Option<String>,
    pub dirty: bool,
    pub list: Vec<String>,
    pub lib_sel: Option<String>,
    pub status: String,
    pub pos_text: String,
    pub state_text: String,
    pub sides: i64,
    board_rect: Rect,
}

impl Default for Drawer {
    fn default() -> Self {
        Self {
            open: true,
            focus: false,
            tool: DrawerTool::Poly,
            grid_n: 16,
            view: BoardView::default(),
            strokes: Vec::new(),
            draft: None,
            sel: None,
            drag: None,
            follow: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            redo_kept: Vec::new(),
            clipboard: None,
            pastes: 0,
            name: String::new(),
            saved_name: None,
            dirty: false,
            list: Vec::new(),
            lib_sel: None,
            status: String::new(),
            pos_text: String::new(),
            state_text: String::new(),
            sides: 6,
            board_rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(600.0)),
        }
    }
}

impl Drawer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a shape from the library (does not clear the library list; the caller refreshes) (upstream open_shape).
    pub fn open_shape(&mut self, name: &str, strokes: Vec<Stroke>) {
        self.strokes = strokes;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.redo_kept.clear();
        self.draft = None;
        self.sel = None;
        self.drag = None;
        self.follow = None;
        self.name = name.to_string();
        self.saved_name = (!name.is_empty()).then(|| name.to_string());
        self.dirty = false;
        self.status.clear();
    }

    // ------------------------------------------------------------ coordinates

    /// Pixels per board unit (short side minus the margin).
    fn px(&self) -> f64 {
        ((self.board_rect.width().min(self.board_rect.height()) as f64) - 48.0).max(10.0)
            * self.view.zoom
    }

    fn to_screen(&self, u: f64, v: f64) -> Pos2 {
        let k = self.px();
        let c = self.board_rect.center();
        Pos2::new(
            c.x + ((u - self.view.center[0]) * k) as f32,
            c.y - ((v - self.view.center[1]) * k) as f32,
        )
    }

    fn uv_at(&self, x: f32, y: f32) -> Pt {
        let k = self.px();
        let c = self.board_rect.center();
        [
            self.view.center[0] + (x - c.x) as f64 / k,
            self.view.center[1] - (y - c.y) as f64 / k,
        ]
    }

    fn screen_map(&self) -> impl Fn(Pt) -> [f64; 2] + '_ {
        move |p| {
            let s = self.to_screen(p[0], p[1]);
            [s.x as f64, s.y as f64]
        }
    }

    fn point_map(&self) -> impl Fn(f64, f64) -> Pt + '_ {
        move |x, y| self.uv_at(x as f32, y as f32)
    }

    fn event_pt(&self, pos: Pos2, snap: bool, shift: bool) -> Pt {
        let p = self.uv_at(pos.x, pos.y);
        if snap {
            dt::snap_uv(p[0], p[1], self.grid_n, shift)
        } else {
            [dt::round5(p[0]), dt::round5(p[1])]
        }
    }

    /// Zoom (around the mouse position), upstream on_wheel.
    fn zoom_at(&mut self, pos: Pos2, up: bool) {
        let before = self.uv_at(pos.x, pos.y);
        self.view.zoom = (self.view.zoom * if up { 1.25 } else { 0.8 }).clamp(0.05, 64.0);
        let after = self.uv_at(pos.x, pos.y);
        self.view.center[0] += before[0] - after[0];
        self.view.center[1] += before[1] - after[1];
    }

    // ------------------------------------------------------------ editing

    /// A step to undo: the strokes as they were, and the redo steps are dropped
    /// (upstream push_undo; the undone steps stay in redo_kept for a click that moves nothing).
    fn push_undo(&mut self) {
        self.undo_stack.push(self.strokes.clone());
        if self.undo_stack.len() > 200 {
            self.undo_stack.remove(0);
        }
        self.redo_kept = std::mem::take(&mut self.redo_stack);
    }

    fn undo(&mut self) {
        if self.draft.is_some() {
            self.cancel_draft();
            return;
        }
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.strokes.clone());
            self.strokes = prev;
            self.sel = None;
            self.changed();
        }
    }

    fn redo(&mut self) {
        if self.draft.is_some() {
            self.cancel_draft();
            return;
        }
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.strokes.clone());
            self.strokes = next;
            self.sel = None;
            self.changed();
        }
    }

    fn cancel_draft(&mut self) {
        self.draft = None;
        self.drag = None;
        self.follow = None;
    }

    fn changed(&mut self) {
        self.dirty = true;
        if self.sel.is_some_and(|i| i >= self.strokes.len()) {
            self.sel = None;
        }
    }

    fn commit_stroke(&mut self, st: Stroke) {
        self.push_undo();
        self.strokes.push(st);
        if matches!(self.strokes.last(), Some(Stroke::Curve { .. })) {
            self.sel = Some(self.strokes.len() - 1); // a just-finished curve can be bent by its handles right away
        }
        self.changed();
    }

    fn delete_selected_stroke(&mut self) {
        let Some(i) = self.sel else {
            return;
        };
        if i >= self.strokes.len() {
            return;
        }
        self.push_undo();
        self.strokes.remove(i);
        self.sel = None;
        self.changed();
    }

    fn set_tool(&mut self, tool: DrawerTool) {
        if self.tool == tool {
            return;
        }
        self.tool = tool;
        self.sel = None;
        self.cancel_draft();
    }

    // ------------------------------------------------------------ hit testing

    fn hit_stroke(&self, pos: Pos2) -> Option<usize> {
        let map = self.screen_map();
        dt::stroke_at(&self.strokes, &map, pos.x as f64, pos.y as f64, 8.0)
    }

    /// In the selected curve, the anchor / handle point number under the mouse (ends excluded).
    fn pen_handle_at(&self, pos: Pos2) -> Option<(usize, usize)> {
        let i = dt::selected_curve(&self.strokes, self.sel)?;
        let Stroke::Curve { pts, .. } = &self.strokes[i] else {
            return None;
        };
        let map = self.screen_map();
        for (j, kind) in bezier::pen_handles(pts, true, &[]).into_iter().rev() {
            if kind == HandleKind::End || j >= pts.len() {
                continue;
            }
            let s = map(pts[j]);
            if (s[0] - pos.x as f64).abs() <= 8.0 && (s[1] - pos.y as f64).abs() <= 8.0 {
                return Some((i, j));
            }
        }
        None
    }

    /// Draggable points with the same (u, v) (polyline and curve ends count, ellipses don't) (the group of upstream select_press).
    fn point_group(&self, u: f64, v: f64) -> Vec<(usize, usize)> {
        let mut group = Vec::new();
        for (i, spot, p) in dt::handles(&self.strokes, self.sel) {
            if let Spot::Point(j) = spot
                && (p[0] - u).abs() < 1e-6
                && (p[1] - v).abs() < 1e-6
            {
                group.push((i, j));
            }
        }
        group
    }

    /// Drags a curve point (anchor / handle), the exact mode of upstream drag_point.
    fn drag_pen(&mut self, i: usize, j: usize, pt: Pt, alt: bool) {
        let Some(Stroke::Curve {
            pts, sharp, sym, ..
        }) = self.strokes.get(i)
        else {
            return;
        };
        let mut c = bezier::Curve {
            pts: pts.clone(),
            sharp: sharp.clone(),
            sym: *sym,
            ..Default::default()
        };
        {
            let to_screen = self.screen_map();
            let from_screen = self.point_map();
            bezier::drag_point(&mut c, j, pt, alt, &to_screen, &from_screen, true);
        }
        if let Some(Stroke::Curve {
            pts, sharp, sym, ..
        }) = self.strokes.get_mut(i)
        {
            *pts = c.pts;
            *sharp = c.sharp;
            *sym = c.sym;
        }
    }

    /// Curve symmetry switch (upstream set_curve_symmetry; source is which half of the curve the mouse is on).
    fn set_symmetry_mode(&mut self, mode: Option<Sym>, mouse: Option<Pos2>) {
        let Some(i) = dt::selected_curve(&self.strokes, self.sel) else {
            return;
        };
        let Some(Stroke::Curve {
            pts, sharp, sym, ..
        }) = self.strokes.get(i)
        else {
            return;
        };
        if *sym == mode {
            return;
        }
        let mut c = bezier::Curve {
            pts: pts.clone(),
            sharp: sharp.clone(),
            sym: *sym,
            ..Default::default()
        };
        let source = match mouse {
            Some(p) => {
                let map = self.screen_map();
                bezier::half_at(&c.pts, &map, p.x as f64, p.y as f64)
            }
            None => 0,
        };
        self.push_undo();
        {
            let map = self.screen_map();
            bezier::set_symmetry(&mut c, mode, source as u8, &map, true);
        }
        if let Some(Stroke::Curve {
            pts, sharp, sym, ..
        }) = self.strokes.get_mut(i)
        {
            *pts = c.pts;
            *sharp = c.sharp;
            *sym = c.sym;
        }
        self.changed();
    }

    /// Right-click on a selected curve point: an anchor is deleted / a handle retracts (upstream delete_point).
    fn delete_pen_point(&mut self, pos: Pos2) -> bool {
        let Some((i, j)) = self.pen_handle_at(pos) else {
            return false;
        };
        let Some(Stroke::Curve {
            pts, sharp, sym, ..
        }) = self.strokes.get(i)
        else {
            return false;
        };
        let mut c = bezier::Curve {
            pts: pts.clone(),
            sharp: sharp.clone(),
            sym: *sym,
            ..Default::default()
        };
        match bezier::can_delete(&c, j) {
            None => true,
            Some(CanDelete::Middle) => {
                self.status = rust_i18n::t!("status.middle_anchor").to_string();
                true
            }
            Some(_) => {
                self.push_undo();
                {
                    let map = self.screen_map();
                    bezier::delete_point(&mut c, j, &map, true);
                }
                if let Some(Stroke::Curve {
                    pts, sharp, sym, ..
                }) = self.strokes.get_mut(i)
                {
                    *pts = c.pts;
                    *sharp = c.sharp;
                    *sym = c.sym;
                }
                self.changed();
                true
            }
        }
    }

    fn copy_strokes(&mut self) {
        if self.strokes.is_empty() {
            return;
        }
        let idx = dt::targets(self.sel, self.strokes.len());
        self.clipboard = Some(idx.iter().map(|&i| self.strokes[i].clone()).collect());
        self.pastes = 0;
    }

    fn paste_strokes(&mut self) {
        let Some(clip) = self.clipboard.clone() else {
            return;
        };
        if self.draft.is_some() {
            return;
        }
        self.pastes += 1;
        let d = self.pastes as f64 / self.grid_n as f64;
        self.push_undo();
        let new: Vec<Stroke> = clip
            .iter()
            .map(|st| {
                custom::map_stroke(st, |u, v| [dt::round5(u + d), dt::round5(v - d)], 1.0, 1.0)
            })
            .collect();
        self.sel = (new.len() == 1).then_some(self.strokes.len());
        self.strokes.extend(new);
        self.changed();
    }

    // ------------------------------------------------------------ mouse

    fn on_press(&mut self, pos: Pos2, input: &Input) {
        let pt = self.event_pt(pos, true, input.shift);
        if let Some((start, tool)) = self.follow.take() {
            self.box_draft(tool, start, pt, input.ctrl);
            self.finish_box();
            return;
        }
        if self.tool == DrawerTool::Select {
            self.select_press(pos, pt, input);
            return;
        }
        if let Some((i, j)) = self.pen_handle_at(pos) {
            self.push_undo();
            self.drag = Some(BoardDrag::Pen { i, j });
            return;
        }
        match self.tool {
            DrawerTool::Erase => {
                if let Some(i) = self.hit_stroke(pos) {
                    self.push_undo();
                    self.strokes.remove(i);
                    self.sel = None;
                    self.changed();
                }
            }
            DrawerTool::Poly => {
                if self.draft.is_none() {
                    self.draft = Some(Stroke::Poly {
                        pts: vec![pt, pt],
                        free: false,
                        smooth: 0,
                        k: 1.0,
                        src: None,
                    });
                    self.drag = Some(BoardDrag::Poly);
                } else {
                    self.poly_click(pos, pt);
                }
            }
            DrawerTool::Free => {
                self.draft = Some(Stroke::Poly {
                    pts: vec![pt],
                    free: false,
                    smooth: 0,
                    k: 1.0,
                    src: None,
                });
                self.drag = Some(BoardDrag::Free { last: pos });
            }
            DrawerTool::Arc => match &mut self.draft {
                None => {
                    self.draft = Some(Stroke::Arc {
                        pts: vec![pt, pt],
                        k: 1.0,
                        src: None,
                    });
                }
                Some(Stroke::Arc { pts, .. }) => {
                    if pts.len() == 2 {
                        pts[1] = pt;
                        pts.push(pt);
                    } else {
                        pts[2] = pt;
                        self.finish_arc();
                    }
                }
                _ => {}
            },
            tool if tool.is_box() => {
                self.drag = Some(BoardDrag::Box { start: pt });
            }
            _ => {}
        }
    }

    fn on_drag(&mut self, pos: Pos2, input: &Input) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        match drag {
            BoardDrag::Pan { start, center } => {
                let k = self.px();
                self.view.center = [
                    center[0] - (pos.x - start.x) as f64 / k,
                    center[1] + (pos.y - start.y) as f64 / k,
                ];
                self.drag = Some(BoardDrag::Pan { start, center });
            }
            BoardDrag::Box { start } => {
                let pt = self.event_pt(pos, true, input.shift);
                self.box_draft(self.tool, start, pt, input.ctrl);
                self.drag = Some(BoardDrag::Box { start });
            }
            BoardDrag::Free { last } => {
                let pt = self.event_pt(pos, false, input.shift);
                if pos.distance(last) >= 3.0 {
                    if let Some(Stroke::Poly { pts, .. }) = &mut self.draft {
                        pts.push(pt);
                    }
                    self.drag = Some(BoardDrag::Free { last: pos });
                } else {
                    self.drag = Some(BoardDrag::Free { last });
                }
            }
            BoardDrag::Poly => {
                let pt = self.event_pt(pos, true, input.shift);
                if let Some(Stroke::Poly { pts, .. }) = &mut self.draft
                    && let Some(last) = pts.last_mut()
                {
                    *last = pt;
                }
                self.drag = Some(BoardDrag::Poly);
            }
            BoardDrag::Move { i, start, orig } => {
                let cur = self.event_pt(pos, false, input.shift);
                let (mut du, mut dv) = (cur[0] - start[0], cur[1] - start[1]);
                if !input.shift {
                    let nf = self.grid_n as f64;
                    du = (du * nf).round() / nf;
                    dv = (dv * nf).round() / nf;
                }
                let moved = custom::map_stroke(
                    &orig,
                    |u, v| [dt::round5(u + du), dt::round5(v + dv)],
                    1.0,
                    1.0,
                );
                if let Some(st) = self.strokes.get_mut(i) {
                    *st = moved;
                }
                self.drag = Some(BoardDrag::Move { i, start, orig });
            }
            BoardDrag::Points { group, start } => {
                let pt = self.event_pt(pos, true, input.shift);
                for (a, b) in group.iter().copied() {
                    if matches!(self.strokes.get(a), Some(Stroke::Curve { .. })) {
                        self.drag_pen(a, b, pt, input.alt);
                    } else if let Some(st) = self.strokes.get_mut(a)
                        && let Some(pts) = match st {
                            Stroke::Poly { pts, .. }
                            | Stroke::Curve { pts, .. }
                            | Stroke::Arc { pts, .. } => Some(pts),
                            Stroke::Ellipse { .. } => None,
                        }
                        && let Some(p) = pts.get_mut(b)
                    {
                        *p = pt;
                    }
                }
                self.drag = Some(BoardDrag::Points { group, start });
            }
            BoardDrag::Corner { i, k, box_ } => {
                let corners = [
                    [box_[0], box_[1]],
                    [box_[2], box_[1]],
                    [box_[2], box_[3]],
                    [box_[0], box_[3]],
                ];
                let mut pt = self.event_pt(pos, true, input.shift);
                let opp = corners[(k + 2) % 4];
                if input.ctrl {
                    pt = dt::perfect(opp, pt);
                }
                let new = [
                    opp[0].min(pt[0]),
                    opp[1].min(pt[1]),
                    opp[0].max(pt[0]),
                    opp[1].max(pt[1]),
                ];
                if let Some(Stroke::Ellipse { box_, .. }) = self.strokes.get_mut(i) {
                    *box_ = new;
                }
                self.drag = Some(BoardDrag::Corner { i, k, box_ });
            }
            BoardDrag::Pen { i, j } => {
                let pt = self.event_pt(pos, true, input.shift);
                self.drag_pen(i, j, pt, input.alt);
                self.drag = Some(BoardDrag::Pen { i, j });
            }
        }
    }

    /// Draft following while no mouse button is held (the polyline / arc cursor point, the click-move-click box).
    fn on_hover_draft(&mut self, pos: Pos2, input: &Input) {
        let pt = self.event_pt(pos, true, input.shift);
        if let Some((start, tool)) = self.follow {
            self.box_draft(tool, start, pt, input.ctrl);
            return;
        }
        match &mut self.draft {
            Some(Stroke::Poly { pts, .. }) if self.tool == DrawerTool::Poly => {
                if let Some(last) = pts.last_mut() {
                    *last = pt;
                }
            }
            Some(Stroke::Arc { pts, .. }) => {
                if pts.len() == 2 {
                    pts[1] = pt;
                } else if pts.len() >= 3 {
                    pts[2] = pt;
                }
            }
            _ => {}
        }
    }

    fn on_release(&mut self, pos: Pos2, input: &Input) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        match drag {
            BoardDrag::Pan { .. } => {}
            BoardDrag::Box { start } => {
                let moved = input.press.is_some_and(|p| p.distance(pos) >= 4.0);
                if moved {
                    if self.draft.is_none() {
                        let pt = self.event_pt(pos, true, input.shift);
                        self.box_draft(self.tool, start, pt, input.ctrl);
                    }
                    self.finish_box();
                } else {
                    // clicked: the stroke follows the mouse and the next click fixes it (upstream follow)
                    self.draft = None;
                    self.follow = Some((start, self.tool));
                }
            }
            BoardDrag::Free { .. } => {
                let close = self
                    .draft
                    .as_ref()
                    .and_then(|st| match st {
                        Stroke::Poly { pts, .. } => pts.first().map(|p| self.to_screen(p[0], p[1])),
                        _ => None,
                    })
                    .is_some_and(|first| pos.distance(first) < 12.0);
                let ok = match &mut self.draft {
                    Some(Stroke::Poly { pts, .. }) => {
                        if close && pts.len() >= 3 {
                            pts.push(pts[0]);
                        }
                        pts.len() >= 2
                    }
                    _ => false,
                };
                if ok {
                    if let Some(st) = self.draft.take() {
                        self.commit_stroke(st);
                    }
                } else {
                    self.cancel_draft();
                }
            }
            BoardDrag::Poly => {}
            BoardDrag::Move { .. }
            | BoardDrag::Points { .. }
            | BoardDrag::Corner { .. }
            | BoardDrag::Pen { .. } => {
                // Clicked without moving anything: drop this press's snapshot (upstream select_release;
                // the undone steps stay redoable).
                if self.undo_stack.last() == Some(&self.strokes) {
                    self.undo_stack.pop();
                    self.redo_stack = std::mem::take(&mut self.redo_kept);
                } else {
                    self.changed();
                }
            }
        }
    }

    fn on_double(&mut self) {
        if self.draft.is_some() && self.tool == DrawerTool::Poly {
            self.finish_poly();
        }
    }

    fn right_click(&mut self, pos: Pos2) {
        if self.draft.is_some() || self.follow.is_some() {
            if self.tool == DrawerTool::Poly && self.draft.is_some() {
                self.finish_poly();
            } else {
                self.cancel_draft();
            }
            return;
        }
        if self.delete_pen_point(pos) {
            return;
        }
        match self.hit_stroke(pos) {
            Some(i) => {
                self.sel = Some(i);
                self.changed();
            }
            None => self.sel = None,
        }
    }

    fn select_press(&mut self, pos: Pos2, pt: Pt, input: &Input) {
        let hit = {
            let map = self.screen_map();
            dt::handle_at(
                &self.strokes,
                self.sel,
                &map,
                pos.x as f64,
                pos.y as f64,
                8.0,
            )
        };
        if let Some((i, spot)) = hit {
            self.push_undo();
            self.sel = Some(i);
            self.drag = Some(match spot {
                Spot::Corner(k) => match self.strokes.get(i) {
                    Some(Stroke::Ellipse { box_, .. }) => BoardDrag::Corner { i, k, box_: *box_ },
                    _ => return,
                },
                Spot::Pen { j, .. } => BoardDrag::Pen { i, j },
                Spot::Point(j) => {
                    let p = dt::handles(&self.strokes, Some(i))
                        .into_iter()
                        .find(|(a, b, _)| *a == i && *b == Spot::Point(j))
                        .map(|(_, _, p)| p)
                        .unwrap_or(pt);
                    BoardDrag::Points {
                        group: self.point_group(p[0], p[1]),
                        start: pt,
                    }
                }
            });
            return;
        }
        match self.hit_stroke(pos) {
            Some(i) => {
                self.push_undo();
                self.sel = Some(i);
                self.drag = Some(BoardDrag::Move {
                    i,
                    start: self.event_pt(pos, false, input.shift),
                    orig: self.strokes[i].clone(),
                });
            }
            None => {
                self.sel = None;
                self.drag = Some(BoardDrag::Pan {
                    start: pos,
                    center: self.view.center,
                });
            }
        }
    }

    /// The polyline's next click (upstream poly_point): a click near the start closes and finishes it.
    fn poly_click(&mut self, pos: Pos2, pt: Pt) {
        let first = match &self.draft {
            Some(Stroke::Poly { pts, .. }) => pts.first().copied(),
            _ => None,
        };
        let Some(first) = first else {
            return;
        };
        let first_screen = self.to_screen(first[0], first[1]);
        let Some(Stroke::Poly { pts, .. }) = &mut self.draft else {
            return;
        };
        if pts.len() >= 3 && pos.distance(first_screen) < 8.0 {
            let n = pts.len();
            pts[n - 1] = pts[0];
            let done = dt::dedupe_points(&pts.clone());
            self.draft = None;
            self.drag = None;
            self.commit_stroke(Stroke::Poly {
                pts: done,
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            });
            return;
        }
        let last = pts.len() - 1;
        pts[last] = pt;
        pts.push(pt);
    }

    /// Finishes the polyline (Enter / double-click / right-click) (upstream finish_poly).
    fn finish_poly(&mut self) {
        let Some(Stroke::Poly { pts, .. }) = &self.draft else {
            return;
        };
        let mut done = pts.clone();
        done.pop(); // the last one is the point following the mouse
        let done = dt::dedupe_points(&done);
        self.draft = None;
        self.drag = None;
        if done.len() >= 2 {
            self.commit_stroke(Stroke::Poly {
                pts: done,
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            });
        }
    }

    /// Finishes the three-point arc (upstream commit).
    fn finish_arc(&mut self) {
        if let Some(st) = self.draft.take() {
            self.drag = None;
            self.commit_stroke(st);
        }
    }

    /// Draft for the box tools (with Ctrl, square / circle stay regular).
    fn box_draft(&mut self, tool: DrawerTool, start: Pt, pt: Pt, ctrl: bool) {
        let pt = if ctrl && matches!(tool, DrawerTool::Square | DrawerTool::Circle) {
            dt::perfect(start, pt)
        } else {
            pt
        };
        self.draft = Some(match tool {
            DrawerTool::Line => Stroke::Poly {
                pts: vec![start, pt],
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            },
            DrawerTool::Curve => Stroke::Curve {
                pts: dt::curve_pts(start, pt),
                sharp: Vec::new(),
                sym: None,
                src: None,
            },
            DrawerTool::Square => Stroke::Poly {
                pts: dt::box_pts(start, pt),
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            },
            DrawerTool::Triangle => Stroke::Poly {
                pts: dt::triangle_pts(start, pt),
                free: false,
                smooth: 0,
                k: 1.0,
                src: None,
            },
            _ => Stroke::Ellipse {
                box_: [
                    start[0].min(pt[0]),
                    start[1].min(pt[1]),
                    start[0].max(pt[0]),
                    start[1].max(pt[1]),
                ],
                src: None,
            },
        });
    }

    /// Decides on release whether the draft is big enough (the ok of upstream on_release).
    fn finish_box(&mut self) {
        let tool = self.tool;
        let Some(st) = self.draft.take() else {
            return;
        };
        let ok = match &st {
            Stroke::Ellipse { box_, .. } => box_[2] > box_[0] && box_[3] > box_[1],
            Stroke::Poly { pts, .. } => {
                let us: Vec<f64> = pts.iter().map(|p| p[0]).collect();
                let vs: Vec<f64> = pts.iter().map(|p| p[1]).collect();
                let wide = us.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                    > us.iter().copied().fold(f64::INFINITY, f64::min);
                let tall = vs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                    > vs.iter().copied().fold(f64::INFINITY, f64::min);
                if matches!(tool, DrawerTool::Line) {
                    wide || tall
                } else {
                    wide && tall
                }
            }
            Stroke::Curve { pts, .. } => {
                let us: Vec<f64> = pts.iter().map(|p| p[0]).collect();
                let vs: Vec<f64> = pts.iter().map(|p| p[1]).collect();
                us.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                    > us.iter().copied().fold(f64::INFINITY, f64::min)
                    || vs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                        > vs.iter().copied().fold(f64::INFINITY, f64::min)
            }
            _ => false,
        };
        if ok {
            self.commit_stroke(st);
        }
    }

    // ------------------------------------------------------------ shape library actions

    /// Refreshes the library list (upstream refresh_list); selects select when it is in the list.
    pub fn refresh_list(&mut self, dir: &Path, select: Option<&str>) {
        self.list = library_names(dir);
        if let Some(name) = select
            && self.list.iter().any(|n| n == name)
        {
            self.lib_sel = Some(name.to_string());
        }
    }

    /// The name saved into the library; None when it was not saved (upstream save).
    pub fn save(&mut self, dir: &Path) -> Option<String> {
        let name = clean_name(&self.name);
        if self.strokes.is_empty() {
            self.status = rust_i18n::t!("drawer.draw_first").to_string();
            return None;
        }
        if name.is_empty() {
            self.status = rust_i18n::t!("drawer.name_first").to_string();
            return None;
        }
        let taken = library_names(dir)
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&name));
        let same = self
            .saved_name
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(&name));
        if taken && !same && !confirm(rust_i18n::t!("drawer.replace", name = name.clone()).as_ref())
        {
            return None;
        }
        self.strokes = join_strokes(&self.strokes);
        self.sel = None;
        if let Err(e) = save_shape(dir, &name, &self.strokes) {
            self.status = rust_i18n::t!("drawer.save_failed", e = e.to_string()).to_string();
            return None;
        }
        self.name = name.clone();
        self.saved_name = Some(name.clone());
        self.dirty = false;
        self.refresh_list(dir, Some(&name));
        self.status = rust_i18n::t!("drawer.saved", name = name.clone()).to_string();
        Some(name)
    }

    /// Use: with no changes, use the saved name directly; otherwise save first (upstream use).
    fn use_shape(&mut self, dir: &Path) -> Option<String> {
        let same =
            self.saved_name.as_deref() == Some(clean_name(&self.name).as_str()) && !self.dirty;
        if same {
            self.saved_name.clone()
        } else {
            self.save(dir)
        }
    }

    /// Confirms when there are changes (upstream keep_changes).
    fn keep_changes(&self) -> bool {
        !(self.dirty && !self.strokes.is_empty())
            || confirm(rust_i18n::t!("drawer.discard").as_ref())
    }

    fn open_selected(&mut self, dir: &Path) {
        // no shape selected: upstream does nothing at all
        let Some(name) = self.lib_sel.clone() else {
            return;
        };
        if !self.keep_changes() {
            return;
        }
        match load_shape(dir, &name) {
            Some(strokes) => self.open_shape(&name, strokes),
            None => {
                self.status = rust_i18n::t!("drawer.read_failed", name = name.clone()).to_string()
            }
        }
    }

    fn new_shape(&mut self) {
        if self.keep_changes() {
            self.open_shape("", Vec::new());
        }
    }

    fn delete_selected(&mut self, dir: &Path) {
        let Some(name) = self.lib_sel.clone() else {
            return;
        };
        let saved = saved_names(dir)
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&name));
        if !saved {
            self.status = rust_i18n::t!("drawer.builtin", name = name.clone()).to_string();
            return;
        }
        let back = builtin_name(&name)
            .map(|b| rust_i18n::t!("drawer.builtin_back", b = b).to_string())
            .unwrap_or_default();
        if !confirm(rust_i18n::t!("drawer.delete_ask", name = name.clone(), back = back).as_ref()) {
            return;
        }
        if let Err(e) = std::fs::remove_file(shape_file(dir, &name)) {
            self.status = rust_i18n::t!("drawer.delete_failed", e = e.to_string()).to_string();
        }
        self.refresh_list(dir, None);
        self.lib_sel = None;
    }

    /// Rename is a port addition (upstream's board has no rename): the error messages reuse upstream wording where possible.
    fn rename_selected(&mut self, dir: &Path) {
        let Some(old) = self.lib_sel.clone() else {
            return;
        };
        if !shape_file(dir, &old).exists() {
            self.status = rust_i18n::t!("drawer.builtin", name = old.clone()).to_string();
            return;
        }
        let new = clean_name(&self.name);
        if new.is_empty() || new.eq_ignore_ascii_case(&old) {
            return;
        }
        if shape_file(dir, &new).exists()
            && !confirm(rust_i18n::t!("drawer.replace", name = new.clone()).as_ref())
        {
            return;
        }
        if let Err(e) = std::fs::rename(shape_file(dir, &old), shape_file(dir, &new)) {
            self.status = rust_i18n::t!("drawer.rename_failed", e = e.to_string()).to_string();
            return;
        }
        if self.saved_name.as_deref() == Some(old.as_str()) {
            self.saved_name = Some(new.clone());
            self.name = new.clone();
        }
        self.refresh_list(dir, Some(&new));
    }

    // ------------------------------------------------------------ painting

    fn draw_stroke(&self, painter: &egui::Painter, st: &Stroke, color: Color32, width: f32) {
        let pts: Vec<Pos2> = custom::stroke_points(st)
            .iter()
            .map(|p| self.to_screen(p[0], p[1]))
            .collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
        } else if let Some(p) = pts.first() {
            painter.circle_filled(*p, 2.0, color);
        }
    }

    fn paint(&self, painter: &egui::Painter) {
        let (tl, br) = (self.to_screen(0.0, 1.0), self.to_screen(1.0, 0.0));
        let board = Rect::from_min_max(
            Pos2::new(tl.x.min(br.x), tl.y.min(br.y)),
            Pos2::new(tl.x.max(br.x), tl.y.max(br.y)),
        );
        painter.rect_filled(board, 0.0, Color32::WHITE);
        self.paint_grid(painter);
        painter.rect_stroke(
            board,
            0.0,
            egui::Stroke::new(1.0, BOARD_EDGE),
            egui::StrokeKind::Inside,
        );
        let w = 2.0_f32;
        for (i, st) in self.strokes.iter().enumerate() {
            let selected = self.sel == Some(i);
            self.draw_stroke(
                painter,
                st,
                if selected { SEL_COLOR } else { STROKE_COLOR },
                if selected { w + 1.0 } else { w },
            );
        }
        self.paint_handles(painter);
        for p in open_ends(&self.strokes) {
            painter.circle_filled(self.to_screen(p[0], p[1]), 4.0, OPEN_END_COLOR);
        }
        if let Some(st) = &self.draft {
            self.draw_stroke(painter, st, DRAFT_COLOR, w);
            self.paint_draft_points(painter, st);
        }
    }

    /// Grid (the grid part of upstream redraw): when too dense, draw only every fourth line.
    fn paint_grid(&self, painter: &egui::Painter) {
        let n = self.grid_n.max(1) as f64;
        let k = self.px() / n;
        let major = if (self.grid_n % 4 == 0) && self.grid_n > 4 {
            (self.grid_n / 4) as f64
        } else {
            n
        };
        let step = if k >= 5.0 {
            1.0
        } else if k * major >= 5.0 {
            major
        } else {
            0.0
        };
        if step <= 0.0 {
            return;
        }
        let rect = self.board_rect;
        let [u_lo, v_hi] = self.uv_at(rect.min.x, rect.min.y);
        let [u_hi, v_lo] = self.uv_at(rect.max.x, rect.max.y);
        for (lo, hi, vertical) in [(u_lo, u_hi, true), (v_lo, v_hi, false)] {
            let mut i = (lo * n / step).ceil() * step;
            while i <= hi * n {
                let color = if (i * 2.0 - n).abs() < 1e-9 {
                    GRID_MID
                } else if (i % major).abs() < 1e-9 {
                    GRID_MAJOR
                } else {
                    GRID_MINOR
                };
                if vertical {
                    let x = self.to_screen(i / n, 0.0).x;
                    painter.line_segment(
                        [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
                        egui::Stroke::new(1.0, color),
                    );
                } else {
                    let y = self.to_screen(0.0, i / n).y;
                    painter.line_segment(
                        [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
                        egui::Stroke::new(1.0, color),
                    );
                }
                i += step;
            }
        }
    }

    fn paint_handles(&self, painter: &egui::Painter) {
        let selected_curve = dt::selected_curve(&self.strokes, self.sel);
        if let Some(i) = selected_curve
            && let Some(Stroke::Curve { pts, .. }) = self.strokes.get(i)
        {
            for (a, h) in bezier::handle_lines(pts, &[]) {
                let pa = self.to_screen(a[0], a[1]);
                let ph = self.to_screen(h[0], h[1]);
                painter.line_segment([pa, ph], egui::Stroke::new(3.5, Color32::WHITE));
                painter.line_segment([pa, ph], egui::Stroke::new(1.5, HANDLE_COLOR));
            }
            for (j, kind) in bezier::pen_handles(pts, true, &[]) {
                if kind == HandleKind::End || j >= pts.len() {
                    continue;
                }
                let p = self.to_screen(pts[j][0], pts[j][1]);
                match kind {
                    HandleKind::Ctrl => {
                        painter.circle_filled(p, 4.5, HANDLE_COLOR);
                        painter.circle_stroke(p, 4.5, egui::Stroke::new(1.0, Color32::WHITE));
                    }
                    _ => {
                        painter.circle_filled(p, 5.5, Color32::WHITE);
                        painter.circle_stroke(p, 5.5, egui::Stroke::new(2.0, HANDLE_COLOR));
                    }
                }
            }
        }
        if self.tool != DrawerTool::Select {
            return;
        }
        for (i, spot, p) in dt::handles(&self.strokes, self.sel) {
            if matches!(spot, Spot::Pen { .. }) {
                continue; // curve anchors / handles were already drawn above
            }
            let q = self.to_screen(p[0], p[1]);
            let r = Rect::from_center_size(q, Vec2::splat(8.0));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(
                    2.0,
                    if self.sel == Some(i) {
                        SEL_COLOR
                    } else {
                        HANDLE_COLOR
                    },
                ),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn paint_draft_points(&self, painter: &egui::Painter, st: &Stroke) {
        let pts: Vec<Pt> = match st {
            Stroke::Ellipse { box_, .. } => vec![
                [box_[0], box_[1]],
                [box_[2], box_[1]],
                [box_[2], box_[3]],
                [box_[0], box_[3]],
            ],
            Stroke::Curve { pts, .. } => {
                if pts.is_empty() {
                    Vec::new()
                } else {
                    vec![pts[0], pts[pts.len() - 1]]
                }
            }
            Stroke::Poly { pts, .. } | Stroke::Arc { pts, .. } => pts.clone(),
        };
        for (j, p) in pts.iter().enumerate() {
            let q = self.to_screen(p[0], p[1]);
            if matches!(st, Stroke::Arc { .. }) && j == 1 && pts.len() == 3 {
                painter.circle_filled(q, 5.5, Color32::WHITE);
                painter.circle_stroke(q, 5.5, egui::Stroke::new(2.0, HANDLE_COLOR));
            } else {
                let r = Rect::from_center_size(q, Vec2::splat(7.0));
                painter.rect_filled(r, 0.0, Color32::WHITE);
                painter.rect_stroke(
                    r,
                    0.0,
                    egui::Stroke::new(1.0, DRAFT_COLOR),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    /// Status text (the end of upstream redraw).
    fn update_state_text(&mut self) {
        let mut text = if self.strokes.is_empty() {
            rust_i18n::t!("drawer.state_nothing").to_string()
        } else if strokes_closed(&self.strokes) {
            rust_i18n::t!("drawer.state_closed").to_string()
        } else if open_paths(&self.strokes).len() == 1 {
            rust_i18n::t!("drawer.state_one_gap").to_string()
        } else {
            rust_i18n::t!("drawer.state_open").to_string()
        };
        if self.dirty && !self.strokes.is_empty() {
            text += rust_i18n::t!("drawer.state_unsaved").as_ref();
        }
        self.state_text = text;
    }
}

// ---------------------------------------------------------------- input

struct Input {
    pos: Option<Pos2>,
    interact: Option<Pos2>,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    secondary_pressed: bool,
    middle_pressed: bool,
    middle_released: bool,
    double: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
    scroll_y: f32,
    press: Option<Pos2>,
}

fn read_input(ui: &egui::Ui) -> Input {
    ui.input(|i| Input {
        pos: i.pointer.hover_pos(),
        interact: i.pointer.interact_pos(),
        primary_pressed: i.pointer.primary_pressed(),
        primary_released: i.pointer.primary_released(),
        primary_down: i.pointer.primary_down(),
        secondary_pressed: i.pointer.secondary_pressed(),
        middle_pressed: i.pointer.button_pressed(egui::PointerButton::Middle),
        middle_released: i.pointer.button_released(egui::PointerButton::Middle),
        double: i
            .pointer
            .button_double_clicked(egui::PointerButton::Primary),
        shift: i.modifiers.shift,
        ctrl: i.modifiers.command || i.modifiers.ctrl,
        alt: i.modifiers.alt,
        scroll_y: i.smooth_scroll_delta.y,
        press: i.pointer.press_origin(),
    })
}

// ---------------------------------------------------------------- UI

/// Drawer window entry point: draws it when `App.drawer` is Some (upstream's Drawer window).
pub fn drawer_ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.drawer.take() else {
        return;
    };
    let dir = library_dir(app);
    let mut use_name: Option<String> = None;
    let mut open = d.open;
    let response = egui::Window::new(rust_i18n::t!("drawer.title"))
        .default_size([1000.0, 720.0])
        .min_size([700.0, 500.0])
        .resizable(true)
        .open(&mut open)
        .show(ctx, |ui| {
            d.toolbar_ui(ui);
            ui.separator();
            egui::Panel::right("drawer_side")
                .resizable(true)
                .default_size(280.0)
                .show(ui, |ui| {
                    if let Some(name) = d.side_ui(ui, &dir) {
                        use_name = Some(name);
                    }
                });
            egui::CentralPanel::default().show(ui, |ui| {
                d.board_ui(ui, ctx);
            });
        });
    d.open = open;

    if let Some(resp) = &response {
        let rect = resp.response.rect;
        if ctx.input(|i| i.pointer.any_pressed()) {
            let hover = ctx.input(|i| i.pointer.hover_pos());
            d.focus = hover.is_some_and(|p| rect.contains(p));
        }
    }

    if d.focus && !ctx.egui_wants_keyboard_input() {
        drawer_keys(&mut d, ctx);
    }
    d.update_state_text();

    if let Some(name) = use_name {
        app.use_custom(&name);
    }
    if !d.open && d.keep_changes() {
        app.drawer = None;
    } else {
        d.open = true;
        app.drawer = Some(d);
    }
}

/// Drawer shortcuts (only while the drawer has focus; upstream on_key and Ctrl+Z etc.).
fn drawer_keys(d: &mut Drawer, ctx: &egui::Context) {
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::COMMAND, Key::Z) || i.consume_key(Modifiers::CTRL, Key::Z) {
            d.undo();
        }
        if i.consume_key(Modifiers::COMMAND, Key::Y) || i.consume_key(Modifiers::CTRL, Key::Y) {
            d.redo();
        }
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            d.cancel_draft();
            d.sel = None;
        }
        if i.consume_key(Modifiers::NONE, Key::Enter) {
            d.finish_poly();
        }
        if i.consume_key(Modifiers::NONE, Key::Delete)
            || i.consume_key(Modifiers::NONE, Key::Backspace)
        {
            d.delete_selected_stroke();
        }
        for tool in DrawerTool::ALL {
            if let Some(key) = tool_key(tool)
                && i.consume_key(Modifiers::NONE, key)
            {
                d.set_tool(tool);
            }
        }
    });
}

fn tool_key(tool: DrawerTool) -> Option<Key> {
    Some(match tool.hotkey() {
        "v" => Key::V,
        "l" => Key::L,
        "p" => Key::P,
        "f" => Key::F,
        "c" => Key::C,
        "a" => Key::A,
        "s" => Key::S,
        "o" => Key::O,
        "t" => Key::T,
        "e" => Key::E,
        _ => return None,
    })
}

/// Drawer tool -> help topic id (upstream DRAWER_TOOL_TOPICS; Triangle is a port addition and uses the general topic).
fn tool_topic(tool: DrawerTool) -> &'static str {
    match tool {
        DrawerTool::Select => "drawer_select",
        DrawerTool::Line => "drawer_line",
        DrawerTool::Poly => "drawer_poly",
        DrawerTool::Free => "drawer_free",
        DrawerTool::Curve => "drawer_curve",
        DrawerTool::Arc => "drawer_arc",
        DrawerTool::Square => "drawer_square",
        DrawerTool::Circle => "drawer_circle",
        DrawerTool::Triangle => "drawer",
        DrawerTool::Erase => "drawer_erase",
    }
}

fn sym_text(sym: Option<Sym>) -> String {
    match sym {
        None => rust_i18n::t!("drawer.sym_off"),
        Some(Sym::Mirror) => rust_i18n::t!("drawer.sym_mirror"),
        Some(Sym::Turn) => rust_i18n::t!("drawer.sym_turn"),
    }
    .to_string()
}

impl Drawer {
    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in DrawerTool::ALL {
                let text = format!("{} ({})", tool.ui_label(), tool.hotkey().to_uppercase());
                if ui.selectable_label(self.tool == tool, text).clicked() {
                    self.set_tool(tool);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(rust_i18n::t!("drawer.grid"));
            egui::ComboBox::from_id_salt("drawer_grid")
                .selected_text(self.grid_n.to_string())
                .width(56.0)
                .show_ui(ui, |ui| {
                    for n in dt::GRIDS {
                        if ui
                            .selectable_label(self.grid_n == n, n.to_string())
                            .clicked()
                        {
                            self.grid_n = n;
                        }
                    }
                });
            if ui.button(rust_i18n::t!("drawer.undo")).clicked() {
                self.undo();
            }
            if ui.button(rust_i18n::t!("drawer.clear")).clicked() && !self.strokes.is_empty() {
                self.push_undo();
                self.strokes.clear();
                self.sel = None;
                self.changed();
            }
            if ui.button(rust_i18n::t!("drawer.reset_view")).clicked() {
                self.view = BoardView::default();
            }
            ui.label(
                rust_i18n::t!(
                    "drawer.zoom",
                    pct = format!("{:.0}", self.view.zoom * 100.0)
                )
                .to_string(),
            );
            if ui.button(rust_i18n::t!("drawer.copy_stroke")).clicked() {
                self.copy_strokes();
            }
            let can_paste = self.clipboard.is_some() && self.draft.is_none();
            if ui
                .add_enabled(can_paste, egui::Button::new(rust_i18n::t!("drawer.paste")))
                .clicked()
            {
                self.paste_strokes();
            }
        });
        let has_sel = self.sel.is_some();
        let mut sym_pick: Option<Option<Sym>> = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    has_sel,
                    egui::Button::new(rust_i18n::t!("drawer.delete_stroke")),
                )
                .clicked()
            {
                self.delete_selected_stroke();
            }
            if ui
                .add_enabled(
                    has_sel,
                    egui::Button::new(rust_i18n::t!("drawer.flip_sideways")),
                )
                .clicked()
            {
                self.flip_or_turn(true);
            }
            if ui
                .add_enabled(
                    has_sel,
                    egui::Button::new(rust_i18n::t!("drawer.flip_upside_down")),
                )
                .clicked()
            {
                self.flip_or_turn(false);
            }
            if ui
                .add_enabled(has_sel, egui::Button::new(rust_i18n::t!("menu.turn_left")))
                .clicked()
            {
                self.turn(true);
            }
            if ui
                .add_enabled(has_sel, egui::Button::new(rust_i18n::t!("menu.turn_right")))
                .clicked()
            {
                self.turn(false);
            }
            let is_curve = dt::selected_curve(&self.strokes, self.sel).is_some();
            let now = match self.sel.and_then(|i| self.strokes.get(i)) {
                Some(Stroke::Curve { sym, .. }) => *sym,
                _ => None,
            };
            ui.add_enabled_ui(is_curve, |ui| {
                egui::ComboBox::from_id_salt("drawer_sym")
                    .selected_text(sym_text(now))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for (label, mode) in [
                            (rust_i18n::t!("drawer.sym_off"), None),
                            (rust_i18n::t!("drawer.sym_mirror"), Some(Sym::Mirror)),
                            (rust_i18n::t!("drawer.sym_turn"), Some(Sym::Turn)),
                        ] {
                            if ui.selectable_label(now == mode, label).clicked() {
                                sym_pick = Some(mode);
                            }
                        }
                    });
            });
            ui.label(rust_i18n::t!("drawer.sides"));
            ui.add(egui::DragValue::new(&mut self.sides).range(3..=64));
            if ui.button(rust_i18n::t!("drawer.polygon")).clicked() {
                let pts = dt::polygon_pts([0.5, 0.5], SIDES_SPAN, self.sides as usize);
                self.commit_stroke(Stroke::Poly {
                    pts,
                    free: false,
                    smooth: 0,
                    k: 1.0,
                    src: None,
                });
            }
        });
        if let Some(mode) = sym_pick {
            let mouse = ui.ctx().input(|i| i.pointer.hover_pos());
            self.set_symmetry_mode(mode, mouse);
        }
    }

    fn flip_or_turn(&mut self, sideways: bool) {
        let idx = dt::targets(self.sel, self.strokes.len());
        if idx.is_empty() {
            return;
        }
        self.push_undo();
        self.strokes = dt::flip_strokes(&self.strokes, &idx, sideways);
        self.changed();
    }

    fn turn(&mut self, counter_clockwise: bool) {
        let idx = dt::targets(self.sel, self.strokes.len());
        if idx.is_empty() {
            return;
        }
        self.push_undo();
        self.strokes = dt::turn_strokes(&self.strokes, &idx, self.grid_n, !counter_clockwise);
        self.changed();
    }

    fn side_ui(&mut self, ui: &mut egui::Ui, dir: &Path) -> Option<String> {
        let mut used = None;
        ui.heading(rust_i18n::t!("drawer.library"));
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .id_salt("drawer_library")
            .show(ui, |ui| {
                for name in self.list.clone() {
                    if ui
                        .selectable_label(self.lib_sel.as_deref() == Some(name.as_str()), &name)
                        .clicked()
                    {
                        self.lib_sel = Some(name);
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui.button(rust_i18n::t!("drawer.open")).clicked() {
                self.open_selected(dir);
            }
            if ui.button(rust_i18n::t!("drawer.delete")).clicked() {
                self.delete_selected(dir);
            }
            if ui.button(rust_i18n::t!("drawer.new")).clicked() {
                self.new_shape();
            }
        });
        ui.horizontal(|ui| {
            ui.label(rust_i18n::t!("drawer.name"));
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(150.0));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(rust_i18n::t!("drawer.save")).clicked() {
                self.save(dir);
            }
            if ui.button(rust_i18n::t!("drawer.save_as")).clicked() {
                self.save(dir);
            }
            if ui.button(rust_i18n::t!("drawer.rename")).clicked() {
                self.rename_selected(dir);
            }
            if ui.button(rust_i18n::t!("drawer.use")).clicked()
                && let Some(name) = self.use_shape(dir)
            {
                used = Some(name);
            }
        });
        ui.separator();
        ui.label(&self.pos_text);
        ui.label(
            egui::RichText::new(&self.state_text).color(if strokes_closed(&self.strokes) {
                Color32::from_rgb(0x1d, 0x6b, 0x1d)
            } else {
                Color32::from_rgb(0x9a, 0x4b, 0x00)
            }),
        );
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(&self.status).weak());
        }
        ui.separator();
        // Help at the bottom of the side panel: the current tool's + the general topic (upstream update_side_help)
        if let (Some(t), Some(dr)) = (
            crate::help_texts::by_id(tool_topic(self.tool)),
            crate::help_texts::by_id("drawer"),
        ) {
            ui.label(
                egui::RichText::new(
                    rust_i18n::t!(
                        "drawer.help",
                        tool = t.title,
                        tool_text = t.text,
                        drawer_text = dr.text
                    )
                    .to_string(),
                )
                .small()
                .weak(),
            );
        }
        used
    }

    fn board_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        self.board_rect = rect;
        let painter = ui.painter_at(rect);
        let input = read_input(ui);
        if let Some(pos) = input.pos.filter(|p| rect.contains(*p)) {
            let p = self.uv_at(pos.x, pos.y);
            let n = self.grid_n as f64;
            self.pos_text = rust_i18n::t!(
                "drawer.pos",
                x = format!("{:+.1}", (p[0] - 0.5) * n),
                y = format!("{:+.1}", (p[1] - 0.5) * n)
            )
            .to_string();
        }
        if input.scroll_y.abs() > 0.0
            && let Some(pos) = input.pos.filter(|p| rect.contains(*p))
        {
            self.zoom_at(pos, input.scroll_y > 0.0);
        }
        if input.middle_pressed
            && let Some(pos) = input.pos.filter(|p| rect.contains(*p))
        {
            self.drag = Some(BoardDrag::Pan {
                start: pos,
                center: self.view.center,
            });
        }
        if input.primary_pressed
            && let Some(pos) = input.pos.filter(|p| rect.contains(*p))
        {
            self.on_press(pos, &input);
        }
        if input.primary_down {
            if self.drag.is_some() {
                if let Some(pos) = input.interact.or(input.pos) {
                    self.on_drag(pos, &input);
                }
            } else if let Some(pos) = input.pos.filter(|p| rect.contains(*p)) {
                self.on_hover_draft(pos, &input);
            }
        } else if let Some(pos) = input.pos.filter(|p| rect.contains(*p)) {
            // No button held: the polyline / arc cursor point and the click-then-follow box trail the mouse
            self.on_hover_draft(pos, &input);
        }
        if input.middle_released && matches!(self.drag, Some(BoardDrag::Pan { .. })) {
            self.drag = None;
        }
        if input.primary_released
            && let Some(pos) = input.interact.or(input.pos)
        {
            self.on_release(pos, &input);
        }
        if input.double && input.pos.is_some_and(|p| rect.contains(p)) {
            self.on_double();
        }
        if input.secondary_pressed
            && let Some(pos) = input.pos.filter(|p| rect.contains(*p))
        {
            self.right_click(pos);
        }
        self.paint(&painter);
        if input.pos.is_some_and(|p| rect.contains(p)) {
            let on_handle = if self.draft.is_none() {
                let map = self.screen_map();
                dt::handle_at(
                    &self.strokes,
                    self.sel,
                    &map,
                    input.pos.map(|p| p.x as f64).unwrap_or(0.0),
                    input.pos.map(|p| p.y as f64).unwrap_or(0.0),
                    8.0,
                )
                .is_some()
            } else {
                false
            };
            ctx.set_cursor_icon(if on_handle {
                egui::CursorIcon::Move
            } else {
                egui::CursorIcon::Crosshair
            });
        }
        let _ = response;
    }
}

impl App {
    /// The drawer's "Use on the piano roll": new custom shapes use this library shape (upstream app.use_custom).
    pub fn use_custom(&mut self, name: &str) {
        self.custom_defaults.shape = name.to_string();
        self.select(None, false);
        self.tool = Tool::Custom;
        self.cancel_draft();
        self.schedule_autosave();
    }

    /// Opens the drawer (upstream panel_custom.open_drawer): first loads the shape selected in the panel.
    pub fn open_drawer(&mut self) {
        if let Some(d) = self.drawer.as_mut() {
            d.open = true;
            d.focus = true;
            return;
        }
        let name = self.custom_defaults.shape.clone();
        let mut d = Drawer::new();
        if let Some(strokes) = load_shape(&self.library_dir, &name) {
            d.open_shape(&name, strokes);
        }
        let dir = self.library_dir.clone();
        d.refresh_list(&dir, Some(&name));
        self.drawer = Some(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "spiderweb-drawer-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn poly(pts: Vec<Pt>) -> Stroke {
        Stroke::Poly {
            pts,
            free: false,
            smooth: 0,
            k: 1.0,
            src: None,
        }
    }

    /// Upstream drawer undo/redo: undo keeps the step for redo, a new edit drops it, and a click
    /// that moved nothing gets the undone steps back.
    #[test]
    fn undo_and_redo_keep_the_steps() {
        let mut d = Drawer::new();
        d.strokes = vec![poly(vec![[0.0, 0.0], [1.0, 0.0]])];
        d.push_undo();
        d.strokes.push(poly(vec![[0.0, 1.0], [1.0, 1.0]]));
        d.push_undo();
        d.strokes.push(poly(vec![[0.0, 2.0], [1.0, 2.0]]));
        d.undo();
        assert_eq!(d.strokes.len(), 2);
        assert_eq!(d.redo_stack.len(), 1);
        d.redo();
        assert_eq!(d.strokes.len(), 3);
        assert!(d.redo_stack.is_empty());
        // Undo, then an edit: the undone step is dropped from redo (but kept for a click that
        // moved nothing, upstream redo_kept).
        d.undo();
        assert_eq!(d.redo_stack.len(), 1);
        d.push_undo();
        assert!(d.redo_stack.is_empty());
        assert_eq!(d.redo_kept.len(), 1);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let strokes = vec![
            poly(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]),
            Stroke::Ellipse {
                box_: [0.1, 0.2, 0.9, 0.8],
                src: None,
            },
            Stroke::Curve {
                // 7 points = two segments: sharp / sym only take effect then (upstream clean_curve drops short curves)
                pts: vec![
                    [0.0, 0.0],
                    [0.2, 0.0],
                    [0.4, 1.0],
                    [0.5, 0.5],
                    [0.6, 0.0],
                    [0.8, 1.0],
                    [1.0, 1.0],
                ],
                sharp: vec![1],
                sym: Some(Sym::Mirror),
                src: None,
            },
        ];
        save_shape(&dir, "Spider", &strokes).expect("write library file");
        let text = std::fs::read_to_string(shape_file(&dir, "Spider")).expect("read library file");
        assert!(
            text.contains("\"strokes\""),
            "the library file must have a strokes key"
        );
        let got = load_shape(&dir, "Spider").expect("read the shape back");
        assert_eq!(got, strokes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_falls_back_to_builtin() {
        let dir = temp_dir("builtin");
        let circle = load_shape(&dir, "Circle").expect("built-in circle");
        assert_eq!(
            circle,
            vec![Stroke::Ellipse {
                box_: [0.0, 0.0, 1.0, 1.0],
                src: None
            }]
        );
        assert!(load_shape(&dir, "Nothing").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_file_reads_as_none_not_builtin() {
        let dir = temp_dir("broken");
        std::fs::write(shape_file(&dir, "Circle"), "not json").expect("write broken file");
        assert!(load_shape(&dir, "Circle").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_names_merge_files_and_builtins() {
        let dir = temp_dir("names");
        save_shape(&dir, "Spider", &[poly(vec![[0.0, 0.0], [1.0, 0.0]])])
            .expect("write library file");
        save_shape(&dir, "circle", &[poly(vec![[0.0, 0.0], [1.0, 1.0]])])
            .expect("write same-named file");
        let names = library_names(&dir);
        assert!(names.contains(&"Spider".to_string()));
        assert!(
            !names.contains(&"Circle".to_string()),
            "the file overrides the built-in: only circle is kept"
        );
        assert!(names.contains(&"Square".to_string()));
        assert!(names.contains(&"Triangle".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_name_strips_bad_characters() {
        assert_eq!(clean_name("  my/weird:name*."), "myweirdname");
        assert_eq!(clean_name("ok"), "ok");
    }

    #[test]
    fn template_prefers_saved_shape_and_normalizes() {
        let dir = temp_dir("template");
        let raw = vec![poly(vec![[2.0, 1.0], [4.0, 1.0], [4.0, 3.0], [2.0, 3.0]])];
        save_shape(&dir, "Wide", &raw).expect("write library file");
        let (strokes, ratio) = library_template(&dir, "Wide").expect("template");
        match &strokes[0] {
            Stroke::Poly { pts, .. } => {
                let us: Vec<f64> = pts.iter().map(|p| p[0]).collect();
                let vs: Vec<f64> = pts.iter().map(|p| p[1]).collect();
                assert!((us.iter().copied().fold(f64::INFINITY, f64::min)).abs() < 1e-9);
                assert!((us.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 1.0).abs() < 1e-9);
                assert!((vs.iter().copied().fold(f64::INFINITY, f64::min)).abs() < 1e-9);
                assert!((vs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 1.0).abs() < 1e-9);
            }
            other => panic!("expected poly, got {other:?}"),
        }
        assert!((ratio - 1.0).abs() < 1e-9);
        // File missing: fall back to the built-in
        let (tri, _) = library_template(&dir, "Triangle").expect("built-in triangle");
        assert_eq!(tri.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_shape_flows_into_the_roll_custom_tool() {
        // Main flow: draw a shape -> Save -> use as a roll Custom template -> produces notes
        let dir = temp_dir("flow");
        let square = vec![poly(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.0, 0.0],
        ])];
        save_shape(&dir, "Box", &square).expect("write library file");
        let (strokes, aspect) = crate::roll_live::builtin_template(&dir, "Box").expect("template");
        assert!((aspect - 1.0).abs() < 1e-9);
        let shape = crate::roll_live::new_custom_parts(
            &Shape::default(),
            &custom::CustomDefaults::default(),
            "Box",
            &strokes,
            [0.0, 60.0],
            [4.0, 64.0],
        );
        assert_eq!(shape.kind, Kind::Custom);
        assert_eq!(shape.name, "Box");
        assert!(
            !custom::custom_notes(&shape, 960.0).is_empty(),
            "it must produce notes when placed on the roll"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_and_refit_keep_the_drawing_in_the_unit_box() {
        let raw = vec![poly(vec![
            [2.0, 2.0],
            [3.0, 2.0],
            [3.0, 4.0],
            [2.0, 4.0],
            [2.0, 2.0],
        ])];
        let (norm, ratio) = normalize_strokes(&raw);
        let pts = match &norm[0] {
            Stroke::Poly { pts, .. } => pts.clone(),
            other => panic!("expected poly, got {other:?}"),
        };
        assert!((pts[2][0] - 1.0).abs() < 1e-9);
        assert!((pts[2][1] - 1.0).abs() < 1e-9);
        assert!((ratio.unwrap_or(0.0) - 0.5).abs() < 1e-9);

        // refit: re-fits the frame when the strokes do not fill it (the frame moves, the drawing doesn't)
        let mut sh = Shape {
            kind: Kind::Custom,
            strokes: vec![poly(vec![[0.25, 0.25], [0.75, 0.75]])],
            pts: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            ..Shape::default()
        };
        custom::refit(&mut sh);
        match &sh.strokes[0] {
            Stroke::Poly { pts, .. } => {
                assert!((pts[0][0]).abs() < 1e-9 && (pts[0][1]).abs() < 1e-9);
                assert!((pts[1][0] - 1.0).abs() < 1e-9 && (pts[1][1] - 1.0).abs() < 1e-9);
            }
            other => panic!("expected poly, got {other:?}"),
        }
        assert!((sh.pts[0][0] - 0.25).abs() < 1e-9);
        assert!((sh.pts[0][1] - 0.25).abs() < 1e-9);
        assert!((sh.pts[1][0] - 0.75).abs() < 1e-9);
    }
}
