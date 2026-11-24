//! The open server's channels in the side pane, in the server's order: each category's header and its channels. A row is dragged, or moved by Alt and an arrow key, by whoever manages the
//! channels; a header folds its category; a right press on a channel or the menu key opens its menu for every member,
//! its gear for whoever manages the channels. A text channel with unread messages is strong with a dot beside it, its
//! mentions counted at its end; a muted one is faint, its mentions still counted. A voice channel's row holds the
//! people in its call.

use crate::discussion::Handles;
use crate::labels::{label, phrase};
use crate::menu::{MenuChoice, MenuEntry, choice_row, entry_rows, menu_with_submenu, submenu};
use crate::{Button, Icon, IconClass, ListRow, Renderer, TextClass, Theme, coloured, icon, seat_row};
use design::{ButtonVariant, Content, IconSize, Label, Role, Space, TypeStyle};
use iced_core::widget::Id;
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, button, container, scrollable};
use screens::{
    Channel, DiscussionMessage, Fold, ListKey, ListMenuView, Lobby, LobbyMessage, PaneRow, Panel, Place, PlaceMessage,
    Roll, Sight, TalkMessage, Window, WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The pane's scrollable, which a drag held at its edge scrolls.
const LIST: &str = "pane-channels";
/// How far a category's header and its "+" stand in from the row's ends, and apart.
const HEADER_INSET: f32 = Space::Xs.px();
/// How far a category's channels stand in from its heading.
const CATEGORY_INSET: f32 = Space::M.px();
/// A channel's own line: its name's line and the row's padding above and below it.
const ROW_LINE: f32 = TypeStyle::Body.line_height() + 2.0 * (Space::S.px() - Space::Xxs.px());

/// Where an unread channel's dot stands beside its row.
#[derive(Clone, Copy)]
enum Dot {
    /// Beside the row's middle: a row of one line.
    Middle,
    /// Beside the row's first line: a voice channel, whose people are listed under it.
    FirstLine,
}

/// What a channel's messages ask of its row: the dot of unread ones, how many call this account, and what is heard
/// of them. Nothing while its messages are open, and a muted channel keeps only its mentions.
struct Asked {
    unread: bool,
    mentions: u32,
    faint: bool,
    heard: Option<Sentence>,
}

fn asked(place: &Place, channel: &terms::ChannelId, name: &str, open: bool, now: Option<i64>) -> Asked {
    let standing = place.channel_standing(channel).unwrap_or_default();
    let muted = now.is_some_and(|now| place.channel_muted(channel, now));
    let unread = standing.unread > 0 && !muted && !open;
    let heard = match (muted, standing.mentions, unread) {
        (true, mentions @ 1.., _) => Some(Sentence::ChannelMutedMentions(name.to_owned(), mentions)),
        (false, mentions @ 1.., _) => Some(Sentence::ChannelMentions(name.to_owned(), mentions)),
        (false, 0, true) => Some(Sentence::ChannelUnread(name.to_owned())),
        (_, 0, _) => None,
    };
    Asked { unread, mentions: standing.mentions, faint: muted, heard }
}

/// The room left of every row, where an unread channel's dot stands.
const MARK_GUTTER: f32 = Space::Xs.px() + Space::Xxs.px();

/// The channel list of `place` as its `lobby` stands: its rows, or why there are none.
pub(crate) fn channel_list<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    lobby: &'a Lobby,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let column: Part<'a, R> = match lobby {
        Lobby::Loading => iced_widget::Space::new()._boxed(),
        Lobby::Failed(failed) => {
            let said = words.say(&words::failure::said(failed));
            Column::with_children([
                crate::refusal(said),
                Button::new(phrase(Phrase::TryAgain, words))
                    .variant(ButtonVariant::Secondary)
                    .compact()
                    .on_press(Message::At(id, PlaceMessage::Lobby(LobbyMessage::Retry)))
                    .view::<R>(),
            ])
            .spacing(Space::Xxs.px())
            ._boxed()
        }
        Lobby::Ready(listed) => {
            let rows = place.pane_rows();
            if listed.categories.is_empty() && place.manages_channels() {
                create_category_row(id, words)
            } else if rows.is_empty() {
                no_channels_yet(words)
            } else {
                let now = window.wall();
                let mut inset = 0.0;
                let drawn: Vec<(ListKey, Part<'a, R>)> = rows
                    .iter()
                    .enumerate()
                    .map(|(at, row)| {
                        let key = row.key();
                        if matches!(row, PaneRow::Category(..)) {
                            inset = CATEGORY_INSET;
                        }
                        let (row, dot) = pane_row(place, *row, at == 0, inset, now, handles, words);
                        let row = container(row)
                            .padding(Padding { bottom: Space::Xxs.px(), left: MARK_GUTTER, ..Padding::ZERO })
                            ._boxed();
                        let row = match dot {
                            // Beside the row's middle, not the middle of the row and the gap under it.
                            Some(Dot::Middle) => {
                                let dot = container(crate::unread_dot())
                                    .center_y(Length::Fill)
                                    .padding(Padding { bottom: Space::Xxs.px(), ..Padding::ZERO });
                                iced_widget::stack([row, dot._boxed()])._boxed()
                            }
                            // Beside the channel's own line, over the people under it.
                            Some(Dot::FirstLine) => {
                                let dot = container(crate::unread_dot()).center_y(Length::Fixed(ROW_LINE));
                                iced_widget::stack([row, dot._boxed()])._boxed()
                            }
                            None => row,
                        };
                        (key, row)
                    })
                    .collect();
                crate::reorder(
                    phrase(Phrase::ChannelsInOrder, words),
                    drawn,
                    move |moved| place.may_move(moved),
                    move |moved| Message::At(id, PlaceMessage::Lobby(LobbyMessage::Move(moved))),
                )
            }
        }
    };
    let mut parts = vec![talk_rows(place, words)];
    if matches!(lobby, Lobby::Ready(_)) && talk_row_count(place) > 0.0 {
        parts.push(divider());
    }
    parts.push(column);
    let column = Column::with_children(parts)._boxed();
    let padded = container(column)
        .padding(Padding {
            top: Space::M.px(),
            bottom: Space::M.px(),
            left: Space::S.px() - MARK_GUTTER,
            right: Space::S.px(),
        })
        .width(Length::Fill);
    let list = scrollable(padded)
        .id(Id::new(LIST))
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .height(Length::Fill);
    // A right press where no row is opens what is made in the list; a row's own is the row's.
    let list = iced_widget::mouse_area(crate::edge_scroll::edge_scroll(list._boxed(), Id::new(LIST)))
        .on_right_press(Message::Menu(id, screens::MenuFrom::Pane));
    list._boxed()
}

/// The rows atop the list that open the lists of what is not a channel's: the direct messages, then the threads this
/// account follows, each with the count of what is unread in it, each only where the server has them.
fn talk_rows<'a, R: Renderer + 'a>(place: &'a Place, words: &'a Words) -> Part<'a, R> {
    let id = place.id();
    let marked = place.marked();
    let row = |roll: Roll, mark: Icon, name: Phrase, unread: u32| {
        let said = phrase(name, words);
        let mut row = ListRow::new(said.clone())
            .leading(icon(mark, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed())
            .selected(marked == Some(screens::Marked::Roll(roll)))
            .unread(unread > 0)
            .quiet();
        if unread > 0 {
            row = row.trailing(crate::count_badge(unread));
        }
        let row = row.on_press(Message::At(id, PlaceMessage::Talk(TalkMessage::Show(roll))));
        container(row.view()).padding(Padding { bottom: Space::Xxs.px(), left: MARK_GUTTER, ..Padding::ZERO })._boxed()
    };
    let rows = [
        place.offers_conversations().then(|| {
            row(Roll::Conversations, Icon::MessageSquare, Phrase::DirectMessages, place.conversations_unread())
        }),
        place
            .offers_threads()
            .then(|| row(Roll::Threads, Icon::MessagesSquare, Phrase::Threads, place.threads_unread())),
    ];
    Column::with_children(rows.into_iter().flatten())._boxed()
}

/// The line between the rows that open lists and the channels under them.
fn divider<'a, R: Renderer + 'a>() -> Part<'a, R> {
    container(crate::panel_parts::hairline())
        .padding(Padding { top: Space::S.px(), bottom: Space::S.px(), left: Space::Xs.px(), right: Space::Xs.px() })
        ._boxed()
}

/// What [`divider`] takes of the list's height, where the rows that open lists are shown.
fn divider_height(place: &Place) -> f32 {
    if talk_row_count(place) > 0.0 { 2.0 * Space::S.px() + design::BORDER_WIDTH } else { 0.0 }
}

/// What a member reads where a server has no channel to show.
fn no_channels_yet<'a, R: Renderer + 'a>(words: &'a Words) -> Part<'a, R> {
    let line = coloured(words.phrase(Phrase::NoChannelsYet), TypeStyle::Small, TextClass::Content(Content::Secondary));
    container(line)
        .padding(Padding {
            top: Space::S.px() - Space::Xxs.px(),
            bottom: Space::S.px() - Space::Xxs.px(),
            left: MARK_GUTTER + Space::S.px(),
            right: Space::S.px(),
        })
        ._boxed()
}

/// Where the first category's header would stand, the row that makes the server's first for whoever manages the
/// channels.
fn create_category_row<'a, R: Renderer + 'a>(id: screens::PlaceId, words: &'a Words) -> Part<'a, R> {
    let said = words.phrase(Phrase::CreateCategory);
    let header = header_button(Icon::Plus, said.to_uppercase(), label(&said), Message::CreateCategory(id));
    let row = Row::with_children([header]).padding(Padding::from([0.0, HEADER_INSET])).align_y(Alignment::Center);
    container(row)
        .padding(Padding { top: above_header(true), bottom: Space::Xxs.px(), left: MARK_GUTTER, ..Padding::ZERO })
        ._boxed()
}

/// How many rows [`talk_rows`] puts atop the list.
fn talk_row_count(place: &Place) -> f32 {
    f32::from(u8::from(place.offers_threads()) + u8::from(place.offers_conversations()))
}

/// The room above a category's header: a little under the pane's top or its divider, a gap that sets it apart from
/// the rows above it everywhere else.
fn above_header(first: bool) -> f32 {
    if first { Space::Xxs.px() } else { Space::S.px() + Space::Xxs.px() }
}

/// One row of the list: a category's header, a text channel, or a voice channel with the people in its call. `first`
/// is the list's first row, which takes [`above_header`]'s room at the top of the pane; `inset` how far a channel stands in, under a category;
/// `now` the wall clock a mute is measured by. Whether the unread dot stands beside it comes with it.
fn pane_row<'a, R: Renderer + 'a>(
    place: &'a Place,
    row: PaneRow<'a>,
    first: bool,
    inset: f32,
    now: Option<i64>,
    handles: &Handles<'_>,
    words: &'a Words,
) -> (Part<'a, R>, Option<Dot>) {
    let id = place.id();
    let manages = place.manages_channels();
    let menu = Message::ListMenu(id, row.key());
    let joinable = place.panel() != Panel::Lost;
    // A channel's menu opens for every member, a category's for whoever manages the channels: by a right press, or
    // the menu key on the focused row, as everything with a menu here opens it.
    let with_menu = |row: Part<'a, R>, everyone: bool| -> Part<'a, R> {
        if manages || everyone { crate::context::on_menu(row, menu.clone()) } else { row }
    };
    // A category's own stand in from its heading, so the list shows whose they are.
    let under = |row: Part<'a, R>| container(row).padding(Padding { left: inset, ..Padding::ZERO })._boxed();
    match row {
        PaneRow::Category(category, fold) => {
            let header = category_header(place, category, fold, words);
            let above = above_header(first);
            let header = with_menu(header, false);
            (container(header).padding(Padding { top: above, ..Padding::ZERO })._boxed(), None)
        }
        PaneRow::Text(channel, sight) => {
            let open = place.marked().is_some_and(|marked| marked == screens::Marked::Channel(channel.id.clone()));
            let asked = asked(place, &channel.id, &channel.name, open, now);
            let unread = asked.unread;
            let row = ListRow::new(label(&channel.name))
                .leading(kind_icon(Icon::Hash, Icon::HashLock, sight, words))
                .selected(open);
            let row = marked(row, &asked, words);
            let opens = Message::At(id, PlaceMessage::Discussion(DiscussionMessage::Open(channel.id.clone())));
            let row = if joinable { row.on_press(opens) } else { row };
            (under(with_menu(row.view(), true)), unread.then_some(Dot::Middle))
        }
        PaneRow::Voice(channel, sight) => {
            let here = place.call().is_some_and(|call| call.live() && call.channel.id == channel.id);
            let seats = place.seats(channel);
            let elsewhere = seats.iter().any(|seat| seat.elsewhere);
            // Its messages are open while the call's chat shows them.
            let open = place.call_chat().is_some_and(|open| open.owner.channel() == Some(&channel.id));
            let asked = asked(place, &channel.id, &channel.name, open, now);
            let unread = asked.unread;
            let marked = place.marked().is_some_and(|marked| marked == screens::Marked::Channel(channel.id.clone()));
            let row = voice_row(id, channel, sight, (here, marked, elsewhere, joinable), &asked, words);
            let line = with_menu(row, true);
            // A row takes the seat back from another device; a press or a right press opens someone's popover.
            let take = Message::At(id, PlaceMessage::Lobby(LobbyMessage::Join(channel.id.clone())));
            let mut lines = vec![line];
            lines.extend(seats.iter().map(|seat| {
                let open = seat
                    .user_id
                    .as_ref()
                    .map(|user_id| Message::Person(id, user_id.clone(), screens::PersonFrom::Seat));
                let press = if joinable && seat.elsewhere { Some(take.clone()) } else { open.clone() };
                let picture = seat.user_id.as_ref().and_then(|user_id| place.picture_of(user_id)).and_then(handles);
                seat_row(seat, picture, press, open, words)
            }));
            (under(Column::with_children(lines).spacing(Space::Xxs.px())._boxed()), unread.then_some(Dot::FirstLine))
        }
    }
}

/// The channel's or category's menu `open`: its items, a chevron on each that opens a submenu, and the submenu open
/// beside the item it hangs from: the durations of a mute, or the levels with the one taken checked.
pub(crate) fn list_menu_items<'a, R: Renderer + 'a>(open: &ListMenuView, words: &'a Words) -> Part<'a, R> {
    let (place, key) = (open.menu.place, open.menu.key.clone());
    let pick = |pick| Message::ListPick(place, key.clone(), pick);
    let entry = |said, pick, danger, rule_before| MenuEntry {
        label: phrase(said, words),
        message: pick,
        danger,
        rule_before,
        icon: None,
    };
    let entries = open
        .items
        .iter()
        .map(|item| (entry(item.said, pick(item.pick), item.danger, item.rule_before), item.opens))
        .collect();
    let sub = open.sub.as_ref().and_then(|sub| {
        let from = open.items.get(sub.from)?;
        let rows = sub
            .items
            .iter()
            .flat_map(|item| match item.chosen {
                Some(chosen) => vec![choice_row(MenuChoice {
                    leading: None,
                    label: phrase(item.said, words),
                    note: item.note.map(|note| phrase(note, words)),
                    chosen,
                    message: pick(item.pick),
                })],
                None => entry_rows(vec![entry(item.said, pick(item.pick), false, false)]),
            })
            .collect();
        Some((sub.from, submenu(phrase(from.said, words), rows)))
    });
    menu_with_submenu(label(&open.title), entries, sub)
}

/// A channel's mark: its kind's icon, or the same with a lock while it is private, named Private.
pub(crate) fn kind_icon<'a, R: Renderer + 'a>(plain: Icon, locked: Icon, sight: Sight, words: &Words) -> Part<'a, R> {
    let class = IconClass::Role(Role::Content(Content::Secondary));
    match sight {
        Sight::Everyone => icon(plain, IconSize::S, class)._boxed(),
        Sight::Private => {
            let named = phrase(Phrase::PrivateMark, words);
            let mark = crate::Labelled::new(
                icon(locked, IconSize::S, class),
                design::Semantics { role: design::SemanticRole::Image, label: named.clone() },
            );
            crate::tooltip(mark._boxed(), &named, iced_widget::tooltip::Position::Bottom)
        }
    }
}

/// A voice channel's own row; `(here, shown, elsewhere, joinable)`: this account is in its call here, the content
/// shows that call, it is in the call on another device, and whether a press may join it now. Its tooltip says what a
/// press does: open the call, take the seat back, join.
fn voice_row<'a, R: Renderer + 'a>(
    id: screens::PlaceId,
    channel: &'a Channel,
    sight: Sight,
    (here, shown, elsewhere, joinable): (bool, bool, bool, bool),
    asked: &Asked,
    words: &'a Words,
) -> Part<'a, R> {
    let tip = if here {
        words.say(&Sentence::OpenChannel(channel.name.clone()))
    } else if elsewhere {
        words.phrase(Phrase::SwitchHere).to_string()
    } else {
        words.say(&Sentence::JoinChannel(channel.name.clone()))
    };
    let row = ListRow::new(label(&channel.name))
        .leading(kind_icon(Icon::Volume, Icon::VolumeLock, sight, words))
        .selected(shown)
        .tip(label(&tip));
    let row = marked(row, asked, words);
    if joinable {
        row.on_press(Message::At(id, PlaceMessage::Lobby(LobbyMessage::Join(channel.id.clone()))))
    } else {
        row
    }
    .view()
}

/// `row` saying what its channel's messages ask: bold while unread, faint while muted, the count of what calls
/// this account at its end, and all of it heard.
fn marked<'a, R: Renderer + 'a>(row: ListRow<'a, Message, R>, asked: &Asked, words: &Words) -> ListRow<'a, Message, R> {
    let mut row = row.unread(asked.unread).faint(asked.faint).quiet();
    if let Some(heard) = &asked.heard {
        row = row.heard(label(&words.say(heard)));
    }
    if asked.mentions > 0 {
        row = row.trailing(crate::count_badge(asked.mentions));
    }
    row
}

/// The button across a header's width: a mark and the `shown` text beside it in the header's style, named `named` for
/// whoever does not see it.
fn header_button<'a, R: Renderer + 'a>(mark: Icon, shown: String, named: Label, press: Message) -> Part<'a, R> {
    let quiet = IconClass::Role(Role::Content(Content::Secondary));
    let line = Row::with_children([
        icon(mark, IconSize::Mark, quiet)._boxed(),
        coloured(shown, TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary))
            .wrapping(iced_core::text::Wrapping::None)
            .width(Length::Fill)
            ._boxed(),
    ])
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center);
    let inner = button(line)
        .class(crate::button::ButtonClass::Row { selected: false, tone: crate::button::RowTone::Quiet })
        .padding(Padding::from([Space::Xs.px(), Space::Xs.px()]))
        .width(Length::Fill)
        .on_press(press.clone());
    let semantics = design::Semantics { role: design::SemanticRole::Button, label: named };
    crate::button::actionable(inner, semantics, Some(press))
}

/// A category's header: its chevron and its name, which fold it or open it, and for whoever manages the channels a
/// "+" that makes a channel in it.
fn category_header<'a, R: Renderer + 'a>(
    place: &'a Place,
    category: &'a screens::Category,
    fold: Fold,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let chevron = match fold {
        Fold::Open => Icon::ChevronDown,
        Fold::Folded => Icon::ChevronRight,
    };
    let fold_press = Message::At(id, PlaceMessage::Lobby(LobbyMessage::Fold(category.id.clone())));
    let header = header_button(chevron, category.name.to_uppercase(), label(&category.name), fold_press);
    let mut cells = vec![header];
    if place.manages_channels() {
        let create = Button::new(label(&words.say(&Sentence::CreateChannelIn(category.name.clone()))))
            .icon_only(Icon::Plus)
            .variant(ButtonVariant::Ghost)
            .small()
            .on_press(Message::CreateChannelIn(id, category.id.clone()));
        cells.push(create.view::<R>());
    }
    Row::with_children(cells)
        .spacing(HEADER_INSET)
        .padding(Padding::from([0.0, HEADER_INSET]))
        .align_y(Alignment::Center)
        ._boxed()
}

/// Where the row `key` of `place`'s list stands in a pane `width` wide whose left edge is at `left`, scrolled to its
/// top: what a menu opened with no row focused hangs from. Measured as the rows are drawn.
pub(crate) fn row_box(place: &Place, key: &ListKey, left: f32, width: f32) -> Option<iced_core::Rectangle> {
    let header = design::Space::Xs.px() * 2.0 + TypeStyle::CaptionStrong.line_height();
    let line = TypeStyle::Body.line_height() + 2.0 * (Space::S.px() - Space::Xxs.px());
    let mut y = design::PANE_HEADER_HEIGHT
        + design::BORDER_WIDTH
        + Space::M.px()
        + talk_row_count(place) * (line + Space::Xxs.px())
        + divider_height(place);
    let mut inset = 0.0;
    for (at, row) in place.pane_rows().iter().enumerate() {
        let (top, tall, below) = match row {
            PaneRow::Category(..) => {
                inset = CATEGORY_INSET;
                (y + above_header(at == 0), header, 0.0)
            }
            PaneRow::Text(..) => (y, line, 0.0),
            PaneRow::Voice(channel, _) => {
                #[expect(clippy::cast_precision_loss, reason = "a call holds few people")]
                let seats = place.seats(channel).len() as f32;
                (y, line, seats * (design::PERSON_ROW_HEIGHT + Space::Xxs.px()))
            }
        };
        if row.key() == *key {
            let stood = if matches!(row, PaneRow::Category(..)) { 0.0 } else { inset };
            let x = left + Space::S.px() + stood;
            return Some(iced_core::Rectangle { x, y: top, width: width - 2.0 * Space::S.px() - stood, height: tall });
        }
        y = top + tall + below + Space::Xxs.px();
    }
    None
}
