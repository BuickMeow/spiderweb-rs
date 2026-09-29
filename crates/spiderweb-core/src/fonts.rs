//! fonts: cross-platform font reading (corresponds to Python notes/fonts.py; the original goes through Windows GDI).
//!
//! Units are em: 1.0 = font size, x to the right, y up (baseline at 0). A glyph outline is a number of closed cubic Bezier
//! curves (point lists are anchor, handle, handle, anchor, ..., first and last the same point, matching bezier.py's representation),
//! and the outline direction is kept as in the font (holes run the opposite way), which is exactly what the non-zero winding rule needs.
//!
//! Differences from the GDI original (see each function's docs):
//! - glyphs come straight from the font file's `glyf` / `CFF` outlines: lines are turned into cubic Beziers with thirds, quadratics into cubics;
//! - kerning uses only the `kern` table (GDI does the same), GPOS is not supported;
//! - italic / bold are not synthesised: when the requested weight or italic is unavailable the nearest glyph is picked;
//! - [`get_font`] returns a shared handle (`Arc<Font>`), glyphs are read on demand and cached (thread-safe).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use ttf_parser::{Face, OutlineBuilder};

use crate::Pt;

/// Capital height approximation used when no 'H' outline is found (corresponds to the original `or 0.7`).
const CAP_FALLBACK: f64 = 0.7;

/// Global font cache limit: when it exceeds this before an insert, the whole cache is cleared (corresponds to the original 40).
const CACHE_LIMIT: usize = 40;

/// A glyph: advance (forward width, em) and closed outlines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    /// Forward width (em).
    pub advance: f64,
    /// Outlines: each is a cubic Bezier point list (anchor, handle, handle, anchor, ...) with first and last the same.
    pub contours: Vec<Vec<Pt>>,
}

/// A loaded font; use [`get_font`] to get the cached shared handle.
///
/// Metrics and kerning are read at load time; glyphs are read and cached on demand by [`Font::glyph`].
pub struct Font {
    /// The requested font family name.
    pub family: String,
    /// The font name actually matched (corresponds to the original GetTextFaceW).
    pub face: String,
    /// Requested weight (100..900).
    pub weight: i32,
    /// Whether italic was requested.
    pub italic: bool,
    /// Height above the baseline (em, positive).
    pub ascent: f64,
    /// Depth below the baseline (em, positive; corresponds to the original tmDescent).
    pub descent: f64,
    /// Line height, baseline to baseline (em; corresponds to tmHeight + tmExternalLeading).
    pub line_height: f64,
    /// Capital letter height: the top of the 'H' outline, or 0.7 when there is none.
    pub cap: f64,
    /// Kerning pairs (left, right) -> em offset; empty when the font has no `kern` table.
    pub kerning: HashMap<(char, char), f64>,
    /// Cache of glyphs read so far (the original `self._glyphs`); read via [`Font::glyph`].
    glyphs: Mutex<HashMap<char, Glyph>>,
    /// Font file data (the whole file; for TTC, `index` picks the face).
    data: Arc<Vec<u8>>,
    /// Face index of the font within the file.
    index: u32,
}

impl Font {
    /// Whether the requested family matches the face actually found (the original `found`).
    pub fn found(&self) -> bool {
        self.face.to_lowercase() == self.family.to_lowercase()
    }

    /// A character's advance and outlines; glyphs read once are cached and the font is not parsed again.
    ///
    /// Non-BMP characters fall back to '?' as in the original; characters missing from the font get only an advance (the original falls back to GGO_METRICS).
    pub fn glyph(&self, ch: char) -> Glyph {
        let mut cache = self.glyphs.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(g) = cache.get(&ch) {
            return g.clone();
        }
        let g = self.read_glyph(ch);
        cache.insert(ch, g.clone());
        g
    }

    /// Read-only view of the glyph cache (characters not read yet are absent). For normal use go through [`Font::glyph`].
    pub fn glyphs(&self) -> MutexGuard<'_, HashMap<char, Glyph>> {
        self.glyphs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read one glyph from the font data (without writing the cache).
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
            // The original falls back to GGO_METRICS for missing characters: take the default glyph's advance, no outline.
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

    /// Placeholder used when no font is available (all glyphs empty, found is false).
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

/// ttf-parser outline callbacks -> cubic Bezier point lists (the equivalent of the original `_parse` + GGO_BEZIER).
struct Outline {
    /// Scale from font units -> em.
    scale: f64,
    /// Completed outlines.
    contours: Vec<Vec<Pt>>,
    /// Outline being assembled.
    current: Vec<Pt>,
}

impl Outline {
    /// `upem`: the font's design units per em.
    fn new(upem: f64) -> Self {
        Self {
            scale: 1.0 / upem,
            contours: Vec::new(),
            current: Vec::new(),
        }
    }

    /// Font unit coordinates -> em.
    fn point(&self, x: f32, y: f32) -> Pt {
        [f64::from(x) * self.scale, f64::from(y) * self.scale]
    }

    /// Line: put two control points at the thirds (the rule of the original `_parse.line_to`).
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
            // Every outline should start with move_to; treat this as the start of a new outline to be safe.
            None => self.current.push(p),
        }
    }

    /// Quadratic to cubic: c1 = q0 + 2/3 (q1 - q0), c2 = q2 + 2/3 (q1 - q2).
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

    /// Cubic Bezier: add the two control points and the end point directly.
    fn push_curve(&mut self, c1: Pt, c2: Pt, p: Pt) {
        self.current.push(c1);
        self.current.push(c2);
        self.current.push(p);
    }

    /// Finish the current outline: if it did not return to the start, close it with a straight line;
    /// drop outlines with fewer than 4 points (the original `_parse` finishing rule).
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

    /// Take all outlines (an unclosed last one is finished too).
    fn finish(mut self) -> Vec<Vec<Pt>> {
        self.close_contour();
        self.contours
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        // ttf-parser always sends move_to before each outline; finish the previous one just in case.
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

/// System font database (scanned on first use, reused afterwards).
fn database() -> &'static Database {
    static DATABASE: OnceLock<Database> = OnceLock::new();
    DATABASE.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        db
    })
}

/// Assemble a [`Font`] from a ttf-parser face; returns `None` when the data is incomplete.
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
            // ttf's descender is negative while the original tmDescent is positive, so unify to positive.
            (-descender / upem).abs(),
            // Approximates GDI's tmHeight + tmExternalLeading.
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
    // cap = the top of the 'H' outline (reading 'H' also puts it in the glyph cache, as in the original).
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

/// Horizontal kerning pairs in the `kern` table -> em (the equivalent of the original GetKerningPairsW).
///
/// Only format 0 subtables that can be enumerated whole are handled; the first non-empty horizontal
/// subtable is used (old-style GDI also only recognises this kind). GPOS kerning is not read.
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
    // Glyphs appearing in the pairs -> characters (for the same glyph take the character with the smallest code point).
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

/// Look up a system font; when family / weight / italic do not match, fall back to the default regular face, and return an empty shell when no font is available at all.
fn load_font(family: &str, weight: i32, italic: bool) -> Font {
    let db = database();
    let named = Query {
        families: &[Family::Name(family)],
        weight: Weight(weight.clamp(1, 1000) as u16),
        stretch: Stretch::Normal,
        style: if italic { Style::Italic } else { Style::Normal },
    };
    let id = db.query(&named).or_else(|| {
        // As in the original: when the Windows family is not found pick a substitute font yourself (regular).
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

/// Font cache key (family name lowercased, corresponds to the original `(family.lower(), int(weight), bool(italic))`).
#[derive(Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: String,
    weight: i32,
    italic: bool,
}

/// One font, cached in the process (reading system fonts is not cheap).
///
/// The same (family, weight, italic) returns the same `Arc<Font>` (glyph caches shared);
/// when the cache exceeds 40 entries it is cleared whole (as in the original).
pub fn get_font(family: &str, weight: i32, italic: bool) -> Arc<Font> {
    static FONTS: OnceLock<Mutex<HashMap<FontKey, Arc<Font>>>> = OnceLock::new();
    let key = FontKey {
        family: family.to_lowercase(),
        weight,
        italic,
    };
    let cache = FONTS.get_or_init(|| Mutex::new(HashMap::new()));
    // Hold the lock across the whole look-up, read and store so one key maps to exactly one Arc (reading system fonts is not cheap anyway).
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

/// Installed font family names, sorted and deduplicated (corresponds to the original tkinter `families`, dropping the vertical "@"-prefixed variants).
///
/// The sort key is the lowercased name (same as the original `key=str.lower`); ties are broken by the original name so the result is stable.
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

    /// Run the outline callbacks on fixed data (not relying on any system font).
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
        // Line 0 -> 0.3: the two control points at 1/3 and 2/3, then a straight line closes back to the start.
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
                // line_to(0, 0.5): thirds
                [1.0 / 3.0, 0.5],
                [1.0 / 6.0, 0.5],
                [0.0, 0.5],
                // close(): close from (0, 0.5) back to (0, 0) with a straight line
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
        // The single-point outline is dropped; a straight-line outline closed back to the start has 7 points and is kept.
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
