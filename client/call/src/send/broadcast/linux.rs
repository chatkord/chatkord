//! Opening a pick on Linux: the portal's PipeWire node, and the sound the share hears, which for a window KWin names.

use super::send::Opening;
use crate::screen::{self, Frames};
use crate::send::share_sound::{self, SoundAsked};
use choices::ShareQuality;
use std::sync::Arc;

/// Opens the capture of the portal's `selection` on the blocking pool, deciding what the share hears beside it.
pub(super) fn open(
    selection: &::share::Selection,
    quality: ShareQuality,
    sound: SoundAsked,
    encoders: Arc<media::Survey>,
) -> Result<Opening, words::Failure> {
    let (node, kind, switches) = (selection.source.node_id, selection.source.kind, quality.sound);
    let remote = selection.remote.try_clone().map_err(|_| crate::failure::capture(capture::CaptureError::Connect))?;
    Ok(tokio::task::spawn_blocking(move || {
        let started = screen::begin(remote, node, &quality, &encoders)?;
        // Asks KWin, for up to a second, only for a window whose sound is on.
        let owner = || started.source_name().and_then(|name| ::share::owner(&name));
        let decided = share_sound::decide(sound, kind, switches, owner, ::sound::share::holds_us);
        Ok((started, decided))
    }))
}

/// Opens a test's own pictures, whose sound is taken as a whole screen's.
pub(super) fn open_frames(
    frames: Box<dyn Frames>,
    quality: ShareQuality,
    sound: SoundAsked,
    encoders: Arc<media::Survey>,
) -> Opening {
    let switches = quality.sound;
    tokio::task::spawn_blocking(move || {
        let started = screen::begin_with(frames, &quality, &encoders)?;
        Ok((started, share_sound::decide(sound, None, switches, || None, ::sound::share::holds_us)))
    })
}
