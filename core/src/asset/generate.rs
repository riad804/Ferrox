//! Derived-asset generation: **thumbnails** (from image assets) and
//! **waveforms** (from audio assets). Sync helpers are WASM-safe; running them
//! on the background [`crate::task`] pool is native-only.
//!
//! Proxy (low-res video) generation joins here once the video decode pipeline
//! lands.

use crate::audio::waveform::{generate_waveform, WaveformBucket};
use crate::error::Result;
use crate::filters::ThumbnailFilter;
use crate::frame::Frame;
use crate::traits::Filter;

use super::{AssetId, AssetManager};

/// Generate an aspect-fit thumbnail (≤ `max_w × max_h`) for an image asset.
pub fn thumbnail(mgr: &AssetManager, id: AssetId, max_w: u32, max_h: u32) -> Result<Frame> {
    let image = mgr.load_image(id)?;
    ThumbnailFilter::new(max_w.max(1), max_h.max(1)).process((*image).clone())
}

/// Generate a `buckets`-wide waveform overview for an audio asset.
pub fn waveform(mgr: &AssetManager, id: AssetId, buckets: usize) -> Result<Vec<WaveformBucket>> {
    let audio = mgr.load_audio(id)?;
    Ok(generate_waveform(&audio, buckets))
}

/// Native background generation on a [`crate::task::TaskManager`].
#[cfg(not(target_arch = "wasm32"))]
pub mod background {
    use std::sync::Arc;

    use crate::audio::waveform::WaveformBucket;
    use crate::frame::Frame;
    use crate::task::{Priority, TaskHandle, TaskManager, TaskOutcome};

    use super::super::{AssetId, AssetManager};

    /// Submit thumbnail generation to the task pool; the result arrives via
    /// `on_complete`.
    pub fn thumbnail(
        mgr: Arc<AssetManager>,
        tasks: &TaskManager,
        id: AssetId,
        max_w: u32,
        max_h: u32,
        on_complete: impl FnOnce(TaskOutcome<Frame>) + Send + 'static,
    ) -> TaskHandle {
        tasks.submit(
            Priority::Normal,
            move |ctrl| {
                ctrl.checkpoint()?;
                super::thumbnail(&mgr, id, max_w, max_h).map_err(|e| e.to_string())
            },
            on_complete,
        )
    }

    /// Submit waveform generation to the task pool.
    pub fn waveform(
        mgr: Arc<AssetManager>,
        tasks: &TaskManager,
        id: AssetId,
        buckets: usize,
        on_complete: impl FnOnce(TaskOutcome<Vec<WaveformBucket>>) + Send + 'static,
    ) -> TaskHandle {
        tasks.submit(
            Priority::Normal,
            move |ctrl| {
                ctrl.checkpoint()?;
                super::waveform(&mgr, id, buckets).map_err(|e| e.to_string())
            },
            on_complete,
        )
    }
}
