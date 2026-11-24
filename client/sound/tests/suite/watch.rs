//! The system's sound devices heard through the one listener two holders share. The device is a null sink made and
//! removed through `pactl`, which PipeWire's pulse server answers.

use std::process::Command;
use std::time::Duration;
use testserver::Undo;

use sound::Sound;

use tier::skipped;

/// A null sink, gone when this is dropped or the test crashes, so nothing is left on the machine.
struct NullSink {
    _unload: Undo,
}

impl NullSink {
    fn load() -> Option<Self> {
        let made = Command::new("pactl")
            .args(["load-module", "module-null-sink", "sink_name=chatkord_sound_test"])
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

mod lab {
    use super::*;

    #[tokio::test]
    async fn two_holders_both_hear_a_device_arrive_through_one_listener() {
        let (Ok(mut first), Ok(mut second)) = (Sound::shared().watch().await, Sound::shared().watch().await) else {
            return skipped("no PipeWire to listen to");
        };
        // The devices already there are told once; take that first.
        let _ = tokio::time::timeout(Duration::from_secs(3), first.changed()).await;
        let _ = tokio::time::timeout(Duration::from_secs(3), second.changed()).await;
        let Some(_sink) = NullSink::load() else {
            return skipped("no pactl, or no pulse server to ask");
        };
        let told = async { tokio::join!(first.changed(), second.changed()) };
        assert!(tokio::time::timeout(Duration::from_secs(5), told).await.is_ok(), "both holders hear the new device");
    }

    #[tokio::test]
    async fn a_forwarded_change_reaches_the_channel() {
        let Ok(changes) = Sound::shared().watch().await else { return skipped("no PipeWire to listen to") };
        let (tell, mut told) = tokio::sync::mpsc::channel(4);
        changes.forward(tell, || "changed");
        // The devices already there are told once; take that first.
        let _ = tokio::time::timeout(Duration::from_secs(3), told.recv()).await;
        let Some(_sink) = NullSink::load() else {
            return skipped("no pactl, or no pulse server to ask");
        };
        let heard = tokio::time::timeout(Duration::from_secs(5), told.recv()).await;
        assert_eq!(heard, Ok(Some("changed")), "a new device is forwarded");
    }

    #[tokio::test]
    async fn forwarding_ends_with_the_channel_and_lets_the_listener_go() {
        let Ok(mut first) = Sound::shared().watch().await else { return skipped("no PipeWire to listen to") };
        // The devices already there are told once, to the holder that started the listening; a second holder made after
        // that hears nothing until a device really comes or goes, so only the channel closing can end its forwarding.
        let _ = tokio::time::timeout(Duration::from_secs(3), first.changed()).await;
        let Ok(second) = Sound::shared().watch().await else { return skipped("no PipeWire to listen to") };
        let (tell, told) = tokio::sync::mpsc::channel::<&str>(4);
        second.forward(tell.clone(), || "changed");
        drop(told);
        for _ in 0..20 {
            if tell.strong_count() == 1 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the forwarding task still holds its sender");
    }
}
