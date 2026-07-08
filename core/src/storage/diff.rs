//! Structural diff between two [`Project`] states.
//!
//! Tracks and clips are **positional** (no stable ids in the model), so the diff
//! is index-based: it reports canvas/format changes, track additions/removals,
//! and per-index clip changes. This is enough to power "unsaved changes"
//! indicators, autosave-worthiness checks, and a compact change log; it is not a
//! move-detecting merge algorithm.

use crate::timeline::Project;

/// A single detected change between two projects.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A top-level canvas/format field changed (e.g. `"width"`, `"fps"`).
    Field(&'static str),
    /// A visual track was added at `index` (only present in the new project).
    TrackAdded(usize),
    /// A visual track at `index` was removed (only present in the old project).
    TrackRemoved(usize),
    /// The clip count of the visual track at `index` changed `old` → `new`.
    TrackClipCount { index: usize, old: usize, new: usize },
    /// The clip at `track`/`clip` differs between the two projects.
    ClipChanged { track: usize, clip: usize },
    /// An audio track was added at `index`.
    AudioTrackAdded(usize),
    /// An audio track at `index` was removed.
    AudioTrackRemoved(usize),
    /// The audio track at `index` changed (clips or track params).
    AudioTrackChanged(usize),
}

/// The complete set of changes from `old` to `new`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectDiff {
    pub changes: Vec<Change>,
}

impl ProjectDiff {
    /// `true` when the two projects are structurally identical.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Number of detected changes.
    pub fn len(&self) -> usize {
        self.changes.len()
    }
}

/// Compute the structural diff from `old` to `new`.
pub fn diff(old: &Project, new: &Project) -> ProjectDiff {
    let mut changes = Vec::new();
    diff_fields(old, new, &mut changes);
    diff_tracks(old, new, &mut changes);
    diff_audio(old, new, &mut changes);
    ProjectDiff { changes }
}

fn diff_fields(old: &Project, new: &Project, out: &mut Vec<Change>) {
    if old.width != new.width {
        out.push(Change::Field("width"));
    }
    if old.height != new.height {
        out.push(Change::Field("height"));
    }
    if old.fps != new.fps {
        out.push(Change::Field("fps"));
    }
    if old.background != new.background {
        out.push(Change::Field("background"));
    }
    if old.sample_rate != new.sample_rate {
        out.push(Change::Field("sample_rate"));
    }
    if old.channels != new.channels {
        out.push(Change::Field("channels"));
    }
}

fn diff_tracks(old: &Project, new: &Project, out: &mut Vec<Change>) {
    let common = old.tracks.len().min(new.tracks.len());
    for i in 0..common {
        let (ot, nt) = (&old.tracks[i], &new.tracks[i]);
        if ot == nt {
            continue;
        }
        if ot.clips.len() != nt.clips.len() {
            out.push(Change::TrackClipCount { index: i, old: ot.clips.len(), new: nt.clips.len() });
        }
        let cc = ot.clips.len().min(nt.clips.len());
        for c in 0..cc {
            if ot.clips[c] != nt.clips[c] {
                out.push(Change::ClipChanged { track: i, clip: c });
            }
        }
    }
    for i in common..new.tracks.len() {
        out.push(Change::TrackAdded(i));
    }
    for i in common..old.tracks.len() {
        out.push(Change::TrackRemoved(i));
    }
}

fn diff_audio(old: &Project, new: &Project, out: &mut Vec<Change>) {
    let common = old.audio_tracks.len().min(new.audio_tracks.len());
    for i in 0..common {
        if old.audio_tracks[i] != new.audio_tracks[i] {
            out.push(Change::AudioTrackChanged(i));
        }
    }
    for i in common..new.audio_tracks.len() {
        out.push(Change::AudioTrackAdded(i));
    }
    for i in common..old.audio_tracks.len() {
        out.push(Change::AudioTrackRemoved(i));
    }
}
