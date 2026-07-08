//! The [`RenderBackend`] port — the **replaceable** rendering backend the
//! compositor and (Phase 4) render graph execute their per-node kernels through.
//!
//! A [`Capabilities`] query lets callers adapt (e.g. pick GPU when present). The
//! CPU backend ([`super::CpuBackend`]) is always available; the GPU backend
//! ([`super::gpu::GpuBackend`], `gpu` feature) runs kernels on `wgpu` — which
//! itself abstracts Metal / Vulkan / DX12 / OpenGL / WebGPU / WebGL — and falls
//! back to CPU per operation.

use crate::blend::BlendMode;
use crate::color::ColorGrade;
use crate::error::Result;
use crate::frame::Frame;
use crate::keyer::Keyer;
use crate::mask::Mask;

/// What a backend can do (used to select and adapt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// True if operations run on the GPU (some may still fall back to CPU).
    pub gpu_accelerated: bool,
}

/// A replaceable rendering backend: the set of frame kernels the compositor and
/// render graph need. All frames are `Rgba8`.
pub trait RenderBackend: Send + Sync {
    /// A human-readable backend name (e.g. `"cpu"`, `"gpu"`).
    fn name(&self) -> &str;

    /// The backend's capabilities.
    fn capabilities(&self) -> Capabilities;

    /// Apply a color grade to a frame.
    fn color_grade(&self, frame: Frame, grade: &ColorGrade) -> Result<Frame>;

    /// Resize a frame to `width × height`.
    fn resize(&self, frame: Frame, width: u32, height: u32) -> Result<Frame>;

    /// Apply a chroma key (green/blue-screen) to a frame.
    fn chroma_key(&self, frame: Frame, keyer: &Keyer) -> Result<Frame>;

    /// Apply a vector mask (multiplying alpha) to a frame.
    fn apply_mask(&self, frame: Frame, mask: &Mask) -> Result<Frame>;

    /// Composite `top` onto `base` at `(x, y)` with `opacity` and `mode`.
    fn composite(&self, base: &mut Frame, top: &Frame, x: i32, y: i32, opacity: f32, mode: BlendMode) -> Result<()>;
}
