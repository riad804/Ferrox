//! Phase 1 plugin system: version + capability negotiation, lifecycle,
//! registry lookup, event emission, and built-in effect adapters running real
//! engine code.

use std::sync::{Arc, Mutex};

use ferrox_core::plugin::{
    register_builtins, AudioFxPlugin, Capability, CapabilitySet, ColorGradePlugin, KeyerPlugin,
    Lifecycle, MaskPlugin, Plugin, PluginError, PluginKind, PluginManager, PluginMetadata,
    TransitionBuiltin, Version, PLUGIN_API_VERSION,
};
use ferrox_core::{AudioFrame, Event, EventListener, EventSink, Frame, InProcessBus, PixelFormat};

// ── version negotiation ─────────────────────────────────────────────────────

#[test]
fn version_parse_and_compat() {
    assert_eq!(Version::parse("1.2.3"), Some(Version::new(1, 2, 3)));
    assert_eq!(Version::parse("2"), Some(Version::new(2, 0, 0)));
    // caret rule: host 1.4.0 runs a plugin needing 1.2.0, but not 2.0.0 or 1.5.0.
    let host = Version::new(1, 4, 0);
    assert!(host.is_compatible_with(&Version::new(1, 2, 0)));
    assert!(!host.is_compatible_with(&Version::new(2, 0, 0)));
    assert!(!host.is_compatible_with(&Version::new(1, 5, 0)));
}

// ── a minimal test plugin ───────────────────────────────────────────────────

struct NeedyPlugin {
    meta: PluginMetadata,
    caps: CapabilitySet,
}
impl Plugin for NeedyPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn required_capabilities(&self) -> CapabilitySet {
        self.caps.clone()
    }
}

fn needy(api: Version, caps: CapabilitySet) -> Arc<dyn Plugin> {
    Arc::new(NeedyPlugin {
        meta: PluginMetadata::new("test.needy", "Needy", Version::new(1, 0, 0), PluginKind::VideoEffect, api),
        caps,
    })
}

#[test]
fn rejects_incompatible_api_version() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    let res = mgr.register(needy(Version::new(2, 0, 0), CapabilitySet::new()));
    assert!(matches!(res, Err(PluginError::Incompatible { .. })));
    assert_eq!(mgr.count(), 0);
}

#[test]
fn rejects_missing_capabilities() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new()); // host provides nothing
    let caps: CapabilitySet = [Capability::Gpu].into_iter().collect();
    let res = mgr.register(needy(PLUGIN_API_VERSION, caps));
    assert!(matches!(res, Err(PluginError::MissingCapabilities { .. })));
}

#[test]
fn accepts_when_host_provides_capabilities() {
    let host: CapabilitySet = [Capability::Gpu, Capability::Simd].into_iter().collect();
    let mgr = PluginManager::new(PLUGIN_API_VERSION, host);
    let caps: CapabilitySet = [Capability::Gpu].into_iter().collect();
    assert!(mgr.register(needy(PLUGIN_API_VERSION, caps)).is_ok());
    assert_eq!(mgr.count(), 1);
}

#[test]
fn duplicate_registration_errors() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    mgr.register(needy(PLUGIN_API_VERSION, CapabilitySet::new())).unwrap();
    assert!(matches!(mgr.register(needy(PLUGIN_API_VERSION, CapabilitySet::new())), Err(PluginError::Duplicate(_))));
}

// ── lifecycle ───────────────────────────────────────────────────────────────

#[test]
fn lifecycle_transitions() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    mgr.register(needy(PLUGIN_API_VERSION, CapabilitySet::new())).unwrap();
    assert_eq!(mgr.lifecycle("test.needy"), Some(Lifecycle::Registered));
    assert!(!mgr.is_enabled("test.needy"));

    mgr.enable("test.needy").unwrap();
    assert_eq!(mgr.lifecycle("test.needy"), Some(Lifecycle::Enabled));
    assert!(mgr.is_enabled("test.needy"));
    assert!(mgr.enabled("test.needy").is_some());

    mgr.disable("test.needy").unwrap();
    assert_eq!(mgr.lifecycle("test.needy"), Some(Lifecycle::Disabled));
    assert!(mgr.enabled("test.needy").is_none());
}

#[test]
fn enable_unknown_errors() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    assert!(matches!(mgr.enable("nope"), Err(PluginError::NotFound(_))));
}

// ── events ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Capture {
    events: Mutex<Vec<Event>>,
}
impl EventSink for Capture {
    fn publish(&self, e: Event) {
        self.events.lock().unwrap().push(e);
    }
}

#[test]
fn manager_emits_lifecycle_events() {
    let sink = Arc::new(Capture::default());
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new()).with_event_sink(sink.clone());
    mgr.register(needy(PLUGIN_API_VERSION, CapabilitySet::new())).unwrap();
    mgr.enable("test.needy").unwrap();
    mgr.disable("test.needy").unwrap();
    mgr.unregister("test.needy").unwrap();

    let events = sink.events.lock().unwrap().clone();
    assert_eq!(
        events,
        vec![
            Event::PluginLoaded { id: "test.needy".into() },
            Event::PluginEnabled { id: "test.needy".into() },
            Event::PluginDisabled { id: "test.needy".into() },
            Event::PluginUnloaded { id: "test.needy".into() },
        ]
    );
}

#[test]
fn bus_delivers_plugin_events_to_listeners() {
    #[derive(Default)]
    struct Counter(Mutex<usize>);
    impl EventListener for Counter {
        fn on_event(&self, _e: &Event) {
            *self.0.lock().unwrap() += 1;
        }
    }
    let bus = Arc::new(InProcessBus::new());
    let listener = Arc::new(Counter::default());
    bus.subscribe(listener.clone());
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new()).with_event_sink(bus);
    register_builtins(&mgr).unwrap(); // 5 registers + 5 enables = 10 events
    assert_eq!(*listener.0.lock().unwrap(), 10);
}

// ── built-in adapters run real engine code ──────────────────────────────────

#[test]
fn builtins_register_and_are_discoverable() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();
    assert_eq!(mgr.count(), 5);
    // 3 video effects (color, keyer, mask), 1 audio, 1 transition.
    assert_eq!(mgr.ids_by_kind(PluginKind::VideoEffect).len(), 3);
    assert_eq!(mgr.ids_by_kind(PluginKind::AudioEffect), vec![AudioFxPlugin::ID.to_string()]);
    assert_eq!(mgr.ids_by_kind(PluginKind::Transition), vec![TransitionBuiltin::ID.to_string()]);
    assert!(mgr.is_enabled(ColorGradePlugin::ID));
}

#[test]
fn keyer_plugin_removes_green() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();
    let plugin = mgr.enabled(KeyerPlugin::ID).unwrap();
    let ve = plugin.as_video_effect().unwrap();
    let params = serde_json::json!({ "key": [0, 255, 0], "tolerance": 0.2, "softness": 0.1, "despill": false });
    let frame = Frame::new(1, 1, PixelFormat::Rgba8, vec![0, 255, 0, 255]);
    let out = ve.apply_video(frame, &params).unwrap();
    assert_eq!(out.data[3], 0, "green keyed to transparent");
}

#[test]
fn mask_plugin_applies_coverage() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();
    let plugin = mgr.enabled(MaskPlugin::ID).unwrap();
    let ve = plugin.as_video_effect().unwrap();
    // A rectangle mask covering only the right half → left pixel alpha 0.
    let params = serde_json::json!({ "shape": "rectangle", "x": 0.5, "y": 0.0, "w": 0.5, "h": 1.0 });
    let frame = Frame::new(4, 1, PixelFormat::Rgba8, vec![255u8; 16]);
    let out = ve.apply_video(frame, &params).unwrap();
    assert_eq!(out.data[3], 0, "left pixel outside mask");
}

#[test]
fn transition_plugin_builds_animation() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();
    let plugin = mgr.enabled(TransitionBuiltin::ID).unwrap();
    let tr = plugin.as_transition().unwrap();
    let anim = tr.build_transition(&serde_json::json!({ "type": "fade_in", "secs": 1.0 }), 5.0).unwrap();
    assert!(anim.opacity.is_some(), "fade-in animates opacity");
}

#[test]
fn color_grade_plugin_applies_real_grade() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();

    let plugin = mgr.enabled(ColorGradePlugin::ID).unwrap();
    let ve = plugin.as_video_effect().unwrap();
    // A CDL with slope 2.0 doubles a mid-grey pixel: 64 → 128.
    let params = serde_json::json!({ "cdl": { "slope": [2.0, 2.0, 2.0] } });
    let frame = Frame::new(1, 1, PixelFormat::Rgba8, vec![64, 64, 64, 255]);
    let out = ve.apply_video(frame, &params).unwrap();
    assert_eq!(out.data[0], 128);
}

#[test]
fn audio_fx_plugin_applies_real_gain() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();

    let plugin = mgr.enabled(AudioFxPlugin::ID).unwrap();
    let ae = plugin.as_audio_effect().unwrap();
    // -6 dB gain ≈ ×0.501.
    let params = serde_json::json!({ "type": "gain", "db": -6.0 });
    let frame = AudioFrame::new(48_000, 1, vec![1.0; 100]);
    let out = ae.apply_audio(frame, &params).unwrap();
    assert!((out.samples[0] - 0.501).abs() < 2e-3, "got {}", out.samples[0]);
}

#[test]
fn wrong_kind_projection_is_none() {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();
    // The color-grade plugin is not an audio effect.
    let plugin = mgr.get(ColorGradePlugin::ID).unwrap();
    assert!(plugin.as_audio_effect().is_none());
}
