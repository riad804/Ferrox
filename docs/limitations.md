# Known Limitations

This document is an honest accounting of ferrox's current limitations. Where
a limitation is architectural, the long-term plan is described.

---

## Video decoding

### VP9 — no pixel decoder

**Status**: Not implemented.

**Why**: There is no complete, production-quality pure-Rust VP9 pixel decoder
available on crates.io as of 2026. `oxideav-vp9` exists but describes itself
as a "scaffold pending clean-room re-implementation" and has no usable decode
path. Writing a spec-compliant VP9 decoder is a 3–12 month engineering effort.

**Workaround**: Use VP8 source video (fully supported via `oxideav-vp8`), or
transcode to VP8 with ffmpeg before processing.

**Long-term plan**: Track `oxideav-vp9` for maturity; evaluate `dav1d`
pure-Rust port proposals.

---

### H.264 — no pixel decoder

**Status**: Not implemented.

**Why**: `h264-reader` (crates.io) parses NAL syntax but provides no inverse
DCT, motion compensation, or deblocking filter — it is a bitstream parser, not
a decoder. A baseline-profile H.264 decoder requires implementing:

- Entropy coding (CAVLC / CABAC)
- Inverse transforms (4×4 and 8×8 DCT)
- Intra/inter prediction
- In-loop deblocking filter

This is a substantial, well-specified but time-intensive project.

**Workaround**: Convert H.264 source to VP8 with ffmpeg:
```sh
ffmpeg -i input.mp4 -c:v libvpx output.webm
```

**Long-term plan**: Monitor `openh264-rs` and community pure-Rust H.264
efforts. A baseline-profile-only decoder covering most web video is feasible.

---

## Audio encoding

### MP3 encoding — not implemented

**Status**: Encoding to MP3 is not supported. Only WAV output is currently
available for audio encoding.

**Why**: There is no pure-Rust MP3 encoder on crates.io. The dominant MP3
encoder (`lame`) is LGPL C code.

**Workaround**: Encode to WAV and use an external tool to re-encode to MP3.

**Long-term plan**: `minimp3` (C) could be used with explicit licensing
acceptance; alternatively, an open-source pure-Rust encoder could be developed.

### Opus encoding — not implemented

**Status**: Decoding Opus (via symphonia) is supported. Encoding is not.

**Why**: `opus-encoder` on crates.io wraps `libopus` (C). No pure-Rust Opus
encoder exists yet.

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
