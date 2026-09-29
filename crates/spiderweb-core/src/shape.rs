//! 数据模型：形状（Shape）及其各类型的专属设置，对应 Python 版形状字典。

use crate::Pt;

/// 形状种类（Python 的 sh["kind"]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Line,
    Poly,
    Free,
    Curve,
    Arc,
    Custom,
    Funnel,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Line => "line",
            Kind::Poly => "poly",
            Kind::Free => "free",
            Kind::Curve => "curve",
            Kind::Arc => "arc",
            Kind::Custom => "custom",
            Kind::Funnel => "funnel",
        }
    }
}

/// 自定义形状的填充方式（custom.py FILLS）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    Empty,
    Fill,
    Spam,
    OutlineSpam,
}

/// Spam 起点对齐（custom.py ALIGNS）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Auto,
    Aligned,
    /// 放不下整门限的余量分给两端（custom.py 的 "centred"）。
    Centred,
}

/// Spam 收尾（custom.py ENDS）：一段里放不下整门限的零头怎么处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ends {
    /// 够半个门限算一个整门限，否则丢掉（每段至少一个门限）。
    Round,
    /// 零头留成短线。
    Keep,
    /// 丢掉；短于一个门限的段原样留一个音符（没有 ends 的旧形状按这个读）。
    #[default]
    Drop,
    /// 同 drop，但音符不短于四分之一门限（不够就居中长出来）。
    Min,
    /// 段里的门限拉伸 / 压缩，正好放下整数个。
    Stretch,
}

/// 对称曲线的方式（bezier.py Symmetry）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sym {
    Mirror,
    Turn,
}

/// A custom shape's stroke (custom.py's stroke dictionary).
///
/// `src` is which shape the stroke came from (convert.py's "Turn into live shape"): strokes from
/// one shape share a number; None = no `src` (grouped as -1, like Python's missing key).
#[derive(Clone, Debug, PartialEq)]
pub enum Stroke {
    Poly {
        pts: Vec<Pt>,
        /// 自由笔画的（可"画整齐"）
        free: bool,
        smooth: i64,
        k: f64,
        src: Option<i64>,
    },
    Curve {
        pts: Vec<Pt>,
        sharp: Vec<usize>,
        sym: Option<Sym>,
        src: Option<i64>,
    },
    Arc {
        pts: Vec<Pt>,
        k: f64,
        src: Option<i64>,
    },
    Ellipse {
        box_: [f64; 4],
        src: Option<i64>,
    },
}

impl Stroke {
    /// Which shape the stroke came from (convert.py's `st.get("src")`); None = no `src`.
    pub fn src(&self) -> Option<i64> {
        match self {
            Stroke::Poly { src, .. }
            | Stroke::Curve { src, .. }
            | Stroke::Arc { src, .. }
            | Stroke::Ellipse { src, .. } => *src,
        }
    }

    /// Set [`Stroke::src`].
    pub fn set_src(&mut self, value: Option<i64>) {
        match self {
            Stroke::Poly { src, .. }
            | Stroke::Curve { src, .. }
            | Stroke::Arc { src, .. }
            | Stroke::Ellipse { src, .. } => *src = value,
        }
    }
}

/// 肿瘤形状（tumour.py SHAPES）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TumourShape {
    #[default]
    Triangle,
    Square,
    Circle,
    Parabola,
}

/// 肿瘤方向（tumour.py SIDES）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TumourSide {
    #[default]
    Alt,
    Left,
    Right,
    Random,
}

/// 肿瘤跟随线条的方式（tumour.py WRAPS）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TumourWrap {
    #[default]
    Simple,
    Wrap,
}

/// 肿瘤设置（sh["tumour"]），默认值同 tumour.TUMOUR_DEFAULTS。
#[derive(Clone, Debug, PartialEq)]
pub struct Tumour {
    pub on: bool,
    pub shape: TumourShape,
    pub size: f64,
    pub length: f64,
    pub dist: f64,
    pub side: TumourSide,
    pub wrap: TumourWrap,
    pub start: f64,
    pub end: f64,
    pub ease: f64,
    pub fit: bool,
    pub seed: i64,
    pub mirror: bool,
    pub k: f64,
}

impl Default for Tumour {
    fn default() -> Self {
        Self {
            on: true,
            shape: TumourShape::Triangle,
            size: 3.0,
            length: 0.125,
            dist: 0.125,
            side: TumourSide::Alt,
            wrap: TumourWrap::Simple,
            start: 0.0,
            end: 1.0,
            ease: 0.0,
            fit: false,
            seed: 1,
            mirror: false,
            k: 0.25,
        }
    }
}

/// 文本设置（text.py TEXT_DEFAULTS + bbox/cap/k/holes）。
#[derive(Clone, Debug, PartialEq)]
pub struct TextSettings {
    pub text: String,
    pub font: String,
    pub size: f64,
    pub unit: TextUnit,
    pub weight: i32,
    pub italic: bool,
    pub tracking: f64,
    pub leading: f64,
    pub align: TextAlign,
    pub threshold: f64,
    pub grow: f64,
    pub bbox: [f64; 4],
    pub cap: f64,
    pub k: f64,
    pub holes: Vec<usize>,
}

impl Default for TextSettings {
    /// text.TEXT_DEFAULTS 的值（bbox 用 project.py 对文本默认设置的 `[0, 0, 1, 1]`，
    /// cap / k 用 `or 0.7` / `or 1.0` 的兜底值）。
    fn default() -> Self {
        Self {
            text: String::new(),
            font: "Arial".to_string(),
            size: 24.0,
            unit: TextUnit::Font,
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 100.0,
            align: TextAlign::Left,
            threshold: 50.0,
            grow: 0.0,
            bbox: [0.0, 0.0, 1.0, 1.0],
            cap: 0.7,
            k: 1.0,
            holes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextUnit {
    #[default]
    Font,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 漏斗里一条曲线（funnel.py clean_curve 的产物）。
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelCurve {
    pub pts: Vec<Pt>,
    pub sharp: Vec<usize>,
    pub link: Option<i64>,
    pub flip: bool,
}

/// 漏斗的一个起点（funnel.py "starts" 元素）。
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelStart {
    pub line: usize,
    pub at: f64,
    pub ends: [Option<FunnelCurve>; 2],
}

/// 漏斗填充方式（funnel.py FUNNEL_FILLS）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FunnelFill {
    #[default]
    Spam,
    Long,
}

/// 门限变化方式（funnel.py GATE_CHANGES）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GateChange {
    #[default]
    Steps,
    Smooth,
}

/// 门限跟随对象（funnel.py GATE_FOLLOWS）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GateFollow {
    #[default]
    Time,
    Curve,
}

/// 墙模式（funnel.py WALL_MODES）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WallMode {
    #[default]
    In,
    Past,
}

/// The shapes a live shape was made of (convert.py's `sh["from"]`): the originals, plus the new
/// shape's strokes and box frame right after the conversion. "Split into separate shapes" uses it
/// to give the originals back, as long as the drawing wasn't changed (moving the whole shape is ok).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeFrom {
    pub shapes: Vec<Shape>,
    pub strokes: Vec<Stroke>,
    pub pts: Vec<Pt>,
}

/// 一个形状。字段与 Python 形状字典一一对应（各类型专属字段并存，按 kind 生效）。
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub kind: Kind,
    /// 形状的点：(beat, pitch)。漏斗：[线起点, 线终点, 墙1, 墙2, (额外线...)]
    pub pts: Vec<Pt>,
    pub vel0: f64,
    pub vel1: f64,
    /// 速度包络；空 = vel0 → vel1 直线。
    pub vel_env: Vec<Pt>,
    pub end_dot: bool,
    pub tumour: Option<Tumour>,
    /// 自由笔"画整齐"灵敏度 0..100；0 = 保持原样。
    pub smooth: i64,
    /// 弧 / 画整齐时的屏幕比例：几个 beat 对应一个 key。
    pub k: f64,
    /// 曲线锚点中的尖角（anchor 序号）。
    pub sharp: Vec<usize>,
    pub sym: Option<Sym>,
    /// Anchor numbers where a joined curve has a gap (joined.py's `gaps`; convert.py splits a
    /// curve into one stroke per piece here). Empty for a normal curve.
    pub gaps: Vec<i64>,
    // ---- custom ----
    pub name: String,
    pub strokes: Vec<Stroke>,
    pub fill: Fill,
    pub gate: f64,
    pub align: Align,
    /// spam 收尾（1.2.0 新增；旧形状读作 drop）。
    pub ends: Ends,
    /// 轮廓重叠处也填上（否则重叠互相抵消，even-odd）。
    pub union: bool,
    /// Fill / Spam "Outline"：轮廓的音符单独一条通道（1.2.0 新增）。
    pub apart: bool,
    pub text: Option<TextSettings>,
    /// 粘贴的音符（pack_notes 的文本）。
    pub notes: Option<String>,
    pub own_vel: bool,
    /// Which shapes this live shape was made of (convert.py's `from`; new in 1.2.0).
    pub from: Option<ShapeFrom>,
    // ---- funnel ----
    pub starts: Vec<FunnelStart>,
    pub funnel_fill: FunnelFill,
    pub gate0: f64,
    pub gate1: f64,
    pub vary: bool,
    pub change: GateChange,
    pub follow: GateFollow,
    pub wall: WallMode,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            kind: Kind::Line,
            pts: Vec::new(),
            vel0: 127.0,
            vel1: 127.0,
            vel_env: Vec::new(),
            end_dot: false,
            tumour: None,
            smooth: 0,
            k: 1.0,
            sharp: Vec::new(),
            sym: None,
            gaps: Vec::new(),
            name: String::new(),
            strokes: Vec::new(),
            fill: Fill::Empty,
            gate: 0.0625,
            align: Align::Auto,
            ends: Ends::Drop,
            union: false,
            apart: false,
            text: None,
            notes: None,
            own_vel: false,
            from: None,
            starts: Vec::new(),
            funnel_fill: FunnelFill::Spam,
            gate0: 0.0625,
            gate1: 0.0625,
            vary: false,
            change: GateChange::Steps,
            follow: GateFollow::Time,
            wall: WallMode::In,
        }
    }
}

impl Shape {
    /// 新建一个形状（对应 engine.make_shape 的简化：不做种类专属初始化，由调用方负责）。
    pub fn new(kind: Kind, pts: Vec<Pt>) -> Self {
        let mut sh = Self {
            kind,
            pts,
            ..Self::default()
        };
        if kind == Kind::Funnel {
            sh.starts = Vec::new();
        }
        sh
    }

    /// 速度包络（envelope.velocity_env）。
    pub fn velocity_env(&self) -> Vec<Pt> {
        if self.vel_env.is_empty() {
            vec![[0.0, self.vel0], [1.0, self.vel1]]
        } else {
            self.vel_env.clone()
        }
    }
}
