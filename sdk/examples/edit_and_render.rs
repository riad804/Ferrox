//! Minimal end-to-end example: build a project, subscribe to events, render a
//! frame, undo, and persist. Run with `cargo run -p ferrox-sdk --example edit_and_render`.

use std::sync::Arc;

use ferrox_sdk::{
    Clip, ClipSource, Editor, Event, EventListener, InProcessBus, Transform,
};

struct Logger;
impl EventListener for Logger {
    fn on_event(&self, event: &Event) {
        println!("event: {event:?}");
    }
}

fn main() -> ferrox_sdk::Result<()> {
    // Wire an event bus (Dependency Injection via the builder).
    let bus = Arc::new(InProcessBus::new());
    bus.subscribe(Arc::new(Logger));
    let editor = Editor::builder(640, 360, 30.0).with_event_sink(bus).build();

    let track = editor.add_track()?;
    editor.add_clip(
        track,
        Clip::new(ClipSource::Solid { width: 640, height: 360, r: 30, g: 90, b: 160, a: 255 }, 0.0, 5.0, Transform::default()),
    )?;

    let frame = editor.render_frame(1.0, 0, 0)?;
    println!("rendered {} RGBA bytes", frame.len());

    editor.undo()?;
    println!("after undo, tracks still present: {}", editor.with_project(|p| p.tracks.len())?);

    let json = editor.save_json()?;
    println!("project JSON is {} bytes", json.len());
    Ok(())
}
