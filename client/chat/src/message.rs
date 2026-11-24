//! A message read at the edge: its id, its author, its body and its marks, its files, the message it answers, its
//! reactions and its pin checked once, then trusted.

use crate::body::Body;
use crate::mark::{EmojiId, Mark, MarkKind};
use proto::chatkord_v1 as wire;
use proto::files::{AttachmentName, FileId, MediaType};
use proto::rules::LinkTarget;
pub use terms::MessageId;
use terms::{ChannelId, MemberId, RoleId, TimelineId};

/// Who wrote a message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Author {
    /// A member, by their id at the server.
    Member(MemberId),
    /// An account that is gone: one per account, so two leavers are never one author.
    Former(String),
    /// An incoming webhook's post, kept with the names it was made under.
    App {
        /// The name the post was made under.
        name: String,
        /// The webhook's own name when it posted.
        webhook_name: String,
    },
}

/// A message as a timeline holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// Its id.
    pub id: MessageId,
    /// Who wrote it.
    pub author: Author,
    /// Its order in the timeline: a later message has a greater place.
    pub place: u64,
    /// The timeline's revision at its last change; of two copies the higher is the message now.
    pub revision: u64,
    /// Its text; none once deleted.
    pub body: Option<Body>,
    /// When it was sent, in seconds since the epoch.
    pub created_at: i64,
    /// When it was last edited or deleted; none if never.
    pub edited_at: Option<i64>,
    /// The files sent with it, in the order sent; none once deleted.
    pub attachments: Vec<Attachment>,
    /// The message it answers; none for none.
    pub reply: Option<Reply>,
    /// It answers with a mention of the answered message's author.
    pub mention_author: bool,
    /// Each emoji it was reacted with, in the order first reacted.
    pub reactions: Vec<Reaction>,
    /// The reader's own reactions, of `reactions`, where the server told them: only in an answer to the reader.
    pub mine: Vec<ReactionEmoji>,
    /// When it was pinned, in seconds since the epoch; none if not pinned.
    pub pinned_at: Option<i64>,
    /// The thread started on it; none before one was.
    pub thread: Option<ThreadSummary>,
    /// The preview of the link it holds, as its sender's client made it; none for none, and once deleted.
    pub preview: Option<Card>,
}

/// The preview of a link under a message: the sender's own claim of what the page says, never fetched by a reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// The link, an `https` address the message's text holds.
    pub url: String,
    /// The link's host, read from `url` by the one parse of a link both ends share.
    pub host: String,
    /// The page's title, never empty.
    pub title: String,
    /// The page's description; empty for none.
    pub description: String,
    /// The page's picture; none for none.
    pub picture: Option<CardPicture>,
}

/// The picture of a [`Card`]: its size, and the thumbnail a message shows it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardPicture {
    /// The picture itself, which an edit that keeps the preview sends again.
    pub file: FileId,
    /// Its width in pixels, never 0.
    pub width: u32,
    /// Its height in pixels, never 0.
    pub height: u32,
    /// Its thumbnail, a file's id.
    pub thumbnail: FileId,
}

/// What a message shows of the thread started on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadSummary {
    /// The thread's timeline.
    pub timeline: TimelineId,
    /// The living replies in it.
    pub replies: u32,
    /// When the last living reply was sent, in seconds since the epoch; none with none.
    pub last_reply_at: Option<i64>,
}

/// A file sent with a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// The file, by its id.
    pub file: FileId,
    /// The name it was sent with: shown, and offered when it is saved.
    pub name: AttachmentName,
    /// Its size in bytes.
    pub size: u64,
    /// What it is, as the server judged it.
    pub media: MediaType,
    /// What a picture is beside its bytes; none for any other file.
    pub picture: Option<Picture>,
}

/// A picture sent as a file: its size, and the smaller copy a message shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Its width in pixels, never 0.
    pub width: u32,
    /// Its height in pixels, never 0.
    pub height: u32,
    /// Its thumbnail, a file's id; none where the server made none.
    pub thumbnail: Option<FileId>,
}

/// The message a reply answers, as the reply keeps it: it may since have been deleted.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Reply {
    /// Its id.
    pub id: MessageId,
    /// Its place in the timeline, by which it is asked for when it is not held.
    pub place: u64,
}

/// One emoji a reaction is made with.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReactionEmoji {
    /// A Unicode emoji, as `proto::rules::unicode_emoji` takes it.
    Unicode(String),
    /// A server's own emoji, by its id.
    Custom(EmojiId),
}

impl ReactionEmoji {
    /// The emoji as the wire names it.
    pub fn wire(&self) -> wire::ReactionKey {
        let emoji = match self {
            ReactionEmoji::Unicode(text) => wire::reaction_key::Emoji::Unicode(text.clone()),
            ReactionEmoji::Custom(id) => wire::reaction_key::Emoji::CustomId(id.as_str().to_owned()),
        };
        wire::ReactionKey { emoji: Some(emoji) }
    }

    /// The emoji the wire names, or none out of shape.
    pub fn of_wire(key: &wire::ReactionKey) -> Option<ReactionEmoji> {
        match key.emoji.as_ref()? {
            wire::reaction_key::Emoji::Unicode(text) => {
                proto::rules::unicode_emoji(text).ok().map(|text| ReactionEmoji::Unicode(text.to_owned()))
            }
            wire::reaction_key::Emoji::CustomId(id) => EmojiId::parse(id).map(ReactionEmoji::Custom),
        }
    }
}

/// One emoji reacted with on a message, and how many reacted with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    /// The emoji.
    pub emoji: ReactionEmoji,
    /// How many reacted with it, at least 1.
    pub count: u32,
}

/// Why a message from the wire was passed over: a server not keeping the protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// No id, or an empty one.
    NoId,
    /// No author, or an empty one.
    NoAuthor,
    /// A span that does not fit the body, or names nothing.
    Spans,
    /// A file whose id, name or type is out of shape.
    Attachment,
    /// A reply naming no message.
    Reply,
    /// A reaction whose emoji is out of shape, or counted none.
    Reaction,
    /// A thread naming no timeline.
    Thread,
}

/// A writer's messages in one timeline deleted at once (`MessagesWithdrawn`), read at the edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Withdrawn {
    /// The writer as messages read before they left name them.
    member: Author,
    /// The writer as their messages name them now.
    former: Author,
    /// Every one of theirs at or before this place is deleted.
    through: u64,
    /// The timeline's revision after it.
    revision: u64,
    /// When they were deleted, in seconds since the epoch.
    deleted_at: i64,
}

impl TryFrom<&wire::MessagesWithdrawn> for Withdrawn {
    type Error = Unread;

    fn try_from(told: &wire::MessagesWithdrawn) -> Result<Withdrawn, Unread> {
        let member = MemberId::parse(&told.user_id).ok_or(Unread::NoAuthor)?;
        if told.former_author.is_empty() {
            return Err(Unread::NoAuthor);
        }
        Ok(Withdrawn {
            member: Author::Member(member),
            former: Author::Former(told.former_author.clone()),
            through: told.through,
            revision: told.revision,
            deleted_at: told.deleted_at,
        })
    }
}

impl Withdrawn {
    /// The last place it reaches.
    pub fn through(&self) -> u64 {
        self.through
    }

    /// The timeline's revision after it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Deletes `message` if this reaches it: the writer's, at or before `through`, and older than this. Answers
    /// whether it changed.
    pub fn apply(&self, message: &mut Message) -> bool {
        let theirs = message.author == self.member || message.author == self.former;
        if !theirs || message.place > self.through || message.revision >= self.revision {
            return false;
        }
        message.author = self.former.clone();
        message.body = None;
        message.revision = self.revision;
        message.edited_at = Some(self.deleted_at);
        true
    }
}

impl TryFrom<&wire::Message> for Message {
    type Error = Unread;

    fn try_from(message: &wire::Message) -> Result<Message, Unread> {
        let id = MessageId::parse(&message.message_id).ok_or(Unread::NoId)?;
        let author = match &message.author {
            Some(wire::message::Author::UserId(id)) => Author::Member(MemberId::parse(id).ok_or(Unread::NoAuthor)?),
            Some(wire::message::Author::FormerAuthor(id)) if !id.is_empty() => Author::Former(id.clone()),
            Some(wire::message::Author::App(app)) if !app.name.is_empty() => {
                Author::App { name: app.name.clone(), webhook_name: app.webhook_name.clone() }
            }
            Some(wire::message::Author::FormerAuthor(_) | wire::message::Author::App(_)) | None => {
                return Err(Unread::NoAuthor);
            }
        };
        let body = match &message.body {
            Some(text) => {
                let marks = message.spans.iter().map(mark_of).collect::<Result<Vec<Mark>, Unread>>()?;
                Some(Body::new(text.clone(), marks).map_err(|_| Unread::Spans)?)
            }
            None => None,
        };
        let reply = message
            .reply
            .as_ref()
            .map(|reply| {
                Ok(Reply { id: MessageId::parse(&reply.message_id).ok_or(Unread::Reply)?, place: reply.place })
            })
            .transpose()?;
        let reactions = message
            .reactions
            .iter()
            .map(|reaction| {
                let emoji = reaction.emoji.as_ref().and_then(ReactionEmoji::of_wire).ok_or(Unread::Reaction)?;
                if reaction.count == 0 {
                    return Err(Unread::Reaction);
                }
                Ok(Reaction { emoji, count: reaction.count })
            })
            .collect::<Result<Vec<Reaction>, Unread>>()?;
        let mine =
            message.mine.iter().map(ReactionEmoji::of_wire).collect::<Option<Vec<_>>>().ok_or(Unread::Reaction)?;
        let thread = message
            .thread
            .as_ref()
            .map(|thread| {
                let timeline = TimelineId::parse(&thread.timeline_id).ok_or(Unread::Thread)?;
                Ok(ThreadSummary { timeline, replies: thread.replies, last_reply_at: thread.last_reply_at })
            })
            .transpose()?;
        Ok(Message {
            id,
            author,
            place: message.place,
            revision: message.revision,
            body,
            created_at: message.created_at,
            edited_at: message.edited_at,
            attachments: message.attachments.iter().map(attachment_of).collect::<Result<Vec<_>, Unread>>()?,
            reply,
            mention_author: message.mention_author,
            reactions,
            mine,
            pinned_at: message.pinned_at,
            thread,
            preview: message.body.as_ref().and(message.preview.as_deref()).and_then(card_of),
        })
    }
}

/// A preview as a card: one whose link is not an `https` address without credentials is passed over alone, so it
/// never costs its message.
fn card_of(card: &wire::LinkCard) -> Option<Card> {
    let target = LinkTarget::parse(&card.url).filter(|target| target.is_https() && !target.has_userinfo())?;
    if card.title.is_empty() {
        return None;
    }
    let picture = card.picture.as_ref().and_then(|picture| {
        let thumbnail = FileId::parse(picture.thumbnail.as_deref()?).ok()?;
        let file = FileId::parse(&picture.file_id).ok()?;
        (picture.width > 0 && picture.height > 0).then_some(CardPicture {
            file,
            width: picture.width,
            height: picture.height,
            thumbnail,
        })
    });
    Some(Card {
        host: target.host().to_owned(),
        url: card.url.clone(),
        title: card.title.clone(),
        description: card.description.clone(),
        picture,
    })
}

/// A span as the mark it is: its range in scalars and what it names.
fn mark_of(span: &wire::Span) -> Result<Mark, Unread> {
    use wire::span::Kind;
    let named = |id: Option<MarkKind>| id.ok_or(Unread::Spans);
    let kind = match span.kind.as_ref().ok_or(Unread::Spans)? {
        Kind::UserId(id) => named(MemberId::parse(id).map(MarkKind::Mention))?,
        Kind::RoleId(id) => named(RoleId::parse(id).map(MarkKind::Role))?,
        Kind::Everyone(_) => MarkKind::Everyone,
        Kind::Here(_) => MarkKind::Here,
        Kind::ChannelId(id) => named(ChannelId::parse(id).map(MarkKind::Channel))?,
        Kind::EmojiId(id) => named(EmojiId::parse(id).map(MarkKind::CustomEmoji))?,
        Kind::Link(_) => MarkKind::Link,
    };
    let scalars = |at: u32| usize::try_from(at).map_err(|_| Unread::Spans);
    Ok(Mark { range: scalars(span.start)?..scalars(span.end)?, kind })
}

/// A file as the wire states it: a picture only where it states a size.
fn attachment_of(attachment: &wire::Attachment) -> Result<Attachment, Unread> {
    let file = FileId::parse(&attachment.file_id).map_err(|_| Unread::Attachment)?;
    let name = AttachmentName::parse(&attachment.name).map_err(|_| Unread::Attachment)?;
    let media = MediaType::from_word(&attachment.media_type).ok_or(Unread::Attachment)?;
    let thumbnail = attachment.thumbnail.as_deref().map(FileId::parse).transpose().map_err(|_| Unread::Attachment)?;
    let picture = (media.picture().is_some() && attachment.width > 0 && attachment.height > 0).then_some(Picture {
        width: attachment.width,
        height: attachment.height,
        thumbnail,
    });
    Ok(Attachment { file, name, size: attachment.size, media, picture })
}

/// The marks of `body` as the wire's spans, in its order.
pub fn spans_of(body: &Body) -> Vec<wire::Span> {
    use wire::span::Kind;
    body.marks()
        .iter()
        .filter_map(|mark| {
            let kind = match &mark.kind {
                MarkKind::Mention(id) => Kind::UserId(id.as_str().to_owned()),
                MarkKind::Role(id) => Kind::RoleId(id.as_str().to_owned()),
                MarkKind::Everyone => Kind::Everyone(wire::Everyone {}),
                MarkKind::Here => Kind::Here(wire::Here {}),
                MarkKind::Channel(id) => Kind::ChannelId(id.as_str().to_owned()),
                MarkKind::CustomEmoji(id) => Kind::EmojiId(id.as_str().to_owned()),
                MarkKind::Link => Kind::Link(wire::Link {}),
            };
            let start = u32::try_from(mark.range.start).ok()?;
            let end = u32::try_from(mark.range.end).ok()?;
            Some(wire::Span { start, end, kind: Some(kind) })
        })
        .collect()
}

impl Message {
    /// Whether it was deleted: its place, author and time stay, its text is gone.
    pub fn deleted(&self) -> bool {
        self.body.is_none()
    }

    /// Whether it was edited and is not deleted.
    pub fn edited(&self) -> bool {
        self.body.is_some() && self.edited_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    fn wire(id: &str, author: Option<wire::message::Author>) -> wire::Message {
        wire::Message { message_id: id.into(), timeline_id: "general".into(), author, ..wire::Message::default() }
    }

    #[test]
    fn a_message_without_an_id_or_an_author_is_passed_over() {
        let member = Some(wire::message::Author::UserId(ADA.into()));
        assert_eq!(Message::try_from(&wire("", member.clone())), Err(Unread::NoId));
        assert_eq!(Message::try_from(&wire("m1", None)), Err(Unread::NoAuthor));
        assert_eq!(
            Message::try_from(&wire("m1", Some(wire::message::Author::UserId(String::new())))),
            Err(Unread::NoAuthor)
        );
        let read = Message::try_from(&wire("m1", member)).map(|message| message.author);
        assert_eq!(read, Ok(Author::Member(MemberId::parse(ADA).expect("a member id"))));
    }

    #[test]
    fn an_apps_post_keeps_both_names_and_one_with_no_name_is_passed_over() {
        let app = |name: &str| {
            Some(wire::message::Author::App(Box::new(wire::App { name: name.into(), webhook_name: "Builds".into() })))
        };
        let read = Message::try_from(&wire("m1", app("Nightly"))).map(|message| message.author);
        assert_eq!(read, Ok(Author::App { name: "Nightly".into(), webhook_name: "Builds".into() }));
        assert_eq!(Message::try_from(&wire("m1", app(""))), Err(Unread::NoAuthor));
    }

    #[test]
    fn spans_are_read_as_marks_and_one_that_does_not_fit_passes_the_message_over() {
        use wire::span::Kind;
        let span = |start, end, kind| wire::Span { start, end, kind: Some(kind) };
        let mut told = wire("m1", Some(wire::message::Author::UserId(ADA.into())));
        told.body = Some("hi @Ada see https://example.invalid :owl:".into());
        told.spans = vec![
            span(3, 7, Kind::UserId(ADA.into())),
            span(12, 34, Kind::Link(wire::Link {})),
            span(35, 40, Kind::EmojiId("owl".into())),
        ];
        let read = Message::try_from(&told).expect("read");
        let body = read.body.expect("a body");
        let marks: Vec<(std::ops::Range<usize>, MarkKind)> =
            body.marks().iter().map(|mark| (mark.range.clone(), mark.kind.clone())).collect();
        assert_eq!(
            marks,
            [
                (3..7, MarkKind::Mention(MemberId::parse(ADA).expect("a member id"))),
                (12..34, MarkKind::Link),
                (35..40, MarkKind::CustomEmoji(EmojiId::parse("owl").expect("an emoji id"))),
            ]
        );
        assert_eq!(spans_of(&body), told.spans, "the marks go back out as the spans they came in as");
        told.spans.push(span(38, 99, Kind::Link(wire::Link {})));
        assert_eq!(Message::try_from(&told), Err(Unread::Spans), "past the body");
        told.spans = vec![wire::Span { start: 0, end: 2, kind: None }];
        assert_eq!(Message::try_from(&told), Err(Unread::Spans), "naming nothing");
    }

    #[test]
    fn files_reactions_a_reply_and_a_pin_are_read() {
        let mut told = wire("m1", Some(wire::message::Author::UserId(ADA.into())));
        told.body = Some("dice".into());
        told.attachments = vec![wire::Attachment {
            file_id: "0fa1000000000000000000000000ff10".into(),
            name: "dice.png".into(),
            size: 2_400_000,
            media_type: "image/png".into(),
            width: 1600,
            height: 1200,
            thumbnail: Some("0fa1000000000000000000000000ff11".into()),
        }];
        let thumbs = wire::ReactionKey { emoji: Some(wire::reaction_key::Emoji::Unicode("👍".into())) };
        told.reactions = vec![wire::Reaction { emoji: Some(thumbs.clone()), count: 3 }];
        told.mine = vec![thumbs];
        told.reply = Some(wire::Reply { message_id: "m0".into(), place: 4 });
        told.pinned_at = Some(1_700_000_000);
        let read = Message::try_from(&told).expect("read");
        let picture = read.attachments.first().and_then(|file| file.picture).expect("a picture");
        assert_eq!((picture.width, picture.height), (1600, 1200));
        assert_eq!(read.reactions, [Reaction { emoji: ReactionEmoji::Unicode("👍".into()), count: 3 }]);
        assert_eq!(read.mine, [ReactionEmoji::Unicode("👍".into())]);
        assert_eq!(read.reply.map(|reply| reply.place), Some(4));
        assert_eq!(read.pinned_at, Some(1_700_000_000));
        told.reactions[0].count = 0;
        assert_eq!(Message::try_from(&told), Err(Unread::Reaction));
    }

    #[test]
    fn a_former_author_is_never_a_member() {
        let former = Some(wire::message::Author::FormerAuthor(ADA.into()));
        let read = Message::try_from(&wire("m1", former)).map(|message| message.author);
        assert_eq!(read, Ok(Author::Former(ADA.into())));
        assert_ne!(Author::Former(ADA.into()), Author::Member(MemberId::parse(ADA).expect("a member id")));
    }

    #[test]
    fn a_thread_is_read_with_its_replies_and_one_naming_no_timeline_passes_the_message_over() {
        let author = Some(wire::message::Author::UserId(ADA.into()));
        let mut told = wire("m1", author);
        told.thread = Some(wire::ThreadSummary { timeline_id: "t-1".into(), replies: 3, last_reply_at: Some(90) });
        let read = Message::try_from(&told).expect("read").thread;
        assert_eq!(
            read,
            Some(ThreadSummary {
                timeline: TimelineId::parse("t-1").expect("a timeline"),
                replies: 3,
                last_reply_at: Some(90)
            })
        );
        told.thread = Some(wire::ThreadSummary { timeline_id: String::new(), replies: 1, last_reply_at: None });
        assert_eq!(Message::try_from(&told), Err(Unread::Thread));
    }

    #[test]
    fn a_preview_is_a_card_with_its_host_read_from_its_link_and_one_out_of_shape_costs_only_itself() {
        let mut told = wire("m1", Some(wire::message::Author::UserId(ADA.into())));
        told.body = Some("see https://blog.example.invalid/a".into());
        let card = |url: &str, title: &str| wire::LinkCard {
            url: url.into(),
            title: title.into(),
            description: "About it".into(),
            picture: Some(wire::Attachment {
                file_id: "0fa1000000000000000000000000ff10".into(),
                media_type: "image/png".into(),
                width: 800,
                height: 400,
                thumbnail: Some("0fa1000000000000000000000000ff11".into()),
                ..wire::Attachment::default()
            }),
        };
        told.preview = Some(Box::new(card("https://blog.example.invalid:8443/a", "A page")));
        let read = Message::try_from(&told).expect("read").preview.expect("a card");
        assert_eq!((read.host.as_str(), read.title.as_str()), ("blog.example.invalid", "A page"));
        assert_eq!(read.picture.map(|picture| (picture.width, picture.height)), Some((800, 400)));
        for (url, title) in [
            ("http://blog.example.invalid/a", "T"),
            ("https://u@blog.example.invalid/a", "T"),
            ("https://blog.example.invalid/a", ""),
        ] {
            told.preview = Some(Box::new(card(url, title)));
            let read = Message::try_from(&told).expect("the message is still read");
            assert_eq!(read.preview, None, "{url} {title}");
        }
        told.preview = Some(Box::new(card("https://blog.example.invalid/a", "A page")));
        told.body = None;
        assert_eq!(Message::try_from(&told).expect("read").preview, None, "a deleted message holds none");
    }
}
