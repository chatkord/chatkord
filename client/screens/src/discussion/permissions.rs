//! What this account may do in a discussion, as the words of its owner say.

use super::*;

impl Place {
    /// What this account may do in the discussion `pane` names, as its owner's words say: nothing with none open there.
    pub(super) fn discussion_grants(&self, pane: Pane) -> Grants {
        self.in_pane(pane).map_or(Grants::NONE, |open| self.grants_of(&open.owner))
    }

    /// Whether this account may send in the discussion `pane` names.
    pub fn may_send(&self, pane: Pane) -> bool {
        self.discussion_grants(pane).holds(Grant::SendMessage)
    }

    /// Whether `message` is this account's own.
    pub fn wrote(&self, message: &chat::Message) -> bool {
        wrote(message, self.me())
    }

    /// Whether this account may edit `message` in the discussion `pane` names: its own, not deleted.
    pub fn may_edit(&self, pane: Pane, message: &chat::Message) -> bool {
        may_edit(message, self.me(), self.discussion_grants(pane))
    }

    /// Whether this account may delete `message` in the discussion `pane` names: its own, or anyone's with the word to.
    pub fn may_delete(&self, pane: Pane, message: &chat::Message) -> bool {
        may_delete(message, self.me(), self.discussion_grants(pane))
    }

    /// Whether the menu of the discussion `pane` names offers Pin or Unpin for `message`: with the word to pin there.
    pub fn may_pin(&self, pane: Pane, message: &chat::Message) -> bool {
        !message.deleted() && self.discussion_grants(pane).holds(Grant::PinMessages)
    }

    /// Whether this account may react in the discussion `pane` names, which the hover bar's React and the picker
    /// offer.
    pub fn may_react(&self, pane: Pane) -> bool {
        self.discussion_grants(pane).holds(Grant::AddReactions)
    }

    /// Whether this account may attach files in the discussion `pane` names, which the composer's attach button
    /// offers.
    pub fn may_attach(&self, pane: Pane) -> bool {
        self.discussion_grants(pane).holds(Grant::AttachFiles)
    }
}

/// Whether this account may react to `message` with `emoji`: with the word to, and within the most distinct emoji a
/// message holds unless the emoji is among them already.
pub(super) fn may_react(message: &chat::Message, emoji: &chat::ReactionEmoji, grants: Grants) -> bool {
    let counted = message.reactions.iter().any(|reaction| reaction.emoji == *emoji);
    grants.holds(Grant::AddReactions) && (counted || message.reactions.len() < proto::rules::MAX_REACTIONS)
}

pub(super) fn wrote(message: &chat::Message, me: Option<&MemberId>) -> bool {
    matches!((&message.author, me), (chat::Author::Member(author), Some(me)) if author == me)
}

pub(super) fn may_edit(message: &chat::Message, me: Option<&MemberId>, grants: Grants) -> bool {
    wrote(message, me) && !message.deleted() && grants.holds(Grant::SendMessage)
}

pub(super) fn may_delete(message: &chat::Message, me: Option<&MemberId>, grants: Grants) -> bool {
    !message.deleted()
        && ((wrote(message, me) && grants.holds(Grant::SendMessage)) || grants.holds(Grant::ManageMessages))
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, chatting, discussion, message_id, said, standing, tell, told};
    use crate::harness::{sends, wire};
    use crate::{Asking, DialogMessage, WindowMessage, place};
    use proto::rules::Grant;
    use std::time::Instant;

    #[test]
    fn a_member_edits_and_deletes_only_their_own_and_a_manager_deletes_any() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, told(said(2, 6, "grace", "hi all")));
        assert!(tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Edit(message_id(2)))).is_empty());
        assert!(window.places()[0].discussion().is_some_and(|open| open.editing.is_none()), "not one's own");
        assert!(
            tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Delete(message_id(2)))).is_empty()
        );
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::ManageMessages]);
        let todo = tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Delete(message_id(2))));
        assert!(todo.is_empty(), "one press only asks: {todo:?}");
        assert!(
            matches!(window.dialog().map(crate::Dialog::asking), Some(Asking::DeleteMessage(deleting))
                if deleting.line() == Some("hi all")),
            "{:?}",
            window.dialog()
        );
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)] if sends(ask, wire::DeleteMessage { message_id: "m2".into() })),
            "{todo:?}"
        );
    }
}
