//! Plugin identity: a semver [`Version`] and the [`PluginMetadata`] descriptor.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::kind::PluginKind;

/// A semantic version (`major.minor.patch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }

    /// Parse `"1.2.3"` (patch optional → defaults to 0).
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.trim().split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next().unwrap_or("0").parse().ok()?;
        let patch = it.next().unwrap_or("0").parse().ok()?;
        Some(Self { major, minor, patch })
    }

    /// Whether a host at `self` can run a plugin that targets `required` — the
    /// caret rule: same major, and host ≥ required.
    pub fn is_compatible_with(&self, required: &Version) -> bool {
        self.major == required.major && *self >= *required
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Descriptive, discoverable metadata for a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Globally unique id, e.g. `"ferrox.builtin.color"`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The plugin's own version.
    pub version: Version,
    /// What kind of plugin this is.
    pub kind: PluginKind,
    /// The ferrox plugin-API version this plugin was built against.
    pub api_version: Version,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
}

impl PluginMetadata {
    /// Minimal metadata; enrich with the `with_*` builders.
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: Version, kind: PluginKind, api_version: Version) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version,
            kind,
            api_version,
            author: String::new(),
            description: String::new(),
        }
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}
