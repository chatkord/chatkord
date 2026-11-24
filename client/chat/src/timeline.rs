//! One channel's messages as this client holds them: a contiguous stretch of its history, every copy the newest one
//! read, bounded so scrolling far never holds the whole channel.

use std::collections::{BTreeMap, HashMap};

use proto::chatkord_v1 as wire;

use crate::message::{Message, MessageId};

/// The most messages a timeline holds; past it, the end away from where the person reads is let go.
pub const MAX_KEPT: usize = 1_000;

/// A page of history read at the edge, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    /// The messages, in place order; one out of shape is passed over.
    pub messages: Vec<Message>,
    /// Older messages are kept before the first.
    pub older: bool,
    /// Newer messages follow the last.
    pub newer: bool,
    /// The channel's revision when it was read.
    pub revision: u64,
}

impl From<&wire::MessagePage> for Page {
    fn from(page: &wire::MessagePage) -> Page {
        Page {
            messages: page.messages.iter().filter_map(|message| Message::try_from(message).ok()).collect(),
            older: page.older,
            newer: page.newer,
            revision: page.revision,
        }
    }
}

/// Which end of a timeline a page extends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Before the first message held.
    Older,
    /// After the last message held.
    Newer,
}

/// A contiguous stretch of a channel's messages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    by_place: BTreeMap<u64, Message>,
    places: HashMap<MessageId, u64>,
    older: bool,
    newer: bool,
    revision: u64,
}

impl Timeline {
    /// The timeline a page begins: the newest, a jump to a message, or a reload.
    pub fn from_page(page: Page) -> Timeline {
        let mut timeline =
            Timeline { older: page.older, newer: page.newer, revision: page.revision, ..Timeline::default() };
        for message in page.messages {
            timeline.keep(message);
        }
        timeline
    }

    /// Adds a page read at `end`, from the message there; what goes past [`MAX_KEPT`] is let go at the other end.
    pub fn extend(&mut self, end: End, page: Page) {
        match end {
            End::Older => self.older = page.older,
            End::Newer => self.newer = page.newer,
        }
        self.revision = self.revision.max(page.revision);
        for message in page.messages {
            self.keep(message);
        }
        while self.by_place.len() > MAX_KEPT {
            let gone = match end {
                End::Older => self.by_place.pop_last().map(|(_, message)| (message, End::Newer)),
                End::Newer => self.by_place.pop_first().map(|(_, message)| (message, End::Older)),
            };
            let Some((message, trimmed)) = gone else { break };
            self.places.remove(&message.id);
            match trimmed {
                End::Older => self.older = true,
                End::Newer => self.newer = true,
            }
        }
    }

    /// A message told as it is now. A copy held is replaced only by one of a higher revision, so a stale edit never
    /// wins; one not held is taken only where it falls inside what is held or after its newest end. Answers whether
    /// the timeline changed.
    pub fn told(&mut self, message: Message) -> bool {
        self.revision = self.revision.max(message.revision);
        if let Some(place) = self.places.get(&message.id) {
            return match self.by_place.get_mut(place) {
                Some(held) if message.revision > held.revision => {
                    *held = message;
                    true
                }
                Some(_) | None => false,
            };
        }
        let inside = match (self.by_place.first_key_value(), self.by_place.last_key_value()) {
            (Some((&first, _)), Some((&last, _))) => {
                (first..=last).contains(&message.place) || (message.place > last && !self.newer)
            }
            _ => !self.newer,
        };
        if inside {
            self.keep(message);
        }
        inside
    }

    /// A writer's messages withdrawn at once: each one held that it reaches is deleted. Answers whether the timeline
    /// changed.
    pub fn withdrawn(&mut self, withdrawn: &crate::Withdrawn) -> bool {
        self.revision = self.revision.max(withdrawn.revision());
        let mut changed = false;
        for message in self.by_place.range_mut(..=withdrawn.through()).map(|(_, message)| message) {
            changed |= withdrawn.apply(message);
        }
        changed
    }

    /// The changes a catch-up told, in revision order, and the channel's revision then.
    pub fn caught_up(&mut self, changed: Vec<Message>, revision: u64) -> bool {
        let mut moved = false;
        for message in changed {
            moved |= self.told(message);
        }
        self.revision = self.revision.max(revision);
        moved
    }

    fn keep(&mut self, message: Message) {
        match self.by_place.get(&message.place) {
            Some(held) if held.revision >= message.revision => {}
            Some(_) | None => {
                self.places.insert(message.id.clone(), message.place);
                self.by_place.insert(message.place, message);
            }
        }
    }

    /// The newest revision read: where a catch-up starts.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Older messages are kept before the first held.
    pub fn older(&self) -> bool {
        self.older
    }

    /// Newer messages follow the last held: the newest is not here.
    pub fn newer(&self) -> bool {
        self.newer
    }

    /// The messages held, oldest first.
    pub fn messages(&self) -> impl DoubleEndedIterator<Item = &Message> + ExactSizeIterator {
        self.by_place.values()
    }

    /// The message held under `id`.
    pub fn get(&self, id: &MessageId) -> Option<&Message> {
        self.places.get(id).and_then(|place| self.by_place.get(place))
    }

    /// The message held at `place`.
    pub fn at(&self, place: u64) -> Option<&Message> {
        self.by_place.get(&place)
    }

    /// The oldest message held.
    pub fn first(&self) -> Option<&Message> {
        self.by_place.values().next()
    }

    /// The newest message held.
    pub fn last(&self) -> Option<&Message> {
        self.by_place.values().next_back()
    }

    /// How many messages are held.
    pub fn len(&self) -> usize {
        self.by_place.len()
    }

    /// Whether none is held.
    pub fn is_empty(&self) -> bool {
        self.by_place.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::message::Author;

    const ADA: &str = "ada";

    pub(crate) fn message(place: u64, revision: u64, body: &str) -> Message {
        Message {
            id: MessageId::parse(&format!("m{place}")).expect("a place makes an id"),
            author: Author::Member(terms::MemberId::parse(ADA).expect("a member id")),
            place,
            revision,
            body: Some(crate::Body::plain(body)),
            created_at: 1_700_000_000 + i64::try_from(place).unwrap_or(0) * 60,
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

    fn page(places: std::ops::Range<u64>, older: bool, newer: bool) -> Page {
        Page { messages: places.map(|place| message(place, place, "hi")).collect(), older, newer, revision: 1_000 }
    }

    fn bodies(timeline: &Timeline) -> Vec<String> {
        timeline.messages().filter_map(|message| message.body.as_ref().map(|body| body.text().to_string())).collect()
    }

    #[test]
    fn a_stale_edit_never_overwrites_a_newer_one() {
        let mut timeline = Timeline::from_page(page(1..4, false, false));
        assert!(timeline.told(message(2, 20, "second edit")));
        assert!(!timeline.told(message(2, 10, "first edit")), "an older copy told late changes nothing");
        assert_eq!(bodies(&timeline), ["hi", "second edit", "hi"]);
        let changes = vec![message(2, 15, "a catch-up's older copy")];
        assert!(!timeline.caught_up(changes, 30));
        assert_eq!(bodies(&timeline), ["hi", "second edit", "hi"]);
    }

    #[test]
    fn a_withdrawal_deletes_the_writer_s_messages_through_its_place_and_no_newer_copy() {
        let mut timeline = Timeline::from_page(page(1..6, false, false));
        let mut bo = message(3, 3, "bo's");
        bo.author = Author::Member(terms::MemberId::parse("bo").expect("a member id"));
        timeline.told(Message { revision: 4, ..bo.clone() });
        let mut later = message(6, 6, "bo's, later");
        later.author = Author::Former("f7".into());
        timeline.told(later);
        let told = proto::chatkord_v1::MessagesWithdrawn {
            timeline_id: "1".into(),
            user_id: "bo".into(),
            former_author: "f7".into(),
            through: 5,
            revision: 1_001,
            deleted_at: 1_700_000_999,
        };
        let withdrawn = crate::Withdrawn::try_from(&told).expect("read");
        assert!(timeline.withdrawn(&withdrawn));
        let gone: Vec<u64> = timeline.messages().filter(|message| message.deleted()).map(|m| m.place).collect();
        assert_eq!(gone, [3], "bo's, through place 5; ada's and the one past it stay");
        assert_eq!(
            timeline.get(&bo.id).map(|m| (&m.author, m.edited_at)),
            Some((&Author::Former("f7".into()), Some(1_700_000_999)))
        );
        assert_eq!(timeline.revision(), 1_001);
        assert!(!timeline.withdrawn(&withdrawn), "told twice, it changes nothing");
        let nameless = proto::chatkord_v1::MessagesWithdrawn { user_id: String::new(), ..told };
        assert!(crate::Withdrawn::try_from(&nameless).is_err(), "a server not keeping the protocol is passed over");
    }

    #[test]
    fn a_new_message_is_taken_only_at_the_newest_end() {
        let mut timeline = Timeline::from_page(page(1..4, true, false));
        assert!(timeline.told(message(9, 9, "new")), "the newest end is held: it is taken");
        let mut back_in_time = Timeline::from_page(page(1..4, true, true));
        assert!(!back_in_time.told(message(9, 9, "new")), "with newer messages unread, it would leave a gap");
        assert_eq!(back_in_time.len(), 3);
        assert!(!back_in_time.told(message(10, 1_001, "newer")));
        assert_eq!(back_in_time.revision(), 1_001, "a message passed over still says how far the channel was read");
    }

    #[test]
    fn a_page_extends_its_end_and_lets_the_far_end_go_past_the_bound() {
        let newest = u64::try_from(MAX_KEPT).unwrap_or(0) + 50;
        let mut timeline = Timeline::from_page(page(newest - 100..newest, true, false));
        timeline.extend(End::Older, page(1..newest - 100, false, true));
        assert_eq!(timeline.len(), MAX_KEPT);
        assert_eq!(timeline.first().map(|message| message.place), Some(1), "the page read is kept whole");
        assert!(!timeline.older(), "the oldest page says nothing is before it");
        assert!(timeline.newer(), "the newest end was let go, so newer messages follow");
        timeline.extend(End::Newer, page(newest - 100..newest, true, false));
        assert_eq!(timeline.last().map(|message| message.place), Some(newest - 1));
        assert!(timeline.older() && !timeline.newer());
        assert!(timeline.get(&message(1, 1, "").id).is_none(), "a message let go is gone from the index too");
    }

    #[test]
    fn a_tombstone_replaces_its_message_and_keeps_its_place() {
        let mut timeline = Timeline::from_page(page(1..4, false, false));
        let mut tombstone = message(2, 50, "");
        tombstone.body = None;
        assert!(timeline.told(tombstone));
        let held = timeline.get(&message(2, 0, "").id);
        assert!(held.is_some_and(Message::deleted));
        assert_eq!(timeline.len(), 3);
    }
}
