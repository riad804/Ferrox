//! Phase 14 project storage: container round-trip, compression, header parsing,
//! schema migration, snapshot history / crash recovery, and structural diff.

use ferrox_core::storage::{
    decode, diff, encode, read_header, Change, Compression, SnapshotHistory, HEADER_LEN, MAGIC,
    SCHEMA_VERSION,
};
use ferrox_core::timeline::{Clip, ClipSource, Track, Transform};
use ferrox_core::Project;

fn solid(r: u8, g: u8, b: u8) -> ClipSource {
    ClipSource::Solid { width: 16, height: 16, r, g, b, a: 255 }
}

fn clip(source: ClipSource, start: f64, duration: f64) -> Clip {
    Clip::new(source, start, duration, Transform::default())
}

fn sample_project() -> Project {
    Project::new(1920, 1080, 30.0)
        .with_background(10, 20, 30)
        .with_track(Track::new().with_clip(clip(solid(255, 0, 0), 0.0, 2.0)))
}

#[test]
fn round_trips_uncompressed() {
    let p = sample_project();
    let bytes = encode(&p, Compression::None).unwrap();
    assert_eq!(&bytes[0..4], MAGIC);
    assert_eq!(decode(&bytes).unwrap(), p);
}

#[test]
fn round_trips_compressed() {
    let p = sample_project();
    let bytes = encode(&p, Compression::Deflate).unwrap();
    assert_eq!(decode(&bytes).unwrap(), p);
}

#[test]
fn compression_shrinks_repetitive_projects() {
    // A project with many identical clips compresses well.
    let mut track = Track::new();
    for _ in 0..200 {
        track = track.with_clip(clip(solid(7, 7, 7), 0.0, 1.0));
    }
    let p = Project::new(1280, 720, 30.0).with_track(track);
    let raw = encode(&p, Compression::None).unwrap();
    let zip = encode(&p, Compression::Deflate).unwrap();
    assert!(zip.len() < raw.len(), "deflate should shrink: {} vs {}", zip.len(), raw.len());
    assert_eq!(decode(&zip).unwrap(), decode(&raw).unwrap());
}

#[test]
fn header_reports_version_and_compression() {
    let bytes = encode(&sample_project(), Compression::Deflate).unwrap();
    let h = read_header(&bytes).unwrap();
    assert_eq!(h.version, SCHEMA_VERSION);
    assert_eq!(h.compression, Compression::Deflate);
    assert_eq!(h.payload_len, bytes.len() - HEADER_LEN);
}

#[test]
fn rejects_bad_magic_and_truncation() {
    assert!(decode(b"XXXX").is_err(), "bad magic");
    assert!(decode(&[]).is_err(), "empty");
    let mut bytes = encode(&sample_project(), Compression::None).unwrap();
    bytes.truncate(HEADER_LEN + 2); // chop the payload
    assert!(decode(&bytes).is_err(), "truncated payload");
}

#[test]
fn rejects_future_schema_version() {
    let mut bytes = encode(&sample_project(), Compression::None).unwrap();
    // Bump the stored version past what we support.
    let future = (SCHEMA_VERSION + 1).to_le_bytes();
    bytes[4] = future[0];
    bytes[5] = future[1];
    assert!(read_header(&bytes).is_err());
    assert!(decode(&bytes).is_err());
}

#[test]
fn loads_legacy_v0_loose_json() {
    // Simulate a pre-container v0 document: hand-build a container with an old
    // header version wrapping loose JSON (no audio fields). Migration + serde
    // defaults must bring it up to a valid current Project.
    let json = r#"{"width":640,"height":480,"fps":24.0}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&0u16.to_le_bytes()); // version 0
    bytes.push(0); // no compression
    bytes.push(0); // reserved
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(json.as_bytes());

    let p = decode(&bytes).unwrap();
    assert_eq!((p.width, p.height), (640, 480));
    assert_eq!(p.fps, 24.0);
    assert_eq!(p.sample_rate, 48_000, "serde default fills v0 gap");
    assert_eq!(p.channels, 2);
    assert!(p.audio_tracks.is_empty());
}

// ── snapshot history / recovery ─────────────────────────────────────────────

#[test]
fn snapshot_history_recovers_latest() {
    let mut hist = SnapshotHistory::new(4);
    assert!(hist.is_empty());
    assert!(hist.recover().is_err(), "nothing to recover yet");

    let mut p = sample_project();
    hist.capture(&p, 0.0).unwrap();
    p.fps = 60.0;
    let rev = hist.capture(&p, 1.0).unwrap();

    assert_eq!(hist.len(), 2);
    assert_eq!(hist.recover().unwrap().fps, 60.0, "latest wins");
    assert_eq!(hist.get(rev).unwrap().to_project().unwrap().fps, 60.0);
    assert_eq!(hist.latest().unwrap().revision, rev);
    assert!(hist.total_bytes() > 0);
}

#[test]
fn snapshot_history_evicts_oldest_at_capacity() {
    let mut hist = SnapshotHistory::new(2);
    for i in 0..5 {
        let mut p = sample_project();
        p.width = 100 + i;
        hist.capture(&p, i as f64).unwrap();
    }
    assert_eq!(hist.len(), 2, "capacity enforced");
    let revs: Vec<u64> = hist.iter().map(|s| s.revision).collect();
    assert_eq!(revs, vec![3, 4], "oldest evicted, monotonic revisions kept");
    assert_eq!(hist.recover().unwrap().width, 104);
}

// ── diff ────────────────────────────────────────────────────────────────────

#[test]
fn diff_of_identical_projects_is_empty() {
    let p = sample_project();
    assert!(diff(&p, &p).is_empty());
}

#[test]
fn diff_detects_field_and_track_changes() {
    let old = sample_project();
    let mut new = old.clone();
    new.fps = 60.0;
    new.width = 3840;
    new.tracks.push(Track::new()); // added track
    // change a clip in track 0
    new.tracks[0].clips[0].duration = 5.0;

    let d = diff(&old, &new);
    assert!(d.changes.contains(&Change::Field("fps")));
    assert!(d.changes.contains(&Change::Field("width")));
    assert!(d.changes.contains(&Change::TrackAdded(1)));
    assert!(d.changes.contains(&Change::ClipChanged { track: 0, clip: 0 }));
}

#[test]
fn diff_detects_track_removal_and_clip_count() {
    let old = sample_project();
    let mut with_two = old.clone();
    with_two.tracks[0] = with_two.tracks[0].clone().with_clip(clip(solid(0, 0, 0), 2.0, 1.0));

    // old→with_two: track 0 gained a clip.
    let d = diff(&old, &with_two);
    assert!(d
        .changes
        .iter()
        .any(|c| matches!(c, Change::TrackClipCount { index: 0, old: 1, new: 2 })));

    // with_two→old-with-no-tracks: a track was removed.
    let empty = Project::new(1920, 1080, 30.0);
    let d2 = diff(&with_two, &empty);
    assert!(d2.changes.contains(&Change::TrackRemoved(0)));
}
