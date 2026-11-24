//! Sending what the composer holds, and the answer to a send.

use super::*;

impl Place {
    /// What the server takes in files, as it said in its greeting.
    pub fn file_limits(&self) -> terms::FileLimits {
        self.limits
    }

    /// A send was answered: stored, it joins its channel's timeline wherever that is open (the first reply of a
    /// thread joins the thread's, which the server tells); refused or lost, its draft goes back to its composer,
    /// whatever is open and whether the connection still stands.
    pub(super) fn sent(
        &mut self,
        timeline: &TimelineId,
        thread_of: Option<&MessageId>,
        nonce: terms::Nonce,
        sent: Result<chat::Message, LinkError>,
    ) {
        let asked = thread_of.cloned().map_or_else(|| Compose::Timeline(timeline.clone()), Compose::NewThread);
        let Some(held) = self.held.get_mut(&asked) else { return };
        let Some(at) = held.unsent.iter().position(|unsent| unsent.nonce == nonce) else { return };
        let unsent = held.unsent.remove(at);
        let opens: Vec<&mut Discussion> = match &mut self.status {
            Status::Signed(signed) => signed.open_mut().collect(),
            Status::Reconnecting(again) => again.discussion.iter_mut().collect(),
            Status::Door(_) => Vec::new(),
        };
        // A thread begun meanwhile holds its composer under its own timeline now.
        let begun = |open: &Discussion| matches!((&open.owner, thread_of), (Owner::Thread { root, .. }, Some(asked)) if root == asked);
        let compose = opens.iter().find(|open| begun(open)).map_or_else(|| asked.clone(), |open| open.compose.clone());
        let opens = opens.into_iter().filter(|open| open.compose == compose);
        if compose != asked && self.held.get(&asked).is_some_and(|held| held.unsent.is_empty()) {
            self.held.remove(&asked);
        }
        let held = self.held.entry(compose.clone()).or_default();
        match sent {
            Ok(message) => {
                // The first reply of a thread is in the thread's timeline, which the server tells.
                for open in opens.filter(|_| thread_of.is_none()) {
                    open.keep_mine(std::slice::from_ref(&message));
                    if let History::Ready(read) = &mut open.history {
                        read.told(message.clone());
                    }
                    open.scroll_to_newest();
                    open.rows_changed();
                }
            }
            Err(failed) => {
                // The draft comes back to an empty composer with its nonce, so sending it again is the same send.
                let composing = &mut held.composing;
                if composing.draft.text().trim().is_empty() {
                    composing.draft = unsent.draft;
                    composing.nonce = Some(unsent.nonce);
                    composing.preview =
                        unsent.preview.map(|(link, made)| Previewing { link, stage: PreviewStage::Ready(made) });
                }
                for open in opens {
                    open.refused = Some(failed.clone());
                }
            }
        }
    }

    /// The channels a `#name` typed in a discussion may become a mark of on send: each the account sees, by name.
    pub(super) fn markable_channels(&self) -> Vec<(String, ChannelId)> {
        let Some(Lobby::Ready(listed)) = self.lobby() else { return Vec::new() };
        let text = listed.text.iter().map(|channel| (channel.name.clone(), channel.id.clone()));
        text.chain(listed.voice.iter().map(|channel| (channel.name.clone(), channel.id.clone()))).collect()
    }

    /// Where a send from `compose` goes: its timeline, else for a thread yet to begin its channel's, with the root.
    fn destination(&self, compose: &Compose) -> Option<(TimelineId, Option<MessageId>)> {
        match compose {
            Compose::Timeline(timeline) => Some((timeline.clone(), None)),
            Compose::NewThread(root) => {
                let Some(Lobby::Ready(listed)) = self.lobby() else { return None };
                let Some(Owner::Thread { channel, .. }) = self.owner_of_compose(compose) else { return None };
                Some((timeline_of(listed, &channel)?.timeline, Some(root.clone())))
            }
        }
    }

    /// Sends what `compose`'s composer holds: its text and marks, its files once all are sent, and the message it
    /// answers. A link mark the server would refuse is left as plain text, so it never holds the message back.
    pub(super) fn send(&mut self, pane: Pane, compose: &Compose, grants: Grants, at: Moment) -> Vec<Ask> {
        if !grants.holds(Grant::SendMessage) {
            return Vec::new();
        }
        let Some((timeline, thread_of)) = self.destination(compose) else { return Vec::new() };
        let markable = self.markable_channels();
        let Some(held) = self.held.get_mut(compose) else { return Vec::new() };
        let composing = &mut held.composing;
        if !composing.files_ready() {
            return Vec::new();
        }
        let files = if grants.holds(Grant::AttachFiles) { composing.ready_files() } else { Vec::new() };
        let carries = if files.is_empty() { Carries::Nothing } else { Carries::Files };
        let Ok(body) = composing.draft.send(carries, |name| named(&markable, name)) else { return Vec::new() };
        let spans = sendable_spans(&body);
        let preview = composing.preview.take().and_then(|taken| match taken.stage {
            PreviewStage::Ready(made) if made.fits(body.text(), &spans) => Some((taken.link, made)),
            PreviewStage::Waiting(_) | PreviewStage::Fetching | PreviewStage::Ready(_) | PreviewStage::Failed => None,
        });
        composing.declined.clear();
        let reply = composing.reply.take().map(|reply| (reply.to, reply.mention_author));
        let nonce = composing.nonce.take().unwrap_or(at.nonce);
        let draft = std::mem::take(&mut composing.draft);
        composing.files.retain(|file| matches!(file.state, AttachState::Refused(_)));
        composing.notice = None;
        composing.caret = None;
        held.typed_at = None;
        let made = preview.as_ref().map(|(_, made)| made.clone());
        held.unsent.push(Unsent { nonce, body: body.text().to_owned(), draft, preview });
        if let Some(open) = self.pane_mut(pane) {
            open.refused = None;
            open.unread_after = None;
        }
        vec![Ask::SendMessage {
            timeline,
            thread_of,
            body: body.text().to_owned(),
            spans,
            files,
            reply,
            nonce,
            preview: made,
        }]
    }
}

/// The preview an edited message goes on with: the one it holds, while its link is still one of the links the edit
/// sends as links; its picture is sent again, as an edit releases the file the card held.
pub(super) fn kept_preview(
    message: Option<&chat::Message>,
    body: &chat::Body,
    spans: &[proto::chatkord_v1::Span],
) -> Option<PreviewMade> {
    let card = message?.preview.as_ref()?;
    let made = PreviewMade {
        url: card.url.clone(),
        title: card.title.clone(),
        description: card.description.clone(),
        picture: card.picture.map(|picture| PreviewPicture::Sent(picture.file)),
    };
    made.fits(body.text(), spans).then_some(made)
}

/// Every channel of `markable` called exactly `name`.
pub(super) fn named(markable: &[(String, ChannelId)], name: &str) -> Vec<ChannelId> {
    markable.iter().filter(|(called, _)| called == name).map(|(_, id)| id.clone()).collect()
}

/// `body`'s marks as the spans a send carries: a link the server's rule refuses is left as text, and past the most a
/// body holds the links go first, so a message is never refused for a link.
pub(super) fn sendable_spans(body: &chat::Body) -> Vec<proto::chatkord_v1::Span> {
    use proto::chatkord_v1::span::Kind;
    let text = body.text();
    let spans = chat::spans_of(body);
    let named = spans.iter().filter(|span| !matches!(span.kind, Some(Kind::Link(_)))).count();
    let mut room = proto::rules::MAX_SPANS.saturating_sub(named);
    spans
        .into_iter()
        .filter(|span| match span.kind {
            Some(Kind::Link(_)) => {
                let taken = room > 0 && proto::rules::spans_shape(text, std::slice::from_ref(span)).is_ok();
                room -= usize::from(taken);
                taken
            }
            _ => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discussion::tests::timeline;
    use crate::harness::{
        ADA, Clock, GENERAL, channels_holding, chatting, discussion, general_compose, general_timeline, in_call, said,
        standing, tell,
    };
    use crate::place::PlaceId;
    use crate::window::Window;
    use crate::{WindowMessage, place};
    use proto::rules::Grant;
    use std::time::Instant;
    use terms::{Ended, LinkError};

    #[test]
    fn a_send_tried_again_after_a_lost_reply_is_the_same_send() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Draft("  see you at nine \n".into())));
        let todo = tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Send));
        let [crate::Ask::At(_, place::Ask::SendMessage { body, nonce, .. })] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(body, "see you at nine", "the white space at either end is left off");
        let first = *nonce;
        let lost = place::DiscussionMessage::Sent {
            timeline: general_timeline(),
            thread_of: None,
            nonce: first,
            sent: Err(LinkError::Timeout),
        };
        tell(&mut window, &clock, owls, discussion(lost));
        let place = &window.places()[0];
        assert_eq!(
            place.composing(&general_compose()).map(|composing| composing.draft.text()),
            Some("  see you at nine \n"),
            "the draft kept as typed, its spans where they were"
        );
        assert!(
            place.discussion().is_some_and(|open| open.refused.is_some())
                && place.unsent(&general_compose()).is_empty()
        );
        let later = Moment { nonce: terms::Nonce::new([9; 16]), ..clock.at() };
        let todo = window.update(WindowMessage::At(owls, discussion(place::DiscussionMessage::Send)), later);
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::SendMessage { nonce, .. })] if *nonce == first),
            "{todo:?}"
        );
        let sent = Ok(said(2, 6, ADA, "see you at nine"));
        tell(
            &mut window,
            &clock,
            owls,
            discussion(place::DiscussionMessage::Sent {
                timeline: general_timeline(),
                thread_of: None,
                nonce: first,
                sent: sent.map(Box::new),
            }),
        );
        assert_eq!(timeline(&window).len(), 2);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Draft("another".into())));
        let todo = window.update(WindowMessage::At(owls, discussion(place::DiscussionMessage::Send)), later);
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::SendMessage { nonce, .. })] if *nonce != first),
            "a new message is a new send: {todo:?}"
        );
    }

    const RANDOM: &str = "random";

    /// A send from #general still unanswered, with #random listed beside it, and the send's nonce.
    fn sending(clock: &Clock) -> (Window, PlaceId, terms::Nonce) {
        let (mut window, owls) = chatting(clock);
        let writer = [Grant::ChannelRead, Grant::SendMessage].into_iter().collect();
        let mut listed = channels_holding(writer);
        let timeline = terms::TimelineId::parse("t-random").unwrap();
        listed.text.push(place::TextChannel {
            id: crate::harness::channel_id(RANDOM),
            name: "random".into(),
            timeline,
            revision: 0,
        });
        listed.standings.insert(
            crate::harness::channel_id(RANDOM),
            place::ChannelStanding { grants: writer, ..place::ChannelStanding::default() },
        );
        tell(&mut window, clock, owls, place::Message::Lobby(place::LobbyMessage::Channels(Ok(listed))));
        tell(&mut window, clock, owls, discussion(place::DiscussionMessage::Draft("see you at nine".into())));
        let todo = tell(&mut window, clock, owls, discussion(place::DiscussionMessage::Send));
        let [crate::Ask::At(_, place::Ask::SendMessage { nonce, .. })] = &todo[..] else { panic!("{todo:?}") };
        let nonce = *nonce;
        assert_eq!(window.places()[0].unsent(&general_compose()).len(), 1);
        (window, owls, nonce)
    }

    /// The send from [`sending`] fails; its text and nonce are #general's composer's again.
    fn lost_and_kept(window: &mut Window, clock: &Clock, owls: PlaceId, nonce: terms::Nonce, when: &str) {
        let lost = place::DiscussionMessage::Sent {
            timeline: general_timeline(),
            thread_of: None,
            nonce,
            sent: Err(LinkError::Timeout),
        };
        tell(window, clock, owls, discussion(lost));
        let place = &window.places()[0];
        let kept = place.composing(&general_compose()).map(|composing| (composing.draft.text(), composing.nonce));
        assert_eq!(kept, Some(("see you at nine", Some(nonce))), "{when}");
        assert!(place.unsent(&general_compose()).is_empty(), "{when}");
    }

    #[test]
    fn a_send_that_fails_after_its_channel_closed_goes_back_to_that_channel_s_composer() {
        let clock = Clock(Instant::now());

        let (mut window, owls, nonce) = sending(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Open(crate::harness::channel_id(RANDOM))));
        assert!(
            window.places()[0]
                .discussion()
                .is_some_and(|open| matches!(&open.owner, Owner::Channel(id) if id.as_str() == RANDOM))
        );
        lost_and_kept(&mut window, &clock, owls, nonce, "another channel open");
        let place = &window.places()[0];
        assert!(
            place
                .composing(&Compose::Timeline(terms::TimelineId::parse("t-random").unwrap()))
                .is_none_or(|other| other.draft.text().is_empty()),
            "not the open one's"
        );
        assert!(place.discussion().is_some_and(|open| open.refused.is_none()), "nor its refusal");
        tell(
            &mut window,
            &clock,
            owls,
            discussion(place::DiscussionMessage::Open(crate::harness::channel_id(GENERAL))),
        );
        let later = Moment { nonce: terms::Nonce::new([9; 16]), ..clock.at() };
        let todo = window.update(WindowMessage::At(owls, discussion(place::DiscussionMessage::Send)), later);
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::SendMessage { nonce: again, .. })] if *again == nonce),
            "sent again from its channel, it is the same send: {todo:?}"
        );

        let (mut window, owls, nonce) = sending(&clock);
        in_call(&mut window, &clock, owls);
        assert!(window.places()[0].discussion().is_none());
        lost_and_kept(&mut window, &clock, owls, nonce, "in a call, no channel open");

        let (mut window, owls, nonce) = sending(&clock);
        let ended = Ended { silent: true, ..Ended::default() };
        tell(&mut window, &clock, owls, place::Message::Session(place::SessionMessage::Disconnected(ended)));
        lost_and_kept(&mut window, &clock, owls, nonce, "the connection being made again");
        assert!(window.places()[0].discussion().is_some_and(|open| open.refused.is_some()), "said in its channel");
    }

    #[test]
    fn nothing_is_sent_without_the_word_to_send_or_past_the_bound() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let over = "x".repeat(proto::rules::MAX_MESSAGE_CHARS + 1);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Draft(over)));
        assert!(tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Send)).is_empty());
        standing(&mut window, &clock, owls, &[Grant::ChannelRead]);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Draft("hi".into())));
        assert!(!window.places()[0].may_send(crate::place::Pane::Content));
        assert!(tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::Send)).is_empty());
    }
}
