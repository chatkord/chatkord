//! A shared screen's card in a call, at whatever size it is given: the host's picture, the wait for its first, or why
//! it is not shown, with whose screen it is at its bottom left.

use crate::typography::text;
use crate::{Button, Face, Icon, IconClass, Labelled, Renderer, Surfaced, Theme, centre_panel, icon, spinner};
use design::{ButtonVariant, IconSize, Label, Role, SemanticRole, Semantics, Space, TILE_LEAST, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget, alignment};
use iced_widget::{Column, Row, Stack, container};
use words::{Phrase, Words};

/// A stream the viewer paused: what is said over its last picture, and Resume.
pub struct Paused<Message> {
    /// What is said over the picture: "Paused".
    pub said: String,
    /// The name of the button that starts it again.
    pub resume: Label,
    /// What Resume sends.
    pub on_resume: Message,
}

/// The veil over a paused stream's last picture, shaped as the reconnecting veil is, with no circle: what it says and
/// an accent Resume.
pub(crate) fn paused_veil<'a, Message: Clone + 'a, R: Renderer + 'a>(
    paused: Paused<Message>,
) -> Element<'a, Message, Theme, R> {
    let resume = Button::new(paused.resume).variant(ButtonVariant::Primary).on_press(paused.on_resume);
    let held = Column::with_children([
        text(paused.said, TypeStyle::BodyStrong).align_x(alignment::Horizontal::Center)._boxed(),
        resume.view::<R>(),
    ])
    .spacing(Space::S.px())
    .align_x(Alignment::Center);
    container(held).padding(Space::M.px()).center(Length::Fill).class(Surfaced::Veil)._boxed()
}

/// What a screen's card holds.
pub enum ScreenCard<'a, Message, R> {
    /// Someone's screen this device cannot show, and why.
    Unshown {
        /// Why, in a sentence.
        said: String,
    },
    /// Someone's screen playing in a window of its own, and what is said in its place.
    Out {
        /// What is said in place of the picture.
        said: String,
    },
    /// This account's own screen going out with nothing of it shown, what is said instead, and Stop sharing.
    Own {
        /// What is said in place of the picture.
        said: String,
        /// What Stop sharing sends.
        stop: Message,
    },
    /// A screen's first picture awaited: the turning circle and what is said.
    Starting {
        /// What is said under the circle.
        said: String,
    },
    /// A screen playing here: the host's picture, and what is said over it, dimmed, while it reconnects.
    Playing {
        /// The host's picture of it.
        picture: Element<'a, Message, Theme, R>,
        /// What is said over the dimmed picture while it reconnects; `None` while it plays.
        reconnecting: Option<String>,
        /// What a right-click sends: the popover of its sound, where it has one.
        menu: Option<Message>,
        /// What a double-click on the picture sends: it fills the whole display, where it can.
        double_click: Option<Message>,
        /// How much it is zoomed, in percent, while zoom is on.
        zoom: Option<u16>,
        /// Why its sound is not heard while the picture plays, in a sentence; `None` while it is, or it has none.
        unheard: Option<String>,
        /// That this computer cannot keep up with it, in a sentence; `None` while it can.
        behind: Option<String>,
        /// That the viewer paused it, over its last picture; `None` while it plays.
        paused: Option<Paused<Message>>,
    },
}

/// A note on a playing stream, after its `glyph`, in a pill like its name's above it: it wraps on a narrow card.
fn note_pill<'a, Message: 'a, R: Renderer + 'a>(glyph: Icon, said: String) -> Element<'a, Message, Theme, R> {
    container(
        Row::with_children([
            icon(glyph, IconSize::S, IconClass::Role(Role::OnScrim))._boxed(),
            text(said, TypeStyle::CaptionStrong)._boxed(),
        ])
        .spacing(Space::Xs.px())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([Space::Xs.px(), Space::S.px()]))
    .class(Surfaced::ScrimPill)
    ._boxed()
}

/// The name of a screen after a screen glyph, in a pill, and how much it is zoomed while zoom is on.
pub(crate) fn name_pill<'a, Message: 'a, R: Renderer + 'a>(
    title: String,
    zoom: Option<u16>,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let zoom = zoom.map(|percent| text(words.percent(percent), TypeStyle::Small)._boxed());
    container(
        Row::with_children(
            [
                icon(Icon::Monitor, IconSize::S, IconClass::Role(Role::OnScrim))._boxed(),
                text(title, TypeStyle::SmallStrong)._boxed(),
            ]
            .into_iter()
            .chain(zoom),
        )
        .spacing(Space::S.px())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([Space::Xs.px(), Space::M.px() - Space::Xxs.px()]))
    .class(Surfaced::ScrimPill)
    ._boxed()
}

/// Whether a card at `size` is too short for a sentence and an action: the strip under a pinned screen.
fn small(size: (f32, f32)) -> bool {
    size.1 < TILE_LEAST / 2.0
}

/// What a card at `size` shows while what it plays is in a window of its own: `said`, or its glyph alone where a
/// sentence has no room.
pub(crate) fn out_face<'a, Message: Clone + 'a, R: Renderer + 'a>(
    said: String,
    size: (f32, f32),
) -> Element<'a, Message, Theme, R> {
    if small(size) {
        let ink = IconClass::Role(Role::OnScrim);
        container(icon(Icon::PictureInPicture, IconSize::M, ink)).center(Length::Fill)._boxed()
    } else {
        centre_panel::<Message, R>(Face::Picture(Icon::PictureInPicture), Some(said), None, Vec::new())
    }
}

/// The card called `title` at `size`.
pub fn screen_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: String,
    size: (f32, f32),
    held: ScreenCard<'a, Message, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let glyph = |at: IconSize| icon(Icon::Monitor, at, IconClass::Role(Role::OnScrim));
    let saying = |face: Face, said: String| centre_panel::<Message, R>(face, Some(said), None, Vec::new());
    let small = small(size);
    let offer = |said: String, action: Label, message: Message, variant: ButtonVariant| {
        let pill = Button::new(action).variant(variant).on_press(message).view::<R>();
        centre_panel::<Message, R>(Face::Picture(Icon::Monitor), Some(said), None, vec![pill])
    };
    let mut layers: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    let mut menu = None;
    let mut zoomed = None;
    let mut notes = Vec::new();
    match held {
        ScreenCard::Out { said } => layers.push(out_face(said, size)),
        ScreenCard::Unshown { .. } | ScreenCard::Own { .. } if small => {
            layers.push(container(glyph(IconSize::M)).center(Length::Fill)._boxed());
        }
        ScreenCard::Unshown { said } => layers.push(saying(Face::Picture(Icon::Monitor), said)),
        ScreenCard::Starting { said } => layers.push(saying(Face::PictureWaiting, said)),
        ScreenCard::Own { said, stop } => {
            layers.push(offer(said, crate::labels::phrase(Phrase::StopSharing, words), stop, ButtonVariant::OnPicture));
        }
        ScreenCard::Playing {
            picture,
            reconnecting,
            menu: right_click,
            double_click,
            zoom,
            unheard,
            behind,
            paused,
        } => {
            menu = right_click;
            zoomed = zoom;
            notes.extend(behind.map(|said| (Icon::Gauge, said)));
            notes.extend(unheard.map(|said| (Icon::VolumeOff, said)));
            let picture = container(picture).width(Length::Fill).height(Length::Fill);
            layers.push(match double_click {
                Some(message) => iced_widget::mouse_area(picture).on_double_click(message)._boxed(),
                None => picture._boxed(),
            });
            if let Some(paused) = paused.filter(|_| reconnecting.is_none()) {
                layers.push(paused_veil(paused));
            }
            if let Some(said) = reconnecting {
                let waiting = Column::with_children([
                    spinner(Role::OnScrim),
                    text(said, TypeStyle::BodyStrong).align_x(alignment::Horizontal::Center)._boxed(),
                ])
                .spacing(Space::S.px())
                .align_x(Alignment::Center);
                layers.push(
                    container(waiting).padding(Space::M.px()).center(Length::Fill).class(Surfaced::Veil)._boxed(),
                );
            }
        }
    }
    let inset = if small { Space::Xs.px() } else { design::PILL_INSET };
    let corner = |part: Element<'a, Message, Theme, R>| {
        container(part)
            .align_x(alignment::Horizontal::Left)
            .align_y(alignment::Vertical::Bottom)
            .padding(inset)
            .width(Length::Fill)
            .height(Length::Fill)
            ._boxed()
    };
    let said = crate::labels::label(&title);
    let pill = if small {
        container(text(title, TypeStyle::CaptionStrong))
            .padding(Padding::from([0.0, Space::S.px() - Space::Xxs.px()]))
            .class(Surfaced::ScrimPill)
            ._boxed()
    } else {
        name_pill(title, zoomed, words)
    };
    let pill = match notes.is_empty() || small {
        true => pill,
        false => Column::with_children(notes.into_iter().map(|(glyph, said)| note_pill(glyph, said)).chain([pill]))
            .spacing(Space::Xs.px())
            ._boxed(),
    };
    layers.push(corner(pill));
    let card = container(Stack::with_children(layers))
        .width(Length::Fixed(size.0))
        .height(Length::Fixed(size.1))
        .clip(true)
        .class(Surfaced::Screen);
    // Keyboard users reach the same popover by the bar's Volume, so the card adds no stop for Tab.
    let card: Element<'a, Message, Theme, R> = match menu {
        Some(message) => iced_widget::mouse_area(card).on_right_press(message)._boxed(),
        None => card._boxed(),
    };
    Labelled::new(card, Semantics { role: SemanticRole::ListItem, label: said })._boxed()
}
