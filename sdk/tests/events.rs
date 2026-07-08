//! Phase 0: the Editor publishes domain events through an injected sink, and the
//! in-process bus fans them out to subscribers.

use std::sync::{Arc, Mutex};

use ferrox_sdk::{
    Clip, ClipSource, Editor, Event, EventListener, EventSink, InProcessBus, Transform,
};

#[derive(Default)]
struct CapturingSink {
    events: Mutex<Vec<Event>>,
}
impl EventSink for CapturingSink {
    fn publish(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

#[derive(Default)]
struct CapturingListener {
    count: Mutex<usize>,
}
impl EventListener for CapturingListener {
    fn on_event(&self, _event: &Event) {
        *self.count.lock().unwrap() += 1;
    }
}

fn clip() -> Clip {
    Clip::new(ClipSource::Solid { width: 8, height: 8, r: 1, g: 2, b: 3, a: 255 }, 0.0, 1.0, Transform::default())
}

#[test]
fn editor_publishes_events_through_injected_sink() {
    let sink = Arc::new(CapturingSink::default());
    let editor = Editor::builder(64, 64, 30.0).with_event_sink(sink.clone()).build();

    let t = editor.add_track().unwrap(); // ProjectChanged
    editor.add_clip(t, clip()).unwrap(); // ProjectChanged
    editor.undo().unwrap(); // Undo
    editor.redo().unwrap(); // Redo

    let events = sink.events.lock().unwrap().clone();
    assert_eq!(
        events,
        vec![Event::ProjectChanged, Event::ProjectChanged, Event::Undo, Event::Redo]
    );
}

#[test]
fn default_editor_has_no_sink_and_stays_silent() {
    // The no-op default must not panic and must produce identical behaviour.
    let editor = Editor::new(16, 16, 30.0);
    let t = editor.add_track().unwrap();
    editor.add_clip(t, clip()).unwrap();
    assert_eq!(editor.with_project(|p| p.tracks[0].clips.len()).unwrap(), 1);
}

#[test]
fn in_process_bus_fans_out_to_subscribers() {
    let bus = Arc::new(InProcessBus::new());
    let listener = Arc::new(CapturingListener::default());
    bus.subscribe(listener.clone());
    assert_eq!(bus.listener_count(), 1);

    let editor = Editor::builder(8, 8, 30.0).with_event_sink(bus.clone()).build();
    editor.add_track().unwrap();
    editor.add_clip(0, clip()).unwrap();

    assert_eq!(*listener.count.lock().unwrap(), 2, "two ProjectChanged events delivered");
}

#[test]
fn dropped_listener_unsubscribes() {
    let bus = InProcessBus::new();
    {
        let listener = Arc::new(CapturingListener::default());
        bus.subscribe(listener.clone());
        assert_eq!(bus.listener_count(), 1);
    } // listener dropped here
    assert_eq!(bus.listener_count(), 0, "weakly-held listener pruned after drop");
}

#[test]
fn editor_exposes_plugin_manager_with_builtins() {
    use ferrox_sdk::PluginKind;
    let editor = Editor::new(16, 16, 30.0);
    let plugins = editor.plugins();
    assert_eq!(plugins.count(), 5, "built-ins registered on the editor");
    assert_eq!(plugins.ids_by_kind(PluginKind::VideoEffect).len(), 3);
    assert!(plugins.is_enabled("ferrox.builtin.color_grade"));
}

#[test]
fn editor_plugin_events_reach_the_editor_bus() {
    // Built-ins register silently at construction (no startup event spam), but a
    // runtime plugin toggle publishes on the editor's sink.
    let sink = Arc::new(CapturingSink::default());
    let editor = Editor::builder(16, 16, 30.0).with_event_sink(sink.clone()).build();
    assert!(
        sink.events.lock().unwrap().is_empty(),
        "no events emitted during construction"
    );
    editor.plugins().disable("ferrox.builtin.mask").unwrap();
    assert_eq!(sink.events.lock().unwrap().len(), 1);
    assert!(matches!(sink.events.lock().unwrap()[0], Event::PluginDisabled { .. }));
}
