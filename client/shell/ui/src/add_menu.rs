//! Adding a role or a member to a channel's list: a pill with a dashed edge and a "+", and the menu it opens hung
//! from it, the roles it offers with their dots, then a search of the members with their pictures, or why none is
//! offered. Whether it is open, the search and what each pick does are the screen's.

use crate::layer::{Layered, Placement};
use crate::menu::{MenuKind, menu_panel};
use crate::whom::Naming;
use crate::{Button, Icon, ListRow, Renderer, TextClass, Theme, coloured};
use design::{Border, ButtonVariant, Content, DASH, Label, MENU_WIDTH, Popup, Role, Space, TypeStyle};
use iced_core::{Element, Length, Padding, Rectangle, Widget, mouse};
use iced_widget::canvas::{Canvas, Frame, Geometry, LineDash, Path, Program, Stroke};
use iced_widget::{container, stack};
use screens::Whom;
use screens::channel_settings::{AddMenu, Offer};
use words::{Phrase, Words};

/// What the add button and its menu send.
#[derive(Clone, Debug)]
pub(crate) enum AddSaid {
    /// The button pressed, or the open menu left: it opens, or closes.
    Toggle,
    /// The members' search, as typed.
    Query(String),
    /// A role or member picked.
    Pick(Whom),
}

/// Where the menu hangs from its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hang {
    /// From its start, the menu's width held for it: a button on a line of its own.
    Start,
    /// Its end at the button's end: a button at the end of a run of others.
    End,
}

/// The button `said`, with `menu` hung from it as `hang` says while it is open; `query` is the members' search as
/// typed, and `on` makes what each does a message.
pub(crate) fn add_menu<'a, Message: Clone + 'a, R: Renderer + 'a>(
    said: Label,
    menu: Option<AddMenu>,
    query: &'a str,
    hang: Hang,
    naming: &Naming<'_>,
    on: impl Fn(AddSaid) -> Message + Copy + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let open = menu.is_some();
    let button = add_button(said, open, on(AddSaid::Toggle));
    let layer = menu.map(|menu| panel(menu, query, naming, on, words));
    let gap = Space::Xs.px() + Space::Xxs.px();
    match hang {
        Hang::Start => {
            let held = container(button).width(Length::Fill.max(MENU_WIDTH));
            Layered::new(held._boxed(), layer, Placement::Below { gap }, on(AddSaid::Toggle), true)._boxed()
        }
        Hang::End => crate::menu::menu_hung(button, layer, on(AddSaid::Toggle)),
    }
}

/// A pill with a dashed edge, a "+" and `said`, that opens a menu.
fn add_button<'a, Message: Clone + 'a, R: Renderer + 'a>(
    said: Label,
    open: bool,
    toggle: Message,
) -> Element<'a, Message, Theme, R> {
    let button = Button::new(said)
        .icon(Icon::Plus)
        .variant(ButtonVariant::Quiet)
        .compact()
        .opens(Popup::Menu, open)
        .on_press(toggle);
    let edge = Canvas::<DashedEdge, Message, Theme, R>::new(DashedEdge).width(Length::Fill).height(Length::Fill);
    stack([button.view::<R>()]).push_under(edge._boxed())._boxed()
}

/// A dashed pill round the room it is given, in the strong edge's colour (iced draws no dashed border).
struct DashedEdge;

impl<Message, R: Renderer> Program<Message, Theme, R> for DashedEdge {
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
        // The stroke's middle half its width inside, so the whole edge shows.
        let inset = design::BORDER_WIDTH / 2.0;
        let size = iced_core::Size::new(bounds.width - design::BORDER_WIDTH, bounds.height - design::BORDER_WIDTH);
        let radius = size.height / 2.0;
        let edge = Path::rounded_rectangle(iced_core::Point::new(inset, inset), size, radius.into());
        let stroke =
            Stroke::default().with_color(theme.color(Role::Border(Border::Strong))).with_width(design::BORDER_WIDTH);
        frame.stroke(&edge, Stroke { line_dash: LineDash { segments: &DASH, offset: 0 }, ..stroke });
        vec![frame.into_geometry()]
    }
}

/// The menu: the roles it offers, then the members' search and the members it finds, or why none.
fn panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    menu: AddMenu,
    query: &'a str,
    naming: &Naming<'_>,
    on: impl Fn(AddSaid) -> Message + Copy + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let offer = |offer: Offer| -> Element<'a, Message, Theme, R> {
        ListRow::<Message, R>::new(naming.name(&offer.name))
            .menu_item()
            .leading(naming.mark(offer.look, &offer.name))
            .on_press(on(AddSaid::Pick(offer.whom)))
            .view()
    };
    let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    if !menu.roles.is_empty() {
        rows.push(heading(Phrase::Roles, words));
        rows.extend(menu.roles.into_iter().map(offer));
        rows.push(
            container(crate::panel_parts::hairline()).padding(Padding::from([Space::Xs.px(), Space::S.px()]))._boxed(),
        );
    }
    rows.push(heading(Phrase::Members, words));
    let search = crate::search_field(
        crate::labels::phrase(Phrase::FindAMember, words),
        words.phrase(Phrase::FindAMember).as_ref(),
        crate::labels::phrase(Phrase::ClearSearch, words),
        query,
        move |typed| on(AddSaid::Query(typed)),
        on(AddSaid::Query(String::new())),
    );
    rows.push(container(search).padding(Padding::from([Space::Xxs.px(), Space::Xs.px() + Space::Xxs.px()]))._boxed());
    rows.extend(menu.members.into_iter().map(offer));
    rows.extend(menu.none.map(|none| {
        let said = coloured(words.say(&none), TypeStyle::Small, TextClass::Content(Content::Secondary));
        container(said).padding(Padding::from([Space::S.px(), Space::S.px() + Space::Xxs.px()]))._boxed()
    }));
    menu_panel(MenuKind::Actions, crate::labels::phrase(Phrase::AddToChannel, words), rows, Length::Fixed(MENU_WIDTH))
}

/// A section's name in the menu, quiet and small.
fn heading<'a, Message: 'a, R: Renderer + 'a>(said: Phrase, words: &Words) -> Element<'a, Message, Theme, R> {
    let said =
        coloured(words.phrase(said).to_string(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary));
    container(said)
        .padding(Padding {
            top: Space::Xs.px() + Space::Xxs.px(),
            bottom: Space::Xs.px(),
            left: Space::S.px() + Space::Xxs.px(),
            right: Space::S.px() + Space::Xxs.px(),
        })
        ._boxed()
}
