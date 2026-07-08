//! [`PlaybackController`] — ties the [`Transport`] to the renderer: render the
//! current frame at a chosen [`RenderProfile`] (typically adaptive preview),
//! and mix the audio window for A/V sync. The host owns the loop (native thread
//! or the browser's animation frame) and feeds elapsed time.

use crate::audio::mixer::mix;
use crate::audio::AudioFrame;
use crate::color::Lut3D;
use crate::error::Result;
use crate::frame::Frame;
use crate::render::RenderProfile;
use crate::timeline::Project;

use super::transport::Transport;

/// Drives playback of a project: advance the playhead, render the current frame,
/// and mix synchronised audio.
pub struct PlaybackController {
    transport: Transport,
}

impl PlaybackController {
    /// A controller for `project` (duration = the later of video/audio ends).
    pub fn for_project(project: &Project) -> Self {
        let duration = project.duration().max(project.audio_duration());
        Self { transport: Transport::new(duration, project.fps) }
    }

    /// Mutable access to the transport (play/pause/seek/speed/…).
    pub fn transport(&mut self) -> &mut Transport {
        &mut self.transport
    }

    /// The current playhead position (seconds).
    pub fn position(&self) -> f64 {
        self.transport.position()
    }

    /// Advance the playhead by `elapsed_secs` and return the new position.
    pub fn advance(&mut self, elapsed_secs: f64) -> f64 {
        self.transport.advance(elapsed_secs)
    }

    /// Render the frame at the current playhead using `profile`.
    pub fn render(&self, project: &Project, profile: &RenderProfile, output_lut: Option<&Lut3D>) -> Result<Frame> {
        crate::render::preview::render(project, self.transport.position(), profile, output_lut)
    }

    /// Mix the audio for the window `[from, to)` seconds (A/V sync: the host
    /// mixes the same window the playhead advanced over).
    pub fn mix_audio(&self, project: &Project, from: f64, to: f64) -> Result<AudioFrame> {
        mix(project, from.min(to), from.max(to))
    }
}
