# FFmpeg / ImageMagick Migration Guide

This guide shows equivalent `ferrox` commands for common `ffmpeg` and
`magick` (ImageMagick) operations.

---

## Probe / inspect

| Tool | Command |
|------|---------|
| ffprobe | `ffprobe -v quiet -print_format json -show_streams input.mp4` |
| **ferrox** | `ferrox probe input.mp4` |
| **ferrox (compact)** | `ferrox probe input.mp4 --compact` |

---

## Image conversion

| Tool | Command |
|------|---------|
| ffmpeg | `ffmpeg -i input.png output.jpg` |
| magick | `magick input.png output.jpg` |
| **ferrox** | `ferrox convert input.png output.jpg` |

---

## Image resize

| Tool | Command |
|------|---------|
| ffmpeg | `ffmpeg -i input.jpg -vf scale=640:480 output.jpg` |
| magick | `magick input.jpg -resize 640x480! output.jpg` |
| **ferrox** | `ferrox resize input.jpg output.jpg -W 640 -H 480` |

---

## Image filters

### Blur
```sh
# ffmpeg
ffmpeg -i input.png -vf "gblur=sigma=2" output.png

# ferrox
ferrox filter-apply input.png output.png -f "blur=2.0"
```

### Grayscale
```sh
# ffmpeg
ffmpeg -i input.png -vf "hue=s=0" output.png

# magick
magick input.png -colorspace Gray output.png

# ferrox
ferrox filter-apply input.png output.png -f "grayscale"
```

### Brightness / contrast
```sh
# ffmpeg
ffmpeg -i input.png -vf "eq=brightness=0.1:contrast=1.3" output.png

# ferrox (brightness delta 25, contrast factor 1.3)
ferrox filter-apply input.png output.png -f "brightness=25,contrast=1.3"
```

### Chain multiple filters
```sh
# ffmpeg
ffmpeg -i input.png -vf "scale=640:480,gblur=sigma=1.5,hue=s=0" output.png

# ferrox
ferrox filter-apply input.png output.png -f "scale=640:480,blur=1.5,grayscale"
```

---

## Audio conversion

### WAV → WAV (copy)
```sh
# ffmpeg
ffmpeg -i input.wav output.wav

# ferrox
ferrox audio-convert input.wav output.wav
```

### MP3 → WAV
```sh
# ffmpeg
ffmpeg -i input.mp3 output.wav

# ferrox
ferrox audio-convert input.mp3 output.wav
```

### Adjust volume
```sh
# ffmpeg  (0.5 = half volume)
ffmpeg -i input.wav -af "volume=0.5" output.wav

# ferrox
ferrox audio-volume input.wav output.wav --gain 0.5
```

### Resample
```sh
# ffmpeg
ffmpeg -i input.wav -ar 44100 output.wav

# ferrox
ferrox audio-resample input.wav output.wav -r 44100
```

---

## Video frame extraction

```sh
# ffmpeg (10 frames from start)
ffmpeg -i input.webm -vframes 10 frame_%03d.png

# ferrox
ferrox video-extract-frames input.webm frame_%03d.png --count 10

# ffmpeg (frames 5–15)
ffmpeg -i input.webm -vf "select=between(n\,5\,14)" -vframes 10 frame_%03d.png

# ferrox
ferrox video-extract-frames input.webm frame_%03d.png --start-frame 5 --count 10
```

---

## Extract audio from video

```sh
# ffmpeg
ffmpeg -i input.webm -vn -acodec pcm_s16le output.wav

# ferrox
ferrox video-extract-audio input.webm output.wav
```

---

## Transcode video

```sh
# ffmpeg (AV1 via libaom)
ffmpeg -i input.webm -c:v libaom-av1 -crf 30 output.webm

# ferrox (AV1 via rav1e, quantizer 0–255 lower=better)
ferrox transcode input.webm output.webm --codec av1 --quantizer 80

# ffmpeg (resize + transcode)
ffmpeg -i input.webm -vf scale=1280:720 -c:v libaom-av1 output.webm

# ferrox
ferrox transcode input.webm output.webm --resize 1280x720
```

---

## Animated GIF

### Decode GIF to frames
```sh
# ffmpeg
ffmpeg -i animation.gif frame_%03d.png

# ferrox
ferrox gif-decode animation.gif frame_%03d.png
```

### Encode frames to GIF
```sh
# ffmpeg
ffmpeg -framerate 10 -i frame_%03d.png -vf palettegen animation.gif

# ferrox (delay in centiseconds: 10 cs = 100 ms = 10 fps)
ferrox gif-encode frame_*.png -o animation.gif --delay 10
```

---

## HLS segmentation

```sh
# ffmpeg
ffmpeg -i input.webm -c:v libx264 -hls_time 10 -hls_list_size 0 output.m3u8

# ferrox (AV1 segments in WebM containers)
# Via Rust API — CLI flag planned:
# ferrox hls input.webm -o hls_out/ --segment-time 10
```

> **Note**: HLS via CLI is available in the Rust API (`ferrox_core::hls_segment`).
> A `ferrox hls` subcommand is a planned addition.

---

## HTTP service

```sh
# curl: probe a remote image
curl -s -X POST http://localhost:8080/probe \
  -F "file=@input.png" | jq .

# curl: apply filters and download result
curl -s -X POST http://localhost:8080/process \
  -F "file=@input.png" \
  -F 'job={"output_format":"jpg","filter_complex":"scale=320:240,blur=2.0"}' \
  -o output.jpg
```

---

## Feature comparison

| Feature | ffmpeg | ferrox |
|---------|--------|--------|
| PNG/JPEG encode + decode | ✅ | ✅ |
| Image filters | ✅ | ✅ (blur, resize, grayscale, brightness, contrast, saturation, negate, flip, rotate, crop, thumbnail, overlay, pad, text) |
| MP3 decode | ✅ (libmp3lame / C) | ✅ (symphonia / pure Rust) |
| FLAC decode | ✅ | ✅ (claxon / pure Rust) |
| Vorbis decode | ✅ | ✅ (lewton / pure Rust) |
| VP8 decode | ✅ (libvpx / C) | ✅ (oxideav-vp8 / pure Rust) |
| VP9 decode | ✅ (libvpx / C) | ❌ (see limitations.md) |
| H.264 decode | ✅ (openh264 / C) | ❌ (see limitations.md) |
| AV1 encode | ✅ (libaom / C) | ✅ (rav1e / pure Rust) |
| Animated GIF | ✅ | ✅ |
| HLS segmentation | ✅ | ✅ (API) / 🔜 (CLI) |
| GPU acceleration | ✅ (CUDA/OpenCL) | ✅ (wgpu/WGSL, feature = "gpu") |
| WASM support | ❌ | ✅ (image + audio codecs) |
| No C dependencies | ❌ | ✅ (except rav1e NASM) |
