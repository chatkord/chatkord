//! The pages, the news and the changes that make a discussion's timeline.

use super::*;
use crate::place::ops;

pub(super) fn page_read(open: &mut Discussion, end: Option<End>, page: Result<chat::Page, LinkError>) -> Vec<Ask> {
    match (end, page) {
        (None, Ok(page)) => {
            let early = match &mut open.history {
                History::Loading { early } => std::mem::take(early),
                History::Ready(_) | History::Failed(_) => Vec::new(),
            };
            open.keep_mine(&page.messages);
            let mut timeline = Timeline::from_page(page);
            for told in early {
                match told {
                    Early::Told(message) => timeline.told(*message),
                    Early::Withdrawn(withdrawn) => timeline.withdrawn(&withdrawn),
                };
            }
            open.history = History::Ready(timeline);
            open.reading = None;
            open.rows_changed();
        }
        (None, Err(failed)) => {
            open.history = History::Failed(failed);
            open.reading = None;
        }
        (Some(end), page) if open.reading == Some(end) => {
            open.reading = None;
            match (page, &mut open.history) {
                (Ok(page), History::Ready(_)) => {
                    open.keep_mine(&page.messages);
                    if let History::Ready(timeline) = &mut open.history {
                        timeline.extend(end, page);
                    }
                    open.rows_changed();
                }
                (Err(failed), _) => open.refused = Some(failed),
                (Ok(_), History::Loading { .. } | History::Failed(_)) => {}
            }
        }
        (Some(_), _) => {}
    }
    open.read_more().into_iter().collect()
}

pub(super) fn message_told(open: &mut Discussion, message: Box<chat::Message>) -> Vec<Ask> {
    open.pin_told(&message);
    if message.deleted()
        && open.picking.as_ref().is_some_and(|picking| picking.target == PickFor::Message(message.id.clone()))
    {
        open.picking = None;
    }
    match &mut open.history {
        History::Loading { early } => early.push(Early::Told(message)),
        History::Ready(read) => {
            if open.editing.as_ref().is_some_and(|editing| editing.id == message.id) && message.deleted() {
                open.editing = None;
            }
            if read.told(*message) {
                open.rows_changed();
            }
        }
        History::Failed(_) => {}
    }
    Vec::new()
}

pub(super) fn message_withdrawn(open: &mut Discussion, withdrawn: chat::Withdrawn) -> Vec<Ask> {
    match &mut open.history {
        History::Loading { early } => early.push(Early::Withdrawn(withdrawn)),
        History::Ready(read) => {
            if read.withdrawn(&withdrawn) {
                open.rows_changed();
            }
        }
        History::Failed(_) => {}
    }
    Vec::new()
}

pub(super) fn caught_up(
    open: &mut Discussion,
    timeline: TimelineId,
    changes: Result<Caught, LinkError>,
) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    match changes {
        Ok(caught) if caught.reload => {
            open.history = History::Loading { early: Vec::new() };
            open.reading = None;
            open.scroll_to_newest();
            return ControlFlow::Break(vec![ops::history(timeline, terms::Anchor::Newest, None)]);
        }
        Ok(caught) => {
            open.keep_mine(&caught.changed);
            if let History::Ready(read) = &mut open.history
                && read.caught_up(caught.changed, caught.revision)
            {
                open.rows_changed();
            }
        }
        Err(failed) => open.refused = Some(failed),
    }
    ControlFlow::Continue(Vec::new())
}

pub(super) fn reload(open: &mut Discussion) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    open.history = History::Loading { early: Vec::new() };
    open.reading = None;
    open.scroll_to_newest();
    let Some(timeline) = open.timeline_id().cloned() else { return ControlFlow::Break(Vec::new()) };
    ControlFlow::Continue(vec![ops::history(timeline, terms::Anchor::Newest, None)])
}

pub(super) fn jump_to(open: &mut Discussion, place: u64) -> Vec<Ask> {
    open.pins = None;
    open.scroll = Scroll::At { row: RowKey::Message(place), offset: 0.0 };
    if open.timeline().is_some_and(|timeline| timeline.at(place).is_some()) {
        open.follow_scroll();
        Vec::new()
    } else {
        open.history = History::Loading { early: Vec::new() };
        open.reading = None;
        open.first_page().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::discussion::tests::timeline;
    use crate::harness::{
        ADA, Clock, accepted, chatting, discussion, first_page, flow, general_timeline, said, tell, told,
    };
    use crate::harness::{sends, wire};
    use crate::place;
    use crate::window::Window;
    use proto::rules::Grant;
    use std::time::Instant;
    use terms::Ended;

    #[test]
    fn the_open_channel_survives_a_reconnect_and_asks_what_changed_since_it_read() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, told(said(2, 9, "grace", "still here?")));
        let ended = Ended { silent: true, ..Ended::default() };
        tell(&mut window, &clock, owls, place::Message::Session(place::SessionMessage::Disconnected(ended)));
        assert!(window.places()[0].discussion().is_some(), "shown as it was while the connection is made again");
        tell(&mut window, &clock, owls, place::Message::Session(place::SessionMessage::Reconnect(1)));
        let todo = tell(&mut window, &clock, owls, flow(accepted([Grant::ChannelRead].into_iter().collect())));
        assert!(
            todo.iter().any(
                |done| matches!(done, crate::Ask::At(_, ask) if sends(ask, wire::CatchUp { timeline_id: general_timeline().as_str().into(), since: 9 }))
            ),
            "{todo:?}"
        );
        let caught =
            place::Caught { changed: vec![said(2, 12, "grace", "edited while away")], reload: false, revision: 12 };
        let changes = Ok(caught);
        tell(
            &mut window,
            &clock,
            owls,
            discussion(place::DiscussionMessage::CaughtUp { timeline: general_timeline(), changes }),
        );
        assert_eq!(timeline(&window).revision(), 12);
        assert_eq!(
            timeline(&window).last().and_then(|message| message.body.as_ref()).map(chat::Body::text),
            Some("edited while away")
        );
        let changes = Ok(place::Caught { changed: Vec::new(), reload: true, revision: 900 });
        let todo = tell(
            &mut window,
            &clock,
            owls,
            discussion(place::DiscussionMessage::CaughtUp { timeline: general_timeline(), changes }),
        );
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, ask)] if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.anchor.is_none())
            ),
            "{todo:?}"
        );
    }

    #[test]
    fn a_message_told_while_the_first_page_loads_is_kept_over_it() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::JumpToPresent));
        tell(&mut window, &clock, owls, told(said(1, 8, ADA, "hello, edited")));
        let page = chat::Page { messages: vec![said(1, 5, ADA, "hello")], older: false, newer: false, revision: 5 };
        tell(&mut window, &clock, owls, first_page(page));
        assert_eq!(
            timeline(&window).first().and_then(|message| message.body.as_ref()).map(chat::Body::text),
            Some("hello, edited")
        );
    }

    #[test]
    fn a_leaver_s_messages_withdrawn_while_the_first_page_loads_and_after_are_deleted() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::JumpToPresent));
        let withdrawn = |through, revision| {
            let told = proto::chatkord_v1::MessagesWithdrawn {
                timeline_id: general_timeline().as_str().into(),
                user_id: "bo".into(),
                former_author: "f1".into(),
                through,
                revision,
                deleted_at: 1_763_035_900,
            };
            let kind = Some(proto::chatkord_v1::event::Kind::MessagesWithdrawn(told));
            let event = proto::chatkord_v1::Event { cursor: Vec::new(), kind };
            place::Message::heard(&event).expect("read at the edge")
        };
        tell(&mut window, &clock, owls, withdrawn(2, 6));
        let messages = vec![said(1, 1, "bo", "one"), said(2, 2, ADA, "two"), said(3, 3, "bo", "three")];
        tell(&mut window, &clock, owls, first_page(chat::Page { messages, older: false, newer: false, revision: 5 }));
        let bodies = |window: &Window| -> Vec<Option<String>> {
            timeline(window)
                .messages()
                .map(|message| message.body.as_ref().map(|body| body.text().to_owned()))
                .collect()
        };
        assert_eq!(
            bodies(&window),
            [None, Some("two".into()), Some("three".into())],
            "told before the page, kept over it"
        );
        tell(&mut window, &clock, owls, withdrawn(3, 7));
        assert_eq!(bodies(&window), [None, Some("two".into()), None], "and after it");
        assert_eq!(timeline(&window).revision(), 7);
    }
}
