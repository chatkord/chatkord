//! One file fetched from a host: a GET on a connection of its own, redirects followed up to a bound, each wait
//! bounded, the answer read a part at a time. Where a connection goes, and which redirects are taken, is the
//! [`Route`]'s, so each user keeps its own policy.

use http_body_util::{BodyExt, Empty};
use hyper::body::{Body, Bytes, Incoming};
use hyper::client::conn::http1;
use hyper::header::{ACCEPT, CONTENT_TYPE, HOST, LOCATION, USER_AGENT};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::fmt;
use std::time::Duration;
use terms::LinkError;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// A byte stream a request rides on, TLS included.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Io for T {}

/// A connection a [`Route`] opened.
pub type Stream = Box<dyn Io>;

/// Why a fetch did not give a file. `Display` is the operator's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GetError {
    /// The host could not be reached or did not prove who it is: the connection's own failure.
    Reach(LinkError),
    /// The route refused the address it would have connected to.
    Refused,
    /// The host took too long to connect, to answer or to send the next part of an answer.
    Timeout,
    /// The connection ended before the answer did.
    Lost,
    /// The host answered with this status, neither the file nor a redirect.
    Status(u16),
    /// A redirect led nowhere the route follows.
    BadRedirect,
    /// More redirects than the fetch follows.
    TooManyRedirects,
    /// An answer longer than its bound.
    TooLarge,
    /// The request could not be written.
    Malformed,
}

impl fmt::Display for GetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GetError::Reach(failed) => write!(f, "the host: {failed}"),
            GetError::Refused => f.write_str("the address is not one this fetch reaches"),
            GetError::Timeout => f.write_str("the host took too long to answer"),
            GetError::Lost => f.write_str("the connection ended early"),
            GetError::Status(status) => write!(f, "the host answered {status}"),
            GetError::BadRedirect => f.write_str("a redirect led somewhere this fetch does not follow"),
            GetError::TooManyRedirects => f.write_str("the host redirected too many times"),
            GetError::TooLarge => f.write_str("an answer was over its bound"),
            GetError::Malformed => f.write_str("the request could not be written"),
        }
    }
}

impl std::error::Error for GetError {}

/// How long a fetch waits for each step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    /// For a connection, its TLS handshake included.
    pub connect: Duration,
    /// For an answer's head, and for each next part of its body.
    pub stall: Duration,
}

impl Default for Patience {
    fn default() -> Patience {
        Patience { connect: Duration::from_secs(15), stall: Duration::from_secs(30) }
    }
}

/// One address a fetch asks, and the policy of reaching it.
pub trait Route: Sized + Send + Sync {
    /// The request's `Host`.
    fn authority(&self) -> &str;

    /// The request's target in origin form: path and query.
    fn origin_form(&self) -> &str;

    /// The request's `User-Agent`.
    fn user_agent(&self) -> String;

    /// The request's `Accept`, when it states one.
    fn accept(&self) -> Option<&str> {
        None
    }

    /// A connection to this address, TLS included. Bounded by [`Patience::connect`] along with the handshake.
    fn connect(&self) -> impl Future<Output = Result<Stream, GetError>> + Send;

    /// Where a redirect's `location` leads from here.
    fn redirected(&self, location: &str) -> Result<Self, GetError>;
}

/// An answer whose head said 200: its body, read a part at a time, on a connection that ends with it.
pub struct Answer {
    body: Incoming,
    content_type: Option<String>,
    stall: Duration,
    _connection: Driven,
}

impl Answer {
    /// The body's length, when the head stated one.
    pub fn stated_length(&self) -> Option<u64> {
        self.body.size_hint().exact()
    }

    /// The head's `Content-Type` as written, when it states one.
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// The body's next part; none once it has ended.
    pub async fn next(&mut self) -> Result<Option<Bytes>, GetError> {
        loop {
            let frame = timeout(self.stall, self.body.frame()).await.map_err(|_| GetError::Timeout)?;
            match frame {
                None => return Ok(None),
                Some(Err(_)) => return Err(GetError::Lost),
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        return Ok(Some(data));
                    }
                }
            }
        }
    }

    /// The whole body, refused past `most` bytes, by its stated length before any of it is read.
    pub async fn whole(mut self, most: usize) -> Result<Vec<u8>, GetError> {
        if self.stated_length().is_some_and(|stated| stated > u64::try_from(most).unwrap_or(u64::MAX)) {
            return Err(GetError::TooLarge);
        }
        let mut whole = Vec::new();
        while let Some(part) = self.next().await? {
            if part.len() > most - whole.len() {
                return Err(GetError::TooLarge);
            }
            whole.extend_from_slice(&part);
        }
        Ok(whole)
    }
}

/// The task that drives a connection, ended with the answer it carries.
struct Driven(JoinHandle<()>);

impl Drop for Driven {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The file at `first`, following at most `redirects` redirects as the route allows them; the answer, and the
/// address it came from.
pub async fn get<R: Route>(first: R, patience: Patience, redirects: usize) -> Result<(Answer, R), GetError> {
    let mut at = first;
    for _ in 0..=redirects {
        let (response, connection) = once(&at, patience).await?;
        match response.status() {
            StatusCode::OK => {
                let content_type =
                    response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(str::to_string);
                let stall = patience.stall;
                return Ok((Answer { body: response.into_body(), content_type, stall, _connection: connection }, at));
            }
            StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT => {
                let location = response.headers().get(LOCATION).and_then(|value| value.to_str().ok());
                at = at.redirected(location.ok_or(GetError::BadRedirect)?)?;
            }
            status => return Err(GetError::Status(status.as_u16())),
        }
    }
    Err(GetError::TooManyRedirects)
}

/// One GET of `at` on a connection of its own: the answer's head, and the connection its body arrives on.
async fn once<R: Route>(at: &R, patience: Patience) -> Result<(Response<Incoming>, Driven), GetError> {
    let mut request = Request::get(at.origin_form()).header(HOST, at.authority()).header(USER_AGENT, at.user_agent());
    if let Some(accept) = at.accept() {
        request = request.header(ACCEPT, accept);
    }
    let request = request.body(Empty::<Bytes>::new()).map_err(|_| GetError::Malformed)?;
    let connected = async { handshake(at.connect().await?).await };
    let (mut sender, connection) = timeout(patience.connect, connected).await.map_err(|_| GetError::Timeout)??;
    let response = timeout(patience.stall, sender.send_request(request)).await.map_err(|_| GetError::Timeout)?;
    Ok((response.map_err(|_| GetError::Lost)?, connection))
}

/// HTTP/1.1 over `io`, its connection driven by a task of its own.
async fn handshake(io: Stream) -> Result<(http1::SendRequest<Empty<Bytes>>, Driven), GetError> {
    let (sender, connection) = http1::handshake(TokioIo::new(io)).await.map_err(|_| GetError::Lost)?;
    let driven = tokio::spawn(async move {
        // Its failure reaches the request or the body that rides on it.
        let _ = connection.await;
    });
    Ok((sender, Driven(driven)))
}
