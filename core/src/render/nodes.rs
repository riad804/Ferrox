//! [`RenderNode`] and the built-in nodes of the render graph (Phase 4).
//!
//! Each node consumes zero or more input frames and produces one, executed
//! through a [`RenderBackend`]. Nodes covering a backend kernel (color, resize,
//! chroma-key, mask, composite) run on it (and thus on the GPU when available);
//! the rest use pure engine filters. A [`CustomNode`] wraps an arbitrary
//! closure — the seam for plugin render nodes and user-defined effects.

use crate::blend::BlendMode;
use crate::color::{ColorGrade, Lut3D};
use crate::error::{Error, Result};
use crate::filters::{BlurFilter, CropFilter};
use crate::frame::Frame;
use crate::keyer::Keyer;
use crate::mask::Mask;
use crate::timeline::ClipSource;
use crate::traits::Filter;

use super::backend::RenderBackend;

/// A node in the render graph: `input_count` inputs → one output frame.
pub trait RenderNode: Send + Sync {
    fn eval(&self, inputs: &[&Frame], backend: &dyn RenderBackend) -> Result<Frame>;
    fn input_count(&self) -> usize;
    fn name(&self) -> &str {
        "node"
    }
}

/// A leaf that renders a [`ClipSource`] (image / solid) — 0 inputs.
pub struct SourceNode {
    pub source: ClipSource,
}
impl RenderNode for SourceNode {
    fn eval(&self, _inputs: &[&Frame], _backend: &dyn RenderBackend) -> Result<Frame> {
        self.source.render()
    }
    fn input_count(&self) -> usize {
        0
    }
    fn name(&self) -> &str {
        "source"
    }
}

/// A solid-colour background leaf — 0 inputs.
pub struct SolidNode {
    pub width: u32,
    pub height: u32,
    pub rgba: [u8; 4],
}
impl RenderNode for SolidNode {
    fn eval(&self, _inputs: &[&Frame], _backend: &dyn RenderBackend) -> Result<Frame> {
        let data = self.rgba.repeat((self.width * self.height) as usize);
        Ok(Frame::new(self.width, self.height, crate::frame::PixelFormat::Rgba8, data))
    }
    fn input_count(&self) -> usize {
        0
    }
    fn name(&self) -> &str {
        "solid"
    }
}

macro_rules! unary_backend_node {
    ($name:ident, $tag:literal, $field:ident: $ty:ty, $call:ident) => {
        pub struct $name {
            pub $field: $ty,
        }
        impl RenderNode for $name {
            fn eval(&self, inputs: &[&Frame], backend: &dyn RenderBackend) -> Result<Frame> {
                backend.$call(inputs[0].clone(), &self.$field)
            }
            fn input_count(&self) -> usize {
                1
            }
            fn name(&self) -> &str {
                $tag
            }
        }
    };
}

unary_backend_node!(ColorNode, "color", grade: ColorGrade, color_grade);
unary_backend_node!(ChromaKeyNode, "chroma_key", keyer: Keyer, chroma_key);
unary_backend_node!(MaskNode, "mask", mask: Mask, apply_mask);

/// Resize (a Transform's scale) — 1 input, via the backend.
pub struct ResizeNode {
    pub width: u32,
    pub height: u32,
}
impl RenderNode for ResizeNode {
    fn eval(&self, inputs: &[&Frame], backend: &dyn RenderBackend) -> Result<Frame> {
        backend.resize(inputs[0].clone(), self.width, self.height)
    }
    fn input_count(&self) -> usize {
        1
    }
    fn name(&self) -> &str {
        "resize"
    }
}

/// Apply a 3D LUT — 1 input (CPU; no backend kernel yet).
pub struct LutNode {
    pub lut: Lut3D,
}
impl RenderNode for LutNode {
    fn eval(&self, inputs: &[&Frame], _backend: &dyn RenderBackend) -> Result<Frame> {
        let mut f = inputs[0].clone();
        self.lut.apply_frame(&mut f)?;
        Ok(f)
    }
    fn input_count(&self) -> usize {
        1
    }
    fn name(&self) -> &str {
        "lut"
    }
}

/// Gaussian blur — 1 input.
pub struct BlurNode {
    pub sigma: f32,
}
impl RenderNode for BlurNode {
    fn eval(&self, inputs: &[&Frame], _backend: &dyn RenderBackend) -> Result<Frame> {
        BlurFilter::new(self.sigma).process(inputs[0].clone())
    }
    fn input_count(&self) -> usize {
        1
    }
    fn name(&self) -> &str {
        "blur"
    }
}

/// Crop a sub-rectangle — 1 input.
pub struct CropNode {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl RenderNode for CropNode {
    fn eval(&self, inputs: &[&Frame], _backend: &dyn RenderBackend) -> Result<Frame> {
        CropFilter::new(self.x, self.y, self.width, self.height).process(inputs[0].clone())
    }
    fn input_count(&self) -> usize {
        1
    }
    fn name(&self) -> &str {
        "crop"
    }
}

/// Composite input\[1\] (top) onto input\[0\] (base) — 2 inputs, via the backend.
pub struct CompositeNode {
    pub x: i32,
    pub y: i32,
    pub opacity: f32,
    pub mode: BlendMode,
}
impl RenderNode for CompositeNode {
    fn eval(&self, inputs: &[&Frame], backend: &dyn RenderBackend) -> Result<Frame> {
        let mut base = inputs[0].clone();
        backend.composite(&mut base, inputs[1], self.x, self.y, self.opacity, self.mode)?;
        Ok(base)
    }
    fn input_count(&self) -> usize {
        2
    }
    fn name(&self) -> &str {
        "composite"
    }
}

/// An arbitrary custom node — the extension seam for plugin render nodes and
/// user effects.
pub struct CustomNode {
    inputs: usize,
    #[allow(clippy::type_complexity)]
    func: Box<dyn Fn(&[&Frame], &dyn RenderBackend) -> Result<Frame> + Send + Sync>,
}
impl CustomNode {
    pub fn new(inputs: usize, func: impl Fn(&[&Frame], &dyn RenderBackend) -> Result<Frame> + Send + Sync + 'static) -> Self {
        Self { inputs, func: Box::new(func) }
    }
}
impl RenderNode for CustomNode {
    fn eval(&self, inputs: &[&Frame], backend: &dyn RenderBackend) -> Result<Frame> {
        if inputs.len() != self.inputs {
            return Err(Error::Filter(format!("custom node expected {} inputs, got {}", self.inputs, inputs.len())));
        }
        (self.func)(inputs, backend)
    }
    fn input_count(&self) -> usize {
        self.inputs
    }
    fn name(&self) -> &str {
        "custom"
    }
}
