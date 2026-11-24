//! A link's preview, made by the sender's own client: the page fetched with a policy of its own (`https` on port
//! 443 only, every address vetted, small bounds, a deadline), scanned for its title, description and picture, and
//! the picture fetched the same way.

use crate::get::{Answer, GetError, Patience, Route, Stream, get};
use crate::guard::vet;
use crate::net::Reach;
use crate::scan::{self, Flow, Scan, ScanRefused};
use crate::url::{Host, PORT, Url};
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

/// The most bytes read of a page in all; what is past them is not. The page is scanned as it arrives and not kept:
/// memory stays the scan's held tag and the values it found.
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;

/// The most a picture may be, by its stated length or as it arrives.
pub const MAX_PICTURE_BYTES: usize = 2 * 1024 * 1024;

/// The most redirects a preview follows, a page's or a picture's.
pub const MAX_REDIRECTS: usize = 3;

/// The request's `User-Agent`: generic, with no version, system or owner.
const USER_AGENT: &str = "Mozilla/5.0 (compatible; Chatkord link preview)";

/// How long one step waits, inside the total deadline.
const PATIENCE: Patience = Patience { connect: Duration::from_secs(5), stall: Duration::from_secs(5) };

/// What a link shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The address asked for, as given.
    pub url: String,
    /// The page's title, within [`scan::MAX_TITLE`] scalar values.
    pub title: String,
    /// The page's description, within [`scan::MAX_DESCRIPTION`] scalar values, when it has one.
    pub description: Option<String>,
    /// The page's picture, when it has one that was fetched.
    pub picture: Option<Picture>,
}

/// A page's picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// The picture's bytes, as the host sent them, within [`MAX_PICTURE_BYTES`].
    pub bytes: Vec<u8>,
    /// What the host said it is.
    pub media_type: MediaType,
}

/// The picture types a preview takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    /// `image/png`.
    Png,
    /// `image/jpeg`.
    Jpeg,
    /// `image/webp`.
    Webp,
}

impl MediaType {
    /// The media type's name, as a wire states it.
    pub fn name(self) -> &'static str {
        match self {
            MediaType::Png => "image/png",
            MediaType::Jpeg => "image/jpeg",
            MediaType::Webp => "image/webp",
        }
    }
}

/// Why a link has no preview. A failed preview is silent: these are for a log.
#[derive(Clone, Debug, PartialEq, Eq, report::Public)]
pub enum PreviewError {
    /// The address is not `https` on port 443 with a host, or is over its bound.
    BadUrl,
    /// A name resolved to an address that is not globally routable, or to none.
    Refused,
    /// The page was not fetched.
    Fetch(GetError),
    /// The deadline passed before the page was read.
    Timeout,
    /// The answer is not `text/html`.
    NotHtml,
    /// The page is not UTF-8, or says it is another charset.
    NotUtf8,
    /// The page has no title.
    NoTitle,
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreviewError::BadUrl => f.write_str("the link is not an https address a preview fetches"),
            PreviewError::Refused => f.write_str("the link leads to an address a preview does not reach"),
            PreviewError::Fetch(failed) => write!(f, "the page was not fetched: {failed}"),
            PreviewError::Timeout => f.write_str("the page took too long"),
            PreviewError::NotHtml => f.write_str("the link is not a web page"),
            PreviewError::NotUtf8 => f.write_str("the page is not UTF-8"),
            PreviewError::NoTitle => f.write_str("the page has no title"),
        }
    }
}

impl std::error::Error for PreviewError {}

impl From<GetError> for PreviewError {
    fn from(failed: GetError) -> PreviewError {
        match failed {
            GetError::Refused => PreviewError::Refused,
            GetError::Timeout => PreviewError::Timeout,
            other => PreviewError::Fetch(other),
        }
    }
}

/// What kind of file a hop asks for, which sets its `Accept`.
#[derive(Clone, Copy)]
enum Kind {
    Page,
    Picture,
}

/// One address a preview asks, over `net`. Its connect resolves, vets and connects, so no hop is reached unchecked.
struct Hop<'a, N> {
    net: &'a N,
    url: Url,
    authority: String,
    kind: Kind,
}

impl<'a, N: Reach> Hop<'a, N> {
    fn new(net: &'a N, url: Url, kind: Kind) -> Hop<'a, N> {
        Hop { net, authority: url.authority(), url, kind }
    }
}

impl<N: Reach> Route for Hop<'_, N> {
    fn authority(&self) -> &str {
        &self.authority
    }

    fn origin_form(&self) -> &str {
        self.url.target()
    }

    fn user_agent(&self) -> String {
        USER_AGENT.to_string()
    }

    fn accept(&self) -> Option<&str> {
        Some(match self.kind {
            Kind::Page => "text/html",
            Kind::Picture => "image/webp,image/png,image/jpeg",
        })
    }

    async fn connect(&self) -> Result<Stream, GetError> {
        let found = match self.url.host_kind() {
            Host::Ip(ip) => vec![SocketAddr::new(*ip, PORT)],
            Host::Name(name) => self.net.resolve(name, PORT).await?,
        };
        let vetted = vet(found).map_err(|_| GetError::Refused)?;
        self.net.connect(&vetted, &self.url.server_name()).await
    }

    fn redirected(&self, location: &str) -> Result<Self, GetError> {
        let url = self.url.join(location).map_err(|_| GetError::BadRedirect)?;
        Ok(Hop::new(self.net, url, self.kind))
    }
}

/// The preview of `link` over `net`, its page and picture together within `within`. A picture that cannot be had
/// (refused, over its bound, not a picture, late) leaves a preview without one; a page that cannot be read or has
/// no title leaves none.
pub async fn preview<N: Reach>(net: &N, link: &str, within: Duration) -> Result<Preview, PreviewError> {
    let deadline = Instant::now() + within;
    let first = Url::parse(link).map_err(|_| PreviewError::BadUrl)?;
    let (head, at) = timeout_at(deadline, page(net, first)).await.map_err(|_| PreviewError::Timeout)??;
    let title = head.title.ok_or(PreviewError::NoTitle)?;
    let picture = match head.image.and_then(|image| at.join(&image).ok()) {
        Some(url) => timeout_at(deadline, picture(net, url)).await.ok().and_then(Result::ok),
        None => None,
    };
    Ok(Preview { url: link.to_string(), title, description: head.description, picture })
}

/// The page at `first`, scanned, and the address it was read from after its redirects.
async fn page<N: Reach>(net: &N, first: Url) -> Result<(scan::Head, Url), PreviewError> {
    let (mut answer, hop) = get(Hop::new(net, first, Kind::Page), PATIENCE, MAX_REDIRECTS).await?;
    let content_type = answer.content_type().ok_or(PreviewError::NotHtml)?;
    if media_type(content_type) != "text/html" {
        return Err(PreviewError::NotHtml);
    }
    if scan::charset_of(content_type).is_some_and(scan::not_utf8) {
        return Err(PreviewError::NotUtf8);
    }
    let mut scan = Scan::default();
    let mut read = 0;
    let cut = loop {
        let Some(part) = answer.next().await? else { break false };
        let kept = part.get(..MAX_PAGE_BYTES - read).unwrap_or(&part);
        read += kept.len();
        if scan.feed(kept) == Flow::Done {
            break false;
        }
        if read >= MAX_PAGE_BYTES {
            break true;
        }
    };
    let head = scan.finish(cut).map_err(|refused| match refused {
        ScanRefused::NotUtf8 | ScanRefused::OtherCharset => PreviewError::NotUtf8,
    })?;
    Ok((head, hop.url))
}

/// The picture at `first`: a type a preview takes, within [`MAX_PICTURE_BYTES`].
pub(crate) async fn picture<N: Reach>(net: &N, first: Url) -> Result<Picture, GetError> {
    let (answer, _) = get(Hop::new(net, first, Kind::Picture), PATIENCE, MAX_REDIRECTS).await?;
    let media_type = picture_type(&answer).ok_or(GetError::Malformed)?;
    let bytes = answer.whole(MAX_PICTURE_BYTES).await?;
    Ok(Picture { bytes, media_type })
}

fn picture_type(answer: &Answer) -> Option<MediaType> {
    match media_type(answer.content_type()?).as_str() {
        "image/png" => Some(MediaType::Png),
        "image/jpeg" => Some(MediaType::Jpeg),
        "image/webp" => Some(MediaType::Webp),
        _ => None,
    }
}

/// A `Content-Type`'s type and subtype, lower case, without its parameters.
fn media_type(content_type: &str) -> String {
    content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase()
}
