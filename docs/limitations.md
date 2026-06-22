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

**Limitation**: 8-bit YUV420 only. 10/12-bit HDR VP9 profiles return an error.

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

**Limitation**: OpenH264 supports Baseline and Main profiles; High-profile
features (8×8 DCT, B-frames in some modes) may not decode correctly.

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

### TS / MPEG-TS muxer — not implemented

**Status**: HLS segments are written as WebM files, not MPEG-TS (`.ts`).

**Why**: MPEG-TS muxing is not currently implemented in Rust without C bindings.
HLS v6+ supports WebM/fMP4 segments with `#EXT-X-MAP`, but older HLS players
(iOS < 10, some Android devices) require MPEG-TS.

**Workaround**: Use ffmpeg to re-segment WebM HLS output into TS:
```sh
ffmpeg -i index.m3u8 -c copy -hls_segment_type mpegts output.m3u8
```

### fMP4 (fragmented MP4) muxer — not implemented

**Status**: Output is WebM. MP4 output requires an fMP4 muxer.

**Long-term plan**: Implement a minimal fMP4 muxer (ISO 14496-12) for
broader device compatibility.

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

### Video codecs excluded from WASM builds

**Status**: `video-codecs` and `encode` features cannot be compiled to
`wasm32-wasi` because `oxideav-vp8` and `rav1e` depend on platform-specific
assembly optimisations or OS threading primitives.

**Supported in WASM**: PNG/JPEG image codecs, all audio decoders, all image
filters, GIF encode/decode, filter graph.

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
