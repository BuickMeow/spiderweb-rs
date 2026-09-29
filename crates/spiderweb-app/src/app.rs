//! 主窗口状态：工具栏、侧栏、形状编辑、撤销、自动保存、播放与文件操作。
//! 钢琴卷帘的绘制与交互在 roll.rs，侧栏在 panels.rs。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use spiderweb_core::custom::notes_shape;
use spiderweb_core::engine::{self, Mode, Split};
use spiderweb_core::shape::{Kind, Shape, TextSettings};
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
use crate::text_dialog::FontDialog;

/// 程序版本（原版 files/about.py 的 VERSION；帮助窗口标题与 about 文案用）。
pub const VERSION: &str = "1.1.0";

/// 每个形状的音符与（粘贴音符的）track 列。
type NotesAndTracks = (Vec<[i64; 4]>, Option<Vec<i64>>);

/// 工具（原版 TOOLS + SHAPE_TOOLS）。
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

    /// 界面上的工具名（按钮 / 提示；形状名等数据仍用 [`Tool::label`]）。
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

/// 选中的漏斗里高亮的一个 part（原版 app.parts 的元素）：
/// 一条线，或一条曲线（起点号, 墙端）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartId {
    Line(usize),
    Curve(usize, usize),
}

/// 工程文本框（原版 pvar）。
#[derive(Clone, Debug)]
pub struct Pvar {
    pub ppq: String,
    pub bpm: String,
    pub beats: String,
    pub output: String,
}

/// 确认"这一步会生成很多音符"用的待确认状态。
pub struct PendingBig {
    pub shapes: Vec<Shape>,
    pub total: i64,
}

pub struct App {
    pub shapes: Vec<Shape>,
    pub sels: BTreeSet<usize>,
    pub sel: Option<usize>,
    pub draft: Option<Shape>,
    /// 正在画的方 / 圆 / 三角是哪个工具（原版 draft["draw"]）
    pub draft_draw: Option<Tool>,
    /// 选中的自定义形状被拾取的笔画号（原版 app.stroke）
    pub stroke: Option<usize>,
    pub drag: Option<Drag>,
    pub follow: Option<Drag>,
    pub arc_bend: bool,
    /// 右键按下时已处理（曲线删点 / 收手柄）：松开时不再取消选择。
    pub right_done: bool,
    /// 右拖试听的现场（原版 self._scrub）：还没拖够 4px 时 tick 为 None。
    pub right_drag: Option<RightDrag>,
    /// 右拖试听正在响的 (通道, 音高) -> 最新音符起点（原版 self._scrub_held）。
    pub scrub_held: BTreeMap<(u8, i64), i64>,
    /// 打开着的右键菜单（原版 roll_menu.show_menu）。
    pub shape_menu: Option<MenuState>,
    pub tool: Tool,
    pub draw_tool: Tool,
    pub live: bool,
    pub snap: String,
    pub show_lines: bool,
    pub show_notes: bool,
    pub show_velocity: bool,
    pub channel_mode: ChannelMode,
    pub channel_split: ChannelSplit,
    pub ppq: i64,
    pub beats: i64,
    pub bpm: f64,
    pub pvar: Pvar,
    pub defaults: Shape,
    pub custom_defaults: CustomDefaults,
    pub funnel_defaults: FunnelDefaults,
    /// 选中漏斗里高亮的线与曲线（原版 app.parts）。
    pub parts: BTreeSet<PartId>,
    /// 高亮的 part 里点中的那个（联动的曲线换个颜色）。
    pub part_main: Option<PartId>,
    /// 漏斗面板的 gate 文本框（tick）。
    pub funnel_text: [String; 2],
    /// 漏斗面板的公式输入框。
    pub funnel_formula: String,
    pub free_smooth: i64,
    pub text_defaults: TextSettings,
    pub playhead: f64,
    pub rendered: Vec<[i64; 6]>,
    pub note_counts: Vec<usize>,
    pub slot_count: usize,
    pub undo_stack: Vec<String>,
    pub redo_stack: Vec<String>,
    pub edit_key: Option<String>,
    pub view: View,
    /// 力度面板状态（原版 app.vel_tool + VelocityPane 的现场）
    pub vel: VelocityState,
    pub status: String,
    pub position: Option<String>,
    pub player: Player,
    pub midi_device: String,
    pub autosave_path: PathBuf,
    pub dirty: bool,
    pub last_autosave: Instant,
    pub pending_big: Option<PendingBig>,
    /// Delete all 的确认框（原版 messagebox.askyesno）：待确认的形状数。
    pub pending_delete_all: Option<usize>,
    pub clipboard: Vec<Shape>,
    pub loaded: bool,
    pub panel_sel: Option<usize>,
    pub vel_text: [String; 2],
    /// SPIDERWEB_PERF=1：显示帧时间 HUD 并打印加载 / 重算耗时
    pub perf: bool,
    /// 帧时间指数平均（ms）
    pub frame_ms: f32,
    /// 正在输入的文本（原版 roll.typing）
    pub typing: Option<Typing>,
    /// 文本剪切板的内容，等有 ctx 的时候写进系统剪贴板
    pub text_clipboard: Option<String>,
    /// 字体选择窗口
    pub font_dialog: Option<FontDialog>,
    /// 已安装的字体族名（第一次用到时读一次）
    pub font_families: Vec<String>,
    /// 自定义形状面板的 gate 文本框（ticks）
    pub custom_gate_text: String,
    /// 首次使用的 tip（help.rs）
    pub tips: Tips,
    /// 帮助窗口状态（help.rs）
    pub help: HelpState,
    /// 抽屉窗口（原版 app.drawer）
    pub drawer: Option<Drawer>,
    /// 图形库目录（可执行文件旁的 shapes/，原版 drawer.LIBRARY）
    pub library_dir: PathBuf,
    /// 自定义 wgpu 实例化音符渲染器（没有 wgpu render state 时为 None，走 painter 回退）
    pub note_gpu: Option<crate::note_gpu::NoteGpu>,
    /// 音符 / 选择的修订号：变化时 note_gpu 才重建 instance buffer
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
        // eframe 用 wgpu 后端时拿到渲染状态，建立音符 GPU 渲染器
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
            show_lines: true,
            show_notes: true,
            show_velocity: false,
            channel_mode: ChannelMode::Single,
            channel_split: ChannelSplit::Key,
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
            slot_count: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            edit_key: None,
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
            clipboard: Vec::new(),
            loaded: false,
            panel_sel: None,
            vel_text: ["127".into(), "127".into()],
            perf,
            frame_ms: 0.0,
            typing: None,
            text_clipboard: None,
            font_dialog: None,
            font_families: Vec::new(),
            custom_gate_text: "60".into(),
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

    // ------------------------------------------------------------ 工程

    pub fn scale(&self) -> f32 {
        1.0
    }

    pub fn read_project(&self) -> Result<(i64, f64, i64), String> {
        proj::read_project(&self.pvar.ppq, &self.pvar.bpm, &self.pvar.beats)
            .map_err(|e| format!("{e:?}"))
    }

    /// PPQ 变化后重新求值（pvar 文本不合法时保持原值）。
    pub fn on_project_change(&mut self) {
        if let Ok((ppq, bpm, beats)) = self.read_project() {
            self.ppq = ppq;
            self.bpm = bpm;
            self.beats = beats;
            self.shapes_changed();
        }
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

    /// 保存工程到指定路径。
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

    /// 打开工程文件。
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
                let note = if self.ppq >= i64::from(spiderweb_io::midi::PPQ_WARN) {
                    rust_i18n::t!(
                        "status.ppq_warning",
                        ppq = self.ppq.to_string(),
                        warn = spiderweb_io::midi::PPQ_WARN.to_string()
                    )
                    .to_string()
                } else {
                    String::new()
                };
                self.status = rust_i18n::t!(
                    "status.saved_midi",
                    notes = self.rendered.len().to_string(),
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

    // ------------------------------------------------------------ 音符

    pub fn notes_tracks(&self, sh: &Shape) -> NotesAndTracks {
        engine::shape_notes_tracks(sh, self.ppq as f64)
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

    /// 重新计算全部音符并刷新界面（原版 shapes_changed）。
    pub fn shapes_changed(&mut self) {
        let t0 = Instant::now();
        let got: Vec<NotesAndTracks> = self.shapes.iter().map(|sh| self.notes_tracks(sh)).collect();
        let lists: Vec<Vec<[i64; 4]>> = got.iter().map(|(n, _)| n.clone()).collect();
        let tracks: Vec<Option<Vec<i64>>> = got.iter().map(|(_, t)| t.clone()).collect();
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
        // 音符变了：note_gpu 下一帧重建 instance buffer
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

    // ------------------------------------------------------------ 选择 / 编辑

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
        // 选到了别的形状（或打字时选了东西）：这次输入结束（原版 select_many）
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
        // 选择变了：选中音符要换 layer 颜色，note_gpu 下一帧重建 instance buffer
        self.notes_revision = self.notes_revision.wrapping_add(1);
    }

    /// 拾取选中自定义形状的笔画 k（None = 取消拾取）（原版 set_stroke）。
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
        self.push_undo();
        self.shapes.push(sh);
        self.select(Some(self.shapes.len() - 1), false);
        self.shapes_changed();
    }

    pub fn delete_selected(&mut self) {
        if self.sels.is_empty() {
            return;
        }
        crate::roll_text::end_typing(self);
        self.push_undo();
        for i in self.sels.iter().rev() {
            self.shapes.remove(*i);
        }
        self.select(None, false);
        self.shapes_changed();
    }

    /// Delete 键：选中自定义形状的一条笔画时删笔画，否则删形状（原版 on_key 的 delete）。
    pub fn delete_pressed(&mut self) {
        if self.sels.len() == 1
            && let (Some(i), Some(k)) = (self.sel, self.stroke)
        {
            crate::roll_live::delete_stroke(self, i, k);
            return;
        }
        self.delete_selected();
    }

    /// Delete all：先弹原版的确认框（Ctrl+Z 能撤销）。
    pub fn delete_all(&mut self) {
        if self.shapes.is_empty() {
            return;
        }
        self.pending_delete_all = Some(self.shapes.len());
    }

    /// 确认后真正清空（原版 delete_all 的确认部分之后）。
    fn do_delete_all(&mut self) {
        self.cancel_draft();
        self.push_undo();
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
        self.add_copies(&shapes, shift);
    }

    pub fn add_copies(&mut self, shapes: &[Shape], shift: f64) {
        self.push_undo();
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
        self.push_undo();
        for &i in &idx {
            let sh = &mut self.shapes[i];
            for p in &mut sh.pts {
                if sideways {
                    p[0] = mid2 - p[0];
                } else {
                    p[1] = mid2 - p[1];
                }
            }
            if let Some(tm) = sh.tumour.as_mut() {
                tm.mirror = !tm.mirror;
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
        self.push_undo();
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
            if let Some(tm) = sh.tumour.as_mut() {
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
        self.add_copies(&shapes, at - start);
    }

    // ------------------------------------------------------------ Domino 剪贴板

    pub fn copy_to_domino(&mut self) {
        let notes: Vec<[i64; 6]> = if self.sels.is_empty() {
            self.rendered.clone()
        } else {
            self.rendered
                .iter()
                .filter(|n| self.sels.contains(&(n[5] as usize)))
                .copied()
                .collect()
        };
        if notes.is_empty() {
            self.status = if self.sels.is_empty() {
                rust_i18n::t!("status.no_notes_to_copy").to_string()
            } else {
                rust_i18n::t!("status.selected_shapes_no_notes").to_string()
            };
            return;
        }
        let ppq = self.ppq;
        match spiderweb_domino::clip_data(&notes, self.ppq as u16, self.beats) {
            Ok(raw) => {
                if spiderweb_domino::put_on_clipboard(&raw) {
                    let what = if self.sels.is_empty() {
                        rust_i18n::t!("status.all_notes", n = notes.len().to_string()).to_string()
                    } else {
                        rust_i18n::t!(
                            "status.notes_count",
                            n = notes.len().to_string(),
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
                    self.status = rust_i18n::t!(
                        "status.copied_domino",
                        what = what,
                        ppq = ppq.to_string(),
                        place = place
                    )
                    .to_string();
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
            spiderweb_domino::ClipboardGet::Data(raw) => match spiderweb_domino::read_notes(&raw) {
                Ok((rows, their_ppq)) => {
                    if let Some(mut sh) = notes_shape(&rows, self.ppq as f64, "Pasted notes") {
                        // 粘贴起点对齐到播放线（原版把复制内容的起点放在播放线）
                        let t0 =
                            rows.iter().map(|r| r[0]).min().unwrap_or(0) as f64 / self.ppq as f64;
                        let shift = self.playhead - t0;
                        for p in &mut sh.pts {
                            p[0] += shift;
                        }
                        self.add_shape(sh);
                        let n = rows.len();
                        let note = match their_ppq {
                            Some(p) if i64::from(p) != self.ppq => {
                                rust_i18n::t!("status.domino_ppq", ppq = p.to_string()).to_string()
                            }
                            _ => String::new(),
                        };
                        self.status = rust_i18n::t!(
                            "status.pasted_domino",
                            n = n.to_string(),
                            s = if n == 1 { "" } else { "s" },
                            note = note
                        )
                        .to_string();
                    } else {
                        self.status = rust_i18n::t!("status.domino_no_notes").to_string();
                    }
                }
                Err(e) => {
                    self.status = rust_i18n::t!("status.clipboard_read_error", e = format!("{e:?}"))
                        .to_string()
                }
            },
        }
    }

    // ------------------------------------------------------------ 撤销

    pub fn snapshot(&self) -> String {
        let arr: Vec<serde_json::Value> = self.shapes.iter().map(shape_to_json).collect();
        serde_json::to_string(&serde_json::Value::Array(arr)).unwrap_or_default()
    }

    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > 300 {
            let cut = self.undo_stack.len() - 300;
            self.undo_stack.drain(..cut);
        }
        self.redo_stack.clear();
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
        let src = if from_undo {
            &mut self.undo_stack
        } else {
            &mut self.redo_stack
        };
        let Some(snap) = src.pop() else {
            return;
        };
        let current = self.snapshot();
        if from_undo {
            self.redo_stack.push(current);
        } else {
            self.undo_stack.push(current);
        }
        let value: serde_json::Value =
            serde_json::from_str(&snap).unwrap_or(serde_json::Value::Array(Vec::new()));
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

    // ------------------------------------------------------------ 吸附

    pub fn snap_beats(&self) -> Option<f64> {
        if self.snap == "Off" {
            return None;
        }
        self.snap
            .split('/')
            .nth(1)
            .and_then(|d| d.parse::<f64>().ok())
            .map(|d| 4.0 / d)
    }

    #[allow(dead_code)] // 数字框步进待移植
    pub fn snap_ticks(&self) -> i64 {
        match self.snap_beats() {
            Some(sb) => ((sb * self.ppq as f64).round() as i64).max(1),
            None => 1,
        }
    }

    // ------------------------------------------------------------ 播放

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

    /// 右拖试听：把鼠标下（tick `t_to`）与刚扫过的音符发声（原版 app.scrub）。
    /// false = MIDI 打不开，右拖作废、状态栏写错误。
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
            self.player.note(k.0, k.1.clamp(0, 127) as u8, 0);
            self.scrub_held.remove(&k);
        }
        for (k, (s, v)) in &now {
            if self.scrub_held.get(k) != Some(s) {
                if self.scrub_held.contains_key(k) {
                    self.player.note(k.0, k.1.clamp(0, 127) as u8, 0);
                }
                self.player.note(k.0, k.1.clamp(0, 127) as u8, *v);
                self.scrub_held.insert(*k, *s);
            }
        }
        let ppq = self.ppq.max(1) as f64;
        self.set_playhead(t_to / ppq);
        true
    }

    /// 右拖松开：全部 note off（原版 app.scrub_end）。
    pub fn scrub_end(&mut self) {
        for ((ch, p), _) in std::mem::take(&mut self.scrub_held) {
            self.player.note(ch, p.clamp(0, 127) as u8, 0);
        }
    }

    // ------------------------------------------------------------ 工具

    /// 双击右键：Select <-> 上次的绘图工具（原版 toggle_select_tool）。
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

    /// 提交草稿：Live 绘制 / 方框进自定义形状（原版 commit_draft -> live_commit）。
    pub fn commit_draft(&mut self) {
        if let Some(sh) = self.draft.take()
            && !crate::roll_live::live_commit(self, &sh)
        {
            self.add_shape(sh);
        }
        self.draft_draw = None;
        self.arc_bend = false;
    }

    #[allow(dead_code)] // 拖动后补算待移植
    pub fn catch_up_notes(&mut self) {
        self.shapes_changed();
    }

    /// 提交大形状前确认（原版 confirm_big）。true = 直接提交。
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
        // 抽屉有键盘焦点：按键归抽屉（原版 in_drawer），卷帘快捷键让路
        if self.drawer.as_ref().is_some_and(|d| d.focus) {
            return;
        }
        // 正在打字：按键都归文本（原版 on_key 的 typing 优先级），快捷键让路
        if self.typing.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        // 右键菜单开着：Esc / 点击由菜单自己处理，快捷键让路
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
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::H)) {
                self.flip(true);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::J)) {
                self.flip(false);
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
                    self.help.open = false; // Esc 先关帮助窗口（原版 HelpWindow 的 Escape）
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
                // 高亮的漏斗线与曲线优先（原版 delete_parts）
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
            // 工具热键
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
                        // 换工具时弹这个工具的 tip（看过的不会再弹）
                        self.tips.show(crate::help::tool_topic(tool));
                    }
                }
            }
        });
    }

    pub fn update_playing(&mut self, ctx: &egui::Context) {
        if self.player.running() {
            self.playhead = self.player.position(self.ppq.max(1));
            // 播放线接近右边缘时翻页
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
                notes = self.rendered.len().to_string()
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
                rust_i18n::t!("status.selected", shapes = shapes, notes = n.to_string())
                    .to_string(),
            );
        }
        if !self.status.is_empty() {
            parts.push(self.status.clone());
        }
        parts.join("     ")
    }
}

/// 右拖扫过 tick `t_from..t_to` 时该响的音符（原版 app.scrub 的 `now`）：
/// 每个 (通道, 音高) 只留起点最新的那个，值 = (起点 tick, 力度)。
/// 命中条件同原版：起点落在扫过的区间里，或正被扫过（`s <= t_to < e`）。
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
        // panic 等错误写进 errors.log 后，在状态栏提示一次（errors.rs）
        if let Some(msg) = crate::errors::take_pending() {
            self.status = msg;
        }
        // 文字输入优先：Event::Text / 编辑键先给正在输入的文本，剩下的才轮到快捷键
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
        // 帮助窗口与首次使用 tip（help.rs）
        crate::help::help_ui(self, &ctx);
        crate::help::tips_ui(self, &ctx);

        crate::drawer::drawer_ui(self, &ctx);

        if let Some(pending) = self.pending_big.take() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new("Spiderweb")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(
                        rust_i18n::t!("confirm.big", total = pending.total.to_string()).to_string(),
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
            let dt = ctx.input(|i| i.stable_dt); // 上一帧耗时
            self.frame_ms = self.frame_ms * 0.9 + dt * 1000.0 * 0.1;
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

/// 一个渲染音符行 (start, end, pitch, velocity, slot, owner)。
#[cfg(test)]
fn test_note(s: i64, e: i64, p: i64, v: i64, slot: i64) -> [i64; 6] {
    [s, e, p, v, slot, 0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_hits_takes_notes_under_and_swept() {
        let rendered = vec![
            test_note(0, 100, 60, 64, 0),   // 扫之前就结束了
            test_note(0, 400, 61, 70, 0),   // 还在响（第二个条件）
            test_note(200, 300, 62, 80, 0), // 起点在扫过的区间里
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
            test_note(220, 260, 62, 90, 0), // 同键更晚：背靠背连击响最新的
            test_note(100, 400, 62, 50, 9), // slot 9 = 鼓通道后的通道 10，另一路
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
        // 区间不相交就不响
        let missed = scrub_hits(&rendered, 500.0, 600.0);
        assert!(missed.is_empty());
    }
}
