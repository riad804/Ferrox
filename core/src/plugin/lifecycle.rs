//! Plugin lifecycle states.

/// Where a registered plugin is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Registered but not yet enabled.
    Registered,
    /// Active and usable.
    Enabled,
    /// Registered but temporarily inactive.
    Disabled,
    /// Failed to enable (its `on_enable` returned an error).
    Failed,
}
