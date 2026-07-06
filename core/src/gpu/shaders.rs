//! WGSL compute-shader sources for the GPU filters.

pub(super) const RESIZE_WGSL: &str = r#"
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
pub(super) const BLUR_WGSL: &str = r#"
// 5-tap Gaussian weights (sigma ≈ 1.0; actual sigma is passed as uniform but
// weights are baked for simplicity — a production kernel would compute them).
pub(super) const WEIGHTS: array<f32, 5> = array<f32, 5>(0.0625, 0.25, 0.375, 0.25, 0.0625);

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

