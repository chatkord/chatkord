//! How loud this device plays each member's sounds, a person's own choice that no one else hears.

use std::collections::BTreeMap;
use terms::MemberId;

/// A volume from silent to as sent, a whole percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Volume(u8);

impl Volume {
    /// As sent: every sound starts here.
    pub const FULL: Volume = Volume(100);

    /// Not heard: a sound muted for this device alone.
    pub const SILENT: Volume = Volume(0);

    /// The volume of `percent`; none above [`Volume::FULL`].
    pub fn new(percent: u16) -> Option<Volume> {
        u8::try_from(percent).ok().filter(|percent| *percent <= Volume::FULL.0).map(Volume)
    }

    /// Whether it is not heard: Mute for me's undo is offered.
    pub fn silent(self) -> bool {
        self == Volume::SILENT
    }

    /// The whole percent.
    pub fn percent(self) -> u8 {
        self.0
    }

    /// The share of the sound as sent, from 0.0 to 1.0, which the engine mixes it at.
    pub fn fraction(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

impl Default for Volume {
    fn default() -> Volume {
        Volume::FULL
    }
}

/// Which of a member's sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Audio {
    /// Their microphone.
    Voice,
    /// What they share with their screen.
    Share,
}

/// One member's volumes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loudness {
    /// Their microphone's.
    pub voice: Volume,
    /// Their share's sound's.
    pub share: Volume,
}

impl Loudness {
    fn of(&mut self, audio: Audio) -> &mut Volume {
        match audio {
            Audio::Voice => &mut self.voice,
            Audio::Share => &mut self.share,
        }
    }
}

/// The volumes of one server's members that differ from [`Volume::FULL`], by the id the server gives them, and for
/// each sound muted here this run, the volume it had.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Volumes {
    kept: BTreeMap<MemberId, Loudness>,
    before_mute: BTreeMap<(MemberId, Audio), Volume>,
}

impl Volumes {
    /// The volume `user_id`'s `audio` plays at.
    pub fn of(&self, user_id: &MemberId, audio: Audio) -> Volume {
        let loudness = self.kept.get(user_id).copied().unwrap_or_default();
        match audio {
            Audio::Voice => loudness.voice,
            Audio::Share => loudness.share,
        }
    }

    /// Plays `user_id`'s `audio` at `volume`, forgetting what it had before a mute; a member back at full on both is no
    /// longer kept.
    pub fn set(&mut self, user_id: &MemberId, audio: Audio, volume: Volume) {
        self.before_mute.remove(&(user_id.clone(), audio));
        let loudness = self.kept.entry(user_id.clone()).or_default();
        *loudness.of(audio) = volume;
        if *loudness == Loudness::default() {
            self.kept.remove(user_id);
        }
    }

    /// Mutes `user_id`'s `audio` here, or unmutes it to the volume it had before, else to full; the volume it now plays
    /// at. What it had is remembered for this run alone, as Sharkord does.
    pub fn toggle_mute(&mut self, user_id: &MemberId, audio: Audio) -> Volume {
        let key = (user_id.clone(), audio);
        let now = self.of(user_id, audio);
        if now == Volume::SILENT {
            let back = self.before_mute.remove(&key).unwrap_or(Volume::FULL);
            self.set(user_id, audio, back);
            back
        } else {
            self.set(user_id, audio, Volume::SILENT);
            self.before_mute.insert(key, now);
            Volume::SILENT
        }
    }

    /// Each member kept and their volumes.
    pub fn iter(&self) -> impl Iterator<Item = (&MemberId, Loudness)> {
        self.kept.iter().map(|(user_id, loudness)| (user_id, *loudness))
    }
}

impl FromIterator<(MemberId, Loudness)> for Volumes {
    fn from_iter<I: IntoIterator<Item = (MemberId, Loudness)>>(members: I) -> Volumes {
        let mut volumes = Volumes::default();
        for (user_id, loudness) in members {
            volumes.set(&user_id, Audio::Voice, loudness.voice);
            volumes.set(&user_id, Audio::Share, loudness.share);
        }
        volumes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    fn ada() -> MemberId {
        MemberId::parse(ADA).expect("a member id")
    }

    #[test]
    fn a_volume_is_a_whole_percent_up_to_full() {
        assert_eq!(Volume::new(100), Some(Volume::FULL));
        assert_eq!(Volume::new(101), None);
        assert_eq!(Volume::new(356), None, "nor one that wraps to a small one");
        assert_eq!(Volume::new(0).map(Volume::fraction), Some(0.0));
        assert_eq!(Volume::new(40).map(Volume::fraction), Some(0.4));
    }

    #[test]
    fn a_members_two_sounds_are_kept_apart_and_dropped_once_both_are_full_again() {
        let ada = ada();
        let mut volumes = Volumes::default();
        let quiet = Volume::new(40).unwrap();
        volumes.set(&ada, Audio::Voice, quiet);
        assert_eq!((volumes.of(&ada, Audio::Voice), volumes.of(&ada, Audio::Share)), (quiet, Volume::FULL));
        volumes.set(&ada, Audio::Share, Volume::new(0).unwrap());
        volumes.set(&ada, Audio::Voice, Volume::FULL);
        assert_eq!(volumes.iter().count(), 1, "the share's sound is still kept");
        volumes.set(&ada, Audio::Share, Volume::FULL);
        assert_eq!(volumes, Volumes::default(), "nothing is kept for a member heard as sent");
    }

    #[test]
    fn a_mute_silences_one_sound_and_its_undo_brings_back_the_volume_before_until_it_is_moved() {
        let ada = ada();
        let mut volumes = Volumes::default();
        let quiet = Volume::new(40).unwrap();
        volumes.set(&ada, Audio::Voice, quiet);
        assert_eq!(volumes.toggle_mute(&ada, Audio::Voice), Volume::SILENT);
        assert_eq!(volumes.of(&ada, Audio::Share), Volume::FULL, "the other sound plays on");
        assert_eq!(volumes.toggle_mute(&ada, Audio::Voice), quiet, "unmuted, it is as loud as it was");
        assert_eq!(volumes.toggle_mute(&ada, Audio::Voice), Volume::SILENT);
        volumes.set(&ada, Audio::Voice, Volume::SILENT);
        assert_eq!(volumes.toggle_mute(&ada, Audio::Voice), Volume::FULL, "moved since, the volume before is gone");
        volumes.set(&ada, Audio::Share, Volume::SILENT);
        assert_eq!(volumes.toggle_mute(&ada, Audio::Share), Volume::FULL, "silent with nothing before it: full");
        let kept: Volumes = volumes.iter().map(|(user_id, loudness)| (user_id.clone(), loudness)).collect();
        assert_eq!(kept.iter().collect::<Vec<_>>(), volumes.iter().collect::<Vec<_>>(), "only the volumes are kept");
    }
}
