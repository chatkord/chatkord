//! What a timeline of a thousand messages costs the window each time one is told: its rows built again, the heights
//! kept to them, the row in view followed and the rows to build picked. Its own binary, timed alone.
#![allow(clippy::expect_used, clippy::panic, reason = "tests: a failed step is the test failing")]

mod timing {
    use chat::{Author, Body, Heights, Message, MessageId, Page, Row, RowKey, Scroll, TimeZone, Timeline};
    use std::time::{Duration, Instant};
    use terms::MemberId;

    /// What one told message may cost the window at most, a thousand held, unoptimised as the gate builds it: half a
    /// 60 Hz frame. Measured in the commit that set it, with what an optimised build takes.
    const BUDGET: Duration = Duration::from_millis(8);

    fn message(place: u64, author: &MemberId) -> Message {
        Message {
            id: MessageId::parse(&format!("m{place}")).expect("an id"),
            author: Author::Member(author.clone()),
            place,
            revision: place,
            body: Some(Body::plain("a line of chat, about as long as most are")),
            created_at: 1_763_035_200 + i64::try_from(place).expect("small") * 61,
            edited_at: None,
            attachments: Vec::new(),
            reply: None,
            mention_author: false,
            reactions: Vec::new(),
            mine: Vec::new(),
            pinned_at: None,
            thread: None,
            preview: None,
        }
    }

    #[test]
    fn a_thousand_messages_are_rowed_and_windowed_in_budget() {
        let (ada, bo) = (MemberId::parse("ada").expect("an id"), MemberId::parse("bo").expect("an id"));
        let author = |place: u64| if place.is_multiple_of(3) { &bo } else { &ada };
        let messages = (1..=1_000).map(|place| message(place, author(place))).collect();
        let mut timeline = Timeline::from_page(Page { messages, older: true, newer: false, revision: 1_000 });
        let zone = TimeZone::UTC;
        let mut heights = Heights::new(44.0);
        let keys: Vec<RowKey> = chat::rows(&timeline, &zone).iter().map(Row::key).collect();
        for (at, row) in keys.iter().enumerate() {
            heights.measured(row.clone(), if at.is_multiple_of(7) { 88.0 } else { 44.0 });
        }
        let scroll = heights.scroll(&keys, 5_000.0);
        assert!(matches!(scroll, Scroll::At { .. }), "{scroll:?}");
        let mut took = Vec::new();
        for place in 1_001..1_201 {
            let started = Instant::now();
            timeline.told(message(place, author(place)));
            let keys: Vec<RowKey> = chat::rows(&timeline, &zone).iter().map(Row::key).collect();
            heights.retain(&keys);
            let from_end = heights.distance(&keys, &scroll).expect("the row in view is held");
            let built = heights.window_keeping(&keys, from_end, 900.0, 600.0, &[]);
            took.push(started.elapsed());
            assert!(built.end > built.first, "rows are built");
        }
        took.sort_unstable();
        let median = took[took.len() / 2];
        assert!(median < BUDGET, "a told message cost {median:?} against {BUDGET:?}, a thousand held");
    }
}
