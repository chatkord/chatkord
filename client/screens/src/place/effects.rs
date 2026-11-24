//! What a place asks of its host, and what it tells the window.

use super::*;
use proto::chatkord_v1::wire_message::Body;

/// One op asked of the server, made where its answer is read: the op as the schema has it, and what its reply (or
/// the refusal) becomes. The host sends it and carries the answer back, deciding nothing of either.
pub struct Asked {
    op: Box<Body>,
    answer: Answer,
}

/// What the answer of an ask becomes, once it is read as the reply its op names.
pub type Answer = Box<dyn FnOnce(Result<Body, LinkError>) -> Option<Message> + Send>;

impl Asked {
    /// `op`, whose reply (or a refusal, or a reply of another op as `LinkError::Malformed`) becomes `answer`'s message.
    pub fn new<A, M>(op: A, answer: impl FnOnce(Result<A::Reply, LinkError>) -> M + Send + 'static) -> Self
    where
        A: proto::ops::Ask,
        M: Into<Message>,
    {
        Self::heard(op, move |read| Some(answer(read).into()))
    }

    /// `op`, whose answer nothing on screen waits for: `told` takes it, to note a failure, and the window hears nothing.
    pub fn unheard<A>(op: A, told: impl FnOnce(Result<A::Reply, LinkError>) + Send + 'static) -> Self
    where
        A: proto::ops::Ask,
    {
        Self::heard(op, move |read| {
            told(read);
            None
        })
    }

    fn heard<A>(op: A, answer: impl FnOnce(Result<A::Reply, LinkError>) -> Option<Message> + Send + 'static) -> Self
    where
        A: proto::ops::Ask,
    {
        let read = move |answered: Result<Body, LinkError>| {
            answer(answered.and_then(|body| <A::Reply as proto::ops::Reply>::of(body).ok_or(LinkError::Malformed)))
        };
        Self { op: Box::new(op.into()), answer: Box::new(read) }
    }

    /// The op to send.
    pub fn op(&self) -> &Body {
        &self.op
    }

    /// The op to send and what to do with its answer: [`Asked::answered`].
    pub fn into_parts(self) -> (Body, Answer) {
        (*self.op, self.answer)
    }

    /// What the answer `answered` becomes: the message to tell the window, none for an ask that is not heard.
    pub fn answered(self, answered: Result<Body, LinkError>) -> Option<Message> {
        (self.answer)(answered)
    }
}

impl std::fmt::Debug for Asked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Asked").field(&self.op).finish()
    }
}

/// A reply read as done: the answer of an op that answers with nothing.
pub(crate) fn done<R>(answer: Result<R, LinkError>) -> Result<(), LinkError> {
    answer.map(|_| ())
}

impl Ask {
    /// The op this ask sends the server, when it is one op and no more.
    pub fn op(&self) -> Option<&Body> {
        match self {
            Self::Op(asked) => Some(asked.op()),
            _ => None,
        }
    }
}

/// What a server's place asks of the host.
#[derive(Debug)]
pub enum Ask {
    /// What the sign-in asks, carried out for this server.
    Door(door::Ask),
    /// Ask the server one op, whose answer is read into the message `Asked` names.
    Op(Asked),
    /// List the applications playing sound now; report [`crate::CallMessage::SoundApps`].
    ListSoundApps,
    /// Tell the server this device went idle, or that input is back; nothing is reported.
    Activity {
        /// No input for the host's limit.
        idle: bool,
    },
    /// Show a desktop popup for a message that would alert, under this timeline's id, in place of an earlier one for
    /// it; nothing is reported, but a click is the window's [`crate::WindowMessage::PopupClicked`].
    Popup {
        /// The timeline the message is in.
        timeline: TimelineId,
        /// Its title.
        title: Sentence,
        /// Its body.
        body: Sentence,
    },
    /// Take away the popup shown for this timeline, its messages being read.
    Withdraw {
        /// The timeline.
        timeline: TimelineId,
    },
    /// Send a message; report [`DiscussionMessage::Sent`] with its nonce.
    SendMessage {
        /// The timeline it goes to: for the first reply of a thread, its channel's.
        timeline: TimelineId,
        /// The root of the thread it begins; none for any other message.
        thread_of: Option<chat::MessageId>,
        /// Its text, already in the shape the server takes.
        body: String,
        /// What its text names, in the shape `proto::rules::spans_shape` takes.
        spans: Vec<proto::chatkord_v1::Span>,
        /// The files uploaded for it, in the order shown.
        files: Vec<FileId>,
        /// The message it answers, and whether it mentions that message's author.
        reply: Option<(chat::MessageId, bool)>,
        /// The same for every retry of this send.
        nonce: terms::Nonce,
        /// The preview of one of its links, made by this device.
        preview: Option<PreviewMade>,
    },
    /// Replace one's own message's text; report [`DiscussionMessage::Changed`].
    EditMessage {
        /// The message's id at the server.
        message_id: chat::MessageId,
        /// Its new text.
        body: String,
        /// What its new text names.
        spans: Vec<proto::chatkord_v1::Span>,
        /// The preview the message goes on with, as an edit replaces it.
        preview: Option<PreviewMade>,
    },
    /// Read the page at `link`, a first link of a draft of `compose`, for its preview, and send its picture too when
    /// `picture`; report [`DiscussionMessage::Previewed`].
    FetchPreview {
        /// The composer that holds the draft.
        compose: Compose,
        /// The link, as the draft holds it.
        link: String,
        /// Whether this account may attach files, so the picture may go to the server.
        picture: bool,
    },
    /// Tell the server this account is writing in a timeline; nothing is reported.
    Typing {
        /// The timeline.
        timeline: TimelineId,
    },
    /// Open the system's picker for files to attach, any type, several at once; report [`DiscussionMessage::Picked`].
    PickFiles {
        /// The composer they go to.
        compose: Compose,
    },
    /// Send the file at `path` as an attachment, reporting [`DiscussionMessage::Upload`] as it goes and when it is
    /// done; one over the server's limit is refused before it is sent.
    Upload {
        /// The composer that holds it.
        compose: Compose,
        /// The upload, as the composer knows it.
        upload: Upload,
        /// The file on this device.
        path: std::path::PathBuf,
        /// The name it is sent with.
        name: proto::files::AttachmentName,
    },
    /// Stop an upload under way; its report is never sent.
    CancelUpload {
        /// The upload.
        upload: Upload,
    },
    /// Ask where to save a file, offering `name`, and fetch it there; report [`DiscussionMessage::Saved`].
    SaveFile {
        /// The file.
        file: FileId,
        /// The name it was sent with.
        name: proto::files::AttachmentName,
    },
    /// Fetch a picture a message or the emoji show (a thumbnail, an emoji) and hold it to paint; report
    /// [`ServerMessage::FileReady`], or [`ServerMessage::FileUnfetched`].
    FetchFile {
        /// The file.
        file: FileId,
        /// What kind of file it is, which bounds what is read of it.
        kind: proto::files::FileKind,
    },
    /// Open a web link in the system's browser; report [`DiscussionMessage::NotOpened`] when it could not be.
    OpenLink(String),
    /// Open the system's picker for an emoji's picture; report [`crate::EmojiPageMessage::Picked`].
    PickEmoji,
    /// Send these bytes as an emoji's picture; report [`crate::EmojiPageMessage::Uploaded`].
    UploadEmoji {
        /// The picture as picked, within the most an emoji's may be.
        bytes: Vec<u8>,
    },
    /// Ask the server, signed in again, for what it told since this cursor; nothing is reported, since what was
    /// missed comes as events.
    Resume(terms::Cursor),
    /// Join the channel's voice room and run its call until its loop returns; report [`CallReturn::told`].
    Join {
        /// The channel's id at the server.
        channel_id: ChannelId,
        /// How loud each member is played from the start.
        volumes: Volumes,
    },
    /// Keep how loud a member's sound plays here, with the server's address, for the next run.
    KeepVolume {
        /// The address the server is kept under.
        server: ServerUrl,
        /// The member's id at the server.
        user_id: MemberId,
        /// Which of their sounds.
        audio: Audio,
        /// How loud it plays.
        volume: Volume,
    },
    /// Leave the call running for this server; its loop's return reports the end.
    Leave,
    /// Carry out what the person asked of the running call: a state is what holds from then on, an act is done once,
    /// and neither is dropped. It reports nothing, the room says what came of it.
    Call(Control),
    /// Open the desktop's picker for a screen to share; report [`CallMessage::Starting`] and begin it, sent as `quality`
    /// says, when one is picked.
    ChooseScreen(ShareQuality),
    /// List the screens and applications for our own picker, with a still of each, and again every few seconds
    /// ([`CallMessage::Sources`]), until [`Ask::StopSources`].
    WatchSources,
    /// Stop the listing [`Ask::WatchSources`] began.
    StopSources,
    /// Share the source our own picker selected, sent as `quality` says; report [`CallMessage::Starting`] as it begins, or
    /// a notice when it is gone.
    ShareSource {
        /// The source as the host listed it.
        key: SourceKey,
        /// How the share is sent.
        quality: ShareQuality,
    },
    /// Wait, then report [`SessionMessage::Reconnect`] with `attempt`.
    Wait {
        /// The try the wait is for, counted from 1; a report for a try since overtaken is dropped.
        attempt: u32,
        /// How long to wait, drawn from the backoff.
        delay: Duration,
    },
    /// End this device's account at the server: end the call here, end the session first when `sign_out` (over the
    /// open connection, else over one made for it with the kept token, within a bound), forget the token, and leave
    /// the server's row as [`Row::after`] says; a token the keyring kept is reported to the window
    /// ([`crate::WindowMessage::TokenLeft`]). [`Row::Forgotten`] also drops everything kept for the server.
    EndAccount {
        /// The server whose account ends here.
        server: Server,
        /// The username the token is kept under; none when no account was known here, so no token is forgotten.
        identity: Option<String>,
        /// End the session at the server first, because one may live there: open here, or kept as a token.
        sign_out: bool,
        /// What the server's row becomes, before [`Row::after`] weighs the token.
        row: Row,
    },
    /// Keep `pin` as the key trusted for the server, in place of the one kept; report [`SessionMessage::KeyTrusted`].
    TrustKey {
        /// The address the server is kept under.
        server: ServerUrl,
        /// The key it presented.
        pin: proto::pin::KeyPin,
    },
    /// Ask the server for its debug report; report [`PageMessage::ServerReported`].
    GetServerReport,
    /// Ask where to save a debug report and write exactly this text there; report
    /// [`ServerAboutMessage::Report`](crate::ServerAboutMessage::Report) with [`ReportMessage::Saved`](crate::ReportMessage::Saved).
    SaveReport(crate::Download),
    /// Hold a picture (the server's, or a person's) from this device's cache; report [`ServerMessage::PictureReady`],
    /// or [`ServerMessage::PictureMissing`] when the cache has none.
    LookUpPicture {
        /// The picture, by its file id.
        picture: FileId,
    },
    /// Fetch a picture (the server's, or a person's) from the server, keep it in the cache and hold it; report
    /// [`ServerMessage::PictureReady`], or [`ServerMessage::PictureUnfetched`].
    FetchPicture {
        /// The picture, by its file id.
        picture: FileId,
        /// Which kind of picture it is, which bounds what is read of it.
        kind: proto::files::FileKind,
    },
    /// Open the system's picker for a picture; report [`PageMessage::Picture`] with what was picked: the row of
    /// whichever page is open takes it.
    PickPicture,
    /// Check these bytes as the server would for `kind`, send them, and make what the server kept ready to paint;
    /// report [`PageMessage::Picture`] with the file's id or the refusal.
    UploadPicture {
        /// The picture as picked, unchecked.
        bytes: Vec<u8>,
        /// Whose picture it is: the server's or this account's.
        kind: proto::files::FileKind,
    },
    /// Keep what the server says of itself beside its address, for the next run.
    RememberServer {
        /// The address the server is kept under.
        server: ServerUrl,
        /// Its name, colour and picture, as it last said.
        identity: ServerIdentity,
    },
    /// List the members; report [`PageMessage::MembersListed`].
    ListMembers,
    /// Keep which categories are folded at the server, for the next run.
    KeepFolded {
        /// The address the server is kept under.
        server: ServerUrl,
        /// Every category folded there now.
        folded: std::collections::BTreeSet<CategoryId>,
    },
}

/// What a person asks of the call they are in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// The pictures sent in the call that the window shows nowhere now, by their senders' ids and kinds.
    Unseen(Vec<(MemberId, Video)>),
    /// Pause a member's picture, watched already or not yet: nothing of it arrives until it is resumed, whatever the
    /// window shows.
    Pause {
        /// The sender's id at the server.
        user_id: MemberId,
        /// Which of their pictures.
        video: Video,
    },
    /// Start a paused picture again.
    Resume {
        /// The sender's id at the server.
        user_id: MemberId,
        /// Which of their pictures.
        video: Video,
    },
    /// Turn on this account's camera as asked, or turn it off (none).
    Camera(Option<CameraAsk>),
    /// Mute this account's microphone, else unmute it.
    Mute(bool),
    /// Deafen this account, else hear again.
    Deafen(bool),
    /// Use this microphone; none for the system's default.
    Microphone(Option<choices::Picked>),
    /// Use this speaker; none for the system's default.
    Speaker(Option<choices::Picked>),
    /// Process the microphone so.
    Processing(choices::VoiceProcessing),
    /// Send this application's sound with the own window share.
    ShareSoundOf(crate::SoundApp),
    /// Stop sending this account's screen, or the one being begun.
    StopShare,
    /// Play a member's sound at a volume, on this device only.
    Volume {
        /// The member's id at the server.
        user_id: MemberId,
        /// Which of their sounds.
        audio: Audio,
        /// How loud it plays.
        volume: Volume,
    },
}

/// What a server's change means to the window as a whole: a toast for what the person cannot see happen, or a sound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Told {
    /// A moment of the call to mark with a sound.
    Chime(Chime),
    /// A change made on the account page, while signed in: the other devices are signed out.
    PasswordChanged,
    /// Someone else gave this account a role above its highest, or moved one of its roles up.
    Raised,
    /// Something that happened to the person, or went wrong where they cannot see it: said in a toast.
    Toast(crate::tell::Telling),
    /// An emoji picked in the picker and used, which this device then keeps as picked lately.
    Picked(chat::ReactionEmoji),
    /// Nothing the window draws changed: the call's levels, heard ten times a second, moved no tile.
    Unseen,
}

/// Who sees or writes in a channel made with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Access {
    /// Seen only by who is let in, and the owner: the default role is denied seeing it.
    Private,
    /// Read by everyone who sees it, written in only by who is let in, and the owner.
    ReadOnly,
}

impl Access {
    /// Who sees or writes in a channel made with it, as the wire names it.
    pub(crate) const fn wire(self) -> proto::chatkord_v1::ChannelAccess {
        match self {
            Access::Private => proto::chatkord_v1::ChannelAccess::Private,
            Access::ReadOnly => proto::chatkord_v1::ChannelAccess::ReadOnly,
        }
    }

    /// The word the default role is denied, and who is let in is allowed.
    pub const fn word(self) -> Grant {
        match self {
            Access::Private => Grant::ChannelRead,
            Access::ReadOnly => Grant::SendMessage,
        }
    }
}

/// Whose choice a channel's overwrite is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Whom {
    /// A role, by its id.
    Role(RoleId),
    /// A member, by their id.
    Member(MemberId),
}

/// What an administrator does to someone in a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Moderation {
    /// Mute them for everyone, else lift it.
    Mute(bool),
    /// Take them out of their call.
    TakeOut,
}
