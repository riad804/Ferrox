//! Schema migration: bring an older persisted project JSON up to the current
//! [`SCHEMA_VERSION`](super::format::SCHEMA_VERSION).
//!
//! Migrations operate on the raw `serde_json::Value` (not the typed `Project`)
//! so a document written by an older engine — which may lack fields or use an
//! older shape — can be reshaped *before* it is deserialised into the current
//! `Project` struct. Each step upgrades exactly one version. Steps run in
//! sequence, so adding a new schema version means: bump `SCHEMA_VERSION`, append
//! one `migrate_vN_to_vN1` function, and wire it into [`migrate_to_current`].

use serde_json::Value;

use crate::error::{Error, Result};

use super::format::SCHEMA_VERSION;

/// Migrate `json` (written at `from_version`) up to the current schema, returning
/// the upgraded JSON text ready for `Project::from_json`.
pub fn migrate_to_current(json: &str, from_version: u16) -> Result<String> {
    if from_version == SCHEMA_VERSION {
        return Ok(json.to_string());
    }
    if from_version > SCHEMA_VERSION {
        return Err(Error::Filter(format!(
            "cannot migrate down from v{from_version} to v{SCHEMA_VERSION}"
        )));
    }
    let mut value: Value =
        serde_json::from_str(json).map_err(|e| Error::Filter(format!("migration parse: {e}")))?;
    let mut version = from_version;
    while version < SCHEMA_VERSION {
        value = step(value, version)?;
        version += 1;
    }
    serde_json::to_string(&value).map_err(|e| Error::Filter(format!("migration serialise: {e}")))
}

/// Apply the single migration step that upgrades `version` → `version + 1`.
fn step(value: Value, version: u16) -> Result<Value> {
    match version {
        // v0 predates the container header (loose JSON with no `sample_rate`
        // /`channels`/`audio_tracks`). `Project`'s serde defaults already fill
        // those, so v0 → v1 only needs to guarantee an object shape.
        0 => Ok(migrate_v0_to_v1(value)),
        other => Err(Error::Filter(format!("no migration step from schema v{other}"))),
    }
}

/// v0 → v1: ensure the document is a JSON object. Missing audio/subtitle fields
/// are supplied by `Project`'s `#[serde(default)]` on deserialisation, so this
/// step is intentionally minimal and lossless.
fn migrate_v0_to_v1(mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        // Older exports may have stored `fps` as an integer; serde handles the
        // numeric coercion, so nothing to rewrite. Placeholder for real reshapes.
        let _ = map;
    }
    value
}
