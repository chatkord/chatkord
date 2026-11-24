//! Reacting to a message, and what this account has reacted with.

use super::*;
use crate::place::ops;

pub(super) fn reactions_of_yours(
    open: &mut Discussion,
    message: MessageId,
    revision: u64,
    mine: Vec<chat::ReactionEmoji>,
) -> Vec<Ask> {
    open.own_reactions(message, revision, mine);
    Vec::new()
}

pub(super) fn react(open: &mut Discussion, id: &MessageId, emoji: &chat::ReactionEmoji, grants: Grants) -> Vec<Ask> {
    let message = open.timeline().and_then(|timeline| timeline.get(id)).filter(|m| !m.deleted());
    match message {
        Some(message) if !open.reacted(message, emoji) && may_react(message, emoji, grants) => {
            vec![ops::react(id, emoji, true)]
        }
        Some(_) | None => Vec::new(),
    }
}

pub(super) fn toggle_reaction(
    open: &mut Discussion,
    id: &MessageId,
    emoji: &chat::ReactionEmoji,
    grants: Grants,
) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let message = open.timeline().and_then(|timeline| timeline.get(id)).filter(|m| !m.deleted());
    let Some(message) = message else { return ControlFlow::Break(Vec::new()) };
    let add = !open.reacted(message, emoji);
    if add && !may_react(message, emoji, grants) {
        return ControlFlow::Break(Vec::new());
    }
    let revision = message.revision;
    if add {
        open.shown_mine(id, revision, emoji.clone());
    } else {
        let mut mine = open.mine.get(id).map(|(_, mine)| mine.clone()).unwrap_or_default();
        mine.retain(|held| held != emoji);
        open.mine.insert(id.clone(), (revision, mine));
    }
    ControlFlow::Continue(vec![ops::react(id, emoji, add)])
}

pub(super) fn answered(open: &mut Discussion, answer: Result<(), LinkError>) -> Vec<Ask> {
    if let Err(failed) = answer {
        open.refused = Some(failed);
    }
    Vec::new()
}
