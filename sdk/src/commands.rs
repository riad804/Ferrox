//! The **command stack** — every mutation of a project goes through a [`Command`]
//! so it can be undone and redone. Each command captures, at `apply` time, the
//! prior state it needs to `revert`, giving exact inverse semantics.

use ferrox_core::{BlendMode, Clip, ColorGrade, Curve, Easing, Keyer, Keyframe, Project, Track};

use crate::error::{Result, SdkError};

/// A reversible mutation of a [`Project`].
pub trait Command: Send + Sync {
    /// Apply the mutation, capturing any state needed to revert it.
    fn apply(&mut self, project: &mut Project) -> Result<()>;
    /// Undo the mutation, restoring the captured prior state.
    fn revert(&mut self, project: &mut Project) -> Result<()>;
    /// A short human-readable name (for UI history / debugging).
    fn name(&self) -> String;
}

/// Which animatable transform field a keyframe command targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimField {
    X,
    Y,
    Scale,
    Opacity,
}

// ── shared accessors ────────────────────────────────────────────────────────

fn track_mut(p: &mut Project, ti: usize) -> Result<&mut Track> {
    p.tracks
        .get_mut(ti)
        .ok_or_else(|| SdkError::InvalidHandle(format!("track {ti}")))
}

fn clip_mut(p: &mut Project, ti: usize, ci: usize) -> Result<&mut Clip> {
    track_mut(p, ti)?
        .clips
        .get_mut(ci)
        .ok_or_else(|| SdkError::InvalidHandle(format!("clip {ci} on track {ti}")))
}

fn anim_field_mut(clip: &mut Clip, field: AnimField) -> &mut Option<Curve> {
    match field {
        AnimField::X => &mut clip.animation.x,
        AnimField::Y => &mut clip.animation.y,
        AnimField::Scale => &mut clip.animation.scale,
        AnimField::Opacity => &mut clip.animation.opacity,
    }
}

// ── track / clip structure ──────────────────────────────────────────────────

/// Append a new (empty) video track.
pub struct AddTrackCommand {
    applied: bool,
}
impl AddTrackCommand {
    pub fn new() -> Self {
        Self { applied: false }
    }
}
impl Default for AddTrackCommand {
    fn default() -> Self {
        Self::new()
    }
}
impl Command for AddTrackCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        p.tracks.push(Track::new());
        self.applied = true;
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        p.tracks.pop();
        Ok(())
    }
    fn name(&self) -> String {
        "AddTrack".into()
    }
}

/// Append a clip to a track.
pub struct AddClipCommand {
    track: usize,
    clip: Clip,
    applied_index: Option<usize>,
}
impl AddClipCommand {
    pub fn new(track: usize, clip: Clip) -> Self {
        Self { track, clip, applied_index: None }
    }
}
impl Command for AddClipCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let t = track_mut(p, self.track)?;
        self.applied_index = Some(t.clips.len());
        t.clips.push(self.clip.clone());
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(i) = self.applied_index.take() {
            track_mut(p, self.track)?.clips.remove(i);
        }
        Ok(())
    }
    fn name(&self) -> String {
        "AddClip".into()
    }
}

/// Remove a clip from a track.
pub struct RemoveClipCommand {
    track: usize,
    index: usize,
    removed: Option<Clip>,
}
impl RemoveClipCommand {
    pub fn new(track: usize, index: usize) -> Self {
        Self { track, index, removed: None }
    }
}
impl Command for RemoveClipCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let t = track_mut(p, self.track)?;
        if self.index >= t.clips.len() {
            return Err(SdkError::InvalidHandle(format!("clip {} on track {}", self.index, self.track)));
        }
        self.removed = Some(t.clips.remove(self.index));
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(clip) = self.removed.take() {
            track_mut(p, self.track)?.clips.insert(self.index, clip);
        }
        Ok(())
    }
    fn name(&self) -> String {
        "RemoveClip".into()
    }
}

/// Move a clip along the timeline (change its `start`).
pub struct MoveClipCommand {
    track: usize,
    index: usize,
    new_start: f64,
    old_start: Option<f64>,
}
impl MoveClipCommand {
    pub fn new(track: usize, index: usize, new_start: f64) -> Self {
        Self { track, index, new_start, old_start: None }
    }
}
impl Command for MoveClipCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        self.old_start = Some(c.start);
        c.start = self.new_start;
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(s) = self.old_start.take() {
            clip_mut(p, self.track, self.index)?.start = s;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "MoveClip".into()
    }
}

/// Trim a clip (change `start` and `duration`).
pub struct TrimClipCommand {
    track: usize,
    index: usize,
    new_start: f64,
    new_duration: f64,
    old: Option<(f64, f64)>,
}
impl TrimClipCommand {
    pub fn new(track: usize, index: usize, new_start: f64, new_duration: f64) -> Self {
        Self { track, index, new_start, new_duration, old: None }
    }
}
impl Command for TrimClipCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        self.old = Some((c.start, c.duration));
        c.start = self.new_start;
        c.duration = self.new_duration;
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some((s, d)) = self.old.take() {
            let c = clip_mut(p, self.track, self.index)?;
            c.start = s;
            c.duration = d;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "TrimClip".into()
    }
}

// ── effect parameters ───────────────────────────────────────────────────────

/// Set a clip's color grade.
pub struct SetColorGradeCommand {
    track: usize,
    index: usize,
    grade: ColorGrade,
    old: Option<ColorGrade>,
}
impl SetColorGradeCommand {
    pub fn new(track: usize, index: usize, grade: ColorGrade) -> Self {
        Self { track, index, grade, old: None }
    }
}
impl Command for SetColorGradeCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        self.old = Some(c.color);
        c.color = self.grade;
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(g) = self.old.take() {
            clip_mut(p, self.track, self.index)?.color = g;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "SetColorGrade".into()
    }
}

/// Set (or clear) a clip's chroma keyer.
pub struct SetKeyerCommand {
    track: usize,
    index: usize,
    keyer: Option<Keyer>,
    old: Option<Option<Keyer>>,
}
impl SetKeyerCommand {
    pub fn new(track: usize, index: usize, keyer: Option<Keyer>) -> Self {
        Self { track, index, keyer, old: None }
    }
}
impl Command for SetKeyerCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        self.old = Some(c.keyer);
        c.keyer = self.keyer;
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(k) = self.old.take() {
            clip_mut(p, self.track, self.index)?.keyer = k;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "SetKeyer".into()
    }
}

/// Set a clip's blend mode.
pub struct SetBlendModeCommand {
    track: usize,
    index: usize,
    blend: BlendMode,
    old: Option<BlendMode>,
}
impl SetBlendModeCommand {
    pub fn new(track: usize, index: usize, blend: BlendMode) -> Self {
        Self { track, index, blend, old: None }
    }
}
impl Command for SetBlendModeCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        self.old = Some(c.blend);
        c.blend = self.blend;
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(b) = self.old.take() {
            clip_mut(p, self.track, self.index)?.blend = b;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "SetBlendMode".into()
    }
}

// ── keyframe animation ──────────────────────────────────────────────────────

/// Add (or overwrite-at-time) a keyframe on one animated transform field.
pub struct AddKeyframeCommand {
    track: usize,
    index: usize,
    field: AnimField,
    t: f64,
    value: f32,
    ease: Easing,
    old: Option<Option<Curve>>,
}
impl AddKeyframeCommand {
    pub fn new(track: usize, index: usize, field: AnimField, t: f64, value: f32, ease: Easing) -> Self {
        Self { track, index, field, t, value, ease, old: None }
    }
}
impl Command for AddKeyframeCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        let field = anim_field_mut(c, self.field);
        self.old = Some(field.clone());
        let mut keys = match field.take() {
            None => Vec::new(),
            Some(Curve::Const(v)) => vec![Keyframe::new(0.0, v)],
            Some(Curve::Keyed(k)) => k.into_iter().filter(|kf| (kf.t - self.t).abs() > 1e-9).collect(),
        };
        keys.push(Keyframe::new(self.t, self.value).with_ease(self.ease));
        *field = Some(Curve::keyed(keys));
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(prev) = self.old.take() {
            let c = clip_mut(p, self.track, self.index)?;
            *anim_field_mut(c, self.field) = prev;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "AddKeyframe".into()
    }
}

/// Remove the keyframe at (approximately) time `t` on a field.
pub struct RemoveKeyframeCommand {
    track: usize,
    index: usize,
    field: AnimField,
    t: f64,
    old: Option<Option<Curve>>,
}
impl RemoveKeyframeCommand {
    pub fn new(track: usize, index: usize, field: AnimField, t: f64) -> Self {
        Self { track, index, field, t, old: None }
    }
}
impl Command for RemoveKeyframeCommand {
    fn apply(&mut self, p: &mut Project) -> Result<()> {
        let c = clip_mut(p, self.track, self.index)?;
        let field = anim_field_mut(c, self.field);
        self.old = Some(field.clone());
        if let Some(Curve::Keyed(k)) = field.take() {
            let kept: Vec<Keyframe> = k.into_iter().filter(|kf| (kf.t - self.t).abs() > 1e-9).collect();
            *field = if kept.is_empty() { None } else { Some(Curve::keyed(kept)) };
        }
        Ok(())
    }
    fn revert(&mut self, p: &mut Project) -> Result<()> {
        if let Some(prev) = self.old.take() {
            let c = clip_mut(p, self.track, self.index)?;
            *anim_field_mut(c, self.field) = prev;
        }
        Ok(())
    }
    fn name(&self) -> String {
        "RemoveKeyframe".into()
    }
}
