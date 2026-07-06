//! Plugin system example: enumerate the built-ins, run a video-effect plugin on
//! a frame, and observe lifecycle events. Run with
//! `cargo run -p ferrox-core --example plugins`.

use std::sync::Arc;

use ferrox_core::plugin::{register_builtins, CapabilitySet, PluginKind, PluginManager, PLUGIN_API_VERSION};
use ferrox_core::{Event, EventListener, Frame, InProcessBus, PixelFormat};

struct Logger;
impl EventListener for Logger {
    fn on_event(&self, e: &Event) {
        println!("event: {e:?}");
    }
}

fn main() {
    // A bus so we can watch plugin lifecycle events.
    let bus = Arc::new(InProcessBus::new());
    let logger = Arc::new(Logger); // keep a strong ref (listeners are held weakly)
    bus.subscribe(logger.clone());

    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new()).with_event_sink(bus);
    register_builtins(&mgr).unwrap();

    println!("video effects: {:?}", mgr.ids_by_kind(PluginKind::VideoEffect));

    // Run the color-grade plugin: double a mid-grey pixel via an ASC-CDL.
    let plugin = mgr.enabled("ferrox.builtin.color_grade").unwrap();
    let ve = plugin.as_video_effect().unwrap();
    let params = serde_json::json!({ "cdl": { "slope": [2.0, 2.0, 2.0] } });
    let frame = Frame::new(1, 1, PixelFormat::Rgba8, vec![64, 64, 64, 255]);
    let out = ve.apply_video(frame, &params).unwrap();
    println!("64 graded → {}", out.data[0]);
}
