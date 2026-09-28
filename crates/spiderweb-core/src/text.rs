//! text：在已安装字体里排版的文字，变成自定义形状的字母轮廓（Python notes/text.py 的逐函数移植）。
//!
//! 文本形状是一个自定义形状，`sh.text` 存设置（TEXT_DEFAULTS 加 text / bbox / cap / k / holes）。
//! 字母在 em 单位里排版（fonts：1.0 = 字号，y 从第一行基线向上）；bbox = 轮廓在 em 单位里的范围，
//! `sh.pts`（自定义形状的框）说明这个 bbox 在卷轴上的位置。于是 em 单位 → 拍 / 音高由形状自己决定
//! （text_axes），重新输入或改设置时，文本留在原处，不管它被怎么移动、缩放、旋转或斜切。
//!
//! 字母如何变成音符：Fill / Spam 用非零环绕规则（重叠的笔画保持填充，O / A / B 的洞保持空），
//! 加阈值（一个 key 的高度有多少在字母里，那个 key 才在那里演奏）。grow 让笔画变粗（keys，可为负），
//! 它按输入时屏幕上的样子算（k = 当时的每 key 拍数）。
//!
//! 与原版的差异：字体查询不在这里做，调用方负责 [`text_font`] / [`crate::fonts::get_font`]；
//! 只用到字体 cap 的函数（[`em_keys`]、[`new_axes`]、[`shown_size`]、[`restyle`]）直接收
//! 解析好的 `font_cap`，这样纯逻辑不依赖机器上装了什么字体。

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::Pt;
use crate::bezier::segments;
use crate::fonts::{Font, get_font};
use crate::shape::{Shape, Stroke, TextAlign, TextSettings, TextUnit};
use crate::{dist, hypot2, round_half_even};

/// 字号单位（text.UNITS）：font = size 是字号（em）占多少 keys，rows = 大写字母正好那么高。
pub const UNITS: [TextUnit; 2] = [TextUnit::Font, TextUnit::Rows];

/// 对齐方式（text.TEXT_ALIGNS）。
pub const TEXT_ALIGNS: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

/// 阈值把每个 key 分成这么多行来看（text.SUB_ROWS，5% 一步）。
pub const SUB_ROWS: usize = 20;

/// 展平容差：字形轮廓用这个容差展平（text.flatten 的默认值）。
pub const FLATTEN_TOL: f64 = 0.004;

/// 文本形状的框／轴：(O, X, Y)，em 点 (x, y) 在卷轴上是 O + x·X + y·Y（拍，音高）。
pub type Axes = (Pt, Pt, Pt);

/// 排版后的一条轮廓：属于第几个字符（glyph 序号按字符递增）与字母轮廓（三次贝塞尔点列）。
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    /// 字符序号（每个字符一个号，含空格；对应 Python layout 的 glyph number）。
    pub glyph: usize,
    /// 轮廓点列（anchor, handle, handle, anchor, ...，首尾闭合）。
    pub pts: Vec<Pt>,
}

/// TEXT_DEFAULTS 的等价物（含 bbox / cap / k / holes 的合理初值）。
pub fn text_defaults() -> TextSettings {
    TextSettings::default()
}

// ---------------------------------------------------------------- 设置清理

/// 从文件里的值取文本设置（坏了就 None，对应 text.clean_text）。
///
/// 逐条对应 Python：缺的键用默认；`float` / `int` 只认数字、布尔与数字字符串；
/// `unit` / `align` 不在白名单就回到默认；weight 夹到 1..1000，threshold 夹到 0..100；
/// cap / k 为 0 时回到 0.7 / 1.0；bbox 必须有且正好 4 个浮点；holes 排序去重。
///
/// 与原版的差异：Python `str()` / 非数字字符串的边界（下划线数字、容器 repr）不复刻，
/// 这种情况返回 None；holes 的负数下标无法放进 `Vec<usize>`，也返回 None。
pub fn clean_text(value: &Value) -> Option<TextSettings> {
    let d = value.as_object()?;
    let defaults = TextSettings::default();
    let mut out = defaults.clone();
    out.text = match d.get("text") {
        None => String::new(),
        Some(v) => py_str(v)?,
    };
    out.font = match d.get("font") {
        Some(v) if py_bool(v) => py_str(v)?,
        _ => defaults.font.clone(),
    };
    out.size = opt_float(d, "size", defaults.size)?;
    out.tracking = opt_float(d, "tracking", defaults.tracking)?;
    out.leading = opt_float(d, "leading", defaults.leading)?;
    out.threshold = opt_float(d, "threshold", defaults.threshold)?;
    out.grow = opt_float(d, "grow", defaults.grow)?;
    out.unit = match d.get("unit").and_then(Value::as_str) {
        Some("rows") => TextUnit::Rows,
        _ => TextUnit::Font,
    };
    out.align = match d.get("align").and_then(Value::as_str) {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ => TextAlign::Left,
    };
    out.weight = match d.get("weight") {
        None => defaults.weight,
        Some(v) => py_int(v)? as i32,
    };
    out.weight = out.weight.clamp(1, 1000);
    out.italic = d.get("italic").map_or(defaults.italic, py_bool);
    // Python min(100.0, max(0.0, x))：NaN 与 ≤ 0 都取 0（连 -0.0 也归成 +0.0），
    // +inf 取 100；不能直接用 clamp：它会把 NaN 原样留下，也不改 -0.0。
    out.threshold = if out.threshold.is_nan() || out.threshold <= 0.0 {
        0.0
    } else if out.threshold > 100.0 {
        100.0
    } else {
        out.threshold
    };
    let bbox = d.get("bbox")?;
    let bbox = match bbox {
        Value::Array(a) => a.iter().map(py_float).collect::<Option<Vec<f64>>>()?,
        Value::String(s) => s
            .chars()
            .map(|c| py_float(&Value::String(c.to_string())))
            .collect::<Option<Vec<f64>>>()?,
        _ => return None,
    };
    if bbox.len() != 4 {
        return None;
    }
    out.bbox = [bbox[0], bbox[1], bbox[2], bbox[3]];
    let cap = opt_float(d, "cap", 0.7)?;
    out.cap = if cap != 0.0 { cap } else { 0.7 };
    let k = opt_float(d, "k", 1.0)?;
    out.k = if k != 0.0 { k } else { 1.0 };
    out.holes = match d.get("holes") {
        None => Vec::new(),
        Some(Value::Array(a)) => {
            let mut holes: Vec<usize> = Vec::with_capacity(a.len());
            for item in a {
                holes.push(usize::try_from(py_int(item)?).ok()?);
            }
            holes.sort_unstable();
            holes.dedup();
            holes
        }
        Some(Value::String(s)) => {
            let mut holes: Vec<usize> = Vec::with_capacity(s.chars().count());
            for c in s.chars() {
                let v = Value::String(c.to_string());
                holes.push(usize::try_from(py_int(&v)?).ok()?);
            }
            holes.sort_unstable();
            holes.dedup();
            holes
        }
        Some(_) => return None,
    };
    Some(out)
}

/// 文本的字体（text.text_font）。
pub fn text_font(tx: &TextSettings) -> Arc<Font> {
    get_font(&tx.font, tx.weight, tx.italic)
}

/// `tx.get("cap") or font.cap` 的等价物：cap 为 0（没设过）时用字体的 cap。
fn cap_or(cap: f64, font_cap: f64) -> f64 {
    if cap != 0.0 { cap } else { font_cap }
}

/// 一个 em 在字号 size 下是多少 keys（size 框里的数，对应 text.em_keys）。
pub fn em_keys(tx: &TextSettings, size: Option<f64>, font_cap: f64) -> f64 {
    let size = size.unwrap_or(tx.size);
    if tx.unit == TextUnit::Font {
        size
    } else {
        size / cap_or(tx.cap, font_cap)
    }
}

// ---------------------------------------------------------------- 排版

/// 在字号下排版 `tx.text`（对应 text.layout）：(轮廓, 光标)。
/// 轮廓 = 每个字符的字形轮廓在 em 单位里平移好；光标 = 每个能放光标的位置（每个字符前，及行尾）。
pub fn layout(tx: &TextSettings, font: &Font) -> (Vec<Contour>, Vec<Pt>) {
    let step = font.line_height * tx.leading / 100.0;
    let track = tx.tracking / 1000.0;
    let mut contours: Vec<Contour> = Vec::new();
    let mut carets: Vec<Pt> = Vec::new();
    let mut g: usize = 0;
    for (n, line) in tx.text.split('\n').enumerate() {
        let y = -(n as f64) * step;
        let mut xs: Vec<f64> = Vec::new();
        let mut x = 0.0;
        let mut prev: Option<char> = None;
        for ch in line.chars() {
            if let Some(p) = prev {
                x += font.kerning.get(&(p, ch)).copied().unwrap_or(0.0) + track;
            }
            xs.push(x);
            x += font.glyph(ch).advance;
            prev = Some(ch);
        }
        let shift = match tx.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -x / 2.0,
            TextAlign::Right => -x,
        };
        for (ch, &cx) in line.chars().zip(xs.iter()) {
            for c in font.glyph(ch).contours {
                contours.push(Contour {
                    glyph: g,
                    pts: c.iter().map(|p| [p[0] + cx + shift, p[1] + y]).collect(),
                });
            }
            g += 1;
        }
        carets.extend(xs.iter().map(|&cx| [cx + shift, y]));
        carets.push([x + shift, y]);
    }
    (contours, carets)
}

/// 一条贝塞尔曲线展平成点列：直的一段就一步，弯的取够多步（对应 text.flatten）。
pub fn flatten(pts: &[Pt], tol: f64) -> Vec<Pt> {
    let Some(&first) = pts.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    for seg in segments(pts) {
        let (p0, p1, p2, p3) = (seg[0], seg[1], seg[2], seg[3]);
        let bend = line_dist(p1, p0, p3).max(line_dist(p2, p0, p3));
        // Python：bend < 1e-9 一步，否则 max(2, min(24, ceil(sqrt(bend/tol) * 2)))
        let n = if bend < 1e-9 {
            1
        } else {
            let n = ((bend / tol).sqrt() * 2.0).ceil();
            if n < 2.0 {
                2
            } else if n > 24.0 {
                24
            } else {
                n as usize
            }
        };
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let mt = 1.0 - t;
            let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
            out.push([
                a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
            ]);
        }
    }
    out
}

/// 点 p 到线段 a-b 的距离（对应 text._line_dist）。
fn line_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let ll = hypot2(dx, dy);
    if ll < 1e-12 {
        dist(p, a)
    } else {
        ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / ll
    }
}

/// 闭合多边形的有向面积（对应 text.area）。
pub fn area(poly: &[Pt]) -> f64 {
    let mut sum = 0.0;
    for w in poly.windows(2) {
        sum += w[0][0] * w[1][1] - w[1][0] * w[0][1];
    }
    sum / 2.0
}

/// 点 (x, y) 绕多边形一圈的环绕数（对应 text.winding）。
pub fn winding(poly: &[Pt], x: f64, y: f64) -> i64 {
    let mut w = 0i64;
    for e in poly.windows(2) {
        let (xa, ya) = (e[0][0], e[0][1]);
        let (xb, yb) = (e[1][0], e[1][1]);
        if (ya <= y) != (yb <= y) && x < xa + (xb - xa) * (y - ya) / (yb - ya) {
            w += if yb > ya { 1 } else { -1 };
        }
    }
    w
}

/// 哪些轮廓是洞（在同字母其余轮廓的奇数层里；O 的中间，对应 text.find_holes）。
pub fn find_holes(contours: &[Contour]) -> Vec<usize> {
    let flat: Vec<Vec<Pt>> = contours
        .iter()
        .map(|c| flatten(&c.pts, FLATTEN_TOL))
        .collect();
    let mut holes = Vec::new();
    for (i, c) in contours.iter().enumerate() {
        let Some(first) = flat[i].first() else {
            continue;
        };
        let (x, y) = (first[0], first[1]);
        let depth = contours
            .iter()
            .enumerate()
            .filter(|(j, h)| *j != i && h.glyph == c.glyph && winding(&flat[*j], x, y) != 0)
            .count();
        if depth % 2 == 1 {
            holes.push(i);
        }
    }
    holes
}

// ---------------------------------------------------------------- em 单位 <-> 卷轴

/// (O, X, Y)：em 点 (x, y) 在卷轴上的位置（对应 text.text_axes）。
/// 不是文本形状（没有 text，或框不够 3 个点）时返回 None。
pub fn text_axes(sh: &Shape) -> Option<Axes> {
    let tx = sh.text.as_ref()?;
    if sh.pts.len() < 3 {
        return None;
    }
    let (b0, p0) = (sh.pts[0][0], sh.pts[0][1]);
    let (b1, p1) = (sh.pts[1][0], sh.pts[1][1]);
    let (b2, p2) = (sh.pts[2][0], sh.pts[2][1]);
    let [x0, y0, x1, y1] = tx.bbox;
    let w = x1 - x0;
    let h = y1 - y0;
    let u = [(b1 - b0) / w, (p1 - p0) / w];
    let v = [(b2 - b0) / h, (p2 - p0) / h];
    let o = [b0 - x0 * u[0] - y0 * v[0], p0 - x0 * u[1] - y0 * v[1]];
    Some((o, u, v))
}

/// 新文本的轴：第一行从 (b, p) 开始，基线在 p 下半个 key，大写字母坐在点到的 key 上、向上长
/// （对应 text.new_axes）。
pub fn new_axes(tx: &TextSettings, b: f64, p: f64, k: f64, font_cap: f64) -> Axes {
    let e = em_keys(tx, None, font_cap);
    ([b, p - 0.5], [e * k, 0.0], [0.0, e])
}

/// 一个 em 沿轴是多少 keys（输入时字母的高度，对应 text.axes_em）。
pub fn axes_em(axes: Axes, k: f64) -> f64 {
    let (_, _, y) = axes;
    hypot2(y[0] / k, y[1])
}

/// 轴整体缩放 f（对应 text.scale_axes）。
pub fn scale_axes(axes: Axes, f: f64) -> Axes {
    let (o, x, y) = axes;
    (o, [x[0] * f, x[1] * f], [y[0] * f, y[1] * f])
}

/// em 点 (x, y) → 卷轴（对应 text.to_roll）。
pub fn to_roll(axes: Axes, x: f64, y: f64) -> Pt {
    let (o, u, v) = axes;
    [o[0] + x * u[0] + y * v[0], o[1] + x * u[1] + y * v[1]]
}

/// (拍, 音高) → em 点；轴退化（平的）时返回 None（对应 text.from_roll）。
pub fn from_roll(axes: Axes, b: f64, p: f64) -> Option<Pt> {
    let (o, x, y) = axes;
    let det = x[0] * y[1] - x[1] * y[0];
    if det.abs() < 1e-15 {
        return None;
    }
    let db = b - o[0];
    let dp = p - o[1];
    Some([(db * y[1] - dp * y[0]) / det, (x[0] * dp - x[1] * db) / det])
}

/// 把 tx 按 axes 排进形状 sh（笔画、框与 sh.text）；没东西可看（没字或只有空格）返回 false
/// 且不改动 sh（对应 text.build）。
pub fn build(sh: &mut Shape, tx: &TextSettings, font: &Font, axes: Axes) -> bool {
    let (contours, _) = layout(tx, font);
    if contours.is_empty() {
        return false;
    }
    let pts: Vec<Pt> = contours
        .iter()
        .flat_map(|c| flatten(&c.pts, FLATTEN_TOL))
        .collect();
    let Some(&first) = pts.first() else {
        return false;
    };
    let (mut x0, mut x1) = (first[0], first[0]);
    let (mut y0, mut y1) = (first[1], first[1]);
    for p in &pts {
        x0 = x0.min(p[0]);
        x1 = x1.max(p[0]);
        y0 = y0.min(p[1]);
        y1 = y1.max(p[1]);
    }
    if x1 - x0 < 1e-6 {
        x0 -= 0.01;
        x1 += 0.01;
    }
    if y1 - y0 < 1e-6 {
        y0 -= 0.01;
        y1 += 0.01;
    }
    let w = x1 - x0;
    let h = y1 - y0;
    sh.strokes = contours
        .iter()
        .map(|c| Stroke::Curve {
            pts: c
                .pts
                .iter()
                .map(|p| [round_dp((p[0] - x0) / w, 7), round_dp((p[1] - y0) / h, 7)])
                .collect(),
            sharp: Vec::new(),
            sym: None,
        })
        .collect();
    sh.pts = vec![
        to_roll(axes, x0, y0),
        to_roll(axes, x1, y0),
        to_roll(axes, x0, y1),
    ];
    let mut settings = tx.clone();
    settings.bbox = [x0, y0, x1, y1];
    settings.cap = font.cap;
    settings.holes = find_holes(&contours);
    sh.text = Some(settings);
    sh.name = text_name(&tx.text);
    true
}

/// Python `round(x, 7)`（银行家舍入）的等价物。
fn round_dp(x: f64, dp: i32) -> f64 {
    let m = 10f64.powi(dp);
    round_half_even(x * m) / m
}

/// 这些轴下 size 框里的数（跟着在卷轴上缩放框走，对应 text.shown_size）。
pub fn shown_size(tx: &TextSettings, axes: Axes, font_cap: f64) -> f64 {
    let e = axes_em(axes, tx.k);
    if tx.unit == TextUnit::Font {
        e
    } else {
        e * cap_or(tx.cap, font_cap)
    }
}

/// 一次设置变更：`None` = 这一项不改（对应 Python changes 字典里没有的键）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextChange {
    pub text: Option<String>,
    pub font: Option<String>,
    pub size: Option<f64>,
    pub unit: Option<TextUnit>,
    pub weight: Option<i32>,
    pub italic: Option<bool>,
    pub tracking: Option<f64>,
    pub leading: Option<f64>,
    pub align: Option<TextAlign>,
    pub threshold: Option<f64>,
    pub grow: Option<f64>,
}

impl TextChange {
    /// 把变更合并进设置（对应 Python `dict(tx, **changes)`）。
    pub fn apply(&self, tx: &TextSettings) -> TextSettings {
        let mut new = tx.clone();
        if let Some(v) = &self.text {
            new.text = v.clone();
        }
        if let Some(v) = &self.font {
            new.font = v.clone();
        }
        if let Some(v) = self.size {
            new.size = v;
        }
        if let Some(v) = self.unit {
            new.unit = v;
        }
        if let Some(v) = self.weight {
            new.weight = v;
        }
        if let Some(v) = self.italic {
            new.italic = v;
        }
        if let Some(v) = self.tracking {
            new.tracking = v;
        }
        if let Some(v) = self.leading {
            new.leading = v;
        }
        if let Some(v) = self.align {
            new.align = v;
        }
        if let Some(v) = self.threshold {
            new.threshold = v;
        }
        if let Some(v) = self.grow {
            new.grow = v;
        }
        new
    }
}

/// 设置变了（字体、字号、单位……）：新的设置与轴（对应 text.restyle）。
/// size 框里的数不动，除非这次是手输的，所以换字体 / 单位能让字母变大变小；第一行的起点留在原处。
///
/// `font_cap` = 旧字体的 cap（仅当 `tx.cap` 为 0 时用于 shown_size）；
/// `new_cap` = 新字体的 cap（对应 Python `text_font(new).cap`）。
pub fn restyle(
    tx: &TextSettings,
    axes: Axes,
    changes: &TextChange,
    font_cap: f64,
    new_cap: f64,
) -> (TextSettings, Axes) {
    let size = changes
        .size
        .unwrap_or_else(|| shown_size(tx, axes, font_cap));
    let mut new = changes.apply(tx);
    new.cap = new_cap;
    new.size = size;
    let before = axes_em(axes, tx.k);
    if before > 1e-12 {
        let axes = scale_axes(axes, em_keys(&new, Some(size), new_cap) / before);
        (new, axes)
    } else {
        (new, axes)
    }
}

/// 形状名："“第一行前 24 个字符…”"（对应 text.text_name）。
pub fn text_name(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    if one.chars().count() > 25 {
        let head: String = one.chars().take(24).collect();
        format!("“{head}…”")
    } else {
        format!("“{one}”")
    }
}

// ---------------------------------------------------------------- 轮廓 -> 音符

/// 字母轮廓在拍 / 音高里的闭合多边形，按 grow 变粗 / 变细（对应 text.text_polys）。
pub fn text_polys(sh: &Shape) -> Vec<Vec<Pt>> {
    let Some(tx) = sh.text.as_ref() else {
        return Vec::new();
    };
    if sh.pts.len() < 3 {
        return Vec::new();
    }
    let (b0, p0) = (sh.pts[0][0], sh.pts[0][1]);
    let (b1, p1) = (sh.pts[1][0], sh.pts[1][1]);
    let (b2, p2) = (sh.pts[2][0], sh.pts[2][1]);
    let (ub, up, vb, vp) = (b1 - b0, p1 - p0, b2 - b0, p2 - p0);
    let mut polys: Vec<Vec<Pt>> = Vec::new();
    for st in &sh.strokes {
        let stroke_pts: &[Pt] = match st {
            Stroke::Poly { pts, .. } | Stroke::Curve { pts, .. } | Stroke::Arc { pts, .. } => pts,
            Stroke::Ellipse { .. } => continue,
        };
        polys.push(
            flatten(stroke_pts, 0.002)
                .into_iter()
                .map(|[u, v]| [b0 + u * ub + v * vb, p0 + u * up + v * vp])
                .collect(),
        );
    }
    if tx.grow != 0.0 {
        let k = tx.k;
        let holes: HashSet<usize> = tx.holes.iter().copied().collect();
        polys = polys
            .iter()
            .enumerate()
            .map(|(i, poly)| {
                let d = if holes.contains(&i) {
                    -tx.grow
                } else {
                    tx.grow
                };
                let scaled: Vec<Pt> = poly.iter().map(|p| [p[0] / k, p[1]]).collect();
                offset(&scaled, d)
                    .into_iter()
                    .map(|p| [p[0] * k, p[1]])
                    .collect()
            })
            .collect();
    }
    polys
}

/// 闭合多边形（首点 = 末点）的"内部"向外长 d（负 = 缩），尖角切平（bevel）免得飞出去
/// （对应 text.offset）。
pub fn offset(poly: &[Pt], d: f64) -> Vec<Pt> {
    let mut pts: Vec<Pt> = Vec::new();
    for w in poly.windows(2) {
        if dist(w[0], w[1]) > 1e-9 {
            pts.push(w[0]);
        }
    }
    let n = pts.len();
    if n < 3 {
        return poly.to_vec();
    }
    let mut closed = pts.clone();
    closed.push(pts[0]);
    // 逆时针：内部在左，所以向外是右
    let s = if area(&closed) > 0.0 { 1.0 } else { -1.0 };
    let mut normals: Vec<Pt> = Vec::with_capacity(n);
    for (i, &a) in pts.iter().enumerate() {
        let b = pts[(i + 1) % n];
        let ll = hypot2(b[0] - a[0], b[1] - a[1]);
        if ll == 0.0 {
            normals.push([0.0, 0.0]);
        } else {
            normals.push([s * (b[1] - a[1]) / ll, -s * (b[0] - a[0]) / ll]);
        }
    }
    let mut out: Vec<Pt> = Vec::new();
    for (i, &p) in pts.iter().enumerate() {
        let a = normals[(i + n - 1) % n];
        let b = normals[i];
        let dot = a[0] * b[0] + a[1] * b[1];
        if dot > -0.5 {
            // 最多约 120 度：移动后的两条边交于一点
            let m = d / (1.0 + dot);
            out.push([p[0] + (a[0] + b[0]) * m, p[1] + (a[1] + b[1]) * m]);
        } else {
            out.push([p[0] + a[0] * d, p[1] + a[1] * d]);
            out.push([p[0] + b[0] * d, p[1] + b[1] * d]);
        }
    }
    if let Some(&first) = out.first() {
        out.push(first);
    }
    out
}

/// 与 [lo, hi) 有交的横平竖直的边（对应 text.row_edges）。
pub fn row_edges(polys: &[Vec<Pt>], lo: f64, hi: f64) -> Vec<[Pt; 2]> {
    let mut out = Vec::new();
    for poly in polys {
        for e in poly.windows(2) {
            let (a, b) = (e[0], e[1]);
            if a[1] != b[1] && a[1].min(b[1]) < hi && a[1].max(b[1]) > lo {
                out.push([a, b]);
            }
        }
    }
    out
}

/// 高度 y 的线上哪里在多边形里（非零规则）：[(x0, x1)]（对应 text.line_spans）。
pub fn line_spans(edges: &[[Pt; 2]], y: f64) -> Vec<[f64; 2]> {
    let mut cross: Vec<(f64, i32)> = Vec::new();
    for e in edges {
        let (xa, ya) = (e[0][0], e[0][1]);
        let (xb, yb) = (e[1][0], e[1][1]);
        if (ya <= y) != (yb <= y) {
            cross.push((
                xa + (xb - xa) * (y - ya) / (yb - ya),
                if yb > ya { 1 } else { -1 },
            ));
        }
    }
    // Python 的 tuple 排序：先 x 后方向（-1 在 1 前）；-0.0 与 0.0 视为相等
    cross.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut out = Vec::new();
    let mut w = 0i32;
    let mut start = 0.0f64;
    for (x, d) in cross {
        let before = w;
        w += d;
        if before == 0 && w != 0 {
            start = x;
        } else if before != 0 && w == 0 {
            out.push([start, x]);
        }
    }
    out
}

/// 至少 `threshold`% 的 key q 高度在字母里的拍范围（对应 text.threshold_spans）。
pub fn threshold_spans(polys: &[Vec<Pt>], q: f64, threshold: f64) -> Vec<[f64; 2]> {
    let lo = q - 0.5;
    let edges = row_edges(polys, lo, lo + 1.0);
    if edges.is_empty() {
        return Vec::new();
    }
    let need = 1i64.max((threshold / 100.0 * SUB_ROWS as f64 - 1e-9).ceil() as i64);
    let mut events: Vec<(f64, i32)> = Vec::new();
    for j in 0..SUB_ROWS {
        let y = lo + (j as f64 + 0.5) / SUB_ROWS as f64;
        for s in line_spans(&edges, y) {
            events.push((s[0], 1));
            events.push((s[1], -1));
        }
    }
    events.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut out: Vec<[f64; 2]> = Vec::new();
    let mut count = 0i64;
    let mut start: Option<f64> = None;
    for (x, d) in events {
        let before = count;
        count += i64::from(d);
        if before < need && need <= count {
            start = Some(x);
        } else if count < need && need <= before {
            let Some(s) = start else {
                continue;
            };
            let mut merged = false;
            if let Some(last) = out.last_mut()
                && s - last[1] < 1e-12
            {
                last[1] = x;
                merged = true;
            }
            if !merged && x > s {
                out.push([s, x]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------- Python float() / int() / str() / truthiness

/// Python `float(x)`：数字、布尔与数字字符串；别的（含 null）失败。
fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Python `int(x)`：数字截断、布尔与整数字符串；别的失败。
fn py_int(v: &Value) -> Option<i64> {
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
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Python `str(x)`：字符串、数字、布尔、None；容器 repr 不复刻。
fn py_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "True".into() } else { "False".into() }),
        Value::Null => Some("None".into()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Python 真值判断：0 / 空串 / 空表 / null 为假。
fn py_bool(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `tx.get(key, default)` 后 `float()`。
fn opt_float(d: &Map<String, Value>, key: &str, default: f64) -> Option<f64> {
    match d.get(key) {
        None => Some(default),
        Some(v) => py_float(v),
    }
}
