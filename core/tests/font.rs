//! Phase 12 font system: registry, fallback resolution, glyph rasterisation, and
//! the glyph cache. Uses any available system font; skips if none is found
//! (no font is bundled).
#![cfg(feature = "filters-extra")]

use std::path::{Path, PathBuf};

use ferrox_core::font::FontManager;

/// Find a general-purpose system font that has a Latin `A` (skips symbol/braille
/// fonts). Collects `.ttf`s under common locations (up to depth 3), returns the
/// first that registers and contains `A`.
fn latin_font() -> Option<Vec<u8>> {
    fn collect(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        if out.len() > 80 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut subs = Vec::new();
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    subs.push(p);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttf")) {
                    out.push(p);
                }
            }
            if depth > 0 {
                for d in subs {
                    collect(&d, depth - 1, out);
                }
            }
        }
    }

    let roots = [
        "/System/Library/Fonts",
        "/System/Library/Fonts/Supplemental",
        "/usr/share/fonts",
        "/Library/Fonts",
        "C:\\Windows\\Fonts",
    ];
    let mut paths = Vec::new();
    for r in roots {
        collect(Path::new(r), 3, &mut paths);
    }
    for p in paths {
        if let Ok(bytes) = std::fs::read(&p) {
            if bytes.is_empty() {
                continue;
            }
            let probe = FontManager::new();
            if probe.register("probe", bytes.clone()).is_ok() && probe.has_glyph("probe", 'A') {
                return Some(bytes);
            }
        }
    }
    None
}

macro_rules! font_or_skip {
    () => {
        match latin_font() {
            Some(f) => f,
            None => {
                eprintln!("skipping: no system font found");
                return;
            }
        }
    };
}

#[test]
fn register_and_query_families() {
    let font = font_or_skip!();
    let m = FontManager::new();
    assert!(!m.contains("Sans"));
    m.register("Sans", font).unwrap();
    assert!(m.contains("Sans"));
    assert_eq!(m.families(), vec!["Sans".to_string()]);
    assert!(m.font_bytes("Sans").is_some());
}

#[test]
fn register_rejects_invalid_font_bytes() {
    let m = FontManager::new();
    assert!(m.register("Bad", vec![1, 2, 3, 4]).is_err());
}

#[test]
fn resolve_uses_fallback_chain() {
    let font = font_or_skip!();
    let m = FontManager::new();
    m.register("Base", font).unwrap();
    m.set_fallback_chain(vec!["Base".to_string()]);
    // A missing family resolves via the fallback chain.
    assert!(m.resolve("DoesNotExist").is_some());
    assert!(m.resolve("Base").is_some());
    // Without any fallback and no match → None.
    let empty = FontManager::new();
    assert!(empty.resolve("Nope").is_none());
}

#[test]
fn has_glyph_for_common_character() {
    let font = font_or_skip!();
    let m = FontManager::new();
    m.register("Sans", font).unwrap();
    // Practically every text font has 'A'.
    assert!(m.has_glyph("Sans", 'A'));
}

#[test]
fn rasterizes_and_caches_glyphs() {
    let font = font_or_skip!();
    let m = FontManager::new();
    m.register("Sans", font).unwrap();

    assert_eq!(m.cached_glyphs(), 0);
    let a = m.rasterize_glyph("Sans", 'A', 48.0).unwrap();
    // 'A' produces a real bitmap with a positive advance.
    assert!(a.width > 0 && a.height > 0);
    assert!(a.advance > 0.0);
    assert_eq!(a.coverage.len(), (a.width * a.height) as usize);
    assert_eq!(m.cached_glyphs(), 1);

    // Second rasterisation is a cache hit (same Arc).
    let a2 = m.rasterize_glyph("Sans", 'A', 48.0).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &a2));
    assert_eq!(m.cached_glyphs(), 1);

    // A different size is a distinct cache entry.
    let _ = m.rasterize_glyph("Sans", 'A', 24.0).unwrap();
    assert_eq!(m.cached_glyphs(), 2);
}

#[test]
fn space_glyph_has_advance_but_no_bitmap() {
    let font = font_or_skip!();
    let m = FontManager::new();
    m.register("Sans", font).unwrap();
    let space = m.rasterize_glyph("Sans", ' ', 32.0).unwrap();
    assert_eq!((space.width, space.height), (0, 0), "space has no outline");
    assert!(space.advance > 0.0, "but it still advances the pen");
}

#[test]
fn rasterize_with_no_font_errors() {
    let m = FontManager::new();
    assert!(m.rasterize_glyph("Missing", 'A', 32.0).is_err());
}
