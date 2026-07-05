//! SDK error type — wraps core errors and adds handle/state-machine failures.

/// Errors surfaced by the Editor SDK.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    /// An error bubbled up from `ferrox-core` (render/codec/filter).
    #[error("core error: {0}")]
    Core(#[from] ferrox_core::Error),

    /// A track/clip/keyframe index or handle did not resolve.
    #[error("invalid handle: {0}")]
    InvalidHandle(String),

    /// JSON (de)serialization of a project or object failed.
    #[error("serialization error: {0}")]
    Serde(String),

    /// The internal lock was poisoned by a panic on another thread.
    #[error("editor state lock poisoned")]
    Poisoned,
}

/// SDK result alias.
pub type Result<T> = std::result::Result<T, SdkError>;
