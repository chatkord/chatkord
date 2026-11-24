//! The server's words read at the edge: places, colours, channels and the events a place reads.

use super::*;
use proto::rules::Kind;

/// Who is in a voice room as the wire states them; one with an empty id is a server not keeping the protocol, and is
/// passed over.
pub(super) fn occupants(people: &[MemberSummary]) -> Vec<Occupant> {
    people
        .iter()
        .filter_map(|person| {
            Some(Occupant {
                user_id: MemberId::parse(&person.user_id)?,
                name: person.display_name.clone(),
                colour: wire_colour(person.colour.as_deref()),
                state: State { mic_muted: person.mic_muted, deafened: person.deafened },
                sharing: person.sharing,
                camera: person.camera,
                held: person.admin_muted,
                place: proto::rules::Place::of_wire(person.place),
            })
        })
        .collect()
}

/// A colour as the wire wrote it: one out of shape is a server not keeping the protocol, and is passed over as none.
pub fn wire_colour(colour: Option<&str>) -> Option<proto::rules::Colour> {
    colour.and_then(|colour| proto::rules::Colour::parse(colour).ok())
}

/// A server's list read, in its order; a channel of a kind this client does not know is passed over, and so is a
/// category with no id. Who is in a voice room is no fact of the list: `VoiceOccupancy` tells it.
pub fn listed(summary: &ChannelsSummary) -> ChannelList {
    let standings = summary
        .standings
        .iter()
        .filter_map(|told| Some((ChannelId::parse(&told.channel_id)?, standing(told))))
        .collect();
    let categories = summary.categories.iter().filter_map(category).collect();
    let mut listed = ChannelList { version: summary.version, standings, categories, ..ChannelList::default() };
    for channel in &summary.channels {
        let Some((made, placed)) = added(channel) else { continue };
        if channel.private {
            listed.private.insert(made.id().clone());
        }
        match made {
            Made::Text(text) => {
                listed.placed.insert(text.id.clone(), placed);
                listed.text.push(text);
            }
            Made::Voice(voice) => {
                listed.placed.insert(voice.id.clone(), placed);
                listed.voice.push(voice);
            }
        }
    }
    listed.arrange();
    listed
}

/// A category as the wire states it; none with no id, a server not keeping the protocol.
fn category(told: &CategorySummary) -> Option<Category> {
    Some(Category { id: CategoryId::parse(&told.category_id)?, name: told.name.clone(), position: told.position })
}

/// Where the wire places a channel; none for a category with no id, a server not keeping the protocol.
fn placement(category_id: &str, position: i64) -> Option<Placement> {
    Some(Placement { category: CategoryId::parse(category_id)?, position })
}

/// A standing as the wire tells it, its words read as `Grants::from_words` reads an account's.
fn standing(told: &proto::chatkord_v1::ChannelStanding) -> ChannelStanding {
    ChannelStanding {
        grants: Grants::from_words(&told.grants),
        read: told.read,
        unread: told.unread.min(proto::rules::COUNT_CAP),
        mentions: told.mentions.min(proto::rules::COUNT_CAP),
    }
}

/// A channel of the list as its kind's entry.
enum Made {
    Text(TextChannel),
    Voice(Channel),
}

impl Made {
    fn id(&self) -> &ChannelId {
        match self {
            Made::Text(channel) => &channel.id,
            Made::Voice(channel) => &channel.id,
        }
    }
}

/// Who sees a channel, as the wire's flag says it.
const fn sight(private: bool) -> Sight {
    if private { Sight::Private } else { Sight::Everyone }
}

/// A channel of the list as its kind's entry and where it stands; none for a kind this client does not know, or a
/// channel naming no timeline or no id, a server not keeping the protocol.
fn added(channel: &ChannelSummary) -> Option<(Made, Placement)> {
    let (id, name) = (ChannelId::parse(&channel.channel_id)?, channel.name.clone());
    let placed = placement(&channel.category_id, channel.position)?;
    let timeline = TimelineId::parse(&channel.timeline_id)?;
    match Kind::from_wire(channel.kind)? {
        Kind::Text => Some((Made::Text(TextChannel { id, name, timeline, revision: channel.revision }), placed)),
        Kind::Voice => Some((Made::Voice(Channel { id, name, people: Vec::new(), timeline }), placed)),
    }
}

/// A channel told made as the change it is.
fn made(told: &ChannelSummary) -> Option<ChannelChange> {
    let seen = sight(told.private);
    Some(match added(told)? {
        (Made::Text(channel), placed) => ChannelChange::AddedText { channel, placed, sight: seen },
        (Made::Voice(channel), placed) => ChannelChange::AddedVoice { channel, placed, sight: seen },
    })
}

/// A change of the list as the place reads it.
fn changed(change: ChannelChange, version: u64) -> Message {
    Message::Lobby(LobbyMessage::Changed { change, version })
}

impl Message {
    /// What the server said of the account's roles, read.
    pub fn standing(changed: &StandingChanged) -> Message {
        Message::Session(SessionMessage::Standing {
            place: proto::rules::Place::of_wire(changed.place),
            grants: Grants::from_words(&changed.grants),
        })
    }

    /// What the connection told means here, if anything.
    pub fn told(heard: &terms::Heard) -> Option<Message> {
        match heard {
            terms::Heard::Event(event) => Self::heard(event),
            terms::Heard::Behind => Some(Message::Session(SessionMessage::Behind)),
            terms::Heard::Ended(ended) => Some(Message::Session(SessionMessage::Disconnected(ended.clone()))),
        }
    }

    /// What an event means here, if anything: room events are a call's to read.
    pub fn heard(event: &Event) -> Option<Message> {
        match event.kind.as_ref()? {
            event::Kind::StandingChanged(changed) => Some(Message::standing(changed)),
            // The state that follows leaves out what the people list and the open channel hold: read as falling behind.
            event::Kind::Rewound(_) => Some(Message::Session(SessionMessage::Behind)),
            event::Kind::ChannelAdded(told) => Some(changed(made(told.channel.as_ref()?)?, told.version)),
            event::Kind::ChannelRenamed(renamed) => Some(changed(
                ChannelChange::Renamed { id: ChannelId::parse(&renamed.channel_id)?, name: renamed.name.clone() },
                renamed.version,
            )),
            event::Kind::ChannelMoved(moved) => Some(changed(
                ChannelChange::Moved {
                    id: ChannelId::parse(&moved.channel_id)?,
                    placed: placement(&moved.category_id, moved.position)?,
                },
                moved.version,
            )),
            event::Kind::CategoryAdded(told) => {
                Some(changed(ChannelChange::CategoryAdded(category(told.category.as_ref()?)?), told.version))
            }
            event::Kind::CategoryRenamed(renamed) => Some(changed(
                ChannelChange::CategoryRenamed {
                    id: CategoryId::parse(&renamed.category_id)?,
                    name: renamed.name.clone(),
                },
                renamed.version,
            )),
            event::Kind::CategoryRemoved(removed) => {
                Some(changed(ChannelChange::CategoryRemoved(CategoryId::parse(&removed.category_id)?), removed.version))
            }
            event::Kind::CategoryMoved(moved) => Some(changed(
                ChannelChange::CategoryMoved { id: CategoryId::parse(&moved.category_id)?, position: moved.position },
                moved.version,
            )),
            event::Kind::RolesChanged(_) => Some(Message::Server(ServerMessage::RolesChanged)),
            event::Kind::ChannelStanding(told) => Some(Message::Lobby(LobbyMessage::Standing {
                channel_id: ChannelId::parse(&told.channel_id)?,
                standing: standing(told),
            })),
            event::Kind::ChannelRemoved(removed) => {
                Some(changed(ChannelChange::Removed(ChannelId::parse(&removed.channel_id)?), removed.version))
            }
            event::Kind::VoiceOccupancy(room) => Some(Message::Lobby(LobbyMessage::Occupancy {
                channel_id: ChannelId::parse(&room.channel_id)?,
                people: occupants(&room.people),
            })),
            event::Kind::MemberRenamed(renamed) => Some(Message::Server(ServerMessage::Renamed {
                user_id: MemberId::parse(&renamed.user_id)?,
                name: renamed.display_name.clone(),
            })),
            event::Kind::ConversationOpened(told) => {
                let conversation = told.conversation.as_ref()?;
                Some(Message::Talk(TalkMessage::Opened(Box::new(conversation.clone()))))
            }
            event::Kind::TimelineStanding(told) => Some(Message::Talk(TalkMessage::Standing {
                timeline: TimelineId::parse(&told.timeline_id)?,
                standing: TimelineStanding::read(told),
            })),
            event::Kind::DeviceInUse(told) => {
                Some(Message::Talk(TalkMessage::DeviceInUse(DeviceUse { alerts_here: told.alerts_here })))
            }
            // Read at the edge: no choice is a server not keeping the protocol, and is passed over; an end already
            // passed reads as Online wherever the choice is read at a time.
            event::Kind::StatusChosen(told) => {
                let choice = proto::status::Choice::from_wire(told.choice())?;
                Some(Message::Talk(TalkMessage::StatusChosen(proto::status::Chosen { choice, until: told.until })))
            }
            event::Kind::MemberJoined(_)
            | event::Kind::MemberLeft(_)
            | event::Kind::ProducerJoined(_)
            | event::Kind::ProducerClosed(_)
            | event::Kind::VoiceClosed(_)
            | event::Kind::RoomSnapshot(_)
            | event::Kind::MemberState(_)
            | event::Kind::ShareWatched(_) => None,
            // Read at the edge: one out of shape is a server not keeping the protocol, and is passed over.
            event::Kind::ReactionsOfYours(told) => {
                let mine = told.mine.iter().map(chat::ReactionEmoji::of_wire).collect::<Option<Vec<_>>>()?;
                Some(Message::Discussion(DiscussionMessage::ReactionsOfYours {
                    timeline: TimelineId::parse(&told.timeline_id)?,
                    message: chat::MessageId::parse(&told.message_id)?,
                    revision: told.revision,
                    mine,
                }))
            }
            event::Kind::TypingNow(told) => Some(Message::Discussion(DiscussionMessage::Typing {
                timeline: TimelineId::parse(&told.timeline_id)?,
                users: told.user_ids.iter().filter_map(|id| MemberId::parse(id)).collect(),
            })),
            event::Kind::EmojiAdded(added) => {
                Some(Message::Emoji(EmojiMessage::Added(ServerEmoji::read(added.emoji.as_ref()?)?)))
            }
            event::Kind::EmojiRenamed(renamed) => Some(Message::Emoji(EmojiMessage::Renamed {
                id: chat::EmojiId::parse(&renamed.emoji_id)?,
                name: proto::rules::emoji_name(&renamed.name).ok()?,
            })),
            event::Kind::EmojiRemoved(removed) => {
                Some(Message::Emoji(EmojiMessage::Removed(chat::EmojiId::parse(&removed.emoji_id)?)))
            }
            event::Kind::NotifyChanged(changed) => {
                Some(Message::Notify(NotifyMessage::Changed(Notifying::read(changed.settings.as_ref()?))))
            }
            event::Kind::PersonArrived(arrived) => {
                let (user_id, person, _) = arrived.person.as_ref().and_then(person_of)?;
                Some(Message::People(PeopleMessage::Arrived { user_id, person }))
            }
            event::Kind::MemberRolesChanged(changed) => Some(Message::People(PeopleMessage::Roles {
                user_id: MemberId::parse(&changed.user_id)?,
                roles: changed.role_ids.iter().filter_map(|id| RoleId::parse(id)).collect(),
            })),
            // Read at the edge: an empty id is a server not keeping the protocol, and is passed over.
            event::Kind::PresenceChanged(told) => Some(Message::People(PeopleMessage::Presence {
                generation: told.generation,
                shown: told
                    .shown
                    .iter()
                    .filter_map(|shown| {
                        let status = proto::chatkord_v1::Status::try_from(shown.status).ok()?;
                        Some((MemberId::parse(&shown.user_id)?, proto::status::Shown::from_wire(status)?))
                    })
                    .collect(),
            })),
            event::Kind::ChannelAccessChanged(told) => Some(changed(
                ChannelChange::Access { id: ChannelId::parse(&told.channel_id)?, sight: sight(told.private) },
                told.version,
            )),
            event::Kind::PersonLeft(left) => {
                MemberId::parse(&left.user_id).map(|user_id| Message::People(PeopleMessage::Left(user_id)))
            }
            event::Kind::MessageChanged(changed) => {
                // A message out of shape is a server not keeping the protocol, and is passed over.
                let wire = changed.message.as_deref()?;
                let message = chat::Message::try_from(wire).ok()?;
                let timeline = TimelineId::parse(&wire.timeline_id)?;
                Some(Message::Discussion(DiscussionMessage::Told { timeline, message: Box::new(message) }))
            }
            event::Kind::MessagesWithdrawn(told) => {
                // One out of shape is a server not keeping the protocol, and is passed over.
                let withdrawn = chat::Withdrawn::try_from(told).ok()?;
                let timeline = TimelineId::parse(&told.timeline_id)?;
                Some(Message::Discussion(DiscussionMessage::Withdrawn { timeline, withdrawn }))
            }
            event::Kind::MemberRecoloured(recoloured) => {
                let colour = recoloured.colour.as_deref();
                // A colour out of shape is a server not keeping the protocol: the change is passed over.
                let read = colour.map(proto::rules::Colour::parse).transpose().ok()?;
                let user_id = MemberId::parse(&recoloured.user_id)?;
                Some(Message::Server(ServerMessage::Recoloured { user_id, colour: read }))
            }
            event::Kind::MemberPictured(pictured) => {
                // A picture out of shape is a server not keeping the protocol: the change is passed over.
                let picture = pictured.picture.as_deref().map(proto::files::FileId::parse).transpose().ok()?;
                let user_id = MemberId::parse(&pictured.user_id)?;
                Some(Message::People(PeopleMessage::Pictured { user_id, picture }))
            }
            // Read at the edge: a name or colour out of shape is a server not keeping the protocol, and is passed over.
            event::Kind::ServerChanged(changed) => {
                let (name, colour, picture) =
                    (changed.name.as_deref(), changed.colour.as_deref(), changed.picture.as_deref());
                let offers = Offers {
                    served: Served::read(&proto::rules::Switch::off(&changed.switched_off)),
                    limits: terms::FileLimits::of_wire(changed.attachment_max_bytes, changed.account_files_max_bytes),
                };
                ServerIdentity::read(name, colour, picture)
                    .map(|identity| Message::Server(ServerMessage::ServerChanged { identity, offers }))
            }
        }
    }
}
