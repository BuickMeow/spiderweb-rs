//! fonts：跨平台字体读取（对应 Python notes/fonts.py；原版走 Windows GDI）。
//!
//! 单位为 em：1.0 = 字号，x 向右，y 向上（基线为 0）。字形轮廓是若干条闭合的三次贝塞尔曲线
//! （点列为 anchor, handle, handle, anchor, ...，首尾同点，与 bezier.py 的表示一致），
//! 轮廓方向保持字体原样（洞的方向相反），这正是非零环绕规则需要的。
//!
//! 与 GDI 原版的差异（详见各函数注释）：
//! - 字形直接来自字体文件的 `glyf` / `CFF` 轮廓：直线按三等分转成三次贝塞尔，二次曲线转成三次；
//! - kerning 只用 `kern` 表（GDI 亦如此），不支持 GPOS；
//! - 不合成斜体 / 加粗：请求的字重或斜体不存在时挑选最接近的字形；
//! - [`get_font`] 返回共享句柄（`Arc<Font>`），字形按需读取并缓存（线程安全）。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use ttf_parser::{Face, OutlineBuilder};

use crate::Pt;

/// 找不到 'H' 轮廓时用的大写高度近似值（对应原版 `or 0.7`）。
const CAP_FALLBACK: f64 = 0.7;

/// 全局字体缓存上限：插入前超过这个数就整体清空（对应原版 40）。
const CACHE_LIMIT: usize = 40;

/// 一个字形：advance（前进宽度，em）与闭合轮廓。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    /// 前进宽度（em）。
    pub advance: f64,
    /// 轮廓：每条是一列三次贝塞尔点（anchor, handle, handle, anchor, ...），首尾相同。
    pub contours: Vec<Vec<Pt>>,
}

/// 一个已加载的字体；用 [`get_font`] 取得缓存的共享句柄。
///
/// 度量与 kerning 在加载时读好；字形由 [`Font::glyph`] 按需读取并缓存。
pub struct Font {
    /// 请求的字体族名。
    pub family: String,
    /// 实际匹配到的字体名（对应原版 GetTextFaceW）。
    pub face: String,
    /// 请求的字重（100..900）。
    pub weight: i32,
    /// 是否请求斜体。
    pub italic: bool,
    /// 基线上方高度（em，正）。
    pub ascent: f64,
    /// 基线下深度（em，正；对应原版 tmDescent）。
    pub descent: f64,
    /// 行高，基线到基线（em；对应 tmHeight + tmExternalLeading）。
    pub line_height: f64,
    /// 大写字母高度：'H' 轮廓的最高点，没有时 0.7。
    pub cap: f64,
    /// kerning 字对 (左, 右) → em 偏移；字体没有 `kern` 表时为空。
    pub kerning: HashMap<(char, char), f64>,
    /// 已读取字形的缓存（原版 `self._glyphs`）；用 [`Font::glyph`] 读取。
    glyphs: Mutex<HashMap<char, Glyph>>,
    /// 字体文件数据（整个文件；TTC 用 `index` 选面）。
    data: Arc<Vec<u8>>,
    /// 字体在文件里的面序号。
    index: u32,
}

impl Font {
    /// 请求的 family 与实际匹配到的 face 是否一致（原版 `found`）。
    pub fn found(&self) -> bool {
        self.face.to_lowercase() == self.family.to_lowercase()
    }

    /// 一个字符的 advance 与轮廓；读过的字形会缓存，之后不再解析字体。
    ///
    /// 非 BMP 字符按原版退回 '?'；字体里没有的字符只有 advance（原版退回 GGO_METRICS）。
    pub fn glyph(&self, ch: char) -> Glyph {
        let mut cache = self.glyphs.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(g) = cache.get(&ch) {
            return g.clone();
        }
        let g = self.read_glyph(ch);
        cache.insert(ch, g.clone());
        g
    }

    /// 字形缓存的只读视图（没读过的字符不在其中）。普通使用请走 [`Font::glyph`]。
    pub fn glyphs(&self) -> MutexGuard<'_, HashMap<char, Glyph>> {
        self.glyphs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 从字体数据里读取一个字形（不写缓存）。
    fn read_glyph(&self, ch: char) -> Glyph {
        let Ok(face) = Face::parse(self.data.as_slice(), self.index) else {
            return Glyph::default();
        };
        let upem = face.units_per_em();
        if upem == 0 {
            return Glyph::default();
        }
        let upem = f64::from(upem);
        let code = if (ch as u32) <= 0xFFFF { ch } else { '?' };
        let Some(gid) = face.glyph_index(code) else {
            // 原版对缺失字符退回 GGO_METRICS：拿默认字形的 advance，没有轮廓。
            let advance = face
                .glyph_index('?')
                .or(Some(ttf_parser::GlyphId(0)))
                .and_then(|g| face.glyph_hor_advance(g))
                .map_or(0.0, |a| f64::from(a) / upem);
            return Glyph {
                advance,
                contours: Vec::new(),
            };
        };
        let advance = face
            .glyph_hor_advance(gid)
            .map_or(0.0, |a| f64::from(a) / upem);
        let mut outline = Outline::new(upem);
        let _ = face.outline_glyph(gid, &mut outline);
        Glyph {
            advance,
            contours: outline.finish(),
        }
    }

    /// 没有任何字体可用时的占位（glyph 全空、found 为 false）。
    fn empty(family: &str, weight: i32, italic: bool) -> Font {
        Font {
            family: family.to_string(),
            face: String::new(),
            weight,
            italic,
            ascent: 0.8,
            descent: 0.2,
            line_height: 1.2,
            cap: CAP_FALLBACK,
            kerning: HashMap::new(),
            glyphs: Mutex::new(HashMap::new()),
            data: Arc::new(Vec::new()),
            index: 0,
        }
    }
}

/// ttf-parser 的轮廓回调 → 三次贝塞尔点列（原版 `_parse` + GGO_BEZIER 的等价物）。
struct Outline {
    /// 字体单位 → em 的缩放。
    scale: f64,
    /// 已完成的轮廓。
    contours: Vec<Vec<Pt>>,
    /// 正在拼的轮廓。
    current: Vec<Pt>,
}

impl Outline {
    /// `upem`：字体每 em 的设计单位数。
    fn new(upem: f64) -> Self {
        Self {
            scale: 1.0 / upem,
            contours: Vec::new(),
            current: Vec::new(),
        }
    }

    /// 字体单位坐标 → em。
    fn point(&self, x: f32, y: f32) -> Pt {
        [f64::from(x) * self.scale, f64::from(y) * self.scale]
    }

    /// 直线：三等分处放两个控制点（原版 `_parse.line_to` 的规则）。
    fn push_line(&mut self, p: Pt) {
        match self.current.last().copied() {
            Some(p0) => {
                self.current
                    .push([p0[0] + (p[0] - p0[0]) / 3.0, p0[1] + (p[1] - p0[1]) / 3.0]);
                self.current.push([
                    p0[0] + (p[0] - p0[0]) * 2.0 / 3.0,
                    p0[1] + (p[1] - p0[1]) * 2.0 / 3.0,
                ]);
                self.current.push(p);
            }
            // 理论上每个轮廓都以 move_to 开头，容错当作新轮廓的起点。
            None => self.current.push(p),
        }
    }

    /// 二次贝塞尔转三次：c1 = q0 + 2/3 (q1 - q0)，c2 = q2 + 2/3 (q1 - q2)。
    fn push_quad(&mut self, q1: Pt, p: Pt) {
        match self.current.last().copied() {
            Some(q0) => {
                self.current.push([
                    q0[0] + (q1[0] - q0[0]) * 2.0 / 3.0,
                    q0[1] + (q1[1] - q0[1]) * 2.0 / 3.0,
                ]);
                self.current.push([
                    p[0] + (q1[0] - p[0]) * 2.0 / 3.0,
                    p[1] + (q1[1] - p[1]) * 2.0 / 3.0,
                ]);
                self.current.push(p);
            }
            None => self.current.push(p),
        }
    }

    /// 三次贝塞尔：直接放两个控制点和终点。
    fn push_curve(&mut self, c1: Pt, c2: Pt, p: Pt) {
        self.current.push(c1);
        self.current.push(c2);
        self.current.push(p);
    }

    /// 结束当前轮廓：没回到起点就补一条直线回去，点数不足 4 的丢掉
    /// （原版 `_parse` 的收尾规则）。
    fn close_contour(&mut self) {
        let Some(&start) = self.current.first() else {
            return;
        };
        if self.current.last() != Some(&start) {
            self.push_line(start);
        }
        if self.current.len() >= 4 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    /// 取走所有轮廓（未闭合的最后一条也会被收好）。
    fn finish(mut self) -> Vec<Vec<Pt>> {
        self.close_contour();
        self.contours
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        // ttf-parser 每个轮廓前必发 move_to；顺手收好上一条以防万一。
        self.close_contour();
        self.current.push(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.push_line(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let q1 = self.point(x1, y1);
        let p = self.point(x, y);
        self.push_quad(q1, p);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = self.point(x1, y1);
        let c2 = self.point(x2, y2);
        let p = self.point(x, y);
        self.push_curve(c1, c2, p);
    }

    fn close(&mut self) {
        self.close_contour();
    }
}

/// 系统字体数据库（第一次用到时才扫描，之后复用）。
fn database() -> &'static Database {
    static DATABASE: OnceLock<Database> = OnceLock::new();
    DATABASE.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        db
    })
}

/// 从 ttf-parser 的字体里组装 [`Font`]；数据不完整时返回 `None`。
fn build_font(
    family: &str,
    weight: i32,
    italic: bool,
    face_name: String,
    data: &[u8],
    index: u32,
) -> Option<Font> {
    let (ascent, descent, line_height, kerning) = {
        let face = Face::parse(data, index).ok()?;
        let upem = face.units_per_em();
        if upem == 0 {
            return None;
        }
        let upem = f64::from(upem);
        let ascender = f64::from(face.ascender());
        let descender = f64::from(face.descender());
        let line_gap = f64::from(face.line_gap());
        (
            ascender / upem,
            // ttf 的 descender 是负的，原版 tmDescent 是正的，统一成正的。
            (-descender / upem).abs(),
            // 近似 GDI 的 tmHeight + tmExternalLeading。
            (ascender - descender + line_gap) / upem,
            read_kerning(&face, upem),
        )
    };
    let mut font = Font {
        family: family.to_string(),
        face: face_name,
        weight,
        italic,
        ascent,
        descent,
        line_height,
        cap: CAP_FALLBACK,
        kerning,
        glyphs: Mutex::new(HashMap::new()),
        data: Arc::new(data.to_vec()),
        index,
    };
    // cap = 'H' 轮廓的最高点（读 'H' 同时把它放进字形缓存，与原版一致）。
    let h = font.glyph('H');
    let top = h
        .contours
        .iter()
        .flatten()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    if top.is_finite() && top > 0.0 {
        font.cap = top;
    }
    Some(font)
}

/// `kern` 表的横向字对 → em（原版 GetKerningPairsW 的等价物）。
///
/// 只处理可以整表枚举的 format 0 子表；取第一张非空的横向子表
/// （老式 GDI 也只认这一种）。GPOS kerning 不读。
fn read_kerning(face: &Face<'_>, upem: f64) -> HashMap<(char, char), f64> {
    let Some(kern) = face.tables().kern else {
        return HashMap::new();
    };
    let mut pairs: Vec<(u16, u16, i16)> = Vec::new();
    for subtable in kern.subtables {
        if !subtable.horizontal || subtable.has_state_machine {
            continue;
        }
        if let ttf_parser::kern::Format::Format0(ref t) = subtable.format {
            for p in t.pairs {
                if p.value != 0 {
                    pairs.push(((p.pair >> 16) as u16, p.pair as u16, p.value));
                }
            }
            if !pairs.is_empty() {
                break;
            }
        }
    }
    if pairs.is_empty() {
        return HashMap::new();
    }
    // 字对里出现的字形 → 字符（同字形取码点最小的字符）。
    let wanted: HashSet<u16> = pairs.iter().flat_map(|&(l, r, _)| [l, r]).collect();
    let mut chars: HashMap<u16, char> = HashMap::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            if !subtable.is_unicode() {
                continue;
            }
            subtable.codepoints(|cp| {
                let Some(ch) = char::from_u32(cp) else {
                    return;
                };
                if let Some(gid) = subtable.glyph_index(cp)
                    && wanted.contains(&gid.0)
                {
                    chars.entry(gid.0).or_insert(ch);
                }
            });
        }
    }
    let mut out = HashMap::new();
    for (left, right, value) in pairs {
        if let (Some(&lc), Some(&rc)) = (chars.get(&left), chars.get(&right)) {
            out.entry((lc, rc)).or_insert(f64::from(value) / upem);
        }
    }
    out
}

/// 查询系统字体；家族/字重/斜体匹配不到时退回默认正体，字体全不可用时返回空壳。
fn load_font(family: &str, weight: i32, italic: bool) -> Font {
    let db = database();
    let named = Query {
        families: &[Family::Name(family)],
        weight: Weight(weight.clamp(1, 1000) as u16),
        stretch: Stretch::Normal,
        style: if italic { Style::Italic } else { Style::Normal },
    };
    let id = db.query(&named).or_else(|| {
        // 对应原版：Windows 家族找不到时自己挑一个替代字体（正体）。
        let fallback = Query {
            families: &[Family::SansSerif],
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        };
        db.query(&fallback)
            .or_else(|| db.faces().next().map(|f| f.id))
    });
    let Some(id) = id else {
        return Font::empty(family, weight, italic);
    };
    let face_name = db
        .face(id)
        .and_then(|f| f.families.first().map(|(name, _)| name.clone()))
        .unwrap_or_else(|| family.to_string());
    db.with_face_data(id, |data, index| {
        build_font(family, weight, italic, face_name, data, index)
    })
    .flatten()
    .unwrap_or_else(|| Font::empty(family, weight, italic))
}

/// 字体缓存键（家族名小写，对应原版 `(family.lower(), int(weight), bool(italic))`）。
#[derive(Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: String,
    weight: i32,
    italic: bool,
}

/// 一个字体，缓存在进程里（读取系统字体不便宜）。
///
/// 同一 (family, weight, italic) 返回同一个 `Arc<Font>`（字形缓存共享）；
/// 缓存超过 40 个时整体清空（对应原版行为）。
pub fn get_font(family: &str, weight: i32, italic: bool) -> Arc<Font> {
    static FONTS: OnceLock<Mutex<HashMap<FontKey, Arc<Font>>>> = OnceLock::new();
    let key = FontKey {
        family: family.to_lowercase(),
        weight,
        italic,
    };
    let cache = FONTS.get_or_init(|| Mutex::new(HashMap::new()));
    // 整个"查—读—存"持锁，保证同一键只对应一个 Arc（读系统字体本来就不便宜）。
    let mut fonts = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(font) = fonts.get(&key) {
        return Arc::clone(font);
    }
    let font = Arc::new(load_font(family, weight, italic));
    if fonts.len() > CACHE_LIMIT {
        fonts.clear();
    }
    fonts.insert(key, Arc::clone(&font));
    font
}

/// 已安装的字体族名，排序去重（对应原版 tkinter `families`，去掉 "@" 开头的竖排变体）。
///
/// 排序键是小写名（与原版 `key=str.lower` 一致），同键时按原名排，保证结果稳定。
pub fn font_families() -> Vec<String> {
    let db = database();
    let mut names: Vec<String> = db
        .faces()
        .filter_map(|f| f.families.first().map(|(name, _)| name.clone()))
        .filter(|name| !name.starts_with('@'))
        .collect();
    names.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用固定数据跑一遍轮廓回调（不依赖任何系统字体）。
    fn outline(upem: f64, build: impl FnOnce(&mut Outline)) -> Vec<Vec<Pt>> {
        let mut o = Outline::new(upem);
        build(&mut o);
        o.finish()
    }

    fn assert_pts(got: &[Pt], want: &[Pt]) {
        assert_eq!(got.len(), want.len(), "点数不同：{got:?} != {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g[0] - w[0]).abs() < 1e-6 && (g[1] - w[1]).abs() < 1e-6,
                "点不同：{g:?} != {w:?}"
            );
        }
    }

    #[test]
    fn line_becomes_cubic_thirds() {
        let c = outline(1.0, |o| {
            o.move_to(0.0, 0.0);
            o.line_to(0.3, 0.0);
            o.close();
        });
        assert_eq!(c.len(), 1);
        // 直线 0 → 0.3：两个控制点在 1/3、2/3 处，收尾再补一条直线回到起点。
        assert_pts(
            &c[0],
            &[
                [0.0, 0.0],
                [0.1, 0.0],
                [0.2, 0.0],
                [0.3, 0.0],
                [0.2, 0.0],
                [0.1, 0.0],
                [0.0, 0.0],
            ],
        );
    }

    #[test]
    fn quad_becomes_cubic() {
        let c = outline(1.0, |o| {
            o.move_to(0.0, 0.0);
            o.quad_to(0.5, 0.0, 0.5, 0.5);
            o.line_to(0.0, 0.5);
            o.close();
        });
        assert_eq!(c.len(), 1);
        let p = &c[0];
        assert_pts(
            &p[..4],
            &[
                [0.0, 0.0],
                // c1 = q0 + 2/3 (q1 - q0)
                [1.0 / 3.0, 0.0],
                // c2 = q2 + 2/3 (q1 - q2)
                [0.5, 1.0 / 6.0],
                [0.5, 0.5],
            ],
        );
        assert_pts(
            &p[4..],
            &[
                // line_to(0, 0.5)：三等分
                [1.0 / 3.0, 0.5],
                [1.0 / 6.0, 0.5],
                [0.0, 0.5],
                // close()：从 (0, 0.5) 补直线回 (0, 0)
                [0.0, 1.0 / 3.0],
                [0.0, 1.0 / 6.0],
                [0.0, 0.0],
            ],
        );
    }

    #[test]
    fn curve_to_keeps_controls() {
        let c = outline(1.0, |o| {
            o.move_to(0.0, 0.0);
            o.curve_to(0.0, 1.0, 1.0, 1.0, 1.0, 0.0);
            o.close();
        });
        let p = &c[0];
        assert_pts(&p[..4], &[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
    }

    #[test]
    fn coordinates_are_scaled_to_em() {
        let c = outline(1000.0, |o| {
            o.move_to(250.0, -100.0);
            o.line_to(750.0, -100.0);
            o.close();
        });
        assert_eq!(c[0][0], [0.25, -0.1]);
        assert_eq!(c[0][3], [0.75, -0.1]);
    }

    #[test]
    fn degenerate_contour_is_dropped() {
        let c = outline(1.0, |o| {
            o.move_to(0.0, 0.0);
            o.close();
            o.move_to(0.5, 0.5);
            o.line_to(0.5, 0.75);
            o.close();
        });
        // 单点轮廓被丢掉；一条直线收尾后补回起点，共 7 个点，保留。
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].len(), 7);
    }

    #[test]
    fn unclosed_contour_is_closed_at_finish() {
        let c = outline(1.0, |o| {
            o.move_to(0.0, 0.0);
            o.line_to(1.0, 0.0);
            o.line_to(1.0, 1.0);
        });
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].first(), c[0].last());
    }
}
