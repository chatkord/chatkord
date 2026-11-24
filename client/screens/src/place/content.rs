//! What the content area shows under the pages over it: one thing at a time, and the side pane's row for it.

use super::{Call, Discussion, Roll};
use terms::ChannelId;

/// What the content area shows under the pages: one thing at a time, so showing one puts the other away.
#[derive(Debug, Default)]
pub(crate) enum Content {
    /// The call's own page: the call open here, or the channels' panel while there is none.
    #[default]
    Call,
    /// A text channel, a conversation or a thread, and the thread open beside a channel.
    Discussion(Box<Seated>),
    /// The list of the threads followed or of the conversations.
    Roll(Roll),
}

/// A discussion in the content area, and the thread opened beside it.
#[derive(Debug)]
pub(crate) struct Seated {
    /// The discussion itself.
    pub(super) open: Discussion,
    /// The thread beside it, which goes with it.
    pub(super) thread: Option<Discussion>,
}

/// The row the side pane marks for what the content area shows: never more than one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Marked {
    /// The row that opens this list, for the list or a discussion reached from it.
    Roll(Roll),
    /// A channel's row: the text channel open, or the voice channel whose live call shows.
    Channel(ChannelId),
}

impl Marked {
    /// The row a discussion open in the content area marks: the list it was reached from, else its own channel.
    pub(super) fn of(open: &Discussion) -> Option<Marked> {
        open.from.map(Marked::Roll).or_else(|| open.owner.channel().cloned().map(Marked::Channel))
    }
}

impl Content {
    /// `open` in the content area, nothing beside it.
    pub(crate) fn seated(open: Discussion) -> Content {
        Content::Discussion(Box::new(Seated { open, thread: None }))
    }

    /// Whether it is the call's own page.
    pub(super) fn is_call(&self) -> bool {
        matches!(self, Content::Call)
    }

    /// The discussion shown, if one is.
    pub(crate) fn discussion(&self) -> Option<&Discussion> {
        match self {
            Content::Discussion(seated) => Some(&seated.open),
            Content::Call | Content::Roll(_) => None,
        }
    }

    /// The same, to change.
    pub(crate) fn discussion_mut(&mut self) -> Option<&mut Discussion> {
        match self {
            Content::Discussion(seated) => Some(&mut seated.open),
            Content::Call | Content::Roll(_) => None,
        }
    }

    /// The thread beside the discussion shown, if one is open.
    pub(crate) fn thread(&self) -> Option<&Discussion> {
        match self {
            Content::Discussion(seated) => seated.thread.as_ref(),
            Content::Call | Content::Roll(_) => None,
        }
    }

    /// Where the thread beside the discussion shown goes: none while no discussion shows.
    pub(crate) fn beside(&mut self) -> Option<&mut Option<Discussion>> {
        match self {
            Content::Discussion(seated) => Some(&mut seated.thread),
            Content::Call | Content::Roll(_) => None,
        }
    }

    /// The list shown, if one is.
    pub(super) fn roll(&self) -> Option<Roll> {
        match self {
            Content::Roll(roll) => Some(*roll),
            Content::Call | Content::Discussion(_) => None,
        }
    }

    /// The discussions it holds: the one shown, then the thread beside it.
    pub(super) fn discussions(&self) -> impl Iterator<Item = &Discussion> {
        let seated = match self {
            Content::Discussion(seated) => Some(&**seated),
            Content::Call | Content::Roll(_) => None,
        };
        seated.into_iter().flat_map(|seated| std::iter::once(&seated.open).chain(seated.thread.as_ref()))
    }

    /// The same, to change.
    pub(super) fn discussions_mut(&mut self) -> impl Iterator<Item = &mut Discussion> {
        let seated = match self {
            Content::Discussion(seated) => Some(&mut **seated),
            Content::Call | Content::Roll(_) => None,
        };
        seated.into_iter().flat_map(|seated| std::iter::once(&mut seated.open).chain(seated.thread.as_mut()))
    }

    /// The discussions it holds, given up as it is put away: the one shown, then the thread beside it.
    pub(super) fn into_discussions(self) -> impl Iterator<Item = Discussion> {
        let seated = match self {
            Content::Discussion(seated) => Some(*seated),
            Content::Call | Content::Roll(_) => None,
        };
        seated.into_iter().flat_map(|seated| std::iter::once(seated.open).chain(seated.thread))
    }

    /// The row the side pane marks for it, with `call` the call open here: on the call's page, its channel while the
    /// call is live.
    pub(super) fn marked(&self, call: Option<&Call>) -> Option<Marked> {
        match self {
            Content::Call => call.filter(|call| call.live()).map(|call| Marked::Channel(call.channel.id.clone())),
            Content::Discussion(seated) => Marked::of(&seated.open),
            Content::Roll(roll) => Some(Marked::Roll(*roll)),
        }
    }
}
