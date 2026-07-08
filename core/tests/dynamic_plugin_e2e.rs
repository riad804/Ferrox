//! End-to-end dynamic-loading test: `dlopen` the example plugin shared library,
//! adapt it, and run its effect. Gated behind `dynamic-plugins`; skips if the
//! example artifact hasn't been built (it is, under `cargo test --workspace`).
#![cfg(all(feature = "dynamic-plugins", not(target_arch = "wasm32")))]

use std::path::PathBuf;

use ferrox_core::plugin::{load_plugin, Plugin, VideoEffectPlugin};
use ferrox_core::{Frame, PixelFormat};

/// Path to the built `ferrox-example-plugin` shared library, if present.
fn example_artifact() -> Option<PathBuf> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent()?.to_path_buf();
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let (prefix, ext) = if cfg!(target_os = "windows") {
        ("", "dll")
    } else if cfg!(target_os = "macos") {
        ("lib", "dylib")
    } else {
        ("lib", "so")
    };
    let path = workspace.join("target").join(profile).join(format!("{prefix}ferrox_example_plugin.{ext}"));
    path.exists().then_some(path)
}

#[test]
fn loads_and_runs_the_example_plugin() {
    let Some(path) = example_artifact() else {
        eprintln!("skipping: build the example plugin first (`cargo build -p ferrox-example-plugin`)");
        return;
    };

    let plugin = load_plugin(&path).expect("load example plugin");
    assert_eq!(plugin.metadata().id, "ferrox.example.invert");
    assert_eq!(plugin.metadata().name, "Invert");

    let ve: &dyn VideoEffectPlugin = plugin.as_video_effect().expect("is a video effect");
    let frame = Frame::new(1, 1, PixelFormat::Rgba8, vec![10, 20, 30, 255]);
    let out = ve.apply_video(frame, &serde_json::Value::Null).unwrap();
    // RGB inverted, alpha preserved.
    assert_eq!(out.data.as_slice(), &[245, 235, 225, 255]);
}
