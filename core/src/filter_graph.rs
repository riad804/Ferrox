//! A simple named-pad filter graph inspired by FFmpeg's filtergraph.
//!
//! # Overview
//!
//! A [`FilterGraph`] is a directed graph of [`FilterNode`]s connected by named
//! pads. Frames flow from *source pads* through one or more nodes to *sink
//! pads*.
//!
//! ## Minimal example
//!
//! ```
//! use ferrox_core::{
//!     filter_graph::FilterGraph,
//!     filters::{BlurFilter, GrayscaleFilter},
//!     frame::{Frame, PixelFormat},
//! };
//!
//! let mut graph = FilterGraph::new();
//! graph.add_filter("blur", BlurFilter::new(2.0));
//! graph.add_filter("gray", GrayscaleFilter);
//! graph.connect("blur", "gray");
//! let frame = Frame::new(8, 8, PixelFormat::Rgb8, vec![128u8; 8*8*3]);
//! let out = graph.run(frame, "blur", "gray").unwrap();
//! assert_eq!(out.width, 8);
//! ```

use crate::{
    error::{Error, Result},
    frame::Frame,
    traits::Filter as _Filter,
};
use std::collections::HashMap;

// ── FilterPlugin ──────────────────────────────────────────────────────────────

/// Trait that every filter node in a [`FilterGraph`] must implement.
///
/// This is the extension point for user-defined filters:
///
/// ```
/// use ferrox_core::{filter_graph::FilterPlugin, frame::Frame, error::Result};
///
/// struct InvertRed;
///
/// impl FilterPlugin for InvertRed {
///     fn name(&self) -> &str { "invert_red" }
///     fn process(&self, frame: Frame) -> Result<Frame> {
///         let mut f = frame;
///         for px in f.data.chunks_exact_mut(3) {
///             px[0] = 255 - px[0];
///         }
///         Ok(f)
///     }
/// }
/// ```
pub trait FilterPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn process(&self, frame: Frame) -> Result<Frame>;
}

/// Adapts any [`crate::traits::Filter`] into a [`FilterPlugin`].
struct PluginAdapter<F: crate::traits::Filter> {
    label: String,
    inner: F,
}

impl<F: crate::traits::Filter + 'static> FilterPlugin for PluginAdapter<F> {
    fn name(&self) -> &str { &self.label }
    fn process(&self, frame: Frame) -> Result<Frame> { self.inner.process(frame) }
}

// ── FilterGraph ───────────────────────────────────────────────────────────────

/// Ordered list of named filter nodes with explicit connection edges.
///
/// Supports:
/// - Linear chains (`a → b → c`)
/// - Splits (one source → many sinks)
/// - Merges / overlays (many sources → one sink, via custom plugin)
pub struct FilterGraph {
    nodes: HashMap<String, Box<dyn FilterPlugin>>,
    /// Adjacency list: node_name → list of successor node names.
    edges: HashMap<String, Vec<String>>,
    /// Insertion-order list of node names (for iterating in order).
    order: Vec<String>,
}

impl FilterGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Register a filter node.
    pub fn add_node(&mut self, name: impl Into<String>, plugin: Box<dyn FilterPlugin>) {
        let name = name.into();
        self.edges.entry(name.clone()).or_default();
        self.order.push(name.clone());
        self.nodes.insert(name, plugin);
    }

    /// Convenience: wrap any [`crate::traits::Filter`] and add it.
    pub fn add_filter<F: crate::traits::Filter + 'static>(
        &mut self,
        name: impl Into<String>,
        filter: F,
    ) {
        let name = name.into();
        let plugin = Box::new(PluginAdapter { label: name.clone(), inner: filter });
        self.add_node(name, plugin);
    }

    /// Connect `from` → `to` (adds a directed edge).
    pub fn connect(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.edges
            .entry(from.into())
            .or_default()
            .push(to.into());
    }

    /// Run a frame through a linear path: `entry` node → … → `exit` node.
    ///
    /// Follows the topological insertion order starting from `entry`, stopping
    /// after `exit`. Branching paths are ignored (only the first successor
    /// edge is followed per node).
    pub fn run(&self, frame: Frame, entry: &str, exit: &str) -> Result<Frame> {
        if !self.nodes.contains_key(entry) {
            return Err(Error::Filter(format!("filter_graph: unknown node '{entry}'")));
        }
        if !self.nodes.contains_key(exit) {
            return Err(Error::Filter(format!("filter_graph: unknown node '{exit}'")));
        }

        let mut current = entry.to_string();
        let mut frame = frame;

        loop {
            let node = self.nodes.get(&current).ok_or_else(|| {
                Error::Filter(format!("filter_graph: missing node '{current}'"))
            })?;
            frame = node.process(frame)?;

            if current == exit {
                break;
            }

            let succs = self.edges.get(&current).map(|v| v.as_slice()).unwrap_or(&[]);
            if succs.is_empty() {
                return Err(Error::Filter(format!(
                    "filter_graph: no path from '{current}' to '{exit}'"
                )));
            }
            current = succs[0].clone();
        }

        Ok(frame)
    }

    /// Run a frame through **all** nodes in insertion order.
    pub fn run_all(&self, frame: Frame) -> Result<Frame> {
        let mut frame = frame;
        for name in &self.order {
            let node = self.nodes.get(name).unwrap();
            frame = node.process(frame)?;
        }
        Ok(frame)
    }

    /// Parse a simplified filtergraph expression and run the frame through it.
    ///
    /// Syntax (subset of FFmpeg's -filter_complex):
    /// ```text
    /// scale=640:480,blur=sigma:2.0,grayscale
    /// ```
    /// Supported tokens: `blur=sigma:<f32>`, `grayscale`, `negate`,
    /// `scale=<w>:<h>`, `brightness=<i32>`, `contrast=<f32>`,
    /// `saturation=<f32>`.
    pub fn parse_and_run(frame: Frame, expr: &str) -> Result<Frame> {
        use crate::filters::{
            BlurFilter, GrayscaleFilter, NegateFilter,
            BrightnessFilter, ContrastFilter, SaturationFilter,
            ResizeFilter,
        };

        let mut f = frame;
        for token in expr.split(',') {
            let token = token.trim();
            if token.is_empty() { continue; }

            if token == "grayscale" || token == "gray" {
                f = GrayscaleFilter.process(f)?;
            } else if token == "negate" || token == "invert" {
                f = NegateFilter.process(f)?;
            } else if let Some(rest) = token.strip_prefix("blur=sigma:").or(token.strip_prefix("blur=")) {
                let sigma: f32 = rest.parse().map_err(|_| {
                    Error::Filter(format!("blur: expected float sigma, got '{rest}'"))
                })?;
                f = BlurFilter::new(sigma).process(f)?;
            } else if let Some(rest) = token.strip_prefix("brightness=") {
                let delta: i32 = rest.parse().map_err(|_| {
                    Error::Filter(format!("brightness: expected i32 delta, got '{rest}'"))
                })?;
                f = BrightnessFilter::new(delta).process(f)?;
            } else if let Some(rest) = token.strip_prefix("contrast=") {
                let factor: f32 = rest.parse().map_err(|_| {
                    Error::Filter(format!("contrast: expected float factor, got '{rest}'"))
                })?;
                f = ContrastFilter::new(factor).process(f)?;
            } else if let Some(rest) = token.strip_prefix("saturation=") {
                let factor: f32 = rest.parse().map_err(|_| {
                    Error::Filter(format!("saturation: expected float factor, got '{rest}'"))
                })?;
                f = SaturationFilter::new(factor).process(f)?;
            } else if let Some(rest) = token.strip_prefix("scale=") {
                let parts: Vec<&str> = rest.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return Err(Error::Filter(format!("scale: expected W:H, got '{rest}'")));
                }
                let w: u32 = parts[0].parse().map_err(|_| {
                    Error::Filter(format!("scale: bad width '{}'", parts[0]))
                })?;
                let h: u32 = parts[1].parse().map_err(|_| {
                    Error::Filter(format!("scale: bad height '{}'", parts[1]))
                })?;
                f = ResizeFilter::new(w, h).process(f)?;
            } else {
                return Err(Error::Filter(format!(
                    "filter_graph: unknown filter token '{token}'"
                )));
            }
        }
        Ok(f)
    }

    /// Node count.
    pub fn len(&self) -> usize { self.nodes.len() }

    pub fn is_empty(&self) -> bool { self.nodes.is_empty() }
}

impl Default for FilterGraph {
    fn default() -> Self { Self::new() }
}
