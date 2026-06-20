# Filter Reference

All filters work on `Rgb8` or `Rgba8` frames. `Yuv420p` frames (raw video) are
rejected with an error — demux and decode the video first.

---

## FilterGraph expression syntax

The `-f`/`--filter-complex` option (CLI) and the `filter_complex` job field
(HTTP service) accept a comma-separated chain of filter tokens:

```
scale=640:480,blur=2.0,brightness=20,grayscale
```

Tokens are applied left to right. Whitespace around commas is ignored.

---

## Image filters

### blur

Gaussian blur.

```
blur=<sigma>
```

| Parameter | Type  | Description              |
|-----------|-------|--------------------------|
| sigma     | f32   | Blur radius (e.g. `2.0`) |

Example: `blur=3.5`

---

### grayscale / gray

Convert to grayscale using luminosity weights (BT.709).

```
grayscale
gray
```

No parameters. The output pixel format is unchanged (Rgb8 stays Rgb8).

---

### negate / invert

Invert each RGB channel (`255 - value`). Alpha is preserved.

```
negate
invert
```

---

### brightness

Add a constant delta to each RGB channel (clamped to 0–255).

```
brightness=<delta>
```

| Parameter | Type | Description                                |
|-----------|------|--------------------------------------------|
| delta     | i32  | Amount to add; negative darkens the image. |

Example: `brightness=30`, `brightness=-20`

---

### contrast

Scale each channel around the midpoint (128).

```
contrast=<factor>
```

| Parameter | Type | Description                                  |
|-----------|------|----------------------------------------------|
| factor    | f32  | `1.0` = no change, `0.0` = flat grey, `2.0` = doubled contrast |

Example: `contrast=1.5`

---

### saturation

Lerp between the greyscale luma value and the original colour.

```
saturation=<factor>
```

| Parameter | Type | Description                                          |
|-----------|------|------------------------------------------------------|
| factor    | f32  | `1.0` = original, `0.0` = greyscale, `2.0` = vivid |

Example: `saturation=0.5`

---

### scale

Resize the image to an exact width × height (nearest-neighbour by default).

```
scale=<width>:<height>
```

| Parameter | Type | Description         |
|-----------|------|---------------------|
| width     | u32  | Output width (px)   |
| height    | u32  | Output height (px)  |

Example: `scale=1280:720`

---

## Programmatic-only filters

The following filters are available in Rust code via `ferrox-core` but are **not
yet exposed as expression tokens** in `--filter-complex`. Use them directly
through the `FilterGraph::add_filter` API.

### flip

Flip horizontally or vertically.

```rust
use ferrox_core::filters::{FlipFilter, FlipAxis};
FlipFilter::new(FlipAxis::Horizontal)
FlipFilter::new(FlipAxis::Vertical)
```

---

### rotate

Rotate 90°, 180°, or 270° clockwise.

```rust
use ferrox_core::filters::{RotateFilter, Rotation};
RotateFilter::new(Rotation::Cw90)
RotateFilter::new(Rotation::Cw180)
RotateFilter::new(Rotation::Cw270)
```

---

### crop

Crop a rectangular region.

```rust
use ferrox_core::filters::CropFilter;
CropFilter::new(x, y, width, height)
```

Returns an error if the crop region exceeds the image bounds.

---

### thumbnail

Resize to fit within a bounding box, optionally crop to exact dimensions.

```rust
use ferrox_core::filters::ThumbnailFilter;
ThumbnailFilter::new(max_width, max_height, crop_to_fit)
```

When `crop_to_fit = true`, the output is exactly `max_width × max_height`.

---

### pad

Add a solid-colour border to reach an exact canvas size.

```rust
use ferrox_core::filters::PadFilter;
PadFilter::new(out_width, out_height, bg_r, bg_g, bg_b)
```

---

### overlay

Composite one frame on top of another at an (x, y) offset.

```rust
use ferrox_core::filters::OverlayFilter;
OverlayFilter::new(overlay_frame, x, y)
```

If `overlay_frame` is `Rgba8`, pixels are alpha-blended. If it is `Rgb8`,
they replace the destination pixels directly.

---

### drawtext *(requires feature `filters-extra`)*

Rasterise a UTF-8 string onto a frame using an in-memory TTF/OTF font.

```rust
use ferrox_core::filters::{DrawTextFilter, TextColor};

let font_data = std::fs::read("font.ttf")?;
let filter = DrawTextFilter::new(
    "Hello, ferrox!",
    x, y,
    scale,
    TextColor { r: 255, g: 255, b: 255, a: 255 },
    font_data,
)?;
```

---

## Custom filters via `FilterPlugin`

Any type implementing `FilterPlugin` can be registered in a `FilterGraph`:

```rust
use ferrox_core::{filter_graph::{FilterGraph, FilterPlugin}, frame::Frame, error::Result};

struct Sepia;

impl FilterPlugin for Sepia {
    fn name(&self) -> &str { "sepia" }
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        for px in frame.data.chunks_exact_mut(3) {
            let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
            px[0] = (r * 0.393 + g * 0.769 + b * 0.189).min(255.0) as u8;
            px[1] = (r * 0.349 + g * 0.686 + b * 0.168).min(255.0) as u8;
            px[2] = (r * 0.272 + g * 0.534 + b * 0.131).min(255.0) as u8;
        }
        Ok(frame)
    }
}

let mut graph = FilterGraph::new();
graph.add_node("sepia", Box::new(Sepia));
```
