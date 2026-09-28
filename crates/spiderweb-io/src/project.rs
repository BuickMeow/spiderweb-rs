//! 工程文件、autosave 与 autosave 备份（Python `files/project.py` 的移植）。
//!
//! - [`Project`]：一个工程的全部设置与形状；[`Project::from_json`] / [`Project::to_json`]
//!   对应 `ProjectFiles.load_file` / `project_data`，[`Project::load`] / [`Project::write`]
//!   走 [`safefile`] 的原子写。
//! - [`project_json`] / [`Project::to_project_json`]：Python 那种“每个设置一行、每个形状一行”
//!   的 JSON 版式（`short_shape`：float 去掉噪声位、`vel_env` 的速度取 4 位小数等）。
//! - [`load_autosave`] / [`backup_path`]：autosave 打不开时改名留档、回退到上次启动时的备份。
//!
//! 与原版的差异：字符串 / 数值的 `str()` 边界（容器、下划线数字）只做近似；autosave 的
//! 时间戳由调用方传入（`stamp`），这样不依赖本地时间与时区，测试也确定。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use spiderweb_core::shape::{
    Align, Fill, FunnelFill, GateChange, GateFollow, Shape, TextSettings, WallMode,
};
use spiderweb_core::smooth::{SMOOTH_DEFAULT, clean_level};
use spiderweb_core::text::clean_text;

use crate::compat::{
    SHAPE_DEFAULTS, ShapeDefaults, ShapeError, align_str, fill_str, py_bool, py_float, py_int,
    py_str, shape_from_json, shape_to_json, short_num_f,
};
use crate::safefile;

/// 本程序版本（Python `files/about.VERSION`）。
pub const VERSION: &str = "1.1.0";
/// 工程文件格式版本（`project_data` 里写死的 2）。
pub const PROJECT_VERSION: i64 = 2;
/// 吸附选项（project.SNAPS）。
pub const SNAPS: [&str; 13] = [
    "Off", "1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64", "1/128", "1/6", "1/12", "1/24",
    "1/48",
];
/// 通道模式（engine.CHANNEL_MODES）。
pub const CHANNEL_MODES: [&str; 3] = ["raw", "single", "auto"];
/// 多通道时重叠的判定方式（engine.SPLITS）。
pub const SPLITS: [&str; 2] = ["key", "time"];

/// 通道模式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelMode {
    Raw,
    #[default]
    Single,
    Auto,
}

impl ChannelMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelMode::Raw => "raw",
            ChannelMode::Single => "single",
            ChannelMode::Auto => "auto",
        }
    }
}

/// 重叠按什么算（按 key 还是按时间）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelSplit {
    #[default]
    Key,
    Time,
}

impl ChannelSplit {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelSplit::Key => "key",
            ChannelSplit::Time => "time",
        }
    }
}

/// 新自定义形状的默认设置（custom.CUSTOM_DEFAULTS + 库里选的图形）。
#[derive(Clone, Debug, PartialEq)]
pub struct CustomDefaults {
    pub fill: Fill,
    pub gate: f64,
    pub align: Align,
    pub shape: String,
}

impl Default for CustomDefaults {
    fn default() -> Self {
        Self {
            fill: Fill::Empty,
            gate: 0.0625,
            align: Align::Auto,
            shape: "Circle".to_string(),
        }
    }
}

/// 新漏斗的默认设置（funnel.FUNNEL_DEFAULTS）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FunnelDefaults {
    pub fill: FunnelFill,
    pub gate0: f64,
    pub gate1: f64,
    pub vary: bool,
    pub change: GateChange,
    pub follow: GateFollow,
    pub wall: WallMode,
}

impl Default for FunnelDefaults {
    fn default() -> Self {
        Self {
            fill: FunnelFill::Spam,
            gate0: 0.0625,
            gate1: 0.0625,
            vary: false,
            change: GateChange::Steps,
            follow: GateFollow::Time,
            wall: WallMode::In,
        }
    }
}

/// 卷轴的缩放与滚动位置（pianoroll.view_state）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub t: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
}

impl ViewState {
    /// `pianoroll.set_view`：四个数都能转成 float 才有效。
    pub fn from_json(v: &Value) -> Option<Self> {
        let d = v.as_object()?;
        Some(Self {
            t: py_float(d.get("t")?)?,
            top: py_float(d.get("top")?)?,
            sx: py_float(d.get("sx")?)?,
            sy: py_float(d.get("sy")?)?,
        })
    }

    pub fn to_json(self) -> Value {
        serde_json::json!({"t": self.t, "top": self.top, "sx": self.sx, "sy": self.sy})
    }
}

/// autosave 里记下的窗口状态（`write_json(window=True)` 的 “window”）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowState {
    pub geometry: String,
    pub maximized: bool,
    pub velocity: bool,
    pub velocity_height: Option<f64>,
    pub midi_device: String,
    pub live: bool,
    /// tips 等其余键（原样保留）。
    pub rest: Map<String, Value>,
}

impl WindowState {
    pub fn from_json(v: Option<&Value>) -> Self {
        let Some(d) = v.and_then(Value::as_object) else {
            return Self::default();
        };
        let mut out = Self::default();
        if let Some(Value::String(s)) = d.get("geometry") {
            out.geometry = s.clone();
        }
        out.maximized = d.get("maximized").is_some_and(py_bool);
        out.velocity = d.get("velocity") == Some(&Value::Bool(true));
        out.velocity_height = d.get("velocity_height").and_then(py_float);
        if let Some(v) = d.get("midi_device")
            && py_bool(v)
        {
            out.midi_device = py_str(v);
        }
        out.live = d.get("live") == Some(&Value::Bool(true));
        for (k, v) in d {
            if !matches!(
                k.as_str(),
                "geometry" | "maximized" | "velocity" | "velocity_height" | "midi_device" | "live"
            ) {
                out.rest.insert(k.clone(), v.clone());
            }
        }
        out
    }

    pub fn to_json(&self) -> Value {
        let mut d = self.rest.clone();
        d.insert("geometry".into(), Value::from(self.geometry.clone()));
        d.insert("maximized".into(), Value::Bool(self.maximized));
        d.insert("velocity".into(), Value::Bool(self.velocity));
        if let Some(h) = self.velocity_height {
            d.insert("velocity_height".into(), Value::from(h));
        }
        d.insert("midi_device".into(), Value::from(self.midi_device.clone()));
        d.insert("live".into(), Value::Bool(self.live));
        Value::Object(d)
    }
}

/// 工程文件的读取错误；Python 里 `load_file` 出错返回 False。
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("工程文件读不开: {0}")]
    Io(#[from] io::Error),
    #[error("工程文件不是 JSON")]
    Json(#[from] serde_json::Error),
    #[error("工程文件不是 JSON 对象")]
    NotObject,
    #[error("工程文件的 defaults / custom_defaults 坏了")]
    Defaults,
    #[error("工程文件的 shapes 坏了")]
    Shapes,
    #[error("工程里的形状坏了: {0}")]
    Shape(#[from] ShapeError),
}

/// Python `project_data` 里键的出现顺序。
const PROJECT_KEYS: [&str; 17] = [
    "version",
    "app_version",
    "ppq",
    "bpm",
    "beats",
    "output",
    "channel_mode",
    "channel_split",
    "snap",
    "defaults",
    "custom_defaults",
    "funnel_defaults",
    "text_defaults",
    "free_smooth",
    "shapes",
    "view",
    "playhead",
];

/// 一个工程（Python App 的工程相关状态）。
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// 工程文件格式版本（写 2）。
    pub version: i64,
    /// 保存它的 Spiderweb 版本。
    pub app_version: String,
    /// PPQ 文本框（数字或表达式，用 [`Project::read_project`] 求值）。
    pub ppq: String,
    /// BPM 文本框。
    pub bpm: String,
    /// 每小节拍数文本框。
    pub beats: String,
    /// MIDI 输出路径。
    pub output: String,
    pub channel_mode: ChannelMode,
    pub channel_split: ChannelSplit,
    pub snap: String,
    pub defaults: ShapeDefaults,
    pub custom_defaults: CustomDefaults,
    pub funnel_defaults: FunnelDefaults,
    pub text_defaults: TextSettings,
    pub free_smooth: i64,
    pub shapes: Vec<Shape>,
    pub view: Option<ViewState>,
    pub playhead: f64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            app_version: VERSION.to_string(),
            ppq: "960".to_string(),
            bpm: "120".to_string(),
            beats: "4".to_string(),
            output: String::new(),
            channel_mode: ChannelMode::Single,
            channel_split: ChannelSplit::Key,
            snap: "1/16".to_string(),
            defaults: SHAPE_DEFAULTS,
            custom_defaults: CustomDefaults::default(),
            funnel_defaults: FunnelDefaults::default(),
            text_defaults: TextSettings::default(),
            free_smooth: SMOOTH_DEFAULT,
            shapes: Vec::new(),
            view: None,
            playhead: 0.0,
        }
    }
}

/// `read_project`：PPQ / BPM / 拍数求值；错误消息与原版一致。
pub fn read_project(
    ppq: &str,
    bpm: &str,
    beats: &str,
) -> Result<(i64, f64, i64), crate::mathexpr::MathError> {
    use crate::mathexpr as mx;
    let ppq = mx::calc_int(ppq, Some(1), Some(65535))
        .map_err(|_| mx::MathError::Message("PPQ must be a whole number from 1 to 65535".into()))?;
    let bpm_value = mx::calc(bpm)?;
    let bpm_value = match bpm_value.as_f64() {
        Some(v) if (4.0..=100_000.0).contains(&v) => v,
        _ => {
            return Err(mx::MathError::Message(
                "BPM must be a number, at least 4".into(),
            ));
        }
    };
    let beats = mx::calc_int(beats, Some(1), Some(32)).map_err(|_| {
        mx::MathError::Message("Beats per bar must be a whole number from 1 to 32".into())
    })?;
    Ok((ppq, bpm_value, beats))
}

impl Project {
    /// 求值 PPQ / BPM / 拍数（`read_project`）。
    pub fn read_project(&self) -> Result<(i64, f64, i64), crate::mathexpr::MathError> {
        read_project(&self.ppq, &self.bpm, &self.beats)
    }

    /// 对应 `load_file` 的数据部分（窗口 / 撤销栈等 UI 状态不在 io 层）。
    pub fn from_json(data: &Value) -> Result<Self, ProjectError> {
        let d = data.as_object().ok_or(ProjectError::NotObject)?;
        let mut p = Self::default();
        for (key, field) in [
            ("ppq", &mut p.ppq),
            ("bpm", &mut p.bpm),
            ("beats", &mut p.beats),
            ("output", &mut p.output),
        ] {
            if let Some(v) = d.get(key) {
                *field = py_str(v);
            }
        }
        let mode = match d.get("channel_mode") {
            Some(Value::String(s)) if CHANNEL_MODES.contains(&s.as_str()) => s.as_str(),
            Some(_) => "single",
            None => {
                if d.get("auto_channels").is_some_and(py_bool) {
                    "auto"
                } else {
                    "single"
                }
            }
        };
        p.channel_mode = match mode {
            "raw" => ChannelMode::Raw,
            "auto" => ChannelMode::Auto,
            _ => ChannelMode::Single,
        };
        p.channel_split = match d.get("channel_split").and_then(Value::as_str) {
            Some("time") => ChannelSplit::Time,
            _ => ChannelSplit::Key,
        };
        if let Some(s) = d.get("snap").and_then(Value::as_str)
            && SNAPS.contains(&s)
        {
            p.snap = s.to_string();
        }
        p.defaults = defaults_from_json(d.get("defaults"))?;
        if let Some(custom) = d.get("custom_defaults").and_then(Value::as_object) {
            p.custom_defaults = custom_defaults_from_json(custom);
        }
        if let Some(funnel) = d.get("funnel_defaults").and_then(Value::as_object)
            && let Some(f) = funnel_defaults_from_json(funnel)
        {
            p.funnel_defaults = f;
        }
        if let Some(Value::Object(m)) = d.get("text_defaults") {
            let mut m = m.clone();
            m.insert(
                "bbox".into(),
                Value::Array(vec![0.0.into(), 0.0.into(), 1.0.into(), 1.0.into()]),
            );
            if let Some(tx) = clean_text(&Value::Object(m)) {
                p.text_defaults = tx;
            }
        }
        p.free_smooth = clean_level(d.get("free_smooth").and_then(py_float).unwrap_or(f64::NAN));
        let shapes = match d.get("shapes") {
            None => &[] as &[Value],
            Some(Value::Array(a)) => a.as_slice(),
            Some(_) => return Err(ProjectError::Shapes),
        };
        for sh in shapes {
            if let Some(shape) = shape_from_json(sh)? {
                p.shapes.push(shape);
            }
        }
        p.view = d.get("view").and_then(ViewState::from_json);
        p.playhead = d
            .get("playhead")
            .and_then(py_float)
            .map_or(0.0, |v| v.max(0.0));
        Ok(p)
    }

    /// 对应 `project_data` 的字典。
    pub fn to_json(&self) -> Value {
        let mut d = Map::new();
        d.insert("version".into(), Value::from(PROJECT_VERSION));
        d.insert("app_version".into(), Value::from(self.app_version.clone()));
        d.insert("ppq".into(), Value::from(self.ppq.clone()));
        d.insert("bpm".into(), Value::from(self.bpm.clone()));
        d.insert("beats".into(), Value::from(self.beats.clone()));
        d.insert("output".into(), Value::from(self.output.clone()));
        d.insert(
            "channel_mode".into(),
            Value::from(self.channel_mode.as_str()),
        );
        d.insert(
            "channel_split".into(),
            Value::from(self.channel_split.as_str()),
        );
        d.insert("snap".into(), Value::from(self.snap.clone()));
        d.insert(
            "defaults".into(),
            serde_json::json!({
                "vel0": self.defaults.vel0,
                "vel1": self.defaults.vel1,
                "end_dot": self.defaults.end_dot,
            }),
        );
        d.insert(
            "custom_defaults".into(),
            serde_json::json!({
                "fill": fill_str(self.custom_defaults.fill),
                "gate": self.custom_defaults.gate,
                "align": align_str(self.custom_defaults.align),
                "shape": self.custom_defaults.shape,
            }),
        );
        d.insert(
            "funnel_defaults".into(),
            serde_json::json!({
                "fill": match self.funnel_defaults.fill { FunnelFill::Long => "long", FunnelFill::Spam => "spam" },
                "gate0": self.funnel_defaults.gate0,
                "gate1": self.funnel_defaults.gate1,
                "vary": self.funnel_defaults.vary,
                "change": match self.funnel_defaults.change { GateChange::Smooth => "smooth", GateChange::Steps => "steps" },
                "follow": match self.funnel_defaults.follow { GateFollow::Curve => "curve", GateFollow::Time => "time" },
                "wall": match self.funnel_defaults.wall { WallMode::Past => "past", WallMode::In => "in" },
            }),
        );
        d.insert(
            "text_defaults".into(),
            text_defaults_json(&self.text_defaults),
        );
        d.insert("free_smooth".into(), Value::from(self.free_smooth));
        d.insert(
            "shapes".into(),
            Value::Array(self.shapes.iter().map(shape_to_json).collect()),
        );
        d.insert(
            "view".into(),
            self.view.map_or(Value::Null, ViewState::to_json),
        );
        d.insert("playhead".into(), Value::from(self.playhead));
        Value::Object(d)
    }

    /// Python `project_json(data)` 的版式：每个设置一行、每个形状一行；`window` 附在最后。
    pub fn to_project_json_with_window(&self, window: Option<&WindowState>) -> String {
        let data = self.to_json();
        let mut extra: Vec<(String, Value)> = Vec::new();
        if let Some(w) = window {
            extra.push(("window".to_string(), w.to_json()));
        }
        project_json_extra(&data, &extra)
    }

    /// [`Project::to_project_json_with_window`] 不带窗口。
    pub fn to_project_json(&self) -> String {
        self.to_project_json_with_window(None)
    }

    /// 原子地保存（Python `write_json`）。
    pub fn write(&self, path: &Path, window: Option<&WindowState>) -> io::Result<()> {
        safefile::write_text(path, &self.to_project_json_with_window(window))
    }

    /// 读一个工程文件（Python `load_file` 的文件部分）。
    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        let text = fs::read_to_string(path)?;
        let data: Value = serde_json::from_str(&text)?;
        Self::from_json(&data)
    }
}

fn defaults_from_json(v: Option<&Value>) -> Result<ShapeDefaults, ProjectError> {
    let Some(v) = v else {
        return Ok(SHAPE_DEFAULTS);
    };
    let d = v.as_object().ok_or(ProjectError::Defaults)?;
    let mut out = SHAPE_DEFAULTS;
    // 原版是 type(SHAPE_DEFAULTS[k])(v)：SHAPE_DEFAULTS 里 vel0 / vel1 是 int，会截断
    if let Some(v) = d.get("vel0") {
        out.vel0 = py_int(v).ok_or(ProjectError::Defaults)? as f64;
    }
    if let Some(v) = d.get("vel1") {
        out.vel1 = py_int(v).ok_or(ProjectError::Defaults)? as f64;
    }
    if let Some(v) = d.get("end_dot") {
        out.end_dot = py_bool(v);
    }
    Ok(out)
}

fn custom_defaults_from_json(d: &Map<String, Value>) -> CustomDefaults {
    let mut out = CustomDefaults::default();
    match d.get("fill").and_then(Value::as_str) {
        Some("fill") => out.fill = Fill::Fill,
        Some("spam") => out.fill = Fill::Spam,
        Some("outline_spam") => out.fill = Fill::OutlineSpam,
        Some("empty") => out.fill = Fill::Empty,
        _ => {}
    }
    match d.get("align").and_then(Value::as_str) {
        Some("auto") => out.align = Align::Auto,
        Some("aligned") => out.align = Align::Aligned,
        _ => {}
    }
    if let Some(gate) = d.get("gate").and_then(py_float) {
        out.gate = gate.max(1e-6);
    }
    if let Some(shape) = d.get("shape")
        && py_bool(shape)
    {
        out.shape = py_str(shape);
    }
    out
}

/// `clean_funnel` 的默认值路径；坏 gate 返回 None（原版 try 后保留旧默认）。
fn funnel_defaults_from_json(d: &Map<String, Value>) -> Option<FunnelDefaults> {
    let mut out = FunnelDefaults::default();
    out.fill = match d.get("fill").and_then(Value::as_str) {
        Some("long") => FunnelFill::Long,
        Some("spam") => FunnelFill::Spam,
        _ => out.fill,
    };
    out.change = match d.get("change").and_then(Value::as_str) {
        Some("smooth") => GateChange::Smooth,
        Some("steps") => GateChange::Steps,
        _ => out.change,
    };
    out.follow = match d.get("follow").and_then(Value::as_str) {
        Some("curve") => GateFollow::Curve,
        Some("time") => GateFollow::Time,
        _ => out.follow,
    };
    out.wall = match d.get("wall").and_then(Value::as_str) {
        Some("past") => WallMode::Past,
        Some("in") => WallMode::In,
        _ => out.wall,
    };
    out.gate0 = py_float(d.get("gate0").unwrap_or(&Value::from(0.0625)))?.max(1e-6);
    out.gate1 = py_float(d.get("gate1").unwrap_or(&Value::from(0.0625)))?.max(1e-6);
    out.vary = match d.get("vary") {
        Some(v) => py_bool(v),
        None => (out.gate0 - out.gate1).abs() > 1e-9,
    };
    if !out.vary {
        out.gate1 = out.gate0;
    }
    Some(out)
}

/// text_defaults 只保存 TEXT_DEFAULTS 的那几个键。
fn text_defaults_json(tx: &TextSettings) -> Value {
    serde_json::json!({
        "font": tx.font,
        "size": tx.size,
        "unit": if tx.unit == spiderweb_core::shape::TextUnit::Rows { "rows" } else { "font" },
        "weight": tx.weight,
        "italic": tx.italic,
        "tracking": tx.tracking,
        "leading": tx.leading,
        "align": match tx.align {
            spiderweb_core::shape::TextAlign::Center => "center",
            spiderweb_core::shape::TextAlign::Right => "right",
            spiderweb_core::shape::TextAlign::Left => "left",
        },
        "threshold": tx.threshold,
        "grow": tx.grow,
    })
}

// ---------------------------------------------------------------- Python 风格 JSON

/// Python `json.dumps` 兼容的序列化（ensure_ascii、", " / ": " 分隔、float 用 repr）。
pub fn dumps(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                python_float_repr(f)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => quote(s),
        Value::Array(a) => format!("[{}]", a.iter().map(dumps).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", quote(k), dumps(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `repr(float)`（JSON 里的数字；inf / nan 写 Infinity / NaN）。
pub fn python_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == f64::INFINITY {
        return "Infinity".to_string();
    }
    if x == f64::NEG_INFINITY {
        return "-Infinity".to_string();
    }
    let s = format!("{x:?}");
    let Some(i) = s.find(['e', 'E']) else {
        return s;
    };
    let (m, e) = s.split_at(i);
    let e = &e[1..];
    let (sign, digits) = match e.strip_prefix('-') {
        Some(d) => ('-', d),
        None => ('+', e.strip_prefix('+').unwrap_or(e)),
    };
    let exp: i32 = digits.parse().unwrap_or(0);
    format!("{m}e{sign}{exp:02}")
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let cp = c as u32;
                if cp > 0xffff {
                    let cp = cp - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (cp >> 10),
                        0xdc00 + (cp & 0x3ff)
                    ));
                } else {
                    out.push_str(&format!("\\u{cp:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn num_eq(a: &Value, b: &Value) -> bool {
    match (py_float(a), py_float(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn short_num12(v: &Value) -> Value {
    match v.as_f64() {
        Some(f) if v.is_f64() => short_num_f(f, 12),
        _ => v.clone(),
    }
}

fn short_pts(v: &Value) -> Value {
    let Some(a) = v.as_array() else {
        return v.clone();
    };
    Value::Array(
        a.iter()
            .map(|p| match p.as_array() {
                Some(row) => Value::Array(row.iter().map(short_num12).collect()),
                None => p.clone(),
            })
            .collect(),
    )
}

fn short_stroke(st: &Value) -> Value {
    let Some(d) = st.as_object() else {
        return st.clone();
    };
    if d.get("kind").and_then(Value::as_str) == Some("ellipse") {
        let mut o = Map::new();
        o.insert("kind".into(), Value::from("ellipse"));
        o.insert(
            "box".into(),
            d.get("box").map_or(Value::Null, |b| {
                Value::Array(
                    b.as_array()
                        .map_or_else(Vec::new, |a| a.iter().map(short_num12).collect()),
                )
            }),
        );
        return Value::Object(o);
    }
    let mut o = d.clone();
    if let Some(pts) = d.get("pts") {
        o.insert("pts".into(), short_pts(pts));
    }
    Value::Object(o)
}

fn short_env(v: &Value) -> Value {
    let Some(a) = v.as_array() else {
        return v.clone();
    };
    let mut out: Vec<Value> = Vec::with_capacity(a.len());
    let mut prev = 0.0f64;
    for row in a {
        let Some(p) = row.as_array() else {
            return v.clone();
        };
        let (Some(u), Some(val)) = (p.first().and_then(py_float), p.get(1).and_then(py_float))
        else {
            return v.clone();
        };
        let mut su = short_num_f(u, 12);
        if let Some(last) = out.last().and_then(Value::as_array).and_then(|r| r.first())
            && num_eq(last, &su)
            && u != prev
        {
            su = Value::from(u);
        }
        out.push(Value::Array(vec![
            su,
            short_num_f(crate::compat::round4(val), 12),
        ]));
        prev = u;
    }
    Value::Array(out)
}

fn short_starts(v: &Value) -> Value {
    let Some(a) = v.as_array() else {
        return v.clone();
    };
    Value::Array(
        a.iter()
            .map(|st| {
                let Some(d) = st.as_object() else {
                    return st.clone();
                };
                let mut o = Map::new();
                o.insert(
                    "line".into(),
                    d.get("line").cloned().unwrap_or_else(|| Value::from(0)),
                );
                o.insert("at".into(), d.get("at").map_or(Value::Null, short_num12));
                o.insert(
                    "ends".into(),
                    Value::Array(d.get("ends").and_then(Value::as_array).map_or_else(
                        Vec::new,
                        |ends| {
                            ends.iter()
                                .map(|c| match c.as_object() {
                                    Some(cd) => {
                                        let mut co = cd.clone();
                                        if let Some(pts) = cd.get("pts") {
                                            co.insert("pts".into(), short_pts(pts));
                                        }
                                        Value::Object(co)
                                    }
                                    None => c.clone(),
                                })
                                .collect()
                        },
                    )),
                );
                Value::Object(o)
            })
            .collect(),
    )
}

fn short_text(v: &Value) -> Value {
    let Some(d) = v.as_object() else {
        return v.clone();
    };
    let mut o = Map::new();
    for (k, x) in d {
        if k == "bbox" {
            o.insert(
                k.clone(),
                Value::Array(
                    x.as_array()
                        .map_or_else(Vec::new, |a| a.iter().map(short_num12).collect()),
                ),
            );
        } else {
            o.insert(k.clone(), short_num12(x));
        }
    }
    Value::Object(o)
}

/// Python `short_shape`：形状字典里该舍入的数字舍入（12 位有效数字）。
pub fn short_shape_value(sh: &Value) -> Value {
    let Some(d) = sh.as_object() else {
        return sh.clone();
    };
    let mut o = Map::new();
    for (k, v) in d {
        let nv = match k.as_str() {
            "vel_env" => short_env(v),
            "pts" => short_pts(v),
            "strokes" => Value::Array(
                v.as_array()
                    .map_or_else(Vec::new, |a| a.iter().map(short_stroke).collect()),
            ),
            "starts" => short_starts(v),
            "tumour" => match v.as_object() {
                Some(t) => {
                    Value::Object(t.iter().map(|(a, b)| (a.clone(), short_num12(b))).collect())
                }
                None => v.clone(),
            },
            "text" => short_text(v),
            "gate" | "gate0" | "gate1" | "k" => short_num12(v),
            _ => v.clone(),
        };
        o.insert(k.clone(), nv);
    }
    Value::Object(o)
}

fn shorten_obj(m: &Map<String, Value>, digits: u32) -> Value {
    Value::Object(
        m.iter()
            .map(|(k, v)| {
                let nv = if v.is_f64() {
                    v.as_f64()
                        .map_or_else(|| v.clone(), |f| short_num_f(f, digits))
                } else {
                    v.clone()
                };
                (k.clone(), nv)
            })
            .collect(),
    )
}

/// Python `project_json(data)`（`data` 是工程字典；顺序由字典自身决定，serde_json 默认按键排序）。
pub fn project_json(data: &Value) -> String {
    project_json_extra(data, &[])
}

/// [`project_json`] 加上 Python `write_json` 在末尾补的 `window`。
pub fn project_json_extra(data: &Value, extra: &[(String, Value)]) -> String {
    let Some(d) = data.as_object() else {
        return "{}\n".to_string();
    };
    let ordered: Vec<(&String, &Value)> = PROJECT_KEYS
        .iter()
        .filter_map(|k| d.get_key_value(*k))
        .chain(
            d.iter()
                .filter(|(k, _)| !PROJECT_KEYS.contains(&k.as_str())),
        )
        .chain(extra.iter().map(|(k, v)| (k, v)))
        .collect();
    let mut lines: Vec<String> = Vec::with_capacity(ordered.len());
    for (k, v) in ordered {
        if k == "shapes" {
            let shapes = v.as_array().cloned().unwrap_or_default();
            if shapes.is_empty() {
                lines.push("\"shapes\": []".to_string());
            } else {
                let body = shapes
                    .iter()
                    .map(|sh| dumps(&short_shape_value(sh)))
                    .collect::<Vec<_>>()
                    .join(",\n  ");
                lines.push(format!("\"shapes\": [\n  {body}\n ]"));
            }
            continue;
        }
        let v = match v {
            Value::Object(m) => shorten_obj(m, 6),
            other => other.clone(),
        };
        lines.push(format!("{}: {}", quote(k), dumps(&v)));
    }
    format!("{{\n {}\n}}\n", lines.join(",\n "))
}

// ---------------------------------------------------------------- autosave

/// autosave.json -> autosave-backup.json（普通工程文件，Open project 也能开）。
pub fn backup_path(autosave: &Path) -> PathBuf {
    let name = autosave
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext_len = match name.rfind('.') {
        Some(i) if i > 0 && !name[..i].chars().all(|c| c == '.') => name.len() - i,
        _ => 0,
    };
    let stem = &name[..name.len() - ext_len];
    let backup = format!("{stem}-backup.json");
    match autosave.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(backup),
        _ => PathBuf::from(backup),
    }
}

/// `load_autosave` 的结果。
#[derive(Debug)]
pub enum AutosaveOutcome {
    /// 没有 autosave 文件。
    Missing,
    /// autosave 正常打开（并已复制成备份）。
    Opened(Box<Project>),
    /// autosave 打不开：改名为 `renamed_to`（失败为 None），回退到备份（没有为 None）。
    Damaged {
        renamed_to: Option<PathBuf>,
        backup: Option<Box<Project>>,
    },
}

/// 启动时打开 autosave（Python `load_autosave`）：能开就把它复制成备份；打不开就改名留档，
/// 再试备份。`stamp` 是时间戳文本（原版 `time.strftime("autosave-broken-%Y%m%d-%H%M%S")`），
/// 由调用方提供，重名时依次加 `-2`、`-3`……
pub fn load_autosave(autosave: &Path, stamp: &str) -> AutosaveOutcome {
    if !autosave.exists() {
        return AutosaveOutcome::Missing;
    }
    if let Ok(project) = Project::load(autosave) {
        if let Ok(bytes) = fs::read(autosave) {
            let backup = backup_path(autosave);
            let _ = safefile::write_bytes(&backup, &bytes);
        }
        return AutosaveOutcome::Opened(Box::new(project));
    }
    let dir = autosave.parent().unwrap_or_else(|| Path::new(""));
    let mut broken = dir.join(format!("{stamp}.json"));
    let mut n = 1;
    while broken.exists() {
        n += 1;
        broken = dir.join(format!("{stamp}-{n}.json"));
    }
    let renamed_to = match fs::rename(autosave, &broken) {
        Ok(()) => Some(broken),
        Err(_) => None,
    };
    let backup = backup_path(autosave);
    let backup = if backup.exists() {
        Project::load(&backup).ok().map(Box::new)
    } else {
        None
    };
    AutosaveOutcome::Damaged { renamed_to, backup }
}

/// `restore_window` 的窗口读取：先读 autosave，读不开（OSError / JSON 坏）才读备份。
pub fn read_autosave_window(autosave: &Path) -> Option<WindowState> {
    if let Some(win) = read_window_file(autosave) {
        return win;
    }
    read_window_file(&backup_path(autosave)).flatten()
}

/// Some(win) = 文件读开（win 可能是 None，表示没有 window 键）；None = 读不开。
fn read_window_file(path: &Path) -> Option<Option<WindowState>> {
    let text = fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let win = data.get("window").filter(|v| !v.is_null());
    Some(win.map(|v| WindowState::from_json(Some(v))))
}
