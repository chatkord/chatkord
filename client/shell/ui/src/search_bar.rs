//! The window's top bar and the search it holds. The bar runs across the pane and the content: empty at its start, the
//! search's field-shaped button in its middle that opens the search layer, and the controls of the view at its end.
//! Where the pane folds away the bar goes with it, and each header ends with the same controls and a magnifier that
//! opens the same layer.

use crate::typography::line;
use crate::{Button, Icon, IconClass, Renderer, Surfaced, TextClass, Theme, icon};
use design::{
    ButtonVariant, Content, IconSize, Label, Popup, Radius, Role, SEARCH_FIELD_HEIGHT, SEARCH_FIELD_WIDTH,
    SEARCH_HINT_HEIGHT, SERVER_BAR_HEIGHT, SemanticRole, Semantics, Space, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, button, container};

/// The search's field-shaped button: `name` ("Search Night Owls") ending in the key `hint`, which sends `press`; `open`
/// while the layer is.
pub fn search_pill<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    hint: String,
    open: bool,
    press: Message,
) -> Element<'a, Message, Theme, R> {
    let secondary = TextClass::Content(Content::Secondary);
    let key = container(line(hint, TypeStyle::MonoCaption).class(secondary))
        .padding(Padding::from([0.0, Space::S.px() - Space::Xxs.px()]))
        .center_y(Length::Fixed(SEARCH_HINT_HEIGHT))
        .class(Surfaced::Framed(Surface::Overlay, Radius::Full));
    let inside = Row::with_children([
        icon(Icon::Search, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
        line(name.as_str().to_string(), TypeStyle::Small).class(secondary).width(Length::Fill)._boxed(),
        key._boxed(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let pill = container(inside)
        .padding(Padding { left: Space::S.px() + Space::Xxs.px(), right: Space::Xs.px(), ..Padding::ZERO })
        .center_y(Length::Fixed(SEARCH_FIELD_HEIGHT))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Raised, Radius::Full));
    let inner = button(pill)
        .class(crate::button::ButtonClass::SearchPill)
        .padding(0)
        .width(Length::Fill)
        .height(Length::Fixed(SEARCH_FIELD_HEIGHT))
        .on_press(press.clone());
    let semantics = Semantics { role: SemanticRole::Opens { layer: Popup::Dialog, open }, label: name };
    crate::button::actionable(inner, semantics, Some(press))
}

/// The top bar over the pane and the content: `search` in its middle where the server offers one, `end` ending the bar.
pub fn top_bar<'a, Message: 'a, R: Renderer + 'a>(
    search: Option<Element<'a, Message, Theme, R>>,
    end: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let start = iced_widget::Space::new().width(Length::Fill);
    let end =
        container(Row::with_children(end).spacing(Space::Xs.px()).align_y(Alignment::Center)).align_right(Length::Fill);
    let middle = search.map(|field| container(field).width(Length::Fixed(SEARCH_FIELD_WIDTH))._boxed());
    let row = Row::with_children(std::iter::once(start._boxed()).chain(middle).chain([end._boxed()]))
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, crate::bar_controls::EDGE]))
        .height(Length::Fixed(SERVER_BAR_HEIGHT));
    let bar = container(row).class(Surfaced::On(Surface::Chrome));
    Column::with_children([bar._boxed(), crate::panel_parts::hairline()])._boxed()
}

/// The magnifier of a content header where the bar is folded away, named `name`, sending `press`.
pub fn search_magnifier<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    open: bool,
    press: Message,
) -> Element<'a, Message, Theme, R> {
    Button::new(name)
        .icon_only(Icon::Search)
        .variant(ButtonVariant::Ghost)
        .compact()
        .opens(Popup::Dialog, open)
        .on_press(press)
        .view::<R>()
}
