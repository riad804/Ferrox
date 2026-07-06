//! Built-in plugins that adapt existing engine effects to the plugin API —
//! proof that the abstraction wraps real code without rewriting the DSP.

use std::sync::Arc;

use serde_json::Value;

use crate::anim::Easing;
use crate::audio::effects::AudioEffect as CoreAudioEffect;
use crate::audio::AudioFrame;
use crate::color::ColorGrade;
use crate::frame::Frame;
use crate::keyer::Keyer;
use crate::mask::Mask;
use crate::timeline::ClipAnimation;
use crate::transitions::{Direction, Transition};

use super::error::{PluginError, Result};
use super::kind::PluginKind;
use super::manager::PluginManager;
use super::metadata::{PluginMetadata, Version};
use super::traits::{AudioEffectPlugin, Plugin, TransitionPlugin, VideoEffectPlugin};
use super::PLUGIN_API_VERSION;

/// Shorthand: build v1.0.0 metadata for a built-in plugin targeting the current API.
fn builtin_meta(id: &str, name: &str, kind: PluginKind, description: &str) -> PluginMetadata {
    PluginMetadata::new(id, name, Version::new(1, 0, 0), kind, PLUGIN_API_VERSION)
        .with_author("ferrox")
        .with_description(description)
}

/// Video-effect plugin wrapping [`ColorGrade`] (ASC-CDL). Params are a
/// `ColorGrade` as JSON.
pub struct ColorGradePlugin {
    meta: PluginMetadata,
}

impl ColorGradePlugin {
    pub const ID: &'static str = "ferrox.builtin.color_grade";

    pub fn new() -> Self {
        let meta = PluginMetadata::new(Self::ID, "Color Grade", Version::new(1, 0, 0), PluginKind::VideoEffect, PLUGIN_API_VERSION)
            .with_author("ferrox")
            .with_description("ASC-CDL primary color grade");
        Self { meta }
    }
}

impl Default for ColorGradePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ColorGradePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn as_video_effect(&self) -> Option<&dyn VideoEffectPlugin> {
        Some(self)
    }
}

impl VideoEffectPlugin for ColorGradePlugin {
    fn apply_video(&self, mut frame: Frame, params: &Value) -> Result<Frame> {
        let grade: ColorGrade = serde_json::from_value(params.clone())
            .map_err(|e| PluginError::Other(format!("color grade params: {e}")))?;
        grade.apply_frame(&mut frame)?;
        Ok(frame)
    }
}

/// Audio-effect plugin wrapping the engine's [`CoreAudioEffect`] stack. Params
/// are an `AudioEffect` as JSON.
pub struct AudioFxPlugin {
    meta: PluginMetadata,
}

impl AudioFxPlugin {
    pub const ID: &'static str = "ferrox.builtin.audio_fx";

    pub fn new() -> Self {
        let meta = PluginMetadata::new(Self::ID, "Audio FX", Version::new(1, 0, 0), PluginKind::AudioEffect, PLUGIN_API_VERSION)
            .with_author("ferrox")
            .with_description("EQ / compressor / reverb / delay / gate");
        Self { meta }
    }
}

impl Default for AudioFxPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AudioFxPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn as_audio_effect(&self) -> Option<&dyn AudioEffectPlugin> {
        Some(self)
    }
}

impl AudioEffectPlugin for AudioFxPlugin {
    fn apply_audio(&self, frame: AudioFrame, params: &Value) -> Result<AudioFrame> {
        let fx: CoreAudioEffect = serde_json::from_value(params.clone())
            .map_err(|e| PluginError::Other(format!("audio fx params: {e}")))?;
        Ok(fx.apply(frame)?)
    }
}

/// Video-effect plugin wrapping the chroma [`Keyer`]. Params are a `Keyer` JSON.
pub struct KeyerPlugin {
    meta: PluginMetadata,
}
impl KeyerPlugin {
    pub const ID: &'static str = "ferrox.builtin.keyer";
    pub fn new() -> Self {
        Self { meta: builtin_meta(Self::ID, "Chroma Key", PluginKind::VideoEffect, "green/blue-screen keyer with despill") }
    }
}
impl Default for KeyerPlugin {
    fn default() -> Self {
        Self::new()
    }
}
impl Plugin for KeyerPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn as_video_effect(&self) -> Option<&dyn VideoEffectPlugin> {
        Some(self)
    }
}
impl VideoEffectPlugin for KeyerPlugin {
    fn apply_video(&self, mut frame: Frame, params: &Value) -> Result<Frame> {
        let keyer: Keyer = serde_json::from_value(params.clone())
            .map_err(|e| PluginError::Other(format!("keyer params: {e}")))?;
        keyer.apply_frame(&mut frame)?;
        Ok(frame)
    }
}

/// Video-effect plugin wrapping a vector [`Mask`]. Params are a `Mask` JSON.
pub struct MaskPlugin {
    meta: PluginMetadata,
}
impl MaskPlugin {
    pub const ID: &'static str = "ferrox.builtin.mask";
    pub fn new() -> Self {
        Self { meta: builtin_meta(Self::ID, "Mask", PluginKind::VideoEffect, "rectangle/ellipse/polygon mask with feather") }
    }
}
impl Default for MaskPlugin {
    fn default() -> Self {
        Self::new()
    }
}
impl Plugin for MaskPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn as_video_effect(&self) -> Option<&dyn VideoEffectPlugin> {
        Some(self)
    }
}
impl VideoEffectPlugin for MaskPlugin {
    fn apply_video(&self, mut frame: Frame, params: &Value) -> Result<Frame> {
        let mask: Mask = serde_json::from_value(params.clone())
            .map_err(|e| PluginError::Other(format!("mask params: {e}")))?;
        mask.apply_frame(&mut frame)?;
        Ok(frame)
    }
}

/// Transition plugin wrapping the [`Transition`] factory. Params:
/// `{ "type": "fade_in" | "fade_out" | "zoom_in" | "slide_in", "secs": f64, … }`.
pub struct TransitionBuiltin {
    meta: PluginMetadata,
}
impl TransitionBuiltin {
    pub const ID: &'static str = "ferrox.builtin.transition";
    pub fn new() -> Self {
        Self { meta: builtin_meta(Self::ID, "Transition", PluginKind::Transition, "fade / slide / zoom transitions") }
    }
}
impl Default for TransitionBuiltin {
    fn default() -> Self {
        Self::new()
    }
}
impl Plugin for TransitionBuiltin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn as_transition(&self) -> Option<&dyn TransitionPlugin> {
        Some(self)
    }
}
impl TransitionPlugin for TransitionBuiltin {
    fn build_transition(&self, params: &Value, clip_duration: f64) -> Result<ClipAnimation> {
        let ty = params.get("type").and_then(|v| v.as_str()).unwrap_or("fade_in");
        let secs = params.get("secs").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let anim = match ty {
            "fade_in" => Transition::fade_in(secs),
            "fade_out" => Transition::fade_out(clip_duration, secs),
            "zoom_in" => Transition::zoom_in(secs, Easing::EaseInOut),
            "slide_in" => {
                let dir = match params.get("direction").and_then(|v| v.as_str()).unwrap_or("left") {
                    "right" => Direction::Right,
                    "up" => Direction::Up,
                    "down" => Direction::Down,
                    _ => Direction::Left,
                };
                let offset = params.get("offset").and_then(|v| v.as_i64()).unwrap_or(100) as i32;
                Transition::slide_in(dir, offset, secs, Easing::EaseOut)
            }
            other => return Err(PluginError::Other(format!("unknown transition '{other}'"))),
        };
        Ok(anim)
    }
}

/// Register and enable all built-in plugins on `manager`.
pub fn register_builtins(manager: &PluginManager) -> Result<()> {
    let plugins: [(Arc<dyn Plugin>, &str); 5] = [
        (Arc::new(ColorGradePlugin::new()), ColorGradePlugin::ID),
        (Arc::new(KeyerPlugin::new()), KeyerPlugin::ID),
        (Arc::new(MaskPlugin::new()), MaskPlugin::ID),
        (Arc::new(AudioFxPlugin::new()), AudioFxPlugin::ID),
        (Arc::new(TransitionBuiltin::new()), TransitionBuiltin::ID),
    ];
    for (plugin, id) in plugins {
        manager.register(plugin)?;
        manager.enable(id)?;
    }
    Ok(())
}
