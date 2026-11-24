//! The registry says when a sound device appears and goes, in a binary that carries libwebrtc's PipeWire stubs. The
//! device is a null sink made and removed through `pactl`, which PipeWire's pulse server answers.

use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use testserver::Undo;

use webrtc as _;

use tier::skipped;

/// A null sink, gone when this is dropped or the test crashes, so nothing is left on the machine.
struct NullSink {
    _unload: Undo,
}

impl NullSink {
    fn load() -> Option<Self> {
        let made = Command::new("pactl")
            .args(["load-module", "module-null-sink", "sink_name=chatkord_registry_test"])
            .output()
            .ok()?;
        let module = String::from_utf8_lossy(&made.stdout).trim().to_string();
        made.status
            .success()
            .then(|| Undo::new("pactl", &["unload-module", &module]))
            .flatten()
            .map(|_unload| Self { _unload })
    }
}

/// Reads changes until none arrives for a while: the devices already there are announced when the listener starts.
fn settled(changes: &Receiver<()>) {
    while changes.recv_timeout(Duration::from_millis(500)).is_ok() {}
}

mod lab {
    use super::*;

    #[test]
    fn a_sound_device_made_and_removed_is_heard() {
        let (tell, changes) = mpsc::channel();
        let registry = match pw::Registry::sound(move || {
            let _ = tell.send(());
        }) {
            Ok(registry) => registry,
            Err(why) => return skipped(why.to_string()),
        };
        settled(&changes);
        let Some(sink) = NullSink::load() else {
            return skipped("no pactl, or no pulse server to ask");
        };
        assert!(changes.recv_timeout(Duration::from_secs(3)).is_ok(), "a device that appears is heard");
        settled(&changes);
        drop(sink);
        assert!(changes.recv_timeout(Duration::from_secs(3)).is_ok(), "a device that goes is heard");
        drop(registry);
    }

    #[test]
    fn a_listener_that_is_dropped_hears_nothing_more() {
        let (tell, changes) = mpsc::channel();
        let registry = match pw::Registry::sound(move || {
            let _ = tell.send(());
        }) {
            Ok(registry) => registry,
            Err(why) => return skipped(why.to_string()),
        };
        settled(&changes);
        drop(registry);
        let Some(_sink) = NullSink::load() else {
            return skipped("no pactl, or no pulse server to ask");
        };
        assert!(changes.recv_timeout(Duration::from_secs(1)).is_err(), "nothing is told after the listener is gone");
    }
}
