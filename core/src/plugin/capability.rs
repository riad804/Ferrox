//! Capability negotiation between the host and a plugin.
//!
//! A plugin declares the host [`Capability`]s it needs; the host advertises what
//! it provides. Registration succeeds only when the host's set is a superset of
//! the plugin's requirements.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A host facility a plugin may require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Gpu,
    Simd,
    Threads,
    FileSystem,
    Network,
    VideoDecode,
    VideoEncode,
    AudioDecode,
    AudioEncode,
}

/// An ordered set of [`Capability`]s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, cap: Capability) -> &mut Self {
        self.0.insert(cap);
        self
    }

    pub fn contains(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The capabilities in `self` that are **not** provided by `host` — empty
    /// when the host fully satisfies these requirements.
    pub fn missing_from(&self, host: &CapabilitySet) -> Vec<Capability> {
        self.0.iter().copied().filter(|c| !host.contains(*c)).collect()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}
