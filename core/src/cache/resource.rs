//! [`ResourceCache`] — the engine's central cache facade, grouping typed
//! [`SharedCache`]s for the resource kinds that exist today (decoded frames,
//! imported images, 3D LUTs, decoded audio). GPU texture/shader and font caches
//! join here as those subsystems land — each is just another `SharedCache`.
//!
//! Each sub-cache has its own byte budget; [`ResourceCache::total_bytes`]
//! aggregates them for diagnostics.

use crate::audio::AudioFrame;
use crate::color::Lut3D;
use crate::frame::Frame;

use super::lru::Weight;
use super::shared::SharedCache;

// ── Weight impls for engine resources ───────────────────────────────────────

impl Weight for Frame {
    fn weight(&self) -> usize {
        self.data.len()
    }
}

impl Weight for AudioFrame {
    fn weight(&self) -> usize {
        self.samples.len() * std::mem::size_of::<f32>()
    }
}

impl Weight for Lut3D {
    fn weight(&self) -> usize {
        // size³ lattice points × 3 × f32.
        let n = self.size();
        n * n * n * 3 * std::mem::size_of::<f32>()
    }
}

/// Per-cache byte budgets. Defaults are tuned for a mid-range device; override
/// on constrained targets.
#[derive(Debug, Clone, Copy)]
pub struct CacheBudgets {
    pub frames: usize,
    pub images: usize,
    pub luts: usize,
    pub audio: usize,
}

impl Default for CacheBudgets {
    fn default() -> Self {
        Self {
            frames: 128 * 1024 * 1024, // 128 MiB of decoded frames
            images: 64 * 1024 * 1024,  // 64 MiB of imported images
            luts: 8 * 1024 * 1024,     // 8 MiB of LUTs
            audio: 32 * 1024 * 1024,   // 32 MiB of decoded audio
        }
    }
}

/// The engine's central resource cache. Cloneable; clones share the caches.
#[derive(Clone)]
pub struct ResourceCache {
    /// Composited/decoded frames, keyed however the caller composes (e.g.
    /// `"src:frame:42"`).
    pub frames: SharedCache<String, Frame>,
    /// Imported still images, keyed by asset id/path.
    pub images: SharedCache<String, Frame>,
    /// Parsed 3D LUTs, keyed by path/id.
    pub luts: SharedCache<String, Lut3D>,
    /// Decoded audio buffers, keyed by asset id/path.
    pub audio: SharedCache<String, AudioFrame>,
}

impl ResourceCache {
    /// A resource cache with the given per-kind byte budgets.
    pub fn with_budgets(b: CacheBudgets) -> Self {
        Self {
            frames: SharedCache::with_max_bytes(b.frames),
            images: SharedCache::with_max_bytes(b.images),
            luts: SharedCache::with_max_bytes(b.luts),
            audio: SharedCache::with_max_bytes(b.audio),
        }
    }

    /// Total bytes held across all sub-caches.
    pub fn total_bytes(&self) -> usize {
        self.frames.bytes() + self.images.bytes() + self.luts.bytes() + self.audio.bytes()
    }

    /// Total entries held across all sub-caches.
    pub fn total_entries(&self) -> usize {
        self.frames.len() + self.images.len() + self.luts.len() + self.audio.len()
    }

    /// Drop everything from every sub-cache.
    pub fn clear_all(&self) {
        self.frames.clear();
        self.images.clear();
        self.luts.clear();
        self.audio.clear();
    }
}

impl Default for ResourceCache {
    fn default() -> Self {
        Self::with_budgets(CacheBudgets::default())
    }
}
