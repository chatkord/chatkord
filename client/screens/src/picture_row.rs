//! A picture row, a server's on Overview and in the set-up dialog and this account's on its page: Choose a picture, or
//! Change picture and Remove once one is held; Uploading… while one goes up; the refusal in one sentence under the
//! row. What it holds is the server's or the account's picture, or one sent and waiting for the dialog's Save.

use proto::files::{FileId, FileKind};
use terms::FileError;
use terms::LinkError;

/// The row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureRow {
    held: Option<FileId>,
    kind: FileKind,
    doing: Doing,
    refused: Option<Refused>,
}

/// What the row is waiting on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Doing {
    /// Nothing: the row's buttons can be pressed.
    #[default]
    Nothing,
    /// The system's picker is open.
    Picking,
    /// A picture is going up.
    Uploading,
}

/// Why the last picture was not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The picture could not be read, checked or sent.
    File(FileError),
    /// The server refused to set the picture the row held.
    Link(LinkError),
}

impl Refused {
    /// What a person reads of it.
    pub fn sentence(&self) -> words::Sentence {
        match self {
            Self::File(failed) => words::failure::file(failed),
            Self::Link(failed) => words::failure::said(failed),
        }
    }
}

/// What the system's picker came to, as the host read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picked {
    /// The picker was closed with nothing picked.
    Nothing,
    /// The bytes of the file picked, as read from disk.
    File(Vec<u8>),
    /// The picker could not be opened, or what was picked could not be read.
    Unreadable,
}

/// What a person does on the row, and what the host answers.
#[derive(Clone, Debug)]
pub enum Message {
    /// Choose a picture, or Change picture, on the row.
    Choose,
    /// Remove on the row.
    Remove,
    /// The system's picker closed.
    Picked(Picked),
    /// The upload ended: the server's id for the picture, or why it was not taken.
    Uploaded(Result<FileId, FileError>),
}

/// What the row asks of the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Open the system's picker.
    Pick,
    /// Check and send these bytes, and fetch back what the server made of them.
    Upload(Vec<u8>),
    /// The row now holds this picture, or none.
    Hold(Option<FileId>),
}

impl PictureRow {
    /// A row of a server's picture, holding `held`.
    pub(crate) fn new(held: Option<FileId>) -> PictureRow {
        PictureRow { held, kind: FileKind::ServerPicture, doing: Doing::Nothing, refused: None }
    }

    /// A row of this account's own picture, holding `held`.
    pub(crate) fn profile(held: Option<FileId>) -> PictureRow {
        PictureRow { held, kind: FileKind::ProfilePicture, doing: Doing::Nothing, refused: None }
    }

    /// Whose picture the row is: what an upload from it is checked as.
    pub fn kind(&self) -> FileKind {
        self.kind
    }

    /// The picture the row holds; none when it holds none.
    pub fn held(&self) -> Option<FileId> {
        self.held
    }

    /// What the row is waiting on; while it waits, a press does nothing.
    pub fn doing(&self) -> Doing {
        self.doing
    }

    /// Why the last picture was not taken, until the next is picked or the picture removed.
    pub fn refused(&self) -> Option<&Refused> {
        self.refused.as_ref()
    }

    pub(crate) fn update(&mut self, message: Message) -> Option<Ask> {
        match (self.doing, message) {
            (Doing::Nothing, Message::Choose) => {
                self.doing = Doing::Picking;
                Some(Ask::Pick)
            }
            (Doing::Nothing, Message::Remove) => {
                self.refused = None;
                self.held = None;
                Some(Ask::Hold(None))
            }
            (Doing::Picking, Message::Picked(Picked::Nothing)) => {
                self.doing = Doing::Nothing;
                None
            }
            (Doing::Picking, Message::Picked(Picked::Unreadable)) => {
                self.doing = Doing::Nothing;
                self.refused = Some(Refused::File(FileError::Unreadable { kind: self.kind }));
                None
            }
            (Doing::Picking, Message::Picked(Picked::File(bytes))) => {
                self.doing = Doing::Uploading;
                self.refused = None;
                Some(Ask::Upload(bytes))
            }
            (Doing::Uploading, Message::Uploaded(Ok(id))) => {
                self.doing = Doing::Nothing;
                self.held = Some(id);
                Some(Ask::Hold(Some(id)))
            }
            (Doing::Uploading, Message::Uploaded(Err(failed))) => {
                self.doing = Doing::Nothing;
                self.refused = Some(Refused::File(failed));
                None
            }
            // A press while waiting, or an answer to nothing asked: nothing.
            (_, Message::Choose | Message::Remove | Message::Picked(_) | Message::Uploaded(_)) => None,
        }
    }

    /// The server refused to set what the row held: the row says so and shows the server's again.
    pub(crate) fn not_set(&mut self, server: Option<FileId>, failed: LinkError) {
        self.held = server;
        self.refused = Some(Refused::Link(failed));
    }

    /// The server's picture changed, here or on another device; a row that is busy keeps what it is doing.
    pub(crate) fn follow(&mut self, server: Option<FileId>) {
        if self.doing == Doing::Nothing {
            self.held = server;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::files::FileRefusal;

    const OWL: FileId = FileId::from_bytes([7; FileId::LEN]);

    #[test]
    fn a_picture_is_picked_sent_and_held() {
        let mut row = PictureRow::new(None);
        assert_eq!(row.update(Message::Choose), Some(Ask::Pick));
        assert_eq!(row.update(Message::Choose), None, "one picker at a time");
        assert_eq!(row.update(Message::Picked(Picked::File(b"png".to_vec()))), Some(Ask::Upload(b"png".to_vec())));
        assert_eq!(row.doing(), Doing::Uploading);
        assert_eq!(row.update(Message::Remove), None, "nothing is removed while one goes up");
        assert_eq!(row.update(Message::Uploaded(Ok(OWL))), Some(Ask::Hold(Some(OWL))));
        assert_eq!((row.held(), row.doing()), (Some(OWL), Doing::Nothing));
        assert_eq!(row.update(Message::Remove), Some(Ask::Hold(None)));
        assert_eq!(row.held(), None);
    }

    #[test]
    fn a_closed_picker_asks_nothing_and_a_refusal_is_said_until_the_next_try() {
        let mut row = PictureRow::new(Some(OWL));
        row.update(Message::Choose);
        assert_eq!(row.update(Message::Picked(Picked::Nothing)), None);
        assert_eq!((row.held(), row.doing(), row.refused()), (Some(OWL), Doing::Nothing, None));
        row.update(Message::Choose);
        row.update(Message::Picked(Picked::File(Vec::new())));
        let big = FileError::Refused { kind: FileKind::ServerPicture, refusal: FileRefusal::TooLarge };
        assert_eq!(row.update(Message::Uploaded(Err(big.clone()))), None);
        assert_eq!(
            row.refused().map(|refused| refused.sentence().in_english()),
            Some("That picture is over 2 MB. Choose a smaller one.".into())
        );
        assert_eq!(row.held(), Some(OWL), "the picture held stays");
        row.update(Message::Choose);
        row.update(Message::Picked(Picked::File(Vec::new())));
        assert_eq!(row.refused(), None, "a new try clears it");
    }

    #[test]
    fn what_the_picker_could_not_read_is_said() {
        let mut row = PictureRow::new(None);
        row.update(Message::Choose);
        row.update(Message::Picked(Picked::Unreadable));
        assert_eq!(
            row.refused().map(|refused| refused.sentence().in_english()),
            Some("That picture can’t be read. Choose another one.".into())
        );
    }

    #[test]
    fn a_change_from_elsewhere_moves_a_row_that_is_not_busy() {
        let mut row = PictureRow::new(None);
        row.follow(Some(OWL));
        assert_eq!(row.held(), Some(OWL));
        row.update(Message::Choose);
        row.follow(None);
        assert_eq!(row.held(), Some(OWL), "a row picking keeps what it shows");
    }
}
