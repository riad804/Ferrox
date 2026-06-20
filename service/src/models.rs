//! Request / response types shared between handlers.

use serde::{Deserialize, Serialize};

/// Describes a media-processing job submitted to `POST /process`.
#[derive(Debug, Deserialize)]
pub struct Job {
    /// Output container / image format: `"png"`, `"jpg"`, `"jpeg"`.
    pub output_format: String,
    /// Optional filtergraph expression (e.g. `"blur=2.0,grayscale"`).
    #[serde(default)]
    pub filter_complex: Option<String>,
}

/// Generic JSON error body.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

impl ApiError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { error: msg.into() }
    }
}
