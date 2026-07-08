//! End-to-end export test: build a project, render across the time axis, encode
//! AV1, and mux to a fragmented MP4. Validates the whole pipeline produces a
//! well-formed, non-trivial file.

#![cfg(feature = "export")]

use ferrox_sdk::{export_mp4, AscCdl, Clip, ClipAnimation, ClipSource, ColorGrade, Curve, Easing, Editor, ExportSettings, Keyframe, Transform};

fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> ClipSource {
    ClipSource::Solid { width: w, height: h, r, g, b, a: 255 }
}

#[test]
fn exports_a_playable_mp4_with_time_variance() {
    // A 2-second project: red bg clip + a green clip that fades in and slides —
    // exercises timeline sampling, keyframes, and color grade during export.
    let e = Editor::new(64, 48, 30.0);
    let t0 = e.add_track().unwrap();
    let t1 = e.add_track().unwrap();
    e.add_clip(t0, Clip::new(solid(64, 48, 180, 30, 30), 0.0, 2.0, Transform::default())).unwrap();

    let green = Clip::new(solid(32, 24, 30, 200, 30), 0.0, 2.0, Transform::at(4, 4)).with_animation(ClipAnimation {
        opacity: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 1.0)])),
        x: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0).with_ease(Easing::EaseInOut), Keyframe::new(2.0, 20.0)])),
        ..Default::default()
    });
    e.add_clip(t1, green).unwrap();
    e.execute(Box::new(ferrox_sdk::commands::SetColorGradeCommand::new(
        t0,
        0,
        ColorGrade::from_cdl(AscCdl { slope: [1.1, 1.0, 1.0], ..Default::default() }),
    )))
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.mp4");
    let settings = ExportSettings { width: 64, height: 48, fps_num: 30, fps_den: 1, speed: 10, quantizer: 120 };

    let mut last = 0u32;
    export_mp4(&e, &path, &settings, |done, total| {
        assert!(done <= total && done >= last);
        last = done;
    })
    .unwrap();

    // 2s @ 30fps → 60 frames of progress.
    assert_eq!(last, 60, "progress reached the final frame");

    // File exists, is non-trivial, and begins with an ISO-BMFF `ftyp` box.
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 1000, "exported file has real data ({} bytes)", bytes.len());
    assert_eq!(&bytes[4..8], b"ftyp", "MP4 starts with an ftyp box");
}

#[test]
fn export_size_defaults_and_override() {
    let e = Editor::new(32, 32, 24.0);
    let t = e.add_track().unwrap();
    e.add_clip(t, Clip::new(solid(32, 32, 10, 20, 30), 0.0, 0.5, Transform::default())).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.mp4");
    // Override to a different output resolution.
    let settings = ExportSettings { width: 48, height: 48, fps_num: 24, fps_den: 1, speed: 10, quantizer: 150 };
    export_mp4(&e, &path, &settings, |_, _| {}).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn exported_mp4_parses_with_the_demuxer() {
    // The compatibility win: the progressive MP4 must parse with the container
    // demuxer that *fails* on fragmented output.
    use ferrox_core::codecs::video::Mp4Demuxer;
    use ferrox_core::traits::ContainerDemuxer;

    let e = Editor::new(48, 32, 30.0);
    let t = e.add_track().unwrap();
    e.add_clip(t, Clip::new(solid(48, 32, 100, 50, 25), 0.0, 0.5, Transform::default())).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("compat.mp4");
    export_mp4(&e, &path, &ExportSettings { width: 48, height: 32, fps_num: 30, fps_den: 1, speed: 10, quantizer: 150 }, |_, _| {}).unwrap();

    let file = std::fs::File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let demux = Mp4Demuxer::open(file, size).expect("progressive MP4 must parse");
    let streams = demux.streams();
    assert_eq!(streams.len(), 1, "one video track");
    assert!(streams[0].is_video());
    assert_eq!((streams[0].width, streams[0].height), (48, 32), "track dimensions round-trip");
}
