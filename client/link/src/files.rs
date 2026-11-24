//! Files over HTTP on the socket's own port: `POST /v1/files?kind=…` and `GET /v1/files/<id>`, signed in by the
//! session's token, under the same trust as the socket, on one connection a server keeps between requests and dialled
//! again once the server closes it. What comes back is bounded by its kind before it is read.

use crate::session::{Offered, reach};
use crate::trust::Trust;
use futures_util::stream;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::client::conn::http1::{self, SendRequest};
use hyper::header::{AUTHORIZATION, CONNECTION, CONTENT_LENGTH, HOST, HeaderValue};
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use proto::files::{AttachmentName, FileId, FileKind, FileRefusal, ReportId};
use report::MAX_REPORT_BYTES;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use terms::{FileError, LinkError, ReportError, Secret, ServerUrl};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, watch};
use tokio::time::{Instant, timeout, timeout_at};

/// How long a request may take to be answered, connection included: past the server's own minute for an upload's body.
const EXCHANGE: Duration = Duration::from_secs(90);

/// The slowest a transfer may go: one of `n` bytes may take [`EXCHANGE`] and a second for each of these past it.
const LEAST_RATE: u64 = 64 * 1024;

/// The most bytes read from a file, or written to one, at a time.
const CHUNK: usize = 64 * 1024;

/// The most an upload's answer may be: the id, as text.
const ID_ANSWER_BYTES: usize = 2 * FileId::LEN;

/// The most bytes a report's text is, as a download's length is counted.
const REPORT_MOST: u64 = MAX_REPORT_BYTES as u64;

/// A request's body, whole or streamed from a file.
type Body = BoxBody<Bytes, io::Error>;

/// Why a file sent from a path, or fetched into one, did not arrive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathError {
    /// The server could not be reached, refused, or did not answer as one of ours.
    File(FileError),
    /// This device's file could not be read, written or put in place.
    Disk(io::ErrorKind),
}

impl From<FileError> for PathError {
    fn from(failed: FileError) -> Self {
        Self::File(failed)
    }
}

/// One server's files, asked on one kept HTTP/1.1 connection, one request at a time.
pub(crate) struct Lane {
    url: ServerUrl,
    trust: Trust,
    kept: Mutex<Option<SendRequest<Body>>>,
    offered: watch::Receiver<Offered>,
}

impl Lane {
    /// The files of the server at `url`, reached under `trust`, which takes what `offered` says it takes now; nothing
    /// is dialled until the first request.
    pub(crate) fn following(url: ServerUrl, trust: Trust, offered: watch::Receiver<Offered>) -> Lane {
        Lane { url, trust, kept: Mutex::new(None), offered }
    }

    /// A lane whose server takes `limits` and never changes them.
    #[cfg(test)]
    pub(crate) fn new(url: ServerUrl, trust: Trust, limits: terms::FileLimits) -> Lane {
        Lane::following(url, trust, watch::channel(Offered { switched_off: Vec::new(), limits }).1)
    }

    /// The most bytes an upload of `kind` may be here: the kind's bound, and for an attachment the server's own.
    fn most(&self, kind: FileKind) -> u64 {
        let server = match kind {
            FileKind::Attachment => self.offered.borrow().limits.attachment,
            FileKind::ServerPicture | FileKind::ProfilePicture | FileKind::Thumbnail | FileKind::Emoji => None,
        };
        server.map_or(kind.upload_bytes(), |server| server.min(kind.upload_bytes()))
    }

    /// Sends `bytes` as a file of `kind` that takes no name; the server's id.
    pub(crate) async fn upload(&self, token: &Secret, kind: FileKind, bytes: Vec<u8>) -> Result<FileId, FileError> {
        self.upload_named(token, kind, None, bytes).await
    }

    /// Sends `bytes` as a file of `kind`, with `name` for an attachment and none for any other kind; the server's id.
    pub(crate) async fn upload_named(
        &self,
        token: &Secret,
        kind: FileKind,
        name: Option<&AttachmentName>,
        bytes: Vec<u8>,
    ) -> Result<FileId, FileError> {
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let target = target(kind, name, size, self.most(kind))?;
        let body = Full::new(Bytes::from(bytes)).map_err(|never| match never {}).boxed();
        let request = signed(Method::POST, &self.url, &target, token)?
            .header(CONTENT_LENGTH, size)
            .body(body)
            .map_err(|_| FileError::Reach(LinkError::Malformed))?;
        let posted = timeout(allowed(size), self.posted(request)).await;
        posted
            .map_err(|_| Failed::Reach(LinkError::Timeout))
            .and_then(|posted| posted)
            .map_err(|failed| failed.of(kind))
    }

    /// Sends the file at `path` as a file of `kind`, read a chunk at a time as the connection takes it, with `name`
    /// for an attachment and none for any other kind; `progress` is told the bytes sent so far. The server's id.
    pub(crate) async fn upload_path(
        &self,
        token: &Secret,
        kind: FileKind,
        name: Option<&AttachmentName>,
        path: &Path,
        progress: watch::Sender<u64>,
    ) -> Result<FileId, PathError> {
        let opened = path.to_path_buf();
        let (file, size) = blocking(move || {
            let file = File::open(&opened)?;
            let size = file.metadata()?.len();
            Ok((file, size))
        })
        .await
        .map_err(|failed| PathError::Disk(failed.kind()))?;
        let target = target(kind, name, size, self.most(kind))?;
        let unread: Arc<std::sync::Mutex<Option<io::ErrorKind>>> = Arc::default();
        let body = StreamBody::new(chunks(file, size, progress, Arc::clone(&unread))).boxed();
        let request = signed(Method::POST, &self.url, &target, token)?
            .header(CONTENT_LENGTH, size)
            .body(body)
            .map_err(|_| FileError::Reach(LinkError::Malformed))?;
        let posted = timeout(allowed(size), self.posted(request)).await;
        let posted = posted.map_err(|_| Failed::Reach(LinkError::Timeout)).and_then(|posted| posted);
        posted.map_err(|failed| match unread.lock().ok().and_then(|unread| *unread) {
            Some(disk) => PathError::Disk(disk),
            None => PathError::File(failed.of(kind)),
        })
    }

    /// The bytes of file `id`, a file of `kind`; more than the kind keeps is refused unread.
    pub(crate) async fn fetch(&self, token: &Secret, kind: FileKind, id: FileId) -> Result<Vec<u8>, FileError> {
        let most = kind.stored_bytes();
        let started = Instant::now();
        let response = self.asked(token, &file_path(id)).await.map_err(|failed| failed.of(kind))?;
        let length = content_length(&response).unwrap_or(most);
        let limit = usize::try_from(most).map_err(|_| FileError::Reach(LinkError::Malformed))?;
        let read = timeout_at(started + allowed(length), whole(response, limit)).await;
        let read = read.map_err(|_| Failed::Reach(LinkError::Timeout)).and_then(|read| read);
        Ok(read.map_err(|failed| failed.of(kind))?.to_vec())
    }

    /// Fetches file `id`, a file of `kind`, into `path`: written beside it as it arrives and put in place whole, never
    /// past what the kind keeps; on any failure nothing is left behind.
    pub(crate) async fn fetch_to(
        &self,
        token: &Secret,
        kind: FileKind,
        id: FileId,
        path: &Path,
    ) -> Result<(), PathError> {
        let most = kind.stored_bytes();
        let started = Instant::now();
        let partial = partial_of(path).ok_or(PathError::Disk(io::ErrorKind::InvalidInput))?;
        let response = self.asked(token, &file_path(id)).await.map_err(|failed| failed.of(kind))?;
        let length = content_length(&response);
        if length.is_some_and(|length| length > most) {
            return Err(PathError::File(FileError::Reach(LinkError::Malformed)));
        }
        let deadline = started + allowed(length.unwrap_or(most));
        let saved = match timeout_at(deadline, save(response.into_body(), partial.clone(), most)).await {
            Ok(saved) => saved,
            Err(_) => Err(Stopped::Failed(Failed::Reach(LinkError::Timeout))),
        };
        let placed = match saved {
            Ok(()) => {
                let (from, to) = (partial.clone(), path.to_path_buf());
                blocking(move || std::fs::rename(from, to)).await.map_err(|failed| Stopped::Disk(failed.kind()))
            }
            Err(stopped) => Err(stopped),
        };
        if placed.is_err() {
            let _ = blocking(move || std::fs::remove_file(partial)).await;
        }
        placed.map_err(|stopped| stopped.of(kind))
    }

    /// The server's debug report as text, downloaded once by the secret `id` its ask was answered with: the same
    /// request, connection and bound-before-read as a file's. More than [`MAX_REPORT_BYTES`] bytes, or bytes that are
    /// not UTF-8, are refused whole, never cut.
    pub(crate) async fn report(&self, token: &Secret, id: &ReportId) -> Result<String, ReportError> {
        let started = Instant::now();
        let response = self.asked(token, &id.path()).await.map_err(Failed::of_report)?;
        let length = content_length(&response);
        if length.is_some_and(|length| length > REPORT_MOST) {
            return Err(ReportError::Reach(LinkError::Malformed));
        }
        let deadline = started + allowed(length.unwrap_or(REPORT_MOST));
        let read = timeout_at(deadline, whole(response, MAX_REPORT_BYTES)).await;
        let read = read.map_err(|_| Failed::Reach(LinkError::Timeout)).and_then(|read| read);
        let bytes = read.map_err(Failed::of_report)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| ReportError::Reach(LinkError::Malformed))
    }

    async fn asked(&self, token: &Secret, path: &str) -> Result<Response<Incoming>, Failed> {
        let request = signed(Method::GET, &self.url, path, token)
            .map_err(|_| Failed::Reach(LinkError::Malformed))?
            .body(Full::default().map_err(|never| match never {}).boxed())
            .map_err(|_| Failed::Reach(LinkError::Malformed))?;
        timeout(EXCHANGE, self.send(request, StatusCode::OK)).await.map_err(|_| Failed::Reach(LinkError::Timeout))?
    }

    async fn posted(&self, request: Request<Body>) -> Result<FileId, Failed> {
        let response = self.send(request, StatusCode::CREATED).await?;
        let answer = whole(response, ID_ANSWER_BYTES).await?;
        std::str::from_utf8(&answer)
            .ok()
            .and_then(|text| FileId::parse(text).ok())
            .ok_or(Failed::Reach(LinkError::Malformed))
    }

    /// `request` on the kept connection, dialled again when the server closed it; a request the closed connection
    /// handed back unsent is sent once more on the new one. The answer when its status is `expected`.
    async fn send(&self, request: Request<Body>, expected: StatusCode) -> Result<Response<Incoming>, Failed> {
        let mut kept = self.kept.lock().await;
        let mut request = request;
        let mut fresh = false;
        loop {
            let reused = match kept.take() {
                Some(mut sender) => sender.ready().await.is_ok().then_some(sender),
                None => None,
            };
            let mut sender = match reused {
                Some(sender) => sender,
                None => {
                    fresh = true;
                    let mut sender = self.dial().await?;
                    sender.ready().await.map_err(|_| Failed::Reach(LinkError::Connect))?;
                    sender
                }
            };
            match sender.try_send_request(request).await {
                Ok(response) => {
                    if !closes(&response) {
                        *kept = Some(sender);
                    }
                    return checked(response, expected);
                }
                Err(mut unsent) => match unsent.take_message() {
                    Some(back) if !fresh => request = back,
                    _ => return Err(Failed::Reach(LinkError::Connect)),
                },
            }
        }
    }

    async fn dial(&self) -> Result<SendRequest<Body>, Failed> {
        let scheme = if self.url.encrypted() { "https" } else { "http" };
        let uri: Uri = format!("{scheme}://{}/", self.url.authority())
            .parse()
            .map_err(|_| Failed::Reach(LinkError::NoSuchHost))?;
        if !self.url.encrypted() {
            return handshake(reach(&uri).await.map_err(Failed::Reach)?).await;
        }
        handshake(self.trust.secure(&uri).await.map_err(Failed::Reach)?).await
    }
}

async fn handshake<T>(io: T) -> Result<SendRequest<Body>, Failed>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, connection) =
        http1::handshake(TokioIo::new(io)).await.map_err(|_| Failed::Reach(LinkError::Connect))?;
    // The connection ends by itself once its sender is dropped or the server closes it.
    tokio::spawn(connection);
    Ok(sender)
}

/// Where file `id` is downloaded from.
fn file_path(id: FileId) -> String {
    format!("/v1/files/{id}")
}

/// The path and query an upload of `kind` and `size` bytes is posted to, or its refusal before anything is sent: a
/// kind nobody uploads, one over its bound, an attachment without a name or another kind with one.
fn target(kind: FileKind, name: Option<&AttachmentName>, size: u64, most: u64) -> Result<String, FileError> {
    let refused = |refusal| FileError::Refused { kind, refusal };
    if !kind.uploaded() {
        return Err(refused(FileRefusal::BadRequest));
    }
    if size > most {
        return Err(refused(FileRefusal::TooLarge));
    }
    match (kind, name) {
        (FileKind::Attachment, Some(name)) => {
            Ok(format!("/v1/files?kind={}&name={}", kind.word(), name.percent_encoded()))
        }
        (FileKind::Attachment, None) | (_, Some(_)) => Err(refused(FileRefusal::BadRequest)),
        (_, None) => Ok(format!("/v1/files?kind={}", kind.word())),
    }
}

/// How long a transfer of `bytes` may take.
fn allowed(bytes: u64) -> Duration {
    EXCHANGE.saturating_add(Duration::from_secs(bytes / LEAST_RATE))
}

/// `file`'s `size` bytes as a body, a chunk read on the blocking pool each time the connection asks for one;
/// `progress` is told each total sent, and `unread` why the file could not be read.
fn chunks(
    file: File,
    size: u64,
    progress: watch::Sender<u64>,
    unread: Arc<std::sync::Mutex<Option<io::ErrorKind>>>,
) -> impl futures_util::Stream<Item = Result<Frame<Bytes>, io::Error>> + Send + Sync + 'static {
    stream::unfold((Some(file), 0u64, progress), move |(file, sent, progress)| {
        let unread = Arc::clone(&unread);
        async move {
            let mut file = file?;
            if sent >= size {
                return None;
            }
            let want = usize::try_from(size - sent).map_or(CHUNK, |left| left.min(CHUNK));
            let read = blocking(move || {
                let mut chunk = vec![0u8; want];
                let read = file.read(&mut chunk)?;
                if read == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                chunk.truncate(read);
                Ok((file, chunk))
            })
            .await;
            match read {
                Ok((file, chunk)) => {
                    let sent = sent + u64::try_from(chunk.len()).unwrap_or(u64::MAX);
                    progress.send_replace(sent);
                    Some((Ok(Frame::data(Bytes::from(chunk))), (Some(file), sent, progress)))
                }
                Err(failed) => {
                    if let Ok(mut unread) = unread.lock() {
                        *unread = Some(failed.kind());
                    }
                    Some((Err(failed), (None, sent, progress)))
                }
            }
        }
    })
}

/// `body` written to `partial` as it arrives, refused at its first byte past `most`, flushed to the disk at its end.
async fn save(mut body: Incoming, partial: PathBuf, most: u64) -> Result<(), Stopped> {
    let mut file = Some(blocking(move || File::create(partial)).await.map_err(|failed| Stopped::Disk(failed.kind()))?);
    let mut received = 0u64;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| Stopped::Failed(Failed::Reach(LinkError::Lost)))?;
        let Ok(chunk) = frame.into_data() else { continue };
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > most {
            // Past what the kind keeps is the server's fault.
            return Err(Stopped::Failed(Failed::Reach(LinkError::Malformed)));
        }
        let mut open = file.take().ok_or(Stopped::Disk(io::ErrorKind::Other))?;
        let written = blocking(move || open.write_all(&chunk).map(|()| open)).await;
        file = Some(written.map_err(|failed| Stopped::Disk(failed.kind()))?);
    }
    let open = file.ok_or(Stopped::Disk(io::ErrorKind::Other))?;
    blocking(move || open.sync_all()).await.map_err(|failed| Stopped::Disk(failed.kind()))
}

/// What a download's partial file's name ends with, after this process's id: a later run sweeps a dead run's.
pub const PARTIAL: &str = ".partial";

/// Where a download into `path` is written until it is whole: a hidden name beside it, carrying this process's id so
/// a later run can tell a dead run's from a live one's.
fn partial_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!(".{name}.{}{PARTIAL}", std::process::id())))
}

/// `work` on the blocking pool; a task that did not finish is an I/O failure.
async fn blocking<T, F>(work: F) -> io::Result<T>
where
    F: FnOnce() -> io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|_| Err(io::ErrorKind::Other.into()))
}

/// A request's head in origin form: the path, `url`'s authority as its host, the token as its bearer, marked so no log prints it.
fn signed(
    method: Method,
    url: &ServerUrl,
    path: &str,
    token: &Secret,
) -> Result<hyper::http::request::Builder, FileError> {
    let mut bearer = HeaderValue::try_from(format!("Bearer {}", token.expose()))
        .map_err(|_| FileError::Reach(LinkError::Malformed))?;
    bearer.set_sensitive(true);
    Ok(Request::builder().method(method).uri(path).header(HOST, url.authority()).header(AUTHORIZATION, bearer))
}

fn closes(response: &Response<Incoming>) -> bool {
    response.headers().get_all(CONNECTION).iter().any(|value| {
        value.to_str().is_ok_and(|value| value.split(',').any(|token| token.trim().eq_ignore_ascii_case("close")))
    })
}

fn content_length(response: &Response<Incoming>) -> Option<u64> {
    response.headers().get(CONTENT_LENGTH).and_then(|value| value.to_str().ok()).and_then(|value| value.parse().ok())
}

fn checked(response: Response<Incoming>, expected: StatusCode) -> Result<Response<Incoming>, Failed> {
    let status = response.status();
    if status == expected {
        return Ok(response);
    }
    Err(match FileRefusal::of_status(status.as_u16()) {
        Some(refusal) => Failed::Refused(refusal),
        None => Failed::Reach(LinkError::NotChatkord),
    })
}

/// `response`'s body read whole, to at most `most` bytes.
async fn whole(response: Response<Incoming>, most: usize) -> Result<Bytes, Failed> {
    let body = Limited::new(response.into_body(), most).collect().await;
    body.map(|collected| collected.to_bytes()).map_err(|failed| {
        // Past what the kind keeps is the server's fault; anything else is the connection's.
        let over = failed.downcast_ref::<http_body_util::LengthLimitError>().is_some();
        Failed::Reach(if over { LinkError::Malformed } else { LinkError::Lost })
    })
}

/// What an exchange came to before its kind is known.
enum Failed {
    Reach(LinkError),
    Refused(FileRefusal),
}

impl Failed {
    fn of(self, kind: FileKind) -> FileError {
        match self {
            Self::Reach(failed) => FileError::Reach(failed),
            Self::Refused(refusal) => FileError::Refused { kind, refusal },
        }
    }

    fn of_report(self) -> ReportError {
        match self {
            Self::Reach(failed) => ReportError::Reach(failed),
            Self::Refused(refusal) => ReportError::Refused(refusal),
        }
    }
}

/// Why a download into a path stopped before its kind is known.
enum Stopped {
    Failed(Failed),
    Disk(io::ErrorKind),
}

impl Stopped {
    fn of(self, kind: FileKind) -> PathError {
        match self {
            Self::Failed(failed) => PathError::File(failed.of(kind)),
            Self::Disk(disk) => PathError::Disk(disk),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::server::conn::http1 as served;
    use hyper::service::service_fn;
    use hyper::{Response, header};
    use proto::files::PICTURE_UPLOAD_BYTES;
    use std::convert::Infallible;
    use std::sync::Mutex as Shared;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use terms::FileLimits;
    use tokio::net::TcpListener;

    const TOKEN: &str = "the-session-token";
    const ID: &str = "0fa1000000000000000000000000ff10";

    /// What the test server saw of the last request it answered.
    #[derive(Default)]
    struct Seen {
        head: Option<(Method, String, Option<String>, Option<String>)>,
        body: Vec<u8>,
    }

    /// How the test server answers.
    #[derive(Clone, Copy, Default)]
    struct Answering {
        /// `Connection: close` on every answer.
        close: bool,
        /// The body in chunks of no stated length.
        chunked: bool,
        /// Each connection dropped this long after it was accepted.
        drop_after: Option<Duration>,
        /// Answered from the head alone, as a server refusing a stated length does; the body is read on and thrown
        /// away, as the server's lingering close does, since one closed on unread bytes is reset with its answer.
        early: bool,
    }

    /// A test server on this machine and what it saw: the last request and the connections it accepted.
    struct Served {
        url: ServerUrl,
        seen: Arc<Shared<Seen>>,
        accepted: Arc<AtomicUsize>,
    }

    async fn serve(status: u16, body: Vec<u8>) -> (ServerUrl, Arc<Shared<Seen>>) {
        let served = serve_as(status, body, Answering::default()).await;
        (served.url, served.seen)
    }

    /// A server answering every request with `status` and `body`, as `answering` says, keeping what it was sent.
    async fn serve_as(status: u16, body: Vec<u8>, answering: Answering) -> Served {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = ServerUrl::parse(&format!("ws://{}/v1", listener.local_addr().unwrap())).unwrap();
        let seen = Arc::new(Shared::new(Seen::default()));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (kept, counted) = (seen.clone(), accepted.clone());
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let (kept, body) = (kept.clone(), body.clone());
                let answer = service_fn(move |request: hyper::Request<Incoming>| {
                    let (kept, body) = (kept.clone(), body.clone());
                    async move {
                        let named =
                            |name| request.headers().get(name).map(|v: &HeaderValue| v.to_str().unwrap().to_string());
                        let head = (
                            request.method().clone(),
                            request.uri().to_string(),
                            named(header::AUTHORIZATION),
                            named(header::HOST),
                        );
                        let sent = if answering.early {
                            let mut unread = request.into_body();
                            tokio::spawn(async move { while let Some(Ok(_)) = unread.frame().await {} });
                            Vec::new()
                        } else {
                            request.into_body().collect().await.unwrap().to_bytes().to_vec()
                        };
                        *kept.lock().unwrap() = Seen { head: Some(head), body: sent };
                        let body: BoxBody<Bytes, Infallible> = if answering.chunked {
                            let frames: Vec<Result<Frame<Bytes>, Infallible>> =
                                body.chunks(1000).map(|chunk| Ok(Frame::data(Bytes::copy_from_slice(chunk)))).collect();
                            StreamBody::new(stream::iter(frames)).boxed()
                        } else {
                            Full::new(Bytes::from(body)).boxed()
                        };
                        let mut response = Response::new(body);
                        *response.status_mut() = StatusCode::from_u16(status).unwrap();
                        if answering.close {
                            response.headers_mut().insert(CONNECTION, HeaderValue::from_static("close"));
                        }
                        Ok::<_, Infallible>(response)
                    }
                });
                let connection = served::Builder::new().serve_connection(TokioIo::new(stream), answer);
                match answering.drop_after {
                    Some(after) => tokio::spawn(async move {
                        let _ = timeout(after, connection).await;
                    }),
                    None => tokio::spawn(async move {
                        let _ = connection.await;
                    }),
                };
            }
        });
        Served { url, seen, accepted }
    }

    fn token() -> Secret {
        Secret::new(TOKEN.to_string())
    }

    /// One upload on a lane of its own.
    async fn upload(
        url: &ServerUrl,
        trust: &Trust,
        token: &Secret,
        kind: FileKind,
        bytes: Vec<u8>,
    ) -> Result<FileId, FileError> {
        Lane::new(url.clone(), trust.clone(), FileLimits::default()).upload(token, kind, bytes).await
    }

    /// One fetch on a lane of its own.
    async fn fetch(
        url: &ServerUrl,
        trust: &Trust,
        token: &Secret,
        kind: FileKind,
        id: FileId,
    ) -> Result<Vec<u8>, FileError> {
        Lane::new(url.clone(), trust.clone(), FileLimits::default()).fetch(token, kind, id).await
    }

    #[test]
    fn a_partial_download_is_named_by_this_process_so_a_later_run_can_tell_a_dead_runs() {
        let partial = partial_of(Path::new("/tmp/report.txt")).unwrap();
        let name = format!(".report.txt.{}{PARTIAL}", std::process::id());
        assert_eq!(partial, Path::new("/tmp").join(name));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chatkord-link-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn an_upload_is_posted_signed_in_with_its_kind_and_answered_with_the_id() {
        let (url, seen) = serve(201, ID.into()).await;
        let id = upload(&url, &Trust::system(), &token(), FileKind::ServerPicture, b"picture".to_vec()).await;
        assert_eq!(id, Ok(FileId::parse(ID).unwrap()));
        let seen = seen.lock().unwrap();
        let (method, path, bearer, host) = seen.head.clone().unwrap();
        assert_eq!(method, Method::POST);
        assert_eq!(path, "/v1/files?kind=server-picture");
        assert_eq!(bearer.as_deref(), Some("Bearer the-session-token"));
        assert_eq!(host.as_deref(), Some(url.authority()));
        assert_eq!(seen.body, b"picture");
    }

    #[tokio::test]
    async fn a_fetch_gets_the_file_by_its_id() {
        let (url, seen) = serve(200, b"png".to_vec()).await;
        let bytes = fetch(&url, &Trust::system(), &token(), FileKind::ServerPicture, FileId::parse(ID).unwrap()).await;
        assert_eq!(bytes, Ok(b"png".to_vec()));
        let (method, path, bearer, _) = seen.lock().unwrap().head.clone().unwrap();
        assert_eq!((method, path), (Method::GET, format!("/v1/files/{ID}")));
        assert_eq!(bearer.as_deref(), Some("Bearer the-session-token"));
    }

    const REPORT_ID: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";

    /// One report download on a lane of its own.
    async fn report(url: &ServerUrl) -> Result<String, ReportError> {
        let id = ReportId::parse(REPORT_ID.to_owned()).unwrap();
        Lane::new(url.clone(), Trust::system(), FileLimits::default()).report(&token(), &id).await
    }

    #[tokio::test]
    async fn a_report_is_got_by_its_secret_id_signed_in_as_a_file_is() {
        let (url, seen) = serve(200, b"Chatkord debug report: server\n".to_vec()).await;
        assert_eq!(report(&url).await.as_deref(), Ok("Chatkord debug report: server\n"));
        let (method, path, bearer, host) = seen.lock().unwrap().head.clone().unwrap();
        assert_eq!((method, path), (Method::GET, format!("/v1/files/reports/{REPORT_ID}")));
        assert_eq!(bearer.as_deref(), Some("Bearer the-session-token"));
        assert_eq!(host.as_deref(), Some(url.authority()));
    }

    #[tokio::test]
    async fn a_reports_refusals_are_read_by_their_status() {
        for refusal in [
            FileRefusal::Unauthenticated,
            FileRefusal::NotAuthorized,
            FileRefusal::NotFound,
            FileRefusal::TooMany,
            FileRefusal::Unavailable,
            FileRefusal::Internal,
        ] {
            let (url, _) = serve(refusal.status(), Vec::new()).await;
            assert_eq!(report(&url).await, Err(ReportError::Refused(refusal)));
        }
        let (url, _) = serve(418, Vec::new()).await;
        assert_eq!(report(&url).await, Err(ReportError::Reach(LinkError::NotChatkord)));
    }

    #[tokio::test]
    async fn a_report_past_the_bound_is_refused_whole_never_cut() {
        let at = vec![b'x'; MAX_REPORT_BYTES];
        let (url, _) = serve(200, at.clone()).await;
        assert_eq!(report(&url).await.map(|text| text.len()), Ok(MAX_REPORT_BYTES), "at the bound it is kept");
        let over = vec![b'x'; MAX_REPORT_BYTES + 1];
        let (url, _) = serve(200, over.clone()).await;
        assert_eq!(report(&url).await, Err(ReportError::Reach(LinkError::Malformed)), "by its stated length");
        let chunked = Answering { chunked: true, ..Answering::default() };
        let served = serve_as(200, over, chunked).await;
        assert_eq!(report(&served.url).await, Err(ReportError::Reach(LinkError::Malformed)), "by what arrives");
        let served = serve_as(200, at, chunked).await;
        assert_eq!(report(&served.url).await.map(|text| text.len()), Ok(MAX_REPORT_BYTES));
    }

    #[tokio::test]
    async fn a_report_that_is_not_text_is_refused() {
        let (url, _) = serve(200, vec![0xff, 0xfe, b'x']).await;
        assert_eq!(report(&url).await, Err(ReportError::Reach(LinkError::Malformed)));
    }

    #[test]
    fn a_report_id_is_43_characters_of_base64url_and_prints_redacted() {
        assert!(ReportId::parse(REPORT_ID.to_owned()).is_ok());
        let bad = [
            String::new(),
            "short".to_owned(),
            REPORT_ID.replacen('A', "", 1),
            format!("{REPORT_ID}A"),
            REPORT_ID.replace('A', "/"),
            REPORT_ID.replace('b', "+"),
            REPORT_ID.replace('C', "?"),
        ];
        for bad in bad {
            assert!(ReportId::parse(bad.clone()).is_err(), "{bad:?}");
        }
        let id = ReportId::parse(REPORT_ID.to_owned()).unwrap();
        assert!(!format!("{id:?}").contains(REPORT_ID) && format!("{id:?}").contains("[redacted]"));
    }

    #[tokio::test]
    async fn every_refusal_is_read_by_its_status() {
        for refusal in FileRefusal::ALL {
            let (url, _) = serve(refusal.status(), Vec::new()).await;
            let kind = FileKind::ServerPicture;
            let failed = upload(&url, &Trust::system(), &token(), kind, b"picture".to_vec()).await;
            assert_eq!(failed, Err(FileError::Refused { kind, refusal }));
        }
    }

    #[tokio::test]
    async fn an_upload_over_its_kinds_bound_is_refused_before_anything_is_sent() {
        let (url, seen) = serve(201, ID.into()).await;
        let over = vec![0u8; usize::try_from(PICTURE_UPLOAD_BYTES).unwrap() + 1];
        let kind = FileKind::ServerPicture;
        let failed = upload(&url, &Trust::system(), &token(), kind, over).await;
        assert_eq!(failed, Err(FileError::Refused { kind, refusal: FileRefusal::TooLarge }));
        assert!(seen.lock().unwrap().head.is_none(), "nothing reached the server");
    }

    #[tokio::test]
    async fn a_download_past_what_its_kind_keeps_is_refused_unread() {
        let kind = FileKind::ServerPicture;
        let over = vec![0u8; usize::try_from(kind.stored_bytes()).unwrap() + 1];
        let (url, _) = serve(200, over).await;
        let failed = fetch(&url, &Trust::system(), &token(), kind, FileId::parse(ID).unwrap()).await;
        assert_eq!(failed, Err(FileError::Reach(LinkError::Malformed)));
    }

    #[tokio::test]
    async fn an_answer_that_is_not_ours_is_not_taken_for_one() {
        let kind = FileKind::ServerPicture;
        let (url, _) = serve(201, b"not an id".to_vec()).await;
        let failed = upload(&url, &Trust::system(), &token(), kind, b"picture".to_vec()).await;
        assert_eq!(failed, Err(FileError::Reach(LinkError::Malformed)));
        let (url, _) = serve(418, Vec::new()).await;
        let failed = fetch(&url, &Trust::system(), &token(), kind, FileId::parse(ID).unwrap()).await;
        assert_eq!(failed, Err(FileError::Reach(LinkError::NotChatkord)));
    }

    #[tokio::test]
    async fn nothing_listening_is_said_as_the_socket_says_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = ServerUrl::parse(&format!("ws://{}/v1", listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let failed = fetch(&url, &Trust::system(), &token(), FileKind::ServerPicture, FileId::parse(ID).unwrap()).await;
        assert_eq!(failed, Err(FileError::Reach(LinkError::Connect)));
    }

    #[tokio::test]
    async fn two_requests_ride_one_connection() {
        let served = serve_as(201, ID.into(), Answering::default()).await;
        let lane = Lane::new(served.url.clone(), Trust::system(), FileLimits::default());
        let kind = FileKind::ServerPicture;
        assert_eq!(lane.upload(&token(), kind, b"picture".to_vec()).await, Ok(FileId::parse(ID).unwrap()));
        assert_eq!(lane.upload(&token(), kind, b"another".to_vec()).await, Ok(FileId::parse(ID).unwrap()));
        assert_eq!(served.seen.lock().unwrap().body, b"another");
        assert_eq!(served.accepted.load(Ordering::SeqCst), 1, "the second request used the kept connection");
    }

    #[tokio::test]
    async fn a_connection_the_server_closes_is_dialled_again() {
        let served = serve_as(200, b"png".to_vec(), Answering { close: true, ..Answering::default() }).await;
        let lane = Lane::new(served.url.clone(), Trust::system(), FileLimits::default());
        let (kind, id) = (FileKind::ServerPicture, FileId::parse(ID).unwrap());
        for _ in 0..2 {
            assert_eq!(lane.fetch(&token(), kind, id).await, Ok(b"png".to_vec()));
        }
        assert_eq!(served.accepted.load(Ordering::SeqCst), 2, "a closed connection is never asked again");
        let idle = Answering { drop_after: Some(Duration::from_millis(200)), ..Answering::default() };
        let served = serve_as(200, b"png".to_vec(), idle).await;
        let lane = Lane::new(served.url.clone(), Trust::system(), FileLimits::default());
        assert_eq!(lane.fetch(&token(), kind, id).await, Ok(b"png".to_vec()));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(lane.fetch(&token(), kind, id).await, Ok(b"png".to_vec()), "dropped while idle: dialled again");
        assert_eq!(served.accepted.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn the_progress_rises_by_each_chunk_as_it_is_handed_to_the_body() {
        use futures_util::StreamExt as _;
        let dir = scratch("chunks");
        let path = dir.join("sent.bin");
        let size = 3 * CHUNK + 5;
        std::fs::write(&path, vec![7u8; size]).unwrap();
        let (progress, watched) = watch::channel(0u64);
        let unread = Arc::new(std::sync::Mutex::new(None));
        let mut body = std::pin::pin!(chunks(File::open(&path).unwrap(), size as u64, progress, unread));
        let mut told = Vec::new();
        while let Some(frame) = body.next().await {
            assert!(frame.is_ok(), "a chunk was read");
            told.push(*watched.borrow());
        }
        let chunk = CHUNK as u64;
        assert_eq!(told, [chunk, 2 * chunk, 3 * chunk, size as u64], "one step a chunk, to the whole");
    }

    #[tokio::test]
    async fn an_attachment_is_streamed_from_its_file_named_and_its_progress_told() {
        let served = serve_as(201, ID.into(), Answering::default()).await;
        let lane = Lane::new(served.url.clone(), Trust::system(), FileLimits::default());
        let dir = scratch("upload");
        let path = dir.join("sent.bin");
        let sent: Vec<u8> = (0..3 * CHUNK + 5).map(|at| u8::try_from(at % 251).unwrap()).collect();
        std::fs::write(&path, &sent).unwrap();
        let (progress, mut watched) = watch::channel(0u64);
        let told = tokio::spawn(async move {
            let mut told = Vec::new();
            while watched.changed().await.is_ok() {
                told.push(*watched.borrow_and_update());
            }
            told
        });
        let name = AttachmentName::parse("a b+ά.bin").unwrap();
        let id = lane.upload_path(&token(), FileKind::Attachment, Some(&name), &path, progress).await;
        assert_eq!(id, Ok(FileId::parse(ID).unwrap()));
        let told = told.await.unwrap();
        let size = u64::try_from(sent.len()).unwrap();
        assert_eq!(told.last(), Some(&size), "{told:?}");
        assert!(told.is_sorted(), "never fell: {told:?}");
        let seen = served.seen.lock().unwrap();
        let (method, target, _, _) = seen.head.clone().unwrap();
        assert_eq!((method, target), (Method::POST, "/v1/files?kind=attachment&name=a%20b+%CE%AC.bin".to_string()));
        assert_eq!(seen.body, sent);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_upload_s_name_goes_with_an_attachment_alone() {
        let (url, seen) = serve(201, ID.into()).await;
        let lane = Lane::new(url, Trust::system(), FileLimits::default());
        let dir = scratch("names");
        let path = dir.join("sent.bin");
        std::fs::write(&path, b"bytes").unwrap();
        let name = AttachmentName::parse("sent.bin").unwrap();
        for (kind, name) in [(FileKind::Attachment, None), (FileKind::Emoji, Some(&name)), (FileKind::Thumbnail, None)]
        {
            let failed = lane.upload_path(&token(), kind, name, &path, watch::channel(0).0).await;
            assert_eq!(failed, Err(PathError::File(FileError::Refused { kind, refusal: FileRefusal::BadRequest })));
        }
        let (token, gone) = (token(), dir.join("gone"));
        let missing = lane.upload_path(&token, FileKind::Attachment, Some(&name), &gone, watch::channel(0).0).await;
        assert_eq!(missing, Err(PathError::Disk(io::ErrorKind::NotFound)));
        assert!(seen.lock().unwrap().head.is_none(), "nothing reached the server");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_refusal_answered_before_the_body_is_read_is_that_refusal() {
        let refusal = FileRefusal::AccountFull;
        let served =
            serve_as(refusal.status(), Vec::new(), Answering { early: true, close: true, ..Answering::default() })
                .await;
        let lane = Lane::new(served.url, Trust::system(), FileLimits::default());
        let dir = scratch("early");
        let path = dir.join("large.bin");
        std::fs::write(&path, vec![0u8; 16 * 1024 * 1024]).unwrap();
        let name = AttachmentName::parse("large.bin").unwrap();
        let kind = FileKind::Attachment;
        let failed = lane.upload_path(&token(), kind, Some(&name), &path, watch::channel(0).0).await;
        assert_eq!(failed, Err(PathError::File(FileError::Refused { kind, refusal })));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_attachment_past_the_servers_greeted_most_is_refused_before_anything_is_sent() {
        let (url, seen) = serve(201, ID.into()).await;
        let lane = Lane::new(url, Trust::system(), FileLimits { attachment: Some(4), account: None });
        let dir = scratch("server-most");
        let (at_most, over) = (dir.join("four.bin"), dir.join("five.bin"));
        std::fs::write(&at_most, b"four").unwrap();
        std::fs::write(&over, b"five!").unwrap();
        let name = AttachmentName::parse("five.bin").unwrap();
        let kind = FileKind::Attachment;
        let failed = lane.upload_path(&token(), kind, Some(&name), &over, watch::channel(0).0).await;
        assert_eq!(failed, Err(PathError::File(FileError::Refused { kind, refusal: FileRefusal::TooLarge })));
        assert!(seen.lock().unwrap().head.is_none(), "nothing reached the server");
        let taken = lane.upload_path(&token(), kind, Some(&name), &at_most, watch::channel(0).0).await;
        assert!(taken.is_ok(), "at its most: {taken:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_download_is_written_to_its_path_and_one_past_its_bound_leaves_nothing() {
        let kind = FileKind::Emoji;
        let (dir, id) = (scratch("download"), FileId::parse(ID).unwrap());
        let path = dir.join("kept.png");
        let (url, _) = serve(200, b"emoji".to_vec()).await;
        assert_eq!(
            Lane::new(url, Trust::system(), FileLimits::default()).fetch_to(&token(), kind, id, &path).await,
            Ok(())
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"emoji");
        std::fs::remove_file(&path).unwrap();
        let over = vec![7u8; usize::try_from(kind.stored_bytes()).unwrap() + 1];
        for chunked in [false, true] {
            let served = serve_as(200, over.clone(), Answering { chunked, ..Answering::default() }).await;
            let failed =
                Lane::new(served.url, Trust::system(), FileLimits::default()).fetch_to(&token(), kind, id, &path).await;
            assert_eq!(failed, Err(PathError::File(FileError::Reach(LinkError::Malformed))), "chunked: {chunked}");
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing left behind, chunked: {chunked}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
