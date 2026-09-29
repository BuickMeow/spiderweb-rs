//! The tumour window and the graph window (upstream `window/tumour_window.py` and
//! `window/graph_window.py`).
//!
//! The side panel only shows a summary line and a button (`panel_tumour.py`); the settings
//! live in this window. It stays open while you work and follows the selection. Each setting
//! in [`spiderweb_core::tumour::GRAPH_KEYS`] has a "…" button that opens its graph window:
//! along the whole line the setting is multiplied by the graph (100 % = as typed), with drag /
//! add / remove points, presets and a formula.

use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

use spiderweb_core::Pt;
use spiderweb_core::joined::{shown_tumour, unify_tumours};
use spiderweb_core::shape::{Tumour, TumourShape, TumourSide, TumourWrap};
use spiderweb_core::tumour::{GRAPH_KEYS, GRAPH_LIMIT, clean_graph_pts};

use crate::app::App;

/// The tick settings (stored in beats, shown in ticks).
const TICKS: [&str; 3] = ["length", "dist", "ease"];

/// One number box of the tumour window.
struct NumRow {
    key: &'static str,
    label: String,
    unit: String,
    value: f64,
    lo: f64,
    hi: f64,
    speed: f64,
}

/// A number row parsed back into a tumour setting (the box shows ticks / percent).
fn row_change(key: &str, value: f64, ppq: f64) -> TumourChange {
    match key {
        "size" => TumourChange::Size(value),
        "length" => TumourChange::Length(value / ppq),
        "dist" => TumourChange::Dist(value / ppq),
        "rot" => TumourChange::Rot(value),
        "slant" => TumourChange::Slant(value / 100.0),
        "ease" => TumourChange::Ease(value / ppq),
        _ => TumourChange::Size(value),
    }
}

/// One tumour setting change (only the named field changes; the others stay as they are).
#[derive(Clone, Copy, Debug, PartialEq)]
enum TumourChange {
    On(bool),
    Shape(TumourShape),
    Size(f64),
    Length(f64),
    Dist(f64),
    Ease(f64),
    Rot(f64),
    Slant(f64),
    Side(TumourSide),
    Wrap(TumourWrap),
    Start(f64),
    End(f64),
    Fit(bool),
    Seed(i64),
}

impl TumourChange {
    fn key(self) -> &'static str {
        match self {
            TumourChange::On(_) => "on",
            TumourChange::Shape(_) => "shape",
            TumourChange::Size(_) => "size",
            TumourChange::Length(_) => "length",
            TumourChange::Dist(_) => "dist",
            TumourChange::Ease(_) => "ease",
            TumourChange::Rot(_) => "rot",
            TumourChange::Slant(_) => "slant",
            TumourChange::Side(_) => "side",
            TumourChange::Wrap(_) => "wrap",
            TumourChange::Start(_) => "start",
            TumourChange::End(_) => "end",
            TumourChange::Fit(_) => "fit",
            TumourChange::Seed(_) => "seed",
        }
    }

    fn apply(self, tm: &mut Tumour) {
        match self {
            TumourChange::On(v) => tm.on = v,
            TumourChange::Shape(v) => tm.shape = v,
            TumourChange::Size(v) => tm.size = v,
            TumourChange::Length(v) => tm.length = v,
            TumourChange::Dist(v) => tm.dist = v,
            TumourChange::Ease(v) => tm.ease = v,
            TumourChange::Rot(v) => tm.rot = v,
            TumourChange::Slant(v) => tm.slant = v,
            TumourChange::Side(v) => tm.side = v,
            TumourChange::Wrap(v) => tm.wrap = v,
            TumourChange::Start(v) => tm.start = v,
            TumourChange::End(v) => tm.end = v,
            TumourChange::Fit(v) => tm.fit = v,
            TumourChange::Seed(v) => tm.seed = v,
        }
    }
}

pub(crate) fn tumour_shape_name(s: TumourShape) -> String {
    match s {
        TumourShape::Triangle => rust_i18n::t!("tumour_window.triangle"),
        TumourShape::Square => rust_i18n::t!("tumour_window.square"),
        TumourShape::Circle => rust_i18n::t!("tumour_window.circle"),
        TumourShape::Parabola => rust_i18n::t!("tumour_window.parabola"),
    }
    .to_string()
}

fn tumour_side_name(s: TumourSide) -> String {
    match s {
        TumourSide::Alt => rust_i18n::t!("tumour_window.alternating"),
        TumourSide::Left => rust_i18n::t!("tumour_window.left"),
        TumourSide::Right => rust_i18n::t!("tumour_window.right"),
        TumourSide::Random => rust_i18n::t!("tumour_window.random"),
    }
    .to_string()
}

fn tumour_wrap_name(w: TumourWrap) -> String {
    match w {
        TumourWrap::Simple => rust_i18n::t!("tumour_window.straight"),
        TumourWrap::Wrap => rust_i18n::t!("tumour_window.bent_with_the_line"),
    }
    .to_string()
}

/// "New random"'s seed: 1..10^9 (upstream `random.randrange(1, 10**9)`).
fn random_seed() -> i64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(1);
    let mut rnd = spiderweb_core::pyrandom::PyRandom::new(t);
    rnd.randbelow(999_999_999) as i64 + 1
}

/// The tumour settings the window shows: like Python `panel_tumour.shown_tumours`, the first
/// selected line's that has tumours on (else the first one's that has any).
pub fn shown_tumours(app: &App) -> Option<Tumour> {
    let tms: Vec<Tumour> = app
        .tumour_targets()
        .into_iter()
        .filter_map(|i| shown_tumour(&app.shapes[i]).cloned())
        .collect();
    tms.iter()
        .find(|tm| tm.on)
        .cloned()
        .or_else(|| tms.first().cloned())
}

/// What the tumour window changes: one undo step per gesture while typing / dragging.
fn begin_group(app: &mut App, key: &str, fresh: bool) {
    if fresh {
        app.edit_key = None;
    }
    let edit = format!("tumour:{}:{:?}", key, app.sels);
    if app.edit_key.as_deref() != Some(edit.as_str()) {
        app.push_undo();
        app.edit_key = Some(edit);
    }
}

/// A tumour setting changed in the window (upstream `TumourWindow.set`).
fn set_value(app: &mut App, change: TumourChange, fresh: bool) {
    let targets = app.tumour_targets();
    if targets.is_empty() {
        return;
    }
    let key = change.key().to_string();
    begin_group(app, &key, fresh);
    let mixed = {
        let ons: Vec<bool> = targets
            .iter()
            .map(|&i| shown_tumour(&app.shapes[i]).is_some_and(|t| t.on))
            .collect();
        ons.iter().any(|&b| b) && !ons.iter().all(|&b| b)
    };
    let k = app.tumour_k();
    let shown = shown_tumours(app);
    if mixed && key == "on" && change == TumourChange::On(true) {
        // half ticked: the others get the settings shown
        for &i in &targets {
            let Some(sh) = app.shapes.get_mut(i) else {
                continue;
            };
            if shown_tumour(sh).is_some_and(|t| t.on) {
                continue;
            }
            sh.tumours.clear();
            sh.splits.clear();
            sh.tumour = shown.clone().map(|mut t| {
                t.k = k;
                t
            });
        }
    } else {
        // the settings only change shapes with tumours on (mixed) / that have any (else)
        let keep: Vec<usize> = if mixed || key != "on" || change != TumourChange::On(true) {
            targets
                .iter()
                .copied()
                .filter(|&i| match shown_tumour(&app.shapes[i]) {
                    Some(tm) => {
                        if mixed {
                            tm.on
                        } else {
                            true
                        }
                    }
                    None => false,
                })
                .collect()
        } else {
            targets.clone()
        };
        for i in keep {
            let Some(sh) = app.shapes.get_mut(i) else {
                continue;
            };
            unify_tumours(sh);
            let tm = sh
                .tumour
                .get_or_insert_with(|| shown.clone().unwrap_or_default());
            change.apply(tm);
            tm.k = k; // sizes as the roll looks now
        }
    }
    app.shapes_changed();
}

/// A graph changed in the graph window (the window already took its undo step) (upstream
/// `TumourWindow.set_graph`).
fn set_graph(app: &mut App, key: &str, pts: &[Pt]) {
    let targets = app.tumour_targets();
    if targets.is_empty() {
        return;
    }
    let cleaned = clean_graph_pts(pts.to_vec());
    let mixed = {
        let ons: Vec<bool> = targets
            .iter()
            .map(|&i| shown_tumour(&app.shapes[i]).is_some_and(|t| t.on))
            .collect();
        ons.iter().any(|&b| b) && !ons.iter().all(|&b| b)
    };
    let k = app.tumour_k();
    for i in targets {
        let Some(sh) = app.shapes.get_mut(i) else {
            continue;
        };
        let Some(tm) = shown_tumour(sh).cloned() else {
            continue;
        };
        if mixed && !tm.on {
            continue;
        }
        unify_tumours(sh);
        let tm = sh.tumour.get_or_insert(tm);
        match &cleaned {
            Some(g) => {
                tm.graphs.insert(key.to_string(), g.clone());
            }
            None => {
                tm.graphs.remove(key);
            }
        }
        tm.k = k;
    }
    app.shapes_changed();
}

/// Build the number rows for a tumour.
fn number_rows(tm: &Tumour, ppq: f64) -> Vec<NumRow> {
    let n =
        |key: &'static str, label: &str, unit: &str, value: f64, lo: f64, hi: f64, speed: f64| {
            NumRow {
                key,
                label: label.to_string(),
                unit: unit.to_string(),
                value,
                lo,
                hi,
                speed,
            }
        };
    vec![
        n(
            "size",
            &rust_i18n::t!("tumour_window.size"),
            &rust_i18n::t!("unit.keys"),
            tm.size,
            0.0,
            1000.0,
            0.1,
        ),
        n(
            "length",
            &rust_i18n::t!("tumour_window.length"),
            &rust_i18n::t!("unit.ticks"),
            tm.length * ppq,
            0.0,
            10_000_000.0,
            1.0,
        ),
        n(
            "dist",
            &rust_i18n::t!("tumour_window.distance"),
            &rust_i18n::t!("unit.ticks"),
            tm.dist * ppq,
            1.0,
            10_000_000.0,
            1.0,
        ),
        n(
            "rot",
            &rust_i18n::t!("tumour_window.rotation"),
            &rust_i18n::t!("unit.degrees"),
            tm.rot,
            -180.0,
            180.0,
            1.0,
        ),
        n(
            "slant",
            &rust_i18n::t!("tumour_window.slant"),
            "%",
            tm.slant * 100.0,
            -100.0,
            100.0,
            1.0,
        ),
        n(
            "ease",
            &rust_i18n::t!("tumour_window.lead_in"),
            &rust_i18n::t!("unit.ticks"),
            tm.ease * ppq,
            0.0,
            10_000_000.0,
            1.0,
        ),
    ]
}

fn tip(key: &str) -> String {
    match key {
        "size" => rust_i18n::t!("tumour_window.how_far_the_bumps_stick_out"),
        "length" => rust_i18n::t!("tumour_window.how_long_each_bump_is_along"),
        "dist" => rust_i18n::t!("tumour_window.from_the_start_of_one_bump"),
        "ease" => rust_i18n::t!("tumour_window.smooth_start_and_end_over_this"),
        "rot" => rust_i18n::t!("tumour_window.tilts_every_bump_its_two_feet"),
        "slant" => rust_i18n::t!("tumour_window.square_bumps_only_slants_the_square"),
        "side" => rust_i18n::t!("tumour_window.which_side_of_the_line_the"),
        "wrap" => rust_i18n::t!("tumour_window.only_matters_where_the_line_curves"),
        "range" => rust_i18n::t!("tumour_window.only_this_part_of_the_line"),
        "fit" => rust_i18n::t!("tumour_window.fit_the_bumps_evenly_the_distance"),
        _ => "".into(),
    }
    .to_string()
}

/// Open (or lift) the tumour window.
pub fn open_tumour_window(app: &mut App) {
    app.tumour_window_open = true;
}

/// The tumour window and its graph window.
pub fn tumour_window_ui(app: &mut App, ctx: &egui::Context) {
    if app.tumour_window_open {
        let mut open = true;
        egui::Window::new(rust_i18n::t!("tumour_window.tumours"))
            .collapsible(false)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
                open &= tumour_body_ui(app, ui);
            });
        if !open {
            app.tumour_window_open = false;
            app.graph_window = None;
        }
    }
    graph_window_ui(app, ctx);
}

/// The tumour window's contents; false = the window was closed.
fn tumour_body_ui(app: &mut App, ui: &mut egui::Ui) -> bool {
    let mut open = true;
    let targets = app.tumour_targets();
    let tm = shown_tumours(app).unwrap_or(Tumour {
        on: false,
        ..Tumour::default()
    });
    let mut any_change: Option<(TumourChange, bool)> = None;
    let mut reroll = false;
    let mut open_graph: Option<(&'static str, String, String)> = None;
    let ppq = app.ppq.max(1) as f64;
    let mixed = {
        let ons: Vec<bool> = targets
            .iter()
            .map(|&i| shown_tumour(&app.shapes[i]).is_some_and(|t| t.on))
            .collect();
        ons.iter().any(|&b| b) && !ons.iter().all(|&b| b)
    };

    // what is selected
    let what = if targets.is_empty() {
        rust_i18n::t!("tumour_window.select_a_line_polyline_freehand_stroke").to_string()
    } else if targets.len() == 1 {
        let i = targets[0];
        rust_i18n::t!(
            "tumour_window.shape_2",
            i = (i + 1).to_string(),
            shape_label = app.shape_label(&app.shapes[i])
        )
        .to_string()
    } else {
        let on = targets
            .iter()
            .filter(|&&i| shown_tumour(&app.shapes[i]).is_some_and(|t| t.on))
            .count();
        if on == 0 || on == targets.len() {
            rust_i18n::t!(
                "tumour_window.shapes_they_all_change_together",
                n = targets.len().to_string()
            )
            .to_string()
        } else {
            rust_i18n::t!(
                "tumour_window.shapes_with_tumours_the_settings_change",
                n = targets.len().to_string(),
                on = on.to_string()
            )
            .to_string()
        }
    };
    ui.label(egui::RichText::new(what).weak());
    ui.add_space(2.0);

    ui.horizontal(|ui| {
        let mut on = tm.on;
        let cb = ui.add_enabled(
            !targets.is_empty(),
            egui::Checkbox::new(&mut on, rust_i18n::t!("tumour_window.tumours")),
        );
        if cb.changed() {
            let value = if mixed { true } else { on };
            any_change = Some((TumourChange::On(value), true));
        }
        ui.add_enabled_ui(tm.on && !targets.is_empty(), |ui| {
            ui.label(rust_i18n::t!("tumour_window.shape"));
            let mut shape = tm.shape;
            egui::ComboBox::from_id_salt("tumour_win_shape")
                .selected_text(tumour_shape_name(shape))
                .width(90.0)
                .show_ui(ui, |ui| {
                    for s in [
                        TumourShape::Triangle,
                        TumourShape::Square,
                        TumourShape::Circle,
                        TumourShape::Parabola,
                    ] {
                        if ui
                            .selectable_label(shape == s, tumour_shape_name(s))
                            .clicked()
                        {
                            shape = s;
                        }
                    }
                });
            if shape != tm.shape {
                any_change = Some((TumourChange::Shape(shape), true));
            }
        });
    });

    let enabled = tm.on && !targets.is_empty();
    let rows = number_rows(&tm, ppq);
    let square = tm.shape == TumourShape::Square;
    for row in &rows {
        ui.horizontal(|ui| {
            ui.label(&row.label).on_hover_text(tip(row.key));
            let mut v = row.value;
            let active = enabled && (row.key != "slant" || square);
            let resp = ui.add_enabled(
                active,
                egui::DragValue::new(&mut v)
                    .speed(row.speed)
                    .range(row.lo..=row.hi)
                    .max_decimals(3),
            );
            if resp.changed() {
                any_change = Some((row_change(row.key, v, ppq), resp.drag_started()));
            }
            if GRAPH_KEYS.contains(&row.key) {
                let has = tm.graphs.contains_key(row.key);
                let label = if has {
                    rust_i18n::t!("tumour_window.graph", unit = row.unit.clone()).to_string()
                } else {
                    row.unit.clone()
                };
                let color = if has {
                    Color32::from_rgb(0x0a, 0x50, 0xe0)
                } else {
                    ui.visuals().weak_text_color()
                };
                let btn = ui
                    .add_enabled(
                        active,
                        egui::Button::new("…").min_size(Vec2::new(20.0, 0.0)),
                    )
                    .on_hover_text(rust_i18n::t!(
                        "tumour_window.a_graph_this_number_changes_along"
                    ));
                if btn.clicked() {
                    open_graph = Some((row.key, row.label.clone(), row.unit.clone()));
                }
                ui.colored_label(color, label).on_hover_text(tip(row.key));
            } else {
                ui.weak(&row.unit);
            }
        });
    }

    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(rust_i18n::t!("tumour_window.side"));
            let mut side = tm.side;
            egui::ComboBox::from_id_salt("tumour_win_side")
                .selected_text(tumour_side_name(side))
                .width(105.0)
                .show_ui(ui, |ui| {
                    for s in [
                        TumourSide::Alt,
                        TumourSide::Left,
                        TumourSide::Right,
                        TumourSide::Random,
                    ] {
                        if ui
                            .selectable_label(side == s, tumour_side_name(s))
                            .clicked()
                        {
                            side = s;
                        }
                    }
                });
            if side != tm.side {
                any_change = Some((TumourChange::Side(side), true));
            }
            let mut wrap = tm.wrap;
            egui::ComboBox::from_id_salt("tumour_win_wrap")
                .selected_text(tumour_wrap_name(wrap))
                .width(140.0)
                .show_ui(ui, |ui| {
                    for w in [TumourWrap::Simple, TumourWrap::Wrap] {
                        if ui
                            .selectable_label(wrap == w, tumour_wrap_name(w))
                            .clicked()
                        {
                            wrap = w;
                        }
                    }
                });
            if wrap != tm.wrap {
                any_change = Some((TumourChange::Wrap(wrap), true));
            }
        });

        ui.horizontal(|ui| {
            ui.label(rust_i18n::t!("tumour_window.range"))
                .on_hover_text(tip("range"));
            let mut start = (tm.start * 100.0 * 1000.0).round() / 1000.0;
            let r = ui.add(
                egui::DragValue::new(&mut start)
                    .speed(1.0)
                    .range(0.0..=100.0)
                    .max_decimals(3),
            );
            if r.changed() {
                any_change = Some((TumourChange::Start(start / 100.0), r.drag_started()));
            }
            ui.label(rust_i18n::t!("tumour_window.percent_to"));
            let mut end = (tm.end * 100.0 * 1000.0).round() / 1000.0;
            let r = ui.add(
                egui::DragValue::new(&mut end)
                    .speed(1.0)
                    .range(0.0..=100.0)
                    .max_decimals(3),
            );
            if r.changed() {
                any_change = Some((TumourChange::End(end / 100.0), r.drag_started()));
            }
            ui.label(rust_i18n::t!("unit.percent"));
            let mut fit = tm.fit;
            let cb = ui.checkbox(&mut fit, rust_i18n::t!("tumour_window.fit"));
            if cb.changed() {
                any_change = Some((TumourChange::Fit(fit), true));
            }
            cb.on_hover_text(tip("fit"));
            let can_reroll = tm.side == TumourSide::Random;
            if ui
                .add_enabled(
                    can_reroll,
                    egui::Button::new(rust_i18n::t!("tumour_window.new_random")),
                )
                .on_hover_text(rust_i18n::t!("tumour_window.random_sides_pick_them_again"))
                .clicked()
            {
                reroll = true;
            }
        });
    });

    let own = targets.iter().any(|&i| !app.shapes[i].tumours.is_empty());

    let info = if targets.is_empty() {
        rust_i18n::t!("tumour_window.select_a_line_polyline_freehand_stroke").to_string()
    } else if own {
        rust_i18n::t!("tumour_window.the_joined_shapes_kept_their_own").to_string()
    } else if enabled {
        rust_i18n::t!("tumour_window.bumps_along_the_line_the_line").to_string()
    } else {
        rust_i18n::t!("tumour_window.tick_tumours_to_put_bumps_along").to_string()
    };
    let color = if own {
        Color32::from_rgb(0xc0, 0x39, 0x2b)
    } else {
        ui.visuals().weak_text_color()
    };
    ui.colored_label(color, egui::RichText::new(info).size(10.0).color(color));

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(rust_i18n::t!("tumour_window.close")).clicked() {
            open = false;
        }
    });

    if let Some((change, fresh)) = any_change {
        set_value(app, change, fresh);
    }
    if reroll {
        let seed = random_seed();
        set_value(app, TumourChange::Seed(seed), true);
    }
    if let Some((key, label, unit)) = open_graph {
        if app.graph_window.as_ref().is_some_and(|g| g.key != key) {
            app.graph_window = None;
        }
        if app.graph_window.is_none() {
            app.graph_window = Some(GraphWindow::new(app, key, label, unit));
        }
    }
    open
}

// ---------------------------------------------------------------------------
// the graph window
// ---------------------------------------------------------------------------

/// The graph views (shown heights, %) and their labels.
const VIEWS: [(i32, i32); 7] = [
    (0, 100),
    (0, 200),
    (0, 400),
    (-100, 100),
    (-200, 200),
    (-400, 400),
    (-1000, 1000),
];

const FLAT: [[f64; 2]; 2] = [[0.0, 1.0], [1.0, 1.0]];

fn view_text(v: (i32, i32)) -> String {
    rust_i18n::t!("graph_window.to", v = v.0.to_string(), v2 = v.1.to_string()).to_string()
}

/// The presets (upstream graph_window.PRESETS).
fn presets() -> Vec<(String, Vec<Pt>)> {
    vec![
        (
            rust_i18n::t!("graph_window.flat_off").to_string(),
            vec![[0.0, 1.0], [1.0, 1.0]],
        ),
        (
            rust_i18n::t!("graph_window.rise").to_string(),
            vec![[0.0, 0.0], [1.0, 1.0]],
        ),
        (
            rust_i18n::t!("graph_window.fall").to_string(),
            vec![[0.0, 1.0], [1.0, 0.0]],
        ),
        (
            rust_i18n::t!("graph_window.hill").to_string(),
            vec![[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]],
        ),
        (
            rust_i18n::t!("graph_window.valley").to_string(),
            vec![[0.0, 1.0], [0.5, 0.0], [1.0, 1.0]],
        ),
    ]
}

/// One open graph window (upstream `GraphWindow`).
pub struct GraphWindow {
    pub key: &'static str,
    label: String,
    unit: String,
    pts: Vec<Pt>,
    drag: Option<usize>,
    view: usize,
    formula: String,
    formula_error: Option<String>,
    /// The selection the session started with (Cancel goes back to that).
    targets: Vec<usize>,
    session_shapes: String,
    session_graphs: Vec<Option<Vec<Pt>>>,
    step: Option<usize>,
    exact: bool,
    open: bool,
}

impl GraphWindow {
    /// Open a graph window for one setting (the current graph of the shown line).
    pub fn new(app: &mut App, key: &'static str, label: String, unit: String) -> Self {
        let mut w = Self {
            key,
            label,
            unit,
            pts: FLAT.to_vec(),
            drag: None,
            view: 1,
            formula: String::new(),
            formula_error: None,
            targets: Vec::new(),
            session_shapes: String::new(),
            session_graphs: Vec::new(),
            step: None,
            exact: true,
            open: true,
        };
        w.begin(app);
        w.pts = w.current(app);
        if !w.fits(w.view_range()) {
            w.fit_view();
        }
        w
    }

    /// The graph as it is when the window opened / these shapes were selected.
    fn begin(&mut self, app: &App) {
        self.targets = app.tumour_targets();
        self.session_shapes = app.snapshot();
        self.session_graphs = self
            .targets
            .iter()
            .map(|&i| shown_tumour(&app.shapes[i]).and_then(|tm| tm.graphs.get(self.key).cloned()))
            .collect();
        self.step = None;
        self.exact = true;
    }

    /// The graph the window shows now (the shown tumour of the selection).
    fn current(&self, app: &App) -> Vec<Pt> {
        shown_tumours(app)
            .and_then(|tm| tm.graphs.get(self.key).cloned())
            .unwrap_or_else(|| FLAT.to_vec())
    }

    /// Stopped at the first selected line, like the tumour window.
    fn shown_tm(&self, app: &App) -> Tumour {
        shown_tumours(app).unwrap_or_default()
    }

    /// One undo step for everything done to the graph until OK (or other shapes are selected).
    fn push_undo(&mut self, app: &mut App) {
        if self.step.is_none() || app.undo_stack.len() != self.step.unwrap_or(0) {
            if self.step.is_some() {
                // something else changed in between: Cancel can only put the graph back
                self.exact = false;
            }
            app.push_undo();
            self.step = Some(app.undo_stack.len());
        }
        app.edit_key = None;
    }

    /// Put the graph on the selected lines.
    fn store(&self, app: &mut App) {
        set_graph(app, self.key, &self.pts);
    }

    fn view_range(&self) -> (i32, i32) {
        VIEWS.get(self.view).copied().unwrap_or(VIEWS[1])
    }

    fn fits(&self, v: (i32, i32)) -> bool {
        self.pts
            .iter()
            .all(|p| v.0 as f64 / 100.0 - 1e-9 <= p[1] && p[1] <= v.1 as f64 / 100.0 + 1e-9)
    }

    fn fit_view(&mut self) {
        let order = [1usize, 0, 2, 3, 4, 5, 6];
        self.view = order
            .iter()
            .copied()
            .find(|&i| self.fits(VIEWS[i]))
            .unwrap_or(6);
    }

    fn set_points(&mut self, app: &mut App, pts: Vec<Pt>) {
        self.push_undo(app);
        self.pts = pts;
        if !self.fits(self.view_range()) {
            self.fit_view();
        }
        self.store(app);
    }

    fn apply_formula(&mut self, app: &mut App) {
        let text = self.formula.clone();
        let f = match spiderweb_io::mathexpr::formula(&text) {
            Ok(f) => f,
            Err(e) => {
                self.formula_error = Some(match e {
                    spiderweb_io::mathexpr::MathError::Message(m) => m,
                    other => other.to_string(),
                });
                return;
            }
        };
        let mut pts: Vec<Pt> = Vec::with_capacity(41);
        for i in 0..=40 {
            let x = i as f64 / 40.0;
            match f.eval(x) {
                Ok(v) if v.is_finite() => {
                    let v2 = v / 100.0;
                    if v2.is_finite() {
                        pts.push([x, v2.clamp(-GRAPH_LIMIT, GRAPH_LIMIT)]);
                        continue;
                    }
                    self.formula_error =
                        Some(rust_i18n::t!("graph_window.it_doesn_t_give_a_number").to_string());
                    return;
                }
                Ok(_) | Err(_) => {
                    self.formula_error =
                        Some(rust_i18n::t!("graph_window.it_doesn_t_give_a_number").to_string());
                    return;
                }
            }
        }
        self.formula_error = None;
        // (points on a straight stretch aren't needed)
        let mut keep: Vec<Pt> = vec![pts[0]];
        for w in pts.windows(3) {
            let (a, b, c) = (w[0], w[1], w[2]);
            if ((b[1] - a[1]) - (c[1] - b[1])).abs() > 1e-9 {
                keep.push(b);
            }
        }
        keep.push(pts[pts.len() - 1]);
        self.set_points(app, keep);
    }

    /// Keep the graph.
    fn ok(&mut self, app: &mut App) {
        let _ = app;
        self.open = false;
    }

    /// Put the graph back as it was when the window opened (or when these shapes were selected).
    fn cancel(&mut self, app: &mut App) {
        if let Some(step) = self.step {
            if self.exact && app.undo_stack.len() == step {
                // nothing else changed: exactly as it was
                app.undo_stack.pop();
                let value: serde_json::Value =
                    serde_json::from_str(&self.session_shapes).unwrap_or_default();
                let mut shapes = Vec::new();
                if let Some(arr) = value.as_array() {
                    for v in arr {
                        if let Ok(Some(sh)) = spiderweb_io::compat::shape_from_json(v) {
                            shapes.push(sh);
                        }
                    }
                }
                app.shapes = shapes;
                app.edit_key = None;
                app.shapes_changed();
            } else {
                // other changes since then: just this graph goes back
                app.push_undo();
                for (&i, g) in self.targets.iter().zip(self.session_graphs.iter()) {
                    let Some(sh) = app.shapes.get_mut(i) else {
                        continue;
                    };
                    let Some(tm) = sh.tumour.as_mut() else {
                        continue;
                    };
                    match g {
                        Some(g) => {
                            tm.graphs.insert(self.key.to_string(), g.clone());
                        }
                        None => {
                            tm.graphs.remove(self.key);
                        }
                    }
                }
                app.shapes_changed();
            }
        }
        self.open = false;
    }

    fn u2x(&self, rect: Rect, u: f64) -> f32 {
        rect.min.x + self.ml() + u as f32 * (rect.width() - self.ml() - self.mr())
    }

    fn f2y(&self, rect: Rect, f: f64) -> f32 {
        let (lo, hi) = self.view_range();
        rect.min.y
            + self.mt()
            + ((hi as f64 / 100.0 - f) / ((hi - lo) as f64 / 100.0)) as f32
                * (rect.height() - self.mt() - self.mb())
    }

    fn x2u(&self, rect: Rect, x: f32) -> f64 {
        (((x - rect.min.x - self.ml()) / (rect.width() - self.ml() - self.mr())) as f64)
            .clamp(0.0, 1.0)
    }

    fn y2f(&self, rect: Rect, y: f32) -> f64 {
        let (lo, hi) = self.view_range();
        let f = hi as f64 / 100.0
            - ((y - rect.min.y - self.mt()) / (rect.height() - self.mt() - self.mb())) as f64
                * ((hi - lo) as f64 / 100.0);
        f.clamp(lo as f64 / 100.0, hi as f64 / 100.0)
    }

    fn ml(&self) -> f32 {
        46.0
    }
    fn mr(&self) -> f32 {
        12.0
    }
    fn mt(&self) -> f32 {
        10.0
    }
    fn mb(&self) -> f32 {
        22.0
    }

    /// A new point at the mouse (snapped unless Shift), in u order (upstream `press`).
    fn add_at(&mut self, app: &mut App, rect: Rect, p: Pos2, shift: bool) -> Option<usize> {
        const U_SNAP: f64 = 1.0 / 40.0;
        const F_SNAP: f64 = 0.05;
        let mut u = self.x2u(rect, p.x);
        if !shift {
            u = (u / U_SNAP).round() * U_SNAP;
        }
        if u <= 1e-9 || u >= 1.0 - 1e-9 {
            return None;
        }
        let mut f = self.y2f(rect, p.y);
        if !shift {
            f = (f / F_SNAP).round() * F_SNAP;
        }
        let i = self
            .pts
            .iter()
            .position(|q| q[0] > u)
            .unwrap_or(self.pts.len());
        self.push_undo(app);
        self.pts.insert(i, [u, f]);
        self.store(app);
        Some(i)
    }

    fn point_at(&self, rect: Rect, p: Pos2) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, q) in self.pts.iter().enumerate() {
            let d = (self.u2x(rect, q[0]) - p.x)
                .abs()
                .max((self.f2y(rect, q[1]) - p.y).abs());
            if d <= 8.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// What f (a multiplier) makes of the box's number, as the box shows it.
    fn value_text(&self, app: &App, f: f64) -> String {
        let tm = self.shown_tm(app);
        let factor = if TICKS.contains(&self.key) {
            app.ppq.max(1) as f64
        } else if self.key == "slant" {
            100.0
        } else {
            1.0
        };
        let key_value = match self.key {
            "size" => tm.size,
            "length" => tm.length,
            "dist" => tm.dist,
            "rot" => tm.rot,
            "slant" => tm.slant,
            _ => 0.0,
        };
        let v = key_value * f * factor;
        rust_i18n::t!(
            "graph_window.text",
            f = spiderweb_io::mathexpr::fmt(round1(f * 100.0)),
            v = spiderweb_io::mathexpr::fmt(round2(v)),
            unit = self.unit.clone()
        )
        .to_string()
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// The graph window (upstream `graph_window_ui`: one at a time).
fn graph_window_ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = app.graph_window.take() else {
        return;
    };
    // other shapes selected: what was done to the last ones is kept, like OK
    if w.targets != app.tumour_targets() {
        w.begin(app);
    }
    if w.drag.is_none() {
        let now = w.current(app);
        if now != w.pts {
            w.pts = now;
            if !w.fits(w.view_range()) {
                w.fit_view();
            }
        }
    }

    let mut close = false;
    let mut cancel = false;
    egui::Window::new(rust_i18n::t!("graph_window.graph", label = w.label.clone()))
        .collapsible(false)
        .resizable(false)
        .default_width(470.0)
        .show(ctx, |ui| {
            let info = if app.tumour_targets().is_empty() {
                rust_i18n::t!(
                    "graph_window.select_a_line_with_tumours_to",
                    name = w.label.to_lowercase()
                )
                .to_string()
            } else {
                let split = w
                    .value_text(app, 1.0)
                    .split("= ")
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                rust_i18n::t!(
                    "graph_window.along_the_line_100_the_box",
                    label = w.label.clone(),
                    split = split
                )
                .to_string()
            };
            ui.label(info);
            let size = Vec2::new(440.0, 220.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            w.canvas_ui(app, ui, rect, &resp);
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (name, pts) in presets() {
                    if ui.button(name).clicked() {
                        w.set_points(app, pts);
                    }
                }
                ui.label(rust_i18n::t!("graph_window.show"));
                let mut view = w.view;
                egui::ComboBox::from_id_salt("graph_view")
                    .selected_text(view_text(w.view_range()))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for (i, v) in VIEWS.iter().enumerate() {
                            if ui.selectable_label(view == i, view_text(*v)).clicked() {
                                view = i;
                            }
                        }
                    });
                if view != w.view {
                    w.view = view;
                }
            });
            ui.horizontal(|ui| {
                ui.label(rust_i18n::t!("graph_window.formula"));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut w.formula)
                        .desired_width(300.0)
                        .hint_text("100*x   50+50*sin(x*2*pi)"),
                );
                let apply = ui.button(rust_i18n::t!("graph_window.apply")).clicked()
                    || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if apply {
                    w.apply_formula(app);
                }
            });
            match &w.formula_error {
                Some(e) => {
                    ui.colored_label(
                        Color32::from_rgb(0xd0, 0x00, 0x00),
                        rust_i18n::t!("graph_window.can_t_use_it", msg = e.clone()).to_string(),
                    );
                }
                None => {
                    ui.weak(rust_i18n::t!("graph_window.x_0_at_the_line_s"));
                }
            }
            ui.weak(rust_i18n::t!("graph_window.drag_a_point_to_move_it"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(rust_i18n::t!("graph_window.cancel")).clicked() {
                    cancel = true;
                }
                if ui.button(rust_i18n::t!("graph_window.ok")).clicked() {
                    close = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });

    if cancel {
        w.cancel(app);
        return;
    }
    if close {
        w.ok(app);
        return;
    }
    if w.open {
        app.graph_window = Some(w);
    }
}

impl GraphWindow {
    /// Draw the graph and handle the mouse (drag to move, click to add, right-click to remove).
    fn canvas_ui(&mut self, app: &mut App, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response) {
        let painter = ui.painter_at(rect);
        let (lo, hi) = self.view_range();
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        let x0 = self.u2x(rect, 0.0);
        let x1 = self.u2x(rect, 1.0);
        let y_top = self.f2y(rect, hi as f64 / 100.0);
        let y_bot = self.f2y(rect, lo as f64 / 100.0);
        // outside the tumour range: no bumps there, so greyed out
        let tm = self.shown_tm(app);
        let (r0, r1) = if tm.start <= tm.end {
            (tm.start, tm.end)
        } else {
            (tm.end, tm.start)
        };
        for (a, b) in [(0.0, r0), (r1, 1.0)] {
            if b - a > 1e-9 {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(self.u2x(rect, a), y_top),
                        Pos2::new(self.u2x(rect, b), y_bot),
                    ),
                    0.0,
                    Color32::from_rgb(0xe4, 0xe4, 0xe4),
                );
            }
        }
        let step = ((hi - lo) / 8).max(25);
        let mut v = lo;
        while v <= hi {
            let y = self.f2y(rect, v as f64 / 100.0);
            let color = if v == 0 || v == 100 {
                Color32::from_rgb(0x9f, 0xb2, 0xcf)
            } else {
                Color32::from_rgb(0xd3, 0xdf, 0xf0)
            };
            painter.line_segment(
                [Pos2::new(x0, y), Pos2::new(x1, y)],
                Stroke::new(1.0, color),
            );
            painter.text(
                Pos2::new(x0 - 4.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{v} %"),
                egui::FontId::proportional(9.0),
                Color32::from_rgb(0x33, 0x33, 0x33),
            );
            v += step;
        }
        for j in 1..4 {
            let x = self.u2x(rect, j as f64 / 4.0);
            painter.line_segment(
                [Pos2::new(x, y_top), Pos2::new(x, y_bot)],
                Stroke::new(1.0, Color32::from_rgb(0xd3, 0xdf, 0xf0)),
            );
        }
        painter.rect_stroke(
            Rect::from_min_max(Pos2::new(x0, y_top), Pos2::new(x1, y_bot)),
            0.0,
            Stroke::new(1.0, Color32::from_rgb(0x80, 0x80, 0x80)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            Pos2::new(x0, y_bot + 4.0),
            egui::Align2::LEFT_TOP,
            rust_i18n::t!("graph_window.line_start"),
            egui::FontId::proportional(9.0),
            Color32::from_rgb(0x33, 0x33, 0x33),
        );
        painter.text(
            Pos2::new(x1, y_bot + 4.0),
            egui::Align2::RIGHT_TOP,
            rust_i18n::t!("graph_window.line_end"),
            egui::FontId::proportional(9.0),
            Color32::from_rgb(0x33, 0x33, 0x33),
        );
        if r0 > 1e-9 || r1 < 1.0 - 1e-9 {
            painter.text(
                Pos2::new((self.u2x(rect, r0) + self.u2x(rect, r1)) / 2.0, y_bot + 4.0),
                egui::Align2::CENTER_TOP,
                rust_i18n::t!("graph_window.tumour_range"),
                egui::FontId::proportional(9.0),
                Color32::from_rgb(0x77, 0x77, 0x77),
            );
        }
        let red = Color32::from_rgb(0xd0, 0x00, 0x00);
        let line: Vec<Pos2> = self
            .pts
            .iter()
            .map(|q| Pos2::new(self.u2x(rect, q[0]), self.f2y(rect, q[1])))
            .collect();
        if line.len() >= 2 {
            painter.add(egui::Shape::line(line.clone(), Stroke::new(1.5, red)));
        }
        for (i, p) in line.iter().enumerate() {
            if i == 0 || i == line.len() - 1 {
                painter.rect_stroke(
                    Rect::from_center_size(*p, Vec2::splat(8.0)),
                    0.0,
                    Stroke::new(1.5, red),
                    egui::StrokeKind::Inside,
                );
            } else {
                painter.circle_stroke(*p, 4.0, Stroke::new(1.5, red));
            }
            painter.circle_filled(*p, 2.5, Color32::WHITE);
        }

        // mouse
        let has_targets = !app.tumour_targets().is_empty();
        let shift = ui.input(|i| i.modifiers.shift);
        let pointer = resp.interact_pointer_pos();
        let hover = pointer.filter(|p| rect.contains(*p) && !shift);
        if has_targets && resp.drag_started() {
            if let Some(p) = pointer {
                if let Some(i) = self.point_at(rect, p) {
                    self.drag = Some(i);
                } else if let Some(i) = self.add_at(app, rect, p, shift) {
                    self.drag = Some(i);
                }
            }
        } else if has_targets && resp.clicked() {
            // a plain click adds a point here (upstream ButtonPress without one under it)
            if let Some(p) = pointer
                && self.point_at(rect, p).is_none()
            {
                self.add_at(app, rect, p, shift);
            }
        } else if has_targets && resp.dragged() {
            if let (Some(i), Some(p)) = (self.drag, pointer) {
                let (mut u, mut f) = (self.x2u(rect, p.x), self.y2f(rect, p.y));
                if !shift {
                    u = (u / (1.0 / 40.0)).round() * (1.0 / 40.0);
                    f = (f / 0.05).round() * 0.05;
                }
                if i == 0 || i == self.pts.len() - 1 {
                    u = self.pts[i][0]; // the ends stay at the line's start / end
                } else {
                    u = u.clamp(self.pts[i - 1][0], self.pts[i + 1][0]);
                }
                if [u, f] != self.pts[i] {
                    self.push_undo(app);
                    self.pts[i] = [u, f];
                    self.store(app);
                }
            }
        } else if resp.drag_stopped() {
            self.drag = None;
            self.store(app);
        }
        if has_targets
            && resp.secondary_clicked()
            && let Some(p) = pointer
            && let Some(i) = self.point_at(rect, p)
            && i != 0
            && i != self.pts.len() - 1
        {
            self.push_undo(app);
            self.pts.remove(i);
            self.store(app);
        }

        // what's under the mouse (or the point being dragged)
        let at = self
            .drag
            .and_then(|i| self.pts.get(i))
            .copied()
            .or_else(|| {
                hover.map(|p| {
                    let u = self.x2u(rect, p.x);
                    let mut f = 1.0;
                    for w in self.pts.windows(2) {
                        let (a, b) = (w[0], w[1]);
                        if u <= b[0] {
                            let d = b[0] - a[0];
                            f = if d <= 0.0 {
                                b[1]
                            } else {
                                a[1] + (b[1] - a[1]) * ((u - a[0]) / d).clamp(0.0, 1.0)
                            };
                            break;
                        }
                        f = b[1];
                    }
                    [u, f]
                })
            });
        if let Some(at) = at {
            painter.text(
                Pos2::new(x1 - 4.0, y_top + 4.0),
                egui::Align2::RIGHT_TOP,
                rust_i18n::t!(
                    "graph_window.at_of_the_line",
                    at = spiderweb_io::mathexpr::fmt(round1(at[0] * 100.0)),
                    value_text = self.value_text(app, at[1])
                ),
                egui::FontId::proportional(10.0),
                Color32::from_rgb(0x0a, 0x50, 0xe0),
            );
        }
    }
}
