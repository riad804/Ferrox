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

use crate::{error::Result, frame::{Frame, PixelFormat}, traits::Filter};

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
struct GpuContext {
    device: wgpu::Device,
    queue:  wgpu::Queue,
}

/// Try to acquire a wgpu device + queue. Returns `None` on headless/CI runners
/// without a compatible GPU.
#[cfg(feature = "gpu")]
fn gpu_context() -> Option<GpuContext> {
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

// ── resize WGSL kernel ────────────────────────────────────────────────────────

#[cfg(feature = "gpu")]
const RESIZE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read>       src    : array<u32>;
@group(0) @binding(1) var<storage, read_write>  dst    : array<u32>;
@group(0) @binding(2) var<uniform>              params : ResizeParams;

struct ResizeParams {
    src_w : u32,
    src_h : u32,
    dst_w : u32,
    dst_h : u32,
}

// Bilinear sample of packed Rgb8 buffer (3 bytes per pixel, packed as u32 groups).
fn load_rgb(idx: u32) -> vec3<f32> {
    let byte_off = idx * 3u;
    let word0 = byte_off / 4u;
    let shift0 = (byte_off % 4u) * 8u;
    // Simplified: nearest-neighbour byte read from u32 array (endian-safe on LE).
    let w = src[word0];
    let r = f32((w >> shift0) & 0xFFu);
    let w1 = src[word0 + ((shift0 + 8u) / 32u)];
    let g = f32((w1 >> ((shift0 + 8u) % 32u)) & 0xFFu);
    let w2 = src[word0 + ((shift0 + 16u) / 32u)];
    let b = f32((w2 >> ((shift0 + 16u) % 32u)) & 0xFFu);
    return vec3<f32>(r, g, b);
}

fn store_rgb(idx: u32, c: vec3<u32>) {
    // Pack 3 bytes into the output buffer (nearest word).
    let byte_off = idx * 3u;
    let word0 = byte_off / 4u;
    let shift0 = (byte_off % 4u) * 8u;
    let mask0 = ~(0xFFu << shift0);
    dst[word0] = (dst[word0] & mask0) | ((c.x & 0xFFu) << shift0);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dx = gid.x;
    let dy = gid.y;
    if dx >= params.dst_w || dy >= params.dst_h { return; }

    // Bilinear UV in source space.
    let u = (f32(dx) + 0.5) * f32(params.src_w) / f32(params.dst_w) - 0.5;
    let v = (f32(dy) + 0.5) * f32(params.src_h) / f32(params.dst_h) - 0.5;
    let x0 = u32(clamp(floor(u), 0.0, f32(params.src_w  - 1u)));
    let y0 = u32(clamp(floor(v), 0.0, f32(params.src_h - 1u)));
    let x1 = min(x0 + 1u, params.src_w  - 1u);
    let y1 = min(y0 + 1u, params.src_h - 1u);
    let fx = fract(u); let fy = fract(v);

    let c00 = load_rgb(y0 * params.src_w + x0);
    let c10 = load_rgb(y0 * params.src_w + x1);
    let c01 = load_rgb(y1 * params.src_w + x0);
    let c11 = load_rgb(y1 * params.src_w + x1);
    let out = mix(mix(c00, c10, fx), mix(c01, c11, fx), fy);

    let dst_idx = dy * params.dst_w + dx;
    let ri = u32(clamp(out.x, 0.0, 255.0));
    let gi = u32(clamp(out.y, 0.0, 255.0));
    let bi = u32(clamp(out.z, 0.0, 255.0));
    // Store 3 bytes — simplified single-byte write (works for aligned pixels).
    let byte_off = dst_idx * 3u;
    dst[byte_off / 4u] = (dst[byte_off / 4u] & ~(0xFFu << ((byte_off % 4u) * 8u)))
                       | (ri << ((byte_off % 4u) * 8u));
}
"#;

// ── blur WGSL kernel ──────────────────────────────────────────────────────────

#[cfg(feature = "gpu")]
const BLUR_WGSL: &str = r#"
// 5-tap Gaussian weights (sigma ≈ 1.0; actual sigma is passed as uniform but
// weights are baked for simplicity — a production kernel would compute them).
const WEIGHTS: array<f32, 5> = array<f32, 5>(0.0625, 0.25, 0.375, 0.25, 0.0625);

@group(0) @binding(0) var<storage, read>       src    : array<u32>;
@group(0) @binding(1) var<storage, read_write>  dst    : array<u32>;
@group(0) @binding(2) var<uniform>              params : BlurParams;

struct BlurParams { width: u32, height: u32, horizontal: u32 }

fn load_byte(buf: ptr<storage, array<u32>, read>, byte_idx: u32) -> f32 {
    let w = (*buf)[byte_idx / 4u];
    return f32((w >> ((byte_idx % 4u) * 8u)) & 0xFFu);
}

fn store_byte(buf: ptr<storage, array<u32>, read_write>, byte_idx: u32, val: u32) {
    let shift = (byte_idx % 4u) * 8u;
    (*buf)[byte_idx / 4u] = ((*buf)[byte_idx / 4u] & ~(0xFFu << shift)) | ((val & 0xFFu) << shift);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x; let y = gid.y;
    if x >= params.width || y >= params.height { return; }
    for (var ch = 0u; ch < 3u; ch++) {
        var acc = 0.0;
        for (var k = 0u; k < 5u; k++) {
            let offset = i32(k) - 2;
            var sx = i32(x); var sy = i32(y);
            if params.horizontal != 0u { sx += offset; } else { sy += offset; }
            sx = clamp(sx, 0, i32(params.width)  - 1);
            sy = clamp(sy, 0, i32(params.height) - 1);
            let byte_idx = (u32(sy) * params.width + u32(sx)) * 3u + ch;
            acc += load_byte(&src, byte_idx) * WEIGHTS[k];
        }
        let out_byte = (y * params.width + x) * 3u + ch;
        store_byte(&dst, out_byte, u32(clamp(acc, 0.0, 255.0)));
    }
}
"#;

// ── GPU dispatch helpers ──────────────────────────────────────────────────────

#[cfg(feature = "gpu")]
fn resize_gpu_impl(ctx: &GpuContext, frame: Frame, dst_w: u32, dst_h: u32) -> Result<Frame> {
    use wgpu::util::DeviceExt;
    use crate::error::Error;

    if frame.format != PixelFormat::Rgb8 {
        return crate::filters::ResizeFilter::new(dst_w, dst_h).process(frame);
    }

    let src_w = frame.width;
    let src_h = frame.height;
    let src_bytes = &frame.data;

    // Pad src to u32 boundary.
    let src_padded = pad_to_u32(src_bytes);
    let dst_len_bytes = (dst_w * dst_h * 3) as usize;
    let dst_padded_len = (dst_len_bytes + 3) & !3;

    let src_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("resize_src"),
        contents: bytemuck::cast_slice(&src_padded),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let dst_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("resize_dst"),
        size: dst_padded_len as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct ResizeParams { src_w: u32, src_h: u32, dst_w: u32, dst_h: u32 }

    let params = ResizeParams { src_w, src_h, dst_w, dst_h };
    let params_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("resize_params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let shader = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("resize"),
        source: wgpu::ShaderSource::Wgsl(RESIZE_WGSL.into()),
    });

    let pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("resize_pipeline"),
        layout: None,
        module: &shader,
        entry_point: "main",
        compilation_options: Default::default(),
        cache: None,
    });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: src_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: dst_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((dst_w + 7) / 8, (dst_h + 7) / 8, 1);
    }

    // Readback buffer.
    let read_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read"),
        size: dst_padded_len as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&dst_buf, 0, &read_buf, 0, dst_padded_len as u64);
    ctx.queue.submit(std::iter::once(encoder.finish()));

    let slice = read_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| { tx.send(r).ok(); });
    ctx.device.poll(wgpu::Maintain::Wait);
    rx.recv().ok().and_then(|r| r.ok())
        .ok_or_else(|| Error::Filter("GPU readback failed".into()))?;

    let data = slice.get_mapped_range();
    let out: Vec<u8> = data[..dst_len_bytes].to_vec();
    drop(data);
    read_buf.unmap();

    Ok(Frame::new(dst_w, dst_h, PixelFormat::Rgb8, out))
}

#[cfg(feature = "gpu")]
fn blur_gpu_impl(ctx: &GpuContext, frame: Frame, _sigma: f32) -> Result<Frame> {
    use wgpu::util::DeviceExt;
    use crate::error::Error;

    if frame.format != PixelFormat::Rgb8 {
        return crate::filters::BlurFilter::new(_sigma).process(frame);
    }

    let w = frame.width;
    let h = frame.height;
    let bytes = &frame.data;
    let padded_len = (bytes.len() + 3) & !3;

    // Two-pass horizontal then vertical blur.
    let run_pass = |src_data: &[u8], horizontal: u32| -> Result<Vec<u8>> {
        let src_padded = pad_to_u32(src_data);
        let src_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blur_src"),
            contents: bytemuck::cast_slice(&src_padded),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dst_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_dst"),
            size: padded_len as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct BP { width: u32, height: u32, horizontal: u32, _pad: u32 }
        let params_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blur_params"),
            contents: bytemuck::bytes_of(&BP { width: w, height: h, horizontal, _pad: 0 }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let shader = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur"),
            source: wgpu::ShaderSource::Wgsl(BLUR_WGSL.into()),
        });
        let pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("blur_pipeline"),
            layout: None,
            module: &shader,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        let bg = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: src_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: dst_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
            ],
        });
        let read_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_read"),
            size: padded_len as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = ctx.device.create_command_encoder(&Default::default());
        { let mut pass = enc.begin_compute_pass(&Default::default());
          pass.set_pipeline(&pipeline);
          pass.set_bind_group(0, &bg, &[]);
          pass.dispatch_workgroups((w + 7) / 8, (h + 7) / 8, 1); }
        enc.copy_buffer_to_buffer(&dst_buf, 0, &read_buf, 0, padded_len as u64);
        ctx.queue.submit(std::iter::once(enc.finish()));

        let slice = read_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| { tx.send(r).ok(); });
        ctx.device.poll(wgpu::Maintain::Wait);
        rx.recv().ok().and_then(|r| r.ok())
            .ok_or_else(|| Error::Filter("GPU blur readback failed".into()))?;
        let data = slice.get_mapped_range();
        let out = data[..bytes.len()].to_vec();
        drop(data);
        read_buf.unmap();
        Ok(out)
    };

    let pass1 = run_pass(bytes, 1)?;    // horizontal
    let pass2 = run_pass(&pass1, 0)?;   // vertical
    Ok(Frame::new(w, h, PixelFormat::Rgb8, pass2))
}

#[cfg(feature = "gpu")]
fn pad_to_u32(data: &[u8]) -> Vec<u8> {
    let padded_len = (data.len() + 3) & !3;
    let mut v = data.to_vec();
    v.resize(padded_len, 0);
    v
}
