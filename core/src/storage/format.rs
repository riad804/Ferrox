//! The `.ferrox` document container: a versioned, optionally-compressed wrapper
//! around a JSON-serialised [`Project`].
//!
//! Byte layout (little-endian):
//! ```text
//! offset 0  : b"FRRX"            magic (4 bytes)
//! offset 4  : u16 schema_version (current = SCHEMA_VERSION)
//! offset 6  : u8  compression    (0 = none, 1 = deflate)
//! offset 7  : u8  reserved (0)
//! offset 8  : u32 payload_len    (bytes of the following payload)
//! offset 12 : payload            (JSON, deflate-compressed when flagged)
//! ```
//! The header is deliberately fixed-size and forward-scannable so a future
//! reader can reject or migrate unknown versions without parsing the payload.

use crate::error::{Error, Result};
use crate::timeline::Project;

use super::migration::migrate_to_current;

/// Container magic bytes.
pub const MAGIC: &[u8; 4] = b"FRRX";
/// Current on-disk schema version. Bump when the persisted shape changes and
/// add a migration step in [`super::migration`].
pub const SCHEMA_VERSION: u16 = 1;
/// Fixed header length in bytes.
pub const HEADER_LEN: usize = 12;

/// How the payload is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Deflate,
}

impl Compression {
    fn tag(self) -> u8 {
        match self {
            Compression::None => 0,
            Compression::Deflate => 1,
        }
    }
    fn from_tag(tag: u8) -> Result<Self> {
        match tag {
            0 => Ok(Compression::None),
            1 => Ok(Compression::Deflate),
            other => Err(Error::Filter(format!("unknown compression tag {other}"))),
        }
    }
}

/// Encode a project into a `.ferrox` container with the given compression.
pub fn encode(project: &Project, compression: Compression) -> Result<Vec<u8>> {
    let json = project.to_json()?;
    let payload = match compression {
        Compression::None => json.into_bytes(),
        Compression::Deflate => miniz_oxide::deflate::compress_to_vec(json.as_bytes(), 6),
    };
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    out.push(compression.tag());
    out.push(0); // reserved
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// The parsed header of a container, without decoding the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u16,
    pub compression: Compression,
    pub payload_len: usize,
}

/// Read and validate a container header.
pub fn read_header(bytes: &[u8]) -> Result<Header> {
    if bytes.len() < HEADER_LEN {
        return Err(Error::Filter("truncated ferrox container header".into()));
    }
    if &bytes[0..4] != MAGIC {
        return Err(Error::Filter("not a ferrox container (bad magic)".into()));
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    let compression = Compression::from_tag(bytes[6])?;
    let payload_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if version > SCHEMA_VERSION {
        return Err(Error::Filter(format!(
            "container schema v{version} is newer than supported v{SCHEMA_VERSION}"
        )));
    }
    Ok(Header { version, compression, payload_len })
}

/// Decode a `.ferrox` container back into a [`Project`], migrating older schema
/// versions forward as needed.
pub fn decode(bytes: &[u8]) -> Result<Project> {
    let header = read_header(bytes)?;
    let payload = &bytes[HEADER_LEN..];
    if payload.len() < header.payload_len {
        return Err(Error::Filter("truncated ferrox container payload".into()));
    }
    let payload = &payload[..header.payload_len];
    let json_bytes = match header.compression {
        Compression::None => payload.to_vec(),
        Compression::Deflate => miniz_oxide::inflate::decompress_to_vec(payload)
            .map_err(|e| Error::Filter(format!("deflate inflate failed: {e:?}")))?,
    };
    let json =
        String::from_utf8(json_bytes).map_err(|e| Error::Filter(format!("payload not UTF-8: {e}")))?;
    // Migrate the raw JSON value across schema versions, then deserialise.
    let migrated = migrate_to_current(&json, header.version)?;
    Project::from_json(&migrated)
}
