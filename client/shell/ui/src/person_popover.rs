//! What opens on a person wherever they show, or on the sound of their shared screen: their name, their status and the
//! roles they hold, on the account's own its status menu, how loud they play on this device, and for an administrator
//! what they may do to them, as the canvas's popover has it.

use crate::layer::on_panel;
use crate::menu::{MenuEntry, entry_rows};
use crate::panel_parts::{hairline, section_heading};
use crate::role_badge::role_badge;
use crate::status_mark::status_mark_dot;
use crate::volume_row::{VolumeWords, volume_row};
use crate::{Button, Labelled, ListRow, Renderer, TextClass, Theme, coloured, text};
use design::{
    BORDER_WIDTH, ButtonVariant, Content, INFO_PANEL_WIDTH, Label, MENU_RADIUS, Rgb, SemanticRole, Semantics, Space,
    Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::StatusDot;

/// What the popover says, in the window's words.
pub struct PersonWords {
    /// Their name, or whose screen's sound it is, over the rest and the popover's name for a screen reader.
    pub name: Label,
    /// The line over the volume row.
    pub volume: Label,
    /// The heading over what an administrator may do.
    pub as_admin: Label,
    /// What a screen reader calls the list of their roles.
    pub roles: Label,
}

/// A person's status, as their popover says it under their name.
pub struct Presence {
    /// Its dot; none where the account is not in.
    pub dot: Option<StatusDot>,
    /// The word for it.
    pub said: String,
}

/// The account's status menu, on its own popover.
pub struct StatusChoices<Message> {
    /// What a screen reader calls the menu.
    pub label: Label,
    /// Online, Away, Do not disturb, Invisible, in order.
    pub rows: Vec<StatusChoice<Message>>,
}

/// One status of the menu, with its durations under it while it is being picked.
pub struct StatusChoice<Message> {
    /// Its word.
    pub label: Label,
    /// Its dot.
    pub dot: Option<StatusDot>,
    /// It is the account's status now: checked.
    pub chosen: bool,
    /// Beside it while it is chosen and has an end: when it ends.
    pub until: Option<String>,
    /// Its durations are open under it: it stands out.
    pub picking: bool,
    /// What a press sends.
    pub message: Message,
    /// The durations under it, each with what its press sends; none while it is not being picked.
    pub durations: Vec<(Label, Message)>,
}

/// A role a person holds, as their popover shows it.
pub struct HeldRole {
    /// Its name.
    pub name: String,
    /// Its colour; none for a role with no colour, whose badge has no dot.
    pub colour: Option<Rgb>,
}

/// The popover of an app's post: the `name` it posted under with the badge that says it is an app, and under them
/// `about`, who carries the post.
pub fn app_popover<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    badge: &str,
    about: String,
) -> Element<'a, Message, Theme, R> {
    let head = Row::with_children([
        text(name.as_str().to_string(), TypeStyle::BodyStrong).width(Length::Shrink)._boxed(),
        role_badge(badge, None),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let lines =
        vec![head._boxed(), coloured(about, TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed()];
    let column = Column::with_children(lines)
        .spacing(Space::S.px() + Space::Xxs.px())
        .width(Length::Fixed(INFO_PANEL_WIDTH - 2.0 * (Space::L.px() + BORDER_WIDTH)));
    Labelled::new(
        on_panel(container(column).padding(BORDER_WIDTH)._boxed(), Space::L, MENU_RADIUS),
        Semantics { role: SemanticRole::Dialog, label: name },
    )
    ._boxed()
}

/// How loud a sound plays here, and what moving its row sends.
pub struct Heard<'a, Message> {
    /// The row's own words.
    pub words: VolumeWords,
    /// Where the slider is.
    pub percent: u16,
    /// Silent here.
    pub muted: bool,
    /// What a move of the slider sends.
    pub on_change: Box<dyn Fn(u16) -> Message + 'a>,
    /// What letting the slider go sends.
    pub on_commit: Box<dyn Fn(u16) -> Message + 'a>,
    /// What Mute for me sends.
    pub on_mute: Message,
}

/// The popover headed by `words.name`, with their `face` before it where there is one: their status (`presence`, none
/// for a screen's sound), the `roles` they hold as
/// badges that wrap, the account's `status` menu on its own, their volume row while `heard`, the `message` button that
/// opens a conversation with them, and the `admin` entries under their heading, none for an account that may do
/// nothing to them.
#[expect(
    clippy::too_many_arguments,
    reason = "each is a separate part of the popover, which a caller leaves out or gives"
)]
pub fn person_popover<'a, Message: Clone + 'a, R: Renderer + 'a>(
    words: PersonWords,
    face: Option<Element<'a, Message, Theme, R>>,
    presence: Option<Presence>,
    roles: Vec<HeldRole>,
    status: Option<StatusChoices<Message>>,
    heard: Option<Heard<'a, Message>>,
    message: Option<(Label, Message)>,
    admin: Vec<MenuEntry<Message>>,
) -> Element<'a, Message, Theme, R> {
    let secondary = |said: String| coloured(said, TypeStyle::Caption, TextClass::Content(Content::Secondary));
    let mut head = vec![text(words.name.as_str().to_string(), TypeStyle::BodyStrong)._boxed()];
    if let Some(presence) = presence {
        let dot = presence.dot.map(|dot| status_mark_dot(dot, Surface::Overlay));
        let line = Row::with_children(dot.into_iter().chain([secondary(presence.said)._boxed()]))
            .spacing(Space::Xs.px() + Space::Xxs.px())
            .align_y(Alignment::Center);
        head.push(line._boxed());
    }
    let head = Column::with_children(head).spacing(Space::Xs.px() + Space::Xxs.px());
    let head: Element<'a, Message, Theme, R> = match face {
        Some(face) => Row::with_children([face, head.width(Length::Fill)._boxed()])
            .spacing(Space::S.px())
            .align_y(Alignment::Center)
            ._boxed(),
        None => head._boxed(),
    };
    let mut lines = vec![head];
    if !roles.is_empty() {
        let badges = roles.into_iter().map(|role| role_badge(&role.name, role.colour));
        let wrapped = Row::with_children(badges)
            .spacing(Space::Xs.px() + Space::Xxs.px())
            .wrap()
            .vertical_spacing(Space::Xs.px() + Space::Xxs.px());
        lines.push(Labelled::new(wrapped, Semantics { role: SemanticRole::List, label: words.roles })._boxed());
    }
    if let Some(status) = status {
        lines.push(container(hairline()).padding(Padding::from([Space::Xxs.px(), 0.0]))._boxed());
        lines.push(status_menu(status));
    }
    if let Some(heard) = heard {
        lines.push(secondary(words.volume.as_str().to_string())._boxed());
        lines.push(volume_row(
            heard.words,
            heard.percent,
            heard.muted,
            heard.on_change,
            heard.on_commit,
            heard.on_mute,
        ));
    }
    if let Some((said, open)) = message {
        lines.push(Button::new(said).variant(ButtonVariant::Primary).on_press(open).width(Length::Fill).view::<R>());
    }
    if !admin.is_empty() {
        lines.push(container(hairline()).padding(Padding::from([Space::Xxs.px(), 0.0]))._boxed());
        lines.push(section_heading(&words.as_admin)._boxed());
        lines.extend(entry_rows(admin));
    }
    // The panel's border is inside its width, as the canvas's popover has it.
    let column = Column::with_children(lines)
        .spacing(Space::S.px() + Space::Xxs.px())
        .width(Length::Fixed(INFO_PANEL_WIDTH - 2.0 * (Space::L.px() + BORDER_WIDTH)));
    Labelled::new(
        on_panel(container(column).padding(BORDER_WIDTH)._boxed(), Space::L, MENU_RADIUS),
        Semantics { role: SemanticRole::Dialog, label: words.name },
    )
    ._boxed()
}

/// The account's status menu: each status a choice with its dot, the one chosen checked with its end beside it, and
/// under the one being picked its durations, set in to its word as the canvas has them.
fn status_menu<'a, Message: Clone + 'a, R: Renderer + 'a>(
    status: StatusChoices<Message>,
) -> Element<'a, Message, Theme, R> {
    let mut rows = Vec::new();
    for choice in status.rows {
        let mut row = ListRow::<Message, R>::new(choice.label).menu_choice(choice.chosen).wrapped();
        if let Some(dot) = choice.dot {
            row = row.after_mark(status_mark_dot(dot, Surface::Overlay));
        }
        if let Some(until) = &choice.until {
            row = row.noted(until);
        }
        rows.push(row.selected(choice.chosen || choice.picking).on_press(choice.message).view());
        if choice.durations.is_empty() {
            continue;
        }
        let durations = choice
            .durations
            .into_iter()
            .map(|(label, message)| ListRow::<Message, R>::new(label).menu_item().on_press(message).view());
        let indent = Space::Xl.px() + Space::Xs.px() + Space::Xxs.px();
        rows.push(
            container(Column::with_children(durations).spacing(Space::Xxs.px()))
                .padding(Padding { left: indent, ..Padding::ZERO })
                ._boxed(),
        );
    }
    let menu = Column::with_children(rows).spacing(Space::Xxs.px()).width(Length::Fill);
    Labelled::new(menu, Semantics { role: SemanticRole::Menu, label: status.label })._boxed()
}
