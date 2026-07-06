//! The [`PluginManager`] — thread-safe façade over the [`PluginRegistry`] that
//! performs capability/version **negotiation** on registration, drives the
//! **lifecycle** (enable/disable), and publishes plugin [`Event`]s on the bus.

use std::sync::{Arc, RwLock};

use crate::event::{Event, EventSink, NoopSink};

use super::capability::CapabilitySet;
use super::error::{PluginError, Result};
use super::kind::PluginKind;
use super::lifecycle::Lifecycle;
use super::metadata::Version;
use super::registry::PluginRegistry;
use super::traits::Plugin;

/// Manages the set of plugins available to the engine.
pub struct PluginManager {
    registry: RwLock<PluginRegistry>,
    host_api_version: Version,
    host_capabilities: CapabilitySet,
    events: RwLock<Arc<dyn EventSink>>,
}

impl PluginManager {
    /// A manager advertising `host_api_version` and `host_capabilities`, with no
    /// event sink (events are dropped). Use [`PluginManager::with_event_sink`]
    /// to observe lifecycle events.
    pub fn new(host_api_version: Version, host_capabilities: CapabilitySet) -> Self {
        Self {
            registry: RwLock::new(PluginRegistry::new()),
            host_api_version,
            host_capabilities,
            events: RwLock::new(Arc::new(NoopSink)),
        }
    }

    /// Inject an event sink (builder style).
    pub fn with_event_sink(self, sink: Arc<dyn EventSink>) -> Self {
        self.set_event_sink(sink);
        self
    }

    /// Replace the event sink at runtime. Registering the built-ins before
    /// wiring the real sink keeps startup silent.
    pub fn set_event_sink(&self, sink: Arc<dyn EventSink>) {
        *self.events.write().unwrap_or_else(|e| e.into_inner()) = sink;
    }

    fn emit(&self, event: Event) {
        self.events.read().unwrap_or_else(|e| e.into_inner()).publish(event);
    }

    fn registry(&self) -> std::sync::RwLockReadGuard<'_, PluginRegistry> {
        self.registry.read().unwrap_or_else(|e| e.into_inner())
    }

    fn registry_mut(&self) -> std::sync::RwLockWriteGuard<'_, PluginRegistry> {
        self.registry.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a plugin after negotiating API version + capabilities. Emits
    /// [`Event::PluginLoaded`] on success.
    pub fn register(&self, plugin: Arc<dyn Plugin>) -> Result<()> {
        let (id, required_api) = {
            let m = plugin.metadata();
            (m.id.clone(), m.api_version)
        };
        // Version negotiation.
        if !self.host_api_version.is_compatible_with(&required_api) {
            return Err(PluginError::Incompatible {
                id,
                required: required_api.to_string(),
                host: self.host_api_version.to_string(),
            });
        }
        // Capability negotiation.
        let missing = plugin.required_capabilities().missing_from(&self.host_capabilities);
        if !missing.is_empty() {
            return Err(PluginError::MissingCapabilities {
                id,
                missing: missing.iter().map(|c| format!("{c:?}")).collect(),
            });
        }
        self.registry_mut().insert(plugin)?;
        self.emit(Event::PluginLoaded { id });
        Ok(())
    }

    /// Unregister a plugin. Emits [`Event::PluginUnloaded`].
    pub fn unregister(&self, id: &str) -> Result<()> {
        self.registry_mut().remove(id)?;
        self.emit(Event::PluginUnloaded { id: id.to_string() });
        Ok(())
    }

    /// Enable a plugin (runs `on_enable`). Emits [`Event::PluginEnabled`].
    pub fn enable(&self, id: &str) -> Result<()> {
        let plugin = self.get(id).ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        match plugin.on_enable() {
            Ok(()) => {
                self.registry_mut().set_lifecycle(id, Lifecycle::Enabled)?;
                self.emit(Event::PluginEnabled { id: id.to_string() });
                Ok(())
            }
            Err(e) => {
                let _ = self.registry_mut().set_lifecycle(id, Lifecycle::Failed);
                Err(e)
            }
        }
    }

    /// Disable a plugin (runs `on_disable`). Emits [`Event::PluginDisabled`].
    pub fn disable(&self, id: &str) -> Result<()> {
        let plugin = self.get(id).ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        plugin.on_disable()?;
        self.registry_mut().set_lifecycle(id, Lifecycle::Disabled)?;
        self.emit(Event::PluginDisabled { id: id.to_string() });
        Ok(())
    }

    /// Get a plugin by id (any lifecycle state).
    pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        self.registry().get(id)
    }

    /// Get a plugin only if it is currently enabled.
    pub fn enabled(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        let reg = self.registry();
        match reg.lifecycle(id) {
            Some(Lifecycle::Enabled) => reg.get(id),
            _ => None,
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        matches!(self.registry().lifecycle(id), Some(Lifecycle::Enabled))
    }

    pub fn lifecycle(&self, id: &str) -> Option<Lifecycle> {
        self.registry().lifecycle(id)
    }

    /// Ids of all plugins of a given kind.
    pub fn ids_by_kind(&self, kind: PluginKind) -> Vec<String> {
        self.registry().ids_by_kind(kind)
    }

    /// Number of registered plugins.
    pub fn count(&self) -> usize {
        self.registry().len()
    }
}
