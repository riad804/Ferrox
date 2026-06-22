//! SIMD-accelerated pixel operations (feature = `"simd"`).
//!
//! When the `simd` feature is enabled, operations use the `wide` crate's
//! `f32x8` lanes (256-bit AVX2 / NEON equivalent via autovectorisation).
//! When disabled, the functions compile to identical scalar logic — no API
//! difference for callers.
//!
//! # Accelerated ops
//!
//! | Function              | Description                                         |
//! |-----------------------|-----------------------------------------------------|
//! | [`brightness_simd`]   | Add delta to all RGB channels, clamp 0–255          |
//! | [`contrast_simd`]     | Scale each channel around midpoint 128              |
//! | [`brightness_contrast_simd`] | Both operations in one pass (avoids 2 allocs) |

/// Add `delta` (signed) to every byte in `pixels`, clamping to 0–255.
///
/// The buffer is interpreted as a flat sequence of channel bytes (e.g., Rgb8).
pub fn brightness_simd(pixels: &mut [u8], delta: i32) {
    #[cfg(feature = "simd")]
    {
        use wide::f32x8;
        const LANES: usize = 8;
        let delta_v = f32x8::splat(delta as f32);

        let chunks = pixels.len() / LANES;
        let remainder = pixels.len() % LANES;

        for i in 0..chunks {
            let base = i * LANES;
            let v = f32x8::new([
                pixels[base    ] as f32, pixels[base + 1] as f32,
                pixels[base + 2] as f32, pixels[base + 3] as f32,
                pixels[base + 4] as f32, pixels[base + 5] as f32,
                pixels[base + 6] as f32, pixels[base + 7] as f32,
            ]);
            let out = (v + delta_v).max(f32x8::ZERO).min(f32x8::splat(255.0));
            let arr: [f32; 8] = out.into();
            for (j, &val) in arr.iter().enumerate() {
                pixels[base + j] = val as u8;
            }
        }
        // scalar tail
        let base = chunks * LANES;
        for i in 0..remainder {
            let v = pixels[base + i] as i32 + delta;
            pixels[base + i] = v.clamp(0, 255) as u8;
        }
    }
    #[cfg(not(feature = "simd"))]
    {
        for p in pixels.iter_mut() {
            let v = *p as i32 + delta;
            *p = v.clamp(0, 255) as u8;
        }
    }
}

/// Multiply each channel around midpoint 128 by `factor`, clamping 0–255.
pub fn contrast_simd(pixels: &mut [u8], factor: f32) {
    #[cfg(feature = "simd")]
    {
        use wide::f32x8;
        const LANES: usize = 8;
        let factor_v  = f32x8::splat(factor);
        let mid_v     = f32x8::splat(128.0);
        let zero_v    = f32x8::ZERO;
        let max_v     = f32x8::splat(255.0);

        let chunks    = pixels.len() / LANES;
        let remainder = pixels.len() % LANES;

        for i in 0..chunks {
            let base = i * LANES;
            let v = f32x8::new([
                pixels[base    ] as f32, pixels[base + 1] as f32,
                pixels[base + 2] as f32, pixels[base + 3] as f32,
                pixels[base + 4] as f32, pixels[base + 5] as f32,
                pixels[base + 6] as f32, pixels[base + 7] as f32,
            ]);
            let out = (factor_v * (v - mid_v) + mid_v).max(zero_v).min(max_v);
            let arr: [f32; 8] = out.into();
            for (j, &val) in arr.iter().enumerate() {
                pixels[base + j] = val as u8;
            }
        }
        let base = chunks * LANES;
        for i in 0..remainder {
            let v = factor * (pixels[base + i] as f32 - 128.0) + 128.0;
            pixels[base + i] = v.clamp(0.0, 255.0) as u8;
        }
    }
    #[cfg(not(feature = "simd"))]
    {
        for p in pixels.iter_mut() {
            let v = factor * (*p as f32 - 128.0) + 128.0;
            *p = v.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Apply brightness and contrast in a single pass.
///
/// Equivalent to calling [`brightness_simd`] then [`contrast_simd`] but
/// avoids iterating the buffer twice.
pub fn brightness_contrast_simd(pixels: &mut [u8], delta: i32, factor: f32) {
    #[cfg(feature = "simd")]
    {
        use wide::f32x8;
        const LANES: usize = 8;
        let delta_v   = f32x8::splat(delta as f32);
        let factor_v  = f32x8::splat(factor);
        let mid_v     = f32x8::splat(128.0);
        let zero_v    = f32x8::ZERO;
        let max_v     = f32x8::splat(255.0);

        let chunks    = pixels.len() / LANES;
        let remainder = pixels.len() % LANES;

        for i in 0..chunks {
            let base = i * LANES;
            let v = f32x8::new([
                pixels[base    ] as f32, pixels[base + 1] as f32,
                pixels[base + 2] as f32, pixels[base + 3] as f32,
                pixels[base + 4] as f32, pixels[base + 5] as f32,
                pixels[base + 6] as f32, pixels[base + 7] as f32,
            ]);
            let bright = (v + delta_v).max(zero_v).min(max_v);
            let out = (factor_v * (bright - mid_v) + mid_v).max(zero_v).min(max_v);
            let arr: [f32; 8] = out.into();
            for (j, &val) in arr.iter().enumerate() {
                pixels[base + j] = val as u8;
            }
        }
        let base = chunks * LANES;
        for i in 0..remainder {
            let b = (pixels[base + i] as i32 + delta).clamp(0, 255) as f32;
            let v = factor * (b - 128.0) + 128.0;
            pixels[base + i] = v.clamp(0.0, 255.0) as u8;
        }
    }
    #[cfg(not(feature = "simd"))]
    {
        for p in pixels.iter_mut() {
            let b = (*p as i32 + delta).clamp(0, 255) as f32;
            let v = factor * (b - 128.0) + 128.0;
            *p = v.clamp(0.0, 255.0) as u8;
        }
    }
}
