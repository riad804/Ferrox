//! [`DependencyGraph`] — a directed graph of asset → asset dependencies, used to
//! cascade reference counts (releasing a composite releases what it references)
//! and to answer "what depends on this?".

use std::collections::{HashMap, HashSet};

use super::AssetId;

/// A directed dependency graph between assets.
#[derive(Debug, Default)]
pub struct DependencyGraph {
    /// parent → children it depends on.
    deps: HashMap<AssetId, HashSet<AssetId>>,
    /// child → parents that depend on it.
    dependents: HashMap<AssetId, HashSet<AssetId>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `parent` depends on `child`.
    pub fn add_dependency(&mut self, parent: AssetId, child: AssetId) {
        self.deps.entry(parent).or_default().insert(child);
        self.dependents.entry(child).or_default().insert(parent);
    }

    /// Remove an asset and all edges touching it.
    pub fn remove(&mut self, id: AssetId) {
        if let Some(children) = self.deps.remove(&id) {
            for c in children {
                if let Some(set) = self.dependents.get_mut(&c) {
                    set.remove(&id);
                }
            }
        }
        if let Some(parents) = self.dependents.remove(&id) {
            for p in parents {
                if let Some(set) = self.deps.get_mut(&p) {
                    set.remove(&id);
                }
            }
        }
    }

    /// Direct dependencies of `id`.
    pub fn dependencies(&self, id: AssetId) -> Vec<AssetId> {
        self.deps.get(&id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    /// Direct dependents (things that depend on `id`).
    pub fn dependents(&self, id: AssetId) -> Vec<AssetId> {
        self.dependents.get(&id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    /// All transitive dependencies of `id` (depth-first, cycle-safe).
    pub fn transitive_dependencies(&self, id: AssetId) -> Vec<AssetId> {
        let mut seen = HashSet::new();
        let mut stack: Vec<AssetId> = self.dependencies(id);
        let mut out = Vec::new();
        while let Some(n) = stack.pop() {
            if seen.insert(n) {
                out.push(n);
                stack.extend(self.dependencies(n));
            }
        }
        out
    }
}
