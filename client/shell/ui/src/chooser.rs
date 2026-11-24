//! The composer's chooser, as Discord's: over the composer while a word starts with `@`, `#` or `:`, the matches in
//! groups under their headings, the highlighted one chosen by Enter, the arrows moving it, Escape closing it.

use crate::panel_parts::section_heading;
use crate::reactions::{ReactionEmoji, emoji_in_slot};
use crate::typography::coloured;
use crate::{
    AvatarKind, Held, Icon, IconClass, Labelled, ListRow, Renderer, Surfaced, TextClass, Theme, avatar_pictured, icon,
};
use design::{
    AvatarSize, CHOOSER_GLYPH, CHOOSER_HEIGHT, CHOOSER_LEADING, CHOOSER_PAD, CHOOSER_PICTURE, CHOOSER_WIDTH, Content,
    Elevation, IconSize, Label, MENU_RADIUS, PersonColour, Rgb, Role, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::widget::{Meta, Tree};
use iced_core::{Element, Event, Layout, Length, Padding, Rectangle, Shell, Widget, mouse, renderer};
use iced_widget::{Column, container, scrollable};

/// What stands before a match's name.
#[derive(Clone, Debug, PartialEq)]
pub enum ChooserLeading {
    /// A member's picture: the one they chose as the host holds it, else their initials in their colour.
    Person(PersonColour, Option<Held>),
    /// A role's dot in its colour, or the strong edge's for a role with none.
    Role(Option<Rgb>),
    /// A text channel's mark.
    TextChannel,
    /// A voice channel's mark.
    VoiceChannel,
    /// `@everyone` or `@here`.
    Everyone,
    /// An emoji: unicode, or a server's own with its picture.
    Emoji(ReactionEmoji),
}

/// One match.
#[derive(Clone, Debug)]
pub struct ChooserRow<Message> {
    /// What stands before its name.
    pub leading: ChooserLeading,
    /// Its name as it is inserted: a member's, `@Regulars`, `general`, `:party_owl:`.
    pub name: Label,
    /// What is said of it at the row's end, muted: a username, a category, a server.
    pub secondary: Option<String>,
    /// What choosing it sends, by a press or by Enter while it is highlighted.
    pub choose: Message,
}

/// Matches under one heading.
#[derive(Clone, Debug)]
pub struct ChooserGroup<Message> {
    /// Its heading, already said: "Members", "Emoji matching :par".
    pub heading: Label,
    /// Its matches, in order.
    pub rows: Vec<ChooserRow<Message>>,
}

/// What the chooser's keys send while it is open.
#[derive(Clone, Debug)]
pub struct ChooserKeys<Message> {
    /// The arrow up: the match above is highlighted.
    pub up: Message,
    /// The arrow down: the match below is highlighted.
    pub down: Message,
    /// Enter or Tab: the highlighted match is chosen.
    pub choose: Message,
    /// Escape: the chooser closes, the text kept.
    pub escape: Message,
}

/// The chooser named `label` for a screen reader, its `groups` in order, the `highlighted` match (counted across the
/// groups) drawn chosen. It scrolls where it is taller than its most.
pub fn chooser<'a, Message: Clone + 'a, R: Renderer + 'a>(
    label: Label,
    groups: Vec<ChooserGroup<Message>>,
    highlighted: usize,
) -> Element<'a, Message, Theme, R> {
    let mut lines: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    let mut at = 0;
    for (index, group) in groups.into_iter().enumerate() {
        let top = if index == 0 { Space::Xs.px() } else { Space::S.px() };
        lines.push(
            container(section_heading(&group.heading))
                .padding(Padding { top, bottom: Space::Xs.px(), left: Space::S.px(), right: Space::S.px() })
                ._boxed(),
        );
        for row in group.rows {
            lines.push(match_row(row, at == highlighted));
            at += 1;
        }
    }
    let list = scrollable(Column::with_children(lines).spacing(Space::Xxs.px()))
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .width(Length::Fill);
    let panel = container(list)
        .padding(CHOOSER_PAD)
        .width(Length::Fixed(CHOOSER_WIDTH))
        .class(Surfaced::Panel(Elevation::Overlay, MENU_RADIUS));
    let panel = crate::at_most::at_most(panel, CHOOSER_HEIGHT);
    Labelled::new(panel, Semantics { role: SemanticRole::List, label })._boxed()
}

/// `content` (the composer, with the chooser over it while one is open) where the chooser's keys send `keys` before
/// the field sees them, so Enter chooses rather than sends; with none, every key goes on to the field. Kept around the
/// composer whether a chooser is open or not, so opening one never rebuilds the field and takes its focus.
pub fn chooser_keys<'a, Message: Clone + 'a, R: Renderer + 'a>(
    content: Element<'a, Message, Theme, R>,
    keys: Option<ChooserKeys<Message>>,
) -> Element<'a, Message, Theme, R> {
    Keys { content, keys }._boxed()
}

fn match_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    row: ChooserRow<Message>,
    highlighted: bool,
) -> Element<'a, Message, Theme, R> {
    let mut drawn = ListRow::<Message, R>::new(row.name.clone())
        .leading(
            container(leading::<Message, R>(&row.leading, &row.name)).center_x(Length::Fixed(CHOOSER_LEADING))._boxed(),
        )
        .selected(highlighted)
        .on_press(row.choose);
    if let Some(secondary) = row.secondary {
        drawn = drawn.trailing(coloured(secondary, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed());
    }
    drawn.view()
}

fn leading<'a, Message: 'a, R: Renderer + 'a>(
    leading: &ChooserLeading,
    name: &Label,
) -> Element<'a, Message, Theme, R> {
    let muted = IconClass::Role(Role::Content(Content::Secondary));
    match leading {
        ChooserLeading::Person(colour, picture) => {
            avatar_pictured(name.clone(), AvatarKind::Called(*colour), AvatarSize::S, picture.clone())
        }
        ChooserLeading::Role(colour) => crate::list_row::colour_dot(*colour),
        ChooserLeading::TextChannel => icon(Icon::Hash, IconSize::Bar, muted)._boxed(),
        ChooserLeading::VoiceChannel => icon(Icon::Volume, IconSize::Bar, muted)._boxed(),
        ChooserLeading::Everyone => icon(Icon::AtSign, IconSize::Bar, muted)._boxed(),
        ChooserLeading::Emoji(emoji) => emoji_in_slot(emoji, CHOOSER_GLYPH, CHOOSER_PICTURE),
    }
}

struct Keys<W, Message> {
    content: W,
    keys: Option<ChooserKeys<Message>>,
}

impl<W, Message> Meta for Keys<W, Message> {}

impl<W, Message> Keys<W, Message> {
    /// What a plain arrow, Enter, Tab or Escape press asks of the chooser.
    fn said(&self, event: &Event) -> Option<&Message> {
        let Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(named), modifiers, .. }) = event else {
            return None;
        };
        let keys = self.keys.as_ref()?;
        if modifiers.shift() || modifiers.command() || modifiers.alt() {
            return None;
        }
        match named {
            Named::ArrowUp => Some(&keys.up),
            Named::ArrowDown => Some(&keys.down),
            Named::Enter | Named::Tab => Some(&keys.choose),
            Named::Escape => Some(&keys.escape),
            _ => None,
        }
    }
}

impl<Message, W, R> Widget<Message, Theme, R> for Keys<W, Message>
where
    Message: Clone,
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &R,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(message) = self.said(event) {
            shell.publish(message.clone());
            shell.capture_event();
            return;
        }
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
    }
}
