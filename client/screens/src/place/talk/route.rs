//! What the place does with each message about conversations and threads.

use super::*;

/// What the Message button of a member's popover and the rows of the lists do, and what the server answers.
#[derive(Clone, Debug)]
pub enum TalkMessage {
    /// A list opened in the content area, from the side pane's row.
    Show(Roll),
    /// The conversations the server listed, or why not.
    Conversations(Result<wire::Conversations, LinkError>),
    /// The threads the server says this account follows, or why not.
    Followed(Result<wire::FollowedThreads, LinkError>),
    /// A conversation was opened with this account in it, by itself or by the other member.
    Opened(Box<wire::Conversation>),
    /// A timeline's standing, as the server tells this account's sockets.
    Standing {
        /// The timeline.
        timeline: TimelineId,
        /// The standing now.
        standing: TimelineStanding,
    },
    /// The member's popover's "Message": their conversation opens, made first if there is none.
    Message(MemberId),
    /// The server's answer to opening a conversation.
    Made(Result<Box<wire::Conversation>, LinkError>),
    /// A conversation's row pressed in the list, by its id.
    OpenConversation(ConversationId),
    /// A thread's row pressed in the list, by its timeline.
    OpenFollowed(TimelineId),
    /// A message's thread chip pressed: the thread opens beside the channel.
    OpenThread(MessageId),
    /// "Reply in thread" on a message's menu: its thread opens beside the channel, a message with none opening a
    /// composer that begins it with its first reply.
    StartThread(MessageId),
    /// The thread pane's close.
    CloseThread,
    /// The back arrow of a conversation or a thread opened from a list: the list shows again.
    Back,
    /// "Follow thread" or "Unfollow thread" on a thread's header.
    Follow {
        /// The thread's timeline.
        timeline: TimelineId,
        /// Follow, else stop following.
        follow: bool,
    },
    /// The server's answer to following or unfollowing.
    FollowAnswered {
        /// The thread's timeline.
        timeline: TimelineId,
        /// What was asked.
        follow: bool,
        /// The answer.
        answer: Result<(), LinkError>,
    },
    /// Whether this device alerts for the account, as the server tells it.
    DeviceInUse(DeviceUse),
    /// The account's own status choice, as the server tells its sockets.
    StatusChosen(proto::status::Chosen),
    /// The server's answer to a status asked from the status menu.
    StatusSet(Result<(), LinkError>),
}

impl Place {
    /// The list open in the content area in place of a discussion, if one is.
    pub fn roll(&self) -> Option<Roll> {
        match &self.status {
            Status::Signed(signed) => signed.content.roll(),
            Status::Reconnecting(_) | Status::Door(_) => None,
        }
    }

    /// Opens what owns the timeline `timeline` in the content area, as a popup's click asks: its channel, its
    /// conversation or its thread; nothing where it is none this place knows.
    pub(crate) fn open_timeline(&mut self, timeline: &TimelineId, zone: chat::TimeZone) -> Vec<Ask> {
        match self.owner_of(timeline) {
            Some(Owner::Channel(id)) => self.open_discussion(&id, zone),
            Some(Owner::Conversation(id)) => self.open_conversation(&id),
            Some(Owner::Thread { channel, root }) => {
                let preview = match &self.status {
                    Status::Signed(signed) => signed.talk.thread(timeline).and_then(|thread| thread.preview.clone()),
                    Status::Reconnecting(_) | Status::Door(_) => None,
                };
                let owner = Owner::Thread { channel, root: root.clone() };
                self.seat(owner, timeline.clone(), Some(Root::Brief { message: root, preview }), None)
            }
            None => Vec::new(),
        }
    }

    /// A list opened in the content area: the discussion open leaves with where it was scrolled.
    fn show_roll(&mut self, roll: Roll) -> Vec<Ask> {
        let served = match roll {
            Roll::Threads => self.served.on(proto::rules::Switch::Threads),
            Roll::Conversations => self.served.on(proto::rules::Switch::Conversations),
        };
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if !served {
            return Vec::new();
        }
        let mut todo = signed.show(Content::Roll(roll), &mut self.held, &self.roles);
        // A list that failed is asked for again by opening it.
        let again = match roll {
            Roll::Threads => matches!(signed.talk.followed, Loaded::Failed(_)).then_some(ops::list_followed()),
            Roll::Conversations => {
                matches!(signed.talk.conversations, Loaded::Failed(_)).then_some(ops::list_conversations())
            }
        };
        if let Some(ask) = again {
            match roll {
                Roll::Threads => signed.talk.followed = Loaded::Loading,
                Roll::Conversations => signed.talk.conversations = Loaded::Loading,
            }
            todo.push(ask);
        }
        todo
    }

    /// The back arrow: a discussion reached from a list closes and the list shows again.
    fn back_to_roll(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let Some(from) = signed.content.discussion().and_then(|open| open.from) else { return Vec::new() };
        signed.show(Content::Roll(from), &mut self.held, &self.roles)
    }

    /// Opens the discussion of `owner`'s `timeline` in the content area in place of what is there, the list it was
    /// reached from kept so the back arrow returns to it; what asks first is returned.
    pub(crate) fn seat(
        &mut self,
        owner: Owner,
        timeline: TimelineId,
        root: Option<Root>,
        from: Option<Roll>,
    ) -> Vec<Ask> {
        let zone = self.zone.clone();
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if signed.content.discussion().is_some_and(|open| open.owner == owner) {
            return close_page(signed, &self.roles);
        }
        let tally = signed.tally(&owner, &timeline);
        let scroll = self.held.get(&Compose::Timeline(timeline.clone())).map(|held| held.scroll.clone());
        let scroll = scroll.unwrap_or_default();
        let open = Discussion::new(owner, timeline, zone, scroll).tally(tally).reached(from, root);
        let first = open.first_page();
        let mut todo = signed.show(Content::seated(open), &mut self.held, &self.roles);
        todo.extend(first);
        todo
    }

    /// The conversation or thread read through: its place in the lists follows what is told.
    pub(in crate::place) fn on_talk(&mut self, message: TalkMessage) -> Vec<Ask> {
        match message {
            TalkMessage::Show(roll) => self.show_roll(roll),
            TalkMessage::Conversations(answer) => self.conversations_listed(answer),
            TalkMessage::Followed(answer) => self.followed_listed(answer),
            TalkMessage::Opened(told) => self.conversation_opened(&told),
            TalkMessage::Standing { timeline, standing } => self.timeline_standing(&timeline, standing),
            TalkMessage::Message(user_id) => self.message_member(&user_id),
            TalkMessage::Made(answer) => self.conversation_made(answer),
            TalkMessage::OpenConversation(id) => self.open_conversation(&id),
            TalkMessage::OpenFollowed(timeline) => self.open_followed(&timeline),
            TalkMessage::OpenThread(message) => self.open_thread(&message),
            TalkMessage::StartThread(message) => self.start_thread(&message),
            TalkMessage::CloseThread => self.close_thread(),
            TalkMessage::Back => self.back_to_roll(),
            TalkMessage::Follow { timeline, follow } => self.follow_thread(&timeline, follow),
            TalkMessage::FollowAnswered { timeline, follow, answer } => self.follow_answered(&timeline, follow, answer),
            TalkMessage::DeviceInUse(device) => {
                self.device = Some(device);
                Vec::new()
            }
            TalkMessage::StatusChosen(chosen) => {
                self.status_chosen(chosen);
                Vec::new()
            }
            // The window says a refusal; what the account chose comes as `StatusChosen`.
            TalkMessage::StatusSet(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, conversation_told, conversation_wire, tell};
    use crate::place;
    use crate::place::talk::tests::{talk, talkative};
    use crate::place::{Panel, Roll, TalkMessage};
    use proto::chatkord_v1 as wire;
    use std::time::Instant;

    #[test]
    fn a_list_opens_in_the_content_area_and_a_conversation_from_it_goes_back_to_it() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        for (id, with, unread, last) in [("c-old", "bo", 0, 10), ("c-new", "cy", 0, 50), ("c-unread", "di", 2, 5)] {
            let mut told = conversation_wire(id, Some(with), true);
            told.last =
                Some(wire::Preview { user_id: Some(with.into()), text: format!("from {with}"), created_at: last });
            told.standing = Some(wire::TimelineStanding {
                timeline_id: told.timeline_id.clone(),
                read: 1,
                unread,
                mentions: unread,
                followed: false,
            });
            conversation_told(&mut window, &clock, owls, told);
        }
        tell(&mut window, &clock, owls, talk(TalkMessage::Show(Roll::Conversations)));
        let place = &window.places()[0];
        assert_eq!((place.panel(), place.roll()), (Panel::Conversations, Some(Roll::Conversations)));
        assert!(place.discussion().is_none(), "the channel gave way to the list");
        let place::Rows::Ready(rows) = place.conversation_rows() else { panic!("listed") };
        let order: Vec<&str> = rows.iter().map(|row| row.conversation.id.as_str()).collect();
        assert_eq!(order, ["c-unread", "c-new", "c-old"], "unread first, then newest");
        assert_eq!(place.conversations_unread(), 2);
        assert_eq!(rows[1].preview.map(|line| (line.mine, line.text)), Some((false, "from cy")));
        let id = terms::ConversationId::parse("c-new").unwrap();
        tell(&mut window, &clock, owls, talk(TalkMessage::OpenConversation(id)));
        let place = &window.places()[0];
        assert_eq!(
            (place.panel(), place.discussion().and_then(|open| open.from)),
            (Panel::Discussion, Some(Roll::Conversations))
        );
        tell(&mut window, &clock, owls, talk(TalkMessage::Back));
        assert_eq!(window.places()[0].panel(), Panel::Conversations, "back to the list it came from");
    }
}
