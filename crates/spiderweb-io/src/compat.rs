//! 形状 ⇄ JSON：Python 工程文件里的形状字典与 Rust [`Shape`] 的互转。
//!
//! - [`shape_from_json`] 对应 `notes/engine.clean_shape`：从文件里读出的形状字典 -> 有效形状，
//!   不支持的种类返回 `Ok(None)`，Python 里会抛异常（整个工程读不开）的坏字段返回 `Err`。
//! - [`shape_to_json`] 是逆转换，输出 Python `clean_shape` 会得到的形状字典（键的取舍与省略规则
//!   相同：curve 的 `sharp` 空则省略、`sym` 没有则省略，折线笔画的 `free`/`smooth`/`k` 只在
//!   free 时出现，等等）。
//!
//! 与 Python 的差异（都写在对应函数上）：漏斗的老版本 `bends` 列表曲线用默认曲线代替
//! （Python 会拟合），`funnel.rs` 也还没移植，所以这里只做加载所需的清洗。

use serde_json::{Map, Value};

use spiderweb_core::arc::clean_k as arc_k;
use spiderweb_core::custom::{BOX_STROKE, check_notes, clean_curve, clean_strokes};
use spiderweb_core::joined;
use spiderweb_core::shape::{
    Align, Ends, Fill, FunnelCurve, FunnelFill, FunnelStart, GateChange, GateFollow, Kind, Shape,
    ShapeFrom, Stroke, Sym, TextSettings, Tumour, WallMode,
};
use spiderweb_core::smooth::{SMOOTH_DEFAULT, clean_level};
use spiderweb_core::text::clean_text;
use spiderweb_core::tumour::clean_tumour;
use spiderweb_core::{Pt, round_half_even};

/// 新形状的默认速度 / 尾点（engine.SHAPE_DEFAULTS）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeDefaults {
    pub vel0: f64,
    pub vel1: f64,
    pub end_dot: bool,
}

impl Default for ShapeDefaults {
    fn default() -> Self {
        SHAPE_DEFAULTS
    }
}

/// SHAPE_DEFAULTS 的值。
pub const SHAPE_DEFAULTS: ShapeDefaults = ShapeDefaults {
    vel0: 127.0,
    vel1: 127.0,
    end_dot: false,
};

/// 形状种子的四种填充方式（custom.FILLS）。
pub const FILLS: [Fill; 4] = [Fill::Empty, Fill::Fill, Fill::Spam, Fill::OutlineSpam];
/// 种子的 spam 起点（custom.ALIGNS）。
pub const ALIGNS: [Align; 3] = [Align::Auto, Align::Aligned, Align::Centred];
/// 能长肿瘤的形状种类（tumour.LINE_KINDS）。
pub const LINE_KINDS: [Kind; 5] = [Kind::Line, Kind::Poly, Kind::Free, Kind::Curve, Kind::Arc];

/// 形状字典的坏字段；Python 里这些会让 `load_file` 失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShapeError {
    #[error("形状不是字典")]
    NotObject,
    #[error("形状的 pts 坏了")]
    Pts,
    #[error("形状的 vel_env 坏了")]
    VelEnv,
    #[error("形状的 vel0 / vel1 坏了")]
    Vel,
    #[error("自定义形状的 gate 坏了")]
    Gate,
    #[error("形状的 strokes 坏了")]
    Strokes,
    #[error("形状的 notes 坏了")]
    Notes,
}

// ---------------------------------------------------------------- Python 值转换

/// Python `float(x)`：数字、布尔、数字字符串；别的失败。
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Python `int(x)`：数字截断、布尔与整数字符串；别的失败。
pub fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok()
            } else {
                n.as_f64().map(|f| f.trunc() as i64)
            }
        }
        Value::Bool(b) => Some(i64::from(*b)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Python 真值判断。
pub fn py_bool(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `str(x)`（容器按 repr 的近似写法；形状里用到的都是标量）。
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Null => "None".to_string(),
        Value::Array(a) => {
            let items: Vec<String> = a
                .iter()
                .map(|x| match x {
                    Value::String(s) => {
                        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
                    }
                    other => py_str(other),
                })
                .collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, x)| {
                    let key = k.replace('\\', "\\\\").replace('\'', "\\'");
                    match x {
                        Value::String(s) => format!("'{}': '{}'", key, s),
                        other => format!("'{}': {}", key, py_str(other)),
                    }
                })
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// 数字（f64）-> JSON 数字；整数值写成整数（Python `short_num` 的 12 位有效数字版）。
pub fn short_num_f(x: f64, digits: u32) -> Value {
    if !x.is_finite() {
        return Value::from(x);
    }
    let rounded: f64 = if x == 0.0 {
        0.0
    } else {
        let s = format!("{:.*e}", digits.saturating_sub(1) as usize, x);
        s.parse().unwrap_or(x)
    };
    num_value(rounded)
}

/// f64 -> JSON：整数写成整数，否则写浮点（保持 -0.0）。
pub(crate) fn num_value(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x >= i64::MIN as f64 && x <= i64::MAX as f64 {
        if x == 0.0 && x.is_sign_negative() {
            return Value::from(-0.0);
        }
        return Value::from(x as i64);
    }
    Value::from(x)
}

// ---------------------------------------------------------------- 读（clean_shape）

/// 形状种类名 -> [`Kind`]。
pub fn kind_from_str(s: &str) -> Option<Kind> {
    Some(match s {
        "line" => Kind::Line,
        "poly" => Kind::Poly,
        "free" => Kind::Free,
        "curve" => Kind::Curve,
        "arc" => Kind::Arc,
        "custom" => Kind::Custom,
        "funnel" => Kind::Funnel,
        _ => return None,
    })
}

fn pt_of(v: &Value) -> Result<Pt, ShapeError> {
    let a = v.as_array().ok_or(ShapeError::Pts)?;
    if a.len() != 2 {
        return Err(ShapeError::Pts);
    }
    Ok([
        py_float(&a[0]).ok_or(ShapeError::Pts)?,
        py_float(&a[1]).ok_or(ShapeError::Pts)?,
    ])
}

fn pts_of(v: &Value) -> Result<Vec<Pt>, ShapeError> {
    let a = v.as_array().ok_or(ShapeError::Pts)?;
    a.iter().map(pt_of).collect()
}

fn fill_from_str(s: &str) -> Fill {
    match s {
        "fill" => Fill::Fill,
        "spam" => Fill::Spam,
        "outline_spam" => Fill::OutlineSpam,
        _ => Fill::Empty,
    }
}

/// fill -> Python 名。
pub fn fill_str(f: Fill) -> &'static str {
    match f {
        Fill::Empty => "empty",
        Fill::Fill => "fill",
        Fill::Spam => "spam",
        Fill::OutlineSpam => "outline_spam",
    }
}

/// align -> Python 名。
pub fn align_str(a: Align) -> &'static str {
    match a {
        Align::Auto => "auto",
        Align::Aligned => "aligned",
        Align::Centred => "centred",
    }
}

/// ends -> Python 名（custom.ENDS）。
pub fn ends_str(e: Ends) -> &'static str {
    match e {
        Ends::Round => "round",
        Ends::Keep => "keep",
        Ends::Drop => "drop",
        Ends::Min => "min",
        Ends::Stretch => "stretch",
    }
}

/// Python 名 -> ends；不认识的按旧形状的 drop（`clean_shape`）。
pub fn ends_from_str(s: &str) -> Ends {
    match s {
        "round" => Ends::Round,
        "keep" => Ends::Keep,
        "min" => Ends::Min,
        "stretch" => Ends::Stretch,
        _ => Ends::Drop,
    }
}

/// 从文件里读出的形状字典 -> 有效形状（`clean_shape`）。
///
/// `Ok(None)` = Python 返回 None 的情况（种类不支持、点数不够、坏笔画被跳空……），
/// `Err` = Python 会抛异常让整个工程读不开的情况（pts / vel_env / gate 坏掉）。
pub fn shape_from_json(value: &Value) -> Result<Option<Shape>, ShapeError> {
    let sh = value.as_object().ok_or(ShapeError::NotObject)?;
    let Some(kind) = sh
        .get("kind")
        .and_then(Value::as_str)
        .and_then(kind_from_str)
    else {
        return Ok(None);
    };
    let Some(pts_v) = sh.get("pts") else {
        return Ok(None);
    };
    let pts = match pts_v {
        Value::Null => return Ok(None),
        Value::Array(a) => {
            if a.is_empty() {
                return Ok(None);
            }
            pts_of(pts_v)?
        }
        _ => return Err(ShapeError::Pts),
    };

    let mut out = Shape {
        kind,
        pts: pts.clone(),
        vel0: sh.get("vel0").map_or(Ok(SHAPE_DEFAULTS.vel0), field_vel)?,
        vel1: sh.get("vel1").map_or(Ok(SHAPE_DEFAULTS.vel1), field_vel)?,
        end_dot: sh.get("end_dot").is_some_and(py_bool),
        ..Shape::default()
    };

    if let Some(env) = sh.get("vel_env")
        && py_bool(env)
    {
        let a = env.as_array().ok_or(ShapeError::VelEnv)?;
        let mut rows = Vec::with_capacity(a.len());
        for row in a {
            let p = pt_of(row).map_err(|_| ShapeError::VelEnv)?;
            rows.push([p[0], p[1].clamp(1.0, 127.0)]);
        }
        out.vel_env = rows;
    }
    if LINE_KINDS.contains(&kind) {
        out.tumour = sh.get("tumour").and_then(clean_tumour);
    }

    match kind {
        Kind::Custom => return custom_shape(sh, out),
        Kind::Arc => {
            if out.pts.len() != 3 {
                return Ok(None);
            }
            out.k = arc_k_field(sh);
        }
        Kind::Free => {
            if let Some(v) = sh.get("smooth") {
                let f = py_float(v).unwrap_or(f64::NAN);
                out.smooth = clean_level(f);
                out.k = arc_k_field(sh);
            }
        }
        Kind::Curve => {
            let n = out.pts.len();
            if n < 4 {
                return Ok(None);
            }
            let keep = n - (n - 1) % 3;
            let stroke = clean_curve(value, &out.pts[..keep]);
            if let Stroke::Curve {
                pts, sharp, sym, ..
            } = stroke
            {
                out.pts = pts;
                out.sharp = sharp;
                out.sym = sym;
            }
            // a joined curve's pieces / tumours (joined.py)
            joined::clean_joined(sh, &mut out);
            if joined::is_joined(&out) {
                out.sym = None; // a joined curve has no symmetric halves
            }
        }
        Kind::Funnel => match funnel_shape(sh, out.clone()) {
            Some(f) => out = f,
            None => return Ok(None),
        },
        Kind::Line | Kind::Poly => {}
    }
    Ok(Some(out))
}

fn field_vel(v: &Value) -> Result<f64, ShapeError> {
    py_float(v).ok_or(ShapeError::Vel)
}

/// `arc_k(sh)` 的 `k`（坏了回 1.0）。
fn arc_k_field(sh: &Map<String, Value>) -> f64 {
    arc_k(py_float(sh.get("k").unwrap_or(&Value::from(1.0))).unwrap_or(1.0))
}

fn custom_shape(sh: &Map<String, Value>, mut out: Shape) -> Result<Option<Shape>, ShapeError> {
    let strokes_v = sh.get("strokes").cloned().unwrap_or(Value::Null);
    let strokes = match &strokes_v {
        Value::Null => Vec::new(),
        Value::Array(_) => clean_strokes(&strokes_v),
        v if py_bool(v) => return Err(ShapeError::Strokes),
        _ => Vec::new(),
    };
    if out.pts.len() != 3 || strokes.is_empty() {
        return Ok(None);
    }
    out.name = py_str(sh.get("name").unwrap_or(&Value::from("")));
    out.strokes = strokes;
    out.fill = sh
        .get("fill")
        .and_then(Value::as_str)
        .map_or(Fill::Empty, fill_from_str);
    let gate = py_float(sh.get("gate").unwrap_or(&Value::from(0.0625))).ok_or(ShapeError::Gate)?;
    out.gate = gate.max(1e-6);
    out.align = match sh.get("align").and_then(Value::as_str) {
        Some("aligned") => Align::Aligned,
        Some("centred") => Align::Centred,
        _ => Align::Auto,
    };
    // 旧形状没有 ends，读作 drop（它们的音符保持原样）；union / apart 只在正好是 true 时才有
    // （Python `sh.get(k) is True`）
    out.ends = sh
        .get("ends")
        .and_then(Value::as_str)
        .map_or(Ends::Drop, ends_from_str);
    out.union = matches!(sh.get("union"), Some(Value::Bool(true)));
    out.apart = matches!(sh.get("apart"), Some(Value::Bool(true)));
    // The shapes it was made of (convert.py). Anything off (a bad old shape, missing keys, points
    // that aren't three 2-number rows) drops it, like Python's try/except around the whole block.
    if let Some(fr) = sh.get("from").and_then(Value::as_object) {
        out.from = shape_from_of(fr);
    }
    if let Some(tx) = sh.get("text").filter(|v| v.is_object()) {
        out.text = clean_text(tx);
    }
    if let Some(notes) = sh.get("notes") {
        let Some(text) = notes.as_str() else {
            return Ok(None);
        };
        if !check_notes(text) {
            return Ok(None);
        }
        out.notes = Some(text.to_string());
        out.strokes = vec![BOX_STROKE.clone()];
        out.fill = Fill::Empty;
        if sh.get("own_vel").is_some_and(py_bool) {
            out.own_vel = true;
        }
    }
    Ok(Some(out))
}

/// `clean_shape`'s `sh["from"]` (convert.py): the valid original shapes + the new shape's strokes
/// and box frame. Python wraps the whole block in try/except; anything off drops all of it, so this
/// returns None the same way.
fn shape_from_of(fr: &Map<String, Value>) -> Option<ShapeFrom> {
    let items = fr.get("shapes")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    let mut shapes = Vec::with_capacity(items.len());
    for o in items {
        if !o.is_object() {
            return None; // clean_shape(non-dict) is None -> all(olds) is false
        }
        let sh = shape_from_json(o).ok()??;
        shapes.push(sh);
    }
    let pts_v = fr.get("pts")?;
    let pts_arr = pts_v.as_array()?;
    if pts_arr.len() != 3 {
        return None;
    }
    let mut pts = Vec::with_capacity(3);
    for p in pts_arr {
        pts.push(pt_of(p).ok()?);
    }
    let strokes = match fr.get("strokes")? {
        Value::Array(_) => clean_strokes(fr.get("strokes")?),
        v if py_bool(v) => return None,
        _ => Vec::new(),
    };
    Some(ShapeFrom {
        shapes,
        strokes,
        pts,
    })
}

/// 漏斗的设置与曲线（Python 的 try 包住整段：出错就整个形状无效）。
fn funnel_shape(sh: &Map<String, Value>, mut out: Shape) -> Option<Shape> {
    let starts_v = sh.get("starts");
    let mut pts = out.pts.clone();
    // Python sh.get("starts") 在缺失与显式 null 时都是 None
    let missing_starts = starts_v.is_none_or(|v| v.is_null());
    let starts_json = if pts.len() == 2 && missing_starts {
        let (new_pts, starts) = old_funnel(sh)?;
        pts = new_pts;
        Some(starts)
    } else {
        starts_v.cloned()
    };
    if pts.len() < 4 || !pts.len().is_multiple_of(2) {
        return None;
    }
    out.pts = pts;
    let cfg = clean_funnel(sh)?;
    out.funnel_fill = cfg.0;
    out.gate0 = cfg.1;
    out.gate1 = cfg.2;
    out.vary = cfg.3;
    out.change = cfg.4;
    out.follow = cfg.5;
    out.wall = cfg.6;
    // 没有 starts 键 = 空（原版 clean_starts(None) -> []）
    out.starts = clean_starts(
        starts_json.as_ref().unwrap_or(&Value::Null),
        (out.pts.len() / 2 - 1) as i64,
    )?;
    Some(out)
}

/// `clean_funnel`：(fill, gate0, gate1, vary, change, follow, wall)；坏 gate 返回 None。
fn clean_funnel(
    sh: &Map<String, Value>,
) -> Option<(FunnelFill, f64, f64, bool, GateChange, GateFollow, WallMode)> {
    let fill = match sh.get("fill").and_then(Value::as_str) {
        Some("long") => FunnelFill::Long,
        _ => FunnelFill::Spam,
    };
    let change = match sh.get("change").and_then(Value::as_str) {
        Some("smooth") => GateChange::Smooth,
        _ => GateChange::Steps,
    };
    let follow = match sh.get("follow").and_then(Value::as_str) {
        Some("curve") => GateFollow::Curve,
        _ => GateFollow::Time,
    };
    let wall = match sh.get("wall").and_then(Value::as_str) {
        Some("past") => WallMode::Past,
        _ => WallMode::In,
    };
    let gate0 = py_float(sh.get("gate0").unwrap_or(&Value::from(0.0625)))?.max(1e-6);
    let gate1 = py_float(sh.get("gate1").unwrap_or(&Value::from(0.0625)))?.max(1e-6);
    let vary = match sh.get("vary") {
        Some(v) => py_bool(v),
        None => (gate0 - gate1).abs() > 1e-9,
    };
    let gate1 = if vary { gate1 } else { gate0 };
    Some((fill, gate0, gate1, vary, change, follow, wall))
}

/// 漏斗第一版的 `[起点, 墙顶]` + sides -> 线 + 墙与起点（`old_funnel`）。
fn old_funnel(sh: &Map<String, Value>) -> Option<(Vec<Pt>, Value)> {
    let pts = sh.get("pts")?.as_array()?;
    if pts.len() != 2 {
        return None;
    }
    let (b0, p0) = (
        py_float(&pts[0].as_array()?[0])?,
        py_float(&pts[0].as_array()?[1])?,
    );
    let (b1, p1) = (
        py_float(&pts[1].as_array()?[0])?,
        py_float(&pts[1].as_array()?[1])?,
    );
    let bend = match sh.get("bend") {
        Some(v) if py_bool(v) => {
            let a = v.as_array()?;
            vec![py_float(&a[0])?, py_float(&a[1])?]
        }
        _ => vec![0.75, 0.2],
    };
    let bend = clamp_bend(bend[0], bend[1]);
    let ends = if sh.get("sides").and_then(Value::as_str) == Some("one") {
        Value::Array(vec![
            Value::Array(vec![Value::from(bend[0]), Value::from(bend[1])]),
            Value::Null,
        ])
    } else {
        let one = Value::Array(vec![Value::from(bend[0]), Value::from(bend[1])]);
        Value::Array(vec![one.clone(), one])
    };
    let starts = Value::Array(vec![
        serde_json::json!({"line": 0, "at": 0.0, "ends": ends}),
    ]);
    let new_pts = if sh.get("sides").and_then(Value::as_str) == Some("one") {
        vec![[b0, p0], [b1, p0], [b1, p1], [b1, p0]]
    } else {
        let h = (p1 - p0).abs();
        vec![[b0, p0], [b1, p0], [b1, p0 + h], [b1, p0 - h]]
    };
    Some((new_pts, starts))
}

fn clamp_bend(u: f64, f: f64) -> [f64; 2] {
    [u.clamp(0.01, 0.99), f.clamp(0.001, 0.999)]
}

/// 漏斗第一版的 bends 列表 -> 默认曲线（原版用最小二乘拟合，这里从简）。
/// DEFAULT_CURVE（funnel.py）。
const DEFAULT_CURVE: [[f64; 2]; 4] = [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]];

/// `clean_starts`：起点列表 + 线数 -> 有效的起点。
fn clean_starts(starts: &Value, lines: i64) -> Option<Vec<FunnelStart>> {
    let items = match starts {
        Value::Null => return Some(Vec::new()),
        Value::Array(a) => a,
        v if py_bool(v) => return None,
        _ => return Some(Vec::new()),
    };
    let mut out: Vec<FunnelStart> = Vec::new();
    for st in items {
        let st = st.as_object()?;
        let raw: Vec<Option<&Value>> = {
            let ends = match st.get("ends") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(a)) => a.iter().collect(),
                Some(_) => return None,
            };
            (0..2)
                .map(|i| ends.get(i).copied().filter(|v| !v.is_null()))
                .collect()
        };
        let mut ends = [None, None];
        for (i, raw_end) in raw.iter().enumerate() {
            if let Some(c) = raw_end {
                ends[i] = clean_funnel_curve(c)?;
            }
        }
        let line = py_int(st.get("line").unwrap_or(&Value::from(0)))?;
        if (ends[0].is_some() || ends[1].is_some()) && line >= 0 && line < lines {
            let at = py_float(st.get("at").unwrap_or(&Value::from(0.0)))?.clamp(0.0, 1.0);
            out.push(FunnelStart {
                line: line as usize,
                at,
                ends,
            });
        }
    }
    out.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal))
    });
    Some(out)
}

/// `clean_curve`（funnel.py）：旧的 bends 列表用默认曲线代替。
fn clean_funnel_curve(c: &Value) -> Option<Option<FunnelCurve>> {
    if !py_bool(c) {
        return Some(None);
    }
    if c.is_array() {
        return Some(Some(default_funnel_curve()));
    }
    let d = c.as_object()?;
    let pts_v = d.get("pts").cloned().unwrap_or(Value::Null);
    let pts = pts_v.as_array()?;
    let mut pts: Vec<Pt> = pts
        .iter()
        .map(|p| {
            let a = p.as_array()?;
            Some([py_float(a.first()?)?, py_float(a.get(1)?)?])
        })
        .collect::<Option<_>>()?;
    if pts.len() < 4 || !(pts.len() - 1).is_multiple_of(3) {
        return Some(Some(default_funnel_curve()));
    }
    pts[0] = [0.0, 0.0];
    let last = pts.len() - 1;
    pts[last] = [1.0, 1.0];
    let n = (pts.len() as i64 - 1) / 3 + 1;
    let sharp = match d.get("sharp") {
        None => Vec::new(),
        Some(v) => {
            let values = match v {
                Value::Array(a) => a.clone(),
                Value::String(s) => s.chars().map(|ch| Value::String(ch.to_string())).collect(),
                _ => return None,
            };
            let mut out = Vec::new();
            for a in values {
                let i = py_int(&a)?;
                if 0 < i && i < n - 1 {
                    out.push(i as usize);
                }
            }
            out.sort_unstable();
            out.dedup();
            out
        }
    };
    let (link, flip) = match d.get("link") {
        None | Some(Value::Null) => (None, false),
        Some(v) => (Some(py_int(v)?), d.get("flip").is_some_and(py_bool)),
    };
    Some(Some(FunnelCurve {
        pts,
        sharp,
        link,
        flip,
    }))
}

fn default_funnel_curve() -> FunnelCurve {
    FunnelCurve {
        pts: DEFAULT_CURVE.to_vec(),
        sharp: Vec::new(),
        link: None,
        flip: false,
    }
}

// ---------------------------------------------------------------- 写（Shape -> 字典）

fn pt_value(p: Pt) -> Value {
    Value::Array(vec![Value::from(p[0]), Value::from(p[1])])
}

fn pts_value(pts: &[Pt]) -> Value {
    Value::Array(pts.iter().map(|p| pt_value(*p)).collect())
}

fn stroke_value(st: &Stroke) -> Value {
    let mut o = Map::new();
    match st {
        Stroke::Poly {
            pts,
            free,
            smooth,
            k,
            ..
        } => {
            o.insert("kind".into(), Value::from("poly"));
            o.insert("pts".into(), pts_value(pts));
            if *free {
                o.insert("free".into(), Value::Bool(true));
                o.insert("smooth".into(), Value::from(*smooth));
                o.insert("k".into(), Value::from(*k));
            }
        }
        Stroke::Curve {
            pts, sharp, sym, ..
        } => {
            o.insert("kind".into(), Value::from("curve"));
            o.insert("pts".into(), pts_value(pts));
            if !sharp.is_empty() {
                o.insert(
                    "sharp".into(),
                    Value::Array(sharp.iter().map(|&i| Value::from(i as i64)).collect()),
                );
            }
            if let Some(s) = sym {
                o.insert("sym".into(), Value::from(sym_str(*s)));
            }
        }
        Stroke::Arc { pts, k, .. } => {
            o.insert("kind".into(), Value::from("arc"));
            o.insert("pts".into(), pts_value(pts));
            o.insert("k".into(), Value::from(*k));
        }
        Stroke::Ellipse { box_, .. } => {
            o.insert("kind".into(), Value::from("ellipse"));
            o.insert(
                "box".into(),
                Value::Array(box_.iter().map(|&x| Value::from(x)).collect()),
            );
        }
    }
    if let Some(src) = st.src() {
        o.insert("src".into(), Value::from(src));
    }
    Value::Object(o)
}

/// [`Sym`] -> Python 名。
pub fn sym_str(s: Sym) -> &'static str {
    match s {
        Sym::Mirror => "mirror",
        Sym::Turn => "turn",
    }
}

fn tumour_value(tm: &Tumour) -> Value {
    serde_json::json!({
        "on": tm.on,
        "shape": tumour_shape_str(tm.shape),
        "size": tm.size,
        "length": tm.length,
        "dist": tm.dist,
        "side": tumour_side_str(tm.side),
        "wrap": tumour_wrap_str(tm.wrap),
        "start": tm.start,
        "end": tm.end,
        "ease": tm.ease,
        "fit": tm.fit,
        "seed": tm.seed,
        "mirror": tm.mirror,
        "k": tm.k,
    })
}

fn tumour_shape_str(s: spiderweb_core::shape::TumourShape) -> &'static str {
    use spiderweb_core::shape::TumourShape as T;
    match s {
        T::Triangle => "triangle",
        T::Square => "square",
        T::Circle => "circle",
        T::Parabola => "parabola",
    }
}

fn tumour_side_str(s: spiderweb_core::shape::TumourSide) -> &'static str {
    use spiderweb_core::shape::TumourSide as T;
    match s {
        T::Alt => "alt",
        T::Left => "left",
        T::Right => "right",
        T::Random => "random",
    }
}

fn tumour_wrap_str(s: spiderweb_core::shape::TumourWrap) -> &'static str {
    use spiderweb_core::shape::TumourWrap as T;
    match s {
        T::Simple => "simple",
        T::Wrap => "wrap",
    }
}

fn text_value(tx: &TextSettings) -> Value {
    serde_json::json!({
        "text": tx.text,
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
        "bbox": tx.bbox,
        "cap": tx.cap,
        "k": tx.k,
        "holes": tx.holes,
    })
}

fn funnel_curve_value(c: &FunnelCurve) -> Value {
    let mut o = Map::new();
    o.insert("pts".into(), pts_value(&c.pts));
    o.insert(
        "sharp".into(),
        Value::Array(c.sharp.iter().map(|&i| Value::from(i as i64)).collect()),
    );
    if let Some(link) = c.link {
        o.insert("link".into(), Value::from(link));
        o.insert("flip".into(), Value::Bool(c.flip));
    }
    Value::Object(o)
}

fn start_value(st: &FunnelStart) -> Value {
    serde_json::json!({
        "line": st.line as i64,
        "at": st.at,
        "ends": [
            st.ends[0].as_ref().map(funnel_curve_value),
            st.ends[1].as_ref().map(funnel_curve_value),
        ],
    })
}

/// 有效形状 -> Python `clean_shape` 会得到的形状字典。
pub fn shape_to_json(sh: &Shape) -> Value {
    let mut o = Map::new();
    o.insert("vel0".into(), num_value(sh.vel0));
    o.insert("vel1".into(), num_value(sh.vel1));
    o.insert("end_dot".into(), Value::Bool(sh.end_dot));
    o.insert("kind".into(), Value::from(sh.kind.as_str()));
    o.insert("pts".into(), pts_value(&sh.pts));
    if !sh.vel_env.is_empty() {
        o.insert("vel_env".into(), pts_value(&sh.vel_env));
    }
    if let Some(tm) = &sh.tumour {
        o.insert("tumour".into(), tumour_value(tm));
    }
    match sh.kind {
        Kind::Line | Kind::Poly => {}
        Kind::Free => {
            o.insert("smooth".into(), Value::from(sh.smooth));
            o.insert("k".into(), Value::from(sh.k));
        }
        Kind::Arc => {
            o.insert("k".into(), Value::from(sh.k));
        }
        Kind::Curve => {
            if !sh.sharp.is_empty() {
                o.insert(
                    "sharp".into(),
                    Value::Array(sh.sharp.iter().map(|&i| Value::from(i as i64)).collect()),
                );
            }
            let joined = spiderweb_core::joined::is_joined(sh);
            if !sh.gaps.is_empty() {
                o.insert(
                    "gaps".into(),
                    Value::Array(sh.gaps.iter().map(|&i| Value::from(i as i64)).collect()),
                );
            }
            if !sh.tumours.is_empty() {
                o.insert(
                    "tumours".into(),
                    Value::Array(
                        sh.tumours
                            .iter()
                            .map(|tm| tm.as_ref().map_or(Value::Null, tumour_value))
                            .collect(),
                    ),
                );
                if !sh.splits.is_empty() {
                    o.insert(
                        "splits".into(),
                        Value::Array(sh.splits.iter().map(|&i| Value::from(i as i64)).collect()),
                    );
                }
            }
            if !joined && let Some(s) = sh.sym {
                o.insert("sym".into(), Value::from(sym_str(s)));
            }
            if !sh.gaps.is_empty() {
                o.insert(
                    "gaps".into(),
                    Value::Array(sh.gaps.iter().map(|&i| Value::from(i)).collect()),
                );
            }
        }
        Kind::Custom => {
            o.insert("name".into(), Value::from(sh.name.clone()));
            let strokes: Vec<Stroke> = if sh.notes.is_some() {
                vec![BOX_STROKE.clone()]
            } else {
                sh.strokes.clone()
            };
            o.insert(
                "strokes".into(),
                Value::Array(strokes.iter().map(stroke_value).collect()),
            );
            o.insert("fill".into(), Value::from(fill_str(sh.fill)));
            o.insert("gate".into(), num_value(sh.gate));
            o.insert("align".into(), Value::from(align_str(sh.align)));
            // 1.2.0 `clean_shape` always writes ends (a shape without the key is drop)
            o.insert("ends".into(), Value::from(ends_str(sh.ends)));
            if sh.union {
                o.insert("union".into(), Value::Bool(true));
            }
            if sh.apart {
                o.insert("apart".into(), Value::Bool(true));
            }
            if let Some(tx) = &sh.text {
                o.insert("text".into(), text_value(tx));
            }
            if let Some(notes) = &sh.notes {
                o.insert("notes".into(), Value::from(notes.clone()));
                if sh.own_vel {
                    o.insert("own_vel".into(), Value::Bool(true));
                }
            }
            if let Some(fr) = &sh.from {
                o.insert(
                    "from".into(),
                    serde_json::json!({
                        "shapes": fr.shapes.iter().map(shape_to_json).collect::<Vec<_>>(),
                        "strokes": fr.strokes.iter().map(stroke_value).collect::<Vec<_>>(),
                        "pts": pts_value(&fr.pts),
                    }),
                );
            }
        }
        Kind::Funnel => {
            o.insert(
                "fill".into(),
                Value::from(match sh.funnel_fill {
                    FunnelFill::Long => "long",
                    FunnelFill::Spam => "spam",
                }),
            );
            o.insert("gate0".into(), Value::from(sh.gate0));
            o.insert("gate1".into(), Value::from(sh.gate1));
            o.insert("vary".into(), Value::Bool(sh.vary));
            o.insert(
                "change".into(),
                Value::from(match sh.change {
                    GateChange::Smooth => "smooth",
                    GateChange::Steps => "steps",
                }),
            );
            o.insert(
                "follow".into(),
                Value::from(match sh.follow {
                    GateFollow::Curve => "curve",
                    GateFollow::Time => "time",
                }),
            );
            o.insert(
                "wall".into(),
                Value::from(match sh.wall {
                    WallMode::Past => "past",
                    WallMode::In => "in",
                }),
            );
            o.insert(
                "starts".into(),
                Value::Array(sh.starts.iter().map(start_value).collect()),
            );
        }
    }
    Value::Object(o)
}

/// `round(x, 4)`（银行家舍入），`short_env` 用。
pub(crate) fn round4(x: f64) -> f64 {
    round_half_even(x * 1e4) / 1e4
}

/// 默认灵敏度的重导出（`smooth.SMOOTH_DEFAULT`）。
pub const FREE_SMOOTH_DEFAULT: i64 = SMOOTH_DEFAULT;

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_json(extra: Value) -> Value {
        let mut v = serde_json::json!({
            "kind": "custom",
            "pts": [[0, 60], [2, 60], [0, 62]],
            "strokes": [{"kind": "poly", "pts": [[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]}],
            "fill": "fill",
            "gate": 60,
            "align": "centred",
            "name": "x",
        });
        if let Some(o) = extra.as_object() {
            for (k, val) in o {
                v[k] = val.clone();
            }
        }
        v
    }

    /// 1.2.0 工程里的 ends / union / apart 能读进来。
    #[test]
    fn reads_new_custom_keys() {
        let sh = shape_from_json(&custom_json(serde_json::json!({
            "ends": "stretch", "union": true, "apart": true
        })))
        .expect("能读")
        .expect("有效");
        assert_eq!(sh.align, Align::Centred);
        assert_eq!(sh.ends, Ends::Stretch);
        assert!(sh.union && sh.apart);
    }

    /// 旧形状没有这些键：ends 读作 drop，union / apart 关着（只有正好是 true 才算）。
    #[test]
    fn old_custom_keys_are_default() {
        let sh = shape_from_json(&custom_json(
            serde_json::json!({"union": 1, "apart": "yes"}),
        ))
        .expect("能读")
        .expect("有效");
        assert_eq!(sh.ends, Ends::Drop);
        assert!(
            !sh.union && !sh.apart,
            "Python 的 `is True`：1 / \"yes\" 都不算"
        );
    }

    /// 1.2.0 joined curve keys: gaps / splits / tumours read in, sym dropped.
    #[test]
    fn reads_joined_curve_keys() {
        let v = serde_json::json!({
            "kind": "curve",
            "pts": [[0, 60], [1, 60], [2, 60], [3, 60], [4, 60], [5, 60], [6, 60],
                    [7, 60], [8, 60], [9, 60], [10, 60], [11, 60], [12, 60]],
            "sym": "mirror",
            "gaps": [2],
            "splits": [1],
            "tumours": [null, {"on": true}, {"on": true, "size": 2.0}],
        });
        let sh = shape_from_json(&v).expect("能读").expect("有效");
        assert_eq!(sh.gaps, vec![2]);
        assert_eq!(sh.splits, vec![1]);
        assert_eq!(sh.tumours.len(), 3);
        assert!(sh.tumours[0].is_none());
        assert!(sh.tumours[1].as_ref().expect("tm").on);
        assert!((sh.tumours[2].as_ref().expect("tm").size - 2.0).abs() < 1e-12);
        assert!(sh.tumour.is_none());
        assert!(sh.sym.is_none(), "a joined curve has no symmetry");
    }

    /// The joined keys written back are read in again unchanged.
    #[test]
    fn writes_joined_curve_keys() {
        let v = serde_json::json!({
            "kind": "curve",
            "pts": [[0, 60], [1, 60], [2, 60], [3, 60], [4, 60], [5, 60], [6, 60],
                    [7, 60], [8, 60], [9, 60], [10, 60], [11, 60], [12, 60]],
            "gaps": [2],
            "splits": [1],
            "tumours": [null, {"on": true}, {"on": true, "size": 2.0}],
        });
        let sh = shape_from_json(&v).expect("能读").expect("有效");
        let out = shape_to_json(&sh);
        assert_eq!(out["gaps"], Value::Array(vec![Value::from(2)]));
        assert_eq!(out["splits"], Value::Array(vec![Value::from(1)]));
        assert_eq!(out["tumours"].as_array().expect("tumours").len(), 3);
        assert!(out.get("sym").is_none());
        let again = shape_from_json(&out).expect("能读").expect("有效");
        assert_eq!(shape_to_json(&again), out);
    }

    /// Writing: 1.2.0 `clean_shape` always writes ends (old shapes read back as drop);
    /// switches that are off are left out.
    #[test]
    fn writes_new_custom_keys() {
        let mut sh = shape_from_json(&custom_json(serde_json::json!({})))
            .expect("能读")
            .expect("有效");
        let out = shape_to_json(&sh);
        assert_eq!(out["ends"], Value::from("drop"));
        assert!(out.get("union").is_none() && out.get("apart").is_none());
        sh.ends = Ends::Min;
        sh.union = true;
        let out = shape_to_json(&sh);
        assert_eq!(out["ends"], Value::from("min"));
        assert_eq!(out["union"], Value::Bool(true));
        let again = shape_from_json(&out).expect("能读").expect("有效");
        assert_eq!(shape_to_json(&again), out);
    }
}
