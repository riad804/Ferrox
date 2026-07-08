//! # Render backend abstraction (Phase 3)
//!
//! A replaceable [`RenderBackend`] executes the frame kernels the compositor and
//! (Phase 4) render graph need, so the pipeline is:
//!
//! ```text
//! timeline → render graph → RenderBackend (CPU or GPU) → output frame
//! ```
//!
//! - [`CpuBackend`] — always available, pure-Rust reference.
//! - [`GpuBackend`] — `wgpu`-backed (`gpu` feature); one backend for Metal /
//!   Vulkan / DX12 / OpenGL / WebGPU / WebGL, with per-op CPU fallback.
//! - [`default_backend`] — GPU when a compatible adapter is present, else CPU.
//!
//! ```
//! use ferrox_core::render::{default_backend, RenderBackend};
//! use ferrox_core::{ColorGrade, AscCdl, Frame, PixelFormat};
//!
//! let backend = default_backend();
//! let graded = backend.color_grade(
//!     Frame::new(1, 1, PixelFormat::Rgba8, vec![64, 64, 64, 255]),
//!     &ColorGrade::from_cdl(AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() }),
//! ).unwrap();
//! assert_eq!(graded.data[0], 128);
//! ```

pub mod backend;
pub mod cpu;
pub mod graph;
pub mod nodes;
pub mod preview;
#[cfg(feature = "gpu")]
pub mod gpu;

pub use backend::{Capabilities, RenderBackend};
pub use cpu::CpuBackend;
pub use graph::{NodeId, RenderGraph};
pub use preview::{frames_to_skip, render as render_profiled, AdaptiveQuality, RenderProfile};
pub use nodes::{
    ChromaKeyNode, ColorNode, CompositeNode, CropNode, CustomNode, LutNode, MaskNode, RenderNode,
    ResizeNode, SolidNode, SourceNode,
};
#[cfg(feature = "gpu")]
pub use gpu::GpuBackend;

/// The best available backend: the GPU backend when the `gpu` feature is on and
/// a compatible adapter is present, otherwise the CPU backend.
pub fn default_backend() -> Box<dyn RenderBackend> {
    #[cfg(feature = "gpu")]
    {
        if crate::gpu::adapter_available() {
            return Box::new(GpuBackend::new());
        }
    }
    Box::new(CpuBackend)
}
