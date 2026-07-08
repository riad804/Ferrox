//! In-memory snapshot history for autosave and crash recovery.
//!
//! [`SnapshotHistory`] keeps a bounded ring of recent project states as encoded
//! `.ferrox` containers (compressed, so long histories stay cheap). The host
//! drives it: call [`SnapshotHistory::capture`] on an autosave tick, then
//! [`SnapshotHistory::latest`] on relaunch to recover the last good state.
//!
//! It is deliberately storage-only and WASM-safe — persisting the bytes to disk
//! (or IndexedDB, etc.) is the host's responsibility.

use std::collections::VecDeque;

use crate::error::{Error, Result};
use crate::timeline::Project;

use super::format::{decode, encode, Compression};

/// One captured revision: the encoded container plus a monotonic revision id and
/// a caller-supplied logical timestamp (seconds; the host owns the clock so this
/// stays WASM-safe and deterministic in tests).
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub revision: u64,
    pub timestamp: f64,
    pub bytes: Vec<u8>,
}

impl Snapshot {
    /// Decode this snapshot back into a project.
    pub fn to_project(&self) -> Result<Project> {
        decode(&self.bytes)
    }

    /// Encoded size in bytes.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}

/// A bounded ring of recent snapshots (newest last).
#[derive(Debug, Clone)]
pub struct SnapshotHistory {
    entries: VecDeque<Snapshot>,
    capacity: usize,
    next_revision: u64,
}

impl SnapshotHistory {
    /// Create a history keeping at most `capacity` snapshots (min 1).
    pub fn new(capacity: usize) -> Self {
        Self { entries: VecDeque::new(), capacity: capacity.max(1), next_revision: 0 }
    }

    /// Capture `project` at logical `timestamp`, evicting the oldest snapshot if
    /// at capacity. Returns the new snapshot's revision id.
    pub fn capture(&mut self, project: &Project, timestamp: f64) -> Result<u64> {
        let bytes = encode(project, Compression::Deflate)?;
        let revision = self.next_revision;
        self.next_revision += 1;
        self.entries.push_back(Snapshot { revision, timestamp, bytes });
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        Ok(revision)
    }

    /// The most recent snapshot, if any (for crash recovery).
    pub fn latest(&self) -> Option<&Snapshot> {
        self.entries.back()
    }

    /// Recover the latest project state, decoding it.
    pub fn recover(&self) -> Result<Project> {
        self.latest()
            .ok_or_else(|| Error::NotFound("no snapshot to recover".into()))?
            .to_project()
    }

    /// Look up a snapshot by revision id.
    pub fn get(&self, revision: u64) -> Option<&Snapshot> {
        self.entries.iter().find(|s| s.revision == revision)
    }

    /// Iterate snapshots oldest → newest.
    pub fn iter(&self) -> impl Iterator<Item = &Snapshot> {
        self.entries.iter()
    }

    /// Number of retained snapshots.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total bytes held across all retained snapshots.
    pub fn total_bytes(&self) -> usize {
        self.entries.iter().map(Snapshot::size).sum()
    }
}
