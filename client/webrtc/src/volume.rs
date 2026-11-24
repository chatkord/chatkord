//! How loud each received sound is mixed: set from the call at once, while its streams are taken on a lane of their
//! own, and given to a track again whenever the stream is taken anew. Deafened, every track is mixed silent while
//! the speaker plays on, so the window's own sounds are still heard.

use proto::chatkord_v1::TrackKind;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// A member's sound, by who sends it and which of their tracks it is.
type Sound = (String, TrackKind);

/// Whether the call's sound reaches the speaker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mix {
    /// Every sound at its own gain.
    #[default]
    Heard,
    /// Every sound silent; the gains asked meanwhile are kept for when it is heard again.
    Deafened,
}

#[derive(Default)]
struct Mixing {
    /// The gain asked for each sound, kept for as long as the call, so a stream taken again starts at it.
    gains: HashMap<Sound, f64>,
    /// The sounds taken now, and the gain each is mixed at as our chain reads it (an `f32`'s bits).
    tracks: HashMap<Sound, Arc<AtomicU32>>,
    mix: Mix,
}

impl Mixing {
    /// The gain `sound` is mixed at now.
    fn gain(&self, sound: &Sound) -> f64 {
        match self.mix {
            Mix::Heard => self.gains.get(sound).copied().unwrap_or(1.0),
            Mix::Deafened => 0.0,
        }
    }

    /// Mixes `sound`'s track at its gain now.
    fn apply(&self, sound: &Sound) {
        if let Some(ours) = self.tracks.get(sound) {
            let gain = self.gain(sound);
            #[expect(clippy::cast_possible_truncation, reason = "a gain from 0 to 1")]
            ours.store((gain as f32).to_bits(), Ordering::Relaxed);
        }
    }
}

/// The gain of each received sound, shared by the call that sets it and the connection that takes the tracks.
#[derive(Clone, Default)]
pub(crate) struct Volumes(Arc<Mutex<Mixing>>);

impl Volumes {
    fn mixing(&self) -> MutexGuard<'_, Mixing> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Mixes `user_id`'s sound of `kind` at `gain`, from 0.0 (silent) to 1.0 (as sent), now and whenever it is taken.
    pub(crate) fn set(&self, user_id: &str, kind: TrackKind, gain: f64) {
        let mut mixing = self.mixing();
        let sound = (user_id.to_string(), kind);
        mixing.gains.insert(sound.clone(), gain);
        mixing.apply(&sound);
    }

    /// Mixes every sound silent, or each at its own gain again.
    pub(crate) fn hear(&self, mix: Mix) {
        let mut mixing = self.mixing();
        mixing.mix = mix;
        for sound in mixing.tracks.keys() {
            mixing.apply(sound);
        }
    }

    /// A sound just taken, mixed at its gain until [`Volumes::gone`]; the gain as our chain reads it.
    pub(crate) fn taken(&self, user_id: &str, kind: TrackKind) -> Arc<AtomicU32> {
        let mut mixing = self.mixing();
        let sound = (user_id.to_string(), kind);
        let ours = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        mixing.tracks.insert(sound.clone(), Arc::clone(&ours));
        mixing.apply(&sound);
        ours
    }

    /// The sound's track is no longer taken; its gain stays for the next.
    pub(crate) fn gone(&self, user_id: &str, kind: TrackKind) {
        self.mixing().tracks.remove(&(user_id.to_string(), kind));
    }

    /// Lets go of every sound taken.
    pub(crate) fn release(&self) {
        self.mixing().tracks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    fn read(gain: &AtomicU32) -> f32 {
        f32::from_bits(gain.load(Ordering::Relaxed))
    }

    #[test]
    fn deafening_mixes_every_taken_sound_silent_and_gives_each_its_gain_back() {
        let volumes = Volumes::default();
        let gain = volumes.taken(ADA, TrackKind::Microphone);
        assert!((read(&gain) - 1.0).abs() < f32::EPSILON, "taken as sent");
        volumes.hear(Mix::Deafened);
        assert!(read(&gain) == 0.0, "deafened, it is mixed silent");
        volumes.set(ADA, TrackKind::Microphone, 0.5);
        assert!(read(&gain) == 0.0, "a gain asked while deafened stays silent");
        volumes.gone(ADA, TrackKind::Microphone);
        let again = volumes.taken(ADA, TrackKind::Microphone);
        assert!(read(&again) == 0.0, "a sound taken while deafened starts silent");
        volumes.hear(Mix::Heard);
        assert!((read(&again) - 0.5).abs() < f32::EPSILON, "heard again, it is mixed at the gain last asked");
    }

    #[test]
    fn one_sounds_gain_leaves_the_others() {
        let volumes = Volumes::default();
        let (voice, sound) = (volumes.taken(ADA, TrackKind::Microphone), volumes.taken(ADA, TrackKind::ScreenAudio));
        volumes.set(ADA, TrackKind::Microphone, 0.25);
        assert!((read(&voice) - 0.25).abs() < f32::EPSILON);
        assert!((read(&sound) - 1.0).abs() < f32::EPSILON, "the share's sound as it was");
    }
}
