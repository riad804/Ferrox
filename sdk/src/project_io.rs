//! Project persistence — JSON in/out, with file helpers gated off `wasm32`
//! (no `std::fs` on the web; hosts pass JSON strings there instead).

use ferrox_core::Project;

use crate::error::{Result, SdkError};

/// Serialize a project to pretty JSON.
pub fn to_json(project: &Project) -> Result<String> {
    project.to_json().map_err(SdkError::from)
}

/// Parse a project from JSON. Backward compatible: fields added after a project
/// was saved fall back to their `serde` defaults.
pub fn from_json(json: &str) -> Result<Project> {
    Project::from_json(json).map_err(SdkError::from)
}

/// Save a project to a `.json` file. Not available on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_project(project: &Project, path: impl AsRef<std::path::Path>) -> Result<()> {
    std::fs::write(path, to_json(project)?).map_err(|e| SdkError::Core(e.into()))
}

/// Load a project from a `.json` file. Not available on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_project(path: impl AsRef<std::path::Path>) -> Result<Project> {
    let text = std::fs::read_to_string(path).map_err(|e| SdkError::Core(e.into()))?;
    from_json(&text)
}
