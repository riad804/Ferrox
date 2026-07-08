//! # Vector graphics (Phase 13)
//!
//! Renders resolution-independent vector content into raster [`Frame`]s for the
//! timeline. [`SvgImage`] rasterises SVG (via the pure-Rust `resvg`/`tiny-skia`
//! stack, WASM-compatible) behind the `svg` feature.
//!
//! The [`VectorRenderer`] trait abstracts a time-varying vector source so
//! animated formats (Lottie / JSON animations) can plug in later.
//!
//! **Lottie scope note:** a production-grade pure-Rust Lottie renderer does not
//! yet exist in the ecosystem, so Lottie is deferred. Meanwhile, SVG frames can
//! be *animated* through the keyframe engine (Phase 10) — transform/opacity over
//! time — covering many "animated vector" needs without a Lottie runtime.

pub mod svg;

pub use svg::SvgImage;

use crate::error::Result;
use crate::frame::Frame;

/// A vector source that rasterises to a frame at a given size and time.
pub trait VectorRenderer: Send + Sync {
    /// The content's intrinsic (unscaled) size in pixels.
    fn intrinsic_size(&self) -> (u32, u32);

    /// Rasterise at `width × height` for time `t` seconds. Static vectors (SVG)
    /// ignore `t`; animated formats sample their timeline at `t`.
    fn render(&self, width: u32, height: u32, t: f64) -> Result<Frame>;
}
