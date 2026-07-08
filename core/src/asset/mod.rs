//! # Asset manager (Phase 2)
//!
//! Centralised, content-addressed management of media assets — images, video,
//! audio, fonts, LUTs, masks, stickers, SVG, Lottie, subtitles. Composes the
//! [`crate::cache`] (lazy, reference-counted, deduplicated loading) and, on
//! native targets, the [`crate::task`] system (background thumbnail/waveform/
//! proxy generation).
//!
//! - [`AssetManager`] — import, dedup, ref-count, metadata, cached lazy loads.
//! - [`DependencyGraph`] — asset → asset dependencies (cascade release).
//! - Content-addressed [`AssetId`]: identical sources collapse to one asset.

pub mod generate;
pub mod graph;
pub mod manager;

pub use graph::DependencyGraph;
pub use manager::AssetManager;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A content-addressed asset identifier (identical sources → identical id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetId(pub u64);

impl AssetId {
    /// Id derived from raw bytes (content addressing → deduplication).
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(fnv1a(bytes))
    }
    /// Id derived from a string (e.g. a file path).
    pub fn of_str(s: &str) -> Self {
        Self(fnv1a(s.as_bytes()))
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The category of an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Image,
    Video,
    Audio,
    Font,
    Lut,
    Mask,
    Sticker,
    Svg,
    Lottie,
    Subtitle,
}

impl AssetKind {
    /// Guess the kind from a file extension (lowercased).
    pub fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => AssetKind::Image,
            "mp4" | "webm" | "mkv" | "mov" | "ivf" => AssetKind::Video,
            "wav" | "mp3" | "flac" | "ogg" | "aac" | "m4a" | "opus" => AssetKind::Audio,
            "ttf" | "otf" | "woff" | "woff2" => AssetKind::Font,
            "cube" => AssetKind::Lut,
            "svg" => AssetKind::Svg,
            "lottie" => AssetKind::Lottie,
            "srt" | "vtt" | "ass" | "ssa" => AssetKind::Subtitle,
            _ => return None,
        })
    }
}

/// Where an asset's bytes come from.
#[derive(Debug, Clone)]
pub enum AssetSource {
    /// A file on disk (read on native targets; unavailable on `wasm32`).
    File(PathBuf),
    /// In-memory bytes (the portable path; how the web host supplies assets).
    Bytes(Arc<Vec<u8>>),
}

impl AssetSource {
    /// The content-addressed id of this source.
    pub fn id(&self) -> AssetId {
        match self {
            AssetSource::File(p) => AssetId::of_str(&p.to_string_lossy()),
            AssetSource::Bytes(b) => AssetId::of_bytes(b),
        }
    }

    /// The file extension, if any.
    pub fn extension(&self) -> Option<String> {
        match self {
            AssetSource::File(p) => p.extension().and_then(|e| e.to_str()).map(|s| s.to_string()),
            AssetSource::Bytes(_) => None,
        }
    }

    /// A human-readable hint (path or `<bytes>`).
    pub fn hint(&self) -> String {
        match self {
            AssetSource::File(p) => p.display().to_string(),
            AssetSource::Bytes(_) => "<bytes>".to_string(),
        }
    }

    /// Read the source's bytes.
    pub fn read(&self) -> Result<Vec<u8>> {
        match self {
            AssetSource::Bytes(b) => Ok((**b).clone()),
            #[cfg(not(target_arch = "wasm32"))]
            AssetSource::File(p) => std::fs::read(p).map_err(Error::from),
            #[cfg(target_arch = "wasm32")]
            AssetSource::File(_) => Err(Error::UnsupportedFormat("file asset sources are unavailable on wasm".into())),
        }
    }
}

/// Descriptive metadata about a registered asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetMetadata {
    pub id: AssetId,
    pub kind: AssetKind,
    /// Source byte size if known (0 for unread file sources).
    pub byte_size: usize,
    /// Human-readable source hint (path or `<bytes>`).
    pub source: String,
}

/// Detect an asset kind from a path's extension, defaulting to [`AssetKind::Image`]
/// is intentionally avoided — callers pass the kind explicitly when unknown.
pub(crate) fn kind_from_path(path: &Path) -> Option<AssetKind> {
    path.extension().and_then(|e| e.to_str()).and_then(AssetKind::from_extension)
}
