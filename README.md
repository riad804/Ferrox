# ferrox

A pure-Rust media processing pipeline — images, audio, video, GIFs, HLS, GPU filters, and an Axum HTTP service.

[![CI](https://github.com/YOUR_ORG/ferrox/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_ORG/ferrox/actions/workflows/ci.yml)

## Features

- **Image** — PNG/JPEG encode, decode, resize, and a rich filter library
- **Audio** — WAV/MP3/AAC/FLAC/OGG/Opus decode (symphonia, pure Rust); WAV encode; MP3 encode (`mp3-encode`); Opus/Ogg encode (`opus-encode`); volume, resample
- **Video** — WebM/MKV/MP4 demux; VP8 decode; AV1 encode (rav1e); frame extraction
- **Filters** — blur, crop, rotate, flip, brightness, contrast, saturation, negate, grayscale, thumbnail, pad, overlay, text overlay (ab_glyph)
- **HLS** — segment video into WebM HLS segments + M3U8 playlist (`ferrox_core::hls_segment`)
- **GPU filters** — `wgpu`-backed ResizeGpu + BlurGpu with WGSL compute shaders (feature `gpu`, CPU fallback on headless)
- **SIMD** — `wide`-crate brightness/contrast pixel ops (feature `simd`)
- **Fuzzing** — `cargo-fuzz` targets for PNG decoder, MP4 demuxer, M3U8 parser, MP3 decoder
- **GIF** — animated GIF decode and encode with NeuQuant palette quantisation
- **FilterGraph** — named-pad filter DAG with FFmpeg-style expression parsing
- **HTTP service** — Axum-based `ferrox-service` for remote media processing
- **Docker** — multi-stage `Dockerfile` for the CLI + service

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
├── core/        ferrox-core library (codecs, filters, filter graph, GIF)
├── cli/         ferrox CLI binary
├── service/     ferrox-service HTTP binary (Axum)
├── Dockerfile   Multi-stage build (builder → debian-slim runtime)
└── docs/
    └── filters.md   Filter token reference
```

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `image-codecs` | ✅ | PNG/JPEG decode + encode |
| `audio-codecs` | ✅ | WAV/MP3/FLAC/OGG/AAC/Opus decode |
| `video-codecs` | ✅ | WebM/MKV/MP4 demux, VP8 decode |
| `encode`       | ✅ | AV1 encode + WebM mux |
| `filters-extra`| ✅ | Text overlay via `ab_glyph` |
| `gif-support`  | ✅ | Animated GIF decode + encode |
| `gpu`          | ❌ | wgpu GPU filters (ResizeGpu, BlurGpu) |
| `simd`         | ❌ | SIMD pixel ops via `wide` |
| `vp9`          | ❌ | VP9 decode via `libdav1d` (C, BSD-2) — requires system `dav1d` |
| `h264`         | ❌ | H.264 decode via OpenH264 (C, BSD-2) — requires system `openh264` |
| `mp3-encode`   | ❌ | MP3 encode via `libmp3lame` (C, LGPL) — requires system `lame` |
| `opus-encode`  | ❌ | Opus encode via `libopus` (C, BSD-3) — requires system `opus` |

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

## Limitations

See [docs/limitations.md](docs/limitations.md) for an honest accounting of
what is not yet supported (VP9/H.264 decoding, MP3 encoding, MPEG-TS muxing).

## Contributing

1. Fork → branch → PR against `main`.
2. `cargo test --workspace` must pass.
3. `cargo clippy -- -D warnings` must be clean.
4. For new filters: add a unit test in `core/tests/`.
5. For new codecs: add a fuzz target in `fuzz/src/`.

## License

MIT
