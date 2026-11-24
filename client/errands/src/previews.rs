//! A link's preview made on this device: the page read with the bounded, vetted fetch, its picture sent to the server
//! as the sender's own unsent file the way an attachment is, so the message that carries the preview can name it.

use crate::host::Host;
use crate::pictures::Holding;
use fetch::{Internet, MediaType, Preview};
use link::Link;
use proto::chatkord_v1::LinkPreview;
use proto::files::{AttachmentName, FileId, FileKind};
use screens::{PreviewMade, PreviewPicture};
use std::time::Duration;

/// The longest a preview takes, page and picture together.
const WITHIN: Duration = Duration::from_secs(10);

/// The preview of `url`, read by this device; its picture sent too and held to paint when `picture` and the server
/// takes files. None when the page gives no title or any step failed: a failed preview is silent.
pub(crate) async fn make(
    link: Option<Link>,
    trust: &link::Trust,
    url: String,
    picture: bool,
    holding: Holding<impl Host>,
) -> Option<PreviewMade> {
    let net = Internet::new(trust);
    let page = match fetch::preview(&net, &url, WITHIN).await {
        Ok(page) => page,
        Err(why) => {
            report::note!(debug, "a link has no preview", why = why);
            return None;
        }
    };
    let Preview { url, title, description, picture: found } = page;
    let sent = match (picture, found, link) {
        (true, Some(found), Some(link)) => upload(&link, found, &holding).await,
        _ => None,
    };
    let made = PreviewMade {
        url,
        title,
        description: description.unwrap_or_default(),
        picture: sent.map(PreviewPicture::Uploaded),
    };
    made.shaped().then_some(made)
}

/// The picture sent as an attachment, after it is read as the window will paint it; none when it is not one this
/// device draws, or the server refused it.
async fn upload(link: &Link, found: fetch::Picture, holding: &Holding<impl Host>) -> Option<FileId> {
    let (extension, bytes) = (extension(found.media_type), found.bytes);
    let shown = {
        let bytes = bytes.clone();
        crate::errand::blocking(report::word!("show a preview"), move || portrait::shown(&bytes, FileKind::Attachment))
            .await
            .ok()?
            .ok()?
    };
    let name = AttachmentName::parse(&format!("preview.{extension}")).ok()?;
    let file = link.upload_attachment_bytes(&name, bytes).await.ok()?;
    holding.file(file, shown).await;
    Some(file)
}

fn extension(media: MediaType) -> &'static str {
    match media {
        MediaType::Png => "png",
        MediaType::Jpeg => "jpg",
        MediaType::Webp => "webp",
    }
}

/// `made` as the wire carries it for a send or an edit: a picture already sent is fetched and sent again as a new
/// file, as the message that held it releases it on an edit.
pub(crate) async fn wire(link: &Link, made: PreviewMade) -> LinkPreview {
    let file = match made.picture {
        None => None,
        Some(PreviewPicture::Uploaded(file)) => Some(file),
        Some(PreviewPicture::Sent(file)) => again(link, file).await,
    };
    made.wire(file)
}

/// The sent picture `file`, sent again as an attachment of its own; none when it could not be.
async fn again(link: &Link, file: FileId) -> Option<FileId> {
    let bytes = link.fetch(FileKind::Attachment, file).await.ok()?;
    let name = AttachmentName::parse("preview.png").ok()?;
    link.upload_attachment_bytes(&name, bytes).await.ok()
}
