//! The share panel's quality rows: a choice each, with the hint the canvas puts under the codec, then whether it carries sound. The codec's hint
//! says what encodes the codec chosen.

use crate::custom_choice::{CustomField, CustomNote, Taken, custom_choice};
use crate::labels::{label, phrase};
use crate::{Renderer, TextClass, Theme, choice, coloured, switch};
use choices::{
    Bounded, Ceiling, Codec, EncoderOption, EncodesOn, FrameRate, Kbits, PresetOrCustom, QualityPick, Resolution,
    ShareQuality,
};
use design::{Content, Space, TypeStyle};
use iced_core::{Element, Widget};
use iced_widget::Column;
use screens::RateMessage;
use std::rc::Rc;
use words::{Phrase, Sentence, Words};

/// The Bitrate row as the call has it: a preset, or the custom rate as typed, and the line under that rate.
pub struct Bitrate<'a> {
    /// The preset chosen, or Custom with its field's text.
    pub rate: &'a PresetOrCustom<Ceiling>,
    /// Under the custom rate: the ceiling it makes, or why the text typed makes none.
    pub note: Option<Sentence>,
}

/// Resolution, frame rate, bitrate and codec as `quality` and `bitrate` have them, the codecs `encoders` make; a choice
/// sends what `pick` makes of it, and the custom rate what `rate` makes of what is done to it.
pub fn quality_rows<'a, Message: Clone + 'a, R: Renderer + 'a>(
    quality: &ShareQuality,
    encoders: &[EncoderOption],
    bitrate: Bitrate<'a>,
    pick: impl Fn(QualityPick) -> Message,
    rate: impl Fn(RateMessage) -> Message + 'a,
    words: &'a Words,
) -> Vec<Element<'a, Message, Theme, R>> {
    let resolution = choice(
        phrase(Phrase::ShareResolution, words),
        Resolution::ALL.map(|size| (label(&words.resolution(size)), pick(QualityPick::Resolution(size)))).into(),
        Resolution::ALL.iter().position(|size| *size == quality.resolution).unwrap_or_default(),
        false,
    );
    let frame_rate = choice(
        phrase(Phrase::ShareFrameRate, words),
        FrameRate::ALL.map(|rate| (label(&words.frame_rate(rate)), pick(QualityPick::FrameRate(rate)))).into(),
        FrameRate::ALL.iter().position(|rate| *rate == quality.frame_rate).unwrap_or_default(),
        false,
    );
    let ceiling = bitrate_row(bitrate, &pick, rate, words);
    let codecs = Codec::offered(encoders);
    let sent = quality.codec_on(encoders);
    let codec = choice(
        phrase(Phrase::ShareCodec, words),
        codecs.iter().map(|codec| (label(codec.label()), pick(QualityPick::Codec(*codec)))).collect(),
        codecs.iter().position(|codec| *codec == sent).unwrap_or_default(),
        false,
    );
    let encoded = sent.encoded_on(encoders).map(|on| match on {
        EncodesOn::GraphicsCard => words.phrase(Phrase::HardwareEncoded).into_owned(),
        EncodesOn::UntestedCard => {
            format!("{}, {}", words.phrase(Phrase::HardwareEncoded), words.phrase(Phrase::Untested))
        }
        EncodesOn::Processor => words.phrase(Phrase::SoftwareEncoded).into_owned(),
    });
    vec![
        resolution,
        frame_rate,
        ceiling,
        match encoded {
            Some(encoded) => hinted(codec, encoded),
            None => codec,
        },
        switch(
            &phrase(Phrase::ShareScreenSound, words),
            Some(&words.phrase(Phrase::ShareScreenSoundHint)),
            quality.sound.screen,
            pick(QualityPick::ScreenSound(!quality.sound.screen)),
        ),
        switch(
            &phrase(Phrase::ShareWindowSound, words),
            None,
            quality.sound.window,
            pick(QualityPick::WindowSound(!quality.sound.window)),
        ),
    ]
}

/// A row with a quiet line under it.
fn hinted<'a, Message: 'a, R: Renderer + 'a>(
    row: Element<'a, Message, Theme, R>,
    hint: String,
) -> Element<'a, Message, Theme, R> {
    let hint = coloured(hint, TypeStyle::Caption, TextClass::Content(Content::Secondary));
    Column::with_children([row, hint._boxed()]).spacing(Space::Xs.px())._boxed()
}

/// The Bitrate row: its presets, the least first, then Custom, and under Custom its slider and field in Mbit/s.
fn bitrate_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    bitrate: Bitrate<'a>,
    pick: &impl Fn(QualityPick) -> Message,
    rate: impl Fn(RateMessage) -> Message + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let presets =
        Ceiling::ALL.map(|ceiling| (label(&words.ceiling(ceiling)), pick(QualityPick::Ceiling(ceiling)))).into();
    let custom = (phrase(Phrase::Custom, words), rate(RateMessage::Custom));
    let taken = match bitrate.rate {
        PresetOrCustom::Preset(ceiling) => {
            Taken::Preset(Ceiling::ALL.iter().position(|preset| preset == ceiling).unwrap_or_default())
        }
        PresetOrCustom::Custom(typed) => {
            let step = |kbits: Kbits| u16::try_from(kbits.position()).unwrap_or(u16::MAX);
            let refused = bitrate.rate.refused();
            let rate = Rc::new(rate);
            let typing = Rc::clone(&rate);
            Taken::Custom(CustomField {
                name: phrase(Phrase::ShareRateLabel, words),
                steps: step(Kbits::LEAST)..=step(Kbits::MOST),
                at: step(bitrate.rate.slid_to()),
                slid: Rc::new(move |at| rate(RateMessage::Slid(at))),
                typed,
                on_input: Box::new(move |typed| typing(RateMessage::Typed(typed))),
                unit: Some(words.phrase(Phrase::ShareRateUnit).into_owned()),
                on_submit: None,
                note: bitrate.note.map(|note| {
                    let said = words.say(&note);
                    if refused { CustomNote::Refused(said) } else { CustomNote::Said(said) }
                }),
            })
        }
    };
    custom_choice(phrase(Phrase::ShareBitrateHeading, words), presets, custom, taken, false)
}
