//! What the server names a member, a channel, a category, a timeline, an invite, a role, a message or a webhook by: an
//! opaque id, read once at the edge, never empty, and of one kind, so a channel's id is never passed where a member's is
//! asked.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Something a server names by an id.
pub trait Kind {
    /// What a printed id says it names.
    const NAME: &'static str;
}

/// An id of a `K`, as the server spelled it. Its order only keeps a map's iteration steady; it means nothing.
pub struct Id<K>(String, PhantomData<fn() -> K>);

impl<K> Id<K> {
    /// The id as the wire names it, or none when it is empty: "unknown" is never an empty id.
    pub fn parse(id: &str) -> Option<Self> {
        (!id.is_empty()).then(|| Self(id.to_owned(), PhantomData))
    }

    /// The id as the wire takes it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<K> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K> Eq for Id<K> {}

impl<K> Hash for Id<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<K> PartialOrd for Id<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Id<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<K: Kind> fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:?})", K::NAME, self.0)
    }
}

/// An account at a server.
pub enum Member {}
/// A text or voice channel.
pub enum Channel {}
/// A named group of channels in the list.
pub enum Category {}
/// An invite to a server.
pub enum Invite {}
/// A role a member holds.
pub enum Role {}
/// One stream of messages, which a channel owns.
pub enum Timeline {}
/// A message in a channel.
pub enum Message {}
/// Private talk between members of a server, in a timeline of its own.
pub enum Conversation {}
/// A URL a program posts to a channel through.
pub enum Webhook {}

impl Kind for Member {
    const NAME: &'static str = "MemberId";
}
impl Kind for Channel {
    const NAME: &'static str = "ChannelId";
}
impl Kind for Category {
    const NAME: &'static str = "CategoryId";
}
impl Kind for Invite {
    const NAME: &'static str = "InviteId";
}
impl Kind for Role {
    const NAME: &'static str = "RoleId";
}
impl Kind for Timeline {
    const NAME: &'static str = "TimelineId";
}
impl Kind for Message {
    const NAME: &'static str = "MessageId";
}
impl Kind for Conversation {
    const NAME: &'static str = "ConversationId";
}
impl Kind for Webhook {
    const NAME: &'static str = "WebhookId";
}

/// An account's id at its server.
pub type MemberId = Id<Member>;
/// A channel's id at its server.
pub type ChannelId = Id<Channel>;
/// A category's id at its server.
pub type CategoryId = Id<Category>;
/// An invite's id at its server.
pub type InviteId = Id<Invite>;
/// A role's id at its server.
pub type RoleId = Id<Role>;
/// A message's id at its server.
pub type MessageId = Id<Message>;
/// A timeline's id at its server.
pub type TimelineId = Id<Timeline>;
/// A conversation's id at its server.
pub type ConversationId = Id<Conversation>;
/// A webhook's id at its server.
pub type WebhookId = Id<Webhook>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn an_id_is_none_when_empty_and_one_spelling_is_one_id() {
        assert!(MemberId::parse("").is_none(), "unknown is no id, never an empty one");
        let ada = MemberId::parse("7").expect("an id");
        assert_eq!(ada.as_str(), "7");
        assert_eq!(ada, MemberId::parse("7").expect("an id"));
        assert_ne!(ada, MemberId::parse("07").expect("an id"), "an id has one spelling");
        assert_eq!(format!("{ada:?}"), r#"MemberId("7")"#);
        assert_eq!(HashSet::from([ada.clone(), ada]).len(), 1);
    }
}
