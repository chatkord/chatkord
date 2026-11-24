//! What is done in a discussion, and what is reported for it.

use super::*;

/// What a person does in a text channel, and what the host reports about it.
#[derive(Clone, Debug)]
pub enum DiscussionMessage {
    /// A text channel picked in the list, by its id.
    Open(ChannelId),
    /// A page of history read at an end, or the first (`end` none), or why not.
    Page {
        /// The timeline it is of.
        timeline: TimelineId,
        /// The end it extends; none for the first page.
        end: Option<End>,
        /// The page.
        page: Result<chat::Page, LinkError>,
    },
    /// A message the server told, as it is now.
    Told {
        /// The timeline it is in.
        timeline: TimelineId,
        /// The message.
        message: Box<chat::Message>,
    },
    /// A writer's messages the server deleted at once.
    Withdrawn {
        /// The timeline they are in.
        timeline: TimelineId,
        /// Which, and at what revision.
        withdrawn: chat::Withdrawn,
    },
    /// What changed since the revision the timeline had read, or why not.
    CaughtUp {
        /// The timeline.
        timeline: TimelineId,
        /// The changes, whether to read the channel again instead, and its revision now.
        changes: Result<Caught, LinkError>,
    },
    /// The list was scrolled to `from_end` pixels above its end, in a viewport `viewport` tall.
    Scrolled {
        /// From the newest message.
        from_end: f32,
        /// The viewport's height.
        viewport: f32,
    },
    /// What stands above the rows, the channel's start or the line saying older messages are read, was laid out at
    /// this height.
    HeadMeasured {
        /// Its height in pixels.
        height: f32,
    },
    /// A row was laid out at this height.
    Measured {
        /// The row.
        row: RowKey,
        /// Its height in pixels.
        height: f32,
    },
    /// "Jump to present": the newest page read again.
    JumpToPresent,
    /// The first page asked for again after it failed.
    Retry,
    /// The composer's text changed.
    Draft(String),
    /// Send what the composer holds.
    Send,
    /// A send was answered, or why not.
    Sent {
        /// The timeline it went to.
        timeline: TimelineId,
        /// The root of the thread it began, as it was asked.
        thread_of: Option<MessageId>,
        /// The send it answers.
        nonce: terms::Nonce,
        /// The message as stored.
        sent: Result<Box<chat::Message>, LinkError>,
    },
    /// The rows that hold a selection now, as the list found them when the pointer let go.
    Selected(Vec<RowKey>),
    /// A message's menu opened, on a right press or from the keyboard.
    Menu(MessageId),
    /// The menu put away.
    CloseMenu,
    /// Copy a message's text.
    Copy(MessageId),
    /// Edit one's own message in place.
    Edit(MessageId),
    /// The edited text changed.
    EditDraft(String),
    /// Save the edit.
    SaveEdit,
    /// Leave the message as it was.
    CancelEdit,
    /// An edit or a delete was answered with the message as it is now, or why not.
    Changed(Result<Box<chat::Message>, LinkError>),
    /// Delete on a message: the window asks first, for one's own message or, with the word, another's.
    Delete(MessageId),
    /// The window's confirmation of a delete: sent while the message is still one this account may delete.
    DeleteConfirmed(MessageId),
    /// The composer's cursor moved, to this many scalars in.
    Caret(usize),
    /// Down in an open chooser.
    ChooseNext,
    /// Up in an open chooser.
    ChoosePrevious,
    /// A choice of the open chooser pressed, by its place; none for the selected one, by Enter or Tab.
    Choose(Option<usize>),
    /// Escape in the composer: the chooser goes, else the reply.
    Escape,
    /// Reply to a message, from its hover bar or menu.
    Reply(MessageId),
    /// The reply bar's "@ On" pressed: the reply mentions its author, or no longer.
    MentionAuthor,
    /// The reply bar's ✕.
    CancelReply,
    /// The composer's attach button: the system's picker opens.
    Attach,
    /// What the picker gave for a composer, or that this system has none.
    Picked {
        /// The composer it was for.
        compose: Compose,
        /// The files picked; none when the picker was closed.
        picked: Result<Vec<std::path::PathBuf>, NoPicker>,
    },
    /// Files dropped on the open channel.
    Dropped(Vec<std::path::PathBuf>),
    /// What an upload said of itself.
    Upload {
        /// The composer that holds it.
        compose: Compose,
        /// The upload.
        upload: Upload,
        /// How far it is, or the server's file, or why it was refused.
        said: Result<Uploaded, terms::FileError>,
    },
    /// A file's Cancel or Remove in the strip.
    RemoveFile(Upload),
    /// An emoji picked for a message in the picker: this account reacts with it.
    React(MessageId, chat::ReactionEmoji),
    /// A reaction's chip pressed: this account reacts with it, or takes its reaction back.
    Reaction(MessageId, chat::ReactionEmoji),
    /// The server's answer to a reaction.
    Reacted(Result<(), LinkError>),
    /// This account's own reactions on a message, as the server tells them to its sockets.
    ReactionsOfYours {
        /// The timeline.
        timeline: TimelineId,
        /// The message.
        message: MessageId,
        /// The message's revision they are of.
        revision: u64,
        /// The emoji, whole.
        mine: Vec<chat::ReactionEmoji>,
    },
    /// Pin a message, from its menu.
    Pin(MessageId),
    /// Unpin a message, from its menu or the pins panel.
    Unpin(MessageId),
    /// The server's answer to a pin or an unpin.
    Pinned(Result<(), LinkError>),
    /// "Pinned messages" in the header: the panel opens, or closes.
    TogglePins,
    /// The panel's list came near its end: the next page is read.
    MorePins,
    /// A page of pins, or why not.
    Pins {
        /// The timeline.
        timeline: TimelineId,
        /// The page.
        page: Result<chat::Page, LinkError>,
    },
    /// What the host made of a draft's first link, or none when it could not.
    Previewed {
        /// The composer that holds the draft.
        compose: Compose,
        /// The link it was made of.
        link: String,
        /// The preview.
        made: Option<PreviewMade>,
    },
    /// The composer's card for a link: taken out.
    RemovePreview,
    /// The message a reply quotes, read alone, or why not.
    Quoted {
        /// The timeline.
        timeline: TimelineId,
        /// The quoted message's place.
        place: u64,
        /// A page of at most it.
        page: Result<chat::Page, LinkError>,
    },
    /// A reply's quoted line or a pin's Jump: the list goes to the message at this place.
    Jump(u64),
    /// A spoiler pressed: shown.
    Reveal(MessageId, chat::SpoilerId),
    /// A picture pressed: shown over the window at its size.
    View {
        /// The message it was sent with.
        message: MessageId,
        /// The picture.
        file: FileId,
    },
    /// The viewer's Close, or Escape.
    CloseViewer,
    /// A file's Save: the person is asked where.
    Save {
        /// The file.
        file: FileId,
        /// The name it was sent with.
        name: proto::files::AttachmentName,
    },
    /// What came of a save.
    Saved {
        /// The file.
        file: FileId,
        /// The name it was saved as.
        name: proto::files::AttachmentName,
        /// Where it went, or why not.
        saved: Result<SaveAnswer, terms::FileError>,
    },
    /// A web link could not be opened.
    NotOpened,
    /// In the emoji picker.
    Picker(PickerMessage),
    /// Who is writing in a timeline, as the server tells it whole.
    Typing {
        /// The timeline.
        timeline: TimelineId,
        /// Who, by id.
        users: Vec<MemberId>,
    },
}

/// A catch-up's answer, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Caught {
    /// The messages changed, in revision order.
    pub changed: Vec<chat::Message>,
    /// Too much changed: the timeline is read again.
    pub reload: bool,
    /// The timeline's revision now.
    pub revision: u64,
}

impl From<&proto::chatkord_v1::Changes> for Caught {
    fn from(changes: &proto::chatkord_v1::Changes) -> Caught {
        Caught {
            changed: changes.changed.iter().filter_map(|message| chat::Message::try_from(message).ok()).collect(),
            reload: changes.reload,
            revision: changes.revision,
        }
    }
}
