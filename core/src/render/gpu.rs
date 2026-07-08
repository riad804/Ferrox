//! [`GpuBackend`] — a `wgpu`-backed [`RenderBackend`] (`gpu` feature). `wgpu`
//! abstracts Metal / Vulkan / DX12 / OpenGL ES / WebGPU / WebGL, so this one
//! backend covers every platform. Operations with a GPU kernel run on the GPU
//! (with per-op CPU fallback when no adapter is present); the rest delegate to
//! [`CpuBackend`], so results always match the CPU baseline.

use crate::blend::BlendMode;
use crate::color::ColorGrade;
use crate::error::Result;
use crate::frame::Frame;
use crate::gpu::{adapter_available, GpuFilter, ResizeGpu};
use crate::keyer::Keyer;
use crate::mask::Mask;

use super::backend::{Capabilities, RenderBackend};
use super::cpu::CpuBackend;

/// The GPU rendering backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct GpuBackend {
    cpu: CpuBackend,
}

impl GpuBackend {
    pub fn new() -> Self {
        Self { cpu: CpuBackend }
    }
}

impl RenderBackend for GpuBackend {
    fn name(&self) -> &str {
        "gpu"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { gpu_accelerated: adapter_available() }
    }

    fn color_grade(&self, frame: Frame, grade: &ColorGrade) -> Result<Frame> {
        // GPU color-matrix kernel is future work; CPU for now.
        self.cpu.color_grade(frame, grade)
    }

    fn resize(&self, frame: Frame, width: u32, height: u32) -> Result<Frame> {
        // GPU compute resize (auto CPU fallback inside `run_gpu`).
        ResizeGpu::new(width, height).run_gpu(frame)
    }

    fn chroma_key(&self, frame: Frame, keyer: &Keyer) -> Result<Frame> {
        self.cpu.chroma_key(frame, keyer)
    }

    fn apply_mask(&self, frame: Frame, mask: &Mask) -> Result<Frame> {
        self.cpu.apply_mask(frame, mask)
    }

    fn composite(&self, base: &mut Frame, top: &Frame, x: i32, y: i32, opacity: f32, mode: BlendMode) -> Result<()> {
        self.cpu.composite(base, top, x, y, opacity, mode)
    }
}
