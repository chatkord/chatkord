//! Editing and deleting a message.

use super::*;
use crate::place::ops;

pub(super) fn edit_started(open: &mut Discussion, id: MessageId, me: Option<&MemberId>, grants: Grants) -> Vec<Ask> {
    let editable =
        open.timeline().and_then(|timeline| timeline.get(&id)).filter(|message| may_edit(message, me, grants));
    if let Some(draft) = editable.and_then(|message| message.body.as_ref()).map(Draft::of) {
        open.editing = Some(Editing { id, draft, stage: EditStage::Typing });
    }
    Vec::new()
}

pub(super) fn edit_typed(open: &mut Discussion, text: String) -> Vec<Ask> {
    if let Some(editing) = open.editing.as_mut().filter(|editing| editing.stage == EditStage::Typing) {
        editing.draft.edit(text);
    }
    Vec::new()
}

pub(super) fn edit_saved(
    open: &mut Discussion,
    markable: &[(String, ChannelId)],
    previews_served: bool,
) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let Some(editing) = open.editing.as_ref().filter(|editing| editing.stage == EditStage::Typing) else {
        return ControlFlow::Break(Vec::new());
    };
    let Ok(body) = editing.draft.send(Carries::Nothing, |name| named(markable, name)) else {
        return ControlFlow::Break(Vec::new());
    };
    let id = editing.id.clone();
    let unchanged = open
        .timeline()
        .and_then(|timeline| timeline.get(&id))
        .is_some_and(|message| message.body.as_ref() == Some(&body));
    open.refused = None;
    if unchanged {
        open.editing = None;
        return ControlFlow::Break(Vec::new());
    }
    if let Some(editing) = open.editing.as_mut() {
        editing.stage = EditStage::Saving;
    }
    let spans = sendable_spans(&body);
    let preview =
        kept_preview(open.timeline().and_then(|timeline| timeline.get(&id)), &body, &spans).filter(|_| previews_served);
    ControlFlow::Continue(vec![Ask::EditMessage {
        message_id: id.clone(),
        body: body.text().to_owned(),
        spans,
        preview,
    }])
}

pub(super) fn edit_answered(open: &mut Discussion, changed: Result<Box<chat::Message>, LinkError>) -> Vec<Ask> {
    match changed {
        Ok(message) => {
            if open.editing.as_ref().is_some_and(|editing| editing.id == message.id) {
                open.editing = None;
            }
            open.keep_mine(std::slice::from_ref(&message));
            open.pin_told(&message);
            if let History::Ready(read) = &mut open.history
                && read.told(*message)
            {
                open.rows_changed();
            }
        }
        Err(failed) => {
            if let Some(editing) = open.editing.as_mut() {
                editing.stage = EditStage::Typing;
            }
            open.refused = Some(failed);
        }
    }
    Vec::new()
}

pub(super) fn delete_confirmed(
    open: &mut Discussion,
    id: &MessageId,
    me: Option<&MemberId>,
    grants: Grants,
) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let deletable =
        open.timeline().and_then(|timeline| timeline.get(id)).is_some_and(|message| may_delete(message, me, grants));
    if !deletable {
        return ControlFlow::Break(Vec::new());
    }
    open.refused = None;
    ControlFlow::Continue(vec![ops::delete_message(id.as_str().to_owned())])
}

#[cfg(test)]
mod tests {
    use crate::harness::{ADA, Clock, chatting, discussion, message_id, said, standing, tell, told};
    use crate::{DialogMessage, WindowMessage, place};
    use proto::rules::Grant;
    use std::time::Instant;

    #[test]
    fn a_confirmed_delete_clears_the_last_refusal_and_one_no_longer_allowed_sends_nothing() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, told(said(2, 6, "grace", "hi all")));
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::ManageMessages]);
        let failed = crate::harness::refused(proto::chatkord_v1::ErrorCode::NotAuthorized);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Changed(Err(failed))));
        assert!(window.places()[0].discussion().is_some_and(|open| open.refused.is_some()));
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Delete(message_id(2))));
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::DeleteMessage)),
            "{todo:?}"
        );
        assert!(
            window.places()[0].discussion().is_some_and(|open| open.refused.is_none()),
            "the refusal before is not this ask's"
        );

        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Delete(message_id(2))));
        assert!(window.dialog().is_some());
        standing(&mut window, &clock, owls, &[Grant::ChannelRead]);
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(todo.is_empty(), "the word went while it was asked: {todo:?}");
        assert!(window.dialog().is_none());
    }

    #[test]
    fn one_s_own_edit_is_saved_and_an_unchanged_one_asks_nothing() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, told(said(2, 6, ADA, "typo hre")));
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Edit(message_id(2))));
        assert!(tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::SaveEdit)).is_empty());
        assert!(window.places()[0].discussion().is_some_and(|open| open.editing.is_none()), "nothing changed: closed");
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Edit(message_id(2))));
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::EditDraft("typo here".into())));
        let todo = tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::SaveEdit));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::EditMessage { message_id, body, .. })] if message_id.as_str() == "m2" && body == "typo here"),
            "{todo:?}"
        );
    }
}
