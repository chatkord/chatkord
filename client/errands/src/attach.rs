//! A file attached to a message, sent from where the person picked it with its progress told as it goes, and one saved
//! where the person chooses, fetched straight to the disk.

use link::Link;
use proto::files::{AttachmentName, FileId, FileKind, FileRefusal};
use std::future::Future;
use std::path::Path;
use std::time::Duration;
use terms::{FileError, LinkError};
use tokio::sync::watch;

/// The least time between two reports of an upload's progress, so a fast one does not flood the window.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Sends the file at `path` as an attachment named `name`, refused here first when it is over the server's own limit;
/// `progress` is told the bytes sent and the file's size as it goes. The server's id.
pub(crate) async fn upload<F, Told>(
    link: Option<Link>,
    name: &AttachmentName,
    path: &Path,
    progress: F,
) -> Result<FileId, FileError>
where
    F: Fn(u64, u64) -> Told,
    Told: Future<Output = ()>,
{
    let link = link.ok_or(FileError::Reach(LinkError::Offline))?;
    let opened = path.to_path_buf();
    let size = crate::errand::blocking(report::word!("measure a file"), move || {
        std::fs::metadata(opened).map(|meta| meta.len())
    })
    .await
    .map_err(|_| FileError::Disk(std::io::ErrorKind::Other))?
    .map_err(|failed| FileError::Disk(failed.kind()))?;
    if link.file_limits().attachment.is_some_and(|most| size > most) {
        return Err(FileError::Refused { kind: FileKind::Attachment, refusal: FileRefusal::TooLarge });
    }
    progress(0, size).await;
    let (sent, mut watched) = watch::channel(0);
    let sending = link.upload_attachment(name, path, sent);
    tokio::pin!(sending);
    let mut told = tokio::time::Instant::now();
    loop {
        tokio::select! {
            done = &mut sending => return done,
            changed = watched.changed() => {
                if changed.is_err() {
                    return sending.await;
                }
                let so_far = *watched.borrow_and_update();
                if told.elapsed() >= PROGRESS_EVERY {
                    told = tokio::time::Instant::now();
                    progress(so_far, size).await;
                }
            }
        }
    }
}

/// The file at `path` read as a picture this device shows, if its first bytes say it is one; on the blocking pool.
pub(crate) async fn picture_of(path: std::path::PathBuf) -> Option<portrait::Decoded> {
    crate::errand::blocking(report::word!("read a picture"), move || {
        use std::io::Read as _;
        let mut head = [0u8; 16];
        let mut file = std::fs::File::open(&path).ok()?;
        let read = file.read(&mut head).ok()?;
        proto::files::PictureType::sniff(head.get(..read)?)?;
        let bytes = std::fs::read(&path).ok()?;
        portrait::shown(&bytes, FileKind::Attachment).ok()
    })
    .await
    .ok()
    .flatten()
}

/// Asks where to save file `id`, offering `name`, and fetches it there.
pub(crate) async fn save(
    link: Option<Link>,
    id: FileId,
    name: &AttachmentName,
) -> Result<screens::SaveAnswer, FileError> {
    let link = link.ok_or(FileError::Reach(LinkError::Offline))?;
    let Ok(chosen) = platform::save::file(name.as_str()).await else { return Ok(screens::SaveAnswer::NoDialog) };
    let Some(path) = chosen else { return Ok(screens::SaveAnswer::Dismissed) };
    // Where the person saves is known only now, so the dead runs' unfinished downloads there go before this one starts.
    if let Some(folder) = path.parent().map(std::path::Path::to_path_buf) {
        let _ = crate::errand::blocking(report::word!("save a file"), move || {
            let (running, ever) = (platform::partial::running, std::time::SystemTime::UNIX_EPOCH);
            platform::partial::sweep(&folder, link::files::PARTIAL, running, ever)
        })
        .await;
    }
    link.fetch_to(FileKind::Attachment, id, &path).await.map(|()| screens::SaveAnswer::Saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest PNG there is: one pixel.
    const ONE_PIXEL: [u8; 70] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
        0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x64, 0xF8, 0xCF, 0x50, 0x0F, 0x00, 0x03, 0x86, 0x01, 0x80,
        0x5A, 0x34, 0x7D, 0x6B, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[tokio::test]
    async fn a_picture_on_disk_is_read_for_its_tile_and_any_other_file_is_not() {
        let dir = std::env::temp_dir().join(format!("chatkord-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (picture, other) = (dir.join("one.png"), dir.join("notes.txt"));
        std::fs::write(&picture, ONE_PIXEL).unwrap();
        std::fs::write(&other, b"not a picture at all").unwrap();
        let read = picture_of(picture).await.map(|decoded| (decoded.width, decoded.height));
        assert_eq!(read, Some((1, 1)));
        assert!(picture_of(other).await.is_none());
        assert!(picture_of(dir.join("gone.png")).await.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
