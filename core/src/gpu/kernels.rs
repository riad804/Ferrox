//! GPU dispatch: wgpu compute passes for resize and blur.

use crate::error::Result;
use crate::frame::{Frame, PixelFormat};
use crate::traits::Filter;
use super::shaders::{BLUR_WGSL, RESIZE_WGSL};
use super::GpuContext;

pub(super) fn resize_gpu_impl(ctx: &GpuContext, frame: Frame, dst_w: u32, dst_h: u32) -> Result<Frame> {
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
pub(super) fn blur_gpu_impl(ctx: &GpuContext, frame: Frame, _sigma: f32) -> Result<Frame> {
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
pub(super) fn pad_to_u32(data: &[u8]) -> Vec<u8> {
    let padded_len = (data.len() + 3) & !3;
    let mut v = data.to_vec();
    v.resize(padded_len, 0);
    v
}
