//! [`Transport`] — the pure playback state machine (playhead + clock). The host
//! advances it by feeding elapsed wall-clock time; it applies speed, direction,
//! looping, and end handling. WASM-safe and deterministic (no threads/`Instant`).
//!
//! Variable-timestep [`Transport::advance`] doubles as the **frame scheduler**:
//! a host that falls behind passes a larger `elapsed` and the playhead jumps
//! forward, implicitly skipping intermediate frames.

/// Playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

/// A playhead over a timeline of `duration` seconds at `fps`.
#[derive(Debug, Clone, Copy)]
pub struct Transport {
    position: f64,
    duration: f64,
    fps: f64,
    speed: f64,
    reversed: bool,
    looping: bool,
    state: PlayState,
}

impl Transport {
    pub fn new(duration: f64, fps: f64) -> Self {
        Self {
            position: 0.0,
            duration: duration.max(0.0),
            fps: fps.max(1.0),
            speed: 1.0,
            reversed: false,
            looping: false,
            state: PlayState::Stopped,
        }
    }

    // ── transport controls ──────────────────────────────────────────────────

    pub fn play(&mut self) {
        self.state = PlayState::Playing;
    }
    pub fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Paused;
        }
    }
    /// Stop and rewind to the start.
    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
        self.position = 0.0;
    }

    /// Jump to `t` seconds (clamped). Also used for scrubbing.
    pub fn seek(&mut self, t: f64) {
        self.position = t.clamp(0.0, self.duration);
    }

    /// Step by whole frames (positive = forward). Loops or clamps at ends.
    pub fn step(&mut self, frames: i32) {
        let pos = self.position + frames as f64 / self.fps;
        self.position = self.wrap_or_clamp(pos).0;
    }

    // ── configuration ───────────────────────────────────────────────────────

    /// Set playback rate (0 = frozen; > 1 fast, < 1 slow). Negative is ignored
    /// (use [`Transport::set_reversed`] for direction).
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.max(0.0);
    }
    pub fn set_reversed(&mut self, reversed: bool) {
        self.reversed = reversed;
    }
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    // ── queries ─────────────────────────────────────────────────────────────

    pub fn position(&self) -> f64 {
        self.position
    }
    pub fn duration(&self) -> f64 {
        self.duration
    }
    pub fn state(&self) -> PlayState {
        self.state
    }
    pub fn is_playing(&self) -> bool {
        self.state == PlayState::Playing
    }
    pub fn speed(&self) -> f64 {
        self.speed
    }
    /// The current frame index (`position × fps`).
    pub fn current_frame(&self) -> u64 {
        (self.position * self.fps).round() as u64
    }

    /// Advance the playhead by `elapsed_secs` of wall-clock time. Returns the new
    /// position. No-op unless playing. Handles speed, reverse, loop, and pausing
    /// at the ends.
    pub fn advance(&mut self, elapsed_secs: f64) -> f64 {
        if self.state != PlayState::Playing || elapsed_secs <= 0.0 || self.duration <= 0.0 {
            return self.position;
        }
        let dir = if self.reversed { -1.0 } else { 1.0 };
        let pos = self.position + elapsed_secs * self.speed * dir;
        let (clamped, hit_end) = self.wrap_or_clamp(pos);
        self.position = clamped;
        if hit_end && !self.looping {
            self.state = PlayState::Paused;
        }
        self.position
    }

    /// Wrap (looping) or clamp (not) a raw position into range. Returns the
    /// resolved position and whether an end was hit (for non-looping pause).
    fn wrap_or_clamp(&self, pos: f64) -> (f64, bool) {
        if self.looping {
            (pos.rem_euclid(self.duration), false)
        } else if pos >= self.duration {
            (self.duration, true)
        } else if pos <= 0.0 {
            (0.0, true)
        } else {
            (pos, false)
        }
    }
}
