//! A channel's pinned messages, Discord's panel from the channel's header: each pin a card with its author, its time,
//! the way to it and, for who may pin, the way to unpin it, then its words.

use crate::labels::{label, phrase};
use crate::typography::{coloured, line};
use crate::{AvatarKind, Button, Labelled, Renderer, Surfaced, TextClass, Theme, text};
use design::{
    AvatarSize, ButtonVariant, Content, Elevation, PINS_PANEL_WIDTH, Radius, SemanticRole, Semantics, Space, Surface,
    TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container, scrollable};
use words::{Phrase, Words};

/// A pinned message as the panel lists it.
pub struct Pin<'a, Message, R> {
    /// Its author's name.
    pub name: String,
    /// How its author's avatar is drawn.
    pub avatar: AvatarKind,
    /// The picture its author chose, as the host holds it; none for their initials.
    pub picture: Option<crate::Held>,
    /// When it was written, said.
    pub at: String,
    /// Its words, markdown's markers left out: what a screen reader hears.
    pub text: String,
    /// Its words drawn as the list draws them: marks, styles, emoji.
    pub body: Element<'a, Message, Theme, R>,
    /// What Jump sends: the list moves to it.
    pub jump: Message,
    /// What Unpin sends; none for who may not pin.
    pub unpin: Option<Message>,
}

/// The pinned messages' panel: its heading and `pins`, newest pin first, or that none are; at most `most_height` tall,
/// scrolling past it.
pub fn pins_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    pins: Vec<Pin<'a, Message, R>>,
    most_height: f32,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    paged_pins_panel(pins, most_height, None, words)
}

/// [`pins_panel`] that sends `more` as its list's end comes within a panel's height of showing, again for each page
/// that lengthens it.
pub(crate) fn paged_pins_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    pins: Vec<Pin<'a, Message, R>>,
    most_height: f32,
    more: Option<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let held = pins.len();
    let heading = phrase(Phrase::PinnedMessages, words);
    let mut lines: Vec<Element<'a, Message, Theme, R>> = vec![
        container(text(heading.as_str().to_string(), TypeStyle::BodyStrong))
            .padding(Padding::from([Space::Xxs.px(), Space::Xs.px()]))
            ._boxed(),
    ];
    if pins.is_empty() {
        let none = coloured(
            words.phrase(Phrase::NoPins).into_owned(),
            TypeStyle::Small,
            TextClass::Content(Content::Secondary),
        );
        lines.push(container(none).padding(Padding::from([0.0, Space::Xs.px()]))._boxed());
    }
    lines.extend(pins.into_iter().map(|pin| pin_card(pin, None, words)));
    if let Some(more) = more {
        let end = iced_widget::sensor(iced_widget::Space::new()).key(held).anticipate(most_height);
        lines.push(end.on_show(move |_| more.clone())._boxed());
    }
    let list = scrollable(Column::with_children(lines).spacing(Space::S.px()))
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()));
    let panel = container(list)
        .padding(Space::M.px())
        .width(Length::Fixed(PINS_PANEL_WIDTH))
        .height(Length::Shrink.max(most_height))
        .class(Surfaced::Panel(Elevation::Overlay, Radius::Xl));
    Labelled::new(panel, Semantics { role: SemanticRole::Dialog, label: heading })._boxed()
}

/// One pin: its author's picture, name and time, Jump and Unpin, then its words; a search's match names its `place`
/// above them.
pub(crate) fn pin_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    pin: Pin<'a, Message, R>,
    place: Option<String>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let small = |said: Phrase, message: Message| {
        Button::new(phrase(said, words)).variant(ButtonVariant::Secondary).small().on_press(message).view::<R>()
    };
    let mut head: Vec<Element<'a, Message, Theme, R>> = vec![
        crate::avatar_pictured(label(&pin.name), pin.avatar, AvatarSize::S, pin.picture),
        line(pin.name.clone(), TypeStyle::SmallStrong)._boxed(),
        line(pin.at, TypeStyle::Stamp).class(TextClass::Content(Content::Secondary)).width(Length::Fill)._boxed(),
        small(Phrase::Jump, pin.jump),
    ];
    head.extend(pin.unpin.map(|unpin| small(Phrase::Unpin, unpin)));
    let place =
        place.map(|place| line(place, TypeStyle::Caption).class(TextClass::Content(Content::Secondary))._boxed());
    let card = Column::with_children(
        place
            .into_iter()
            .chain([Row::with_children(head).spacing(Space::S.px()).align_y(Alignment::Center)._boxed(), pin.body]),
    )
    .spacing(Space::Xs.px());
    let framed = container(card)
        .padding(Padding::from([Space::S.px() + Space::Xxs.px(), Space::M.px()]))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Inset, Radius::M));
    Labelled::new(
        framed,
        Semantics { role: SemanticRole::ListItem, label: label(&format!("{}: {}", pin.name, pin.text)) },
    )
    ._boxed()
}
