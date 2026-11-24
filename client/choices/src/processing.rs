//! How this device processes its microphone, as a person sets it on Settings › Voice and video: echo cancellation,
//! which noise suppressor, automatic gain or a volume of one's own, and the noise gate with its threshold. The ranges
//! and the defaults are decided here; the host hands the value to the call's chain as it is.

/// Echo cancellation, on or off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum EchoCancellation {
    /// What the speaker plays is cancelled from the microphone.
    #[default]
    On,
    /// Not cancelled.
    Off,
}

/// Which noise suppressor runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum Suppression {
    /// None.
    Off,
    /// The engine's own.
    Standard,
    /// RNNoise: Sharkord's default, the maintainer's call (2026-11-08).
    #[default]
    Rnnoise,
}

/// Whether the microphone's loudness is evened out, or set by the person.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum AutomaticGain {
    /// Evened out; the volume of one's own is not offered.
    #[default]
    On,
    /// The person's [`InputGain`] applies.
    Off,
}

/// The noise gate, on or off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum NoiseGate {
    /// Nothing is held back.
    #[default]
    Off,
    /// Frames under the [`Threshold`] are sent as silence.
    On,
}

/// The microphone's own volume while automatic gain is off, in whole dB from -20 to +20.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputGain(i8);

impl InputGain {
    /// The least and the most a person may set.
    pub const RANGE: std::ops::RangeInclusive<i8> = -20..=20;

    /// `db`, refused outside [`InputGain::RANGE`].
    pub fn new(db: i8) -> Option<Self> {
        Self::RANGE.contains(&db).then_some(Self(db))
    }

    /// In dB.
    pub fn db(self) -> i8 {
        self.0
    }
}

/// The noise gate's threshold, in whole dB below full scale from -70 to 0: the meter's range too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Threshold(i8);

impl Default for Threshold {
    /// Sharkord's.
    fn default() -> Self {
        Self(-48)
    }
}

impl Threshold {
    /// The least and the most a person may set.
    pub const RANGE: std::ops::RangeInclusive<i8> = -70..=0;
    /// How far over a room's measured floor "Set from my room" puts it: sharkord-native-client's, so the gate
    /// closes on the room and opens on a voice.
    const OVER_THE_ROOM: i16 = 8;

    /// `db`, refused outside [`Threshold::RANGE`].
    pub fn new(db: i8) -> Option<Self> {
        Self::RANGE.contains(&db).then_some(Self(db))
    }

    /// Over a room whose quietest moment measured `floor` dB, kept within the range.
    pub fn over_the_room(floor: i16) -> Self {
        let wanted =
            (floor + Self::OVER_THE_ROOM).clamp(i16::from(*Self::RANGE.start()), i16::from(*Self::RANGE.end()));
        Self(i8::try_from(wanted).unwrap_or(*Self::RANGE.start()))
    }

    /// In dB below full scale.
    pub fn db(self) -> i8 {
        self.0
    }
}

/// How this device processes its microphone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceProcessing {
    /// Echo cancellation.
    pub echo: EchoCancellation,
    /// The noise suppressor.
    pub suppression: Suppression,
    /// Automatic gain.
    pub automatic_gain: AutomaticGain,
    /// The volume of one's own, kept while automatic gain is on for when it is turned off.
    pub volume: InputGain,
    /// The noise gate.
    pub gate: NoiseGate,
    /// The gate's threshold, kept while it is off.
    pub threshold: Threshold,
}

impl VoiceProcessing {
    /// The volume of one's own, when it applies: automatic gain off.
    pub fn own_volume(&self) -> Option<InputGain> {
        (self.automatic_gain == AutomaticGain::Off).then_some(self.volume)
    }

    /// The gate's threshold, when it is on.
    pub fn gating(&self) -> Option<Threshold> {
        (self.gate == NoiseGate::On).then_some(self.threshold)
    }
}

/// The meter's range, in dB below full scale: the gate's.
pub const METER_FLOOR: i16 = -70;

/// Where `db` sits on the meter, from 0 to 100.
pub fn on_the_meter(db: i16) -> u8 {
    let span = -METER_FLOOR;
    u8::try_from((db.clamp(METER_FLOOR, 0) - METER_FLOOR) * 100 / span).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_device_starts_as_sharkord_does() {
        let fresh = VoiceProcessing::default();
        assert_eq!(fresh.echo, EchoCancellation::On);
        assert_eq!(fresh.suppression, Suppression::Rnnoise);
        assert_eq!(fresh.automatic_gain, AutomaticGain::On);
        assert_eq!((fresh.own_volume(), fresh.gating()), (None, None), "no volume of one's own, no gate");
        assert_eq!(fresh.threshold.db(), -48);
    }

    #[test]
    fn a_value_outside_its_range_is_refused_not_cut() {
        assert_eq!(InputGain::new(21), None);
        assert_eq!(InputGain::new(-20).map(InputGain::db), Some(-20));
        assert_eq!(Threshold::new(1), None);
        assert_eq!(Threshold::new(-71), None);
    }

    #[test]
    fn the_room_sets_the_threshold_eight_db_over_its_quietest_and_within_the_range() {
        assert_eq!(Threshold::over_the_room(-56).db(), -48);
        assert_eq!(Threshold::over_the_room(-90).db(), -70);
        assert_eq!(Threshold::over_the_room(-2).db(), 0);
    }

    #[test]
    fn the_meter_spans_the_gates_range() {
        assert_eq!(on_the_meter(-70), 0);
        assert_eq!(on_the_meter(-35), 50);
        assert_eq!(on_the_meter(0), 100);
        assert_eq!(on_the_meter(-100), 0);
    }
}
