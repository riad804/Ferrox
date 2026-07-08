//! Plugin subsystem errors.

/// Errors from plugin registration, negotiation, and lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin '{0}' not found")]
    NotFound(String),

    #[error("plugin '{0}' is already registered")]
    Duplicate(String),

    #[error("plugin '{id}' needs host capabilities that are unavailable: {missing:?}")]
    MissingCapabilities { id: String, missing: Vec<String> },

    #[error("plugin '{id}' targets API {required} but host provides {host}")]
    Incompatible { id: String, required: String, host: String },

    #[error("plugin '{0}' is disabled")]
    Disabled(String),

    #[error("plugin failure: {0}")]
    Other(String),
}

/// Plugin subsystem result alias.
pub type Result<T> = std::result::Result<T, PluginError>;

impl From<crate::error::Error> for PluginError {
    fn from(e: crate::error::Error) -> Self {
        PluginError::Other(e.to_string())
    }
}
