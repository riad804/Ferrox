//! [`AssetManager`] — the central registry: content-addressed import with
//! **deduplication**, **reference counting** (with dependency cascade), lazy
//! **cached** decoding, and metadata. Thread-safe (`RwLock`), WASM-safe (file
//! I/O is gated in [`super::AssetSource::read`]).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, RwLock};

use crate::audio::AudioFrame;
use crate::cache::ResourceCache;
use crate::color::Lut3D;
use crate::error::{Error, Result};
use crate::frame::Frame;

use super::graph::DependencyGraph;
use super::{AssetId, AssetKind, AssetMetadata, AssetSource};

struct Record {
    source: AssetSource,
    meta: AssetMetadata,
    refcount: usize,
}

/// The engine's asset registry + cache facade.
pub struct AssetManager {
    records: RwLock<HashMap<AssetId, Record>>,
    graph: RwLock<DependencyGraph>,
    cache: ResourceCache,
}

impl AssetManager {
    /// A manager with a default-budgeted resource cache.
    pub fn new() -> Self {
        Self::with_cache(ResourceCache::default())
    }

    /// A manager sharing an existing resource cache (e.g. the editor's).
    pub fn with_cache(cache: ResourceCache) -> Self {
        Self { records: RwLock::new(HashMap::new()), graph: RwLock::new(DependencyGraph::new()), cache }
    }

    /// The shared resource cache.
    pub fn cache(&self) -> &ResourceCache {
        &self.cache
    }

    fn records(&self) -> std::sync::RwLockReadGuard<'_, HashMap<AssetId, Record>> {
        self.records.read().unwrap_or_else(|e| e.into_inner())
    }
    fn records_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<AssetId, Record>> {
        self.records.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Import a source as `kind`. Deduplicated: importing the same source again
    /// bumps its reference count and returns the existing id.
    pub fn import(&self, source: AssetSource, kind: AssetKind) -> AssetId {
        let id = source.id();
        let mut recs = self.records_mut();
        if let Some(r) = recs.get_mut(&id) {
            r.refcount += 1;
            return id;
        }
        let byte_size = match &source {
            AssetSource::Bytes(b) => b.len(),
            AssetSource::File(_) => 0, // filled lazily on load
        };
        let meta = AssetMetadata { id, kind, byte_size, source: source.hint() };
        recs.insert(id, Record { source, meta, refcount: 1 });
        id
    }

    /// Import a file, inferring the kind from its extension.
    pub fn import_file(&self, path: impl Into<std::path::PathBuf>) -> Result<AssetId> {
        let path = path.into();
        let kind = super::kind_from_path(&path)
            .ok_or_else(|| Error::UnsupportedFormat(format!("unknown asset extension: {}", path.display())))?;
        Ok(self.import(AssetSource::File(path), kind))
    }

    pub fn contains(&self, id: AssetId) -> bool {
        self.records().contains_key(&id)
    }
    pub fn count(&self) -> usize {
        self.records().len()
    }
    pub fn metadata(&self, id: AssetId) -> Option<AssetMetadata> {
        self.records().get(&id).map(|r| r.meta.clone())
    }
    pub fn refcount(&self, id: AssetId) -> Option<usize> {
        self.records().get(&id).map(|r| r.refcount)
    }

    /// Increment an asset's reference count.
    pub fn retain(&self, id: AssetId) -> Result<()> {
        self.records_mut()
            .get_mut(&id)
            .map(|r| r.refcount += 1)
            .ok_or_else(|| Error::NotFound(format!("asset {}", id.0)))
    }

    /// Decrement an asset's reference count. When it reaches zero the asset is
    /// removed (evicted from the cache + graph) and its dependencies released.
    /// Returns `true` if the asset was removed.
    pub fn release(&self, id: AssetId) -> bool {
        let should_remove = match self.records_mut().get_mut(&id) {
            Some(r) => {
                r.refcount = r.refcount.saturating_sub(1);
                r.refcount == 0
            }
            None => return false,
        };
        if !should_remove {
            return false;
        }
        let deps = self.dependencies(id);
        self.records_mut().remove(&id);
        self.graph.write().unwrap_or_else(|e| e.into_inner()).remove(id);
        let key = id.0.to_string();
        self.cache.images.remove(&key);
        self.cache.audio.remove(&key);
        self.cache.luts.remove(&key);
        for child in deps {
            self.release(child); // cascade
        }
        true
    }

    // ── dependency graph ────────────────────────────────────────────────────

    /// Record that `parent` depends on `child` (and retain the child).
    pub fn add_dependency(&self, parent: AssetId, child: AssetId) -> Result<()> {
        if !self.contains(child) {
            return Err(Error::NotFound(format!("asset {}", child.0)));
        }
        self.graph.write().unwrap_or_else(|e| e.into_inner()).add_dependency(parent, child);
        self.retain(child)
    }

    pub fn dependencies(&self, id: AssetId) -> Vec<AssetId> {
        self.graph.read().unwrap_or_else(|e| e.into_inner()).dependencies(id)
    }
    pub fn dependents(&self, id: AssetId) -> Vec<AssetId> {
        self.graph.read().unwrap_or_else(|e| e.into_inner()).dependents(id)
    }
    pub fn transitive_dependencies(&self, id: AssetId) -> Vec<AssetId> {
        self.graph.read().unwrap_or_else(|e| e.into_inner()).transitive_dependencies(id)
    }

    // ── lazy, cached loading ────────────────────────────────────────────────

    fn source_of(&self, id: AssetId) -> Result<AssetSource> {
        self.records().get(&id).map(|r| r.source.clone()).ok_or_else(|| Error::NotFound(format!("asset {}", id.0)))
    }

    /// Load (decoding + caching on first access) an image asset.
    pub fn load_image(&self, id: AssetId) -> Result<Arc<Frame>> {
        let key = id.0.to_string();
        if let Some(f) = self.cache.images.get(&key) {
            return Ok(f);
        }
        let bytes = self.source_of(id)?.read()?;
        Ok(self.cache.images.put(key, decode_image(&bytes)?))
    }

    /// Load (decoding + caching) an audio asset.
    pub fn load_audio(&self, id: AssetId) -> Result<Arc<AudioFrame>> {
        let key = id.0.to_string();
        if let Some(a) = self.cache.audio.get(&key) {
            return Ok(a);
        }
        let source = self.source_of(id)?;
        let bytes = source.read()?;
        Ok(self.cache.audio.put(key, decode_audio(&bytes, source.extension().as_deref())?))
    }

    /// Load (parsing + caching) a `.cube` 3D LUT asset.
    pub fn load_lut(&self, id: AssetId) -> Result<Arc<Lut3D>> {
        let key = id.0.to_string();
        if let Some(l) = self.cache.luts.get(&key) {
            return Ok(l);
        }
        let bytes = self.source_of(id)?.read()?;
        let text = std::str::from_utf8(&bytes).map_err(|e| Error::Filter(format!("lut not UTF-8: {e}")))?;
        Ok(self.cache.luts.put(key, Lut3D::parse_cube(text)?))
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode PNG/JPEG bytes to a [`Frame`].
fn decode_image(data: &[u8]) -> Result<Frame> {
    use crate::codecs::{jpeg::JpegDecoder, png::PngDecoder};
    use crate::traits::Decoder;
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        PngDecoder.decode(Cursor::new(data))
    } else if data.starts_with(&[0xFF, 0xD8]) {
        JpegDecoder.decode(Cursor::new(data))
    } else {
        Err(Error::UnsupportedFormat("image asset must be PNG or JPEG".into()))
    }
}

/// Decode audio bytes: WAV by magic, otherwise by file extension via the registry.
fn decode_audio(data: &[u8], ext: Option<&str>) -> Result<AudioFrame> {
    use crate::codecs::WavDecoder;
    use crate::traits::AudioDecoder;
    if data.starts_with(b"RIFF") {
        return WavDecoder.decode_audio(Cursor::new(data));
    }
    let ext = ext.ok_or_else(|| Error::UnsupportedFormat("audio asset needs a known extension".into()))?;
    let registry = crate::registry::AudioDecoderRegistry::default();
    let decoder = registry
        .get(ext)
        .ok_or_else(|| Error::UnsupportedFormat(format!("no audio decoder for '{ext}'")))?;
    decoder.decode_audio_dyn(&mut Cursor::new(data))
}
