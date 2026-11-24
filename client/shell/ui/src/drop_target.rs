//! What a channel shows while files are dragged over it: the channel shaded, and a card with a dashed accent edge
//! that says a drop attaches them, where they go and how large each may be.

use crate::typography::coloured;
use crate::{Icon, IconClass, Labelled, Renderer, Surfaced, TextClass, Theme, icon, text};
use design::{
    AccentRole, Content, DASH, DROP_CARD_WIDTH, DROP_EDGE, DROP_TILE, IconSize, Radius, Role, SemanticRole, Semantics,
    Space, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Point, Rectangle, Size, Widget, mouse};
use iced_widget::canvas::{Canvas, Frame, Geometry, LineDash, Path, Program, Stroke};
use iced_widget::{Column, container, stack};
use words::{Phrase, Words};

/// `content`, the channel, with the drop's card over it while files are dragged there: `note` says where they go
/// and how large each may be; none while nothing is dragged.
pub fn drop_over<'a, Message: 'a, R: Renderer + 'a>(
    content: Element<'a, Message, Theme, R>,
    note: Option<String>,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    // The content stays the stack's first child whether the card shows, so its fields keep their state, and focus.
    let Some(note) = note else { return stack([content])._boxed() };
    let title = words.phrase(Phrase::DropToAttach).into_owned();
    let tile = container(icon(Icon::Paperclip, IconSize::L, IconClass::Role(Role::Accent(AccentRole::Text))))
        .center(Length::Fixed(DROP_TILE))
        .class(Surfaced::Fill {
            fill: Role::Accent(AccentRole::Tint),
            text: Role::Accent(AccentRole::Text),
            radius: Radius::Xxl,
        });
    let said = Column::with_children([
        tile._boxed(),
        text(title.clone(), TypeStyle::Title)._boxed(),
        coloured(note, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::S.px() + Space::Xxs.px())
    .width(Length::Fill)
    .align_x(Alignment::Center);
    let card =
        container(said).padding(Padding::from([Space::Xxl.px(), Space::Xl.px()])).width(Length::Fixed(DROP_CARD_WIDTH));
    let edged = stack([card._boxed()]).push_under(painted(Part::Card));
    let card = Labelled::new(edged, Semantics { role: SemanticRole::Status, label: crate::labels::label(&title) });
    let layer =
        stack([container(card).center(Length::Fill).padding(Space::L.px())._boxed()]).push_under(painted(Part::Shade));
    stack([content, layer._boxed()])._boxed()
}

/// What the drop paints that a container cannot: the shade over the channel, and the card's ground with its dashed
/// edge (iced draws no dashed border).
#[derive(Clone, Copy)]
enum Part {
    Shade,
    Card,
}

fn painted<'a, Message: 'a, R: Renderer + 'a>(part: Part) -> Element<'a, Message, Theme, R> {
    Canvas::<Part, Message, Theme, R>::new(part).width(Length::Fill).height(Length::Fill)._boxed()
}

impl<Message, R: Renderer> Program<Message, Theme, R> for Part {
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
        match self {
            Part::Shade => frame.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                crate::color::translucent(theme.palette().dialog_scrim()),
            ),
            Part::Card => {
                // The stroke's middle half its width inside, so the whole edge shows.
                let inset = DROP_EDGE / 2.0;
                let size = Size::new(bounds.width - DROP_EDGE, bounds.height - DROP_EDGE);
                let card = Path::rounded_rectangle(Point::new(inset, inset), size, Radius::Xxl.px().into());
                frame.fill(&card, theme.color(Role::Surface(Surface::Base)));
                let stroke =
                    Stroke::default().with_color(theme.color(Role::Accent(AccentRole::Fill))).with_width(DROP_EDGE);
                frame.stroke(&card, Stroke { line_dash: LineDash { segments: &DASH, offset: 0 }, ..stroke });
            }
        }
        vec![frame.into_geometry()]
    }
}
