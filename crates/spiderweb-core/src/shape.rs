//! Data model: shapes (Shape) and their per-kind settings, matching the Python shape dictionary.

use std::collections::BTreeMap;

use crate::Pt;

/// Shape kind (Python's sh["kind"]).
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

/// Custom shape fill mode (custom.py FILLS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    Empty,
    Fill,
    Spam,
    OutlineSpam,
}

/// Spam start alignment (custom.py ALIGNS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Auto,
    Aligned,
    /// Split the leftover that can't fit a whole gate between the two ends (custom.py's "centred").
    Centred,
}

/// Spam ending (custom.py ENDS): what to do with the leftover that can't fit a whole gate in a segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ends {
    /// Half a gate or more counts as a whole gate, otherwise it is dropped (at least one gate per segment).
    Round,
    /// Keep the leftover as a short note.
    Keep,
    /// Drop it; a segment shorter than one gate keeps one note as-is (old shapes without ends read as this).
    #[default]
    Drop,
    /// Same as drop, but notes are never shorter than a quarter gate (grown from the centre if needed).
    Min,
    /// Stretch / squeeze the gates in a segment to fit a whole number of them.
    Stretch,
}

/// How a curve is made symmetric (bezier.py Symmetry).
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
        /// Drawn freehand (can be "Straightened")
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

/// Tumour shape (tumour.py SHAPES).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TumourShape {
    #[default]
    Triangle,
    Square,
    Circle,
    Parabola,
}

/// Tumour side (tumour.py SIDES).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TumourSide {
    #[default]
    Alt,
    Left,
    Right,
    Random,
}

/// How a tumour follows the line (tumour.py WRAPS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TumourWrap {
    #[default]
    Simple,
    Wrap,
}

/// Tumour settings (sh["tumour"]), defaults same as tumour.TUMOUR_DEFAULTS.
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
    /// Each bump tilts by this many degrees, its feet staying on the line
    /// (1.2.0 tumour.py "rot", positive leaning forward).
    pub rot: f64,
    /// The square's top is narrowed by this much of its length (1.2.0 tumour.py "slant";
    /// -1..1, square only).
    pub slant: f64,
    /// Graphs that make a setting change along the line: setting name -> [[u, f], ...]
    /// (1.2.0 tumour.py "graphs"; only keys in [`crate::tumour::GRAPH_KEYS`]).
    pub graphs: BTreeMap<String, Vec<Pt>>,
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
            rot: 0.0,
            slant: 0.0,
            graphs: BTreeMap::new(),
            fit: false,
            seed: 1,
            mirror: false,
            k: 0.25,
        }
    }
}

/// Text settings (text.py TEXT_DEFAULTS + bbox/cap/k/holes).
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
    /// Values from text.TEXT_DEFAULTS (bbox uses project.py's `[0, 0, 1, 1]` default for text
    /// settings, cap / k use the `or 0.7` / `or 1.0` fallbacks).
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

/// A curve inside the funnel (the product of funnel.py clean_curve).
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelCurve {
    pub pts: Vec<Pt>,
    pub sharp: Vec<usize>,
    pub link: Option<i64>,
    pub flip: bool,
}

/// One funnel start (an element of funnel.py "starts").
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelStart {
    pub line: usize,
    pub at: f64,
    pub ends: [Option<FunnelCurve>; 2],
}

/// Funnel fill mode (funnel.py FUNNEL_FILLS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FunnelFill {
    #[default]
    Spam,
    Long,
}

/// Gate change mode (funnel.py GATE_CHANGES).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GateChange {
    #[default]
    Steps,
    Smooth,
}

/// What the gate follows (funnel.py GATE_FOLLOWS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GateFollow {
    #[default]
    Time,
    Curve,
}

/// Wall mode (funnel.py WALL_MODES).
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

/// A shape. Fields correspond one-to-one with the Python shape dictionary (per-kind fields coexist and apply according to kind).
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub kind: Kind,
    /// Shape points: (beat, pitch). Funnel: [line start, line end, wall 1, wall 2, (extra lines...)]
    pub pts: Vec<Pt>,
    pub vel0: f64,
    pub vel1: f64,
    /// Velocity envelope; empty = straight line vel0 -> vel1.
    pub vel_env: Vec<Pt>,
    pub end_dot: bool,
    pub tumour: Option<Tumour>,
    /// Freehand "Straighten" sensitivity 0..100; 0 = keep as drawn.
    pub smooth: i64,
    /// On-screen scale for arcs / Straighten: how many beats per key.
    pub k: f64,
    /// Sharp corners among the curve anchors (anchor indices).
    pub sharp: Vec<usize>,
    pub sym: Option<Sym>,
    /// Segments of a joined curve that aren't drawn and make no notes
    /// (1.2.0 joined.py `sh["gaps"]`; convert.py splits a curve into one stroke per piece here).
    pub gaps: Vec<usize>,
    /// Anchors where a new tumour section starts inside a piece of a joined curve
    /// (1.2.0 joined.py `sh["splits"]`).
    pub splits: Vec<usize>,
    /// Per-piece tumour settings of a joined curve (None = that piece has none)
    /// (1.2.0 joined.py `sh["tumours"]`).
    pub tumours: Vec<Option<Tumour>>,
    // ---- custom ----
    pub name: String,
    pub strokes: Vec<Stroke>,
    pub fill: Fill,
    pub gate: f64,
    pub align: Align,
    /// Spam ending (new in 1.2.0; old shapes read as drop).
    pub ends: Ends,
    /// Fill overlapping outline areas too (otherwise overlaps cancel out, even-odd).
    pub union: bool,
    /// Fill / Spam "Outline": the outline's notes go on a separate channel (new in 1.2.0).
    pub apart: bool,
    pub text: Option<TextSettings>,
    /// Pasted notes (the text of pack_notes).
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
            splits: Vec::new(),
            tumours: Vec::new(),
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
    /// Create a shape (a simplified version of engine.make_shape: no per-kind initialisation, the caller handles it).
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

    /// Velocity envelope (envelope.velocity_env).
    pub fn velocity_env(&self) -> Vec<Pt> {
        if self.vel_env.is_empty() {
            vec![[0.0, self.vel0], [1.0, self.vel1]]
        } else {
            self.vel_env.clone()
        }
    }
}
