//! A message's reactions as Discord draws them: a chip for each emoji with how many gave it, one's own ringed in the
//! accent, a press adding or taking back one's own; and the way to add another after them.

use crate::button::{ButtonClass, actionable};
use crate::typography::text;
use crate::{Button, Icon, Renderer, Surfaced, Theme};
use design::{
    ButtonVariant, CHIP_EMOJI, CHIP_GAP, CHIP_HEIGHT, CHIP_PICTURE, Label, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::text::{LineHeight, Wrapping};
use iced_core::{Alignment, Element, Length, Padding, Pixels, Widget, image};
use iced_widget::{Row, Space as Gap, button, container};
use words::{Sentence, Words};

/// What a reaction is: a unicode emoji, or a server's own by its name and its picture once fetched.
#[derive(Clone, Debug, PartialEq)]
pub enum ReactionEmoji {
    /// A unicode emoji, drawn in the bundled emoji font.
    Unicode(String),
    /// A server's own emoji: its name without colons, and its picture; drawn as `:name:` until the picture is held.
    Custom {
        /// Its name, without colons.
        name: String,
        /// Its picture, once fetched.
        picture: Option<image::Handle>,
    },
}

impl ReactionEmoji {
    /// What a screen reader and a fallback say of it: the emoji itself, or `:name:`.
    fn said(&self) -> String {
        match self {
            ReactionEmoji::Unicode(emoji) => emoji.clone(),
            ReactionEmoji::Custom { name, .. } => format!(":{name}:"),
        }
    }
}

/// One reaction under a message.
#[derive(Clone, Debug)]
pub struct ReactionChip<Message> {
    /// The emoji.
    pub emoji: ReactionEmoji,
    /// How many gave it, at least one.
    pub count: u32,
    /// This account gave it: drawn ringed in the accent.
    pub mine: bool,
    /// What a press sends: taking one's own back when `mine`, else adding it.
    pub toggle: Message,
}

/// A message's `chips` in a row that wraps, then, while `add` is given, the way to add another (its label and what it
/// sends), said in `words`.
pub fn reactions<'a, Message: Clone + 'a, R: Renderer + 'a>(
    chips: Vec<ReactionChip<Message>>,
    add: Option<(Label, Message)>,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let mut cells: Vec<Element<'a, Message, Theme, R>> = chips.into_iter().map(|each| chip(each, words)).collect();
    if let Some((label, message)) = add {
        cells.push(
            Button::new(label)
                .icon_only(Icon::SmilePlus)
                .variant(ButtonVariant::Ghost)
                .small()
                .on_press(message)
                .view(),
        );
    }
    Row::with_children(cells).spacing(Space::Xs.px()).align_y(Alignment::Center).wrap()._boxed()
}

/// The picture or glyph of `emoji` at a chip's size.
pub(crate) fn emoji_face<'a, Message: 'a, R: Renderer + 'a>(
    emoji: &ReactionEmoji,
    glyph: f32,
    picture: f32,
) -> Element<'a, Message, Theme, R> {
    match emoji {
        ReactionEmoji::Unicode(shown) => text(shown.clone(), TypeStyle::Body)
            .font_maybe(crate::fonts::emoji_draws(shown).then(crate::fonts::emoji))
            .size(Pixels(glyph))
            .line_height(LineHeight::Absolute(Pixels(picture)))
            .wrapping(Wrapping::None)
            ._boxed(),
        ReactionEmoji::Custom { picture: Some(handle), .. } => {
            iced_widget::image(handle.clone()).width(Length::Fixed(picture)).height(Length::Fixed(picture))._boxed()
        }
        ReactionEmoji::Custom { picture: None, .. } => {
            text(emoji.said(), TypeStyle::Small).wrapping(Wrapping::None)._boxed()
        }
    }
}

/// `emoji` in a slot `picture` square (a picker's cell, a chooser's row): as [`emoji_face`], but a server's own whose
/// picture is not yet fetched holds its square, its name left to the label.
pub(crate) fn emoji_in_slot<'a, Message: 'a, R: Renderer + 'a>(
    emoji: &ReactionEmoji,
    glyph: f32,
    picture: f32,
) -> Element<'a, Message, Theme, R> {
    match emoji {
        ReactionEmoji::Custom { picture: None, .. } => container(Gap::new())
            .width(Length::Fixed(picture))
            .height(Length::Fixed(picture))
            .class(Surfaced::Fill {
                fill: design::Role::Surface(design::Surface::Inset),
                text: design::Role::Content(design::Content::Secondary),
                radius: design::Radius::S,
            })
            ._boxed(),
        ReactionEmoji::Unicode(_) | ReactionEmoji::Custom { picture: Some(_), .. } => emoji_face(emoji, glyph, picture),
    }
}

fn chip<'a, Message: Clone + 'a, R: Renderer + 'a>(
    chip: ReactionChip<Message>,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let said = words.say(&Sentence::Reaction { emoji: chip.emoji.said(), count: chip.count, yours: chip.mine });
    let label = crate::labels::label(&said);
    let count = text(words.count(u64::from(chip.count)), TypeStyle::SmallStrong).wrapping(Wrapping::None);
    let content = Row::with_children([emoji_face(&chip.emoji, CHIP_EMOJI, CHIP_PICTURE), count._boxed()])
        .spacing(CHIP_GAP)
        .align_y(Alignment::Center);
    let inner = button::Button::new(iced_widget::container(content).center_y(Length::Fixed(CHIP_HEIGHT)))
        .class(ButtonClass::Chip { mine: chip.mine })
        .padding(Padding::from([0.0, Space::S.px()]))
        .on_press(chip.toggle.clone());
    let pressable =
        actionable(inner, Semantics { role: SemanticRole::Button, label: label.clone() }, Some(chip.toggle));
    crate::tooltip(pressable, &label, iced_widget::tooltip::Position::Top)
}
