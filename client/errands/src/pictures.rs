//! A picture of a server or of a member: sent from a file picked, or fetched, and kept in this device's cache; and
//! where a place's pictures are held.

use crate::host::Host;
use crate::mailbox::Mailbox;
use link::Link;
use proto::files::{FileId, FileKind};
use screens::{PlaceId, PlaceMessage, ServerMessage, WindowMessage};
use std::sync::Arc;

/// Where a place's pictures are held, and who is told of each the host lets go to make room.
pub(crate) struct Holding<H> {
    /// The host that holds them.
    pub(crate) host: Arc<H>,
    /// The place they are held for.
    pub(crate) place: PlaceId,
    /// Who is told.
    pub(crate) mailbox: Mailbox,
}

impl<H: Host> Holding<H> {
    /// Holds `square` to paint wherever `picture` is shown.
    pub(crate) async fn picture(&self, picture: FileId, square: portrait::Square) {
        let gone = self.host.hold_picture(self.place, picture, square);
        self.let_go(gone).await;
    }

    /// Holds a fetched file's `picture` to paint wherever `file` is shown.
    pub(crate) async fn file(&self, file: FileId, picture: portrait::Decoded) {
        let gone = self.host.hold_file(self.place, file, picture);
        self.let_go(gone).await;
    }

    async fn let_go(&self, gone: Vec<(PlaceId, FileId)>) {
        for (place, file) in gone {
            self.mailbox.tell(WindowMessage::At(place, PlaceMessage::Server(ServerMessage::LetGo(file)))).await;
        }
    }
}

/// A picked picture checked as the server checks one of `kind` (on the blocking pool), sent, and what the server kept
/// of it fetched back, kept in the cache and held to paint; its file's id.
pub(crate) async fn upload_picture(
    platform: &platform::Platform,
    link: Option<Link>,
    kind: FileKind,
    bytes: Vec<u8>,
    holding: Holding<impl Host>,
) -> Result<FileId, terms::FileError> {
    use terms::FileError;
    let link = link.ok_or(FileError::Reach(terms::LinkError::Offline))?;
    let checked = crate::errand::blocking(report::word!("check a picture"), move || {
        portrait::before_upload(&bytes, kind).map(|()| bytes)
    })
    .await;
    let bytes = checked.map_err(|_| FileError::Unreadable { kind })??;
    let picture = link.upload(kind, bytes).await?;
    let kept = link.fetch(kind, picture).await?;
    let square =
        platform::cache::keep_picture(platform, picture, kept).await.map_err(|_| FileError::Unreadable { kind })?;
    holding.picture(picture, square).await;
    Ok(picture)
}

/// A picture of `kind` fetched from the server and kept in the cache; the square to paint.
pub(crate) async fn fetch_picture(
    platform: &platform::Platform,
    link: Option<Link>,
    kind: FileKind,
    picture: FileId,
) -> Result<portrait::Square, terms::FileError> {
    use terms::FileError;
    let link = link.ok_or(FileError::Reach(terms::LinkError::Offline))?;
    let bytes = link.fetch(kind, picture).await?;
    platform::cache::keep_picture(platform, picture, bytes).await.map_err(|_| FileError::Unreadable { kind })
}
