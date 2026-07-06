//! The [`PluginRegistry`] — the in-memory store of registered plugins keyed by
//! id, with their lifecycle state. Concurrency + events are added by the
//! [`super::manager::PluginManager`]; this type is the plain data structure.

use std::collections::HashMap;
use std::sync::Arc;

use super::error::{PluginError, Result};
use super::kind::PluginKind;
use super::lifecycle::Lifecycle;
use super::metadata::PluginMetadata;
use super::traits::Plugin;

struct Entry {
    plugin: Arc<dyn Plugin>,
    lifecycle: Lifecycle,
}

/// A registry of plugins keyed by their unique id.
#[derive(Default)]
pub struct PluginRegistry {
    by_id: HashMap<String, Entry>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a plugin (lifecycle `Registered`). Errors on a duplicate id.
    pub fn insert(&mut self, plugin: Arc<dyn Plugin>) -> Result<()> {
        let id = plugin.metadata().id.clone();
        if self.by_id.contains_key(&id) {
            return Err(PluginError::Duplicate(id));
        }
        self.by_id.insert(id, Entry { plugin, lifecycle: Lifecycle::Registered });
        Ok(())
    }

    /// Remove a plugin by id, returning it.
    pub fn remove(&mut self, id: &str) -> Result<Arc<dyn Plugin>> {
        self.by_id.remove(id).map(|e| e.plugin).ok_or_else(|| PluginError::NotFound(id.to_string()))
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        self.by_id.get(id).map(|e| Arc::clone(&e.plugin))
    }

    pub fn metadata(&self, id: &str) -> Option<&PluginMetadata> {
        self.by_id.get(id).map(|e| e.plugin.metadata())
    }

    pub fn lifecycle(&self, id: &str) -> Option<Lifecycle> {
        self.by_id.get(id).map(|e| e.lifecycle)
    }

    /// Update the lifecycle state of a plugin.
    pub fn set_lifecycle(&mut self, id: &str, state: Lifecycle) -> Result<()> {
        let e = self.by_id.get_mut(id).ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        e.lifecycle = state;
        Ok(())
    }

    /// Ids of all plugins of a given kind.
    pub fn ids_by_kind(&self, kind: PluginKind) -> Vec<String> {
        self.by_id
            .values()
            .filter(|e| e.plugin.metadata().kind == kind)
            .map(|e| e.plugin.metadata().id.clone())
            .collect()
    }

    /// All registered plugin ids.
    pub fn ids(&self) -> Vec<String> {
        self.by_id.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}
