//! Sounds on Linux: a playback stream of PipeWire's for a sound (`pw::Play`), drained before it is taken down.

use super::{PlayError, RATE};
use std::cell::OnceCell;
use std::sync::Arc;

impl From<pw::Error> for PlayError {
    fn from(why: pw::Error) -> Self {
        match why {
            pw::Error::Unavailable(_) => Self::Unavailable,
            pw::Error::Refused(_) | pw::Error::Pod => Self::Refused,
        }
    }
}

/// Where chimes are played from: the sound server's playback connection, opened by the first sound. A sound
/// playing borrows it, so it outlives the [`Chimes`] that play from it.
#[derive(Default)]
pub struct Stage {
    play: OnceCell<pw::Play>,
}

/// Plays one sound at a time, the newest in place of one still playing, from the thread that holds them. Dropping
/// them ends the sound playing.
pub struct Chimes<'stage> {
    stage: &'stage Stage,
    playing: Option<pw::Playing<'stage>>,
}

impl<'stage> Chimes<'stage> {
    /// Chimes that play from `stage`, nothing connected until the first.
    pub fn new(stage: &'stage Stage) -> Self {
        Self { stage, playing: None }
    }

    /// Connects to the sound server now, so the first sound plays at once.
    pub fn open(&mut self) -> Result<(), PlayError> {
        self.connected().map(|_| ())
    }

    fn connected(&self) -> Result<&'stage pw::Play, PlayError> {
        let stage: &'stage Stage = self.stage;
        match stage.play.get() {
            Some(play) => Ok(play),
            None => {
                let opened = pw::Play::open()?;
                Ok(stage.play.get_or_init(|| opened))
            }
        }
    }

    /// Plays `samples`, one channel at [`RATE`] from -1 to 1, on the speaker with the sound server's id `speaker`
    /// (`node.name`), or the default; one no longer there plays on the default. Returns once the sound has started,
    /// a few milliseconds.
    pub fn play(&mut self, samples: Arc<[f32]>, speaker: Option<&str>) -> Result<(), PlayError> {
        // The sound it replaces is cut before the next starts, so two never play at once.
        self.playing = None;
        let play = self.connected()?;
        self.playing = Some(play.start(samples, RATE, speaker)?);
        Ok(())
    }

    /// Whether a sound is playing, so its stream is looked at soon for having drained.
    pub fn playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Takes down a sound that has drained.
    pub fn settle(&mut self) {
        if self.playing.as_ref().is_some_and(pw::Playing::drained) {
            self.playing = None;
        }
    }
}
