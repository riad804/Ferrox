Here is my analysis of every limitation and the concrete plan to eliminate each one, phase by phase. I will ask you to approve before I start any phase.

Current limitations mapped to solutions
Phase 7 — Audio encoding (MP3 + Opus)
Limitation	Solution	Crate	License
MP3 encoding	mp3lame-encoder — safe Rust wrapper around libmp3lame	mp3lame-encoder 0.2	LGPL (opt-in feature mp3-encode)
Opus encoding	opus Rust crate wraps libopus; audiopus is a pure-safe wrapper	audiopus 0.3	BSD-3 (opt-in feature opus-encode)
Both are C-backed, so they go behind feature flags like vp9/h264. The default build stays pure-Rust.

Phase 8 — Container muxing (MPEG-TS + fMP4)
Limitation	Solution
MPEG-TS muxer	Implement a minimal pure-Rust MPEG-TS muxer (H.264/AAC PAT+PMT+PES framing — ~600 lines). No crate needed.
fMP4 muxer	mp4 crate (already a dep for demuxing) has write support in v0.14; alternatively implement minimal moov/moof/mdat atoms (~800 lines pure Rust).
Both are pure Rust — no new C dependencies.

Phase 9 — HLS upgrade (fMP4 segments + full compatibility)
Once fMP4 muxer exists: switch HLS from WebM segments to fMP4 segments + #EXT-X-MAP init segment. This makes HLS compatible with iOS < 10 and all Android players (not just HLS v6+).

Phase 10 — GPU on all platforms / WASM video
Limitation	Solution
GPU unavailable in Docker/CI	Already gracefully falls back — nothing to fix.
WASM video codecs excluded	Compile VP8 decoder to WASM by replacing oxideav-vp8 (uses OS threads) with rav1d WASM build or a pure-Rust VP8 subset for WASM target.
Phase 11 — HDR VP9 (10/12-bit) + H.264 High Profile
Limitation	Solution
VP9 8-bit only	libdav1d fully supports 10/12-bit; remove the guard and add a Yuv420p10 pixel format variant.
H.264 High Profile gaps	OpenH264 does support High Profile in 2.x releases; the limitation is overstated. Remove the caveat after testing.
Execution order

Phase 7  → mp3-encode + opus-encode features         (1–2 sessions)
Phase 8  → MPEG-TS muxer + fMP4 muxer               (2–3 sessions)
Phase 9  → HLS upgrade to fMP4 segments              (1 session)
Phase 10 → WASM video codec support                  (1–2 sessions)
Phase 11 → HDR VP9 + H.264 High Profile polish       (1 session)
Each phase results in a commit. After Phase 11, the limitations doc will be empty (or contain only honest notes like "MPEG-TS on WASM requires WASI threads").