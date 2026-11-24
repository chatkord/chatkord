//! Where a discussion's list is scrolled, which rows it builds and when it reads another page.

use super::*;
use crate::place::ops;

/// Pixels of rows built above and below the viewport, so a scroll shows them before they are measured.
pub const OVERSCAN: f32 = 600.0;

/// A row's height until one is measured: a one-line message.
pub(super) const FIRST_GUESS: f32 = 44.0;

/// The viewport's height until the list says it: tall, so the first screen is whole on any monitor.
pub(super) const FIRST_VIEWPORT: f32 = 2400.0;

/// A page is read once the viewport comes this close to the end of what is held.
const LOAD_MARGIN: f32 = 800.0;

impl Discussion {
    /// The first page to read: around the row it was left at, else the newest; none for a thread yet to begin.
    pub(crate) fn first_page(&self) -> Option<Ask> {
        let anchor = match &self.scroll {
            Scroll::At { row: RowKey::Message(place), .. } => terms::Anchor::Around(*place),
            Scroll::At { row: RowKey::Day(_), .. } | Scroll::Newest => terms::Anchor::Newest,
        };
        Some(ops::history(self.timeline_id()?.clone(), anchor, None))
    }

    /// The timeline changed: its rows are known again, heights of rows let go are forgotten, and the row in view
    /// stays where it was, the list's offset following it.
    pub(super) fn rows_changed(&mut self) {
        self.keys = self.rows().iter().map(Row::key).collect();
        self.heights.retain(&self.keys);
        self.follow_scroll();
    }

    /// The offset that keeps the row in view where it was; a row no longer held leaves the offset as it is.
    pub(super) fn follow_scroll(&mut self) {
        match self.heights.distance(&self.keys, &self.scroll) {
            Some(from_end) => self.move_to(from_end),
            None => self.scroll = self.heights.scroll(&self.keys, self.from_end),
        }
    }

    /// Back to the newest end.
    pub(super) fn scroll_to_newest(&mut self) {
        self.scroll = Scroll::Newest;
        self.move_to(0.0);
    }

    /// The list moved by the window, not by the person: told to the front end by [`Discussion::moved`].
    fn move_to(&mut self, from_end: f32) {
        if (from_end - self.from_end).abs() >= 0.5 {
            self.moved += 1;
        }
        self.from_end = from_end;
    }

    /// Its messages drawn at another size: the heights of rows not built are forgotten, guessed from the built ones
    /// the list measures again, and the row in view stays where it was.
    pub(super) fn rescaled(&mut self) {
        let built = self.window();
        let kept = self.keys.get(built.first..built.end).unwrap_or_default().to_vec();
        self.heights.retain(&kept);
        self.follow_scroll();
    }

    /// The list's rows, oldest first. A thread's replies follow its root, so the day of the first is said only when
    /// it is not the root's.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = self.timeline().map(|timeline| chat::rows(timeline, &self.zone)).unwrap_or_default();
        let root_day = self.root.as_ref().and_then(Root::sent_at).and_then(|at| chat::local(at, &self.zone));
        if let (Some(root), Some(Row::Day(first))) = (root_day, rows.first())
            && root.date() == *first
        {
            rows.remove(0);
        }
        rows
    }

    /// Which rows to build for where the list is scrolled.
    pub fn window(&self) -> chat::Window {
        self.heights.window_keeping(&self.keys, self.from_end, self.viewport, OVERSCAN, &self.selected)
    }

    /// The room above the rows when they are shorter than the viewport, so the list stands on its newest end; none
    /// in a thread, whose replies stand under its root.
    pub fn room_above(&self) -> f32 {
        match self.owner {
            Owner::Thread { .. } => 0.0,
            Owner::Channel(_) | Owner::Conversation(_) => {
                let head = if self.shows_start() || self.reading_older() { self.head } else { 0.0 };
                (self.viewport - self.heights.total(&self.keys) - head).max(0.0)
            }
        }
    }

    /// How far the viewport's bottom is above the newest message held: where the list is to be, which the front end
    /// sets when rows below the one in view changed.
    pub fn scrolled_up(&self) -> f32 {
        self.from_end
    }

    /// Where the list is scrolled, by the row in view.
    pub fn scroll(&self) -> &Scroll {
        &self.scroll
    }

    /// How many times the window moved the list itself; the front end puts [`Discussion::scrolled_up`] on the list
    /// each time this changes, and never while the person scrolls it.
    pub fn moved(&self) -> u64 {
        self.moved
    }

    /// Older messages are being read.
    pub fn reading_older(&self) -> bool {
        self.reading == Some(End::Older)
    }

    /// The newest messages are not held: the list offers to jump to them.
    pub fn behind_present(&self) -> bool {
        self.timeline().is_some_and(Timeline::newer)
    }

    /// The start of the channel is held: nothing older exists.
    pub fn at_start(&self) -> bool {
        self.timeline().is_some_and(|timeline| !timeline.older())
    }

    /// The channel's start stands above its rows: the oldest message is held and built.
    pub fn shows_start(&self) -> bool {
        self.at_start() && self.window().first == 0
    }

    /// A page asked for at `end` of the timeline, if none is out and that end has more.
    pub(super) fn read_more(&mut self) -> Option<Ask> {
        let timeline = self.timeline()?;
        if self.reading.is_some() {
            return None;
        }
        let above = self.heights.total(&self.keys) - self.from_end - self.viewport;
        let (end, anchor) = if timeline.older() && above < LOAD_MARGIN {
            (End::Older, terms::Anchor::Before(timeline.first()?.place))
        } else if timeline.newer() && self.from_end < LOAD_MARGIN {
            (End::Newer, terms::Anchor::After(timeline.last()?.place))
        } else {
            return None;
        };
        let timeline = self.timeline_id()?.clone();
        self.reading = Some(end);
        Some(ops::history(timeline, anchor, Some(end)))
    }
}

pub(super) fn scrolled(open: &mut Discussion, from_end: f32, viewport: f32) -> Vec<Ask> {
    open.viewport = viewport.max(1.0);
    let from_end = from_end.max(0.0);
    if (from_end - open.reported).abs() >= 0.5 {
        open.reported = from_end;
        open.from_end = from_end;
        open.scroll = open.heights.scroll(&open.keys, from_end);
    }
    open.read_more().into_iter().collect()
}

pub(super) fn row_measured(open: &mut Discussion, row: RowKey, height: f32) -> Vec<Ask> {
    if open.heights.measured(row, height) {
        open.follow_scroll();
    }
    open.read_more().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use crate::harness::wire;
    use crate::harness::{ADA, Clock, GENERAL, channels, chatting, discussion, first_page, said, tell, told};
    use crate::place;
    use crate::place::PlaceId;
    use crate::window::Window;
    use std::time::Instant;

    /// A channel shorter than its list stands on its newest end, its start above the rows included, so nothing scrolls.
    #[test]
    fn a_short_channel_s_room_leaves_out_its_start_so_it_does_not_scroll() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::JumpToPresent));
        let page = chat::Page { messages: Vec::new(), older: false, newer: false, revision: 1 };
        tell(&mut window, &clock, owls, first_page(page));
        let short = place::DiscussionMessage::Scrolled { from_end: 0.0, viewport: 300.0 };
        tell(&mut window, &clock, owls, discussion(short));
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::HeadMeasured { height: 120.0 }));
        let open = window.places()[0].discussion().unwrap();
        assert!(open.shows_start());
        assert_eq!(open.room_above(), 180.0, "the start and the room fill the list exactly");
    }

    #[test]
    fn scrolling_near_the_oldest_held_reads_the_page_before_it_once() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        tell(&mut window, &clock, owls, discussion(place::DiscussionMessage::JumpToPresent));
        let short = place::DiscussionMessage::Scrolled { from_end: 0.0, viewport: 300.0 };
        tell(&mut window, &clock, owls, discussion(short));
        let messages = (10..60).map(|n| said(n, n, ADA, "line")).collect();
        let todo = tell(
            &mut window,
            &clock,
            owls,
            first_page(chat::Page { messages, older: true, newer: false, revision: 60 }),
        );
        assert!(todo.is_empty(), "the newest page fills a short viewport: {todo:?}");
        let scrolled = place::DiscussionMessage::Scrolled { from_end: 1_900.0, viewport: 300.0 };
        let todo = tell(&mut window, &clock, owls, discussion(scrolled.clone()));
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.anchor == Some(wire::history::Anchor::Before(10)))
            ),
            "{todo:?}"
        );
        assert!(tell(&mut window, &clock, owls, discussion(scrolled)).is_empty(), "one page out at a time");
    }

    /// #general holding messages 10 to 59, each measured 40 pixels tall, its list 300 tall and scrolled up 1,000.
    fn scrolled_up(clock: &Clock) -> (Window, PlaceId) {
        let (mut window, owls) = chatting(clock);
        tell(&mut window, clock, owls, discussion(place::DiscussionMessage::JumpToPresent));
        let messages = (10..60).map(|n| said(n, n, ADA, "line")).collect();
        tell(&mut window, clock, owls, first_page(chat::Page { messages, older: false, newer: false, revision: 60 }));
        let rows: Vec<chat::RowKey> =
            window.places()[0].discussion().map(|open| open.rows().iter().map(chat::Row::key).collect()).unwrap();
        for row in rows {
            tell(&mut window, clock, owls, discussion(place::DiscussionMessage::Measured { row, height: 40.0 }));
        }
        let up = place::DiscussionMessage::Scrolled { from_end: 1_000.0, viewport: 300.0 };
        tell(&mut window, clock, owls, discussion(up));
        (window, owls)
    }

    #[test]
    fn a_page_read_below_keeps_the_rows_in_view_where_they_were() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = scrolled_up(&clock);
        let open = |window: &Window| window.places()[0].discussion().cloned().unwrap();
        let before = open(&window).scroll().clone();
        assert!(matches!(before, chat::Scroll::At { row: chat::RowKey::Message(_), .. }), "{before:?}");
        let moved = open(&window).moved();
        let again = place::DiscussionMessage::Scrolled { from_end: 1_020.0, viewport: 300.0 };
        tell(&mut window, &clock, owls, discussion(again));
        assert_eq!(open(&window).moved(), moved, "a scroll the person makes is the list's own, never moved back");
        let back = place::DiscussionMessage::Scrolled { from_end: 1_000.0, viewport: 300.0 };
        tell(&mut window, &clock, owls, discussion(back));
        let below = place::DiscussionMessage::Measured { row: chat::RowKey::Message(59), height: 140.0 };
        tell(&mut window, &clock, owls, discussion(below));
        assert_eq!(open(&window).scroll(), &before, "the row in view stays the one in view");
        assert_eq!(open(&window).scrolled_up(), 1_100.0, "and the list moves by what grew below it");
        assert_eq!(open(&window).moved(), moved + 1, "which the window moves the list to");
        tell(&mut window, &clock, owls, told(said(60, 61, ADA, "new")));
        assert_eq!(open(&window).scroll(), &before, "a message told below moves nothing in view either");
        assert!(open(&window).scrolled_up() > 1_100.0);
    }

    /// A narrower window wraps the rows below the one in view and the list, holding its distance from the end, says so
    /// again with that distance, before or after the rows' new heights: neither is a scroll the person made.
    #[test]
    fn a_resize_that_wraps_the_rows_below_keeps_the_row_in_view() {
        let clock = Clock(Instant::now());
        let open = |window: &Window| window.places()[0].discussion().cloned().unwrap();
        for measured_first in [false, true] {
            let (mut window, owls) = scrolled_up(&clock);
            let before = open(&window).scroll().clone();
            let resized = place::DiscussionMessage::Scrolled { from_end: 1_000.0, viewport: 280.0 };
            let wrapped = place::DiscussionMessage::Measured { row: chat::RowKey::Message(58), height: 80.0 };
            let told = if measured_first { [wrapped, resized] } else { [resized, wrapped] };
            for message in told {
                tell(&mut window, &clock, owls, discussion(message));
            }
            assert_eq!(open(&window).scroll(), &before, "the row in view stays, measured first: {measured_first}");
            assert_eq!(open(&window).scrolled_up(), 1_040.0, "the list moves by what grew below it");
        }
    }

    #[test]
    fn a_channel_reopened_reads_around_where_it_was_left() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = scrolled_up(&clock);
        let chat::Scroll::At { row: chat::RowKey::Message(place), .. } =
            window.places()[0].discussion().map(|open| open.scroll().clone()).unwrap()
        else {
            panic!("scrolled to a message")
        };
        let mut listed = channels();
        let timeline = terms::TimelineId::parse("t-random").unwrap();
        listed.text.push(place::TextChannel {
            id: crate::harness::channel_id("random"),
            name: "random".into(),
            timeline,
            revision: 0,
        });
        tell(&mut window, &clock, owls, place::Message::Lobby(place::LobbyMessage::Channels(Ok(listed))));
        tell(
            &mut window,
            &clock,
            owls,
            discussion(place::DiscussionMessage::Open(crate::harness::channel_id("random"))),
        );
        let todo = tell(
            &mut window,
            &clock,
            owls,
            discussion(place::DiscussionMessage::Open(crate::harness::channel_id(GENERAL))),
        );
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.anchor == Some(wire::history::Anchor::Around(place)))),
            "{todo:?}"
        );
    }

    #[test]
    fn a_channel_left_for_a_call_reads_around_where_it_was_left() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = scrolled_up(&clock);
        let Some(chat::Scroll::At { row: chat::RowKey::Message(place), .. }) =
            window.places()[0].discussion().map(|open| open.scroll().clone())
        else {
            panic!("scrolled to a message")
        };
        let lounge = place::LobbyMessage::Join(crate::harness::channel_id("lounge"));
        tell(&mut window, &clock, owls, place::Message::Lobby(lounge));
        assert!(window.places()[0].discussion().is_none(), "the call is shown");
        let general = place::DiscussionMessage::Open(crate::harness::channel_id(GENERAL));
        let todo = tell(&mut window, &clock, owls, discussion(general));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.anchor == Some(wire::history::Anchor::Around(place)))),
            "{todo:?}"
        );
    }
}
