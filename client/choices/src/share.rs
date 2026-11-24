//! How a share goes out, as a person chose it: its size, frame rate, a ceiling on its rate and its codec. The encoder
//! is the device's first for that codec.
//! The share panel is the only place they are chosen: it opens on the last share's choice, which a share keeps.

use crate::custom::{Bounded, Preset, PresetOrCustom, TypedRefused};
use proto::rules::SHARE_MAX_BITS_PER_SECOND;

/// The size a share is sent at: the screen's own, or scaled down so that its short side is the choice. Never above the
/// screen: a screen that size or smaller goes as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum Resolution {
    /// The screen's own size.
    #[default]
    Source,
    /// A short side of 1440 pixels.
    P1440,
    /// A short side of 1080 pixels.
    P1080,
    /// A short side of 720 pixels.
    P720,
}

impl Resolution {
    /// Every choice, in the order the row lists them: the smallest first.
    pub const ALL: [Resolution; 4] = [Resolution::P720, Resolution::P1080, Resolution::P1440, Resolution::Source];

    /// The short side it scales to; none for the screen's own size.
    pub fn short_side(self) -> Option<u32> {
        match self {
            Resolution::Source => None,
            Resolution::P1440 => Some(1440),
            Resolution::P1080 => Some(1080),
            Resolution::P720 => Some(720),
        }
    }
}

/// The pictures a second a share is sent at, at most. 60 is the cap the capture and the envelope are built for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum FrameRate {
    /// 60 a second.
    #[default]
    Sixty,
    /// 30 a second.
    Thirty,
    /// 24 a second.
    TwentyFour,
    /// 15 a second.
    Fifteen,
}

impl FrameRate {
    /// Every choice, in the order the row lists them: the fewest first.
    pub const ALL: [FrameRate; 4] = [FrameRate::Fifteen, FrameRate::TwentyFour, FrameRate::Thirty, FrameRate::Sixty];

    /// The pictures a second, at most.
    pub fn per_second(self) -> u32 {
        match self {
            FrameRate::Sixty => 60,
            FrameRate::Thirty => 30,
            FrameRate::TwentyFour => 24,
            FrameRate::Fifteen => 15,
        }
    }
}

/// A custom ceiling, in kbit/s: from [`Kbits::LEAST`] to the most a share is sent at
/// ([`SHARE_MAX_BITS_PER_SECOND`]), in steps of [`Kbits::STEP`]; made only so.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kbits(u32);

impl Kbits {
    /// The step between two custom ceilings, 0.5 Mbit/s.
    pub const STEP: u32 = 500;
    /// The least custom ceiling.
    pub const LEAST: Kbits = Kbits(Kbits::STEP);
    /// The most: what a share is ever sent at.
    pub const MOST: Kbits = Kbits(most_kbits());
    /// Where the custom field starts from Automatic, which has no rate of its own: 12 Mbit/s, as the canvas has it.
    pub const FROM_AUTOMATIC: Kbits = Kbits(12_000);

    /// `kbits`, if it is a step in the bounds.
    pub const fn new(kbits: u32) -> Option<Kbits> {
        if kbits >= Kbits::LEAST.0 && kbits <= Kbits::MOST.0 && kbits.is_multiple_of(Kbits::STEP) {
            Some(Kbits(kbits))
        } else {
            None
        }
    }

    /// The ceiling in bits a second.
    pub fn bits_per_second(self) -> u64 {
        u64::from(self.0) * 1_000
    }

    /// The ceiling in kbit/s, as the device keeps it.
    pub const fn kbits(self) -> u32 {
        self.0
    }
}

/// [`SHARE_MAX_BITS_PER_SECOND`] in kbit/s, held by the compiler to a whole step that fits.
const fn most_kbits() -> u32 {
    let kbits = SHARE_MAX_BITS_PER_SECOND / 1_000;
    assert!(SHARE_MAX_BITS_PER_SECOND.is_multiple_of(1_000) && kbits >> 32 == 0);
    let bytes = kbits.to_le_bytes();
    let kbits = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert!(kbits.is_multiple_of(Kbits::STEP) && kbits >= Kbits::STEP);
    kbits
}

impl report::Public for Kbits {
    fn fact(&self) -> report::Fact {
        report::Fact::Count(u64::from(self.0))
    }
}

impl Bounded for Kbits {
    const LEAST: Kbits = Kbits::LEAST;
    const MOST: Kbits = Kbits::MOST;

    /// Mbit/s with at most one decimal, by a point or a comma: "2.5", "40".
    fn parse(typed: &str) -> Result<Kbits, TypedRefused> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err(TypedRefused::Empty);
        }
        tenths(typed).and_then(|tenths| tenths.checked_mul(100)).and_then(Kbits::new).ok_or(TypedRefused::OutOfBounds)
    }

    fn written(self) -> String {
        let (whole, tenth) = (self.0 / 1_000, self.0 % 1_000 / 100);
        if tenth == 0 { whole.to_string() } else { format!("{whole}.{tenth}") }
    }

    fn nearest(typed: &str) -> Option<Kbits> {
        let mbit: f64 = typed.trim().replace(',', ".").parse().ok().filter(|mbit: &f64| mbit.is_finite())?;
        let steps = (mbit * 1_000.0 / f64::from(Kbits::STEP)).round();
        let (least, most) = (f64::from(Kbits::LEAST.position()), f64::from(Kbits::MOST.position()));
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to the slider's steps")]
        let position = steps.clamp(least, most) as u32;
        Some(Kbits::at_position(position))
    }

    fn position(self) -> u32 {
        self.0 / Kbits::STEP
    }

    fn at_position(position: u32) -> Kbits {
        let steps = position.clamp(Kbits::LEAST.position(), Kbits::MOST.position());
        Kbits(steps * Kbits::STEP)
    }
}

/// The tenths of a unit `typed` writes: digits, then at most one decimal after a point or a comma; none otherwise.
fn tenths(typed: &str) -> Option<u32> {
    let (whole, tenth) = match typed.split_once(['.', ',']) {
        Some((whole, tenth)) => (whole, Some(tenth)),
        None => (typed, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|digit| digit.is_ascii_digit());
    if !digits(whole) || tenth.is_some_and(|tenth| tenth.len() != 1 || !digits(tenth)) {
        return None;
    }
    let tenth = tenth.map_or(Some(0), |tenth| tenth.parse::<u32>().ok())?;
    whole.parse::<u32>().ok()?.checked_mul(10)?.checked_add(tenth)
}

/// A cap on the share's budget, never a target: under it the rate rules still decide, and the network may use less.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum Ceiling {
    /// At most 4 Mbit/s.
    Mbit4,
    /// At most 8 Mbit/s.
    Mbit8,
    /// At most 16 Mbit/s.
    Mbit16,
    /// No cap: the rate rules alone decide.
    #[default]
    Automatic,
    /// At most the rate a person typed or slid.
    Custom(#[public] Kbits),
}

const _: () = assert!(
    Kbits::new(4_000).is_some()
        && Kbits::new(8_000).is_some()
        && Kbits::new(16_000).is_some()
        && Kbits::new(Kbits::FROM_AUTOMATIC.0).is_some(),
    "a preset ceiling a custom one could not be"
);

impl Ceiling {
    /// The presets, in the order the row lists them, the least first; Custom follows them.
    pub const ALL: [Ceiling; 4] = [Ceiling::Mbit4, Ceiling::Mbit8, Ceiling::Mbit16, Ceiling::Automatic];

    /// None where the rate rules alone decide.
    pub fn bits_per_second(self) -> Option<u64> {
        match self {
            Ceiling::Automatic => None,
            Ceiling::Mbit16 => Some(16_000_000),
            Ceiling::Mbit8 => Some(8_000_000),
            Ceiling::Mbit4 => Some(4_000_000),
            Ceiling::Custom(kbits) => Some(kbits.bits_per_second()),
        }
    }

    /// The ceiling as the row with a custom field has it: a preset, or the custom rate as its field writes it.
    pub fn choice(self) -> PresetOrCustom<Ceiling> {
        match self {
            Ceiling::Custom(kbits) => PresetOrCustom::Custom(kbits.written()),
            Ceiling::Mbit4 | Ceiling::Mbit8 | Ceiling::Mbit16 | Ceiling::Automatic => PresetOrCustom::Preset(self),
        }
    }
}

impl Preset for Ceiling {
    type Value = Kbits;

    fn starts_custom(self) -> Kbits {
        match self {
            Ceiling::Custom(kbits) => kbits,
            Ceiling::Automatic => Kbits::FROM_AUTOMATIC,
            Ceiling::Mbit4 | Ceiling::Mbit8 | Ceiling::Mbit16 => self
                .bits_per_second()
                .and_then(|bits| u32::try_from(bits / 1_000).ok())
                .and_then(Kbits::new)
                .unwrap_or(Kbits::FROM_AUTOMATIC),
        }
    }

    fn custom(kbits: Kbits) -> Ceiling {
        Ceiling::Custom(kbits)
    }
}

/// The codec a share is sent in; a viewer decodes either, in hardware where it can.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, report::Public)]
pub enum Codec {
    /// H.264, which every device here encodes, on its graphics card or with x264.
    H264,
    /// AV1.
    Av1,
}

impl Codec {
    /// Every choice, in the order the row lists them.
    pub const ALL: [Codec; 2] = [Codec::H264, Codec::Av1];

    /// "H.264", "AV1": the codec's own name in every language.
    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Av1 => "AV1",
        }
    }

    /// The codecs the row offers on a device with `encoders`: H.264 always, AV1 only where an encoder makes it.
    pub fn offered(encoders: &[EncoderOption]) -> Vec<Codec> {
        Codec::ALL
            .into_iter()
            .filter(|codec| *codec == Codec::H264 || encoders.iter().any(|encoder| encoder.codec == *codec))
            .collect()
    }

    /// The codec a share on a device with `encoders` is sent in until a person picks one: AV1 where a graphics card
    /// here encodes it with an encoder someone tested (sharper text at the same bits), else H.264 (on the processor
    /// x264 costs less than AV1).
    pub fn default_for(encoders: &[EncoderOption]) -> Codec {
        let card_av1 = EncoderOption { codec: Codec::Av1, on: EncodesOn::GraphicsCard };
        if encoders.contains(&card_av1) { Codec::Av1 } else { Codec::H264 }
    }

    /// What encodes it on a device with `encoders`, in the order they are tried: the first of its own; none where
    /// nothing here makes it.
    pub fn encoded_on(self, encoders: &[EncoderOption]) -> Option<EncodesOn> {
        encoders.iter().find(|encoder| encoder.codec == self).map(|encoder| encoder.on)
    }
}

/// What an encoder codes its pictures on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum EncodesOn {
    /// The graphics card's own encoder.
    #[default]
    GraphicsCard,
    /// A graphics card's encoder whose family nobody has tested with Chatkord: tried after every tested one, never
    /// the default codec's reason.
    UntestedCard,
    /// The processor: a software encoder, offered only for a codec no graphics card here encodes.
    Processor,
}

/// One of this device's encoders, as far as a person is told of it: the codec it makes and what it runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EncoderOption {
    /// The codec it makes.
    pub codec: Codec,
    /// What it codes its pictures on.
    pub on: EncodesOn,
}

/// Whether a share carries sound: a whole screen every app's but Chatkord's, a window its own app's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SoundSwitches {
    /// A whole screen's share carries sound.
    pub screen: bool,
    /// A window's share carries sound.
    pub window: bool,
}

impl Default for SoundSwitches {
    fn default() -> Self {
        Self { screen: true, window: true }
    }
}

/// Every choice of how a share goes out: the last share's, kept, or the one being made.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ShareQuality {
    /// The size it is sent at.
    pub resolution: Resolution,
    /// The pictures a second it is sent at, at most.
    pub frame_rate: FrameRate,
    /// The cap on its budget.
    pub ceiling: Ceiling,
    /// The codec a person picked; none until one is, and the device's own then ([`Codec::default_for`]).
    pub codec: Option<Codec>,
    /// Whether it carries sound.
    pub sound: SoundSwitches,
}

/// One row's choice, in the share panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPick {
    /// A size was picked.
    Resolution(Resolution),
    /// A frame rate was picked.
    FrameRate(FrameRate),
    /// A ceiling was picked.
    Ceiling(Ceiling),
    /// A codec was picked.
    Codec(Codec),
    /// The switch for a whole screen's sound was turned on or off.
    ScreenSound(bool),
    /// The switch for a window's sound was turned on or off.
    WindowSound(bool),
}

impl ShareQuality {
    /// Changes the one choice `pick` names and leaves the others.
    pub fn pick(&mut self, pick: QualityPick) {
        match pick {
            QualityPick::Resolution(resolution) => self.resolution = resolution,
            QualityPick::FrameRate(frame_rate) => self.frame_rate = frame_rate,
            QualityPick::Ceiling(ceiling) => self.ceiling = ceiling,
            QualityPick::Codec(codec) => self.codec = Some(codec),
            QualityPick::ScreenSound(on) => self.sound.screen = on,
            QualityPick::WindowSound(on) => self.sound.window = on,
        }
    }

    /// The same choice on a device with `encoders`: a codec picked that it does not make is its own default here.
    pub fn on(self, encoders: &[EncoderOption]) -> Self {
        let codec = self.codec.filter(|codec| Codec::offered(encoders).contains(codec));
        Self { codec, ..self }
    }

    /// The codec it is sent in on a device with `encoders`: the one picked, else the device's own.
    pub fn codec_on(&self, encoders: &[EncoderOption]) -> Codec {
        self.codec.unwrap_or_else(|| Codec::default_for(encoders))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H264_CARD: EncoderOption = EncoderOption { codec: Codec::H264, on: EncodesOn::GraphicsCard };
    const AV1_CARD: EncoderOption = EncoderOption { codec: Codec::Av1, on: EncodesOn::GraphicsCard };
    const AV1_PROCESSOR: EncoderOption = EncoderOption { codec: Codec::Av1, on: EncodesOn::Processor };

    #[test]
    fn the_rows_read_as_the_canvas_has_them_each_ascending() {
        assert_eq!(Ceiling::default(), Ceiling::Automatic);
        assert_eq!(Ceiling::Automatic.bits_per_second(), None, "automatic is no ceiling, not a large one");
        assert_eq!(Codec::ALL.map(Codec::label), ["H.264", "AV1"]);
    }

    #[test]
    fn a_custom_rate_is_a_half_megabit_step_from_half_a_megabit_to_the_most_a_share_sends() {
        assert_eq!(Kbits::LEAST.bits_per_second(), 500_000);
        assert_eq!(Kbits::MOST.bits_per_second(), SHARE_MAX_BITS_PER_SECOND);
        assert_eq!(Kbits::new(2_500).map(Kbits::kbits), Some(2_500));
        assert_eq!(Kbits::new(2_250), None, "between two steps");
        assert_eq!(Kbits::new(0), None);
        assert_eq!(Kbits::new(Kbits::MOST.kbits() + Kbits::STEP), None);
        assert_eq!(Ceiling::Custom(Kbits::LEAST).bits_per_second(), Some(500_000), "a ceiling keeps its meaning");
    }

    #[test]
    fn a_typed_rate_is_mbit_with_at_most_one_decimal_in_its_bounds() {
        let parsed = |typed: &str| Kbits::parse(typed).map(Kbits::kbits);
        assert_eq!(parsed("2.5"), Ok(2_500));
        assert_eq!(parsed(" 40 "), Ok(40_000));
        assert_eq!(parsed("0.5"), Ok(500));
        assert_eq!(parsed("2,5"), Ok(2_500), "a comma is a decimal point too");
        assert_eq!(parsed("2.25"), Err(TypedRefused::OutOfBounds), "two decimals");
        assert_eq!(parsed("2.3"), Err(TypedRefused::OutOfBounds), "between two steps");
        assert_eq!(parsed("45"), Err(TypedRefused::OutOfBounds), "past the most");
        assert_eq!(parsed("0"), Err(TypedRefused::OutOfBounds));
        assert_eq!(parsed(""), Err(TypedRefused::Empty));
        for typed in ["abc", "2.", ".5", "-1", "1e1", "99999999999999999999"] {
            assert_eq!(parsed(typed), Err(TypedRefused::OutOfBounds), "{typed}");
        }
        assert_eq!(Kbits::LEAST.written(), "0.5");
        assert_eq!(Kbits::MOST.written(), "40");
        assert_eq!(Kbits::parse(&Kbits::FROM_AUTOMATIC.written()), Ok(Kbits::FROM_AUTOMATIC));
    }

    #[test]
    fn the_custom_rate_s_slider_holds_the_typed_rate_to_its_steps_and_ends() {
        let typed = |text: &str| PresetOrCustom::<Ceiling>::Custom(text.into());
        assert_eq!(typed("2.5").slid_to(), Kbits::new(2_500).unwrap());
        assert_eq!(typed("45").slid_to(), Kbits::MOST);
        assert_eq!(typed("0").slid_to(), Kbits::LEAST);
        assert_eq!(typed("abc").slid_to(), Kbits::LEAST);
        assert_eq!(PresetOrCustom::<Ceiling>::slid(5).value(), Ok(Ceiling::Custom(Kbits::new(2_500).unwrap())));
        assert_eq!(PresetOrCustom::<Ceiling>::slid(0).value(), Ok(Ceiling::Custom(Kbits::LEAST)), "held to the end");
        assert_eq!(typed("45").value(), Err(TypedRefused::OutOfBounds));
        assert!(typed("45").refused() && !typed("").refused());
        assert_eq!(PresetOrCustom::Preset(Ceiling::Mbit8).custom(), typed("8"), "Custom starts at the preset's rate");
        assert_eq!(PresetOrCustom::Preset(Ceiling::Automatic).custom(), typed("12"));
        assert_eq!(Ceiling::Custom(Kbits::new(2_500).unwrap()).choice(), typed("2.5"));
        assert_eq!(Ceiling::Mbit4.choice(), PresetOrCustom::Preset(Ceiling::Mbit4));
    }

    #[test]
    fn av1_is_offered_only_where_an_encoder_makes_it() {
        assert_eq!(Codec::offered(&[]), [Codec::H264]);
        assert_eq!(Codec::offered(&[H264_CARD, H264_CARD]), [Codec::H264]);
        assert_eq!(Codec::offered(&[H264_CARD, AV1_CARD]), [Codec::H264, Codec::Av1]);
        assert_eq!(Codec::offered(&[H264_CARD, AV1_PROCESSOR]), [Codec::H264, Codec::Av1]);
    }

    #[test]
    fn a_codec_is_encoded_on_what_its_first_encoder_runs_on() {
        assert_eq!(Codec::H264.encoded_on(&[H264_CARD, AV1_PROCESSOR]), Some(EncodesOn::GraphicsCard));
        assert_eq!(Codec::Av1.encoded_on(&[H264_CARD, AV1_PROCESSOR]), Some(EncodesOn::Processor));
        assert_eq!(Codec::Av1.encoded_on(&[AV1_CARD, AV1_PROCESSOR]), Some(EncodesOn::GraphicsCard));
        assert_eq!(Codec::Av1.encoded_on(&[H264_CARD]), None);
    }

    #[test]
    fn av1_kept_where_no_encoder_makes_it_is_the_devices_own_here() {
        let kept = ShareQuality { codec: Some(Codec::Av1), ..ShareQuality::default() };
        assert_eq!(kept.clone().on(&[H264_CARD]).codec_on(&[H264_CARD]), Codec::H264);
        assert_eq!(kept.clone().on(&[H264_CARD, AV1_PROCESSOR]), kept);
    }

    #[test]
    fn until_one_is_picked_a_share_is_av1_where_a_graphics_card_encodes_it_else_h264() {
        let unpicked = ShareQuality::default();
        assert_eq!(unpicked.codec_on(&[H264_CARD, AV1_CARD]), Codec::Av1);
        assert_eq!(unpicked.codec_on(&[AV1_CARD]), Codec::Av1);
        assert_eq!(unpicked.codec_on(&[H264_CARD, AV1_PROCESSOR]), Codec::H264, "AV1 on the processor costs more");
        assert_eq!(unpicked.codec_on(&[H264_CARD]), Codec::H264);
        assert_eq!(unpicked.codec_on(&[]), Codec::H264);
        let picked = ShareQuality { codec: Some(Codec::H264), ..unpicked };
        assert_eq!(picked.codec_on(&[H264_CARD, AV1_CARD]), Codec::H264, "a pick stands");
        let av1_untested = EncoderOption { codec: Codec::Av1, on: EncodesOn::UntestedCard };
        assert_eq!(unpicked.codec_on(&[H264_CARD, av1_untested]), Codec::H264, "nobody tested that AV1 encoder");
        assert_eq!(Codec::offered(&[H264_CARD, av1_untested]), [Codec::H264, Codec::Av1], "but it is offered");
    }

    #[test]
    fn a_pick_changes_its_row_alone() {
        let mut quality = ShareQuality::default();
        quality.pick(QualityPick::FrameRate(FrameRate::Thirty));
        quality.pick(QualityPick::Ceiling(Ceiling::Mbit8));
        quality.pick(QualityPick::Resolution(Resolution::P1080));
        quality.pick(QualityPick::WindowSound(false));
        assert_eq!(
            quality,
            ShareQuality {
                resolution: Resolution::P1080,
                frame_rate: FrameRate::Thirty,
                ceiling: Ceiling::Mbit8,
                codec: None,
                sound: SoundSwitches { screen: true, window: false },
            }
        );
    }

    #[test]
    fn a_share_carries_sound_unless_its_switch_is_off() {
        assert_eq!(ShareQuality::default().sound, SoundSwitches { screen: true, window: true });
    }
}
