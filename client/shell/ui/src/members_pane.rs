//! The members column: everyone on the server, those online first, each a row that opens their popover, someone
//! offline drawn faint and someone in a voice channel marked by a speaker that names it. Only the lines in and round
//! its viewport are built, the rest standing as space of their height.

use crate::list_row::ListRow;
use crate::typography::coloured;
use crate::{AvatarKind, Held, Labelled, Renderer, Surfaced, TextClass, Theme, person_with_status};
use design::{Content, Label, PersonColour, SemanticRole, Semantics, Space, Surface, TypeStyle};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, Space as Gap, container, scrollable};
use screens::{
    ROSTER_BOTTOM, ROSTER_HEADING, ROSTER_HEADING_ABOVE, ROSTER_HEADING_BELOW, ROSTER_ROW, ROSTER_SIDE, ROSTER_SPACING,
    ROSTER_TOP, StatusWord,
};

/// One member as the column lists them.
pub struct MemberRow<Message> {
    /// The name others see.
    pub name: Label,
    /// The colour they are shown in.
    pub colour: PersonColour,
    /// Their status as others are shown it: its dot at their picture's corner, the row faint and with no dot while
    /// they are offline.
    pub status: StatusWord,
    /// The picture they chose, as the host holds it; none for their initials.
    pub picture: Option<Held>,
    /// What a press on the row sends: their popover.
    pub open: Message,
}

/// One line of the column.
pub enum MemberLine<Message> {
    /// A group's heading, with how many it holds.
    Heading(String),
    /// A member.
    Member(MemberRow<Message>),
}

/// The lines of the column built for where it is scrolled, between the spaces standing for the rest, so it scrolls
/// as far as all of them would.
pub struct MemberLines<Message> {
    /// The height of the space for the lines above those built; none when none is above.
    pub before: Option<f32>,
    /// The lines built, in order.
    pub lines: Vec<MemberLine<Message>>,
    /// The height of the space for the lines below; none when none is below.
    pub after: Option<f32>,
}

/// The column named `label`, its lines `built`, scrolling where they are taller than it, and saying each scroll by
/// `on_scroll`, given how far its top is scrolled past and how tall it shows.
pub fn members_pane<'a, Message: Clone + 'a, R: Renderer + 'a>(
    label: Label,
    built: MemberLines<Message>,
    on_scroll: impl Fn(f32, f32) -> Message + 'a,
) -> Element<'a, Message, Theme, R> {
    let space = |height: f32| -> Element<'a, Message, Theme, R> { Gap::new().height(height)._boxed() };
    let mut lines: Vec<Element<'a, Message, Theme, R>> = Vec::with_capacity(built.lines.len() + 2);
    lines.extend(built.before.map(space));
    lines.extend(built.lines.into_iter().map(|line| match line {
        MemberLine::Heading(heading) => {
            let heading =
                coloured(heading.to_uppercase(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary));
            container(heading)
                .padding(Padding {
                    top: ROSTER_HEADING_ABOVE,
                    bottom: ROSTER_HEADING_BELOW,
                    left: Space::S.px(),
                    right: Space::S.px(),
                })
                .height(ROSTER_HEADING)
                ._boxed()
        }
        MemberLine::Member(member) => container(member_row(member)).width(Length::Fill).height(ROSTER_ROW)._boxed(),
    }));
    lines.extend(built.after.map(space));
    let column = Column::with_children(lines).spacing(ROSTER_SPACING).padding(Padding {
        top: ROSTER_TOP,
        bottom: ROSTER_BOTTOM,
        left: ROSTER_SIDE,
        right: ROSTER_SIDE,
    });
    let list = scrollable(column)
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .on_scroll(move |scroll| on_scroll(scroll.viewport.absolute_offset().y, scroll.viewport.bounds.height))
        .width(Length::Fill)
        .height(Length::Fill);
    let pane = container(list).width(Length::Fill).height(Length::Fill).class(Surfaced::On(Surface::Raised));
    Labelled::new(pane, Semantics { role: SemanticRole::List, label })._boxed()
}

/// A member's row: their picture with the dot of their status, their name, and the speaker while they are in voice.
fn member_row<'a, Message: Clone + 'a, R: Renderer + 'a>(member: MemberRow<Message>) -> Element<'a, Message, Theme, R> {
    let offline = member.status == StatusWord::Offline;
    let kind = if offline { AvatarKind::Offline(member.colour) } else { AvatarKind::Called(member.colour) };
    let dot = if offline { None } else { member.status.dot() };
    let picture = person_with_status(member.name.clone(), kind, member.picture, dot, Surface::Raised);
    let mut row = ListRow::new(member.name).leading(picture).on_press(member.open);
    if offline {
        row = row.quiet();
    }
    row.view()
}
