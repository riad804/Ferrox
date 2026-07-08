//! # Font system (Phase 12)
//!
//! A [`FontManager`] registers fonts by family (from bytes — embedded, or
//! host-downloaded Google Fonts, or an emoji font), resolves a family with a
//! **fallback chain** (including per-glyph fallback), rasterises glyphs via
//! `ab_glyph`, and caches them in an LRU **glyph cache** ([`crate::cache`]).
//!
//! Portable and WASM-safe: fonts cross as bytes, so the same manager works on
//! the web (host provides the bytes). Enumerating **system** fonts and
//! downloading **Google** fonts are host/native concerns that simply feed
//! [`FontManager::register`].
//!
//! Gated behind `filters-extra` (the `ab_glyph` rasteriser).

pub mod manager;

pub use manager::FontManager;

use crate::cache::Weight;

/// A rasterised glyph: an alpha **coverage** bitmap plus placement metrics.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    /// Left bearing (px) relative to the pen origin.
    pub left: i32,
    /// Top offset (px) of the bitmap relative to the baseline.
    pub top: i32,
    /// Horizontal advance (px) to the next glyph's origin.
    pub advance: f32,
    /// `width × height` alpha values (0 = transparent, 255 = opaque).
    pub coverage: Vec<u8>,
}

impl Weight for Glyph {
    fn weight(&self) -> usize {
        self.coverage.len().max(1)
    }
}

/// Cache key for a rasterised glyph: the (resolved) family, character, and size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub family: String,
    pub ch: char,
    /// Pixel size as raw `f32` bits (so it's hashable).
    pub px_bits: u32,
}
