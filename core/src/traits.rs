use std::io::{Read, Write};
use crate::{Frame, Result};

/// Identifies a media format.
pub trait Format: Send + Sync {
    fn name(&self) -> &str;
    fn extensions(&self) -> &[&str];
    fn mime_type(&self) -> &str;
}

/// Generic (statically-dispatched) decoder — prefer this in concrete code.
pub trait Decoder: Send + Sync {
    fn decode<R: Read>(&self, reader: R) -> Result<Frame>;
}

/// Generic (statically-dispatched) encoder — prefer this in concrete code.
pub trait Encoder: Send + Sync {
    fn encode<W: Write>(&self, frame: &Frame, writer: W) -> Result<()>;
}

/// Object-safe decoder used by the registry and graph.
///
/// Implementations are provided automatically via the blanket impl below for
/// any type that implements [`Decoder`].
pub trait DynDecoder: Send + Sync {
    fn decode_dyn(&self, reader: &mut dyn Read) -> Result<Frame>;
}

/// Object-safe encoder used by the registry and graph.
pub trait DynEncoder: Send + Sync {
    fn encode_dyn(&self, frame: &Frame, writer: &mut dyn Write) -> Result<()>;
}

impl<T: Decoder> DynDecoder for T {
    fn decode_dyn(&self, reader: &mut dyn Read) -> Result<Frame> {
        self.decode(reader)
    }
}

impl<T: Encoder> DynEncoder for T {
    fn encode_dyn(&self, frame: &Frame, writer: &mut dyn Write) -> Result<()> {
        self.encode(frame, writer)
    }
}

/// Transforms a [`Frame`], returning a (possibly new) frame.
pub trait Filter: Send + Sync {
    fn process(&self, frame: Frame) -> Result<Frame>;
}
