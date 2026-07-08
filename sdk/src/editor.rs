//! The [`Editor`] — the handle-based facade all consumers (WASM, UniFFI, native)
//! drive. It is `Clone` and thread-safe: internal state lives behind an
//! `Arc<RwLock<..>>` (read-heavy: rendering/queries take read locks, commands
//! take write locks). All edits flow through the [`Command`] stack, giving
//! undo/redo for free, and publish [`Event`]s through an injected [`EventSink`].
//!
//! `std::sync::RwLock` (poison-recovered) is used rather than `parking_lot` so
//! the SDK compiles unchanged to `wasm32`.

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use ferrox_core::plugin::{register_builtins, CapabilitySet, PluginManager, PLUGIN_API_VERSION};
use ferrox_core::{compose_frame_graded, Clip, Event, EventSink, NoopSink, Project};

use crate::commands::Command;
use crate::error::{Result, SdkError};
use crate::project_io;

/// Internal mutable state: the project plus the undo/redo stacks.
struct Inner {
    project: Project,
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
}

/// Builder for [`Editor`] — the Dependency-Injection entry point. Wires optional
/// ports (currently the [`EventSink`]); more ports (storage, executor, gpu) plug
/// in here in later phases.
pub struct EditorBuilder {
    project: Project,
    events: Arc<dyn EventSink>,
    plugins: Option<Arc<PluginManager>>,
}

impl EditorBuilder {
    fn new(project: Project) -> Self {
        Self { project, events: Arc::new(NoopSink), plugins: None }
    }

    /// Inject an event sink (defaults to a no-op).
    pub fn with_event_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.events = sink;
        self
    }

    /// Inject a pre-built plugin manager (defaults to one with the built-ins
    /// registered and wired to this editor's event sink).
    pub fn with_plugin_manager(mut self, plugins: Arc<PluginManager>) -> Self {
        self.plugins = Some(plugins);
        self
    }

    /// Construct the editor.
    pub fn build(self) -> Editor {
        let plugins = self.plugins.unwrap_or_else(|| {
            // Register built-ins silently, then wire the editor's bus so only
            // runtime plugin changes emit events.
            let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
            register_builtins(&mgr).expect("built-in plugins register");
            mgr.set_event_sink(Arc::clone(&self.events));
            Arc::new(mgr)
        });
        Editor {
            inner: Arc::new(RwLock::new(Inner { project: self.project, undo: Vec::new(), redo: Vec::new() })),
            events: self.events,
            plugins,
        }
    }
}

/// A cloneable, thread-safe handle to an editing session.
#[derive(Clone)]
pub struct Editor {
    inner: Arc<RwLock<Inner>>,
    events: Arc<dyn EventSink>,
    plugins: Arc<PluginManager>,
}

impl Editor {
    /// Start building an editor with an empty project of the given output spec.
    pub fn builder(width: u32, height: u32, fps: f64) -> EditorBuilder {
        EditorBuilder::new(Project::new(width, height, fps))
    }

    /// Start building an editor from an existing project.
    pub fn builder_from_project(project: Project) -> EditorBuilder {
        EditorBuilder::new(project)
    }

    /// A new editor with an empty project (no event sink).
    pub fn new(width: u32, height: u32, fps: f64) -> Self {
        Self::builder(width, height, fps).build()
    }

    /// Wrap an existing project (no event sink).
    pub fn from_project(project: Project) -> Self {
        Self::builder_from_project(project).build()
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, event: Event) {
        self.events.publish(event);
    }

    /// Run a command: apply it, push to the undo stack, clear the redo stack.
    pub fn execute(&self, mut cmd: Box<dyn Command>) -> Result<()> {
        {
            let mut g = self.write();
            cmd.apply(&mut g.project)?;
            g.undo.push(cmd);
            g.redo.clear();
        }
        self.emit(Event::ProjectChanged);
        Ok(())
    }

    /// Undo the most recent command. Returns `false` if nothing to undo.
    pub fn undo(&self) -> Result<bool> {
        let undone = {
            let mut g = self.write();
            match g.undo.pop() {
                Some(mut cmd) => {
                    cmd.revert(&mut g.project)?;
                    g.redo.push(cmd);
                    true
                }
                None => false,
            }
        };
        if undone {
            self.emit(Event::Undo);
        }
        Ok(undone)
    }

    /// Redo the most recently undone command. Returns `false` if nothing to redo.
    pub fn redo(&self) -> Result<bool> {
        let redone = {
            let mut g = self.write();
            match g.redo.pop() {
                Some(mut cmd) => {
                    cmd.apply(&mut g.project)?;
                    g.undo.push(cmd);
                    true
                }
                None => false,
            }
        };
        if redone {
            self.emit(Event::Redo);
        }
        Ok(redone)
    }

    /// Depth of the undo stack.
    pub fn undo_depth(&self) -> usize {
        self.read().undo.len()
    }

    /// Depth of the redo stack.
    pub fn redo_depth(&self) -> usize {
        self.read().redo.len()
    }

    /// Render the composed output frame at time `t` as RGBA8 bytes. If `width`/
    /// `height` are non-zero and differ from the project size, the composite is
    /// nearest-neighbour resized to that output size.
    pub fn render_frame(&self, t: f64, width: u32, height: u32) -> Result<Vec<u8>> {
        let g = self.read();
        let frame = compose_frame_graded(&g.project, t, None)?;
        if width == 0 || height == 0 || (width == frame.width && height == frame.height) {
            Ok(frame.data)
        } else {
            Ok(resize_rgba_nearest(&frame.data, frame.width, frame.height, width, height))
        }
    }

    /// Serialize the current project to JSON (undo history is not persisted).
    pub fn save_json(&self) -> Result<String> {
        project_io::to_json(&self.read().project)
    }

    /// Replace the project from JSON and clear the undo/redo history.
    pub fn load_json(&self, json: &str) -> Result<()> {
        let project = project_io::from_json(json)?;
        {
            let mut g = self.write();
            g.project = project;
            g.undo.clear();
            g.redo.clear();
        }
        self.emit(Event::ProjectChanged);
        Ok(())
    }

    /// Inspect the current project under a read lock (for hosts/tests).
    pub fn with_project<R>(&self, f: impl FnOnce(&Project) -> R) -> Result<R> {
        Ok(f(&self.read().project))
    }

    /// A snapshot clone of the current project.
    pub fn project_snapshot(&self) -> Result<Project> {
        self.with_project(|p| p.clone())
    }

    /// The plugin manager for this editor (built-ins registered; shares the
    /// editor's event bus).
    pub fn plugins(&self) -> Arc<PluginManager> {
        Arc::clone(&self.plugins)
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
