//! The plugin trait hierarchy.
//!
//! Every plugin implements the object-safe base [`Plugin`] (metadata + lifecycle
//! hooks + capability requirements). Kind-specific behaviour lives in sub-traits
//! ([`VideoEffectPlugin`], [`AudioEffectPlugin`], …); the base trait offers
//! **capability-projection** methods (`as_video_effect`, …) so a registry that
//! stores `Arc<dyn Plugin>` can recover the typed interface without `Any`.
//!
//! More kind traits (Transition/Exporter/Importer/Ai/RenderNode) land in the
//! next increment; their projections default to `None` here.

use serde_json::Value;

use crate::audio::AudioFrame;
use crate::frame::Frame;
use crate::timeline::ClipAnimation;

use super::capability::CapabilitySet;
use super::error::Result;
use super::metadata::PluginMetadata;

/// The object-safe base every plugin implements.
pub trait Plugin: Send + Sync {
    /// Descriptive metadata (id, version, kind, api version).
    fn metadata(&self) -> &PluginMetadata;

    /// Host capabilities this plugin requires (default: none).
    fn required_capabilities(&self) -> CapabilitySet {
        CapabilitySet::new()
    }

    /// Called when the plugin is enabled (default: no-op).
    fn on_enable(&self) -> Result<()> {
        Ok(())
    }

    /// Called when the plugin is disabled (default: no-op).
    fn on_disable(&self) -> Result<()> {
        Ok(())
    }

    /// Recover the video-effect interface, if this plugin is one.
    fn as_video_effect(&self) -> Option<&dyn VideoEffectPlugin> {
        None
    }

    /// Recover the audio-effect interface, if this plugin is one.
    fn as_audio_effect(&self) -> Option<&dyn AudioEffectPlugin> {
        None
    }

    /// Recover the transition interface, if this plugin is one.
    fn as_transition(&self) -> Option<&dyn TransitionPlugin> {
        None
    }

    /// Recover the exporter interface, if this plugin is one.
    fn as_exporter(&self) -> Option<&dyn ExporterPlugin> {
        None
    }

    /// Recover the importer interface, if this plugin is one.
    fn as_importer(&self) -> Option<&dyn ImporterPlugin> {
        None
    }

    /// Recover the render-node interface, if this plugin is one.
    fn as_render_node(&self) -> Option<&dyn RenderNodePlugin> {
        None
    }

    /// Recover the AI interface, if this plugin is one.
    fn as_ai(&self) -> Option<&dyn AiPlugin> {
        None
    }
}

/// A plugin that transforms a video [`Frame`].
pub trait VideoEffectPlugin: Plugin {
    /// Apply the effect to `frame`, parameterised by `params` (JSON).
    fn apply_video(&self, frame: Frame, params: &Value) -> Result<Frame>;
}

/// A plugin that transforms an [`AudioFrame`].
pub trait AudioEffectPlugin: Plugin {
    /// Apply the effect to `frame`, parameterised by `params` (JSON).
    fn apply_audio(&self, frame: AudioFrame, params: &Value) -> Result<AudioFrame>;
}

/// A plugin that builds a timeline transition as a keyframed [`ClipAnimation`]
/// for the incoming clip (the "keyframe engine is king" rule).
pub trait TransitionPlugin: Plugin {
    fn build_transition(&self, params: &Value, clip_duration: f64) -> Result<ClipAnimation>;
}

/// A plugin that encodes composed frames into an output container's bytes.
pub trait ExporterPlugin: Plugin {
    /// The output file extension (e.g. `"mp4"`).
    fn output_extension(&self) -> &str;
    /// Encode `frames` at `fps` into container bytes.
    fn export(&self, frames: &[Frame], fps: f64, params: &Value) -> Result<Vec<u8>>;
}

/// A plugin that decodes media bytes into frames.
pub trait ImporterPlugin: Plugin {
    fn import(&self, data: &[u8], params: &Value) -> Result<Vec<Frame>>;
}

/// A plugin that acts as a node in the render graph (Phase 4): consumes input
/// frames and produces one output frame.
pub trait RenderNodePlugin: Plugin {
    fn render(&self, inputs: &[Frame], params: &Value) -> Result<Frame>;
}

/// A plugin backed by an AI model. Heavy, async inference lives in `ferrox-ai`;
/// this marks the plugin and names the model it fronts.
pub trait AiPlugin: Plugin {
    fn model_id(&self) -> &str;
}
