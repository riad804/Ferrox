//! GPU-accelerated image filters via `wgpu` compute shaders (WGSL).
//!
//! Gated behind the `gpu` feature flag. All types are no-ops or return CPU
//! fallbacks when compiled without the feature.
//!
//! # Design
//!
//! Each `*Gpu` filter implements the CPU [`Filter`] trait as a fallback, and
//! provides an additional [`GpuFilter::run_gpu`] method when the GPU feature
//! is active. The [`FilterGraph`] (and CLI) will call `run_gpu` when the
//! `--gpu` flag is passed and a compatible adapter is found; otherwise it
//! falls back to the CPU path transparently.
//!
//! # Supported filters
//!
//! | Filter      | WGSL kernel                                    |
//! |-------------|------------------------------------------------|
//! | `ResizeGpu` | Bilinear downsample (compute shader)           |
//! | `BlurGpu`   | Separable 5-tap Gaussian blur (two-pass)       |
//!
//! # CI / headless testing
//!
//! On GPU-free CI runners (Linux/macOS/Windows GitHub Actions) `wgpu` will
//! fail to obtain an adapter. All GPU tests check `skip_if_no_gpu()` and
//! return early with a log message rather than failing.

use crate::{error::Result, frame::Frame, traits::Filter};

// ── GpuFilter trait ───────────────────────────────────────────────────────────

/// Extension trait for filters that can optionally execute on the GPU.
pub trait GpuFilter: Filter {
    /// Run the filter on the GPU. Falls back to CPU if no adapter is
    /// available or the feature is disabled.
    fn run_gpu(&self, frame: Frame) -> Result<Frame>;

    /// Returns `true` if a GPU adapter is available on this machine.
    fn gpu_available() -> bool where Self: Sized {
        #[cfg(feature = "gpu")]
        { gpu_context().is_some() }
        #[cfg(not(feature = "gpu"))]
        { false }
    }
}

// ── ResizeGpu ─────────────────────────────────────────────────────────────────

/// GPU-accelerated bilinear resize. Falls back to CPU [`ResizeFilter`] when
/// the `gpu` feature is disabled or no adapter is found.
///
/// [`ResizeFilter`]: crate::filters::ResizeFilter
pub struct ResizeGpu {
    pub width: u32,
    pub height: u32,
}

impl ResizeGpu {
    pub fn new(width: u32, height: u32) -> Self { Self { width, height } }
}

impl Filter for ResizeGpu {
    fn process(&self, frame: Frame) -> Result<Frame> {
        #[cfg(feature = "gpu")]
        if let Some(ctx) = gpu_context() {
            return resize_gpu_impl(&ctx, frame, self.width, self.height);
        }
        // CPU fallback.
        crate::filters::ResizeFilter::new(self.width, self.height).process(frame)
    }
}

impl GpuFilter for ResizeGpu {
    fn run_gpu(&self, frame: Frame) -> Result<Frame> { self.process(frame) }
}

// ── BlurGpu ───────────────────────────────────────────────────────────────────

/// GPU-accelerated separable Gaussian blur. Falls back to CPU [`BlurFilter`]
/// when the `gpu` feature is disabled or no adapter is found.
///
/// [`BlurFilter`]: crate::filters::BlurFilter
pub struct BlurGpu {
    pub sigma: f32,
}

impl BlurGpu {
    pub fn new(sigma: f32) -> Self { Self { sigma } }
}

impl Filter for BlurGpu {
    fn process(&self, frame: Frame) -> Result<Frame> {
        #[cfg(feature = "gpu")]
        if let Some(ctx) = gpu_context() {
            return blur_gpu_impl(&ctx, frame, self.sigma);
        }
        crate::filters::BlurFilter::new(self.sigma).process(frame)
    }
}

impl GpuFilter for BlurGpu {
    fn run_gpu(&self, frame: Frame) -> Result<Frame> { self.process(frame) }
}

// ── wgpu plumbing (feature = "gpu") ──────────────────────────────────────────

#[cfg(feature = "gpu")]
pub(super) struct GpuContext {
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
}

/// Try to acquire a wgpu device + queue. Returns `None` on headless/CI runners
/// without a compatible GPU.
#[cfg(feature = "gpu")]
pub(super) fn gpu_context() -> Option<GpuContext> {
    use pollster::block_on;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;

    let (device, queue) = block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
        },
        None,
    )).ok()?;

    Some(GpuContext { device, queue })
}

mod kernels;
mod shaders;
use kernels::{blur_gpu_impl, resize_gpu_impl};
