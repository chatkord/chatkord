//! The centre panel: one look when an area has nothing else to show. A centred column, a round mark (an icon on a
//! tinted circle, a spinner while waiting), a title, an optional line and up to two pills, the first accented.

use crate::labels::phrase;
use crate::{Button, Icon, IconClass, Renderer, Surfaced, TextClass, Theme, coloured, icon, spinner, text};
use design::{
    AccentRole, ButtonVariant, CENTRE_MARK, CENTRE_MARK_SMALL, CENTRE_WIDTH, Content, IconSize, Radius, Role, Space,
    Status, StatusRole, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget, alignment};
use iced_widget::{Column, Row, container};
use screens::{CentrePanel, Mark, Offer};
use words::Words;

/// What a centre panel's mark draws.
#[derive(Clone, Copy)]
pub(crate) enum Face {
    /// A turning circle.
    Waiting,
    /// An icon in the accent, on its tint.
    Accent(Icon),
    /// An icon in a status, on its tint.
    Status(Icon, Status),
    /// An icon on a picture's ground: in the ink for it, on the picture face's circle.
    Picture(Icon),
    /// A turning circle on a picture's ground.
    PictureWaiting,
}

impl Face {
    /// Whether it is drawn on a picture's ground, whose text is not the window's.
    fn on_picture(&self) -> bool {
        matches!(self, Face::Picture(_) | Face::PictureWaiting)
    }
}

impl From<Mark> for Face {
    fn from(mark: Mark) -> Face {
        match mark {
            Mark::Waiting => Face::Waiting,
            Mark::Offline => Face::Accent(Icon::WifiOff),
            Mark::Channels => Face::Accent(Icon::Hash),
            Mark::Voice => Face::Accent(Icon::Volume),
            Mark::Threads => Face::Accent(Icon::MessagesSquare),
            Mark::Conversations => Face::Accent(Icon::MessageSquare),
            Mark::Categories => Face::Accent(Icon::Folder),
            Mark::Failed => Face::Status(Icon::CircleAlert, Status::Danger),
        }
    }
}

/// How much room a centre panel takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Size {
    /// An area's whole room: centred in it.
    Whole,
    /// A column's smaller state: a smaller mark and type, at the column's top.
    Small,
}

/// A panel with `face`, an optional `title` and `body`, and its `actions`, the first of which is accented by its
/// caller; centred in the room it is given.
pub(crate) fn centre_panel<'a, Message: 'a, R: Renderer + 'a>(
    face: Face,
    title: Option<String>,
    body: Option<String>,
    actions: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    sized_panel(Size::Whole, face, title, body, actions)
}

fn sized_panel<'a, Message: 'a, R: Renderer + 'a>(
    size: Size,
    face: Face,
    title: Option<String>,
    body: Option<String>,
    actions: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let (mark_side, glyph_size, title_style, body_style) = match size {
        Size::Whole => (CENTRE_MARK, IconSize::L, TypeStyle::Title, TypeStyle::Body),
        Size::Small => (CENTRE_MARK_SMALL, IconSize::M, TypeStyle::BodyStrong, TypeStyle::Small),
    };
    let (fill, ink) = match &face {
        Face::Waiting | Face::Accent(_) => (Role::Accent(AccentRole::Tint), Role::Accent(AccentRole::Text)),
        Face::Status(_, status) => (Role::Status(*status, StatusRole::Tint), Role::Status(*status, StatusRole::Text)),
        Face::Picture(_) | Face::PictureWaiting => (Role::ScrimHover, Role::OnScrim),
    };
    let on_picture = face.on_picture();
    let inside: Element<'a, Message, Theme, R> = match face {
        Face::Waiting | Face::PictureWaiting => spinner(ink),
        Face::Accent(glyph) | Face::Status(glyph, _) | Face::Picture(glyph) => {
            icon(glyph, glyph_size, IconClass::Role(ink))._boxed()
        }
    };
    let mark = container(inside).center(Length::Fixed(mark_side)).class(Surfaced::Fill {
        fill,
        text: ink,
        radius: Radius::Full,
    });
    let mut lines: Vec<Element<'a, Message, Theme, R>> = vec![mark._boxed()];
    lines.extend(title.map(|title| text(title, title_style).align_x(alignment::Horizontal::Center)._boxed()));
    lines.extend(body.map(|body| {
        let class = if on_picture { TextClass::Inherit } else { TextClass::Content(Content::Secondary) };
        coloured(body, body_style, class).align_x(alignment::Horizontal::Center)._boxed()
    }));
    if !actions.is_empty() {
        lines.push(Row::with_children(actions).spacing(Space::S.px())._boxed());
    }
    match size {
        Size::Whole => {
            let column = Column::with_children(lines)
                .spacing(Space::M.px())
                .align_x(Alignment::Center)
                .width(Length::Fill.max(CENTRE_WIDTH));
            container(column).center(Length::Fill)._boxed()
        }
        Size::Small => Column::with_children(lines)
            .spacing(Space::S.px() + Space::Xxs.px())
            .align_x(Alignment::Center)
            .padding(Padding::from([Space::L.px(), Space::S.px()]))
            .width(Length::Fill)
            ._boxed(),
    }
}

/// `shown` filling the room it is given, its offers sending what `press` makes of them.
pub fn centre<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &CentrePanel,
    press: impl Fn(Offer) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    offered(Size::Whole, shown, press, words)
}

/// `shown` as a column's smaller state, at the column's top, its offers sending what `press` makes of them.
pub fn centre_small<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &CentrePanel,
    press: impl Fn(Offer) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    offered(Size::Small, shown, press, words)
}

fn offered<'a, Message: Clone + 'a, R: Renderer + 'a>(
    size: Size,
    shown: &CentrePanel,
    press: impl Fn(Offer) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let actions = shown
        .offers
        .iter()
        .enumerate()
        .map(|(at, offer)| {
            let variant = if at == 0 { ButtonVariant::Primary } else { ButtonVariant::Secondary };
            Button::new(phrase(offer.phrase(), words)).variant(variant).on_press(press(*offer)).view::<R>()
        })
        .collect();
    sized_panel(
        size,
        shown.mark.into(),
        shown.title.as_ref().map(|title| words.say(title)),
        shown.body.as_ref().map(|body| words.say(body)),
        actions,
    )
}
