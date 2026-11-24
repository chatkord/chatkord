//! A text channel's messages, in the content area or beside a call: its header, its messages as flat rows grouped by
//! author (the canvas's Chat board), only the rows near the viewport built, each with the line it quotes, its files,
//! its reactions and its tools on hover; then who is typing and the composer with its reply, its files and its
//! chooser. Every value is the model's.

use crate::keep_at::KeepAt;
use crate::rows::Rows;
use crate::typography::message_text;
use crate::{
    AvatarKind, BodyPress, Button, ChooserGroup, ChooserKeys, ChooserLeading, ChooserRow, FileCard, HoverTools, Icon,
    IconClass, MenuEntry, Pending, PendingState, Pin, QuotedReply, ReactionChip, ReactionEmoji, Renderer, Replying,
    Surfaced, TextClass, Theme, avatar_pictured, coloured, icon, menu_at_pointer, menu_items, text,
};
use design::{
    AvatarSize, ButtonVariant, Content, IconSize, MESSAGE_PAD, MessageScale, MessageSize, PANE_HEADER_HEIGHT,
    PersonColour, Popup, Radius, Role, Space, Status, Surface, TypeStyle,
};
use iced_core::widget::operation::Operation;
use iced_core::widget::{Tree, tree};
use iced_core::{
    Alignment, Element, Layout, Length, Padding, Rectangle, Size, Vector, Widget, image, layout, mouse, overlay,
    renderer,
};
use iced_widget::{Column, Row, Space as Gap, container, mouse_area, scrollable, sensor, stack};
use proto::files::FileId;
use proto::rules::Carries;
use screens::chat::{self, Author, Body, Draft, Row as ListedRow, RowKey};
use screens::{
    AttachState, Choice, Choosing, Clicked, Discussion, DiscussionMessage, Owner, Pane, PickFor, PickerMessage,
    Picking, Pins, Place, PlaceId, PlaceMessage, Quoted, Root, TalkMessage, Title, Typers, Window,
    WindowMessage as Message,
};
use terms::MemberId;
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;
/// A run of a header's cells.
type Cells<'a, R> = Vec<Part<'a, R>>;

/// The pictures the host holds, by their file: the server's emoji, a message's thumbnails, a file sent.
pub type Handles<'a> = dyn Fn(FileId) -> Option<image::Handle> + 'a;

/// Where a built row sits in the list: the spaces that stand for the rows not built, the channel's start, or a row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Room,
    Start,
    Loading,
    Before,
    Row(RowKey),
    After,
}

/// What every part of the view reads: the open channel, the pane it is drawn in and the room at its sides.
#[derive(Clone, Copy)]
struct Shown<'a> {
    window: &'a Window,
    place: &'a Place,
    open: &'a Discussion,
    pane: Pane,
    side: f32,
    /// The pane is folded away with the server bar: a magnifier in the header opens the search.
    folded: bool,
    handles: &'a Handles<'a>,
    words: &'a Words,
}

impl Shown<'_> {
    fn said(self, message: DiscussionMessage) -> Message {
        said(self.place.id(), self.pane, message)
    }
}

/// `message` for the discussion `pane` names.
fn said(id: PlaceId, pane: Pane, message: DiscussionMessage) -> Message {
    let message = match pane {
        Pane::Content => PlaceMessage::Discussion(message),
        Pane::CallChat => PlaceMessage::CallChat(message),
        Pane::Thread => PlaceMessage::Thread(message),
    };
    Message::At(id, message)
}

/// What stands around a channel in the content area, which its header answers.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Around {
    /// The members column shows.
    pub(crate) members: bool,
    /// The channels are folded away, so the header opens them.
    pub(crate) folded: bool,
    /// The thread beside the channel covers it where the room is too narrow for both: its back arrow closes it.
    pub(crate) covers: bool,
}

/// The text channel `open` at `place`, drawn in `pane` `size` wide: in the content area with the members column's
/// toggle and what is `around` it, or beside a call with its way out. `handles` gives the pictures the host holds.
#[expect(clippy::too_many_arguments, reason = "each is a separate fact of the window the view is drawn from")]
pub(crate) fn view<'a, R: Renderer + 'static>(
    window: &'a Window,
    place: &'a Place,
    open: &'a Discussion,
    pane: Pane,
    size: Size,
    around: Around,
    handles: &'a Handles<'a>,
    words: &'a Words,
) -> Part<'a, R> {
    let side = screens::frame::message_side(pane, size.width);
    let shown = Shown { window, place, open, pane, side, folded: around.folded, handles, words };
    let choosing = place.choosing(&open.compose);
    let mut layers: Vec<Part<'a, R>> = vec![list(shown)];
    if let Some(choosing) = &choosing {
        layers.push(chooser_layer(shown, choosing));
    }
    let composer_picking = open.picking.as_ref().filter(|picking| picking.target == PickFor::Composer);
    let bottom = crate::menu::menu_hung(
        bottom(shown),
        composer_picking.map(|picking| picker_layer(shown, picking)),
        shown.said(DiscussionMessage::Picker(PickerMessage::Close)),
    );
    let lower = Column::with_children([stack(layers).width(Length::Fill).height(Length::Fill)._boxed(), bottom])
        .height(Length::Fill);
    let keys = choosing.as_ref().map(|_| ChooserKeys {
        up: shown.said(DiscussionMessage::ChoosePrevious),
        down: shown.said(DiscussionMessage::ChooseNext),
        choose: shown.said(DiscussionMessage::Choose(None)),
        escape: shown.said(DiscussionMessage::Escape),
    });
    let lower = crate::chooser_keys(lower._boxed(), keys);
    let column = Column::with_children([
        header(shown, around, (size.height - PANE_HEADER_HEIGHT - 2.0 * Space::S.px()).max(0.0)),
        crate::panel_parts::hairline(),
        lower,
    ])
    .height(Length::Fill);
    let dropping = window.drop_target().filter(|target| target.pane == pane).map(|target| words.say(&target.note));
    let column = crate::drop_over(column._boxed(), dropping, words);
    let menu = open.menu.as_ref().and_then(|chosen| {
        let message = open.timeline()?.get(chosen)?;
        Some(menu_items::<Message, R>(
            crate::labels::label(&author_name(place, &message.author, words)),
            menu_entries(shown, message),
        ))
    });
    let keyed = iced_core::Vector::new(side, PANE_HEADER_HEIGHT + Space::L.px());
    let column = menu_at_pointer(column, menu, keyed, shown.said(DiscussionMessage::CloseMenu));
    // A message's picker opens where it was asked for, by the row's button, its chips' or its menu's, and is kept
    // inside the channel.
    let reacting = open.picking.as_ref().filter(|picking| matches!(picking.target, PickFor::Message(_)));
    let picker = reacting.map(|picking| picker_layer(shown, picking));
    menu_at_pointer(column, picker, keyed, shown.said(DiscussionMessage::Picker(PickerMessage::Close)))
}

/// A message's menu: reply, copy, edit, pin or unpin and delete, each where this account may.
fn menu_entries(shown: Shown<'_>, message: &chat::Message) -> Vec<MenuEntry<Message>> {
    let (place, words) = (shown.place, shown.words);
    let entry = |phrase: Phrase, message: DiscussionMessage, rule_before: bool, danger: bool| MenuEntry {
        label: crate::labels::phrase(phrase, words),
        message: shown.said(message),
        danger,
        rule_before,
        icon: None,
    };
    let id = &message.id;
    let mut entries = Vec::new();
    if place.may_send(shown.pane) {
        entries.push(entry(Phrase::Reply, DiscussionMessage::Reply(id.clone()), false, false));
    }
    let may_thread = place.offers_threads() && shown.pane == Pane::Content && place.may_send(shown.pane);
    if may_thread && matches!(shown.open.owner, Owner::Channel(_)) && !message.deleted() {
        entries.push(MenuEntry {
            label: crate::labels::phrase(Phrase::ReplyInThread, words),
            message: Message::At(place.id(), PlaceMessage::Talk(TalkMessage::StartThread(id.clone()))),
            danger: false,
            rule_before: false,
            icon: None,
        });
    }
    if place.may_react(shown.pane) {
        entries.push(entry(Phrase::React, picker_for(PickFor::Message(id.clone())), false, false));
    }
    entries.push(entry(Phrase::CopyText, DiscussionMessage::Copy(id.clone()), false, false));
    if place.may_edit(shown.pane, message) {
        entries.push(entry(Phrase::Edit, DiscussionMessage::Edit(id.clone()), true, false));
    }
    if place.may_pin(shown.pane, message) {
        entries.push(match message.pinned_at {
            Some(_) => entry(Phrase::UnpinMessage, DiscussionMessage::Unpin(id.clone()), true, false),
            None => entry(Phrase::PinMessage, DiscussionMessage::Pin(id.clone()), true, false),
        });
    }
    if place.may_delete(shown.pane, message) {
        entries.push(entry(Phrase::Delete, DiscussionMessage::Delete(id.clone()), true, true));
    }
    entries
}

/// The discussion's header: a channel's mark and name, then in the content area its pins; a conversation's person; a
/// thread's title with its channel, its bell and its way out. Before them the way back to the list it was opened from,
/// or to the channels where they are folded away, and beside a call the way to close it. Where folded, the search and
/// the members column's toggle end the header of a channel or a conversation. A channel's pins panel is no taller
/// than `most`.
fn header<'a, R: Renderer + 'a>(shown: Shown<'a>, around: Around, most: f32) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let id = place.id();
    let back = |press: Message| {
        Button::new(crate::labels::phrase(Phrase::Back, words))
            .icon_only(Icon::ArrowLeft)
            .variant(ButtonVariant::Ghost)
            .compact()
            .on_press(press)
            .view::<R>()
    };
    let channels = around.folded.then(|| crate::bar_controls::channels_button::<R>(words));
    let list_back = open.from.map(|_| back(Message::At(id, PlaceMessage::Talk(TalkMessage::Back))));
    let (lead, start, end): (Cells<'a, R>, Cells<'a, R>, Cells<'a, R>) = match (shown.pane, place.title(open)) {
        (Pane::CallChat, title) => {
            let close = Button::new(crate::labels::phrase(Phrase::CloseChat, words))
                .icon_only(Icon::Close)
                .variant(ButtonVariant::Quiet)
                .compact()
                .on_press(Message::CloseCallChat)
                .view::<R>();
            let start = vec![
                icon(Icon::MessageSquare, IconSize::Bar, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
                crate::typography::line(called(&title, words), TypeStyle::BodyStrong).width(Length::Fill)._boxed(),
            ];
            (Vec::new(), start, vec![close])
        }
        (_, Title::Person(name)) => {
            let name = name.unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned());
            let held = person_picture(place, open, shown.handles);
            let picture = avatar_pictured(crate::labels::label(&name), person_kind(place, open), AvatarSize::S, held);
            let start =
                vec![picture, crate::typography::line(name, TypeStyle::BodyStrong).width(Length::Fill)._boxed()];
            (channels.into_iter().chain(list_back).collect(), start, folded_end(shown, around))
        }
        (pane, Title::Thread { channel }) => {
            let way_back = match pane {
                Pane::Thread if around.covers => {
                    Some(back(Message::At(id, PlaceMessage::Talk(TalkMessage::CloseThread))))
                }
                Pane::Thread | Pane::Content | Pane::CallChat => list_back,
            };
            // A thread yet to begin has no timeline to follow.
            let bell = open.timeline_id().map(|timeline| {
                let follows = place.follows(timeline);
                let (mark, said) =
                    if follows { (Icon::BellOff, Phrase::UnfollowThread) } else { (Icon::Bell, Phrase::FollowThread) };
                Button::new(crate::labels::phrase(said, words))
                    .icon_only(mark)
                    .variant(ButtonVariant::Ghost)
                    .compact()
                    .on_press(Message::At(
                        id,
                        PlaceMessage::Talk(TalkMessage::Follow { timeline: timeline.clone(), follow: !follows }),
                    ))
                    .view::<R>()
            });
            let titles = Column::with_children(
                std::iter::once(
                    crate::typography::line(words.phrase(Phrase::Thread).into_owned(), TypeStyle::BodyStrong)._boxed(),
                )
                .chain(channel.map(|channel| {
                    coloured(format!("#{channel}"), TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed()
                })),
            )
            .width(Length::Fill);
            let mut end: Vec<Part<'a, R>> = bell.into_iter().collect();
            if pane == Pane::Thread && !around.covers {
                end.push(
                    Button::new(crate::labels::phrase(Phrase::Close, words))
                        .icon_only(Icon::Close)
                        .variant(ButtonVariant::Ghost)
                        .compact()
                        .on_press(Message::At(id, PlaceMessage::Talk(TalkMessage::CloseThread)))
                        .view::<R>(),
                );
            }
            (channels.into_iter().chain(way_back).collect(), vec![titles._boxed()], end)
        }
        (_, Title::Channel(name)) => {
            let name = crate::typography::line(name, TypeStyle::BodyStrong).width(Length::Fill)._boxed();
            let sight = open
                .owner
                .channel()
                .and_then(|channel| place.private_line(channel))
                .map_or(screens::Sight::Everyone, |_| screens::Sight::Private);
            let pins = Button::new(crate::labels::phrase(Phrase::PinnedMessages, words))
                .icon_only(Icon::Pin)
                .variant(ButtonVariant::Ghost)
                .compact()
                .opens(Popup::Dialog, open.pins.is_some())
                .on_press(shown.said(DiscussionMessage::TogglePins))
                .view::<R>();
            let panel = open.pins.as_ref().map(|pins| pins_layer(shown, pins, most));
            let pins = crate::menu::menu_hung(pins, panel, shown.said(DiscussionMessage::TogglePins));
            let start = vec![crate::channel_list::kind_icon(Icon::Hash, Icon::HashLock, sight, words), name];
            let mut end = vec![pins];
            end.extend(folded_end(shown, around));
            (channels.into_iter().chain(list_back).collect(), start, end)
        }
    };
    crate::bar_controls::header(lead, start, end)
}

/// What ends the header of a channel or a conversation in the content area where the bar is folded away with the
/// pane: the search's magnifier, then the members column's toggle; none otherwise, the bar having them.
fn folded_end<'a, R: Renderer + 'a>(shown: Shown<'a>, around: Around) -> Vec<Part<'a, R>> {
    if shown.pane != Pane::Content || !shown.folded {
        return Vec::new();
    }
    crate::bar_controls::folded_end(shown.place, around.members, shown.words)
}

/// What a discussion is called where a sentence names it: the channel, the person (a former member without a name) or
/// the thread's channel.
fn called(title: &Title, words: &Words) -> String {
    match title {
        Title::Channel(name) => name.clone(),
        Title::Person(name) => name.clone().unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned()),
        Title::Thread { channel } => channel.clone().unwrap_or_default(),
    }
}

/// The picture the other person in a conversation chose, as the host holds it.
fn person_picture(place: &Place, open: &Discussion, handles: &Handles<'_>) -> Option<crate::Held> {
    let Owner::Conversation(conversation) = &open.owner else { return None };
    match place.conversation(conversation).map(|conversation| &conversation.with) {
        Some(screens::With::Member(user_id)) => place.picture_of(user_id).and_then(handles),
        Some(screens::With::Gone { .. }) | None => None,
    }
}

/// The look of the other person's picture in a conversation: their colour while they are on the server.
fn person_kind(place: &Place, open: &Discussion) -> AvatarKind {
    let Owner::Conversation(conversation) = &open.owner else { return AvatarKind::Person };
    match place.conversation(conversation).map(|conversation| &conversation.with) {
        Some(screens::With::Member(user_id)) => AvatarKind::Called(screens::colour_of(
            user_id,
            place.directory().person(user_id).and_then(|person| person.colour),
        )),
        Some(screens::With::Gone { .. }) | None => AvatarKind::Person,
    }
}

/// The pinned messages' panel, hung under the header's pins and no taller than `most`: Escape and a press outside it
/// put it away as they do a menu.
fn pins_layer<'a, R: Renderer + 'a>(shown: Shown<'a>, pins: &'a Pins, most: f32) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let cards = pins
        .pinned
        .items()
        .iter()
        .filter_map(|message| {
            let body = message.body.as_ref()?;
            Some(Pin {
                name: author_name(place, &message.author, words),
                avatar: author_avatar(place, &message.author),
                picture: author_picture(place, &message.author, shown.handles),
                at: chat::local(message.created_at, open.zone())
                    .map(|at| words.say(&Sentence::PinnedAt { at, today: shown.window.today() }))
                    .unwrap_or_default(),
                text: unmarked(body, false),
                body: if body.text().is_empty() {
                    files_alone(words)
                } else {
                    body_of(shown, message, body, MessageSize::DEFAULT)
                },
                jump: shown.said(DiscussionMessage::Jump(message.place)),
                unpin: place
                    .may_pin(shown.pane, message)
                    .then(|| shown.said(DiscussionMessage::Unpin(message.id.clone()))),
            })
        })
        .collect();
    let more = pins.pinned.more().then(|| shown.said(DiscussionMessage::MorePins));
    crate::pins_panel::paged_pins_panel(cards, most, more, words)
}

/// What a chooser's matches are grouped under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Heading {
    Members,
    Roles,
    Everyone,
    Channels,
    Emoji,
}

impl Heading {
    fn of(choice: &Choice) -> Heading {
        match choice {
            Choice::Member { .. } => Heading::Members,
            Choice::Role { .. } => Heading::Roles,
            Choice::Everyone | Choice::Here => Heading::Everyone,
            Choice::Channel { .. } => Heading::Channels,
            Choice::Custom(_) | Choice::Unicode(_) => Heading::Emoji,
        }
    }

    fn said(self, query: &chat::Query, words: &Words) -> String {
        match self {
            Heading::Members => words.phrase(Phrase::ChooserMembers).into_owned(),
            Heading::Roles => words.phrase(Phrase::ChooserRoles).into_owned(),
            Heading::Everyone => words.phrase(Phrase::ChooserEveryone).into_owned(),
            Heading::Channels => words.phrase(Phrase::ChooserChannels).into_owned(),
            Heading::Emoji => words.say(&Sentence::EmojiMatching(query.text.clone())),
        }
    }
}

/// The chooser over the composer, at the list's foot, its matches grouped as Discord's.
fn chooser_layer<'a, R: Renderer + 'a>(shown: Shown<'a>, choosing: &Choosing) -> Part<'a, R> {
    let words = shown.words;
    let rows: Vec<(Heading, ChooserRow<Message>)> = choosing
        .choices
        .iter()
        .enumerate()
        .map(|(at, choice)| (Heading::of(choice), chooser_row(shown, at, choice)))
        .collect();
    let mut groups: Vec<ChooserGroup<Message>> = Vec::new();
    let mut last = None;
    for (heading, row) in rows {
        match groups.last_mut().filter(|_| last == Some(heading)) {
            Some(group) => group.rows.push(row),
            None => groups.push(ChooserGroup {
                heading: crate::labels::label(&heading.said(&choosing.query, words)),
                rows: vec![row],
            }),
        }
        last = Some(heading);
    }
    let label = match choosing.query.trigger {
        chat::Trigger::Member => crate::labels::phrase(Phrase::Mentions, words),
        chat::Trigger::Channel => crate::labels::phrase(Phrase::ChooserChannels, words),
        chat::Trigger::Emoji => crate::labels::label(&Heading::Emoji.said(&choosing.query, words)),
    };
    container(crate::chooser(label, groups, choosing.selected))
        .padding(Padding { left: shown.side, ..Padding::ZERO })
        .align_bottom(Length::Fill)
        ._boxed()
}

fn chooser_row(shown: Shown<'_>, at: usize, choice: &Choice) -> ChooserRow<Message> {
    let words = shown.words;
    let (leading, name, secondary) = match choice {
        Choice::Member { id, name, colour } => {
            let picture = shown.place.picture_of(id).and_then(shown.handles);
            (ChooserLeading::Person(screens::colour_of(id, *colour), picture), name.clone(), None)
        }
        Choice::Role { name, colour, .. } => {
            (ChooserLeading::Role(colour.map(screens::colour::rgb)), format!("@{name}"), None)
        }
        Choice::Everyone => (
            ChooserLeading::Everyone,
            "@everyone".to_owned(),
            Some(words.say(&Sentence::EveryoneNote(called(&shown.place.title(shown.open), words)))),
        ),
        Choice::Here => {
            (ChooserLeading::Everyone, "@here".to_owned(), Some(words.phrase(Phrase::HereNote).into_owned()))
        }
        Choice::Channel { name, voice, category, .. } => {
            let leading = if *voice { ChooserLeading::VoiceChannel } else { ChooserLeading::TextChannel };
            (leading, name.clone(), category.clone())
        }
        Choice::Custom(emoji) => {
            let face =
                ReactionEmoji::Custom { name: emoji.name.as_str().to_owned(), picture: (shown.handles)(emoji.file) };
            (ChooserLeading::Emoji(face), emoji.written(), Some(shown.place.server().shown().to_string()))
        }
        Choice::Unicode(emoji) => {
            let face = ReactionEmoji::Unicode(emoji.as_str().to_owned());
            (ChooserLeading::Emoji(face), format!(":{}:", emoji.shortcode().unwrap_or(emoji.name())), None)
        }
    };
    ChooserRow {
        leading,
        name: crate::labels::label(&name),
        secondary,
        choose: shown.said(DiscussionMessage::Choose(Some(at))),
    }
}

fn list<'a, R: Renderer + 'static>(shown: Shown<'a>) -> Part<'a, R> {
    let (window, place, open, words) = (shown.window, shown.place, shown.open, shown.words);
    let (id, pane) = (place.id(), shown.pane);
    let rows = open.rows();
    let built = open.window();
    let scale = window.look().messages;
    let mut slots: Vec<(Slot, Part<'a, R>)> = Vec::with_capacity(built.end - built.first + 4);
    // A short channel stands on its newest end, as every peer app draws one.
    slots.push((Slot::Room, Gap::new().height(open.room_above())._boxed()));
    // What stands above the rows says its height, so the room above leaves it out and a short list does not scroll.
    let head = move |drawn: Part<'a, R>| {
        sensor(drawn)
            .on_resize(move |size: Size| said(id, pane, DiscussionMessage::HeadMeasured { height: size.height }))
            ._boxed()
    };
    if open.shows_start() {
        slots.push((Slot::Start, head(start(shown))));
    }
    if open.reading_older() {
        let loading = coloured(
            words.phrase(Phrase::LoadingOlder).into_owned(),
            TypeStyle::CaptionStrong,
            TextClass::Content(Content::Secondary),
        );
        slots.push((Slot::Loading, head(container(loading).center_x(Length::Fill).padding(Space::S.px())._boxed())));
    }
    slots.push((Slot::Before, Gap::new().height(built.before)._boxed()));
    let first_unread = open.first_unread(place.me());
    for row in rows.get(built.first..built.end).unwrap_or_default() {
        let key = row.key();
        let measured = key.clone();
        let drawn = match row {
            ListedRow::Day(day) => day_line(*day, window.today(), shown.side, scale, words),
            ListedRow::Message { message, starts_run } if first_unread == Some(message.place) => {
                Column::with_children([new_line(shown.side, words), message_row(shown, message, *starts_run, scale)])
                    ._boxed()
            }
            ListedRow::Message { message, starts_run } => message_row(shown, message, *starts_run, scale),
        };
        let measured = sensor(drawn).on_resize(move |size: Size| {
            said(id, pane, DiscussionMessage::Measured { row: measured.clone(), height: size.height })
        });
        slots.push((Slot::Row(key), measured._boxed()));
    }
    slots.push((Slot::After, Gap::new().height(built.after)._boxed()));
    let selected = move |slots: Vec<Slot>| {
        let rows = slots
            .into_iter()
            .filter_map(|slot| match slot {
                Slot::Row(row) => Some(row),
                Slot::Room | Slot::Start | Slot::Loading | Slot::Before | Slot::After => None,
            })
            .collect();
        said(id, pane, DiscussionMessage::Selected(rows))
    };
    let list = iced_core::widget::Id::new(match pane {
        Pane::Content => LIST,
        Pane::CallChat => CALL_LIST,
        Pane::Thread => THREAD_LIST,
    });
    let scrolled = scrollable(Rows::new(slots).on_selected(selected)._boxed())
        .id(list.clone())
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .anchor_bottom()
        .on_scroll(move |scroll| {
            said(
                id,
                pane,
                DiscussionMessage::Scrolled {
                    from_end: scroll.viewport.absolute_offset().y,
                    viewport: scroll.viewport.bounds.height,
                },
            )
        })
        .width(Length::Fill)
        .height(Length::Fill);
    KeepAt::new(scrolled, list, open.scrolled_up(), open.moved())._boxed()
}

/// The channel list in the content area.
const LIST: &str = "discussion";

/// The thread's list beside the channel.
const THREAD_LIST: &str = "thread";

/// The voice channel's list beside its call, which shows at once with the content's.
const CALL_LIST: &str = "call-chat";

/// The composer field's padding above and below its text.
const FIELD_PAD: f32 = Space::S.px() + Space::Xxs.px();

/// The top of a discussion whose oldest message is held: a channel or a conversation, what it is and who reads it; a
/// thread, its root over the count of its replies.
fn start<'a, R: Renderer + 'a>(shown: Shown<'a>) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let name = called(&place.title(open), words);
    let (mark, welcome, start): (Part<'a, R>, String, String) = match (&open.owner, shown.pane) {
        (Owner::Thread { .. }, _) => return thread_root(shown),
        (Owner::Conversation(_), _) => (
            avatar_pictured(
                crate::labels::label(&name),
                person_kind(place, open),
                AvatarSize::L,
                person_picture(place, open, shown.handles),
            ),
            name.clone(),
            words.say(&Sentence::ConversationStart(name)),
        ),
        (Owner::Channel(id), Pane::Content | Pane::Thread) => {
            let start = place.private_line(id).unwrap_or_else(|| Sentence::ChannelStart(name.clone()));
            (
                icon(Icon::Hash, IconSize::L, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
                words.say(&Sentence::ChannelWelcome(name)),
                words.say(&start),
            )
        }
        (Owner::Channel(_), Pane::CallChat) => (
            icon(Icon::MessageSquare, IconSize::L, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
            words.say(&Sentence::VoiceChatWelcome(name.clone())),
            words.say(&Sentence::VoiceChatStart(name)),
        ),
    };
    let column = Column::with_children([
        mark,
        text(welcome, TypeStyle::Heading)._boxed(),
        coloured(start, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xs.px());
    container(column)
        .padding(Padding { top: Space::Xl.px(), bottom: Space::L.px(), left: shown.side, right: shown.side })
        ._boxed()
}

/// The top of a thread: the message it was started on, as the thread's replies read, a rule, and how many replies
/// follow it. A root that was deleted says so.
fn thread_root<'a, R: Renderer + 'a>(shown: Shown<'a>) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let scale = shown.window.look().messages;
    let size = scale.size;
    let secondary = TextClass::Content(Content::Secondary);
    let (name, kind, held, at, body): (String, AvatarKind, Option<crate::Held>, Option<i64>, Option<Part<'a, R>>) =
        match open.root.as_ref() {
            Some(Root::Whole(message)) => (
                author_name(place, &message.author, words),
                author_avatar(place, &message.author),
                author_picture(place, &message.author, shown.handles),
                Some(message.created_at),
                message.body.as_ref().map(|body| {
                    if body.text().is_empty() { files_alone(words) } else { body_of(shown, message, body, size) }
                }),
            ),
            Some(Root::Brief { preview, .. }) => {
                let from = preview.as_ref().and_then(|brief| brief.from.as_ref());
                (
                    from.and_then(|user_id| place.directory().person(user_id).map(|person| person.name.clone()))
                        .unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned()),
                    from.map_or(AvatarKind::Person, |user_id| {
                        AvatarKind::Called(screens::colour_of(
                            user_id,
                            place.directory().person(user_id).and_then(|person| person.colour),
                        ))
                    }),
                    from.and_then(|user_id| place.picture_of(user_id)).and_then(shown.handles),
                    preview.as_ref().map(|brief| brief.at),
                    preview.as_ref().map(|brief| message_text(brief.text.clone(), TypeStyle::Message, size)._boxed()),
                )
            }
            None => (words.phrase(Phrase::FormerMember).into_owned(), AvatarKind::Person, None, None, None),
        };
    let stamp = at.and_then(|at| chat::local(at, open.zone())).map(|at| words.clock(at)).unwrap_or_default();
    let body = body.unwrap_or_else(|| {
        message_text(words.phrase(Phrase::MessageDeleted).into_owned(), TypeStyle::Message, size)
            .class(secondary)
            ._boxed()
    });
    let gutter = avatar_pictured(crate::labels::label(&name), kind, AvatarSize::Author, held);
    let lines = vec![author_line(message_text(name, TypeStyle::Author, size)._boxed(), stamp, size), body];
    let root = message_frame(Some(gutter), lines, shown.side, scale);
    let replies = open.rows().iter().filter(|row| matches!(row, ListedRow::Message { .. })).count();
    let count = (replies > 0).then(|| {
        let said = words.say(&Sentence::Replies(u32::try_from(replies).unwrap_or(u32::MAX)));
        container(coloured(said, TypeStyle::Small, secondary))
            .padding(Padding { left: shown.side, right: shown.side, top: Space::S.px(), ..Padding::ZERO })
            ._boxed()
    });
    let rule = container(crate::panel_parts::hairline()).padding(Padding {
        left: shown.side,
        right: shown.side,
        top: Space::S.px(),
        ..Padding::ZERO
    });
    Column::with_children([root._boxed(), rule._boxed()].into_iter().chain(count))._boxed()
}

fn day_line<'a, R: Renderer + 'a>(
    day: chat::Date,
    today: Option<chat::Date>,
    side: f32,
    scale: MessageScale,
    words: &'a Words,
) -> Part<'a, R> {
    let said = words.say(&Sentence::Day { day, today });
    let line = Row::with_children([
        crate::panel_parts::hairline(),
        coloured(said, TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary))._boxed(),
        crate::panel_parts::hairline(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    container(line).padding(Padding { top: scale.gap.space(), bottom: 0.0, left: side, right: side })._boxed()
}

/// The name a message's author is shown by: the directory's, else a former member.
pub(crate) fn author_name(place: &Place, author: &Author, words: &Words) -> String {
    match author {
        Author::Member(user_id) => place.directory().person(user_id).map(|person| person.name.clone()),
        Author::App { name, .. } => Some(name.clone()),
        Author::Former(_) => None,
    }
    .unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned())
}

/// The name over a message of the discussion shown.
fn shown_name(shown: Shown<'_>, author: &Author) -> String {
    let known = shown.place.author_shown(shown.open, author);
    known.unwrap_or_else(|| shown.words.phrase(Phrase::FormerMember).into_owned())
}

/// The colour a message's author is shown in: the one they picked, else their id's; a former member keeps their id's.
pub(crate) fn author_colour(place: &Place, author: &Author) -> PersonColour {
    match author {
        Author::Member(user_id) => {
            screens::colour_of(user_id, place.directory().person(user_id).and_then(|person| person.colour))
        }
        Author::App { .. } => PersonColour::Swatch(design::Swatch::ALL[0]),
        Author::Former(id) => MemberId::parse(id)
            .map_or(PersonColour::Swatch(design::Swatch::ALL[0]), |user_id| screens::colour_of(&user_id, None)),
    }
}

/// The picture a message's author chose, as the host holds it: a member's, once it is ready; none for an app, a former
/// member, or one who chose none.
pub(crate) fn author_picture(place: &Place, author: &Author, handles: &Handles<'_>) -> Option<crate::Held> {
    match author {
        Author::Member(user_id) => place.picture_of(user_id).and_then(handles),
        Author::App { .. } | Author::Former(_) => None,
    }
}

/// How a message's author's avatar is drawn wherever the message is: a member on their colour, an app or a former
/// member on no one's.
pub(crate) fn author_avatar(place: &Place, author: &Author) -> AvatarKind {
    match author {
        Author::Member(_) => AvatarKind::Called(author_colour(place, author)),
        Author::App { .. } | Author::Former(_) => AvatarKind::Person,
    }
}

/// A body's words without markdown's markers, a hidden spoiler left out; on one line where `one_line`.
/// A message's words on one line where it is quoted, or that it is files alone.
fn one_line(body: &Body, words: &Words) -> String {
    if body.text().is_empty() { words.phrase(Phrase::FilesAlone).into_owned() } else { unmarked(body, true) }
}

/// What stands for the words of a message that is files alone, where only its words are drawn.
pub(crate) fn files_alone<'a, R: Renderer + 'a>(words: &Words) -> Part<'a, R> {
    message_text(words.phrase(Phrase::FilesAlone).into_owned(), TypeStyle::Message, MessageSize::DEFAULT)
        .class(TextClass::Content(Content::Secondary))
        ._boxed()
}

pub(crate) fn unmarked(body: &Body, one_line: bool) -> String {
    let words: String = body.runs().filter(|run| run.style.spoiler.is_none()).map(|run| run.text).collect();
    if one_line { words.split_whitespace().collect::<Vec<_>>().join(" ") } else { words }
}

fn message_row<'a, R: Renderer + 'static>(
    shown: Shown<'a>,
    message: &'a chat::Message,
    starts_run: bool,
    scale: MessageScale,
) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let id = place.id();
    let size = scale.size;
    let name = shown_name(shown, &message.author);
    let at = chat::local(message.created_at, open.zone()).map(|at| words.clock(at)).unwrap_or_default();
    let mut lines: Vec<Part<'a, R>> = Vec::new();
    // The author's name and picture open their popover, or an app's About, under the name.
    let person = match &message.author {
        Author::Member(user_id) => Some(Message::Person(id, user_id.clone(), screens::PersonFrom::Author)),
        Author::App { .. } => Some(Message::AppAbout(id, message.id.clone())),
        Author::Former(_) => None,
    };
    let opens = |shown: Part<'a, R>, named: &str| -> Part<'a, R> {
        match person.clone() {
            Some(open) => crate::button::opening(shown, open, crate::labels::label(named)),
            None => shown,
        }
    };
    if starts_run {
        let author = opens(message_text(name.clone(), TypeStyle::Author, size)._boxed(), &name);
        // An app's name is followed by the badge that says it is one.
        let author = match &message.author {
            Author::App { .. } => {
                Row::with_children([author, crate::role_badge(&words.phrase(Phrase::AppBadge), None)])
                    .spacing(Space::S.px())
                    .align_y(Alignment::Center)
                    ._boxed()
            }
            Author::Member(_) | Author::Former(_) => author,
        };
        lines.push(author_line(author, at.clone(), size));
    }
    let editing = open.editing.as_ref().filter(|editing| editing.id == message.id);
    lines.extend(match (editing, &message.body) {
        (Some(editing), _) => Some(edit(shown, &editing.draft)),
        (None, None) => Some(
            message_text(words.phrase(Phrase::MessageDeleted).into_owned(), TypeStyle::Message, size)
                .class(TextClass::Content(Content::Secondary))
                ._boxed(),
        ),
        (None, Some(body)) if body.text().is_empty() => None,
        (None, Some(body)) => Some({
            let words_of = body_of(shown, message, body, size);
            if message.edited() {
                let edited = message_text(words.phrase(Phrase::Edited).into_owned(), TypeStyle::Stamp, size)
                    .class(TextClass::Content(Content::Secondary));
                Row::with_children([words_of, edited._boxed()])
                    .spacing(Space::Xs.px())
                    .align_y(Alignment::End)
                    .wrap()
                    ._boxed()
            } else {
                words_of
            }
        }),
    });
    lines.extend(attachments(shown, message));
    lines.extend(preview_card(shown, message));
    lines.extend(reaction_row(shown, message));
    lines.extend(thread_row(shown, message));
    let gutter = starts_run.then(|| {
        let held = author_picture(place, &message.author, shown.handles);
        let picture = avatar_pictured(
            crate::labels::label(&name),
            author_avatar(place, &message.author),
            AvatarSize::Author,
            held,
        );
        match person.clone() {
            Some(open) => crate::button::pointer_press(picture, open),
            None => picture,
        }
    });
    let quoted = message.reply.as_ref().filter(|_| !message.deleted()).and_then(|reply| quoted_line(shown, reply));
    let padded = framed(gutter, lines, quoted, shown.side, scale);
    if message.deleted() {
        return padded._boxed();
    }
    let layer = hover_layer(shown, message, starts_run, at, size);
    let picking = open.picking.as_ref().is_some_and(|picking| picking.target == PickFor::Message(message.id.clone()));
    let held = open.menu.as_ref() == Some(&message.id) || picking;
    let called = if place.mentions_me(message) { Called::Reader } else { Called::Others };
    let hovered = Shade::new(crate::reveal(padded._boxed(), layer, held), held, called);
    mouse_area(hovered).on_right_press(shown.said(DiscussionMessage::Menu(message.id.clone())))._boxed()
}

/// The line over a reply: the message it answers, a press going to it, or that it was deleted; none while it is read.
fn quoted_line<'a, R: Renderer + 'a>(shown: Shown<'a>, reply: &chat::Reply) -> Option<Part<'a, R>> {
    let quoted = match shown.open.quote(reply)? {
        Quoted::Message(message) => crate::Quoted::Live {
            name: shown_name(shown, &message.author),
            colour: author_colour(shown.place, &message.author),
            picture: author_picture(shown.place, &message.author, shown.handles),
            snippet: message.body.as_ref().map(|body| one_line(body, shown.words)).unwrap_or_default(),
        },
        Quoted::Deleted => crate::Quoted::Deleted,
    };
    let jump = shown.said(DiscussionMessage::Jump(reply.place));
    Some(crate::quoted_reply(QuotedReply { quoted, jump }, shown.words))
}

/// What shows over a hovered message: its time in the gutter where its run's first line does not say it, and its
/// tools at its top right.
fn hover_layer<'a, R: Renderer + 'a>(
    shown: Shown<'a>,
    message: &chat::Message,
    starts_run: bool,
    at: String,
    size: MessageSize,
) -> Part<'a, R> {
    let tools = HoverTools {
        react: shown.place.may_react(shown.pane).then(|| shown.said(picker_for(PickFor::Message(message.id.clone())))),
        reply: shown.place.may_send(shown.pane).then(|| shown.said(DiscussionMessage::Reply(message.id.clone()))),
        more: shown.said(DiscussionMessage::Menu(message.id.clone())),
        menu_open: shown.open.menu.as_ref() == Some(&message.id),
    };
    let gutter: Part<'a, R> = if starts_run {
        Gap::new().width(Length::Fixed(AvatarSize::Author.px()))._boxed()
    } else {
        let stamp = message_text(at, TypeStyle::Stamp, size).class(TextClass::Content(Content::Secondary));
        container(stamp)
            .center_x(Length::Fixed(AvatarSize::Author.px()))
            .center_y(Length::Fixed(size.line_height(TypeStyle::Message)))
            ._boxed()
    };
    let row =
        Row::with_children([gutter, Gap::new().width(Length::Fill)._boxed(), crate::hover_bar(tools, shown.words)])
            .align_y(Alignment::Start);
    container(row)
        .padding(Padding { top: MESSAGE_PAD, bottom: 0.0, left: shown.side, right: shown.side })
        .width(Length::Fill)
        .align_top(Length::Fill)
        ._boxed()
}

/// A message's pictures as thumbnails, a press opening one, and its other files as cards with Save.
fn attachments<'a, R: Renderer + 'a>(shown: Shown<'a>, message: &'a chat::Message) -> Vec<Part<'a, R>> {
    let (pictures, files): (Vec<_>, Vec<_>) =
        message.attachments.iter().partition(|file| file.picture.and_then(|picture| picture.thumbnail).is_some());
    let mut parts = Vec::new();
    if !pictures.is_empty() {
        let thumbnails = pictures
            .into_iter()
            .map(|file| crate::Picture {
                thumbnail: file
                    .picture
                    .and_then(|picture| picture.thumbnail)
                    .and_then(|thumbnail| (shown.handles)(thumbnail)),
                label: crate::labels::label(file.name.as_str()),
                open: shown.said(DiscussionMessage::View { message: message.id.clone(), file: file.file }),
            })
            .collect();
        parts.push(crate::pictures(thumbnails));
    }
    parts.extend(files.into_iter().map(|file| {
        let card = FileCard {
            name: file.name.as_str().to_owned(),
            note: shown.words.say(&Sentence::FileNote { size: file.size, name: file.name.as_str().to_owned() }),
            save: shown.said(DiscussionMessage::Save { file: file.file, name: file.name.clone() }),
        };
        crate::file_card(card, shown.words)
    }));
    parts
}

/// The preview of a message's link as a card under it, a press opening the link as a link in its text does; none
/// for a message without one. Its picture keeps its box before the thumbnail is held, so the row never changes height.
fn preview_card<'a, R: Renderer + 'a>(shown: Shown<'a>, message: &'a chat::Message) -> Option<Part<'a, R>> {
    let card = message.preview.as_ref()?;
    let picture = card.picture.map(|picture| crate::CardPicture {
        width: picture.width,
        height: picture.height,
        thumbnail: (shown.handles)(picture.thumbnail),
    });
    Some(crate::link_card(crate::LinkCard {
        host: card.host.clone(),
        title: card.title.clone(),
        description: card.description.clone(),
        picture,
        label: crate::link_preview::card_label(&card.title, &card.host, shown.words),
        open: Message::Clicked(shown.place.id(), Clicked::Link(card.url.clone())),
    }))
}

/// A message's reactions as chips, this account's own ringed, a press adding or taking back its own; none without.
fn reaction_row<'a, R: Renderer + 'a>(shown: Shown<'a>, message: &chat::Message) -> Option<Part<'a, R>> {
    let chips: Vec<ReactionChip<Message>> = message
        .reactions
        .iter()
        .filter_map(|reaction| {
            let emoji = match &reaction.emoji {
                chat::ReactionEmoji::Unicode(glyph) => ReactionEmoji::Unicode(glyph.clone()),
                chat::ReactionEmoji::Custom(id) => {
                    let custom = shown.place.emoji().get(id)?;
                    ReactionEmoji::Custom {
                        name: custom.name.as_str().to_owned(),
                        picture: (shown.handles)(custom.file),
                    }
                }
            };
            Some(ReactionChip {
                emoji,
                count: reaction.count,
                mine: shown.open.reacted(message, &reaction.emoji),
                toggle: shown.said(DiscussionMessage::Reaction(message.id.clone(), reaction.emoji.clone())),
            })
        })
        .collect();
    if chips.is_empty() {
        return None;
    }
    let add = shown.place.may_react(shown.pane).then(|| {
        (
            crate::labels::phrase(Phrase::AddReaction, shown.words),
            shown.said(picker_for(PickFor::Message(message.id.clone()))),
        )
    });
    let row = container(crate::reactions(chips, add, shown.words));
    Some(row.padding(Padding { top: Space::Xs.px(), ..Padding::ZERO })._boxed())
}

/// The chip under a message with a thread: how many replies and when the last came, a press opening the thread
/// beside the channel; none where the message has no thread, no reply is left in it or the thread is what is open.
fn thread_row<'a, R: Renderer + 'a>(shown: Shown<'a>, message: &chat::Message) -> Option<Part<'a, R>> {
    let thread = message
        .thread
        .as_ref()
        .filter(|thread| shown.pane == Pane::Content && !message.deleted() && thread.replies > 0)?;
    let (place, words) = (shown.place, shown.words);
    let mut said = words.say(&Sentence::Replies(thread.replies));
    let last = thread.last_reply_at.and_then(|at| chat::local(at, shown.open.zone()));
    if let Some(at) = last {
        said = format!("{said} · {}", words.say(&Sentence::LastReply { at, today: shown.window.today() }));
    }
    let standing = place.thread_standing(&thread.timeline);
    let open = Message::At(place.id(), PlaceMessage::Talk(TalkMessage::OpenThread(message.id.clone())));
    let chip =
        crate::thread_chip(said.clone(), &crate::labels::label(&said), standing.followed && standing.unread > 0, open);
    Some(container(chip).padding(Padding { top: Space::Xs.px(), ..Padding::ZERO })._boxed())
}

/// A message's words at `size`, its roles in the place's colours and the server's emoji as their pictures; a mark,
/// a link or a hidden spoiler pressed asks for what it names.
fn body_of<'a, R: Renderer + 'static>(
    shown: Shown<'a>,
    message: &'a chat::Message,
    body: &'a Body,
    size: MessageSize,
) -> Part<'a, R> {
    let (place, handles) = (shown.place, shown.handles);
    let id = place.id();
    let message_id = message.id.clone();
    crate::RichBody::new(body, size, shown.words)
        .roles(move |role| {
            let listed = place.role_list().roles().iter().find(|listed| &listed.id == role)?;
            listed.colour.map(screens::colour::rgb)
        })
        .emoji(move |emoji| place.emoji().get(emoji).and_then(|custom| handles(custom.file)))
        .revealed(move |spoiler| shown.open.revealed(&message.id, spoiler))
        .channels(move |channel| place.opens_channel(channel))
        .on_press(move |press| {
            Some(match press {
                BodyPress::Person(user_id) => Message::Clicked(id, Clicked::Mention(user_id)),
                BodyPress::Role(role) => Message::Clicked(id, Clicked::Role(role)),
                BodyPress::Channel(channel) => Message::Clicked(id, Clicked::Channel(channel)),
                BodyPress::Link(link) => Message::Clicked(id, Clicked::Link(link)),
                BodyPress::Spoiler(spoiler) => shown.said(DiscussionMessage::Reveal(message_id.clone(), spoiler)),
            })
        })
        .view()
}

/// A run's first line: its author's name, then the time of its first message.
pub(crate) fn author_line<'a, R: Renderer + 'a>(name: Part<'a, R>, at: String, size: MessageSize) -> Part<'a, R> {
    let stamp = message_text(at, TypeStyle::Stamp, size).class(TextClass::Content(Content::Secondary));
    Row::with_children([name, stamp._boxed()]).spacing(Space::S.px()).align_y(Alignment::Center)._boxed()
}

/// A message's row: its author's picture beside its `lines` when it starts a run, under the run gap, else the room the
/// picture takes.
pub(crate) fn message_frame<'a, R: Renderer + 'a>(
    gutter: Option<Part<'a, R>>,
    lines: Vec<Part<'a, R>>,
    side: f32,
    scale: MessageScale,
) -> impl Widget<Message, Theme, R> + 'a {
    framed(gutter, lines, None, side, scale)
}

/// [`message_frame`] with the line a reply quotes over the picture and the lines.
fn framed<'a, R: Renderer + 'a>(
    gutter: Option<Part<'a, R>>,
    lines: Vec<Part<'a, R>>,
    quoted: Option<Part<'a, R>>,
    side: f32,
    scale: MessageScale,
) -> impl Widget<Message, Theme, R> + 'a {
    let top = if gutter.is_some() { scale.gap.space() + MESSAGE_PAD } else { MESSAGE_PAD };
    let gutter = gutter.unwrap_or_else(|| Gap::new().width(Length::Fixed(AvatarSize::Author.px()))._boxed());
    let row =
        Row::with_children([gutter, Column::with_children(lines).width(Length::Fill)._boxed()]).spacing(Space::M.px());
    let shown = Column::with_children(quoted.into_iter().chain([row._boxed()]));
    container(shown).padding(Padding { top, bottom: MESSAGE_PAD, left: side, right: side }).width(Length::Fill)
}

/// One's own message edited in place, with the way out and the way to save under it.
fn edit<'a, R: Renderer + 'static>(shown: Shown<'a>, draft: &'a Draft) -> Part<'a, R> {
    let words = shown.words;
    let (id, pane) = (shown.place.id(), shown.pane);
    let field = crate::Composer::new(crate::labels::phrase(Phrase::EditMessage, words), draft.text())
        .style(TypeStyle::Message)
        .padding(Padding::from([Space::S.px(), Space::M.px()]))
        .on_change(move |text| said(id, pane, DiscussionMessage::EditDraft(text)))
        .on_submit(shown.said(DiscussionMessage::SaveEdit))
        .on_escape(shown.said(DiscussionMessage::CancelEdit))
        .view::<R>();
    let framed = container(crate::focus::take_focus(field))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Inset, Radius::M));
    let link = |phrase: Phrase, message: DiscussionMessage| {
        Button::new(crate::labels::phrase(phrase, words))
            .variant(ButtonVariant::Link)
            .small()
            .on_press(shown.said(message))
            .view::<R>()
    };
    let hint = |phrase: Phrase| {
        coloured(words.phrase(phrase).into_owned(), TypeStyle::Caption, TextClass::Content(Content::Secondary))
    };
    let under = Row::with_children([
        hint(Phrase::EditEscape)._boxed(),
        link(Phrase::EditCancel, DiscussionMessage::CancelEdit),
        hint(Phrase::EditEnter)._boxed(),
        link(Phrase::EditSave, DiscussionMessage::SaveEdit),
    ])
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center);
    Column::with_children([framed._boxed(), under._boxed()]).spacing(Space::Xs.px())._boxed()
}

/// Under the list: the way back to the newest, a refusal, what went wrong with a file or a link, who is typing, and
/// the composer or why there is none.
fn bottom<'a, R: Renderer + 'static>(shown: Shown<'a>) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let mut parts: Vec<Part<'a, R>> = Vec::new();
    if open.behind_present() {
        let jump = Button::new(crate::labels::phrase(Phrase::JumpToPresent, words))
            .variant(ButtonVariant::Secondary)
            .small()
            .on_press(shown.said(DiscussionMessage::JumpToPresent))
            .view::<R>();
        let bar = Row::with_children([
            coloured(
                words.phrase(Phrase::ViewingOlder).into_owned(),
                TypeStyle::Small,
                TextClass::Content(Content::Secondary),
            )
            .width(Length::Fill)
            ._boxed(),
            jump,
        ])
        .align_y(Alignment::Center);
        parts.push(
            container(bar)
                .padding(Padding::from([Space::Xs.px(), Space::M.px()]))
                .class(Surfaced::Framed(Surface::Raised, Radius::M))
                ._boxed(),
        );
    }
    let failed = open.refused.as_ref().map(words::failure::said).into_iter().chain(open.notice.clone());
    for sentence in failed {
        let shown = screens::Callout::new(sentence).of(Status::Danger);
        parts.push(crate::callout::<Message, R>(&shown, crate::CalloutParts::default(), words));
    }
    let mut lower: Vec<Part<'a, R>> = vec![typing(shown)];
    lower.push(if place.may_send(shown.pane) {
        composer(shown)
    } else {
        let sentence = match place.composer_gone(open) {
            Some(gone) => {
                let name = gone.name.unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned());
                words.say(&Sentence::NoLongerHere(name))
            }
            None => words.say(&Sentence::CannotSend(called(&place.title(open), words))),
        };
        container(coloured(sentence, TypeStyle::Body, TextClass::Content(Content::Secondary)))
            .center_x(Length::Fill)
            .padding(Space::M.px())
            .class(Surfaced::Framed(Surface::Inset, Radius::M))
            ._boxed()
    });
    // The notes above take one slot whether any shows, so one appearing never moves the composer and takes its focus.
    let spacing = if parts.is_empty() { 0.0 } else { Space::S.px() };
    let notes = Column::with_children(parts).spacing(Space::S.px());
    let held = Column::with_children([notes._boxed(), Column::with_children(lower).spacing(Space::Xs.px())._boxed()]);
    container(held.spacing(spacing))
        .padding(Padding { top: Space::S.px(), bottom: shown.side, left: shown.side, right: shown.side })
        .width(Length::Fill)
        ._boxed()
}

/// Who is writing here, over the composer: a name, two, or several; the line keeps its room while nobody is, so the
/// composer never moves.
fn typing<'a, R: Renderer + 'a>(shown: Shown<'a>) -> Part<'a, R> {
    let words = shown.words;
    let height = Length::Fixed(TypeStyle::Caption.line_height());
    let writing = shown.open.timeline_id().and_then(|timeline| shown.place.typing(timeline, shown.window.now()));
    let Some(typers) = writing else {
        return Gap::new().height(height)._boxed();
    };
    let said = match typers {
        Typers::One(name) => words.say(&Sentence::OneTyping(name)),
        Typers::Two(first, second) => words.say(&Sentence::TwoTyping(first, second)),
        Typers::Several => words.phrase(Phrase::SeveralTyping).into_owned(),
    };
    let line = Row::with_children([
        TypingDots::new(Role::Content(Content::Secondary))._boxed(),
        coloured(said, TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center);
    container(line).height(height).center_y(height)._boxed()
}

/// The bar over the composer for the draft's first link, while its page is read and once it is made.
fn link_bar<'a, R: Renderer + 'a>(shown: Shown<'a>) -> Option<Part<'a, R>> {
    use screens::LinkPreviewShown;
    let (place, words) = (shown.place, shown.words);
    let bar = match place.link_preview(&shown.open.compose)? {
        LinkPreviewShown::Loading(host) => {
            crate::PreviewBar::Loading(words.say(&Sentence::PreviewWaiting(host.to_owned())))
        }
        LinkPreviewShown::Ready(made) => crate::PreviewBar::Ready {
            host: made.host().to_owned(),
            title: made.title.clone(),
            picture: made.picture.and_then(|picture| (shown.handles)(picture.file())),
        },
    };
    Some(crate::preview_bar(bar, shown.said(DiscussionMessage::RemovePreview), words))
}

/// The composer's frame: the reply bar, the files waiting to send, then the field with the way to attach and send.
fn composer<'a, R: Renderer + 'static>(shown: Shown<'a>) -> Part<'a, R> {
    let (place, open, words) = (shown.place, shown.open, shown.words);
    let (id, pane) = (place.id(), shown.pane);
    let composing = place.composing(&open.compose);
    let draft = composing.map(|composing| &composing.draft);
    let mut frame: Vec<Part<'a, R>> = Vec::new();
    if let Some(reply) = composing.and_then(|composing| composing.reply.as_ref()) {
        let name = match open.quote(&chat::Reply { id: reply.to.clone(), place: reply.place }) {
            Some(Quoted::Message(message)) => author_name(place, &message.author, words),
            Some(Quoted::Deleted) | None => words.phrase(Phrase::FormerMember).into_owned(),
        };
        let replying = Replying {
            name,
            mention: reply.mention_author,
            toggle: shown.said(DiscussionMessage::MentionAuthor),
            cancel: shown.said(DiscussionMessage::CancelReply),
        };
        frame.push(crate::reply_bar(replying, words));
    }
    if let Some(composing) = composing.filter(|composing| !composing.files().is_empty() || composing.notice().is_some())
    {
        frame.push(strip(shown, composing));
    }
    if let Some(bar) = link_bar(shown) {
        frame.push(bar);
    }
    let title = called(&place.title(open), words);
    let named = match (&open.owner, shown.pane) {
        (Owner::Conversation(_), _) => words.say(&Sentence::MessagePerson(title)),
        (Owner::Thread { .. }, _) => words.phrase(Phrase::ReplyInThread).into_owned(),
        (Owner::Channel(_), Pane::CallChat) => words.say(&Sentence::MessageVoiceChannel(title)),
        (Owner::Channel(_), Pane::Content | Pane::Thread) => words.say(&Sentence::MessageChannel(title)),
    };
    let field = crate::Composer::new(crate::labels::label(&named), draft.map_or("", Draft::text))
        .placeholder(named.clone())
        .padding(Padding::from([FIELD_PAD, Space::M.px()]))
        .caret(composing.and_then(screens::Composing::caret), composing.map_or(0, screens::Composing::caret_moved))
        .focus_when(composing.map_or(0, screens::Composing::focus_asked))
        .on_change(move |text| said(id, pane, DiscussionMessage::Draft(text)))
        .on_caret(move |caret| said(id, pane, DiscussionMessage::Caret(caret)))
        .on_submit(shown.said(DiscussionMessage::Send))
        .on_escape(shown.said(DiscussionMessage::Escape))
        .view::<R>();
    // A composer first shown takes the keyboard, as a channel opened does in every peer app; one kept across a switch
    // of channel takes it from `focus_when`.
    let field = crate::focus::take_focus(field)._boxed();
    let mut cells: Vec<Part<'a, R>> = vec![container(field).width(Length::Fill)._boxed()];
    if let Some(count) = draft.and_then(Draft::shown_count) {
        let over = draft.is_some_and(Draft::over);
        let class = if over { TextClass::Status(Status::Danger) } else { TextClass::Content(Content::Secondary) };
        let most = u64::try_from(proto::rules::MAX_MESSAGE_CHARS).unwrap_or(u64::MAX);
        let shown = words.of_most(u64::try_from(count).unwrap_or(u64::MAX), most);
        cells.push(coloured(shown, TypeStyle::Stamp, class)._boxed());
    }
    // Centred on the field's last line, so each sits beside one line or the end of many.
    let line = TypeStyle::Body.line_height() + 2.0 * FIELD_PAD;
    let beside =
        |button: Part<'a, R>| container(button).height(Length::Fixed(line)).center_y(Length::Fixed(line))._boxed();
    if place.may_attach(shown.pane) {
        cells.push(beside(crate::attach_button(Some(shown.said(DiscussionMessage::Attach)), words)));
    }
    let picking = open.picking.as_ref().is_some_and(|picking| picking.target == PickFor::Composer);
    cells.push(beside(
        Button::new(crate::labels::phrase(Phrase::Emoji, words))
            .icon_only(Icon::Smile)
            .variant(ButtonVariant::Quiet)
            .compact()
            .opens(Popup::Dialog, picking)
            .on_press(shown.said(picker_for(PickFor::Composer)))
            .view::<R>(),
    ));
    let sendable = draft
        .is_some_and(|draft| draft.body(composing.map_or(Carries::Nothing, screens::Composing::carries)).is_ok())
        && composing.is_none_or(screens::Composing::files_ready);
    let tip = if draft.is_some_and(Draft::over) { Phrase::TooLongToSend } else { Phrase::Send };
    let mut send =
        Button::new(crate::labels::phrase(tip, words)).icon_only(Icon::Send).variant(ButtonVariant::Primary).compact();
    if sendable {
        send = send.on_press(shown.said(DiscussionMessage::Send));
    }
    cells.push(beside(send.view::<R>()));
    let row = Row::with_children(cells)
        .spacing(Space::Xs.px())
        .align_y(Alignment::End)
        .padding(Padding { right: Space::Xs.px(), ..Padding::ZERO });
    // The reply bar and the strip take one slot whether either shows, so the field never moves and loses its focus.
    container(Column::with_children([Column::with_children(frame)._boxed(), row._boxed()]))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Inset, Radius::M))
        ._boxed()
}

/// The files going with the message, each with its progress, its size or why it was refused, and what the strip
/// says of the files picked last; a rule under them.
fn strip<'a, R: Renderer + 'a>(shown: Shown<'a>, composing: &'a screens::Composing) -> Part<'a, R> {
    let words = shown.words;
    let pending = composing
        .files()
        .iter()
        .map(|file| {
            let state = match &file.state {
                AttachState::Sending { sent, of } => PendingState::Uploading {
                    progress: of.map_or(0.0, |of| fraction(*sent, of)),
                    said: of.map_or_else(
                        || words.phrase(Phrase::Uploading).into_owned(),
                        |size| words.say(&Sentence::UploadProgress { sent: *sent, size }),
                    ),
                },
                AttachState::Ready { size, .. } => {
                    PendingState::Ready { size: size.map(|size| words.bytes(size)).unwrap_or_default() }
                }
                AttachState::Refused(why) => PendingState::Refused(words.say(why)),
            };
            let preview = match file.state {
                AttachState::Ready { file, .. } => (shown.handles)(file),
                AttachState::Sending { .. } | AttachState::Refused(_) => None,
            };
            Pending {
                name: file.name.as_str().to_owned(),
                preview,
                state,
                cancel: shown.said(DiscussionMessage::RemoveFile(file.upload)),
            }
        })
        .collect::<Vec<_>>();
    let mut parts: Vec<Part<'a, R>> = Vec::new();
    if !pending.is_empty() {
        parts.push(crate::pending_strip(pending, words));
    }
    if let Some(noticed) = composing.notice() {
        let said = crate::refusal::<Message, R>(words.say(noticed));
        parts.push(container(said).padding(Padding::from([Space::Xs.px(), Space::M.px()]))._boxed());
    }
    parts.push(crate::panel_parts::hairline());
    Column::with_children(parts)._boxed()
}

/// How much of `of` bytes `sent` is, from 0 to 1, to the thousandth.
fn fraction(sent: u64, of: u64) -> f32 {
    const STEPS: u16 = 1000;
    let steps = sent.saturating_mul(u64::from(STEPS)).checked_div(of).unwrap_or(0).min(u64::from(STEPS));
    f32::from(u16::try_from(steps).unwrap_or(STEPS)) / f32::from(STEPS)
}

/// What opens the emoji picker for `target`, or closes it where it is open for it already.
fn picker_for(target: PickFor) -> DiscussionMessage {
    DiscussionMessage::Picker(PickerMessage::Open(target))
}

/// The emoji picker `picking`, beside what opened it: the server's emoji, the emoji this device picked lately, its
/// tone; its end set in from the list's edge.
fn picker_layer<'a, R: Renderer + 'a>(shown: Shown<'a>, picking: &'a Picking) -> Part<'a, R> {
    let (window, place) = (shown.window, shown.place);
    let (id, pane) = (place.id(), shown.pane);
    let custom: Vec<crate::ServerEmoji> = place
        .emoji()
        .all()
        .iter()
        .map(|own| crate::ServerEmoji {
            id: own.id.clone(),
            name: own.name.as_str().to_owned(),
            picture: (shown.handles)(own.file),
        })
        .collect();
    let recent: Vec<crate::PickedEmoji> = window
        .emoji_picks()
        .recent()
        .iter()
        .filter_map(|emoji| match emoji {
            chat::ReactionEmoji::Unicode(glyph) => chat::emoji::known(glyph).map(crate::PickedEmoji::Unicode),
            chat::ReactionEmoji::Custom(emoji) => Some(crate::PickedEmoji::Custom(emoji.clone())),
        })
        .collect();
    let server = crate::labels::label(place.server().shown());
    let picker = crate::EmojiPicker {
        server: &server,
        server_colour: place.server().colour(),
        custom: &custom,
        recent: &recent,
        query: &picking.query,
        tone: window.emoji_picks().tone,
        shown: picking.section,
    };
    let sends = crate::EmojiSends {
        query: Box::new(move |query| said(id, pane, DiscussionMessage::Picker(PickerMessage::Query(query)))),
        clear: shown.said(DiscussionMessage::Picker(PickerMessage::Query(String::new()))),
        tone: Box::new(Message::EmojiTone),
        section: Box::new(move |section| said(id, pane, DiscussionMessage::Picker(PickerMessage::Section(section)))),
        pick: Box::new(move |picked| {
            let emoji = match picked {
                crate::PickedEmoji::Unicode(each) => chat::ReactionEmoji::Unicode(each.as_str().to_owned()),
                crate::PickedEmoji::Custom(emoji) => chat::ReactionEmoji::Custom(emoji),
            };
            said(id, pane, DiscussionMessage::Picker(PickerMessage::Pick(emoji)))
        }),
    };
    container(crate::emoji_picker(picker, sends, shown.words))
        .padding(Padding { right: shown.side, ..Padding::ZERO })
        ._boxed()
}

/// The line over the first message not read when the channel was opened: a rule in the accent and its mark.
fn new_line<'a, R: Renderer + 'a>(side: f32, words: &Words) -> Part<'a, R> {
    let accent = Role::Accent(design::AccentRole::Fill);
    let rule = container(Gap::new())
        .height(Length::Fixed(design::BORDER_WIDTH))
        .width(Length::Fill)
        .class(Surfaced::Fill { fill: accent, text: accent, radius: Radius::None });
    let mark = container(text(words.phrase(Phrase::NewMark).into_owned(), TypeStyle::CaptionStrong))
        .padding(Padding::from([0.0, Space::Xs.px() - Space::Xxs.px()]))
        .class(Surfaced::Fill { fill: accent, text: Role::Accent(design::AccentRole::OnFill), radius: Radius::S });
    let line = Row::with_children([rule._boxed(), mark._boxed()]).align_y(Alignment::Center);
    let padded = container(line).padding(Padding {
        top: Space::S.px() + Space::Xxs.px(),
        bottom: Space::Xxs.px(),
        left: side,
        right: side,
    });
    crate::Labelled::new(
        padded,
        design::Semantics {
            role: design::SemanticRole::Status,
            label: crate::labels::phrase(Phrase::NewMessages, words),
        },
    )
    ._boxed()
}

/// A message's row shaded under the pointer, or while what it opened (its menu, its picker) is `held` open.
/// Whom a message calls, as its row shows it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Called {
    /// The reader, by name, a role of theirs, `@everyone` or `@here`, or a reply to them.
    Reader,
    /// Someone else, or no one.
    Others,
}

struct Shade<'a, R> {
    /// The row.
    children: [Part<'a, R>; 1],
    held: bool,
    /// Whether the message calls the reader: its row is on the accent's tint, an accent bar at its left.
    called: Called,
}

impl<'a, R: Renderer + 'a> Shade<'a, R> {
    fn new(content: Part<'a, R>, held: bool, called: Called) -> Self {
        Shade { children: [content], held, called }
    }
}

/// Whether the pointer was over the row at the last event, so a change asks for a redraw.
#[derive(Clone, Copy, Debug, Default)]
struct Over(bool);

impl<R: Renderer> iced_core::widget::Meta for Shade<'_, R> {}

impl<R: Renderer> Widget<Message, Theme, R> for Shade<'_, R> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Over>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Over::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        self.children[0].size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        let [content] = &mut self.children;
        content.layout(&mut tree.children[0], renderer, limits);
        tree.size = tree.children[0].size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        let [content] = &mut self.children;
        if let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() {
            content.operate(tree, layout, viewport, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced_core::Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &R,
        shell: &mut iced_core::Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let over = cursor.is_over(layout.bounds());
        let [content] = &mut self.children;
        if let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() {
            content.update(tree, event, layout, cursor, renderer, shell, viewport);
        }
        let state = tree.state.downcast_mut::<Over>();
        if state.0 != over {
            state.0 = over;
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &R,
    ) -> mouse::Interaction {
        let [content] = &self.children;
        layout
            .iter(&tree.children)
            .next()
            .map(|(layout, tree)| content.mouse_interaction(tree, layout, cursor, viewport, renderer))
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        match self.called {
            Called::Reader => {
                renderer.fill_quad(
                    renderer::Quad { bounds, ..renderer::Quad::default() },
                    theme.color(Role::Accent(design::AccentRole::Tint)),
                );
                let bar = Rectangle { width: design::CALLED_BAR, ..bounds };
                renderer.fill_quad(
                    renderer::Quad { bounds: bar, ..renderer::Quad::default() },
                    theme.color(Role::Accent(design::AccentRole::Fill)),
                );
            }
            Called::Others if self.held || cursor.is_over(bounds) => {
                renderer.fill_quad(
                    renderer::Quad { bounds, ..renderer::Quad::default() },
                    theme.color(Role::Surface(Surface::Raised)),
                );
            }
            Called::Others => {}
        }
        let [content] = &self.children;
        if let Some((layout, tree)) = layout.iter(&tree.children).next() {
            content.draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, R>> {
        overlay::from_children(&mut self.children, tree, layout, renderer, viewport, translation, window)
    }
}

/// The typing line's three dots, each brightening and fading in turn as the canvas draws them; still, at full colour,
/// where the system asks for less motion.
struct TypingDots {
    role: Role,
}

/// When the dots began pulsing and how far they are, and whether the last frame was drawn still. Only `draw` sees the
/// theme, and it sees the state read-only, so that word is a `Cell` it sets for the next `update`.
#[derive(Clone, Debug, Default)]
struct Pulse {
    begun: Option<std::time::Instant>,
    since: std::time::Duration,
    still: std::cell::Cell<bool>,
}

impl TypingDots {
    fn new(role: Role) -> Self {
        TypingDots { role }
    }
}

/// A dot's opacity `since` its pulse began: dim, up to full by two fifths of the pulse, down again by four fifths,
/// eased at both ends as CSS's ease-in-out.
fn dot_opacity(since: std::time::Duration) -> f32 {
    let phase = (since.as_secs_f32() / design::TYPING_PULSE.as_secs_f32()).rem_euclid(1.0);
    let rise = |x: f32| x * x * (3.0 - 2.0 * x);
    let up = match phase {
        p if p < 0.4 => rise(p / 0.4),
        p if p < 0.8 => 1.0 - rise((p - 0.4) / 0.4),
        _ => 0.0,
    };
    design::TYPING_DIM + (1.0 - design::TYPING_DIM) * up
}

impl iced_core::widget::Meta for TypingDots {}

impl<R: Renderer> Widget<Message, Theme, R> for TypingDots {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Pulse>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Pulse::default())
    }

    fn size(&self) -> Size<Length> {
        let width = 3.0 * design::TYPING_DOT + 2.0 * design::TYPING_DOT_GAP;
        Size::new(Length::Fixed(width), Length::Fixed(design::TYPING_DOT))
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, _limits: &layout::Limits) {
        tree.size = Size::new(3.0 * design::TYPING_DOT + 2.0 * design::TYPING_DOT_GAP, design::TYPING_DOT);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced_core::Event,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut iced_core::Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let iced_core::Event::Window(iced_core::window::Event::RedrawRequested(now)) = event {
            let pulse = tree.state.downcast_mut::<Pulse>();
            if pulse.still.get() {
                return;
            }
            let begun = *pulse.begun.get_or_insert(*now);
            pulse.since = now.saturating_duration_since(begun);
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let pulse = tree.state.downcast_ref::<Pulse>();
        let still = theme.motion() == design::MotionPreference::Reduced;
        pulse.still.set(still);
        let colour = theme.color(self.role);
        let bounds = layout.bounds();
        let mut late = std::time::Duration::ZERO;
        for at in 0..3_u8 {
            let opacity = if still { 1.0 } else { dot_opacity(pulse.since.saturating_sub(late)) };
            let x = bounds.x + f32::from(at) * (design::TYPING_DOT + design::TYPING_DOT_GAP);
            let dot =
                Rectangle::new(iced_core::Point::new(x, bounds.y), Size::new(design::TYPING_DOT, design::TYPING_DOT));
            let quad = renderer::Quad {
                bounds: dot,
                border: iced_core::Border { radius: (design::TYPING_DOT / 2.0).into(), ..iced_core::Border::default() },
                ..renderer::Quad::default()
            };
            renderer.fill_quad(quad, iced_core::Color { a: colour.a * opacity, ..colour });
            late += design::TYPING_STAGGER;
        }
    }
}
