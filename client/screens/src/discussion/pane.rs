//! Which discussion a pane shows, its title, and a discussion opening and leaving.

use super::*;

/// Which of a server's open discussions a message is for: the one in the content area, the call's own beside the call,
/// or the thread beside the channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    /// The channel, conversation or thread open in the content area.
    Content,
    /// The voice channel's messages beside its call.
    CallChat,
    /// The thread beside the channel in the content area.
    Thread,
}

impl Pane {
    /// Every pane a discussion can be open in.
    pub const ALL: [Pane; 3] = [Pane::Content, Pane::CallChat, Pane::Thread];

    /// `said` as its place takes it for the discussion in this pane.
    pub fn carrying(self, said: DiscussionMessage) -> super::Message {
        match self {
            Pane::Content => super::Message::Discussion(said),
            Pane::CallChat => super::Message::CallChat(said),
            Pane::Thread => super::Message::Thread(said),
        }
    }
}

/// What a discussion is called, in its header and its composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Title {
    /// A channel, by its name.
    Channel(String),
    /// A conversation, by the other person's name; none for someone gone whose name was never known.
    Person(Option<String>),
    /// A thread, by the name of the channel it is in; none while the list does not hold the channel.
    Thread {
        /// The channel's name.
        channel: Option<String>,
    },
}

/// Why a conversation has no composer: its other member is gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerGone {
    /// Their name; none when it was never known, which the front end says as "Former member".
    pub name: Option<String>,
}

impl Place {
    /// The text channel open here.
    pub fn discussion(&self) -> Option<&Discussion> {
        match &self.status {
            Status::Signed(signed) => signed.content.discussion(),
            Status::Reconnecting(again) => again.discussion.as_ref(),
            Status::Door(_) => None,
        }
    }

    /// The voice channel's messages beside its call, while that column is open.
    pub fn call_chat(&self) -> Option<&Discussion> {
        match &self.status {
            Status::Signed(signed) => signed.call_chat.as_ref(),
            Status::Reconnecting(_) | Status::Door(_) => None,
        }
    }

    /// The open channel's messages drawn at another size or spacing.
    pub(crate) fn messages_rescaled(&mut self) {
        let open: Vec<&mut Discussion> = match &mut self.status {
            Status::Signed(signed) => signed.open_mut().collect(),
            Status::Reconnecting(again) => again.discussion.iter_mut().collect(),
            Status::Door(_) => Vec::new(),
        };
        open.into_iter().for_each(Discussion::rescaled);
    }

    /// The sends not yet answered from `compose`, oldest first.
    pub fn unsent(&self, compose: &Compose) -> &[Unsent] {
        self.held.get(compose).map_or(&[], |held| held.unsent.as_slice())
    }

    /// What the composer holds for `compose`.
    pub fn composing(&self, compose: &Compose) -> Option<&Composing> {
        self.held.get(compose).map(|held| &held.composing)
    }

    /// The discussion `pane` names.
    pub fn in_pane(&self, pane: Pane) -> Option<&Discussion> {
        match pane {
            Pane::Content => self.discussion(),
            Pane::CallChat => self.call_chat(),
            Pane::Thread => self.thread(),
        }
    }

    /// What the discussion `open` is called: its channel's name, the other person's, or its thread's channel's.
    pub fn title(&self, open: &Discussion) -> Title {
        match &open.owner {
            Owner::Channel(id) => Title::Channel(self.channel_name(id).unwrap_or_default().to_owned()),
            Owner::Conversation(id) => {
                Title::Person(self.conversation(id).and_then(|conversation| self.conversation_name(conversation)))
            }
            Owner::Thread { channel, .. } => Title::Thread { channel: self.channel_name(channel).map(str::to_owned) },
        }
    }

    /// The name of the channel `id`, of either kind, as the list last said.
    pub fn channel_name(&self, id: &ChannelId) -> Option<&str> {
        let Some(Lobby::Ready(listed)) = self.lobby() else { return None };
        match listed.find(id)? {
            ListEntry::Text(channel) => Some(channel.name.as_str()),
            ListEntry::Voice(channel) => Some(channel.name.as_str()),
            ListEntry::Category(_) => None,
        }
    }

    /// Why the discussion `open` has no composer, if it has none: its conversation's other member is gone.
    pub fn composer_gone(&self, open: &Discussion) -> Option<ComposerGone> {
        let Owner::Conversation(id) = &open.owner else { return None };
        let conversation = self.conversation(id).filter(|conversation| !conversation.sendable)?;
        Some(ComposerGone { name: self.conversation_name(conversation) })
    }

    /// The name the writer of a message in `open` is shown by: a member's own, else, in a conversation, the name its
    /// other member was known by; none for anyone else who is gone.
    pub fn author_shown(&self, open: &Discussion, author: &chat::Author) -> Option<String> {
        let member = match author {
            chat::Author::Member(user_id) => self.directory().person(user_id).map(|person| person.name.clone()),
            chat::Author::App { name, .. } => Some(name.clone()),
            chat::Author::Former(_) => None,
        };
        member.or_else(|| self.composer_gone(open).and_then(|gone| gone.name))
    }

    /// Opens the text channel `channel_id`, its days read in `zone`; the call, if any, goes on in the pane.
    pub(crate) fn open_discussion(&mut self, channel_id: &ChannelId, zone: TimeZone) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if signed.content.discussion().is_some_and(|open| open.owner == Owner::Channel(channel_id.clone())) {
            return close_page(signed, &self.roles);
        }
        let Some((open, first)) = reading(signed, &self.held, channel_id, zone) else { return Vec::new() };
        self.held.entry(open.compose.clone()).or_default().composing.focus_asked += 1;
        let mut todo = signed.show(Content::seated(open), &mut self.held, &self.roles);
        todo.push(first);
        todo
    }

    /// Whether a channel mark naming `channel` opens it for this reader: they see the channel now. A mark of one they
    /// cannot see, or one gone, is its text alone, drawn with no link and no press.
    pub fn opens_channel(&self, channel: &ChannelId) -> bool {
        let Some(Lobby::Ready(listed)) = self.lobby() else { return false };
        listed.text.iter().any(|seen| seen.id == *channel) || listed.voice.iter().any(|seen| seen.id == *channel)
    }

    /// The discussion `pane` names, while signed in.
    pub(crate) fn pane_mut(&mut self, pane: Pane) -> Option<&mut Discussion> {
        let Status::Signed(signed) = &mut self.status else { return None };
        match pane {
            Pane::Content => signed.content.discussion_mut(),
            Pane::CallChat => signed.call_chat.as_mut(),
            Pane::Thread => signed.content.beside().and_then(Option::as_mut),
        }
    }
}

/// The discussion of the channel `channel_id` as it opens, with the ask for its first page.
pub(crate) fn reading(
    signed: &SignedIn,
    held: &std::collections::HashMap<Compose, Held>,
    channel_id: &ChannelId,
    zone: TimeZone,
) -> Option<(Discussion, Ask)> {
    let Lobby::Ready(listed) = &signed.lobby else { return None };
    let channel = timeline_of(listed, channel_id)?;
    let owner = Owner::Channel(channel.id.clone());
    let tally = signed.tally(&owner, &channel.timeline);
    let scroll = held.get(&Compose::Timeline(channel.timeline.clone())).map(|held| held.scroll.clone());
    let open = Discussion::new(owner, channel.timeline, zone, scroll.unwrap_or_default()).tally(tally);
    let first = open.first_page()?;
    Some((open, first))
}

/// The channel `id` of either kind as a discussion holds it: a voice channel's messages are written beside its call.
pub(crate) fn timeline_of(listed: &ChannelList, id: &ChannelId) -> Option<TextChannel> {
    let text = listed.text.iter().find(|channel| channel.id == *id).cloned();
    text.or_else(|| {
        let voice = listed.voice.iter().find(|channel| channel.id == *id)?;
        Some(TextChannel {
            id: voice.id.clone(),
            name: voice.name.clone(),
            timeline: voice.timeline.clone(),
            revision: 0,
        })
    })
}

/// What a discussion closed leaves to its timeline: where it was scrolled, so it reopens there.
pub(crate) fn leave(held: &mut std::collections::HashMap<Compose, Held>, left: Discussion) {
    held.entry(left.compose).or_default().scroll = left.scroll;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discussion::tests::timeline;
    use crate::harness::{Clock, chatting, discussion, message_id, tell};
    use crate::place;
    use crate::place::Panel;
    use std::time::Instant;

    #[test]
    fn a_text_channel_is_listed_and_opened() {
        let clock = Clock(Instant::now());
        let (window, _) = chatting(&clock);
        let place = &window.places()[0];
        assert_eq!(place.panel(), Panel::Discussion);
        let Some(Lobby::Ready(listed)) = place.lobby() else { panic!("listed") };
        assert_eq!(listed.text.len(), 1);
        assert_eq!(timeline(&window).len(), 1);
    }

    #[test]
    fn copying_a_message_puts_its_text_on_the_clipboard() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Copy(message_id(1))));
        assert_eq!(window.copying(), Some("hello"));
    }
}
