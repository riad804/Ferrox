//! # Plugin system (Phase 1)
//!
//! A capability-negotiated, versioned plugin architecture. Plugins implement the
//! object-safe [`Plugin`] base plus a kind-specific trait ([`VideoEffectPlugin`],
//! [`AudioEffectPlugin`], …). The [`PluginManager`] registers them (negotiating
//! [`Version`] + [`Capability`]s), drives their [`Lifecycle`], and publishes
//! plugin [`crate::Event`]s on the bus.
//!
//! Registration is **static** on every platform (call [`register_builtins`] or
//! `manager.register(..)`); dynamic loading (desktop-only, `dynamic-plugins`
//! feature) lands in a later increment. Pure Rust, WASM-safe.
//!
//! ```
//! use ferrox_core::plugin::{PluginManager, register_builtins, PluginKind, PLUGIN_API_VERSION};
//! use ferrox_core::plugin::CapabilitySet;
//!
//! let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
//! register_builtins(&mgr).unwrap();
//! // Built-in video effects: color grade, chroma keyer, mask.
//! assert_eq!(mgr.ids_by_kind(PluginKind::VideoEffect).len(), 3);
//! ```

pub mod abi;
pub mod capability;
pub mod error;
pub mod kind;
pub mod lifecycle;
pub mod manager;
pub mod metadata;
pub mod registry;
pub mod traits;

/// Host-side dynamic loader (desktop only, `dynamic-plugins` feature).
#[cfg(all(feature = "dynamic-plugins", not(target_arch = "wasm32")))]
pub mod dynamic;

mod builtin;

pub use builtin::{
    register_builtins, AudioFxPlugin, ColorGradePlugin, KeyerPlugin, MaskPlugin, TransitionBuiltin,
};
pub use capability::{Capability, CapabilitySet};
pub use error::{PluginError, Result as PluginResult};
pub use kind::PluginKind;
pub use lifecycle::Lifecycle;
pub use manager::PluginManager;
pub use metadata::{PluginMetadata, Version};
pub use registry::PluginRegistry;
pub use traits::{
    AiPlugin, AudioEffectPlugin, ExporterPlugin, ImporterPlugin, Plugin, RenderNodePlugin,
    TransitionPlugin, VideoEffectPlugin,
};

pub use abi::{FerroxPluginV1, FfiBuffer, ENTRY_SYMBOL, PLUGIN_ABI_VERSION};
#[cfg(all(feature = "dynamic-plugins", not(target_arch = "wasm32")))]
pub use dynamic::{load_plugin, DynamicPlugin};

/// The plugin-API version this build of ferrox implements. A plugin is
/// compatible when the host shares its major version and is ≥ its version.
pub const PLUGIN_API_VERSION: Version = Version::new(1, 0, 0);
