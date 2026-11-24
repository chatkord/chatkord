//! Talk beside the channels: conversations between members and the threads started on a channel's messages, the lists
//! that show them, what this account has read in each, and which device of the account alerts. A conversation or a
//! thread is a timeline like a channel's and opens in a [`Discussion`]; what the discussion's owner decides is read
//! from its [`Owner`].

use super::*;
use crate::loaded::Loaded;
use proto::chatkord_v1 as wire;
use proto::rules::{CONVERSATION_GRANTS, CONVERSATION_WRITING, Switch};

mod conversations;
mod followed;
mod idle;
mod route;
mod standing;

pub use conversations::*;
pub use followed::*;
pub use idle::*;
pub use route::*;
pub use standing::*;

/// A list the content area shows in place of a discussion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Roll {
    /// The threads this account follows.
    Threads,
    /// This account's conversations.
    Conversations,
}

/// What this account reads and keeps of conversations and followed threads.
#[derive(Debug, Default)]
pub struct Talk {
    /// The conversations, in the order the server gave them.
    pub(crate) conversations: Loaded<Vec<Conversation>>,
    /// The followed threads, the most recently active first.
    pub(super) followed: Loaded<Vec<FollowedThread>>,
    /// What this account has read of each conversation's and thread's timeline.
    pub(crate) standings: std::collections::HashMap<TimelineId, TimelineStanding>,
}

/// What owns the timeline a discussion shows, from which its grants, standing, name and composer are read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    /// A channel of either kind: a voice channel's messages are written beside its call.
    Channel(ChannelId),
    /// A conversation between members.
    Conversation(ConversationId),
    /// A thread, started on a message of a channel.
    Thread {
        /// The channel its root is in, whose words it asks.
        channel: ChannelId,
        /// Its root message.
        root: MessageId,
    },
}

impl Owner {
    /// The channel it is a channel's own: none for a conversation or a thread.
    pub fn channel(&self) -> Option<&ChannelId> {
        match self {
            Owner::Channel(id) => Some(id),
            Owner::Conversation(_) | Owner::Thread { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, bo_arrives, chatting, every_switch_on};
    use crate::place;
    use crate::place::TalkMessage;
    use crate::window::Ask;

    pub(super) fn talk(message: TalkMessage) -> place::Message {
        place::Message::Talk(message)
    }

    pub(super) fn asks(todo: &[Ask]) -> Vec<&place::Ask> {
        todo.iter().filter_map(|done| if let Ask::At(_, asked) = done { Some(asked) } else { None }).collect()
    }

    /// Ada chatting in #general at a server that offers it all, Bo a member.
    pub(super) fn talkative(clock: &Clock) -> (crate::window::Window, place::PlaceId) {
        let (mut window, owls) = chatting(clock);
        every_switch_on(&mut window, clock, owls);
        bo_arrives(&mut window, clock, owls);
        (window, owls)
    }
}
