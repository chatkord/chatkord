//! Opening a pick on Windows: the monitor or window our picker gave, captured into the encoder's device, and the sound
//! the share hears, which for a window is its program's.

use super::send::Opening;
use crate::screen::{self, Frames};
use crate::send::share_sound::{self, SoundAsked};
use choices::ShareQuality;
use std::sync::Arc;

/// Opens the capture of the picker's `selection` on the blocking pool, deciding what the share hears beside it.
fn open(
    selection: &::share::Selection,
    quality: ShareQuality,
    sound: SoundAsked,
    encoders: Arc<media::Survey>,
) -> Result<Opening, words::Failure> {
    let (item, picked, refresh) = (selection.item.clone(), selection.id, selection.refresh);
    let cursor = selection.cursor == ::share::Cursor::Embedded;
    let (kind, owner, switches) = (selection.source.kind, selection.owner.clone(), quality.sound);
    Ok(tokio::task::spawn_blocking(move || {
        let started = screen::begin(&item, Some(picked), refresh, cursor, &quality, &encoders)?;
        Ok((started, share_sound::decide(sound, kind, switches, || owner, ::sound::share::holds_us)))
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
