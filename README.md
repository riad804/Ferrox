# ferrox

A pure-Rust media processing pipeline — images, audio, video, GIFs, HLS, GPU filters, WASM bindings, and an Axum HTTP service.

[![CI](https://github.com/YOUR_ORG/ferrox/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_ORG/ferrox/actions/workflows/ci.yml)

## Features

- **Image** — PNG/JPEG encode, decode, resize, and a rich filter library
- **Audio** — WAV/MP3/AAC/FLAC/OGG/Opus decode (symphonia, pure Rust); WAV encode; MP3 encode (`mp3-encode`); Opus/Ogg encode (`opus-encode`); volume, resample
- **Video** — WebM/MKV/MP4 demux; VP8 decode (pure Rust); VP9 decode 8/10/12-bit HDR (`vp9`); H.264 Baseline–High Profile decode (`h264`); AV1 encode (rav1e)
- **Container muxing** — WebM, fragmented MP4 (ISO 14496-12), MPEG-TS (ISO 13818-1) — all pure Rust, no C deps
- **HLS** — fMP4 segments (default, iOS ≥ 10 + all Android) or MPEG-TS (legacy, iOS < 10) or WebM; `#EXT-X-MAP` init segment; M3U8 v3/v6
- **Filters** — blur, crop, rotate, flip, brightness, contrast, saturation, negate, grayscale, thumbnail, pad, overlay, text overlay (ab_glyph)
- **FilterGraph** — named-pad filter DAG with FFmpeg-style expression parsing
- **GIF** — animated GIF decode and encode with NeuQuant palette quantisation
- **GPU filters** — `wgpu`-backed ResizeGpu + BlurGpu with WGSL compute shaders (`gpu`, CPU fallback on headless)
- **SIMD** — `wide`-crate brightness/contrast pixel ops (`simd`)
- **WASM** — all pure-Rust features compile to `wasm32`; JS bindings via `wasm-bindgen` (`wasm` feature)
- **HTTP service** — Axum-based `ferrox-service` for remote media processing
- **Docker** — multi-stage `Dockerfile` for the CLI + service
- **Fuzzing** — `cargo-fuzz` targets for PNG decoder, MP4 demuxer, M3U8 parser, MP3 decoder

## Quick start

```sh
cargo build --release
./target/release/ferrox --help
```

## CLI reference

### probe

Print stream metadata as JSON.

```sh
ferrox probe input.mp4
ferrox probe input.png
ferrox probe input.webm --compact
```

Output:
```json
{
  "filename": "input.mp4",
  "format": "mp4",
  "streams": [
    {
      "index": 0,
      "kind": "video",
      "codec": "H264",
      "width": 1920,
      "height": 1080,
      "frame_rate": 30.0
    }
  ]
}
```

### convert

Convert between PNG and JPEG.

```sh
ferrox convert input.png output.jpg
ferrox convert input.jpg output.png
```

### resize

Resize an image (format auto-detected from extension).

```sh
ferrox resize input.jpg output.jpg -W 640 -H 480
```

### filter-apply

Apply a filtergraph expression to an image.

```sh
ferrox filter-apply input.png output.png --filter-complex "scale=640:480,blur=2.0,grayscale"
ferrox filter-apply input.jpg output.jpg -f "brightness=30,contrast=1.2,saturation=0.8"
```

See [docs/filters.md](docs/filters.md) for all supported tokens.

### audio-convert / audio-volume / audio-resample

```sh
ferrox audio-convert input.mp3 output.wav
ferrox audio-volume input.wav output.wav --gain 0.5
ferrox audio-resample input.wav output.wav -r 44100
```

### video-extract-frames

Extract decoded video frames as PNG files.

```sh
ferrox video-extract-frames input.webm frame_%03d.png --count 10
ferrox video-extract-frames input.webm frame_%03d.png --start-frame 5 --count 20
```

### video-extract-audio

Extract audio track to WAV.

```sh
ferrox video-extract-audio input.webm audio.wav
```

### transcode

Re-encode a video with AV1 (rav1e).

```sh
ferrox transcode input.webm output.webm --codec av1 --resize 1280x720
ferrox transcode input.mp4  output.webm --speed 4 --quantizer 80
```

### gif-decode / gif-encode

```sh
ferrox gif-decode animation.gif frames/frame_%03d.png
ferrox gif-encode frame_001.png frame_002.png frame_003.png -o out.gif --delay 8 --palette 128
```

## HLS segmentation

```sh
# fMP4 segments (default — broadest device support)
ferrox hls-segment input.webm --out-dir hls_out --format fmp4

# MPEG-TS segments (legacy players, iOS < 10)
ferrox hls-segment input.webm --out-dir hls_out --format ts

# WebM segments (modern browsers only)
ferrox hls-segment input.webm --out-dir hls_out --format webm
```

Produces `hls_out/seginit.mp4` (init segment, fMP4 only), `hls_out/seg000.mp4`, … and `hls_out/index.m3u8`.

## WASM / JavaScript

All pure-Rust features compile to `wasm32`.  The `wasm` feature adds `wasm-bindgen` JS bindings:

```sh
wasm-pack build core --no-default-features --features wasm --target web
```

```js
import init, { decode_vp8_to_png, resize_image, apply_filter, probe_image } from './pkg/ferrox_core.js';
await init();

const png   = decode_vp8_to_png(vp8Bytes);       // VP8 keyframe → PNG
const small = resize_image(pngBytes, 320, 240);   // resize → PNG
const gray  = apply_filter(pngBytes, "grayscale"); // filtergraph → PNG
const meta  = JSON.parse(probe_image(pngBytes));  // { width, height, format }
```

## HTTP service

`ferrox-service` exposes three endpoints:

| Method | Path       | Description                                    |
|--------|------------|------------------------------------------------|
| GET    | /health    | Liveness probe                                 |
| POST   | /probe     | Probe an uploaded image; returns stream JSON   |
| POST   | /process   | Upload an image + job JSON; returns result     |

### Running

```sh
ferrox-service --addr 0.0.0.0:8080
# or
FERROX_ADDR=0.0.0.0:8080 FERROX_LOG=debug ferrox-service
```

### POST /probe

```sh
curl -s -X POST http://localhost:8080/probe \
  -F "file=@input.png" | jq .
```

### POST /process

The `job` field is a JSON object:

```json
{
  "output_format": "png",
  "filter_complex": "blur=2.0,grayscale"
}
```

```sh
curl -s -X POST http://localhost:8080/process \
  -F "file=@input.png" \
  -F 'job={"output_format":"jpg","filter_complex":"scale=320:240,brightness=10"}' \
  -o output.jpg
```

## Docker

```sh
# Build image
docker build -t ferrox .

# Run the HTTP service
docker run --rm -p 8080:8080 ferrox

# Or run the CLI
docker run --rm -v "$PWD:/data" ferrox-service --entrypoint ferrox probe /data/input.mp4
```

## Architecture

```
ferrox/
├── core/        ferrox-core library (codecs, filters, filter graph, GIF, HLS, WASM bindings)
├── cli/         ferrox CLI binary
├── service/     ferrox-service HTTP binary (Axum)
├── Dockerfile   Multi-stage build (builder → debian-slim runtime)
└── docs/
    ├── filters.md       Filter token reference
    └── limitations.md   Honest accounting of known limitations
```

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `image-codecs`  | ✅ | PNG/JPEG decode + encode |
| `audio-codecs`  | ✅ | WAV/MP3/FLAC/OGG/AAC/Opus decode |
| `video-codecs`  | ✅ | WebM/MKV/MP4 demux, VP8 decode |
| `encode`        | ✅ | AV1 encode + WebM/fMP4/MPEG-TS mux |
| `filters-extra` | ✅ | Text overlay via `ab_glyph` |
| `gif-support`   | ✅ | Animated GIF decode + encode |
| `gpu`           | ❌ | wgpu GPU filters (ResizeGpu, BlurGpu) |
| `simd`          | ❌ | SIMD pixel ops via `wide` |
| `wasm`          | ❌ | wasm-bindgen JS API (for `wasm32` targets) |
| `vp9`           | ❌ | VP9 decode 8/10/12-bit via `libdav1d` (C, BSD-2) |
| `h264`          | ❌ | H.264 Baseline–High decode via OpenH264 (C, BSD-2) |
| `mp3-encode`    | ❌ | MP3 encode via `libmp3lame` (C, LGPL) |
| `opus-encode`   | ❌ | Opus/Ogg encode via `libopus` (C, BSD-3) |

### Installing C library dependencies

| Feature | Linux | macOS | Windows |
|---------|-------|-------|---------|
| `vp9` | `apt install libdav1d-dev` | `brew install dav1d` | `vcpkg install dav1d` |
| `h264` | `apt install libopenh264-dev` | `brew install openh264` | `vcpkg install openh264` |
| `mp3-encode` | `apt install libmp3lame-dev` | `brew install lame` | `vcpkg install mp3lame` |
| `opus-encode` | `apt install libopus-dev` | `brew install opus` | `vcpkg install opus` |

## Video codec support matrix

| Codec | Decode | Encode | Feature flag | Notes |
|-------|--------|--------|--------------|-------|
| VP8   | ✅ pure Rust | — | `video-codecs` (default) | Keyframes only |
| VP9   | ✅ C (libdav1d) | — | `vp9` | 8/10/12-bit, I420/I422/I444 |
| H.264 | ✅ C (OpenH264) | — | `h264` | Baseline, Main, High profiles |
| AV1   | — | ✅ pure Rust (rav1e) | `encode` (default) | |

## Pixel format support

| `PixelFormat` | Bits | Chroma | Source |
|---|---|---|---|
| `Rgb8` | 8 | — | All decoders output or convert to this |
| `Yuv420p` | 8 | 4:2:0 | VP8, VP9, H.264 (YUV output mode) |
| `Yuv420p10` | 10 | 4:2:0 | VP9 HDR (10-bit) |
| `Yuv420p12` | 12 | 4:2:0 | VP9 HDR (12-bit) |
| `Yuv422p` | 8 | 4:2:2 | VP9 |
| `Yuv444p` | 8 | 4:4:4 | VP9 |

HDR frames can be tone-mapped to RGB8 via `yuv420p_hdr_to_rgb8()`.

## Security

```sh
# Check advisories + licences + banned crates
cargo deny check

# Vulnerability scan
cargo audit
```

## Fuzzing

```sh
# Install cargo-fuzz (nightly required)
cargo +nightly fuzz run fuzz_png_decoder
cargo +nightly fuzz run fuzz_mp4_demuxer
cargo +nightly fuzz run fuzz_m3u8_parser
cargo +nightly fuzz run fuzz_mp3_decoder
```

## Contributing

1. Fork → branch → PR against `main`.
2. `cargo test --workspace` must pass.
3. `cargo clippy -- -D warnings` must be clean.
4. For new filters: add a unit test in `core/tests/`.
5. For new codecs: add a fuzz target in `fuzz/src/`.

## License

MIT
