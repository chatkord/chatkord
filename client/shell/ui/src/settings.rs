//! Settings, a server's or this account's: the list of pages on the left under their name, in groups, the page open
//! beside it, and Close at the end of the page's heading line, which leaves the settings. The heading line stays
//! where it is and only what is under it scrolls.

use crate::choice::pill;
use crate::panel_parts::section_heading;
use crate::{Button, Icon, Labelled, Renderer, Surfaced, Theme, text};
use design::{
    ButtonVariant, HIT_TARGET, Label, NAV_ITEM_HEIGHT, PillKind, ROW_WORDS_LEAST, Radius, SETTINGS_NAV_WIDTH,
    SemanticRole, Semantics, Space, Surface, TypeStyle,
};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container, scrollable};
use screens::frame::SettingsShown;

/// A page of the settings as the list shows it: its name, whether it is open, and what opening it sends.
pub struct SettingsPage<Message> {
    /// Its name in the list.
    pub label: Label,
    /// The page open now, drawn chosen.
    pub current: bool,
    /// What opening it sends.
    pub message: Message,
}

/// Pages that belong together, under a heading of their own when they have one.
pub struct SettingsGroup<Message> {
    /// Its heading, or none where the group stands alone.
    pub heading: Option<Label>,
    /// Its pages, in the order listed.
    pub pages: Vec<SettingsPage<Message>>,
}

/// How a settings page is left, and whether its list is beside it.
pub struct SettingsWays<Message> {
    /// Close, which Escape sends too: it leaves the settings.
    pub close: (Label, Message),
    /// Whether the list is beside the page or, folded, which of the two shows.
    pub shown: SettingsShown,
    /// Back to the list, above a page shown alone.
    pub back: (Label, Message),
}

/// What heads an open page, on Close's line.
pub enum Heading<Message> {
    /// Its name.
    Titled(Label),
    /// Back to the list of what the page holds, in place of its name: a role opened alone goes back to the roles.
    /// Settings' own Back to their list is not drawn over it.
    Back(Label, Message),
}

impl<Message> From<Label> for Heading<Message> {
    fn from(name: Label) -> Self {
        Heading::Titled(name)
    }
}

/// The settings named `title`: its pages in `groups`, and the open one: its `heading`, main `action` and Close on one
/// line, then its `content`; `ways` says how it is left and whether the list is beside
/// it.
pub fn settings_frame<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: &Label,
    groups: Vec<SettingsGroup<Message>>,
    heading: impl Into<Heading<Message>>,
    action: Option<Element<'a, Message, Theme, R>>,
    content: Element<'a, Message, Theme, R>,
    ways: SettingsWays<Message>,
) -> Element<'a, Message, Theme, R> {
    let SettingsWays { close, shown, back } = ways;
    let heading = heading.into();
    let name = container(text(title.as_str().to_string(), TypeStyle::BodyStrong)).padding(Padding {
        top: Space::Xs.px(),
        bottom: Space::M.px(),
        left: Space::S.px(),
        right: Space::S.px(),
    });
    let mut items: Vec<Element<'a, Message, Theme, R>> = vec![name._boxed()];
    for (index, group) in groups.into_iter().enumerate() {
        let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
        if let Some(heading) = &group.heading {
            // The first heading sits under the title, which already keeps its room below.
            let top = if index == 0 { Space::S.px() } else { Space::L.px() };
            rows.push(
                container(section_heading(heading))
                    .padding(Padding { top, bottom: Space::Xs.px(), left: Space::S.px(), right: Space::S.px() })
                    ._boxed(),
            );
        }
        rows.extend(group.pages.into_iter().map(|page| {
            pill(PillKind::Page, page.label, page.current, page.message, Length::Fill, Space::M.px(), NAV_ITEM_HEIGHT)
        }));
        let named = group.heading.unwrap_or_else(|| title.clone());
        items.push(
            Labelled::new(
                Column::with_children(rows).spacing(Space::Xxs.px()),
                Semantics { role: SemanticRole::RadioGroup, label: named },
            )
            ._boxed(),
        );
    }
    let list = Column::with_children(items).spacing(Space::Xxs.px());
    let nav =
        container(scrollable(list).direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar())))
            .padding(Padding::from([Space::L.px(), Space::S.px()]))
            .width(if shown == SettingsShown::List { Length::Fill } else { Length::Fixed(SETTINGS_NAV_WIDTH) })
            .height(Length::Fill)
            .class(Surfaced::On(Surface::Raised));
    if shown == SettingsShown::List {
        return nav._boxed();
    }
    let rule = container(iced_widget::Space::new())
        .width(Length::Fixed(design::BORDER_WIDTH))
        .height(Length::Fill)
        .class(Surfaced::Fill {
            fill: design::Role::Border(design::Border::Subtle),
            text: design::Role::Content(design::Content::Primary),
            radius: Radius::None,
        });
    // Alone, a page goes back to the list from above its heading, and its gutters tighten.
    let (back, gutter) = match shown {
        SettingsShown::Page if matches!(heading, Heading::Titled(_)) => {
            let (said, message) = back;
            let button =
                Button::new(said).icon(Icon::ChevronLeft).variant(ButtonVariant::Ghost).compact().on_press(message);
            (Some(button.view::<R>()), shown.gutter())
        }
        SettingsShown::Page | SettingsShown::Beside | SettingsShown::List => (None, shown.gutter()),
    };
    let edge = Space::Xl.px() + Space::Xs.px();
    let (said_close, message) = close;
    let shut = Button::new(said_close).icon(Icon::Close).variant(ButtonVariant::Secondary).compact().on_press(message);
    let head = Column::with_children(back.into_iter().chain([heading_line(heading, action, shut.view::<R>())]))
        .spacing(Space::L.px());
    let head = container(head).padding(Padding { top: edge, bottom: 0.0, left: gutter, right: gutter });
    let body = scrollable(container(content).padding(Padding { top: 0.0, bottom: edge, left: gutter, right: gutter }))
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .width(Length::Fill)
        .height(Length::Fill);
    let page = Column::with_children([head._boxed(), body._boxed()])
        .spacing(Space::L.px())
        .width(Length::Fill)
        .height(Length::Fill);
    match shown {
        SettingsShown::Page => page._boxed(),
        SettingsShown::Beside | SettingsShown::List => {
            Row::with_children([nav._boxed(), rule._boxed(), page._boxed()]).height(Length::Fill)._boxed()
        }
    }
}

/// A page's heading, its main action and Close on one line as tall as a compact button, so all three share a middle
/// and Close ends where the page's content does; where the heading would have too little room, the action goes on a
/// line of its own under it and Close stays on the heading's.
fn heading_line<'a, Message: Clone + 'a, R: Renderer + 'a>(
    heading: Heading<Message>,
    action: Option<Element<'a, Message, Theme, R>>,
    close: Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let title = match heading {
        Heading::Titled(heading) => Labelled::new(
            text(heading.as_str().to_string(), TypeStyle::PageTitle).width(Length::Fill),
            Semantics { role: SemanticRole::Heading, label: heading },
        )
        ._boxed(),
        Heading::Back(said, message) => {
            let button =
                Button::new(said).icon(Icon::ChevronLeft).variant(ButtonVariant::Ghost).compact().on_press(message);
            Row::with_children([button.view::<R>(), iced_widget::Space::new().width(Length::Fill)._boxed()])._boxed()
        }
    };
    let title = container(title).height(Length::Fixed(HIT_TARGET)).align_y(iced_core::Alignment::Center)._boxed();
    let leading = match action {
        Some(action) => crate::beside(title, action, ROW_WORDS_LEAST, Space::S.px(), 0.0),
        None => title,
    };
    Row::with_children([leading, close]).spacing(Space::S.px())._boxed()
}
