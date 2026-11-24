//! A choice of a preset or a value typed or slid: what an invite's number of people and a share's custom rate share.
//! The text typed is kept as typed, so a value out of bounds stays on screen beside its refusal.

/// A value a [`PresetOrCustom`] holds: made only in its bounds, read from typed text, and placed on a slider.
pub trait Bounded: Copy + Eq + Sized {
    /// The least it may be.
    const LEAST: Self;
    /// The most it may be.
    const MOST: Self;

    /// The value `typed` writes, spaces round it left out.
    fn parse(typed: &str) -> Result<Self, TypedRefused>;

    /// The value as its field shows it, which [`Bounded::parse`] reads back.
    fn written(self) -> String;

    /// The value nearest the number `typed` writes, held to the bounds; none when it writes no number.
    fn nearest(typed: &str) -> Option<Self>;

    /// Where it stands on the slider, from `LEAST.position()` to `MOST.position()`, one apart per step.
    fn position(self) -> u32;

    /// The value at slider position `position`, held to the bounds.
    fn at_position(position: u32) -> Self;
}

/// A preset of a row with a custom field: what the field starts at when Custom is pressed on it, and the choice a
/// custom value makes.
pub trait Preset: Copy + Eq {
    /// What the custom field and the slider hold.
    type Value: Bounded;

    /// The value the custom field starts at when Custom is pressed while this preset is chosen.
    fn starts_custom(self) -> Self::Value;

    /// The choice the custom `value` makes.
    fn custom(value: Self::Value) -> Self;
}

/// Why typed text is no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedRefused {
    /// Nothing is typed yet.
    Empty,
    /// It writes no value in the bounds: a letter, a number past either bound, one between two steps.
    OutOfBounds,
}

/// A value as a row of presets with a custom field has it: one of the presets, or the text in the field, which the
/// field and the slider share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetOrCustom<P> {
    /// One of the row's presets.
    Preset(P),
    /// Custom, by the text in its field, kept as typed.
    Custom(String),
}

impl<P: Default> Default for PresetOrCustom<P> {
    fn default() -> Self {
        PresetOrCustom::Preset(P::default())
    }
}

impl<P: Preset> PresetOrCustom<P> {
    /// The choice made, or why the text typed makes none.
    pub fn value(&self) -> Result<P, TypedRefused> {
        match self {
            PresetOrCustom::Preset(preset) => Ok(*preset),
            PresetOrCustom::Custom(typed) => P::Value::parse(typed).map(P::custom),
        }
    }

    /// Where the slider stands: the value typed, held to the bounds; the least where the text writes no number.
    pub fn slid_to(&self) -> P::Value {
        match self {
            PresetOrCustom::Preset(preset) => preset.starts_custom(),
            PresetOrCustom::Custom(typed) => P::Value::nearest(typed).unwrap_or(P::Value::LEAST),
        }
    }

    /// Custom pressed: the field starts at the preset's value, or keeps what is typed in it.
    pub fn custom(&self) -> PresetOrCustom<P> {
        match self {
            PresetOrCustom::Custom(typed) => PresetOrCustom::Custom(typed.clone()),
            PresetOrCustom::Preset(preset) => PresetOrCustom::Custom(preset.starts_custom().written()),
        }
    }

    /// The slider moved to `position`: the field writes the value there.
    pub fn slid(position: u32) -> PresetOrCustom<P> {
        PresetOrCustom::Custom(P::Value::at_position(position).written())
    }

    /// The field's whole text after an edit.
    pub fn typed(typed: String) -> PresetOrCustom<P> {
        PresetOrCustom::Custom(typed)
    }

    /// The value is out of bounds, as the field's refusal says; empty text is no refusal yet.
    pub fn refused(&self) -> bool {
        self.value().err() == Some(TypedRefused::OutOfBounds)
    }
}
