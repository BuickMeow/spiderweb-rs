//! Project files, autosave and autosave backups (port of Python `files/project.py`).
//!
//! - [`Project`]: all settings and shapes of one project; [`Project::from_json`] /
//!   [`Project::to_json`] correspond to `ProjectFiles.load_file` / `project_data`, and
//!   [`Project::load`] / [`Project::write`] go through [`safefile`]'s atomic write.
//! - [`project_json`] / [`Project::to_project_json`]: Python's "one line per setting, one
//!   line per shape" JSON layout (`short_shape`: floats lose noisy digits, `vel_env`
//!   velocities are rounded to 4 decimals, etc.).
//! - [`load_autosave`] / [`backup_path`]: when autosave cannot be opened, rename it for the
//!   record and fall back to the backup from the last launch.
//!
//! Differences from the original: `str()` edges for strings / numbers (containers, numbers
//! with underscores) are only approximated; the autosave timestamp is passed in by the
//! caller (`stamp`), so there is no dependency on local time or timezone and tests are
//! deterministic.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use spiderweb_core::shape::{
    Align, Ends, Fill, FunnelFill, GateChange, GateFollow, Shape, TextSettings, WallMode,
};
use spiderweb_core::smooth::{SMOOTH_DEFAULT, clean_level};
use spiderweb_core::text::clean_text;
use spiderweb_domino::DominoStart;

use crate::compat::{
    SHAPE_DEFAULTS, ShapeDefaults, ShapeError, align_str, ends_str, fill_str, py_bool, py_float,
    py_int, py_str, shape_from_json, shape_to_json, short_num_f,
};
use crate::safefile;
use crate::snap::clean_snap;

/// This program's version (Python `files/about.VERSION`).
pub const VERSION: &str = "1.1.0";
/// Project file format version (hardcoded 2 in `project_data`).
pub const PROJECT_VERSION: i64 = 2;
/// Channel modes (engine.CHANNEL_MODES).
pub const CHANNEL_MODES: [&str; 3] = ["raw", "single", "auto"];
/// How overlaps are decided with multiple channels (engine.SPLITS).
pub const SPLITS: [&str; 2] = ["key", "time"];

/// Channel mode.
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

/// What an overlap is counted by (key or time).
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

/// Default settings for a new custom shape (custom.CUSTOM_DEFAULTS + the figure picked from the library).
#[derive(Clone, Debug, PartialEq)]
pub struct CustomDefaults {
    pub fill: Fill,
    pub gate: f64,
    pub align: Align,
    pub ends: Ends,
    pub union: bool,
    pub apart: bool,
    pub shape: String,
}

impl Default for CustomDefaults {
    fn default() -> Self {
        Self {
            fill: Fill::Empty,
            gate: 0.0625,
            align: Align::Auto,
            ends: Ends::Round,
            union: false,
            apart: false,
            shape: "Circle".to_string(),
        }
    }
}

/// Default settings for a new funnel (funnel.FUNNEL_DEFAULTS).
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

/// Zoom and scroll position of the roll (pianoroll.view_state).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub t: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
}

impl ViewState {
    /// `pianoroll.set_view`: valid only if all four numbers convert to float.
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

/// Window state recorded in autosave (the "window" of `write_json(window=True)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowState {
    pub geometry: String,
    pub maximized: bool,
    pub velocity: bool,
    pub velocity_height: Option<f64>,
    pub midi_device: String,
    pub live: bool,
    /// Remaining keys such as tips (kept as-is).
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

/// Project file read error; in Python `load_file` returns False on error.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("cannot read project file: {0}")]
    Io(#[from] io::Error),
    #[error("project file is not JSON")]
    Json(#[from] serde_json::Error),
    #[error("project file is not a JSON object")]
    NotObject,
    #[error("project defaults / custom_defaults are bad")]
    Defaults,
    #[error("project shapes are bad")]
    Shapes,
    #[error("bad shape in project: {0}")]
    Shape(#[from] ShapeError),
}

/// Key order in Python `project_data`.
const PROJECT_KEYS: [&str; 19] = [
    "version",
    "app_version",
    "ppq",
    "bpm",
    "beats",
    "output",
    "channel_mode",
    "channel_split",
    "keys",
    "domino_start",
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

/// One project (the project-related state of the Python App).
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// Project file format version (written as 2).
    pub version: i64,
    /// The Spiderweb version that saved it.
    pub app_version: String,
    /// PPQ text field (number or expression, evaluated by [`Project::read_project`]).
    pub ppq: String,
    /// BPM text field.
    pub bpm: String,
    /// Beats-per-bar text field.
    pub beats: String,
    /// MIDI output path.
    pub output: String,
    pub channel_mode: ChannelMode,
    pub channel_split: ChannelSplit,
    /// The project's key range: 128 (0-127) or 256 (0-255) (1.2.0's `keys`; any other value is 128).
    pub keys: i64,
    /// Where copying / pasting to Domino starts (1.2.0's `domino_start`; missing files get
    /// `"note"`, the dropdown's initial value).
    pub domino_start: DominoStart,
    /// The snap text ([`crate::snap`]'s spelling; old values are migrated by [`clean_snap`]
    /// when a file is read).
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
            keys: spiderweb_core::paths::KEYS[0],
            domino_start: DominoStart::Note,
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

/// `read_project`: evaluate PPQ / BPM / beats; error messages match the original.
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
    /// Evaluate PPQ / BPM / beats (`read_project`).
    pub fn read_project(&self) -> Result<(i64, f64, i64), crate::mathexpr::MathError> {
        read_project(&self.ppq, &self.bpm, &self.beats)
    }

    /// Corresponds to the data part of `load_file` (UI state such as window / undo stack is not in the io layer).
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
        // Original: self.keys_var.set(str(KEYS[1] if data.get("keys") == KEYS[1] else KEYS[0]))
        p.keys = match d.get("keys") {
            Some(Value::Number(n)) if n.as_f64() == Some(spiderweb_core::paths::KEYS[1] as f64) => {
                spiderweb_core::paths::KEYS[1]
            }
            _ => spiderweb_core::paths::KEYS[0],
        };
        // Upstream: `if "snap" in data: self.snap.set(clean_snap(data["snap"]))` (the old
        // 1/64, 1/128 and Off migrate; unknown values fall back to the default 1/16)
        if let Some(s) = d.get("snap").and_then(Value::as_str) {
            p.snap = clean_snap(s);
        }
        // Upstream only moves the dropdown for a value it knows; missing / bad values keep
        // the initial "note"
        if let Some(ds) = d
            .get("domino_start")
            .and_then(Value::as_str)
            .and_then(DominoStart::parse)
        {
            p.domino_start = ds;
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

    /// The dictionary corresponding to `project_data`.
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
        d.insert("keys".into(), Value::from(self.keys));
        d.insert(
            "domino_start".into(),
            Value::from(self.domino_start.as_str()),
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
            // New keys are only written when not at their defaults (1.2.0 uses round / false /
            // false when it cannot read ends / union / apart, so omitting defaults is still
            // read correctly by 1.2.0, and older project files gain no extra keys)
            custom_defaults_json(&self.custom_defaults),
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

    /// Python `project_json(data)`'s layout: one line per setting, one line per shape; `window` is appended at the end.
    pub fn to_project_json_with_window(&self, window: Option<&WindowState>) -> String {
        let data = self.to_json();
        let mut extra: Vec<(String, Value)> = Vec::new();
        if let Some(w) = window {
            extra.push(("window".to_string(), w.to_json()));
        }
        project_json_extra(&data, &extra)
    }

    /// [`Project::to_project_json_with_window`] without the window.
    pub fn to_project_json(&self) -> String {
        self.to_project_json_with_window(None)
    }

    /// Save atomically (Python `write_json`).
    pub fn write(&self, path: &Path, window: Option<&WindowState>) -> io::Result<()> {
        safefile::write_text(path, &self.to_project_json_with_window(window))
    }

    /// Read one project file (the file part of Python `load_file`).
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
    // The original does type(SHAPE_DEFAULTS[k])(v): vel0 / vel1 in SHAPE_DEFAULTS are ints, so they truncate
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

/// The `custom_defaults` JSON (upstream `dict(self.custom_defaults, shape=...)`: every key
/// is written).
fn custom_defaults_json(cd: &CustomDefaults) -> Value {
    let mut o = Map::new();
    o.insert("fill".into(), Value::from(fill_str(cd.fill)));
    o.insert("gate".into(), Value::from(cd.gate));
    o.insert("align".into(), Value::from(align_str(cd.align)));
    o.insert("ends".into(), Value::from(ends_str(cd.ends)));
    o.insert("union".into(), Value::Bool(cd.union));
    o.insert("apart".into(), Value::Bool(cd.apart));
    o.insert("shape".into(), Value::from(cd.shape.clone()));
    Value::Object(o)
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
        Some("centred") => out.align = Align::Centred,
        _ => {}
    }
    // Original: `if custom.get("ends") in ENDS` — unknown values keep the old default (round)
    out.ends = match d.get("ends").and_then(Value::as_str) {
        Some("round") => Ends::Round,
        Some("keep") => Ends::Keep,
        Some("drop") => Ends::Drop,
        Some("min") => Ends::Min,
        Some("stretch") => Ends::Stretch,
        _ => out.ends,
    };
    // bool(custom.get(key)): missing / 0 / empty string all count as false
    out.union = d.get("union").is_some_and(py_bool);
    out.apart = d.get("apart").is_some_and(py_bool);
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

/// The defaults path of `clean_funnel`; a bad gate returns None (the original keeps the old defaults after the try).
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

/// text_defaults saves only those keys of TEXT_DEFAULTS.
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

// ---------------------------------------------------------------- Python-style JSON

/// Python `json.dumps`-compatible serialisation (ensure_ascii, ", " / ": " separators, floats via repr).
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

/// Python `repr(float)` (numbers in JSON; inf / nan are written as Infinity / NaN).
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

/// Python `short_tumour`: a tumour's own numbers rounded; the `graphs` setting is a
/// point list per setting name, so each of its points is rounded too.
fn short_tumour(v: &Value) -> Value {
    let Some(d) = v.as_object() else {
        return v.clone();
    };
    let mut o = Map::new();
    for (k, b) in d {
        let nv = if k == "graphs" {
            match b.as_object() {
                Some(g) => Value::Object(
                    g.iter()
                        .map(|(a, pts)| (a.clone(), short_pts(pts)))
                        .collect(),
                ),
                None => b.clone(),
            }
        } else {
            short_num12(b)
        };
        o.insert(k.clone(), nv);
    }
    Value::Object(o)
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

/// Python `short_shape`: round the numbers in a shape dict that should be rounded (12 significant digits).
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
            "tumour" => short_tumour(v),
            "tumours" => Value::Array(v.as_array().map_or_else(Vec::new, |a| {
                a.iter()
                    .map(|t| {
                        if py_bool(t) {
                            short_tumour(t)
                        } else {
                            Value::Null
                        }
                    })
                    .collect()
            })),
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

/// Python `project_json(data)` (`data` is the project dict; order is decided by the dict itself, serde_json sorts by key by default).
pub fn project_json(data: &Value) -> String {
    project_json_extra(data, &[])
}

/// [`project_json`] plus the `window` Python `write_json` appends at the end.
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

/// autosave.json -> autosave-backup.json (a normal project file, Open project can open it too).
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

/// Result of `load_autosave`.
#[derive(Debug)]
pub enum AutosaveOutcome {
    /// No autosave file.
    Missing,
    /// autosave opened normally (and was copied to the backup).
    Opened(Box<Project>),
    /// autosave could not be opened: renamed to `renamed_to` (None on failure), falls back to the backup (None if absent).
    Damaged {
        renamed_to: Option<PathBuf>,
        backup: Option<Box<Project>>,
    },
}

/// Open autosave at startup (Python `load_autosave`): if it opens, copy it to the backup;
/// if not, rename it for the record and try the backup. `stamp` is the timestamp text (the
/// original's `time.strftime("autosave-broken-%Y%m%d-%H%M%S")`), supplied by the caller;
/// on name collisions `-2`, `-3`, ... are appended in turn.
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

/// Window reading of `restore_window`: read autosave first, and only read the backup when it cannot be opened (OSError / bad JSON).
pub fn read_autosave_window(autosave: &Path) -> Option<WindowState> {
    if let Some(win) = read_window_file(autosave) {
        return win;
    }
    read_window_file(&backup_path(autosave)).flatten()
}

/// Some(win) = the file opened (win may be None, meaning there is no window key); None = could not be opened.
fn read_window_file(path: &Path) -> Option<Option<WindowState>> {
    let text = fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let win = data.get("window").filter(|v| !v.is_null());
    Some(win.map(|v| WindowState::from_json(Some(v))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 1.2.0 project custom_defaults new keys can be read (unknown ends keeps round; switches follow bool()).
    #[test]
    fn custom_defaults_new_keys_read() {
        let d = json!({"fill": "spam", "gate": 60, "align": "centred", "ends": "keep",
                       "union": true, "apart": 1});
        let m = d.as_object().cloned().unwrap_or_default();
        let cd = custom_defaults_from_json(&m);
        assert_eq!(cd.ends, Ends::Keep);
        assert!(cd.union && cd.apart);
        let bogus = json!({"ends": "nonsense"});
        let m = bogus.as_object().cloned().unwrap_or_default();
        assert_eq!(custom_defaults_from_json(&m).ends, Ends::Round);
    }

    /// Writing: every key comes out, like upstream `dict(self.custom_defaults, shape=...)`.
    #[test]
    fn custom_defaults_new_keys_write() {
        let mut cd = CustomDefaults::default();
        assert_eq!(
            custom_defaults_json(&cd),
            json!({"fill": "empty", "gate": 0.0625, "align": "auto", "ends": "round",
                    "union": false, "apart": false, "shape": "Circle"})
        );
        cd.ends = Ends::Stretch;
        cd.union = true;
        cd.apart = true;
        let v = custom_defaults_json(&cd);
        assert_eq!(v["ends"], json!("stretch"));
        assert_eq!(v["union"], json!(true));
        assert_eq!(v["apart"], json!(true));
    }

    /// 1.2.0's snap text and domino_start: migrated / validated when read, saved as-is.
    #[test]
    fn snap_and_domino_start_round_trip() {
        let p = Project::from_json(&json!({"snap": "1/64", "domino_start": "bar"}))
            .expect("read project");
        assert_eq!(p.snap, "c:/64/1");
        assert_eq!(p.domino_start, DominoStart::Bar);
        assert_eq!(p.to_json()["domino_start"], json!("bar"));
        assert_eq!(p.to_json()["snap"], json!("c:/64/1"));
        // missing / bad domino_start values keep the initial "note"
        for data in [
            json!({}),
            json!({"domino_start": "nope"}),
            json!({"domino_start": 5}),
        ] {
            let p = Project::from_json(&data).expect("read project");
            assert_eq!(p.domino_start, DominoStart::Note);
        }
        // 1.1.0's Off / 1/128 migrate too
        assert_eq!(
            Project::from_json(&json!({"snap": "Off"}))
                .expect("read project")
                .snap,
            "off"
        );
        assert_eq!(
            Project::from_json(&json!({"snap": "1/128"}))
                .expect("read project")
                .snap,
            "c:/128/1"
        );
    }
}
