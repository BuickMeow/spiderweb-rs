//! Main window state: toolbar, side panel, shape editing, undo, autosave, playback and file
//! operations. The piano roll's painting and interaction are in roll.rs, the side panel in
//! panels.rs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use spiderweb_core::custom::notes_shape;
use spiderweb_core::engine::{self, Mode, Split};
use spiderweb_core::joined;
use spiderweb_core::shape::{Kind, Shape, TextSettings};
use spiderweb_domino::DominoStart;
use spiderweb_io::compat::{shape_from_json, shape_to_json};
use spiderweb_io::project::{
    self as proj, AutosaveOutcome, ChannelMode, ChannelSplit, CustomDefaults, FunnelDefaults,
    Project,
};

use crate::drawer::Drawer;
use crate::help::{HelpState, Tips};
use crate::playback::{DEFAULT_DEVICE, Player};
use crate::roll::{Drag, RightDrag, View};
use crate::roll_menu::MenuState;
use crate::roll_text::Typing;
use crate::roll_velocity::VelocityState;
use crate::snap_picker::CustomSnapWindow;
use crate::text_dialog::FontDialog;

/// Program version (VERSION of upstream files/about.py; used by the help window title and the about text).
pub const VERSION: &str = "1.2.0";

/// Notes per shape and the track column (for pasted notes).
type NotesAndTracks = (Vec<[i64; 4]>, Option<Vec<i64>>);

/// Tools (upstream TOOLS + SHAPE_TOOLS).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Line,
    Poly,
    Free,
    Curve,
    Arc,
    Custom,
    Funnel,
    Text,
    Square,
    Circle,
    Triangle,
}

impl Tool {
    pub const ALL: [Tool; 12] = [
        Tool::Select,
        Tool::Line,
        Tool::Poly,
        Tool::Free,
        Tool::Curve,
        Tool::Arc,
        Tool::Custom,
        Tool::Funnel,
        Tool::Text,
        Tool::Square,
        Tool::Circle,
        Tool::Triangle,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Line => "Line",
            Tool::Poly => "Polyline",
            Tool::Free => "Freehand",
            Tool::Curve => "Curve",
            Tool::Arc => "Arc",
            Tool::Custom => "Custom shape",
            Tool::Funnel => "Funnel",
            Tool::Text => "Text",
            Tool::Square => "Square",
            Tool::Circle => "Circle",
            Tool::Triangle => "Triangle",
        }
    }

    /// Tool name shown in the UI (buttons / tips; data like shape names still uses [`Tool::label`]).
    pub fn ui_label(self) -> String {
        match self {
            Tool::Select => rust_i18n::t!("tool.select"),
            Tool::Line => rust_i18n::t!("tool.line"),
            Tool::Poly => rust_i18n::t!("tool.poly"),
            Tool::Free => rust_i18n::t!("tool.free"),
            Tool::Curve => rust_i18n::t!("tool.curve"),
            Tool::Arc => rust_i18n::t!("tool.arc"),
            Tool::Custom => rust_i18n::t!("tool.custom"),
            Tool::Funnel => rust_i18n::t!("tool.funnel"),
            Tool::Text => rust_i18n::t!("tool.text"),
            Tool::Square => rust_i18n::t!("tool.square"),
            Tool::Circle => rust_i18n::t!("tool.circle"),
            Tool::Triangle => rust_i18n::t!("tool.triangle"),
        }
        .to_string()
    }

    pub fn hotkey(self) -> &'static str {
        match self {
            Tool::Select => "v",
            Tool::Line => "l",
            Tool::Poly => "p",
            Tool::Free => "f",
            Tool::Curve => "c",
            Tool::Arc => "a",
            Tool::Custom => "s",
            Tool::Funnel => "n",
            Tool::Text => "x",
            Tool::Square => "q",
            Tool::Circle => "o",
            Tool::Triangle => "t",
        }
    }

    pub fn is_box(self) -> bool {
        matches!(self, Tool::Square | Tool::Circle | Tool::Triangle)
    }
}

/// A highlighted part of the selected funnel (an element of upstream app.parts):
/// a line, or a curve (start number, wall end).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartId {
    Line(usize),
    Curve(usize, usize),
}

/// Project text fields (upstream pvar).
#[derive(Clone, Debug)]
pub struct Pvar {
    pub ppq: String,
    pub bpm: String,
    pub beats: String,
    pub output: String,
}

/// Pending state for confirming "this step will generate a lot of notes".
pub struct PendingBig {
    pub shapes: Vec<Shape>,
    pub total: i64,
}

pub struct App {
    pub shapes: Vec<Shape>,
    pub sels: BTreeSet<usize>,
    pub sel: Option<usize>,
    pub draft: Option<Shape>,
    /// Which tool the square / circle / triangle being drawn uses (upstream draft["draw"])
    pub draft_draw: Option<Tool>,
    /// Picked stroke number of the selected custom shape (upstream app.stroke)
    pub stroke: Option<usize>,
    pub drag: Option<Drag>,
    pub follow: Option<Drag>,
    pub arc_bend: bool,
    /// Already handled on right press (curve point deletion / handle retraction): don't deselect on release.
    pub right_done: bool,
    /// Right-drag preview state (upstream self._scrub): tick is None until the drag exceeds 4px.
    pub right_drag: Option<RightDrag>,
    /// Sounding (channel, pitch) -> latest note start during right-drag preview (upstream self._scrub_held).
    pub scrub_held: BTreeMap<(u8, i64), i64>,
    /// Open context menu (upstream roll_menu.show_menu).
    pub shape_menu: Option<MenuState>,
    pub tool: Tool,
    pub draw_tool: Tool,
    pub live: bool,
    /// The snap text (`spiderweb_io::snap` spelling).
    pub snap: String,
    /// The "Customised snap" window; None = closed.
    pub snap_window: Option<CustomSnapWindow>,
    pub show_lines: bool,
    pub show_notes: bool,
    pub show_velocity: bool,
    pub channel_mode: ChannelMode,
    pub channel_split: ChannelSplit,
    /// Where copying / pasting to Domino starts (1.2.0's Project -> Domino start).
    pub domino_start: DominoStart,
    /// The project's key range: 128 or 256 (upstream app.keys; 0 .. keys - 1)
    pub keys: i64,
    pub ppq: i64,
    pub beats: i64,
    pub bpm: f64,
    pub pvar: Pvar,
    pub defaults: Shape,
    pub custom_defaults: CustomDefaults,
    pub funnel_defaults: FunnelDefaults,
    /// Highlighted lines and curves of the selected funnel (upstream app.parts).
    pub parts: BTreeSet<PartId>,
    /// The clicked one among the highlighted parts (linked curves change color).
    pub part_main: Option<PartId>,
    /// Gate text boxes of the funnel panel (ticks).
    pub funnel_text: [String; 2],
    /// Formula input box of the funnel panel.
    pub funnel_formula: String,
    pub free_smooth: i64,
    pub text_defaults: TextSettings,
    pub playhead: f64,
    pub rendered: Vec<[i64; 6]>,
    pub note_counts: Vec<usize>,
    /// Multi channel: how many channels each shape's notes are spread over (upstream `chans`).
    pub shape_channels: Vec<usize>,
    pub slot_count: usize,
    /// Named undo steps: `(shapes as JSON, name)` (upstream App.undo_stack).
    pub undo_stack: Vec<crate::history::Step>,
    /// The steps undone, still there for redo (upstream App.redo_stack).
    pub redo_stack: Vec<crate::history::Step>,
    pub edit_key: Option<String>,
    /// The History panel (off on a fresh start). Session-only, like the velocity pane: the autosave
    /// window state isn't restored here.
    pub show_history: bool,
    /// The History list is undocked (its own window) (upstream app.history_undocked).
    pub history_undocked: bool,
    pub view: View,
    /// Velocity panel state (upstream app.vel_tool + the VelocityPane state)
    pub vel: VelocityState,
    pub status: String,
    pub position: Option<String>,
    pub player: Player,
    pub midi_device: String,
    pub autosave_path: PathBuf,
    pub dirty: bool,
    pub last_autosave: Instant,
    pub pending_big: Option<PendingBig>,
    /// Delete all confirmation (upstream messagebox.askyesno): number of shapes awaiting confirmation.
    pub pending_delete_all: Option<usize>,
    /// "Turn into live shape" warning when something is lost (convert_ui.rs).
    pub pending_turn_live: Option<crate::convert_ui::PendingTurnLive>,
    pub clipboard: Vec<Shape>,
    pub loaded: bool,
    pub panel_sel: Option<usize>,
    pub vel_text: [String; 2],
    /// SPIDERWEB_PERF=1: show the frame-time HUD and print load / recompute timings
    pub perf: bool,
    /// Exponential average frame time (ms)
    pub frame_ms: f32,
    /// Frame-time samples for the periodic p50/p95 log (SPIDERWEB_PERF)
    pub perf_samples: Vec<f32>,
    /// Text being typed (upstream roll.typing)
    pub typing: Option<Typing>,
    /// Contents of the text clipboard, written to the system clipboard once a ctx is available
    pub text_clipboard: Option<String>,
    /// Font picker dialog
    pub font_dialog: Option<FontDialog>,
    /// Installed font family names (read once on first use)
    pub font_families: Vec<String>,
    /// Gate text box of the custom shape panel (ticks)
    pub custom_gate_text: String,
    /// The tumour window (tumour_window.rs) is open.
    pub tumour_window_open: bool,
    /// The open graph window (only one at a time).
    pub graph_window: Option<crate::tumour_window::GraphWindow>,
    /// First-use tips (help.rs)
    pub tips: Tips,
    /// Help window state (help.rs)
    pub help: HelpState,
    /// Shape drawer window (upstream app.drawer)
    pub drawer: Option<Drawer>,
    /// Shape library directory (shapes/ next to the executable; upstream drawer.LIBRARY)
    pub library_dir: PathBuf,
    /// Custom wgpu instanced note renderer (None without a wgpu render state, falling back to painter)
    pub note_gpu: Option<crate::note_gpu::NoteGpu>,
    /// Note / selection revision: note_gpu rebuilds the instance buffer only when it changes
    pub notes_revision: u64,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let perf = std::env::var("SPIDERWEB_PERF").is_ok();
        let autosave_path = base.join("autosave.json");
        let output = base.join("spiderweb.mid").to_string_lossy().into_owned();
        crate::errors::install(base.clone());
        let library_dir = base.join("shapes");
        // With the wgpu backend, eframe gives us a render state; build the note GPU renderer
        let note_gpu = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| crate::note_gpu::NoteGpu::new(std::sync::Arc::new(rs.clone())));
        let mut app = Self {
            shapes: Vec::new(),
            sels: BTreeSet::new(),
            sel: None,
            draft: None,
            draft_draw: None,
            stroke: None,
            drag: None,
            follow: None,
            arc_bend: false,
            right_done: false,
            right_drag: None,
            scrub_held: BTreeMap::new(),
            shape_menu: None,
            tool: Tool::Select,
            draw_tool: Tool::Line,
            live: false,
            snap: "1/16".to_string(),
            snap_window: None,
            show_lines: true,
            show_notes: true,
            show_velocity: false,
            channel_mode: ChannelMode::Single,
            channel_split: ChannelSplit::Key,
            domino_start: DominoStart::Note,
            keys: 128,
            ppq: 960,
            beats: 4,
            bpm: 120.0,
            pvar: Pvar {
                ppq: "960".to_string(),
                bpm: "120".to_string(),
                beats: "4".to_string(),
                output,
            },
            defaults: Shape::default(),
            custom_defaults: CustomDefaults::default(),
            funnel_defaults: FunnelDefaults::default(),
            parts: BTreeSet::new(),
            part_main: None,
            funnel_text: ["60".into(), "60".into()],
            funnel_formula: String::new(),
            free_smooth: 0,
            text_defaults: TextSettings::default(),
            playhead: 0.0,
            rendered: Vec::new(),
            note_counts: Vec::new(),
            shape_channels: Vec::new(),
            slot_count: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            edit_key: None,
            show_history: false,
            history_undocked: false,
            view: View::default(),
            vel: VelocityState::default(),
            status: String::new(),
            position: None,
            player: Player::default(),
            midi_device: DEFAULT_DEVICE.to_string(),
            autosave_path,
            dirty: false,
            last_autosave: Instant::now(),
            pending_big: None,
            pending_delete_all: None,
            pending_turn_live: None,
            clipboard: Vec::new(),
            loaded: false,
            panel_sel: None,
            vel_text: ["127".into(), "127".into()],
            perf,
            frame_ms: 0.0,
            perf_samples: Vec::new(),
            typing: None,
            text_clipboard: None,
            font_dialog: None,
            font_families: Vec::new(),
            custom_gate_text: "60".into(),
            tumour_window_open: false,
            graph_window: None,
            tips: Tips::new(&base),
            help: HelpState::default(),
            drawer: None,
            library_dir,
            note_gpu,
            notes_revision: 0,
        };
        app.tips.welcome_at = Some(Instant::now());
        cc.egui_ctx
            .set_pixels_per_point(cc.egui_ctx.pixels_per_point());
        let t_load = Instant::now();
        app.load_autosave();
        app.loaded = true;
        app.sync_funnel_text();
        app.shapes_changed();
        if perf {
            eprintln!(
                "[perf] load+render {:.1} ms, {} shapes, {} notes",
                t_load.elapsed().as_secs_f64() * 1000.0,
                app.shapes.len(),
                app.rendered.len()
            );
        }
        app
    }

    // ------------------------------------------------------------ project

    pub fn scale(&self) -> f32 {
        1.0
    }

    pub fn read_project(&self) -> Result<(i64, f64, i64), String> {
        proj::read_project(&self.pvar.ppq, &self.pvar.bpm, &self.pvar.beats)
            .map_err(|e| format!("{e:?}"))
    }

    /// Re-evaluates after PPQ changes (keeps the old values when the pvar text is invalid).
    pub fn on_project_change(&mut self) {
        if let Ok((ppq, bpm, beats)) = self.read_project() {
            self.ppq = ppq;
            self.bpm = bpm;
            self.beats = beats;
            self.shapes_changed();
        }
    }

    /// Keys changed in the panel (128 / 256): the roll follows, notes are recomputed for the new range (upstream on_project_change).
    pub fn on_keys_change(&mut self) {
        self.view.keys = self.keys;
        self.view.clamp();
        self.shapes_changed();
    }

    pub fn load_autosave(&mut self) {
        let stamp = "autosave-broken";
        match proj::load_autosave(&self.autosave_path, stamp) {
            AutosaveOutcome::Missing => {}
            AutosaveOutcome::Opened(p)
            | AutosaveOutcome::Damaged {
                backup: Some(p), ..
            } => {
                self.apply_project(*p);
            }
            AutosaveOutcome::Damaged {
                renamed_to,
                backup: None,
            } => {
                self.status = match renamed_to {
                    Some(p) => rust_i18n::t!(
                        "status.autosave_broken_kept",
                        path = p.display().to_string()
                    )
                    .to_string(),
                    None => rust_i18n::t!("status.autosave_broken").to_string(),
                };
            }
        }
    }

    pub fn apply_project(&mut self, p: Project) {
        self.scrub_end();
        self.right_drag = None;
        self.shape_menu = None;
        self.shapes = p.shapes;
        self.ppq = p.ppq.parse().unwrap_or(960);
        self.beats = p.beats.parse().unwrap_or(4);
        self.bpm = p.bpm.parse().unwrap_or(120.0);
        self.pvar = Pvar {
            ppq: p.ppq,
            bpm: p.bpm,
            beats: p.beats,
            output: if p.output.is_empty() {
                self.pvar.output.clone()
            } else {
                p.output
            },
        };
        self.channel_mode = p.channel_mode;
        self.channel_split = p.channel_split;
        self.domino_start = p.domino_start;
        self.keys = p.keys;
        self.view.keys = self.keys;
        self.snap = p.snap;
        self.defaults.vel0 = p.defaults.vel0;
        self.defaults.vel1 = p.defaults.vel1;
        self.defaults.end_dot = p.defaults.end_dot;
        self.custom_defaults = p.custom_defaults;
        self.funnel_defaults = p.funnel_defaults;
        self.text_defaults = p.text_defaults;
        self.free_smooth = p.free_smooth;
        self.playhead = p.playhead;
        self.view.apply_state(p.view);
        self.sels.clear();
        self.sel = None;
        self.typing = None;
        self.stroke = None;
        self.draft = None;
        self.draft_draw = None;
        self.parts.clear();
        self.part_main = None;
    }

    pub fn to_project(&self) -> Project {
        Project {
            ppq: self.pvar.ppq.clone(),
            bpm: self.pvar.bpm.clone(),
            beats: self.pvar.beats.clone(),
            output: self.pvar.output.clone(),
            channel_mode: self.channel_mode,
            channel_split: self.channel_split,
            keys: self.keys,
            domino_start: self.domino_start,
            snap: self.snap.clone(),
            defaults: spiderweb_io::compat::ShapeDefaults {
                vel0: self.defaults.vel0,
                vel1: self.defaults.vel1,
                end_dot: self.defaults.end_dot,
            },
            custom_defaults: self.custom_defaults.clone(),
            funnel_defaults: self.funnel_defaults,
            text_defaults: self.text_defaults.clone(),
            free_smooth: self.free_smooth,
            shapes: self.shapes.clone(),
            view: self.view.state(),
            playhead: self.playhead,
            ..Project::default()
        }
    }

    pub fn schedule_autosave(&mut self) {
        self.dirty = true;
    }

    pub fn autosave_now(&mut self) {
        let p = self.to_project();
        let _ = p.write(&self.autosave_path, None);
        self.dirty = false;
        self.last_autosave = Instant::now();
    }

    /// Saves the project to a chosen path.
    pub fn save_project_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Spiderweb project", &["json"])
            .save_file()
        {
            let p = self.to_project();
            if let Err(e) = p.write(&path, None) {
                self.status = rust_i18n::t!("status.couldnt_save", e = e.to_string()).to_string();
            }
        }
    }

    /// Opens a project file.
    pub fn open_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Spiderweb project", &["json"])
            .pick_file()
        {
            match Project::load(&path) {
                Ok(p) => {
                    self.apply_project(p);
                    self.shapes_changed();
                }
                Err(e) => {
                    self.status = rust_i18n::t!("status.couldnt_open_project", e = format!("{e:?}"))
                        .to_string()
                }
            }
        }
    }

    pub fn generate_midi(&mut self) {
        if let Err(e) = self.read_project() {
            self.status = e;
            return;
        }
        let path = PathBuf::from(self.pvar.output.clone());
        match spiderweb_io::midi::write_midi(
            &path,
            self.ppq as u16,
            self.bpm,
            self.beats as u8,
            &self.rendered,
        ) {
            Ok(()) => {
                let channels = if self.channel_mode == ChannelMode::Auto {
                    self.slot_count
                } else {
                    1
                };
                let mut note = if self.ppq >= i64::from(spiderweb_io::midi::PPQ_WARN) {
                    rust_i18n::t!(
                        "status.ppq_warning",
                        ppq = self.ppq.to_string(),
                        warn = spiderweb_io::midi::PPQ_WARN.to_string()
                    )
                    .to_string()
                } else {
                    String::new()
                };
                if self.rendered.iter().any(|n| n[2] > 127) {
                    // 256 keys: keys >127 are written to the file as-is, which many MIDI programs cannot read (project.it_has_keys_above_127_256)
                    note += rust_i18n::t!("status.keys_above_127").as_ref();
                }
                self.status = rust_i18n::t!(
                    "status.saved_midi",
                    notes = fmt_int(self.rendered.len() as i64),
                    channels = channels.to_string(),
                    path = path.display().to_string(),
                    note = note
                )
                .to_string();
            }
            Err(e) => {
                self.status = rust_i18n::t!("status.couldnt_save", e = format!("{e:?}")).to_string()
            }
        }
    }

    // ------------------------------------------------------------ notes

    pub fn notes_tracks(&self, sh: &Shape) -> NotesAndTracks {
        engine::shape_notes_tracks(sh, self.ppq as f64, self.keys)
    }

    pub fn notes_of(&self, sh: &Shape) -> Vec<[i64; 4]> {
        self.notes_tracks(sh).0
    }

    pub fn shape_label(&self, sh: &Shape) -> String {
        if sh.notes.is_some() {
            return rust_i18n::t!("shape.pasted_notes").to_string();
        }
        if sh.text.is_some() {
            return rust_i18n::t!(
                "shape.text",
                name = if sh.name.is_empty() { "?" } else { &sh.name }
            )
            .to_string();
        }
        if joined::is_joined(sh) {
            let pieces = sh.gaps.len() + 1;
            return if pieces > 1 {
                rust_i18n::t!("shape.joined_pieces", pieces = pieces.to_string()).to_string()
            } else {
                rust_i18n::t!("shape.joined").to_string()
            };
        }
        if sh.kind == Kind::Custom {
            return rust_i18n::t!(
                "shape.custom",
                name = if sh.name.is_empty() { "?" } else { &sh.name }
            )
            .to_string();
        }
        engine::KINDS
            .iter()
            .find(|(k, _)| *k == sh.kind)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| sh.kind.as_str().to_string())
    }

    /// Recomputes all notes and refreshes the UI (upstream shapes_changed).
    pub fn shapes_changed(&mut self) {
        let t0 = Instant::now();
        let got: Vec<NotesAndTracks> = self.shapes.iter().map(|sh| self.notes_tracks(sh)).collect();
        let lists: Vec<Vec<[i64; 4]>> = got.iter().map(|(n, _)| n.clone()).collect();
        let tracks: Vec<Option<Vec<i64>>> = got.iter().map(|(_, t)| t.clone()).collect();
        // Fill / Spam "Outline": the outline and the inside must get channels of their own.
        let apart: Vec<bool> = self
            .shapes
            .iter()
            .map(spiderweb_core::custom::outline_apart)
            .collect();
        let (rendered, count) = engine::render(
            &lists,
            match self.channel_mode {
                ChannelMode::Raw => Mode::Raw,
                ChannelMode::Single => Mode::Single,
                ChannelMode::Auto => Mode::Auto,
            },
            match self.channel_split {
                ChannelSplit::Key => Split::Key,
                ChannelSplit::Time => Split::Time,
            },
            Some(&tracks),
            Some(&apart),
        );
        self.rendered = rendered;
        self.slot_count = count;
        let mut counts = vec![0usize; self.shapes.len()];
        for n in &self.rendered {
            let owner = n[5] as usize;
            if owner < counts.len() {
                counts[owner] += 1;
            }
        }
        self.note_counts = counts;
        // Multi channel: how many channels each shape spreads over (upstream shapes_changed).
        let mut chans = vec![1usize; self.shapes.len()];
        if self.channel_mode == ChannelMode::Auto && !self.rendered.is_empty() {
            let mut seen: Vec<std::collections::BTreeSet<i64>> =
                vec![std::collections::BTreeSet::new(); self.shapes.len()];
            for n in &self.rendered {
                let owner = n[5] as usize;
                if owner < seen.len() {
                    seen[owner].insert(n[4]);
                }
            }
            for (i, s) in seen.iter().enumerate() {
                chans[i] = s.len();
            }
        }
        self.shape_channels = chans;
        // Notes changed: note_gpu rebuilds the instance buffer next frame
        self.notes_revision = self.notes_revision.wrapping_add(1);
        self.schedule_autosave();
        if self.perf {
            eprintln!(
                "[perf] shapes_changed {:.1} ms, {} notes",
                t0.elapsed().as_secs_f64() * 1000.0,
                self.rendered.len()
            );
        }
    }

    pub fn note_count(&self, sh: &Shape) -> i64 {
        match sh.kind {
            Kind::Custom => spiderweb_core::custom::custom_note_count(sh, self.ppq as f64),
            Kind::Funnel => Some(spiderweb_core::funnel::funnel_note_count(
                sh,
                self.ppq as f64,
            )),
            _ => None,
        }
        .unwrap_or_else(|| self.notes_of(sh).len() as i64)
    }

    // ------------------------------------------------------------ selection / editing

    pub fn selected(&self) -> Option<&Shape> {
        self.sel.and_then(|i| self.shapes.get(i))
    }

    pub fn selected_mut(&mut self) -> Option<&mut Shape> {
        let i = self.sel?;
        self.shapes.get_mut(i)
    }

    pub fn select(&mut self, i: Option<usize>, toggle: bool) {
        if !toggle {
            self.select_many(i.into_iter().collect(), i);
        } else if let Some(i) = i {
            if self.sels.contains(&i) {
                let rest: BTreeSet<usize> = self.sels.iter().copied().filter(|&j| j != i).collect();
                let primary = if self.sel.map(|s| rest.contains(&s)).unwrap_or(false) {
                    self.sel
                } else {
                    rest.iter().next_back().copied()
                };
                self.select_many(rest, primary);
            } else {
                let mut sels = self.sels.clone();
                sels.insert(i);
                self.select_many(sels, Some(i));
            }
        }
        self.edit_key = None;
    }

    pub fn select_many(&mut self, indices: BTreeSet<usize>, primary: Option<usize>) {
        self.sels = indices;
        self.sel = primary;
        // Another shape was selected (or something was selected while typing): this typing session ends (upstream select_many)
        if let Some(ty) = &self.typing {
            let keep = match ty.i {
                Some(i) => self.sels.contains(&i),
                None => self.sels.is_empty(),
            };
            if !keep {
                crate::roll_text::end_typing(self);
            }
        }
        self.stroke = None;
        self.edit_key = None;
        self.parts.clear();
        self.part_main = None;
        // Selection changed: selected notes need a different layer color, so note_gpu rebuilds the instance buffer next frame
        self.notes_revision = self.notes_revision.wrapping_add(1);
    }

    /// Picks stroke k of the selected custom shape (None = unpick) (upstream set_stroke).
    pub fn set_stroke(&mut self, k: Option<usize>) {
        self.stroke = k;
    }

    pub fn select_all(&mut self) {
        if !self.shapes.is_empty() {
            let primary = self.sel.or(Some(self.shapes.len() - 1));
            self.select_many((0..self.shapes.len()).collect(), primary);
        }
    }

    pub fn add_shape(&mut self, sh: Shape) {
        let name = rust_i18n::t!("app.draw", shape_label = self.shape_label(&sh)).to_string();
        self.push_undo(&name);
        self.shapes.push(sh);
        self.select(Some(self.shapes.len() - 1), false);
        self.shapes_changed();
    }

    pub fn delete_selected(&mut self) {
        if self.sels.is_empty() {
            return;
        }
        crate::roll_text::end_typing(self);
        self.push_undo(&rust_i18n::t!("panel.shapes.delete"));
        for i in self.sels.iter().rev() {
            self.shapes.remove(*i);
        }
        self.select(None, false);
        self.shapes_changed();
    }

    /// Delete key: with a stroke of the selected custom shape picked, delete the stroke; otherwise delete the shape (upstream on_key's delete).
    pub fn delete_pressed(&mut self) {
        if self.sels.len() == 1
            && let (Some(i), Some(k)) = (self.sel, self.stroke)
        {
            crate::roll_live::delete_stroke(self, i, k);
            return;
        }
        self.delete_selected();
    }

    /// Delete all: pops the upstream confirmation first (undoable with Ctrl+Z).
    pub fn delete_all(&mut self) {
        if self.shapes.is_empty() {
            return;
        }
        self.pending_delete_all = Some(self.shapes.len());
    }

    /// Actually clears everything after confirmation (the part of upstream delete_all after the confirmation).
    fn do_delete_all(&mut self) {
        self.cancel_draft();
        self.push_undo(&rust_i18n::t!("panel.shapes.delete_all"));
        self.shapes.clear();
        self.select(None, false);
        self.shapes_changed();
    }

    pub fn duplicate(&mut self) {
        if self.sels.is_empty() {
            return;
        }
        let shift = self.snap_beats().unwrap_or(1.0);
        let shapes: Vec<Shape> = self.sels.iter().map(|&i| self.shapes[i].clone()).collect();
        self.add_copies(&shapes, shift, &rust_i18n::t!("panel.shapes.duplicate"));
    }

    pub fn add_copies(&mut self, shapes: &[Shape], shift: f64, name: &str) {
        self.push_undo(name);
        let first = self.shapes.len();
        for sh in shapes {
            let mut new = sh.clone();
            for p in &mut new.pts {
                p[0] += shift;
            }
            self.shapes.push(new);
        }
        let sels: BTreeSet<usize> = (first..self.shapes.len()).collect();
        self.select_many(sels, Some(self.shapes.len() - 1));
        self.shapes_changed();
    }

    pub fn flip(&mut self, sideways: bool) {
        if self.sels.is_empty() {
            return;
        }
        let idx: Vec<usize> = self.sels.iter().copied().collect();
        let mut vals: Vec<f64> = Vec::new();
        for &i in &idx {
            for stroke in engine::shape_strokes(&self.shapes[i]) {
                for p in stroke {
                    vals.push(p[if sideways { 0 } else { 1 }]);
                }
            }
        }
        let (Some(lo), Some(hi)) = (
            vals.iter().copied().reduce(f64::min),
            vals.iter().copied().reduce(f64::max),
        ) else {
            return;
        };
        let mid2 = lo + hi;
        self.push_undo(&if sideways {
            rust_i18n::t!("menu.flip_sideways")
        } else {
            rust_i18n::t!("menu.flip_upside_down")
        });
        for &i in &idx {
            let sh = &mut self.shapes[i];
            for p in &mut sh.pts {
                if sideways {
                    p[0] = mid2 - p[0];
                } else {
                    p[1] = mid2 - p[1];
                }
            }
            for tm in joined::all_tumours_mut(sh) {
                tm.mirror = !tm.mirror; // mirrored: the bumps swap sides too
            }
            if sideways {
                if !sh.vel_env.is_empty() {
                    let mut env = sh.vel_env.clone();
                    env.reverse();
                    for p in &mut env {
                        p[0] = 1.0 - p[0];
                    }
                    sh.vel_env = env;
                }
                std::mem::swap(&mut sh.vel0, &mut sh.vel1);
            }
        }
        self.shapes_changed();
    }

    pub fn rotate(&mut self, clockwise: bool) {
        if self.sels.is_empty() || !self.view.ready {
            return;
        }
        let idx: Vec<usize> = self.sels.iter().copied().collect();
        let mut bs = Vec::new();
        let mut ps = Vec::new();
        for &i in &idx {
            for stroke in engine::shape_strokes(&self.shapes[i]) {
                for p in stroke {
                    bs.push(p[0]);
                    ps.push(p[1]);
                }
            }
        }
        let (Some(blo), Some(bhi), Some(plo), Some(phi)) = (
            bs.iter().copied().reduce(f64::min),
            bs.iter().copied().reduce(f64::max),
            ps.iter().copied().reduce(f64::min),
            ps.iter().copied().reduce(f64::max),
        ) else {
            return;
        };
        let cb = (blo + bhi) / 2.0;
        let cp = (plo + phi) / 2.0;
        let r = self.view.sy / self.view.sx;
        let sign = if clockwise { 1.0 } else { -1.0 };
        self.push_undo(&rust_i18n::t!("app.turn_90"));
        for &i in &idx {
            let sh = &mut self.shapes[i];
            for p in &mut sh.pts {
                let (b, q) = (p[0], p[1]);
                p[0] = cb + sign * (q - cp) * r;
                p[1] = cp - sign * (b - cb) / r;
            }
            if sh.kind == Kind::Arc || (sh.kind == Kind::Free && sh.smooth > 0) {
                sh.k = r * r / sh.k;
            }
            if let Some(tx) = sh.text.as_mut() {
                tx.k = r * r / tx.k;
            }
            for tm in joined::all_tumours_mut(sh) {
                tm.size *= tm.k / r;
                tm.length *= r / tm.k;
                tm.dist *= r / tm.k;
                tm.ease *= r / tm.k;
                tm.k = r * r / tm.k;
            }
        }
        self.shapes_changed();
    }

    pub fn copy_selected(&mut self) {
        if self.sels.is_empty() {
            return;
        }
        self.clipboard = self.sels.iter().map(|&i| self.shapes[i].clone()).collect();
        self.status =
            rust_i18n::t!("status.copied_shapes", n = self.clipboard.len().to_string()).to_string();
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let start = self
            .clipboard
            .iter()
            .filter_map(|sh| {
                engine::shape_strokes(sh)
                    .iter()
                    .flat_map(|s| s.iter().map(|p| p[0]))
                    .reduce(f64::min)
            })
            .reduce(f64::min)
            .unwrap_or(0.0);
        let mut at = self.playhead;
        if let Some(sb) = self.snap_beats() {
            at = (at / sb).round() * sb;
        }
        let shapes = self.clipboard.clone();
        self.add_copies(&shapes, at - start, &rust_i18n::t!("app.paste"));
    }

    // ------------------------------------------------------------ Domino clipboard

    pub fn copy_to_domino(&mut self) {
        let mut notes: Vec<[i64; 6]> = if self.sels.is_empty() {
            self.rendered.clone()
        } else {
            self.rendered
                .iter()
                .filter(|n| self.sels.contains(&(n[5] as usize)))
                .copied()
                .collect()
        };
        // Upstream projects have 128 keys: Domino only has 128, so >127 is not copied
        // (project.copy_to_domino). With 256-key projects (the Domino 256k version uses the same
        // clipboard format) everything is copied.
        let max_key = if self.keys >= 256 { 255 } else { 127 };
        let high = notes.iter().filter(|n| n[2] > max_key).count();
        notes.retain(|n| n[2] <= max_key);
        if notes.is_empty() {
            self.status = if self.sels.is_empty() || high > 0 {
                rust_i18n::t!("status.no_notes_to_copy").to_string()
            } else {
                rust_i18n::t!("status.selected_shapes_no_notes").to_string()
            };
            return;
        }
        let ppq = self.ppq;
        match spiderweb_domino::clip_data(
            &notes,
            self.ppq as u16,
            self.beats * self.ppq,
            self.domino_start,
        ) {
            Ok(raw) => {
                if spiderweb_domino::put_on_clipboard(&raw) {
                    let what = if self.sels.is_empty() {
                        rust_i18n::t!("status.all_notes", n = fmt_int(notes.len() as i64))
                            .to_string()
                    } else {
                        rust_i18n::t!(
                            "status.notes_count",
                            n = fmt_int(notes.len() as i64),
                            s = if notes.len() == 1 { "" } else { "s" }
                        )
                        .to_string()
                    };
                    let tracks = notes.iter().map(|n| n[4]).collect::<BTreeSet<i64>>().len();
                    let place = if tracks == 1 {
                        rust_i18n::t!("status.a_track").to_string()
                    } else {
                        rust_i18n::t!("status.first_of_tracks", n = tracks.to_string()).to_string()
                    };
                    let how = if self.domino_start == DominoStart::Note {
                        rust_i18n::t!("status.how_cursor").to_string()
                    } else {
                        rust_i18n::t!("status.how_bar_line").to_string()
                    };
                    let mut copied = rust_i18n::t!(
                        "status.copied_domino",
                        what = what,
                        ppq = ppq.to_string(),
                        place = place,
                        how = how
                    )
                    .to_string();
                    if high > 0 {
                        copied.push_str(&rust_i18n::t!(
                            "status.notes_above_127_left_out",
                            high = fmt_int(high as i64)
                        ));
                    }
                    self.status = copied;
                    self.tips.show_waiting("domino");
                } else {
                    self.status = rust_i18n::t!("status.clipboard_error").to_string();
                }
            }
            Err(_) => self.status = rust_i18n::t!("status.clipboard_error").to_string(),
        }
    }

    pub fn paste_from_domino(&mut self) {
        match spiderweb_domino::get_from_clipboard() {
            spiderweb_domino::ClipboardGet::Busy => {
                self.status = rust_i18n::t!("status.clipboard_error").to_string();
            }
            spiderweb_domino::ClipboardGet::NoData => {
                self.status = rust_i18n::t!("status.no_domino_notes").to_string();
            }
            spiderweb_domino::ClipboardGet::Data(raw) => {
                let max_key = if self.keys >= 256 { 255 } else { 127 };
                match spiderweb_domino::read_notes_max_key(&raw, max_key) {
                    Ok((mut rows, their_ppq)) => {
                        // "note": the first note is at tick 0, so the clipboard's empty lead is dropped
                        if self.domino_start == DominoStart::Note
                            && let Some(first) = rows.iter().map(|r| r[0]).min()
                        {
                            for r in &mut rows {
                                r[0] -= first;
                            }
                        }
                        let name = rust_i18n::t!("shape.pasted_notes").to_string();
                        if let Some(sh) = notes_shape(&rows, self.ppq as f64, &name) {
                            // the start lands on the play line, itself snapped to the grid (upstream paste_from_domino)
                            let at = match self.snap_beats() {
                                Some(sb) => (self.playhead / sb).round() * sb,
                                None => self.playhead,
                            };
                            self.cancel_draft();
                            self.add_copies(&[sh], at, &rust_i18n::t!("app.paste"));
                            let n = rows.len();
                            let note = match their_ppq {
                                Some(p) if i64::from(p) != self.ppq => {
                                    rust_i18n::t!("status.domino_ppq", ppq = p.to_string())
                                        .to_string()
                                }
                                _ => String::new(),
                            };
                            self.status = rust_i18n::t!(
                                "status.pasted_domino",
                                n = fmt_int(n as i64),
                                s = if n == 1 { "" } else { "s" },
                                note = note
                            )
                            .to_string();
                            self.tips.show_waiting("domino");
                        } else {
                            self.status = rust_i18n::t!("status.domino_no_notes").to_string();
                        }
                    }
                    Err(e) => {
                        self.status =
                            rust_i18n::t!("status.clipboard_read_error", e = format!("{e:?}"))
                                .to_string()
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ undo

    pub fn snapshot(&self) -> String {
        let arr: Vec<serde_json::Value> = self.shapes.iter().map(shape_to_json).collect();
        serde_json::to_string(&serde_json::Value::Array(arr)).unwrap_or_default()
    }

    /// Remember the shapes for Ctrl+Z; `name` is what the step does (the History panel)
    /// (upstream push_undo).
    pub fn push_undo(&mut self, name: &str) {
        let state = self.snapshot();
        crate::history::push(&mut self.undo_stack, &mut self.redo_stack, state, name);
        self.edit_key = None;
    }

    pub fn undo(&mut self) {
        if self.draft.is_some() {
            self.cancel_draft();
            return;
        }
        self.restore(true);
    }

    pub fn redo(&mut self) {
        self.restore(false);
    }

    fn restore(&mut self, from_undo: bool) {
        crate::roll_text::end_typing(self);
        let current = self.snapshot();
        let (src, dst) = if from_undo {
            (&mut self.undo_stack, &mut self.redo_stack)
        } else {
            (&mut self.redo_stack, &mut self.undo_stack)
        };
        let Some((snap, _)) = crate::history::take(src, dst, current) else {
            return;
        };
        self.apply_snapshot(&snap);
    }

    /// Go back / forward to step `row` (upstream history_jump): undo / redo until it is the current
    /// one. The steps on the way keep their names in the other stack.
    pub fn history_jump(&mut self, row: usize) {
        self.cancel_draft();
        let mut undo = std::mem::take(&mut self.undo_stack);
        let mut redo = std::mem::take(&mut self.redo_stack);
        crate::history::jump(&mut undo, &mut redo, row, self);
        self.undo_stack = undo;
        self.redo_stack = redo;
    }

    /// Show one step's shapes (upstream `_restore` after the stacks moved).
    fn apply_snapshot(&mut self, snap: &str) {
        let value: serde_json::Value =
            serde_json::from_str(snap).unwrap_or(serde_json::Value::Array(Vec::new()));
        let mut shapes = Vec::new();
        if let Some(arr) = value.as_array() {
            for v in arr {
                if let Ok(Some(sh)) = shape_from_json(v) {
                    shapes.push(sh);
                }
            }
        }
        self.shapes = shapes;
        self.sels = self
            .sels
            .iter()
            .copied()
            .filter(|&i| i < self.shapes.len())
            .collect();
        if self.sel.map(|s| !self.sels.contains(&s)).unwrap_or(true) {
            self.sel = self.sels.iter().next_back().copied();
        }
        self.shapes_changed();
    }
}

impl crate::history::HistoryHost for App {
    fn snapshot(&mut self) -> String {
        App::snapshot(self)
    }

    fn apply(&mut self, state: &str) {
        self.apply_snapshot(state);
    }
}

impl App {
    // ------------------------------------------------------------ snapping

    /// The snap step in beats (quarter notes), None when snapping is off (upstream `snap_beats`).
    pub fn snap_beats(&self) -> Option<f64> {
        crate::snap_picker::snap_step(&self.snap, self.beats)
    }

    /// The snap step in ticks (1 with snapping off, upstream `App.snap_ticks`).
    #[allow(dead_code)] // number box stepping yet to be ported
    pub fn snap_ticks(&self) -> i64 {
        spiderweb_io::snap::snap_ticks(&self.snap, self.beats as f64, self.ppq)
    }

    // ------------------------------------------------------------ playback

    pub fn toggle_play(&mut self) {
        if self.player.running() {
            self.stop_play();
        } else {
            self.start_play();
        }
    }

    pub fn start_play(&mut self) {
        self.scrub_end();
        let Ok((ppq, bpm, beats)) = self.read_project() else {
            return;
        };
        if let Err(e) = self.player.open(&self.midi_device) {
            self.status = e;
            return;
        }
        let quarter = beats as f64 / 4.0;
        let last = self.rendered.iter().map(|n| n[1]).max().unwrap_or(0) as f64 / ppq as f64;
        let stop = ((last.max(self.playhead) + quarter) / quarter - 1e-9).ceil() * quarter;
        self.player
            .start(&self.rendered, ppq, bpm, self.playhead, stop);
    }

    pub fn stop_play(&mut self) {
        if self.player.running() {
            self.playhead = self.player.position(self.ppq.max(1));
        }
        self.player.stop();
        self.schedule_autosave();
    }

    pub fn set_playhead(&mut self, beat: f64) {
        self.playhead = beat.max(0.0);
        self.schedule_autosave();
    }

    /// Right-drag preview: sounds the notes under the mouse (tick `t_to`) and those just swept
    /// (upstream app.scrub). false = MIDI could not be opened: the right-drag is abandoned and
    /// the error goes to the status bar.
    pub fn scrub(&mut self, t_from: f64, t_to: f64) -> bool {
        let device = self.midi_device.clone();
        if let Err(e) = self.player.open(&device) {
            self.status = e;
            return false;
        }
        let now = scrub_hits(&self.rendered, t_from, t_to);
        let gone: Vec<(u8, i64)> = self
            .scrub_held
            .keys()
            .filter(|k| !now.contains_key(*k))
            .copied()
            .collect();
        for k in gone {
            self.player.note(k.0, k.1, 0);
            self.scrub_held.remove(&k);
        }
        for (k, (s, v)) in &now {
            if self.scrub_held.get(k) != Some(s) {
                if self.scrub_held.contains_key(k) {
                    self.player.note(k.0, k.1, 0);
                }
                self.player.note(k.0, k.1, *v);
                self.scrub_held.insert(*k, *s);
            }
        }
        let ppq = self.ppq.max(1) as f64;
        self.set_playhead(t_to / ppq);
        true
    }

    /// Right release: all notes off (upstream app.scrub_end).
    pub fn scrub_end(&mut self) {
        for ((ch, p), _) in std::mem::take(&mut self.scrub_held) {
            self.player.note(ch, p, 0);
        }
    }

    // ------------------------------------------------------------ tools

    /// Double right-click: Select <-> the last drawing tool (upstream toggle_select_tool).
    pub fn toggle_select_tool(&mut self) {
        self.tool = if self.tool == Tool::Select {
            self.draw_tool
        } else {
            Tool::Select
        };
        self.cancel_draft();
    }

    pub fn cancel_draft(&mut self) {
        crate::roll_text::end_typing(self);
        self.draft = None;
        self.draft_draw = None;
        self.drag = None;
        self.follow = None;
        self.arc_bend = false;
    }

    /// Commits the draft: Live drawing / boxes become custom shapes (upstream commit_draft -> live_commit).
    pub fn commit_draft(&mut self) {
        if let Some(sh) = self.draft.take()
            && !crate::roll_live::live_commit(self, &sh)
        {
            self.add_shape(sh);
        }
        self.draft_draw = None;
        self.arc_bend = false;
    }

    #[allow(dead_code)] // post-drag recompute yet to be ported
    pub fn catch_up_notes(&mut self) {
        self.shapes_changed();
    }

    /// Confirmation before committing a big shape (upstream confirm_big). true = commit directly.
    pub fn confirm_big_draft(&mut self) -> bool {
        let Some(d) = self.draft.clone() else {
            return true;
        };
        let n = self.note_count(&d);
        if n <= 1_000_000 {
            return true;
        }
        self.draft = None;
        self.draft_draw = None;
        self.pending_big = Some(PendingBig {
            shapes: vec![d],
            total: n,
        });
        false
    }

    // ------------------------------------------------------------ eframe

    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // The drawer has keyboard focus: keys go to the drawer (upstream in_drawer) and the roll's shortcuts step aside
        if self.drawer.as_ref().is_some_and(|d| d.focus) {
            return;
        }
        // The Help window is open: its keys type into the search box (upstream type_to_search; the
        // main window doesn't get keys while it has the keyboard).
        if self.help.open {
            return;
        }
        // Typing in progress: keys go to the text (the typing priority of upstream on_key) and shortcuts step aside
        if self.typing.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        // The context menu is open: Esc / clicks are handled by the menu itself and shortcuts step aside
        if self.shape_menu.is_some() {
            return;
        }
        use egui::{Key, KeyboardShortcut, Modifiers};
        let cmd = Modifiers::COMMAND;
        ctx.input_mut(|i| {
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Z)) {
                self.undo();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Y)) {
                self.redo();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::S)) {
                self.save_project_as();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::A)) {
                self.select_all();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::D)) {
                self.duplicate();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::C)) {
                self.copy_selected();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::V)) {
                self.paste();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::C)) {
                self.copy_to_domino();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::V)) {
                self.paste_from_domino();
            }
            // (the Shift one first: consume_shortcut ignores an extra Shift, so Ctrl+G would eat Ctrl+Shift+G)
            if i.consume_shortcut(&KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::G)) {
                self.split_selected();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::G)) {
                self.join_selected();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::H)) {
                self.flip(true);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::J)) {
                self.flip(false);
            }
            // Ctrl+L: turn the selection into one live shape (roll_menu / join_split)
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::L)) {
                crate::convert_ui::turn_into_live(self);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::ArrowLeft)) {
                self.rotate(false);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::ArrowRight)) {
                self.rotate(true);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Space)) {
                self.toggle_play();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Escape)) {
                if self.help.open {
                    self.help.open = false; // Esc closes the help window first (upstream HelpWindow's Escape)
                } else if self.tips.popup.is_some() {
                    self.tips.got_it(); // Esc = Got it
                } else {
                    self.cancel_draft();
                    self.set_stroke(None);
                    self.select(None, false);
                    self.parts.clear();
                    self.part_main = None;
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Delete)) {
                // Highlighted funnel lines and curves take priority (upstream delete_parts)
                if !self.delete_funnel_parts() {
                    self.delete_pressed();
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::G)) {
                self.live = !self.live;
                if self.live {
                    self.tips.show("live");
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F1)) {
                crate::help::open_help(self, None);
            }
            // Tool hotkeys
            for tool in Tool::ALL {
                let key = match tool.hotkey() {
                    "v" => Key::V,
                    "l" => Key::L,
                    "p" => Key::P,
                    "f" => Key::F,
                    "c" => Key::C,
                    "a" => Key::A,
                    "s" => Key::S,
                    "n" => Key::N,
                    "x" => Key::X,
                    "q" => Key::Q,
                    "o" => Key::O,
                    "t" => Key::T,
                    _ => continue,
                };
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, key)) {
                    if tool != Tool::Select {
                        self.draw_tool = tool;
                    }
                    if self.tool != tool {
                        self.tool = tool;
                        self.cancel_draft();
                        // Pop this tool's tip when switching tools (seen ones don't pop again)
                        self.tips.show(crate::help::tool_topic(tool));
                    }
                }
            }
        });
    }

    pub fn update_playing(&mut self, ctx: &egui::Context) {
        if self.player.running() {
            self.playhead = self.player.position(self.ppq.max(1));
            // Page over when the play line nears the right edge
            let x = self.view.x_of(self.playhead);
            if x > self.view.w - 6.0 * self.scale() {
                self.view.t = self.playhead;
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.dirty && self.last_autosave.elapsed() > Duration::from_secs(2) {
            self.autosave_now();
        }
    }

    pub fn status_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(pos) = &self.position {
            parts.push(pos.clone());
        }
        parts.push(
            rust_i18n::t!(
                "status.shapes_notes",
                shapes = self.shapes.len().to_string(),
                notes = fmt_int(self.rendered.len() as i64)
            )
            .to_string(),
        );
        if self.channel_mode == ChannelMode::Auto && self.slot_count > 0 {
            parts.push(rust_i18n::t!("status.tracks", n = self.slot_count.to_string()).to_string());
        }
        if !self.sels.is_empty() {
            let n: usize = self
                .sels
                .iter()
                .map(|&i| self.note_counts.get(i).copied().unwrap_or(0))
                .sum();
            let shapes = if self.sels.len() > 1 {
                rust_i18n::t!("status.many_shapes", n = self.sels.len().to_string()).to_string()
            } else {
                String::new()
            };
            parts.push(
                rust_i18n::t!(
                    "status.selected",
                    shapes = shapes,
                    notes = fmt_int(n as i64)
                )
                .to_string(),
            );
        }
        if !self.status.is_empty() {
            parts.push(self.status.clone());
        }
        parts.join("     ")
    }
}

/// A whole number with thousands separators, like Python's `{n:,}` in the original status lines
/// ("Saved 1,234 notes ...", "all 10,000 notes").
pub fn fmt_int(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Notes that should sound as a right-drag sweeps ticks `t_from..t_to` (upstream app.scrub's `now`):
/// only the one with the latest start is kept per (channel, pitch), value = (start tick, velocity).
/// Same hit condition as upstream: the start falls inside the swept range, or the note is being
/// swept (`s <= t_to < e`).
pub fn scrub_hits(rendered: &[[i64; 6]], t_from: f64, t_to: f64) -> BTreeMap<(u8, i64), (i64, u8)> {
    let lo = t_from.min(t_to);
    let hi = t_from.max(t_to);
    let mut now: BTreeMap<(u8, i64), (i64, u8)> = BTreeMap::new();
    for n in rendered {
        let s = n[0] as f64;
        let e = n[1] as f64;
        if !((lo <= s && s <= hi) || (s <= t_to && t_to < e)) {
            continue;
        }
        let (_, ch) = spiderweb_io::midi::slot_track_channel(n[4]);
        let vel = n[3].clamp(0, 127) as u8;
        let entry = now.entry((ch, n[2])).or_insert((n[0], vel));
        if n[0] > entry.0 {
            *entry = (n[0], vel);
        }
    }
    now
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Once a panic or other error is written to errors.log, show it once in the status bar (errors.rs)
        if let Some(msg) = crate::errors::take_pending() {
            self.status = msg;
        }
        // Text input takes priority: Event::Text / edit keys go to the text being typed first, shortcuts get the rest
        crate::roll_text::text_keyboard(self, &ctx);
        self.handle_shortcuts(&ctx);
        self.update_playing(&ctx);

        egui::Panel::top("toolbar").show(ui, |ui| {
            self.toolbar_ui(ui);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.status_line());
            });
        });
        egui::Panel::right("side")
            .resizable(true)
            .default_size(330.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.side_panel_ui(ui);
                });
            });
        if self.show_velocity {
            egui::Panel::bottom("velocity")
                .resizable(true)
                .default_size(170.0)
                .show(ui, |ui| {
                    crate::roll_velocity::velocity_ui(self, ui);
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                crate::roll::roll_ui(self, ui);
            });

        crate::text_dialog::font_dialog_ui(self, &ctx);
        // Tumour settings window and graph window (tumour_window.rs)
        crate::tumour_window::tumour_window_ui(self, &ctx);
        crate::snap_picker::custom_snap_window_ui(self, &ctx);
        // Help window and first-use tips (help.rs)
        crate::help::help_ui(self, &ctx);
        crate::help::tips_ui(self, &ctx);
        // the History panel's own window, when it's undocked (history.rs)
        crate::history::history_window_ui(self, &ctx);

        crate::drawer::drawer_ui(self, &ctx);

        if let Some(pending) = self.pending_big.take() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new("Spiderweb")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(
                        rust_i18n::t!("confirm.big", total = fmt_int(pending.total)).to_string(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(rust_i18n::t!("common.yes").to_string()).clicked() {
                            go = true;
                        }
                        if ui.button(rust_i18n::t!("common.no").to_string()).clicked() {
                            cancel = true;
                        }
                    });
                });
            if go {
                for sh in pending.shapes {
                    self.add_shape(sh);
                }
            } else if !cancel {
                self.pending_big = Some(pending);
            }
        }

        crate::convert_ui::dialog_ui(self, &ctx);

        if let Some(count) = self.pending_delete_all.take() {
            let mut delete = false;
            let mut cancel = false;
            egui::Window::new("Spiderweb")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(
                        rust_i18n::t!("confirm.delete_all", n = count.to_string()).to_string(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(rust_i18n::t!("common.yes").to_string()).clicked() {
                            delete = true;
                        }
                        if ui.button(rust_i18n::t!("common.no").to_string()).clicked() {
                            cancel = true;
                        }
                    });
                });
            if delete {
                self.do_delete_all();
            } else if !cancel {
                self.pending_delete_all = Some(count);
            }
        }

        if self.perf {
            let dt = ctx.input(|i| i.stable_dt); // time taken by the last frame
            self.frame_ms = self.frame_ms * 0.9 + dt * 1000.0 * 0.1;
            self.perf_samples.push(dt * 1000.0);
            if self.perf_samples.len() >= 600 {
                let mut v = std::mem::take(&mut self.perf_samples);
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let at = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
                eprintln!(
                    "[perf] frames {}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                    v.len(),
                    at(0.5),
                    at(0.95),
                    v.last().copied().unwrap_or(0.0)
                );
            }
            egui::Area::new(egui::Id::new("perf_hud"))
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        let fps = if self.frame_ms > 0.0 {
                            1000.0 / self.frame_ms
                        } else {
                            0.0
                        };
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.2} ms/frame ({:.0} fps)\n{} shapes · {} notes",
                                self.frame_ms,
                                fps,
                                self.shapes.len(),
                                self.rendered.len()
                            ))
                            .monospace(),
                        );
                    });
                });
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self) {
        self.stop_play();
        self.autosave_now();
        self.player.close();
    }
}

/// A rendered note row (start, end, pitch, velocity, slot, owner).
#[cfg(test)]
fn test_note(s: i64, e: i64, p: i64, v: i64, slot: i64) -> [i64; 6] {
    [s, e, p, v, slot, 0]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python `{n:,}` number formatting in the status lines.
    #[test]
    fn integers_get_thousands_separators() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(7), "7");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(1000), "1,000");
        assert_eq!(fmt_int(1234567), "1,234,567");
        assert_eq!(fmt_int(-1234), "-1,234");
    }

    #[test]
    fn scrub_hits_takes_notes_under_and_swept() {
        let rendered = vec![
            test_note(0, 100, 60, 64, 0),   // ended before the sweep
            test_note(0, 400, 61, 70, 0),   // still sounding (the second condition)
            test_note(200, 300, 62, 80, 0), // start inside the swept range
        ];
        let got = scrub_hits(&rendered, 150.0, 250.0);
        assert_eq!(got.get(&(0, 61)), Some(&(0, 70)));
        assert_eq!(got.get(&(0, 62)), Some(&(200, 80)));
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn scrub_hits_keeps_latest_start_per_key() {
        let rendered = vec![
            test_note(200, 300, 62, 80, 0),
            test_note(220, 260, 62, 90, 0), // later on the same key: a back-to-back repeat sounds the latest
            test_note(100, 400, 62, 50, 9), // slot 9 = channel 10 after the drum channel, a different route
        ];
        let got = scrub_hits(&rendered, 150.0, 250.0);
        assert_eq!(got.get(&(0, 62)), Some(&(220, 90)));
        assert_eq!(got.get(&(10, 62)), Some(&(100, 50)));
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn scrub_hits_sweeps_either_direction() {
        let rendered = vec![test_note(300, 400, 61, 64, 0)];
        let right = scrub_hits(&rendered, 150.0, 350.0);
        let left = scrub_hits(&rendered, 350.0, 150.0);
        assert_eq!(right.get(&(0, 61)), Some(&(300, 64)));
        assert_eq!(left.get(&(0, 61)), Some(&(300, 64)));
        // Notes whose ranges don't overlap make no sound
        let missed = scrub_hits(&rendered, 500.0, 600.0);
        assert!(missed.is_empty());
    }
}
