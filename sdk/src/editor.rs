//! The [`Editor`] — the handle-based facade all consumers (CLI, WASM, UniFFI)
//! drive. It is `Clone` and thread-safe: internal state lives behind an
//! `Arc<Mutex<..>>`, so a handle can be shared across threads/FFI and every
//! mutation is serialized. All edits flow through the [`Command`] stack, giving
//! undo/redo for free.

use std::sync::{Arc, Mutex, MutexGuard};

use ferrox_core::{compose_frame_graded, Clip, Project};

use crate::commands::Command;
use crate::error::{Result, SdkError};
use crate::project_io;

/// Internal mutable state: the project plus the undo/redo stacks.
struct Inner {
    project: Project,
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
}

/// A cloneable, thread-safe handle to an editing session.
#[derive(Clone)]
pub struct Editor {
    inner: Arc<Mutex<Inner>>,
}

impl Editor {
    /// A new editor with an empty project of the given output spec.
    pub fn new(width: u32, height: u32, fps: f64) -> Self {
        Self::from_project(Project::new(width, height, fps))
    }

    /// Wrap an existing project.
    pub fn from_project(project: Project) -> Self {
        Self { inner: Arc::new(Mutex::new(Inner { project, undo: Vec::new(), redo: Vec::new() })) }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        self.inner.lock().map_err(|_| SdkError::Poisoned)
    }

    /// Run a command: apply it, push to the undo stack, and clear the redo stack.
    pub fn execute(&self, mut cmd: Box<dyn Command>) -> Result<()> {
        let mut g = self.lock()?;
        cmd.apply(&mut g.project)?;
        g.undo.push(cmd);
        g.redo.clear();
        Ok(())
    }

    /// Undo the most recent command. Returns `false` if nothing to undo.
    pub fn undo(&self) -> Result<bool> {
        let mut g = self.lock()?;
        match g.undo.pop() {
            Some(mut cmd) => {
                cmd.revert(&mut g.project)?;
                g.redo.push(cmd);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Redo the most recently undone command. Returns `false` if nothing to redo.
    pub fn redo(&self) -> Result<bool> {
        let mut g = self.lock()?;
        match g.redo.pop() {
            Some(mut cmd) => {
                cmd.apply(&mut g.project)?;
                g.undo.push(cmd);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Depth of the undo stack.
    pub fn undo_depth(&self) -> usize {
        self.lock().map(|g| g.undo.len()).unwrap_or(0)
    }

    /// Depth of the redo stack.
    pub fn redo_depth(&self) -> usize {
        self.lock().map(|g| g.redo.len()).unwrap_or(0)
    }

    /// Render the composed output frame at time `t` as RGBA8 bytes. If `width`/
    /// `height` are non-zero and differ from the project size, the composite is
    /// nearest-neighbour resized to that output size.
    pub fn render_frame(&self, t: f64, width: u32, height: u32) -> Result<Vec<u8>> {
        let g = self.lock()?;
        let frame = compose_frame_graded(&g.project, t, None)?;
        if width == 0 || height == 0 || (width == frame.width && height == frame.height) {
            Ok(frame.data)
        } else {
            Ok(resize_rgba_nearest(&frame.data, frame.width, frame.height, width, height))
        }
    }

    /// Serialize the current project to JSON (undo history is not persisted).
    pub fn save_json(&self) -> Result<String> {
        let g = self.lock()?;
        project_io::to_json(&g.project)
    }

    /// Replace the project from JSON and clear the undo/redo history.
    pub fn load_json(&self, json: &str) -> Result<()> {
        let project = project_io::from_json(json)?;
        let mut g = self.lock()?;
        g.project = project;
        g.undo.clear();
        g.redo.clear();
        Ok(())
    }

    /// Inspect the current project under the lock (for hosts/tests).
    pub fn with_project<R>(&self, f: impl FnOnce(&Project) -> R) -> Result<R> {
        let g = self.lock()?;
        Ok(f(&g.project))
    }

    /// A snapshot clone of the current project.
    pub fn project_snapshot(&self) -> Result<Project> {
        self.with_project(|p| p.clone())
    }

    // ── ergonomic builders (each wraps a command) ──────────────────────────

    /// Add an empty video track; returns its index.
    pub fn add_track(&self) -> Result<usize> {
        self.execute(Box::new(crate::commands::AddTrackCommand::new()))?;
        self.with_project(|p| p.tracks.len().saturating_sub(1))
    }

    /// Append a clip to a track.
    pub fn add_clip(&self, track: usize, clip: Clip) -> Result<()> {
        self.execute(Box::new(crate::commands::AddClipCommand::new(track, clip)))
    }

    /// Append a clip described by JSON (the FFI-friendly path).
    pub fn add_clip_json(&self, track: usize, clip_json: &str) -> Result<()> {
        let clip: Clip = serde_json::from_str(clip_json).map_err(|e| SdkError::Serde(e.to_string()))?;
        self.add_clip(track, clip)
    }
}

/// Nearest-neighbour resize of an RGBA8 buffer.
fn resize_rgba_nearest(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for oy in 0..dh {
        let sy = (oy * sh / dh).min(sh - 1);
        for ox in 0..dw {
            let sx = (ox * sw / dw).min(sw - 1);
            let si = ((sy * sw + sx) * 4) as usize;
            let di = ((oy * dw + ox) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}
