//! Subtitle data model: [`Subtitle`] → [`Cue`]s, with optional per-syllable
//! [`KaraokeSegment`] timing for word highlighting / karaoke.

use serde::{Deserialize, Serialize};

/// One timed syllable/word within a cue (from ASS `\k` tags), relative to the
/// cue's start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KaraokeSegment {
    pub text: String,
    /// Seconds after the cue start when this segment begins highlighting.
    pub start: f64,
    /// Highlight duration in seconds.
    pub duration: f64,
}

/// A single subtitle cue: text shown over `[start, end)` seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    /// Plain display text (override tags stripped).
    pub text: String,
    /// Karaoke timing; empty for plain cues.
    #[serde(default)]
    pub segments: Vec<KaraokeSegment>,
}

impl Cue {
    pub fn new(start: f64, end: f64, text: impl Into<String>) -> Self {
        Self { start, end, text: text.into(), segments: Vec::new() }
    }

    pub fn is_active(&self, t: f64) -> bool {
        t >= self.start && t < self.end
    }

    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// The number of leading **characters** to highlight at time `t` (karaoke).
    /// Fully-elapsed segments contribute all their chars; the in-progress one
    /// contributes proportionally. Returns 0 when there is no karaoke timing.
    pub fn highlighted_chars(&self, t: f64) -> usize {
        if self.segments.is_empty() {
            return 0;
        }
        let elapsed = t - self.start;
        let mut chars = 0usize;
        for seg in &self.segments {
            let n = seg.text.chars().count();
            if elapsed >= seg.start + seg.duration {
                chars += n; // fully highlighted
            } else if elapsed > seg.start && seg.duration > 0.0 {
                let frac = ((elapsed - seg.start) / seg.duration).clamp(0.0, 1.0);
                chars += (n as f64 * frac).floor() as usize;
                break;
            } else {
                break;
            }
        }
        chars
    }
}

/// A parsed subtitle track.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Subtitle {
    pub cues: Vec<Cue>,
}

impl Subtitle {
    pub fn new(cues: Vec<Cue>) -> Self {
        Self { cues }
    }

    /// All cues active at time `t`.
    pub fn active_cues(&self, t: f64) -> Vec<&Cue> {
        self.cues.iter().filter(|c| c.is_active(t)).collect()
    }

    pub fn len(&self) -> usize {
        self.cues.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }
}
