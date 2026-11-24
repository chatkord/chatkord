//! Conversations between members: the list, who the other member is, and opening one.

use super::*;

/// A message in brief, as a list's row shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brief {
    /// Who wrote it; none when their account is gone.
    pub from: Option<MemberId>,
    /// The start of its text, at most [`proto::rules::PREVIEW_CHARS`] characters.
    pub text: String,
    /// When it was sent, in seconds since the epoch.
    pub at: i64,
}

impl Brief {
    pub(super) fn read(told: &wire::Preview) -> Brief {
        Brief { from: told.user_id.as_deref().and_then(MemberId::parse), text: told.text.clone(), at: told.created_at }
    }

    /// `message` in brief, none once it is deleted.
    fn of(message: &chat::Message) -> Option<Brief> {
        let body = message.body.as_ref()?;
        let from = match &message.author {
            chat::Author::Member(author) => Some(author.clone()),
            chat::Author::Former(_) | chat::Author::App { .. } => None,
        };
        Some(Brief { from, text: proto::rules::preview_text(&body.shown()), at: message.created_at })
    }
}

/// Who the other member of a conversation is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum With {
    /// A member of the server, by their id.
    Member(MemberId),
    /// Someone who left or was banned; their name as it was known while they were here, if it was.
    Gone {
        /// The name others saw them by.
        name: Option<String>,
    },
}

/// A conversation as the list holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    /// Its id at the server.
    pub id: ConversationId,
    /// The timeline of its messages.
    pub timeline: TimelineId,
    /// The other member.
    pub with: With,
    /// Its timeline's revision when it was read.
    pub revision: u64,
    /// Its newest living message in brief; none with none.
    pub last: Option<Brief>,
    /// It takes a message: the other member is still on the server and not banned.
    pub sendable: bool,
}

impl Conversation {
    /// The conversation the wire states, with this account's standing in it; none out of shape (an empty id, or no
    /// member id at all), a server not keeping the protocol.
    fn read(told: &wire::Conversation, me: Option<&MemberId>) -> Option<(Conversation, TimelineStanding)> {
        let id = ConversationId::parse(&told.conversation_id)?;
        let timeline = TimelineId::parse(&told.timeline_id)?;
        let members: Vec<MemberId> = told.user_ids.iter().filter_map(|id| MemberId::parse(id)).collect();
        if members.is_empty() {
            return None;
        }
        let with = match members.into_iter().find(|member| Some(member) != me) {
            Some(other) => With::Member(other),
            None => With::Gone { name: None },
        };
        let standing = told.standing.as_ref().map(TimelineStanding::read).unwrap_or_default();
        let conversation = Conversation {
            id,
            timeline,
            with,
            revision: told.revision,
            last: told.last.as_ref().map(Brief::read),
            sendable: told.sendable,
        };
        Some((conversation, standing))
    }
}

/// A list the content area shows, as far as it was read: its rows first the ones with something unread, then the
/// newest.
pub type Rows<T> = Loaded<Vec<T>>;

/// A conversation as the list's row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ConversationRow<'a> {
    /// The conversation.
    pub conversation: &'a Conversation,
    /// The other person's name; none for someone gone whose name was never known ("Former member").
    pub name: Option<String>,
    /// The colour they picked, if any.
    pub colour: Option<proto::rules::Colour>,
    /// Whether they are signed in; none for someone who is gone.
    pub presence: Option<proto::status::Shown>,
    /// What this account has read of it.
    pub standing: TimelineStanding,
    /// The newest message in brief.
    pub preview: Option<PreviewLine<'a>>,
}

/// A row's preview: "You: " in front when this account wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewLine<'a> {
    /// This account wrote it.
    pub mine: bool,
    /// The start of its text.
    pub text: &'a str,
    /// When it was sent, in seconds since the epoch.
    pub at: i64,
}

impl Talk {
    fn conversation(&self, id: &ConversationId) -> Option<&Conversation> {
        match &self.conversations {
            Loaded::Ready(listed) => listed.iter().find(|held| held.id == *id),
            Loaded::Loading | Loaded::Failed(_) => None,
        }
    }

    pub(super) fn conversation_of(&self, timeline: &TimelineId) -> Option<&Conversation> {
        match &self.conversations {
            Loaded::Ready(listed) => listed.iter().find(|held| held.timeline == *timeline),
            Loaded::Loading | Loaded::Failed(_) => None,
        }
    }

    /// The conversation `told` is, put first, or replacing the one held with its id.
    pub(in crate::place) fn keep(&mut self, conversation: Conversation, standing: TimelineStanding) {
        let kept = self.standings.entry(conversation.timeline.clone()).or_default();
        *kept = kept.over(standing);
        let listed = match &mut self.conversations {
            Loaded::Ready(listed) => listed,
            Loaded::Loading | Loaded::Failed(_) => {
                self.conversations = Loaded::Ready(Vec::new());
                match &mut self.conversations {
                    Loaded::Ready(listed) => listed,
                    Loaded::Loading | Loaded::Failed(_) => return,
                }
            }
        };
        match listed.iter_mut().find(|held| held.id == conversation.id) {
            Some(held) => *held = conversation,
            None => listed.insert(0, conversation),
        }
    }
}

impl Place {
    /// The conversation `id`, once listed.
    pub fn conversation(&self, id: &ConversationId) -> Option<&Conversation> {
        match &self.status {
            Status::Signed(signed) => signed.talk.conversation(id),
            Status::Reconnecting(_) | Status::Door(_) => None,
        }
    }

    /// What this account may do in a discussion `owner` owns: a channel's or a thread's words are its channel's; a
    /// conversation gives its members four words, and takes the writing ones once the other member is gone.
    pub fn grants_of(&self, owner: &Owner) -> Grants {
        match owner {
            Owner::Channel(id) | Owner::Thread { channel: id, .. } => self.grants_in(id),
            Owner::Conversation(id) => match self.conversation(id) {
                Some(conversation) if conversation.sendable => CONVERSATION_GRANTS,
                Some(_) => CONVERSATION_GRANTS.without(CONVERSATION_WRITING),
                None => Grants::NONE,
            },
        }
    }

    /// The name others see `with` by, their colour and whether they are signed in.
    fn person_of(&self, with: &With) -> (Option<String>, Option<proto::rules::Colour>, Option<proto::status::Shown>) {
        match with {
            With::Member(id) => {
                let named = self.directory.person(id);
                let name = self.name_of(id).unwrap_or_else(|| id.as_str().to_owned());
                (Some(name), named.and_then(|person| person.colour), Some(self.directory.presence(id)))
            }
            With::Gone { name } => (name.clone(), None, None),
        }
    }

    /// The name of the other member of the conversation `conversation`: as others see them, else a gone member's
    /// last known name; none for someone gone whose name was never known, which the front end says as "Former member".
    pub fn conversation_name(&self, conversation: &Conversation) -> Option<String> {
        self.person_of(&conversation.with).0
    }

    /// The conversations as the list shows them: the ones with something unread first, then the newest; none while
    /// they are read.
    pub fn conversation_rows(&self) -> Rows<ConversationRow<'_>> {
        let Status::Signed(signed) = &self.status else { return Rows::Loading };
        let me = self.me();
        match &signed.talk.conversations {
            Loaded::Loading => Rows::Loading,
            Loaded::Failed(failed) => Rows::Failed(failed.clone()),
            Loaded::Ready(listed) => {
                let mut rows: Vec<ConversationRow<'_>> = listed
                    .iter()
                    .map(|conversation| {
                        let (name, colour, presence) = self.person_of(&conversation.with);
                        ConversationRow {
                            conversation,
                            name,
                            colour,
                            presence,
                            standing: signed.talk.standing(&conversation.timeline),
                            preview: conversation.last.as_ref().map(|last| PreviewLine {
                                mine: last.from.is_some() && last.from.as_ref() == me,
                                text: &last.text,
                                at: last.at,
                            }),
                        }
                    })
                    .collect();
                rows.sort_by_key(|row| {
                    (row.standing.unread == 0, std::cmp::Reverse(row.preview.map_or(i64::MIN, |line| line.at)))
                });
                Rows::Ready(rows)
            }
        }
    }

    /// The unread messages across this account's conversations, for the side pane's "Direct messages" row.
    pub fn conversations_unread(&self) -> u32 {
        let Status::Signed(signed) = &self.status else { return 0 };
        match &signed.talk.conversations {
            Loaded::Ready(listed) => {
                listed.iter().map(|conversation| signed.talk.standing(&conversation.timeline).unread).sum()
            }
            Loaded::Loading | Loaded::Failed(_) => 0,
        }
    }

    /// A member left: their conversation, if this account has one, keeps their name and takes no more messages.
    pub(in crate::place) fn talk_member_left(&mut self, user_id: &MemberId) {
        let name = self.name_of(user_id);
        let Status::Signed(signed) = &mut self.status else { return };
        let Loaded::Ready(listed) = &mut signed.talk.conversations else { return };
        for conversation in listed.iter_mut().filter(|conversation| conversation.with == With::Member(user_id.clone()))
        {
            conversation.with = With::Gone { name: name.clone() };
            conversation.sendable = false;
        }
    }

    /// The Message button on a member's popover: their conversation opens, made first when this account has none.
    pub(super) fn message_member(&mut self, user_id: &MemberId) -> Vec<Ask> {
        if !self.served.on(proto::rules::Switch::Conversations) || self.me() == Some(user_id) || !self.signed_in() {
            return Vec::new();
        }
        let held = match &self.status {
            Status::Signed(signed) => match &signed.talk.conversations {
                Loaded::Ready(listed) => listed
                    .iter()
                    .find(|conversation| conversation.with == With::Member(user_id.clone()))
                    .map(|conversation| conversation.id.clone()),
                Loaded::Loading | Loaded::Failed(_) => None,
            },
            Status::Reconnecting(_) | Status::Door(_) => None,
        };
        match held {
            Some(id) => self.open_conversation(&id),
            None => vec![ops::open_conversation(user_id)],
        }
    }

    /// The conversation `id` open in the content area. Wherever it was opened from, its back arrow leads to the list
    /// of conversations, the one place that holds it.
    pub(crate) fn open_conversation(&mut self, id: &ConversationId) -> Vec<Ask> {
        let Some(timeline) = self.conversation(id).map(|conversation| conversation.timeline.clone()) else {
            return Vec::new();
        };
        let owner = Owner::Conversation(id.clone());
        self.seat(owner, timeline, None, Some(Roll::Conversations))
    }

    /// A message told in a conversation: its brief is the row's preview.
    pub(crate) fn conversation_told(&mut self, timeline: &TimelineId, message: &chat::Message) {
        let Status::Signed(signed) = &mut self.status else { return };
        let Loaded::Ready(listed) = &mut signed.talk.conversations else { return };
        let Some(conversation) = listed.iter_mut().find(|held| held.timeline == *timeline) else { return };
        if message.revision < conversation.revision {
            return;
        }
        conversation.revision = message.revision;
        if let Some(brief) = Brief::of(message) {
            conversation.last = Some(brief);
        }
    }

    pub(super) fn conversations_listed(&mut self, answer: Result<wire::Conversations, LinkError>) -> Vec<Ask> {
        let me = self.me().cloned();
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match answer {
            Ok(told) => {
                let read: Vec<(Conversation, TimelineStanding)> =
                    told.conversations.iter().filter_map(|c| Conversation::read(c, me.as_ref())).collect();
                for (conversation, standing) in &read {
                    let kept = signed.talk.standings.entry(conversation.timeline.clone()).or_default();
                    *kept = kept.over(*standing);
                }
                // A conversation opened while the list was read stays: it is newer than the list.
                let newer: Vec<Conversation> = match &signed.talk.conversations {
                    Loaded::Ready(held) => held
                        .iter()
                        .filter(|held| !read.iter().any(|(listed, _)| listed.id == held.id))
                        .cloned()
                        .collect(),
                    Loaded::Loading | Loaded::Failed(_) => Vec::new(),
                };
                let listed = newer.into_iter().chain(read.into_iter().map(|(conversation, _)| conversation));
                signed.talk.conversations = Loaded::Ready(listed.collect());
            }
            Err(failed) => signed.talk.conversations = Loaded::Failed(failed),
        }
        Vec::new()
    }

    pub(super) fn conversation_opened(&mut self, told: &wire::Conversation) -> Vec<Ask> {
        let read = Conversation::read(told, self.me());
        if let (Some((conversation, standing)), Status::Signed(signed)) = (read, &mut self.status) {
            signed.talk.keep(conversation, standing);
        }
        Vec::new()
    }

    pub(super) fn conversation_made(&mut self, answer: Result<Box<wire::Conversation>, LinkError>) -> Vec<Ask> {
        let read = answer.ok().and_then(|told| Conversation::read(&told, self.me()));
        let Some((conversation, standing)) = read else { return Vec::new() };
        let id = conversation.id.clone();
        if let Status::Signed(signed) = &mut self.status {
            signed.talk.keep(conversation, standing);
        }
        self.open_conversation(&id)
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::sends;
    use crate::harness::{Clock, conversation_told, conversation_wire, discussion, member, said, tell, timeline_id};
    use crate::place;
    use crate::place::talk::tests::{asks, talk, talkative};
    use crate::place::{Owner, Panel, Roll, TalkMessage};
    use crate::window::{Message, PersonFrom};
    use proto::chatkord_v1 as wire;
    use proto::rules::Grant;
    use std::time::Instant;

    #[test]
    fn message_on_a_popover_opens_the_conversation_made_first_and_then_it_just_opens() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        window.update(Message::Person(owls, member("bo"), PersonFrom::Members), clock.at());
        assert!(window.person().is_some_and(|card| card.may_message), "offered on someone else's popover");
        let todo = tell(&mut window, &clock, owls, talk(TalkMessage::Message(member("bo"))));
        assert!(
            matches!(&asks(&todo)[..], [ask] if sends(ask, wire::OpenConversation { user_id: "bo".into() })),
            "{todo:?}"
        );
        assert!(window.person().is_none(), "the popover is done with");
        let made = Box::new(conversation_wire("c-bo", Some("bo"), true));
        let todo = tell(&mut window, &clock, owls, talk(TalkMessage::Made(Ok(made))));
        assert!(
            matches!(&asks(&todo)[..], [ask] if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.timeline_id == timeline_id("t-c-bo").as_str())),
            "its first page: {todo:?}"
        );
        let place = &window.places()[0];
        let open = place.discussion().expect("the conversation is open");
        assert!(matches!(&open.owner, Owner::Conversation(id) if id.as_str() == "c-bo"));
        assert_eq!(place.panel(), Panel::Discussion);
        assert!(place.may_send(place::Pane::Content), "a conversation gives its members four words");
        assert!(place.composer_gone(open).is_none());
        let again = tell(&mut window, &clock, owls, talk(TalkMessage::Message(member("bo"))));
        assert!(asks(&again).is_empty(), "the one that exists is already open: {again:?}");
        let tell_yourself = tell(&mut window, &clock, owls, talk(TalkMessage::Message(member("Ada"))));
        assert!(asks(&tell_yourself).is_empty(), "no conversation with oneself");
    }

    #[test]
    fn a_conversation_offers_the_channels_the_account_sees_as_marks() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
        let id = terms::ConversationId::parse("c-bo").unwrap();
        tell(&mut window, &clock, owls, talk(TalkMessage::OpenConversation(id)));
        let compose = place::Compose::Timeline(timeline_id("t-c-bo"));
        let channels = |window: &mut crate::window::Window| {
            tell(window, &clock, owls, discussion(place::DiscussionMessage::Draft("see #gen".into())));
            window.places()[0].choosing(&compose).map(|choosing| {
                choosing
                    .choices
                    .iter()
                    .filter_map(|choice| match choice {
                        place::Choice::Channel { name, category, .. } => Some((name.clone(), category.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(channels(&mut window), Some(vec![("general".to_owned(), Some("Chat".to_owned()))]));
    }

    #[test]
    fn a_conversation_with_someone_gone_is_read_but_not_written() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        conversation_told(&mut window, &clock, owls, conversation_wire("c-gone", None, false));
        let todo =
            tell(&mut window, &clock, owls, talk(TalkMessage::Conversations(Ok(wire::Conversations::default()))));
        assert!(asks(&todo).is_empty());
        let id = terms::ConversationId::parse("c-gone").unwrap();
        tell(&mut window, &clock, owls, talk(TalkMessage::OpenConversation(id)));
        let place = &window.places()[0];
        let open = place.discussion().expect("open");
        let gone = place.composer_gone(open).expect("the line stands in place of the composer");
        assert_eq!(gone.name, None, "their name was never known: the front end says it as a former member");
        assert!(!place.may_send(place::Pane::Content));
        assert!(!place.may_attach(place::Pane::Content));
        assert!(!place.may_react(place::Pane::Content));
        assert!(place.grants_of(&open.owner).holds(Grant::ChannelRead), "still read");
    }

    #[test]
    fn a_member_who_leaves_takes_their_conversation_s_composer_with_them_and_leaves_their_name() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
        let id = terms::ConversationId::parse("c-bo").unwrap();
        tell(&mut window, &clock, owls, talk(TalkMessage::OpenConversation(id)));
        assert!(window.places()[0].may_send(place::Pane::Content));
        let from = window.places()[0].discussion().and_then(|open| open.from);
        assert_eq!(from, Some(Roll::Conversations), "opened from no list, its back arrow still leads to its list");
        tell(&mut window, &clock, owls, place::Message::People(place::PeopleMessage::Left(member("bo"))));
        let place = &window.places()[0];
        let gone = place.composer_gone(place.discussion().unwrap()).expect("no composer now");
        assert_eq!(gone.name.as_deref(), Some("Bo"), "the name it was known by");
        let open = place.discussion().unwrap();
        let bo = chat::Author::Member(member("bo"));
        assert_eq!(place.author_shown(open, &bo).as_deref(), Some("Bo"), "and their messages keep it");
    }

    #[test]
    fn a_message_told_in_a_conversation_is_its_rows_preview_and_a_first_message_is_found() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
        let direct = place::DiscussionMessage::Told {
            timeline: timeline_id("t-c-bo"),
            message: Box::new(said(2, 2, "bo", "**psst** over here")),
        };
        tell(&mut window, &clock, owls, discussion(direct));
        let place::Rows::Ready(rows) = window.places()[0].conversation_rows() else { panic!("listed") };
        assert_eq!(rows[0].preview.map(|line| (line.mine, line.text)), Some((false, "psst over here")));
        let mine = place::DiscussionMessage::Told {
            timeline: timeline_id("t-c-bo"),
            message: Box::new(said(3, 3, "Ada", "ok")),
        };
        tell(&mut window, &clock, owls, discussion(mine));
        let place::Rows::Ready(rows) = window.places()[0].conversation_rows() else { panic!("listed") };
        assert_eq!(rows[0].preview.map(|line| (line.mine, line.text)), Some((true, "ok")));
    }
}
