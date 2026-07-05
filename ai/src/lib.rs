//! `ferrox-ai` — the **optional, model-backed** AI layer for ferrox.
//!
//! This crate is deliberately *outside* `ferrox-core`: the core stays pure-Rust,
//! deterministic and WASM-safe, while every AI capability here is an **extension
//! trait** that operates on core data structures (via `AsRef<Clip>` / core types)
//! and is fulfilled by a swappable backend — on-device (`candle`/ONNX) or cloud.
//!
//! The trait methods are **async** because they call out to models/services. The
//! interfaces are defined and stubbed; the heavy models are **not** implemented
//! yet. A [`StubBackend`] implements every trait by returning
//! [`AiError::NotConfigured`], proving the seam compiles and is usable end-to-end.

use ferrox_core::{AudioTrack, Clip, ColorGrade, Mask};

/// Errors surfaced by AI backends.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// No model/credentials configured for this capability (the stub default).
    #[error("AI backend not configured for this capability")]
    NotConfigured,
    /// The backend (cloud or on-device) failed.
    #[error("AI backend error: {0}")]
    Backend(String),
}

/// Result alias for AI operations.
pub type Result<T> = std::result::Result<T, AiError>;

/// A color grade keyed to a timeline time (seconds) — the output of auto color.
pub type ColorKeyframe = (f64, ColorGrade);

/// A mask keyed to a timeline time (seconds) — one frame of an animated roto.
pub type MaskKeyframe = (f64, Mask);

/// Match the color of a `source` clip to a `target` look, returning animated
/// [`ColorGrade`] keyframes to apply to the source.
pub trait AutoColorMatch {
    fn match_color(
        &self,
        source: &(impl AsRef<Clip> + Sync),
        target: &(impl AsRef<Clip> + Sync),
    ) -> impl std::future::Future<Output = Result<Vec<ColorKeyframe>>> + Send;
}

/// Isolate speech from an audio track, returning a spectral [`Mask`] describing
/// the frequency regions to attenuate as background noise.
pub trait SmartVoiceIsolation {
    fn isolate_voice(
        &self,
        track: &AudioTrack,
    ) -> impl std::future::Future<Output = Result<Mask>> + Send;
}

/// Rotoscope a subject in a video clip from a text `prompt`, returning a mask
/// animated across the clip's timeline (one [`MaskKeyframe`] per sampled time).
pub trait Rotoscoping {
    fn rotoscope(
        &self,
        clip: &(impl AsRef<Clip> + Sync),
        prompt: &str,
    ) -> impl std::future::Future<Output = Result<Vec<MaskKeyframe>>> + Send;
}

/// Generate a video clip from a text `prompt` (AIGC, effectively cloud-backed).
pub trait TextToVideo {
    fn generate_video(
        &self,
        prompt: &str,
        seconds: f64,
    ) -> impl std::future::Future<Output = Result<Clip>> + Send;
}

/// Synthesize a spoken-voiceover audio track from `text`.
pub trait Voiceover {
    fn synthesize(
        &self,
        text: &str,
        voice: &str,
    ) -> impl std::future::Future<Output = Result<AudioTrack>> + Send;
}

/// A no-op backend: every capability returns [`AiError::NotConfigured`]. Serves
/// as the default and as a compile-time proof that the trait seam is coherent.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubBackend;

impl AutoColorMatch for StubBackend {
    async fn match_color(
        &self,
        _source: &(impl AsRef<Clip> + Sync),
        _target: &(impl AsRef<Clip> + Sync),
    ) -> Result<Vec<ColorKeyframe>> {
        Err(AiError::NotConfigured)
    }
}

impl SmartVoiceIsolation for StubBackend {
    async fn isolate_voice(&self, _track: &AudioTrack) -> Result<Mask> {
        Err(AiError::NotConfigured)
    }
}

impl Rotoscoping for StubBackend {
    async fn rotoscope(
        &self,
        _clip: &(impl AsRef<Clip> + Sync),
        _prompt: &str,
    ) -> Result<Vec<MaskKeyframe>> {
        Err(AiError::NotConfigured)
    }
}

impl TextToVideo for StubBackend {
    async fn generate_video(&self, _prompt: &str, _seconds: f64) -> Result<Clip> {
        Err(AiError::NotConfigured)
    }
}

impl Voiceover for StubBackend {
    async fn synthesize(&self, _text: &str, _voice: &str) -> Result<AudioTrack> {
        Err(AiError::NotConfigured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrox_core::{AudioTrack, Clip, ClipSource, Transform};

    /// A newtype proving the `AsRef<Clip>` seam works for callers' own wrappers.
    struct MyClip(Clip);
    impl AsRef<Clip> for MyClip {
        fn as_ref(&self) -> &Clip {
            &self.0
        }
    }

    fn sample_clip() -> Clip {
        Clip::new(ClipSource::Solid { width: 4, height: 4, r: 1, g: 2, b: 3, a: 255 }, 0.0, 1.0, Transform::default())
    }

    #[test]
    fn stub_backend_reports_not_configured() {
        let ai = StubBackend;
        let src = MyClip(sample_clip());
        let dst = MyClip(sample_clip());

        let color = pollster::block_on(ai.match_color(&src, &dst));
        assert!(matches!(color, Err(AiError::NotConfigured)));

        let roto = pollster::block_on(ai.rotoscope(&src, "the person"));
        assert!(matches!(roto, Err(AiError::NotConfigured)));

        let voice = pollster::block_on(ai.isolate_voice(&AudioTrack::new()));
        assert!(matches!(voice, Err(AiError::NotConfigured)));

        let gen = pollster::block_on(ai.generate_video("a sunset", 5.0));
        assert!(matches!(gen, Err(AiError::NotConfigured)));

        let vo = pollster::block_on(ai.synthesize("hello", "narrator"));
        assert!(matches!(vo, Err(AiError::NotConfigured)));
    }
}
