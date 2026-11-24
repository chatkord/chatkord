//! A reply, Discord's: the bar over the composer while one is written (whom it answers, whether they are mentioned, the
//! way out), and the quoted line over a reply's row, a connector from its picture up to the message it answers.

use crate::labels::{label, phrase};
use crate::typography::{coloured, line};
use crate::{AvatarKind, Button, Held, Icon, IconClass, Renderer, Surfaced, TextClass, Theme, avatar_pictured, icon};
use design::{
    AccentRole, AvatarSize, Border, ButtonVariant, Content, IconSize, PersonColour, QUOTED_INDENT, QUOTED_LINE_HEIGHT,
    REPLY_BAR_HEIGHT, REPLY_CONNECTOR, Role, SMALL_TARGET, SemanticRole, Semantics, Space, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Point, Rectangle, Widget, mouse};
use iced_widget::canvas::{Canvas, Frame, Geometry, LineCap, Path, Program, Stroke};
use iced_widget::{Column, Row, container};
use words::{Phrase, Words};

/// The reply being written, as its bar shows it.
pub struct Replying<Message> {
    /// The name of whom it answers.
    pub name: String,
    /// Whether they are mentioned, Discord's "@ On".
    pub mention: bool,
    /// What a press on the mention's toggle sends: the other setting.
    pub toggle: Message,
    /// What the close sends: no reply.
    pub cancel: Message,
}

/// The bar over the composer while `replying` is written: "Replying to" its name, the mention's toggle and the close.
pub fn reply_bar<'a, Message: Clone + 'a, R: Renderer + 'a>(
    replying: Replying<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let to = Row::with_children([
        coloured(
            words.phrase(Phrase::ReplyingTo).into_owned(),
            TypeStyle::Small,
            TextClass::Content(Content::Secondary),
        )
        ._boxed(),
        line(replying.name, TypeStyle::SmallStrong).class(TextClass::Content(Content::Primary))._boxed(),
    ])
    .spacing(Space::Xs.px())
    .width(Length::Fill);
    let close = Button::new(phrase(Phrase::CancelReply, words))
        .icon_only(Icon::Close)
        .variant(ButtonVariant::Quiet)
        .small()
        .on_press(replying.cancel)
        .view::<R>();
    let row = Row::with_children([to._boxed(), mention_toggle(replying.mention, replying.toggle, words), close])
        .spacing(Space::Xs.px())
        .align_y(Alignment::Center);
    let bar = container(row)
        .padding(Padding { left: Space::M.px(), right: Space::Xs.px(), ..Padding::ZERO })
        .center_y(Length::Fixed(REPLY_BAR_HEIGHT))
        .width(Length::Fill)
        .class(Surfaced::On(Surface::Raised));
    Column::with_children([bar._boxed(), crate::panel_parts::hairline()])._boxed()
}

/// "@ On" or "@ Off": whether the reply mentions its author, a switch that says which.
fn mention_toggle<'a, Message: Clone + 'a, R: Renderer + 'a>(
    on: bool,
    toggle: Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let (said, class, tone) = if on {
        (Phrase::MentionOn, TextClass::Accent, Role::Accent(AccentRole::Text))
    } else {
        (Phrase::MentionOff, TextClass::Content(Content::Secondary), Role::Content(Content::Secondary))
    };
    let face = Row::with_children([
        icon(Icon::AtSign, IconSize::S, IconClass::Role(tone))._boxed(),
        coloured(words.phrase(said).into_owned(), TypeStyle::CaptionStrong, class)._boxed(),
    ])
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center);
    let inner = iced_widget::button(container(face).center_y(Length::Fixed(SMALL_TARGET)))
        .class(crate::ButtonClass::Button { variant: ButtonVariant::Ghost, loading: false })
        .padding(Padding::from([0.0, Space::S.px()]))
        .on_press(toggle.clone());
    let semantics = Semantics { role: SemanticRole::Switch { on }, label: phrase(Phrase::MentionAuthor, words) };
    crate::button::actionable(inner, semantics, Some(toggle))
}

/// What a reply quotes.
pub enum Quoted {
    /// The message it answers, still there.
    Live {
        /// Its author's name.
        name: String,
        /// Its author's colour.
        colour: PersonColour,
        /// The picture its author chose, as the host holds it; none for their initials.
        picture: Option<Held>,
        /// Its words on one line, markdown's markers left out.
        snippet: String,
    },
    /// The message it answered was deleted.
    Deleted,
}

/// The line over a reply's row.
pub struct QuotedReply<Message> {
    /// What it quotes.
    pub quoted: Quoted,
    /// What a press on it sends: the list moves to the message it quotes.
    pub jump: Message,
}

/// The quoted line over a reply's row: the connector, then the quoted author's picture, name and words, or that the
/// message is gone; a press on it sends its jump.
pub fn quoted_reply<'a, Message: Clone + 'a, R: Renderer + 'a>(
    reply: QuotedReply<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let connector = container(
        Canvas::<Connecting, Message, Theme, R>::new(Connecting)
            .width(Length::Fixed(REPLY_CONNECTOR.width))
            .height(Length::Fixed(REPLY_CONNECTOR.height)),
    )
    .align_bottom(Length::Fill);
    let mut cells: Vec<Element<'a, Message, Theme, R>> = vec![connector._boxed()];
    let said = match reply.quoted {
        Quoted::Live { name, colour, picture, snippet } => {
            cells.push(avatar_pictured(label(&name), AvatarKind::Called(colour), AvatarSize::Mark, picture));
            cells.push(line(name.clone(), TypeStyle::SmallStrong).class(TextClass::Content(Content::Primary))._boxed());
            cells.push(line(snippet.clone(), TypeStyle::Small).class(TextClass::Content(Content::Secondary))._boxed());
            format!("{name}: {snippet}")
        }
        Quoted::Deleted => {
            let gone = words.phrase(Phrase::OriginalDeleted).into_owned();
            let italic =
                iced_core::Font { style: iced_core::font::Style::Italic, ..crate::fonts::font(TypeStyle::Small) };
            cells.push(
                line(gone.clone(), TypeStyle::Small)
                    .font(italic)
                    .class(TextClass::Content(Content::Secondary))
                    ._boxed(),
            );
            gone
        }
    };
    let shown = Row::with_children(cells).spacing(Space::Xs.px()).align_y(Alignment::Center);
    let shown = container(shown)
        .padding(Padding { left: QUOTED_INDENT, ..Padding::ZERO })
        .height(Length::Fixed(QUOTED_LINE_HEIGHT))
        .width(Length::Fill);
    let inner = iced_widget::button(shown)
        .class(crate::ButtonClass::Row { selected: false, tone: crate::RowTone::Quiet })
        .padding(0)
        .on_press(reply.jump.clone());
    let semantics = Semantics { role: SemanticRole::Link, label: label(&said) };
    crate::button::actionable(inner, semantics, Some(reply.jump))
}

/// The connector's stroke, in the strong edge's colour.
struct Connecting;

impl<Message, R: Renderer> Program<Message, Theme, R> for Connecting {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &R,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<R>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let shape = REPLY_CONNECTOR;
        let foot = shape.inset;
        let path = Path::new(|path| {
            path.move_to(Point::new(foot, shape.height));
            path.line_to(Point::new(foot, shape.across + shape.turn));
            path.arc_to(Point::new(foot, shape.across), Point::new(foot + shape.turn, shape.across), shape.turn);
            path.line_to(Point::new(shape.width, shape.across));
        });
        let stroke = Stroke::default()
            .with_color(theme.color(Role::Border(Border::Strong)))
            .with_width(shape.stroke)
            .with_line_cap(LineCap::Butt);
        frame.stroke(&path, stroke);
        vec![frame.into_geometry()]
    }
}
