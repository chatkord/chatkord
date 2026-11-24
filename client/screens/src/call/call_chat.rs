//! A voice channel's messages beside its call, as Discord's text in voice: open in a column on the call's right while
//! a call is shown here and the person keeps the column open, its unread counted on the call header's Chat button.

use super::*;

impl Place {
    /// The call's channel's standing, whose counts the call header's Chat button shows while its column is shut.
    pub fn call_chat_standing(&self) -> Option<ChannelStanding> {
        let call = self.call()?;
        self.channel_standing(&call.channel.id)
    }

    /// Whether the call header's Chat button holds a dot: the call's chat is shut over messages unread.
    pub fn call_chat_dot(&self) -> bool {
        self.call_chat().is_none() && self.call_chat_standing().is_some_and(|standing| standing.unread > 0)
    }

    /// Opens the call's channel's messages beside it while `wanted` and a call is here, else puts them away, keeping
    /// where they were scrolled; what opening asks first is returned.
    pub(crate) fn follow_call_chat(&mut self, wanted: bool, zone: &chat::TimeZone) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let channel = signed.call.as_ref().map(|call| call.channel.id.clone()).filter(|_| wanted);
        let open = signed.call_chat.as_ref().and_then(|open| match &open.owner {
            Owner::Channel(id) => Some(id.clone()),
            Owner::Conversation(_) | Owner::Thread { .. } => None,
        });
        if channel == open {
            return Vec::new();
        }
        if let Some(left) = signed.call_chat.take() {
            leave(&mut self.held, left);
        }
        let Some(id) = channel else { return Vec::new() };
        let Lobby::Ready(listed) = &signed.lobby else { return Vec::new() };
        let Some(channel) = timeline_of(listed, &id) else { return Vec::new() };
        let scroll = self.held.get(&Compose::Timeline(channel.timeline.clone())).map(|held| held.scroll.clone());
        let owner = Owner::Channel(channel.id.clone());
        let tally = signed.tally(&owner, &channel.timeline);
        let open = Discussion::new(owner, channel.timeline, zone.clone(), scroll.unwrap_or_default()).tally(tally);
        let first = open.first_page();
        signed.call_chat = Some(open);
        first.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{self, ADA, Clock, said};
    use crate::place::{self, DiscussionMessage};
    use crate::window::{Ask, Message};
    use std::time::Instant;

    /// The call's chat opens beside it on the Chat button, asks for its channel's newest page, and shows the page
    /// when it comes, though the answer names no pane: it is the call's channel's.
    #[test]
    fn the_call_chat_asks_for_its_channels_messages_and_shows_them() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = harness::ada_in_call(&clock);
        let lounge = terms::TimelineId::parse("t-lounge").unwrap();
        let asked = window.update(Message::ToggleCallChat, clock.at());
        assert!(
            asked
                .iter()
                .any(|ask| matches!(ask, Ask::At(_, ask) if crate::harness::is(ask, crate::harness::Op::History) && matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.timeline_id == lounge.as_str()))),
            "{asked:?}"
        );
        let page =
            chat::Page { messages: vec![said(1, 3, ADA, "voice chat here")], older: false, newer: false, revision: 3 };
        let answered = DiscussionMessage::Page { timeline: lounge, end: None, page: Ok(page) };
        harness::tell(&mut window, &clock, owls, place::Message::Discussion(answered));
        let held = window.places()[0].call_chat().and_then(|open| open.timeline()).map(|held| held.messages().len());
        assert_eq!(held, Some(1), "the page reaches the call's chat");
    }

    /// The Chat button holds a dot while the chat is shut over messages unread, and none once it is open.
    #[test]
    fn the_chat_button_holds_a_dot_while_the_chat_is_shut_over_unread() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = harness::ada_in_call(&clock);
        assert!(!window.places()[0].call_chat_dot(), "nothing unread");
        let standing = place::ChannelStanding { read: 4, unread: 2, ..place::ChannelStanding::default() };
        let channel_id = harness::channel_id("lounge");
        harness::tell(
            &mut window,
            &clock,
            owls,
            place::Message::Lobby(place::LobbyMessage::Standing { channel_id, standing }),
        );
        assert!(window.places()[0].call_chat_dot(), "shut over two unread");
        window.update(Message::ToggleCallChat, clock.at());
        assert!(!window.places()[0].call_chat_dot(), "open");
    }

    /// The column's own close button puts it away where it docks too, not only over the call.
    #[test]
    fn the_call_chats_close_button_closes_it_docked_beside_the_call() {
        let clock = Clock(Instant::now());
        let (mut window, _) = harness::ada_in_call(&clock);
        window.set_room(crate::frame::Room { width: 1440.0, height: 900.0 });
        window.update(Message::ToggleCallChat, clock.at());
        assert_eq!(window.look().call_chat.shown, choices::CallChatShown::Open, "opened by its button");
        let kept = window.update(Message::CloseCallChat, clock.at());
        assert_eq!(window.look().call_chat.shown, choices::CallChatShown::Closed, "closed by its own");
        assert!(matches!(&kept[..], [Ask::Keep(_)]), "and kept closed: {kept:?}");
        assert!(window.update(Message::CloseCallChat, clock.at()).is_empty(), "closed already, nothing to keep");
    }

    /// One column at a time beside a call: the members button, pressed while the chat shows, puts the chat away
    /// and shows the members; the chat button takes the place back.
    #[test]
    fn beside_a_call_the_members_take_the_chats_place_and_the_chat_theirs() {
        let clock = Clock(Instant::now());
        let (mut window, _) = harness::ada_in_call(&clock);
        let room = crate::frame::Room { width: 1440.0, height: 900.0 };
        window.set_room(room);
        window.update(Message::ToggleCallChat, clock.at());
        let frame = window.frame(room);
        assert!(frame.call_chat.is_some() && frame.members.is_none(), "the chat alone: {frame:?}");
        window.update(Message::ToggleMembers, clock.at());
        let frame = window.frame(room);
        assert!(frame.call_chat.is_none() && frame.members.is_some(), "the members alone: {frame:?}");
        window.update(Message::ToggleCallChat, clock.at());
        let frame = window.frame(room);
        assert!(frame.call_chat.is_some() && frame.members.is_none(), "the chat again: {frame:?}");
    }
}
