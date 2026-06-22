# Known Limitations

This document is an honest accounting of ferrox's current limitations. Where
a limitation is architectural, the long-term plan is described.

---

## Video decoding

### VP9 — optional (`vp9` feature, backed by `libdav1d`)

**Status**: ✅ Implemented via the `vp9` feature flag.

**How it works**: When you enable `features = ["vp9"]`, ferrox links against
`libdav1d` (the reference VP9/AV1 decoder by VideoLAN, BSD-2 license). The
`Vp9Decoder` implements `VideoDecoder` and `extract_frames` / `extract_frames_range`
automatically route VP9 packets through it.

**Install system library**:
```sh
# Linux
apt-get install libdav1d-dev
# macOS
brew install dav1d
# Windows (via vcpkg)
vcpkg install dav1d
```

**Trade-off**: Adds a C build dependency and breaks the pure-Rust build
guarantee. The default build (`cargo build`) uses no C code for VP9. Enable
explicitly:
```toml
ferrox-core = { path = "…", features = ["vp9"] }
```

**Supported formats**:

| Bit depth | Layout | ferrox PixelFormat |
|-----------|--------|--------------------|
| 8-bit  | YUV 4:2:0 | `Yuv420p`   |
| 10-bit | YUV 4:2:0 | `Yuv420p10` |
| 12-bit | YUV 4:2:0 | `Yuv420p12` |
| 8-bit  | YUV 4:2:2 | `Yuv422p`   |
| 8-bit  | YUV 4:4:4 | `Yuv444p`   |

10/12-bit samples are stored as little-endian u16 values.  Use
`yuv420p_hdr_to_rgb8()` to tone-map HDR frames to 8-bit RGB for display.

---

### H.264 — optional (`h264` feature, backed by OpenH264)

**Status**: ✅ Implemented via the `h264` feature flag.

**How it works**: When you enable `features = ["h264"]`, ferrox links against
Cisco's `libopenh264` (BSD-2 license, royalty-free patent grant). The
`H264Decoder` handles both Annex B and AVCC packet formats and converts decoded
YUV to RGB8.

**Install system library**:
```sh
# Linux (Cisco PPA or compile from source)
apt-get install libopenh264-dev
# macOS
brew install openh264
# Windows (via vcpkg)
vcpkg install openh264
```

**Trade-off**: Adds a C build dependency. The default build uses no C code for
H.264. Enable explicitly:
```toml
ferrox-core = { path = "…", features = ["h264"] }
```

**Supported profiles**: Baseline, Main, High, High 10, High 4:2:2, High 4:4:4.
Profile detection via `detect_h264_profile()` parses the SPS NAL unit.

**Output modes**: `H264OutputMode::Rgb8` (default) or `H264OutputMode::Yuv420p`
for pipelines that need raw YUV data (e.g. AV1 re-encoding without RGB round-trip).

---

## Audio encoding

### MP3 encoding — optional (`mp3-encode` feature, backed by libmp3lame)

**Status**: ✅ Implemented via the `mp3-encode` feature flag.

**How it works**: `Mp3Encoder` wraps `libmp3lame` (LGPL). Supports CBR and VBR
mode, all standard bitrates (40–320 kbps), mono/stereo/multi-channel input
(multi-channel is down-mixed to stereo).

**Install system library**:
```sh
apt-get install libmp3lame-dev   # Linux
brew install lame                 # macOS
vcpkg install mp3lame             # Windows
```

**Enable**:
```toml
ferrox-core = { path = "…", features = ["mp3-encode"] }
```

**Trade-off**: LGPL license. The default build is LGPL-free.

---

### Opus encoding — optional (`opus-encode` feature, backed by libopus)

**Status**: ✅ Implemented via the `opus-encode` feature flag.

**How it works**: `OpusEncoder` wraps `libopus` (BSD-3, Xiph). Produces
Ogg-wrapped `.opus` output (RFC 7845). Supports mono and stereo; multi-channel
input is down-mixed. Input at non-48 kHz sample rates is automatically
resampled to 48 kHz.

**Install system library**:
```sh
apt-get install libopus-dev   # Linux
brew install opus              # macOS
vcpkg install opus             # Windows
```

**Enable**:
```toml
ferrox-core = { path = "…", features = ["opus-encode"] }
```

---

## Container muxing

### TS / MPEG-TS muxer — ✅ Implemented (pure Rust)

**Status**: `MpegTsMuxer` is a pure-Rust MPEG-TS (ISO 13818-1) muxer.
No C dependencies.  Enabled via the `encode` feature (on by default).

**How it works**: Writes 188-byte transport stream packets with full PAT +
PMT + PES packetisation.  Supports AV1, H.264, and AAC elementary streams.
PCR is embedded in keyframe packets for A/V sync.

```rust
use ferrox_core::{MpegTsMuxer, traits::ContainerMuxer};
let mut ts_file = std::fs::File::create("output.ts")?;
let mut mux = MpegTsMuxer::new(&mut ts_file, &streams, fps_num, fps_den)?;
mux.write_header()?;
// ... write_packet for each EncodedPacket
mux.write_trailer()?;
```

**Limitation**: Single-program only; no TS encryption; PCR derived from video PTS.

---

### fMP4 (fragmented MP4) muxer — ✅ Implemented (pure Rust)

**Status**: `FMp4Muxer` is a pure-Rust ISO 14496-12 fragmented MP4 muxer.
No C dependencies.  Enabled via the `encode` feature (on by default).

**How it works**: Writes `ftyp` → `moov` (with `mvex`/`trex` for fragmented
mode) → `moof`+`mdat` fragment pairs.  Supports AV1 (`av01`), H.264 (`avc1`),
and AAC (`mp4a`/`esds`) tracks.  Fragments are flushed every 30 packets or at
trailer time.

```rust
use ferrox_core::{FMp4Muxer, traits::ContainerMuxer};
let mut mp4_file = std::fs::File::create("output.mp4")?;
let mut mux = FMp4Muxer::new(mp4_file, &streams, fps_num, fps_den)?;
mux.write_header()?;
// ... write_packet for each EncodedPacket
mux.write_trailer()?;
```

**Limitation**: Single video + single audio track; no edit lists; no encryption.

---

## HLS

### HLS fMP4 and MPEG-TS segments — ✅ Implemented

**Status**: The HLS segmenter now supports three output formats via `HlsSegmentFormat`:

| Format | Extension | HLS version | Compatibility |
|--------|-----------|-------------|---------------|
| `WebM` | `.webm` | v3 | Modern browsers only |
| `FMp4` *(default)* | `.mp4` | v6 + `#EXT-X-MAP` | iOS ≥ 10, all Android, all modern browsers |
| `MpegTs` | `.ts` | v3 | All HLS clients incl. iOS < 10 |

For `FMp4`, an init segment (`seginit.mp4`) is written once and referenced via
`#EXT-X-MAP` in the M3U8 playlist.  Each media segment is a self-contained
`moof`+`mdat` pair with a monotonically increasing sequence number and correct
`tfdt` (base decode time).

```rust
use ferrox_core::hls::{HlsOptions, HlsSegmentFormat, segment};

let opts = HlsOptions {
    segment_duration_secs: 6.0,
    format: HlsSegmentFormat::FMp4,
    ..HlsOptions::default()
};
segment(Path::new("input.webm"), &opts)?;
// Produces: hls_out/seginit.mp4, hls_out/seg000.mp4, …, hls_out/index.m3u8
```

---

## GPU acceleration

### GPU not available on all platforms

**Status**: The `gpu` feature requires a Vulkan, Metal, or DX12 adapter via
`wgpu`. It is not available in:

- Docker containers without GPU passthrough
- Standard GitHub Actions runners
- WASM targets

**Behaviour**: When no adapter is found, all `*Gpu` filter types automatically
fall back to their CPU equivalents. No configuration is required — the fallback
is transparent.

---

## WASM

### Video codecs in WASM — ✅ Supported

**Status**: All pure-Rust features compile to both `wasm32-unknown-unknown`
and `wasm32-wasip1`.  This includes `video-codecs` (VP8 decode, MP4/WebM
demux), `image-codecs`, `audio-codecs`, `filters-extra`, and `gif-support`.

### JavaScript bindings — ✅ Implemented (`wasm` feature)

**Status**: A `wasm-bindgen` facade in `core/src/wasm.rs` exposes these
JS-callable functions:

| Function | Description |
|----------|-------------|
| `decode_vp8_to_png(data)` | Decode a VP8 keyframe → PNG bytes |
| `decode_image_to_png(data)` | Normalise PNG/JPEG → PNG bytes |
| `resize_image(data, w, h)` | Resize PNG/JPEG → PNG bytes (Lanczos3) |
| `apply_filter(data, expr)` | Apply a filtergraph expression → PNG bytes |
| `blur_image(data, sigma)` | Gaussian blur → PNG bytes |
| `grayscale_image(data)` | Grayscale → PNG bytes |
| `probe_image(data)` | Return JSON metadata (`width`, `height`, `format`) |
| `decode_gif_frames(data)` | Return packed PNG frames from an animated GIF |

**Build**:
```sh
# Install wasm-pack
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# Build the WASM package
wasm-pack build core --no-default-features --features wasm --target web
# Output: core/pkg/
```

**Usage in JavaScript**:
```js
import init, { decode_vp8_to_png, resize_image, apply_filter } from './pkg/ferrox_core.js';
await init();

const png = decode_vp8_to_png(vp8Bytes);
const small = resize_image(pngBytes, 320, 240);
const gray = apply_filter(pngBytes, "grayscale");
```

**Limitations**:
- `encode` feature (AV1/WebM muxing via `rav1e`) and C-backed features
  (`vp9`, `h264`, `mp3-encode`, `opus-encode`) are excluded from WASM builds.
- `gpu` feature (wgpu) is not available in WASM without WebGPU backend work.

---

## rav1e + NASM

**Status**: `rav1e` (AV1 encoder) uses hand-written NASM assembly for
performance on x86_64. This requires NASM to be installed at compile time.

**Install**:
```sh
# Linux
apt-get install nasm
# macOS
brew install nasm
# Windows
choco install nasm
```

On ARM64 targets (`aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`), NASM
is not required — rav1e uses portable SIMD intrinsics.
