//! The window's people: the members column of the open server, and the popover of the person it is open on, drawn
//! once over the window beside what opened it.

use crate::discussion::Handles;
use crate::labels::{label, phrase};
use crate::layer::Side;
use crate::{
    AvatarKind, Heard, HeldRole, MemberLine, MemberLines, MemberRow, MenuEntry, PersonWords, Presence, StatusChoice,
    StatusChoices, Theme, members_pane,
};
use crate::{Icon, Renderer};
use design::{ACCOUNT_STRIP_HEIGHT, AvatarSize, BORDER_WIDTH, INFO_PANEL_WIDTH, PANE_HEADER_HEIGHT, RAIL_WIDTH, Space};
use iced_core::{Element, Rectangle};
use screens::frame::{Frame, MembersColumn, Room, Shape};
use screens::{
    Moderation, PersonFrom, Place, ROSTER_ROW, ROSTER_SIDE, RosterGroup, RosterLine, StatusMenu, StatusMessage,
    StatusWord, Window, WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The members column of `place`: who is online, then who is not, each opening their popover.
pub(crate) fn members_column<'a, R: Renderer + 'a>(place: &Place, handles: &Handles<'_>, words: &Words) -> Part<'a, R> {
    let roster = place.roster();
    let id = place.id();
    let line = |line: RosterLine<'_>| match line {
        RosterLine::Heading(group, members) => {
            let members = u32::try_from(members).unwrap_or(u32::MAX);
            MemberLine::Heading(words.say(&match group {
                RosterGroup::Online => Sentence::MembersOnline(members),
                RosterGroup::Offline => Sentence::MembersOffline(members),
            }))
        }
        RosterLine::Member(member) => MemberLine::Member(MemberRow {
            name: label(member.name),
            colour: member.colour,
            status: StatusWord::of(member.presence),
            picture: member.picture.and_then(handles),
            open: Message::Person(id, member.user_id.clone(), PersonFrom::Members),
        }),
    };
    let built =
        MemberLines { before: roster.before, lines: roster.lines.into_iter().map(line).collect(), after: roster.after };
    let scrolled = move |offset, viewport| {
        Message::At(id, screens::PlaceMessage::Roster(screens::RosterMessage::Scrolled { offset, viewport }))
    };
    members_pane(phrase(Phrase::Members, words), built, scrolled)
}

/// The popover of the person it is open on, the side of its opener it opens on, and where the screen puts that opener
/// for a popover opened with no control focused.
pub(crate) fn person_over<'a, R: Renderer + 'a>(
    window: &'a Window,
    shaped: &Frame,
    room: Room,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Option<(Part<'a, R>, Side, Rectangle)> {
    let card = window.person()?;
    let place = window.places().iter().find(|place| place.id() == card.open.place)?;
    let id = place.id();
    let user_id = card.open.user_id.clone();
    let said = PersonWords {
        name: label(&card.name),
        volume: phrase(Phrase::Volume, words),
        as_admin: phrase(Phrase::AsAnAdmin, words),
        roles: phrase(Phrase::Roles, words),
    };
    let presence = Presence { dot: card.presence.dot(), said: words.phrase(card.presence.phrase()).into_owned() };
    let status = card.status.as_ref().map(|menu| status_choices(menu, words));
    let roles = card
        .roles
        .iter()
        .map(|role| HeldRole { name: role.name.clone(), colour: role.colour.map(screens::colour::rgb) })
        .collect();
    if let Some(volume) = card.volume {
        let popover = volume_popover(id, &user_id, volume, said, words);
        let (side, fallback) = opener(place, &card.open, shaped, room);
        return Some((popover, side, fallback));
    }
    let admin = card.admin.map_or_else(Vec::new, |admin| {
        let mute = match admin.mute {
            Moderation::Mute(false) => Phrase::UndoMuteForEveryone,
            Moderation::Mute(true) | Moderation::TakeOut => Phrase::MuteForEveryone,
        };
        vec![
            MenuEntry {
                label: phrase(mute, words),
                message: Message::Moderate(admin.mute),
                danger: false,
                rule_before: false,
                icon: Some(Icon::MicOff),
            },
            MenuEntry {
                label: phrase(Phrase::TakeOutOfVoice, words),
                message: Message::Moderate(admin.take_out),
                danger: true,
                rule_before: false,
                icon: Some(Icon::PhoneOff),
            },
        ]
    });
    let message = card.may_message.then(|| {
        (
            phrase(Phrase::Message, words),
            Message::At(id, screens::PlaceMessage::Talk(screens::TalkMessage::Message(user_id.clone()))),
        )
    });
    let colour = place.directory().person(&user_id).map(|named| screens::colour_of(&user_id, named.colour));
    let kind = colour.map_or(AvatarKind::Person, AvatarKind::Called);
    let held = place.picture_of(&user_id).and_then(handles);
    let face = crate::avatar_pictured(label(&card.name), kind, AvatarSize::Author, held);
    let popover = crate::person_popover(said, Some(face), Some(presence), roles, status, None, message, admin);
    let (side, fallback) = opener(place, &card.open, shaped, room);
    Some((popover, side, fallback))
}

/// The account's status `menu` in the window's words: each row's press and, under the one being picked, its durations.
fn status_choices(menu: &StatusMenu, words: &Words) -> StatusChoices<Message> {
    let rows = menu
        .rows
        .iter()
        .map(|row| StatusChoice {
            label: phrase(row.word.phrase(), words),
            dot: row.word.dot(),
            chosen: row.chosen,
            until: row.until.as_ref().map(|until| words.say(until)),
            picking: row.picking,
            message: Message::Status(StatusMessage::Pick(row.choice)),
            durations: if row.picking {
                menu.durations
                    .iter()
                    .map(|duration| (phrase(duration.phrase(), words), Message::Status(StatusMessage::For(*duration))))
                    .collect()
            } else {
                Vec::new()
            },
        })
        .collect();
    StatusChoices { label: phrase(Phrase::Status, words), rows }
}

/// The popover of how loud `user_id`'s voice plays here, which is all a person on a call's card has.
pub(crate) fn volume_popover<'a, R: Renderer + 'a>(
    id: screens::PlaceId,
    user_id: &terms::MemberId,
    volume: choices::Volume,
    said: PersonWords,
    words: &Words,
) -> Part<'a, R> {
    let row = crate::call::sound_controls(id, volume, user_id, choices::Audio::Voice, said.volume.clone(), words);
    let heard = Heard {
        words: row.words,
        percent: row.percent,
        muted: row.muted,
        on_change: row.on_change,
        on_commit: row.on_commit,
        on_mute: row.on_mute,
    };
    crate::person_popover(said, None, None, Vec::new(), None, Some(heard), None, Vec::new())
}

/// The About of the app whose post is open: its name, the badge and who carries the post, under the name that opened
/// it, as a person's popover opens there.
pub(crate) fn app_over<'a, R: Renderer + 'a>(
    window: &'a Window,
    shaped: &Frame,
    words: &'a Words,
) -> Option<(Part<'a, R>, Side, Rectangle)> {
    let about = window.app_about()?;
    let about_line = match about.channel {
        Some(channel) => words.say(&Sentence::WebhookIn { webhook: about.webhook_name, channel }),
        None => words.say(&Sentence::Webhook(about.webhook_name)),
    };
    let popover = crate::app_popover(label(&about.name), &words.phrase(Phrase::AppBadge), about_line);
    let (side, fallback) = (Side::Below, content_corner(shaped));
    Some((popover, side, fallback))
}

/// The top-left corner of the content the window shows, where a popover with no opener to sit beside is put.
fn content_corner(shaped: &Frame) -> Rectangle {
    let rail = if shaped.shape == Shape::Drawer { 0.0 } else { RAIL_WIDTH + BORDER_WIDTH };
    let pane = shaped.pane.filter(|_| !shaped.folded()).map_or(0.0, |pane| pane + BORDER_WIDTH);
    Rectangle { x: rail + pane, y: shaped.top + PANE_HEADER_HEIGHT, width: 0.0, height: 0.0 }
}

/// Which side of its opener a person's popover opens on, and where the screen puts that opener: a row of the members
/// column, else the corner of the content it was opened in.
fn opener(place: &Place, open: &screens::PersonOpen, shaped: &Frame, room: Room) -> (Side, Rectangle) {
    let rail = if shaped.shape == Shape::Drawer { 0.0 } else { RAIL_WIDTH + BORDER_WIDTH };
    let pane = shaped.pane.filter(|_| !shaped.folded()).map_or(0.0, |pane| pane + BORDER_WIDTH);
    let content = Rectangle { width: shaped.content, height: room.height, ..content_corner(shaped) };
    let corner = |side| (side, Rectangle { width: 0.0, height: 0.0, ..content });
    match open.from {
        PersonFrom::Members => {
            let Some(width) = shaped.members.map(|column| match column {
                MembersColumn::Docked(width) | MembersColumn::Over(width) => width,
            }) else {
                return corner(Side::Below);
            };
            let Some(top) = place.roster_top(&open.user_id) else {
                return corner(Side::Below);
            };
            let x = room.width - width + ROSTER_SIDE;
            let y = shaped.top_room() + top;
            (Side::Left, Rectangle { x, y, width: width - 2.0 * ROSTER_SIDE, height: ROSTER_ROW })
        }
        PersonFrom::Seat => {
            (Side::Right, Rectangle { x: rail, y: shaped.top + PANE_HEADER_HEIGHT, width: pane, height: 0.0 })
        }
        PersonFrom::Card => {
            let x = content.x + content.width - INFO_PANEL_WIDTH - Space::L.px() - Space::S.px();
            let y = shaped.top_room() + Space::L.px();
            (Side::Pointer, Rectangle { x, y, width: 0.0, height: 0.0 })
        }
        PersonFrom::Author => corner(Side::Below),
        PersonFrom::AccountBar => {
            let y = room.height - ACCOUNT_STRIP_HEIGHT;
            let x = rail + Space::M.px() - Space::Xs.px();
            (Side::Above, Rectangle { x, y, width: 0.0, height: ACCOUNT_STRIP_HEIGHT })
        }
    }
}
