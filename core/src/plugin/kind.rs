//! The categories of plugin the engine supports.

use serde::{Deserialize, Serialize};

/// What a plugin does — used for discovery and typed lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// Transforms a video [`crate::Frame`].
    VideoEffect,
    /// Transforms an [`crate::AudioFrame`].
    AudioEffect,
    /// Produces a timeline transition.
    Transition,
    /// Writes a project/frames to an output format.
    Exporter,
    /// Reads media/projects into the engine.
    Importer,
    /// A model-backed AI capability.
    Ai,
    /// A node in the render graph.
    RenderNode,
}
