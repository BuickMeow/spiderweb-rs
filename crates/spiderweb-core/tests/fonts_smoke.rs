//! System font smoke test: depends on which fonts are installed, so the assertions stay loose.

use std::sync::Arc;

use spiderweb_core::fonts::{Font, font_families, get_font};

/// Common families first, so every platform has a font with an 'A' outline.
const PREFERRED: &[&str] = &[
    "Helvetica",
    "Arial",
    "Segoe UI",
    "Verdana",
    "DejaVu Sans",
    "Noto Sans",
    "Liberation Sans",
    "Tahoma",
    "Geneva",
    "Times New Roman",
];

/// Find an installed font whose 'A' outline can be read; None when there is none.
fn usable_font() -> Option<Arc<Font>> {
    let families = font_families();
    let mut candidates: Vec<&String> = Vec::new();
    for name in PREFERRED {
        if let Some(f) = families.iter().find(|f| f.eq_ignore_ascii_case(name)) {
            candidates.push(f);
        }
    }
    for f in families.iter().take(20) {
        if !candidates.contains(&f) {
            candidates.push(f);
        }
    }
    for name in candidates {
        let font = get_font(name, 400, false);
        let g = font.glyph('A');
        if font.found() && g.advance > 0.0 && !g.contours.is_empty() {
            return Some(font);
        }
    }
    None
}

#[test]
fn families_are_sorted_and_unique() {
    let families = font_families();
    assert!(!families.is_empty(), "系统里应当至少装了一个字体");
    assert!(
        families.iter().all(|f| !f.starts_with('@')),
        "@ 开头的竖排变体要去掉"
    );

    let mut sorted = families.clone();
    sorted.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    assert_eq!(sorted, families, "应按小写名排序");

    let mut unique = families.clone();
    unique.dedup();
    assert_eq!(unique, families, "不应有重复的族名");
}

#[test]
fn loads_a_system_font() {
    let Some(font) = usable_font() else {
        eprintln!("跳过：这个系统里没有能读出 'A' 轮廓的字体");
        return;
    };
    assert!(
        font.found(),
        "{} 应当匹配到同名 face，实际是 {}",
        font.family,
        font.face
    );
    assert_eq!(font.weight, 400);
    assert!(!font.italic);

    assert!(
        font.ascent > 0.3 && font.ascent < 2.0,
        "ascent = {}",
        font.ascent
    );
    assert!(
        font.descent > 0.0 && font.descent < 1.0,
        "descent = {}",
        font.descent
    );
    assert!(font.line_height > 0.5, "line_height = {}", font.line_height);
    assert!(font.cap > 0.4 && font.cap < 1.2, "cap = {}", font.cap);

    let a = font.glyph('A');
    assert!(a.advance > 0.0, "'A' 的 advance = {}", a.advance);
    assert!(!a.contours.is_empty(), "'A' 应当有轮廓");
    for contour in &a.contours {
        assert!(contour.len() >= 4, "轮廓点数 {}", contour.len());
        assert_eq!(contour.first(), contour.last(), "轮廓应当闭合");
        for p in contour {
            assert!(p[0].is_finite() && p[1].is_finite());
            assert!(
                p[0].abs() < 2.0 && p[1].abs() < 2.0,
                "em 单位下的点 {p:?} 太离谱"
            );
        }
    }

    // The same key hits the cache: the same shared font comes back.
    let again = get_font(&font.family, 400, false);
    assert!(Arc::ptr_eq(&font, &again));

    // An unknown family falls back to another font, with found false.
    let missing = get_font("__spiderweb_no_such_font__", 400, false);
    assert!(!missing.found());

    // Any character can be read without panicking.
    let _ = font.glyph('中');
    let _ = font.glyph(' ');
    let _ = font.glyph('🕷');
}

#[test]
fn kerning_values_are_em_sized() {
    let Some(font) = usable_font() else {
        eprintln!("跳过：这个系统里没有能读出 'A' 轮廓的字体");
        return;
    };
    for ((left, right), value) in &font.kerning {
        assert!(!left.is_control() && !right.is_control());
        assert!(
            value.is_finite() && value.abs() < 1.0,
            "kerning {left:?}{right:?} = {value}"
        );
    }
}
