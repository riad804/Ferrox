//! [`CpuBackend`] — the always-available reference [`RenderBackend`],
//! delegating each kernel to the engine's pure-Rust implementations. This is the
//! correctness baseline the GPU backend is checked against.

use crate::blend::{composite_over, BlendMode};
use crate::color::ColorGrade;
use crate::error::Result;
use crate::frame::Frame;
use crate::keyer::Keyer;
use crate::mask::Mask;
use crate::traits::Filter;
use crate::ResizeFilter;

use super::backend::{Capabilities, RenderBackend};

/// The CPU rendering backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuBackend;

impl RenderBackend for CpuBackend {
    fn name(&self) -> &str {
        "cpu"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { gpu_accelerated: false }
    }

    fn color_grade(&self, mut frame: Frame, grade: &ColorGrade) -> Result<Frame> {
        grade.apply_frame(&mut frame)?;
        Ok(frame)
    }

    fn resize(&self, frame: Frame, width: u32, height: u32) -> Result<Frame> {
        ResizeFilter::new(width, height).process(frame)
    }

    fn chroma_key(&self, mut frame: Frame, keyer: &Keyer) -> Result<Frame> {
        keyer.apply_frame(&mut frame)?;
        Ok(frame)
    }

    fn apply_mask(&self, mut frame: Frame, mask: &Mask) -> Result<Frame> {
        mask.apply_frame(&mut frame)?;
        Ok(frame)
    }

    fn composite(&self, base: &mut Frame, top: &Frame, x: i32, y: i32, opacity: f32, mode: BlendMode) -> Result<()> {
        composite_over(base, top, x, y, opacity, mode);
        Ok(())
    }
}
