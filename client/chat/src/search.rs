//! A search's answer read at the edge: the messages that matched, each with the timeline it is in and, for a reply in
//! a thread, where that thread stands, so a result can be opened where it was written.

use crate::message::{Message, MessageId};
use proto::chatkord_v1 as wire;
use proto::files::FileId;
use terms::{ChannelId, TimelineId};

/// Where a match was written, beyond its timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Home {
    /// A channel's or a conversation's own timeline.
    Timeline,
    /// A thread of a channel, started on one of its messages.
    Thread {
        /// The channel the thread is in.
        channel: ChannelId,
        /// The message the thread was started on.
        root: MessageId,
    },
}

/// One message a search found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The message, as a page of its timeline holds it.
    pub message: Message,
    /// The timeline it is in.
    pub timeline: TimelineId,
    /// Where it stands.
    pub home: Home,
    /// The attachments whose names matched, in the order sent; none when only the body did.
    pub files: Vec<FileId>,
    /// Whether the body matched: so a match naming no file, as every match of a server before file search is.
    pub in_body: bool,
}

/// A page of matches, newest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    /// The matches the page holds; one out of shape is passed over.
    pub found: Vec<Found>,
    /// More matches follow the last.
    pub older: bool,
}

impl Found {
    /// A match read from the wire, or none when it is out of shape: no message, a message passed over, or a thread's
    /// reply naming only half of where the thread is.
    fn of(found: &wire::Found) -> Option<Found> {
        let sent = found.message.as_deref()?;
        let message = Message::try_from(sent).ok()?;
        let timeline = TimelineId::parse(&sent.timeline_id)?;
        let home = match (&found.channel_id, &found.root_message_id) {
            (None, None) => Home::Timeline,
            (Some(channel), Some(root)) => {
                Home::Thread { channel: ChannelId::parse(channel)?, root: MessageId::parse(root)? }
            }
            (Some(_), None) | (None, Some(_)) => return None,
        };
        let files = found.file_ids.iter().map(|id| FileId::parse(id).ok()).collect::<Option<Vec<_>>>()?;
        // A server before file search leaves `in_body` unset; a match that names no file matched by its body.
        let in_body = found.in_body || files.is_empty();
        Some(Found { message, timeline, home, files, in_body })
    }
}

impl From<&wire::SearchPage> for Results {
    fn from(page: &wire::SearchPage) -> Results {
        Results { found: page.found.iter().filter_map(Found::of).collect(), older: page.older }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    fn match_of(id: &str, timeline: &str, thread: Option<(&str, &str)>) -> wire::Found {
        let message = wire::Message {
            message_id: id.into(),
            timeline_id: timeline.into(),
            body: Some("dice".into()),
            author: Some(wire::message::Author::UserId(ADA.into())),
            ..wire::Message::default()
        };
        wire::Found {
            message: Some(Box::new(message)),
            channel_id: thread.map(|(channel, _)| channel.into()),
            root_message_id: thread.map(|(_, root)| root.into()),
            file_ids: Vec::new(),
            in_body: true,
        }
    }

    #[test]
    fn a_match_keeps_its_timeline_and_a_thread_s_reply_where_the_thread_stands() {
        let page = wire::SearchPage {
            found: vec![match_of("m2", "general", None), match_of("m3", "t-1", Some(("general", "m1")))],
            older: true,
        };
        let results = Results::from(&page);
        assert!(results.older);
        let homes: Vec<(&str, Home)> =
            results.found.iter().map(|found| (found.timeline.as_str(), found.home.clone())).collect();
        assert_eq!(
            homes,
            [
                ("general", Home::Timeline),
                (
                    "t-1",
                    Home::Thread {
                        channel: ChannelId::parse("general").expect("a channel"),
                        root: MessageId::parse("m1").expect("a message")
                    }
                )
            ]
        );
    }

    #[test]
    fn a_match_out_of_shape_is_passed_over_alone() {
        let half = wire::Found { channel_id: Some("general".into()), ..match_of("m4", "t-1", None) };
        let none = wire::Found { message: None, ..match_of("m5", "general", None) };
        let page = wire::SearchPage { found: vec![half, none, match_of("m6", "general", None)], older: false };
        let results = Results::from(&page);
        let ids: Vec<&str> = results.found.iter().map(|found| found.message.id.as_str()).collect();
        assert_eq!(ids, ["m6"]);
    }

    #[test]
    fn a_match_naming_no_file_matched_by_its_body_whatever_an_older_server_left_unset() {
        let older = wire::Found { in_body: false, ..match_of("m2", "general", None) };
        let file = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);
        let named = wire::Found { in_body: false, file_ids: vec![file.to_string()], ..match_of("m3", "general", None) };
        let page = wire::SearchPage { found: vec![older, named], older: false };
        let read: Vec<(bool, usize)> =
            Results::from(&page).found.iter().map(|found| (found.in_body, found.files.len())).collect();
        assert_eq!(read, [(true, 0), (false, 1)]);
    }
}
