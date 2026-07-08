//! [`FontManager`] — the font registry, fallback chain, and glyph cache.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};

use crate::cache::SharedCache;
use crate::error::{Error, Result};

use super::{Glyph, GlyphKey};

struct FontEntry {
    bytes: Arc<Vec<u8>>,
    font: Arc<FontVec>,
}

/// Registers fonts by family, resolves them with a fallback chain, and caches
/// rasterised glyphs. Thread-safe and WASM-safe.
pub struct FontManager {
    fonts: RwLock<HashMap<String, FontEntry>>,
    fallback: RwLock<Vec<String>>,
    glyphs: SharedCache<GlyphKey, Glyph>,
}

impl FontManager {
    /// A manager with a default glyph-cache budget (8 MiB).
    pub fn new() -> Self {
        Self {
            fonts: RwLock::new(HashMap::new()),
            fallback: RwLock::new(Vec::new()),
            glyphs: SharedCache::with_max_bytes(8 * 1024 * 1024),
        }
    }

    fn fonts(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, FontEntry>> {
        self.fonts.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Register `bytes` as font `family` (TTF/OTF). Errors on invalid font data.
    pub fn register(&self, family: impl Into<String>, bytes: Vec<u8>) -> Result<()> {
        let family = family.into();
        let bytes = Arc::new(bytes);
        let font = FontVec::try_from_vec((*bytes).clone())
            .map_err(|e| Error::Filter(format!("invalid font '{family}': {e}")))?;
        self.fonts.write().unwrap_or_else(|e| e.into_inner()).insert(family, FontEntry { bytes, font: Arc::new(font) });
        Ok(())
    }

    /// Set the ordered fallback chain (family names tried when a family is
    /// missing or lacks a glyph). Put an emoji font here to cover emoji.
    pub fn set_fallback_chain(&self, chain: Vec<String>) {
        *self.fallback.write().unwrap_or_else(|e| e.into_inner()) = chain;
    }

    /// Append a family to the fallback chain.
    pub fn add_fallback(&self, family: impl Into<String>) {
        self.fallback.write().unwrap_or_else(|e| e.into_inner()).push(family.into());
    }

    pub fn contains(&self, family: &str) -> bool {
        self.fonts().contains_key(family)
    }

    /// All registered family names.
    pub fn families(&self) -> Vec<String> {
        self.fonts().keys().cloned().collect()
    }

    /// The original font bytes for `family` (e.g. to hand to a text filter).
    pub fn font_bytes(&self, family: &str) -> Option<Arc<Vec<u8>>> {
        self.fonts().get(family).map(|e| Arc::clone(&e.bytes))
    }

    /// Resolve a family to a font: the family itself, else the first available
    /// fallback.
    pub fn resolve(&self, family: &str) -> Option<Arc<FontVec>> {
        let fonts = self.fonts();
        if let Some(e) = fonts.get(family) {
            return Some(Arc::clone(&e.font));
        }
        for f in self.fallback.read().unwrap_or_else(|e| e.into_inner()).iter() {
            if let Some(e) = fonts.get(f) {
                return Some(Arc::clone(&e.font));
            }
        }
        None
    }

    /// Whether the font resolved for `family` has a glyph for `ch`.
    pub fn has_glyph(&self, family: &str, ch: char) -> bool {
        self.resolve(family).map(|f| f.glyph_id(ch).0 != 0).unwrap_or(false)
    }

    /// Pick the (family, font) to render `ch`: `family` if it has the glyph,
    /// else the first fallback that does, else any available font.
    fn font_for_char(&self, family: &str, ch: char) -> Option<(String, Arc<FontVec>)> {
        let fonts = self.fonts();
        let has = |e: &FontEntry| e.font.glyph_id(ch).0 != 0;
        if let Some(e) = fonts.get(family) {
            if has(e) {
                return Some((family.to_string(), Arc::clone(&e.font)));
            }
        }
        let fb = self.fallback.read().unwrap_or_else(|e| e.into_inner());
        for f in fb.iter() {
            if let Some(e) = fonts.get(f) {
                if has(e) {
                    return Some((f.clone(), Arc::clone(&e.font)));
                }
            }
        }
        // No font has the glyph — fall back to the family (or first fallback) so
        // we render a .notdef box rather than nothing.
        if let Some(e) = fonts.get(family) {
            return Some((family.to_string(), Arc::clone(&e.font)));
        }
        fb.iter().find_map(|f| fonts.get(f).map(|e| (f.clone(), Arc::clone(&e.font))))
    }

    /// Rasterise `ch` at `px` pixels using the best font for it, caching the
    /// result. Space and other outline-less glyphs return a zero-size glyph with
    /// a valid advance.
    pub fn rasterize_glyph(&self, family: &str, ch: char, px: f32) -> Result<Arc<Glyph>> {
        let (used, font) = self
            .font_for_char(family, ch)
            .ok_or_else(|| Error::NotFound(format!("no font for family '{family}'")))?;
        let key = GlyphKey { family: used, ch, px_bits: px.to_bits() };
        Ok(self.glyphs.get_or_insert_with(key, || rasterize(&font, ch, px)))
    }

    /// Number of glyphs currently cached.
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.len()
    }
}

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Rasterise one glyph to an alpha coverage bitmap.
fn rasterize(font: &FontVec, ch: char, px: f32) -> Glyph {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let gid = font.glyph_id(ch);
    let advance = scaled.h_advance(gid);
    let glyph = gid.with_scale(scale);

    match font.outline_glyph(glyph) {
        Some(outlined) => {
            let b = outlined.px_bounds();
            let w = b.width().ceil().max(0.0) as u32;
            let h = b.height().ceil().max(0.0) as u32;
            let mut coverage = vec![0u8; (w as usize) * (h as usize)];
            outlined.draw(|x, y, c| {
                let i = (y * w + x) as usize;
                if i < coverage.len() {
                    coverage[i] = (c * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            });
            Glyph { width: w, height: h, left: b.min.x.round() as i32, top: b.min.y.round() as i32, advance, coverage }
        }
        None => Glyph { width: 0, height: 0, left: 0, top: 0, advance, coverage: Vec::new() },
    }
}
