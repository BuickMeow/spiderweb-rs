//! 主窗口状态：工具栏、侧栏、形状编辑、撤销、自动保存、播放与文件操作。
//! 钢琴卷帘的绘制与交互在 roll.rs，侧栏在 panels.rs。

use std::collections::BTreeSet;
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

use crate::playback::{DEFAULT_DEVICE, Player};
use crate::roll::{Drag, View};
use crate::roll_text::Typing;
use crate::roll_velocity::VelocityState;
use crate::text_dialog::FontDialog;

#[allow(dead_code)] // 关于窗口待移植
pub const VERSION: &str = "0.1.0";

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
    pub drag: Option<Drag>,
    pub follow: Option<Drag>,
    pub arc_bend: bool,
    /// 右键按下时已处理（曲线删点 / 收手柄）：松开时不再取消选择。
    pub right_done: bool,
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
    pub free_smooth: i64,
    pub text_defaults: TextSettings,
    #[allow(dead_code)] // 自定义形状工具待移植
    pub custom_shape: String,
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
    pub clipboard: Vec<Shape>,
    pub loaded: bool,
    pub panel_sel: Option<usize>,
    pub vel_text: [String; 2],
    /// 正在输入的文本（原版 roll.typing）
    pub typing: Option<Typing>,
    /// 文本剪切板的内容，等有 ctx 的时候写进系统剪贴板
    pub text_clipboard: Option<String>,
    /// 字体选择窗口
    pub font_dialog: Option<FontDialog>,
    /// 已安装的字体族名（第一次用到时读一次）
    pub font_families: Vec<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let autosave_path = base.join("autosave.json");
        let output = base.join("spiderweb.mid").to_string_lossy().into_owned();
        let mut app = Self {
            shapes: Vec::new(),
            sels: BTreeSet::new(),
            sel: None,
            draft: None,
            drag: None,
            follow: None,
            arc_bend: false,
            right_done: false,
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
            free_smooth: 0,
            text_defaults: TextSettings::default(),
            custom_shape: "Circle".to_string(),
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
            clipboard: Vec::new(),
            loaded: false,
            panel_sel: None,
            vel_text: ["127".into(), "127".into()],
            typing: None,
            text_clipboard: None,
            font_dialog: None,
            font_families: Vec::new(),
        };
        cc.egui_ctx
            .set_pixels_per_point(cc.egui_ctx.pixels_per_point());
        app.load_autosave();
        app.loaded = true;
        app.shapes_changed();
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
                    Some(p) => format!("autosave 打不开，已改名为 {}", p.display()),
                    None => "autosave 打不开".to_string(),
                };
            }
        }
    }

    pub fn apply_project(&mut self, p: Project) {
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
            match p.write(&path, None) {
                Ok(()) => self.status = format!("已保存 {}", path.display()),
                Err(e) => self.status = format!("保存失败：{e}"),
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
                    self.status = format!("已打开 {}", path.display());
                }
                Err(e) => self.status = format!("打不开：{e:?}"),
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
            Ok(()) => self.status = format!("已写出 {}", path.display()),
            Err(e) => self.status = format!("写出失败：{e:?}"),
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
            return "Pasted notes".to_string();
        }
        if sh.text.is_some() {
            return format!("Text: {}", if sh.name.is_empty() { "?" } else { &sh.name });
        }
        if sh.kind == Kind::Custom {
            return format!(
                "Custom: {}",
                if sh.name.is_empty() { "?" } else { &sh.name }
            );
        }
        engine::KINDS
            .iter()
            .find(|(k, _)| *k == sh.kind)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| sh.kind.as_str().to_string())
    }

    /// 重新计算全部音符并刷新界面（原版 shapes_changed）。
    pub fn shapes_changed(&mut self) {
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
        self.schedule_autosave();
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
        self.edit_key = None;
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

    pub fn delete_all(&mut self) {
        if self.shapes.is_empty() {
            return;
        }
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
        self.status = format!(
            "已复制 {} 个形状 — Ctrl+V 粘贴到播放线",
            self.clipboard.len()
        );
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
            self.status = "没有音符可复制".to_string();
            return;
        }
        match spiderweb_domino::clip_data(&notes, self.ppq as u16, self.beats) {
            Ok(raw) => {
                if spiderweb_domino::put_on_clipboard(&raw) {
                    self.status = "已复制到 Domino 剪贴板".to_string();
                } else {
                    self.status = "剪贴板不可用（仅 Windows 支持 Domino 格式）".to_string();
                }
            }
            Err(e) => self.status = format!("复制失败：{e:?}"),
        }
    }

    pub fn paste_from_domino(&mut self) {
        match spiderweb_domino::get_from_clipboard() {
            spiderweb_domino::ClipboardGet::Busy | spiderweb_domino::ClipboardGet::NoData => {
                self.status = "剪贴板里没有 Domino 数据".to_string();
            }
            spiderweb_domino::ClipboardGet::Data(raw) => match spiderweb_domino::read_notes(&raw) {
                Ok((rows, _ppq)) => {
                    if let Some(mut sh) = notes_shape(&rows, self.ppq as f64, "From Domino") {
                        // 粘贴起点对齐到播放线（原版把复制内容的起点放在播放线）
                        let t0 =
                            rows.iter().map(|r| r[0]).min().unwrap_or(0) as f64 / self.ppq as f64;
                        let shift = self.playhead - t0;
                        for p in &mut sh.pts {
                            p[0] += shift;
                        }
                        self.add_shape(sh);
                        self.status = "已从 Domino 粘贴".to_string();
                    } else {
                        self.status = "Domino 数据里没有音符".to_string();
                    }
                }
                Err(e) => self.status = format!("Domino 数据读不了：{e:?}"),
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

    // ------------------------------------------------------------ 工具

    pub fn cancel_draft(&mut self) {
        crate::roll_text::end_typing(self);
        self.draft = None;
        self.drag = None;
        self.follow = None;
        self.arc_bend = false;
    }

    pub fn commit_draft(&mut self) {
        if let Some(sh) = self.draft.take() {
            self.add_shape(sh);
        }
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
        self.pending_big = Some(PendingBig {
            shapes: vec![d],
            total: n,
        });
        false
    }

    // ------------------------------------------------------------ eframe

    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // 正在打字：按键都归文本（原版 on_key 的 typing 优先级），快捷键让路
        if self.typing.is_some() || ctx.egui_wants_keyboard_input() {
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
                self.cancel_draft();
                self.select(None, false);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Delete)) {
                self.delete_selected();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::G)) {
                self.live = !self.live;
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
        parts.push(format!(
            "{} shapes · {} notes",
            self.shapes.len(),
            self.rendered.len()
        ));
        if self.channel_mode == ChannelMode::Auto && self.slot_count > 0 {
            parts.push(format!("{} tracks (one channel each)", self.slot_count));
        }
        if !self.sels.is_empty() {
            let n: usize = self
                .sels
                .iter()
                .map(|&i| self.note_counts.get(i).copied().unwrap_or(0))
                .sum();
            parts.push(format!("selected: {} notes", n));
        }
        if !self.status.is_empty() {
            parts.push(self.status.clone());
        }
        parts.join("     ")
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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

        if let Some(pending) = self.pending_big.take() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new("Spiderweb")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(format!(
                        "这一步会生成约 {} 个音符，可能让程序变卡。继续吗？",
                        pending.total
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("继续").clicked() {
                            go = true;
                        }
                        if ui.button("取消").clicked() {
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
    }

    fn on_exit(&mut self) {
        self.stop_play();
        self.autosave_now();
        self.player.close();
    }
}
