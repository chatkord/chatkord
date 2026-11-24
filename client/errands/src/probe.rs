//! The probes behind the settings: the voice's (a microphone's level, a tone, a test) and the cameras', and the
//! window's own sounds they share with a chime.

use crate::mailbox::Mailbox;
use screens::{ProbeHeard, WindowMessage};
use std::sync::Arc;
use tokio::sync::mpsc;

/// How many commands the settings page's probe may be behind by: a few a second at most, from a person's hands.
pub(crate) const PROBE_COMMANDS: usize = 32;

/// Plays `chime` on the speaker this device keeps.
pub(crate) async fn chime(platform: &platform::Platform, chime: screens::Chime) -> Result<(), call::PlayError> {
    let speaker = platform::voice::choice(platform).await.speaker.map(|speaker| speaker.id);
    sound::Sound::shared().chime(chime, speaker).await
}

/// The settings page's probe, from the engine to its end: told to close, or its commands closing. One that could
/// not start, or that ended without being asked to (the voice chain under it stopped), says why on the page.
pub(crate) async fn run_probe(
    engines: Arc<call::Engines>,
    open: call::probe::Open,
    commands: mpsc::Sender<call::probe::Command>,
    queued: mpsc::Receiver<call::probe::Command>,
    mailbox: Mailbox,
) {
    let engine = match engines.engine().await {
        Ok(engine) => engine,
        Err(refused) => {
            mailbox.tell(WindowMessage::Probed(ProbeHeard::Failed(call::failure::voice(refused)))).await;
            return;
        }
    };
    if let Ok(changes) = sound::Sound::shared().watch().await {
        changes.forward(commands, || call::probe::Command::DevicesChanged);
    }
    // A reading missed because the window is behind is the next one's to make good.
    let told = |report: call::probe::Report| {
        let heard = match report {
            call::probe::Report::Sound(sound) => ProbeHeard::Sound(sound),
            call::probe::Report::Loudness(db) => ProbeHeard::Loudness(db),
            call::probe::Report::Notice(subject, said) => ProbeHeard::Notice(subject, said),
        };
        mailbox.try_tell(WindowMessage::Probed(heard));
    };
    if let Err(ended) = call::probe::run(&engine, open, queued, told).await {
        mailbox.tell(WindowMessage::Probed(ProbeHeard::Failed(ended.failure()))).await;
    }
}

/// The settings page's cameras' probe, until told to close or its commands closing, the preview's pictures put in one's
/// own camera slot.
pub(crate) async fn run_camera_probe(
    own: call::Slot,
    queued: mpsc::Receiver<call::camera_probe::Command>,
    mailbox: Mailbox,
) {
    call::camera_probe::run(own, queued, |report| {
        let heard = match report {
            call::camera_probe::Report::Cameras(listed) => ProbeHeard::Cameras(listed),
            call::camera_probe::Report::Test(opened) => ProbeHeard::CameraTest(opened),
        };
        mailbox.try_tell(WindowMessage::Probed(heard));
    })
    .await;
}
