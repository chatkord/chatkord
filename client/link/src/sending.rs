//! The two asks that build their op from more than one value: a message sent and a message edited, with the preview of
//! one of its links. Every other ask is made where its answer is read, by `Link::ask`.

use crate::session::Link;
use proto::chatkord_v1::{EditMessage, LinkPreview, Message, SendMessage, Span};
use proto::files::FileId;
use terms::{LinkError, MessageId, Nonce, TimelineId};

/// A message to send: its text and what it names, its files, the message it answers and the thread it begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sending<'a> {
    /// The timeline it goes to: for the first reply of a thread, its channel's.
    pub timeline: &'a TimelineId,
    /// The root of the thread it begins, else none; the thread's timeline is the server's to make.
    pub thread_of: Option<&'a MessageId>,
    /// Its text, in the shape the server takes.
    pub body: &'a str,
    /// What its text names, sorted and apart.
    pub spans: &'a [Span],
    /// The files uploaded for it, in the order sent.
    pub files: &'a [FileId],
    /// The message it answers, and whether it mentions that message's author.
    pub reply: Option<(&'a MessageId, bool)>,
    /// The same for every retry of this send.
    pub nonce: Nonce,
}

impl Link {
    /// Sends a message with the preview of one of its links, answered with it as stored. Only to a server that serves
    /// `LINK_PREVIEWS` (`Link::switched_off`): one with it switched off refuses the message.
    pub async fn send_with_preview(
        &self,
        sending: Sending<'_>,
        preview: Option<LinkPreview>,
    ) -> Result<Message, LinkError> {
        let files = sending.files.iter().flat_map(|file| *file.as_bytes()).collect();
        self.ask(SendMessage {
            timeline_id: sending.timeline.as_str().to_owned(),
            body: sending.body.into(),
            nonce: sending.nonce.bytes().to_vec(),
            spans: sending.spans.to_vec(),
            files,
            reply_to: sending.reply.map(|(id, _)| id.as_str().to_owned()),
            thread_of: sending.thread_of.map(|root| root.as_str().to_owned()),
            mention_author: sending.reply.is_some_and(|(_, mention)| mention),
            preview: preview.map(Box::new),
        })
        .await
    }

    /// The same, the message's preview becoming `preview`: none drops the one it had. Only to a server that serves
    /// `LINK_PREVIEWS`, as `send_with_preview` says.
    pub async fn edit_with_preview(
        &self,
        message_id: &MessageId,
        body: &str,
        spans: &[Span],
        preview: Option<LinkPreview>,
    ) -> Result<Message, LinkError> {
        let edit = EditMessage {
            message_id: message_id.as_str().to_owned(),
            body: body.into(),
            spans: spans.to_vec(),
            preview: preview.map(Box::new),
        };
        self.ask(edit).await
    }
}
