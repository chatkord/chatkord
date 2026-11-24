//! The rows a channel's list shows: a day line where the day changes, and each message marked where a run of one
//! author's messages starts, as Discord, Slack, Element and Sharkord group them.

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

use crate::message::Message;
use crate::timeline::Timeline;

/// A message carries on its author's run when sent within this many seconds of the one before.
pub const RUN_SECONDS: i64 = 7 * 60;

/// One row of a channel's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row<'a> {
    /// The day the messages under it were sent, where the person is.
    Day(Date),
    /// A message, and whether it starts a run (its author's name and picture shown) or carries one on.
    Message {
        /// The message.
        message: &'a Message,
        /// It starts a run.
        starts_run: bool,
    },
}

/// What a row is known by, so its height and its state follow it wherever the list moves it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RowKey {
    /// A day line, by its day.
    Day(Date),
    /// A message, by its place in the timeline: what stays when an id would not be held, and what orders the rows.
    Message(u64),
}

impl Row<'_> {
    /// What the row is known by.
    pub fn key(&self) -> RowKey {
        match self {
            Row::Day(day) => RowKey::Day(*day),
            Row::Message { message, .. } => RowKey::Message(message.place),
        }
    }
}

/// When `seconds` since the epoch was, where the person is: none for a moment outside what a calendar holds.
pub fn local(seconds: i64, zone: &TimeZone) -> Option<DateTime> {
    Timestamp::from_second(seconds).ok().map(|at| zone.to_datetime(at))
}

/// The moment, in seconds since the epoch, that is `hour` o'clock on the day after the one `seconds` is on, where the
/// person is: none for a moment outside what a calendar holds.
pub fn tomorrow_at(seconds: i64, zone: &TimeZone, hour: i8) -> Option<i64> {
    let today = local(seconds, zone)?.date();
    let morning = today.tomorrow().ok()?.at(hour, 0, 0, 0);
    zone.to_zoned_datetime(morning).ok().map(|zoned| zoned.timestamp().as_second())
}

/// The rows of `timeline`, oldest first, its days read in `zone`.
pub fn rows<'a>(timeline: &'a Timeline, zone: &TimeZone) -> Vec<Row<'a>> {
    let mut rows = Vec::with_capacity(timeline.len() + 8);
    let mut before: Option<(&Message, Option<Date>)> = None;
    for message in timeline.messages() {
        let day = local(message.created_at, zone).map(|at| at.date());
        let new_day = before.is_none_or(|(_, then)| then != day);
        if new_day && let Some(day) = day {
            rows.push(Row::Day(day));
        }
        // A reply always opens a run of its own, its quoted line under its author's name, as Discord draws one.
        let carries_on = !new_day
            && message.reply.is_none()
            && before.is_some_and(|(earlier, _)| {
                earlier.author == message.author
                    && (0..=RUN_SECONDS).contains(&message.created_at.saturating_sub(earlier.created_at))
            });
        rows.push(Row::Message { message, starts_run: !carries_on });
        before = Some((message, day));
    }
    rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn tomorrow_morning_is_the_next_days_hour_where_the_person_is() {
        let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(2));
        // 2025-11-13 23:30 at +02:00 is 21:30 UTC; tomorrow 08:00 there is 06:00 UTC on the 14th.
        let now = 1_763_069_400;
        let morning = super::tomorrow_at(now, &zone, 8).unwrap();
        assert_eq!(morning - now, 8 * 3_600 + 30 * 60);
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(
            super::tomorrow_at(now, &utc, 8).unwrap() - now,
            10 * 3_600 + 30 * 60,
            "the same moment, another zone"
        );
    }

    use super::*;
    use crate::message::Author;
    use crate::timeline::{Page, tests::message};

    const BO: &str = "bo";

    fn timeline(messages: Vec<Message>) -> Timeline {
        Timeline::from_page(Page { messages, older: false, newer: false, revision: 1 })
    }

    fn at(place: u64, created_at: i64) -> Message {
        Message { created_at, ..message(place, place, "hi") }
    }

    fn starts(messages: Vec<Message>, zone: &TimeZone) -> Vec<Option<bool>> {
        let timeline = timeline(messages);
        rows(&timeline, zone)
            .iter()
            .map(|row| match row {
                Row::Day(_) => None,
                Row::Message { starts_run, .. } => Some(*starts_run),
            })
            .collect()
    }

    const NOON: i64 = 1_763_035_200;

    #[test]
    fn a_run_is_one_author_within_seven_minutes_of_the_one_before() {
        let mut other = at(4, NOON + 120 + 420 + 5);
        other.author = Author::Member(terms::MemberId::parse(BO).expect("a member id"));
        let messages = vec![at(1, NOON), at(2, NOON + 120), at(3, NOON + 120 + 420), other, at(5, NOON + 2_000)];
        let starts = starts(messages, &TimeZone::UTC);
        assert_eq!(starts, [None, Some(true), Some(false), Some(false), Some(true), Some(true)]);
    }

    #[test]
    fn a_reply_opens_a_run_of_its_own_and_the_next_message_carries_on_from_it() {
        let mut reply = at(2, NOON + 60);
        reply.reply = Some(crate::message::Reply { id: crate::timeline::tests::message(1, 1, "hi").id, place: 1 });
        let messages = vec![at(1, NOON), reply, at(3, NOON + 120)];
        assert_eq!(starts(messages, &TimeZone::UTC), [None, Some(true), Some(true), Some(false)]);
    }

    #[test]
    fn a_former_author_and_a_member_of_the_same_id_are_two_runs() {
        let mut former = at(2, NOON + 60);
        former.author = Author::Former("ada".into());
        assert_eq!(starts(vec![at(1, NOON), former], &TimeZone::UTC), [None, Some(true), Some(true)]);
    }

    #[test]
    fn a_day_line_starts_each_day_where_the_person_is_and_breaks_the_run() {
        let late = NOON + 11 * 3_600 + 58 * 60;
        let messages = vec![at(1, late), at(2, late + 180)];
        let in_utc = starts(messages.clone(), &TimeZone::UTC);
        assert_eq!(in_utc, [None, Some(true), None, Some(true)], "past midnight in UTC: a new day");
        let west = TimeZone::fixed(jiff::tz::offset(-5));
        assert_eq!(starts(messages, &west), [None, Some(true), Some(false)], "still the evening five hours west");
    }

    #[test]
    fn a_message_before_the_one_before_it_starts_a_run() {
        let starts = starts(vec![at(1, NOON), at(2, NOON - 30)], &TimeZone::UTC);
        assert_eq!(starts, [None, Some(true), Some(true)], "a clock that went back is not a run");
    }
}
