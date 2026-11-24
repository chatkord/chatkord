//! The emoji picker, as Discord's: opened for a message, to react with what is picked, or for the composer, to write it
//! at the cursor; what its search holds and the section its grid starts at. What it remembers on this device, the emoji
//! picked lately and the skin tone, the window holds.

use super::*;
use crate::place::ops;
use chat::{Completion, MarkKind, MessageId, Query, ReactionEmoji, Trigger};

/// What the picker picks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickFor {
    /// A message: this account reacts with what is picked.
    Message(MessageId),
    /// The composer: what is picked is written at its cursor.
    Composer,
}

/// A section of the picker's grid, and the tab that starts the grid at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerSection {
    /// The emoji this device picked lately.
    Recent,
    /// The server's own emoji.
    Server,
    /// One of Unicode's groups.
    Group(chat::emoji::Group),
}

/// The picker open over a discussion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picking {
    /// What it picks for.
    pub target: PickFor,
    /// What its search holds.
    pub query: String,
    /// The section its grid starts at.
    pub section: PickerSection,
}

/// What is done in the emoji picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerMessage {
    /// It opens for this, from a message's hover bar, its menu or its chips' add button, or the composer's smile;
    /// opened for it already, it closes.
    Open(PickFor),
    /// Its search's text changed.
    Query(String),
    /// A tab pressed: the grid starts at its section, the search cleared.
    Section(PickerSection),
    /// An emoji picked: in the tone shown for a Unicode one.
    Pick(ReactionEmoji),
    /// Escape, or a press outside it.
    Close,
}

/// The longest name or shortcode an emoji has: a search longer finds nothing, so it is refused.
fn query_most() -> usize {
    chat::emoji::all()
        .flat_map(|emoji| std::iter::once(emoji.name()).chain(emoji.shortcodes()))
        .map(|name| name.chars().count())
        .chain([proto::rules::MAX_EMOJI_NAME])
        .max()
        .unwrap_or(proto::rules::MAX_EMOJI_NAME)
}

impl Composing {
    /// `completion` written at the cursor, the way the `:` chooser writes its choice, the cursor after it; false
    /// where the draft would not take it, which leaves it as it was.
    fn write(&mut self, completion: Completion) -> bool {
        let before = self.draft.clone();
        let count = self.draft.count();
        let at = self.caret.unwrap_or(count).min(count);
        let mut text: String = self.draft.text().chars().take(at).collect();
        text.push(Trigger::Emoji.symbol());
        text.extend(self.draft.text().chars().skip(at));
        self.draft.edit(text);
        let query = Query { trigger: Trigger::Emoji, text: String::new(), range: at..at + 1 };
        let Some(caret) = self.draft.complete(&query, completion) else {
            self.draft = before;
            return false;
        };
        self.caret = Some(caret);
        self.caret_moved += 1;
        self.selected = 0;
        self.nonce = None;
        true
    }
}

impl Place {
    /// What is done in the picker over the discussion `pane` names, with the words held in its channel; a pick
    /// answers what it did, and the emoji where something was done with it.
    pub(super) fn on_picker(
        &mut self,
        pane: Pane,
        message: PickerMessage,
        grants: Grants,
    ) -> (Vec<Ask>, Option<ReactionEmoji>) {
        if let PickerMessage::Pick(emoji) = message {
            return match self.pick_emoji(pane, &emoji) {
                Some(todo) => (todo, Some(emoji)),
                None => (Vec::new(), None),
            };
        }
        let Some(open) = self.pane_mut(pane) else { return (Vec::new(), None) };
        match message {
            PickerMessage::Open(target) => {
                if open.picking.as_ref().is_some_and(|picking| picking.target == target) {
                    open.picking = None;
                    return (Vec::new(), None);
                }
                let allowed = match &target {
                    PickFor::Message(id) => {
                        grants.holds(Grant::AddReactions)
                            && open.timeline().and_then(|timeline| timeline.get(id)).is_some_and(|held| !held.deleted())
                    }
                    PickFor::Composer => grants.holds(Grant::SendMessage),
                };
                open.menu = None;
                open.picking =
                    allowed.then_some(Picking { target, query: String::new(), section: PickerSection::Recent });
            }
            PickerMessage::Query(query) => {
                if let Some(picking) = open.picking.as_mut()
                    && query.chars().count() <= query_most()
                {
                    picking.query = query;
                }
            }
            PickerMessage::Section(section) => {
                if let Some(picking) = open.picking.as_mut() {
                    picking.query.clear();
                    picking.section = section;
                }
            }
            PickerMessage::Close | PickerMessage::Pick(_) => open.picking = None,
        }
        (Vec::new(), None)
    }

    /// `emoji` picked in the picker over the discussion `pane` names: a reaction to its message, idempotent, or
    /// written in the composer, a server's own as `:name:` with its mark. None where nothing was done with it.
    fn pick_emoji(&mut self, pane: Pane, emoji: &ReactionEmoji) -> Option<Vec<Ask>> {
        let open = self.pane_mut(pane)?;
        let picking = open.picking.take()?;
        let (owner, compose) = (open.owner.clone(), open.compose.clone());
        let grants = self.grants_of(&owner);
        match picking.target {
            PickFor::Message(id) => {
                let open = self.pane_mut(pane)?;
                let message = open.timeline()?.get(&id).filter(|held| !held.deleted())?;
                if !super::may_react(message, emoji, grants) {
                    return None;
                }
                if open.reacted(message, emoji) {
                    return Some(Vec::new());
                }
                let revision = message.revision;
                open.shown_mine(&id, revision, emoji.clone());
                Some(vec![ops::react(&id, emoji, true)])
            }
            PickFor::Composer => {
                if !grants.holds(Grant::SendMessage) {
                    return None;
                }
                let completion = match emoji {
                    ReactionEmoji::Unicode(glyph) => Completion { text: glyph.clone(), mark: None },
                    ReactionEmoji::Custom(id) => {
                        let own = self.emoji.get(id)?;
                        Completion { text: own.written(), mark: Some(MarkKind::CustomEmoji(id.clone())) }
                    }
                };
                let composing = &mut self.held.entry(compose).or_default().composing;
                composing.write(completion).then(Vec::new)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, chatting, discussion, general_compose, message_id, standing, tell};
    use crate::place::{self, DiscussionMessage, PickFor, PickerMessage};
    use crate::{Ask, Keep};
    use chat::ReactionEmoji;
    use proto::rules::Grant;
    use std::time::Instant;

    fn picker(message: PickerMessage) -> place::Message {
        discussion(DiscussionMessage::Picker(message))
    }

    #[test]
    fn an_emoji_picked_for_a_message_reacts_with_it_and_is_kept_as_picked_lately() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::SendMessage, Grant::AddReactions]);
        tell(&mut window, &clock, owls, picker(PickerMessage::Open(PickFor::Message(message_id(1)))));
        let dice = ReactionEmoji::Unicode("🎲".to_owned());
        let todo = tell(&mut window, &clock, owls, picker(PickerMessage::Pick(dice.clone())));
        assert!(
            todo.iter()
                .any(|done| matches!(done, Ask::At(_, ask) if matches!(ask.op(), Some(crate::harness::Body::React(react)) if react.emoji == Some(dice.wire())))),
            "{todo:?}"
        );
        assert!(
            todo.iter().any(|done| matches!(done, Ask::Keep(Keep::Emoji(picks)) if picks.recent() == [dice.clone()])),
            "{todo:?}"
        );
        assert!(window.places()[0].discussion().is_some_and(|open| open.picking.is_none()), "a pick closes it");
        let todo = tell(&mut window, &clock, owls, picker(PickerMessage::Pick(dice)));
        assert!(todo.is_empty(), "nothing open, nothing picked: {todo:?}");
    }

    #[test]
    fn without_the_word_to_react_the_picker_does_not_open_for_a_message() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, picker(PickerMessage::Open(PickFor::Message(message_id(1)))));
        assert!(window.places()[0].discussion().is_some_and(|open| open.picking.is_none()));
    }

    #[test]
    fn an_emoji_picked_for_the_composer_is_written_at_its_cursor() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::Draft("hi there".to_owned())));
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::Caret(2)));
        tell(&mut window, &clock, owls, picker(PickerMessage::Open(PickFor::Composer)));
        let todo =
            tell(&mut window, &clock, owls, picker(PickerMessage::Pick(ReactionEmoji::Unicode("🎲".to_owned()))));
        assert!(todo.iter().any(|done| matches!(done, Ask::Keep(Keep::Emoji(_)))), "{todo:?}");
        let composing = window.places()[0].composing(&general_compose()).cloned().unwrap_or_default();
        assert_eq!(composing.draft.text(), "hi🎲 there");
        assert_eq!(composing.caret(), Some(3), "after it");
    }
}
