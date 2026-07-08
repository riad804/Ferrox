//! Project storage (Phase 14): a versioned, compressed `.ferrox` document
//! container plus schema migration, autosave snapshot history, structural diff,
//! and crash recovery.
//!
//! The engine is WASM-safe: encoding/decoding operate on **bytes**, and the
//! host owns persistence. Native convenience helpers that touch the filesystem
//! ([`save_file`] / [`load_file`]) are compiled only off `wasm32`.
//!
//! ```
//! use ferrox_core::storage::{encode, decode, Compression};
//! use ferrox_core::Project;
//!
//! let project = Project::new(1920, 1080, 30.0);
//! let bytes = encode(&project, Compression::Deflate).unwrap();
//! let restored = decode(&bytes).unwrap();
//! assert_eq!(project, restored);
//! ```

mod diff;
mod format;
mod migration;
mod snapshot;

pub use diff::{diff, Change, ProjectDiff};
pub use format::{decode, encode, read_header, Compression, Header, HEADER_LEN, MAGIC, SCHEMA_VERSION};
pub use snapshot::{Snapshot, SnapshotHistory};

#[cfg(not(target_arch = "wasm32"))]
mod file {
    use std::path::Path;

    use crate::error::Result;
    use crate::timeline::Project;

    use super::{decode, encode, Compression};

    /// Save `project` to `path` as a compressed `.ferrox` container.
    pub fn save_file(project: &Project, path: impl AsRef<Path>) -> Result<()> {
        let bytes = encode(project, Compression::Deflate)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Load and decode a `.ferrox` container from `path`.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Project> {
        let bytes = std::fs::read(path)?;
        decode(&bytes)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file::{load_file, save_file};
