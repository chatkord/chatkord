use crate::files;
use crate::trust::{Handshake, Trust, Vouched};
use futures_util::{SinkExt, StreamExt};
use proto::chatkord_v1::wire_message::Body;
use proto::chatkord_v1::{
    CloseReason, ErrorCode, Event, Heartbeat, LoginAck, ProtoVersion, Resume, ServerChanged, WireMessage, event,
};
use proto::envelope::{
    ASKS_IN_FLIGHT, Frame, HEARTBEAT_SECONDS, IN_CALL_HEARTBEAT_SECONDS, PROTO, Refused, SILENT_SECONDS, close_frame,
    close_reason_of_code, code_of, read_frame, speaks_this_protocol, write_frame,
};
use proto::files::ReportId;
use proto::files::{AttachmentName, FileId, FileKind, FileRefusal};
use proto::ops::{Ask, Reply as _, Tell};
use proto::rules::Switch;
use rustls::HandshakeKind;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use terms::Cursor;
use terms::Heard;
use terms::Secret;
use terms::ServerIdentity;
use terms::{Address, ServerBuild, ServerUrl};
use terms::{ArrivedReport, FileError, FileLimits, ReportError};
use terms::{Ended, LinkError, ServerIs};
use tokio::net::TcpStream;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, interval, timeout};
use tokio_tungstenite::tungstenite::Error as SocketError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, client_async_tls_with_config};

/// The most `Members` pages `Link::list_members` asks for: ten thousand members at the server's page size.
const MAX_MEMBER_PAGES: usize = 100;

/// Events held for a reader that is behind; what comes past them is dropped until the reader is told so.
const EVENT_BACKLOG: usize = 256;
/// The room a reader that fell behind has made before it is told so and the state is asked for whole.
const CATCH_UP_ROOM: usize = EVENT_BACKLOG / 2;
/// Requests queued for the task before a caller waits.
const COMMAND_BACKLOG: usize = 32;
/// How long opening may take, from the upgrade to the greeting.
const OPENING: Duration = Duration::from_secs(10);
/// How long one write may take before the connection is called dead.
const WRITE: Duration = Duration::from_secs(10);
/// How long a client that is leaving waits for its goodbye to be written.
pub const PARTING: Duration = Duration::from_secs(2);
/// How long a request waits for its reply unless its caller names another bound.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(15);
const HELLO_SEQ: u64 = 1;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Reply = oneshot::Sender<Result<Body, LinkError>>;

struct Command {
    body: Body,
    reply: Reply,
}

/// What the task queues for the reader.
#[derive(Debug)]
#[expect(clippy::large_enum_variant, reason = "nearly every one is an event, which a box would allocate for")]
enum Queued {
    Event(Event),
    Behind,
}

impl From<Queued> for Heard {
    fn from(queued: Queued) -> Self {
        match queued {
            Queued::Event(event) => Self::Event(event),
            Queued::Behind => Self::Behind,
        }
    }
}

/// Events in the order the server sent them, then how the connection ended.
pub struct Events {
    events: mpsc::Receiver<Queued>,
    ended: Option<oneshot::Receiver<Ended>>,
}

impl Events {
    /// The next event, or the end of the connection; none once that has been read. Cancelled while it waits for the
    /// end, the end is kept for the next call.
    pub async fn next(&mut self) -> Option<Heard> {
        if let Some(queued) = self.events.recv().await {
            return Some(queued.into());
        }
        let ended = self.ended.as_mut()?.await.ok();
        self.ended = None;
        ended.map(Heard::Ended)
    }

    /// The next event already told, without waiting for one; none when there is none yet. What a server tells as a
    /// session signs in is told before the answer to its first ask.
    pub fn told(&mut self) -> Option<Heard> {
        self.events.try_recv().ok().map(Heard::from)
    }
}

/// What a typed address turned out to be: the one address that greeted, and the connection it greeted on. A
/// credential goes to this address and nowhere else; the greeting carried no secret.
pub struct Resolved {
    /// The connection that greeted, signed in as no one.
    pub link: Link,
    /// What the server tells it unasked.
    pub events: Events,
    /// The address that greeted.
    pub url: ServerUrl,
}

impl Resolved {
    /// The name the server gave itself in its greeting, if it gave one.
    pub fn server_name(&self) -> Option<&str> {
        self.link.greeted.name.as_deref()
    }
}

/// A connection to a server. Dropping it closes the socket.
#[derive(Clone)]
pub struct Link {
    commands: mpsc::Sender<Command>,
    /// Ops nothing answers, sent past the asks in flight.
    tells: mpsc::Sender<Body>,
    /// What the server offers now: its greeting's, then each `ServerChanged`.
    offered: watch::Receiver<Offered>,
    /// The name and colour it gave itself in its greeting, shaped; what a session says after is the window's to keep.
    greeted: ServerIdentity,
    /// How many [`Quickened`] are held; the task beats faster while any is.
    pace: Arc<watch::Sender<usize>>,
    /// Set when this client leaves, whoever else holds the connection: apart from the asks, so a goodbye never waits
    /// behind them.
    leave: Arc<watch::Sender<bool>>,
    /// The round trip of the last heartbeat the server answered.
    round_trip: watch::Receiver<Option<Duration>>,
    /// What vouched for the server at the handshake.
    vouched: Vouched,
    /// Whether the TLS handshake was whole or resumed a session; none at an unencrypted address.
    handshake_kind: Option<HandshakeKind>,
    /// The token this connection signed in with or was issued, set once by the sign-in; a file's request shows it.
    token: Arc<OnceLock<Secret>>,
    /// The server's files, asked one at a time on the connection it keeps for this session.
    lane: Arc<files::Lane>,
    /// The release it named in its greeting, if it named one.
    build: Option<ServerBuild>,
    /// The protocol version it spoke in its greeting.
    protocol: ProtoVersion,
    /// The address the socket connected to, which says how near the server is.
    peer: IpAddr,
}

/// A hold on a faster heartbeat, for as long as it lives: a joined call has it, so its connection always has a
/// round trip, as a call that sends or hears nothing has no media to measure one on.
#[derive(Debug)]
pub struct Quickened {
    pace: Arc<watch::Sender<usize>>,
}

impl Drop for Quickened {
    fn drop(&mut self) {
        self.pace.send_modify(|holds| *holds = holds.saturating_sub(1));
    }
}

impl Link {
    /// The address the socket is connected to: what the server's own scope is read from.
    pub fn peer(&self) -> IpAddr {
        self.peer
    }

    /// Opens the socket at one address and agrees the protocol; nothing is signed in yet. An encrypted address is
    /// checked against `trust`; a plain one is this machine's, the only kind [`ServerUrl::parse`] lets through.
    pub async fn connect(
        url: &ServerUrl,
        trust: &Trust,
        client_build: &str,
        platform: &str,
    ) -> Result<(Self, Events), LinkError> {
        let asked = url.as_str();
        let handshake = if url.encrypted() { Some(trust.handshake(url.authority()).await?) } else { None };
        let connector = handshake.as_ref().map(|handshake| Connector::Rustls(handshake.config()));
        let config = WebSocketConfig::default()
            .max_message_size(Some(proto::envelope::MAX_FRAME_BYTES))
            .max_frame_size(Some(proto::envelope::MAX_FRAME_BYTES));
        let request = asked.into_client_request().map_err(|failed| opening(handshake.as_ref(), failed))?;
        let opened = async {
            let stream = reach(request.uri()).await?;
            let peer = stream.peer_addr().map_err(|_| LinkError::Connect)?.ip();
            client_async_tls_with_config(request, stream, Some(config), connector)
                .await
                .map(|(socket, _)| (socket, peer))
                .map_err(|failed| opening(handshake.as_ref(), failed))
        };
        let (mut socket, peer) = timeout(OPENING, opened).await.map_err(|_| LinkError::Connect)??;
        let vouched = handshake.as_ref().map_or(Ok(Vouched::Plain), Handshake::vouched)?;
        let handshake_kind = match socket.get_ref() {
            MaybeTlsStream::Rustls(secured) => secured.get_ref().1.handshake_kind(),
            _ => None,
        };
        let Greeting { offered, greeted, build, protocol } =
            timeout(OPENING, greet(&mut socket, client_build, platform)).await.map_err(|_| LinkError::Unanswered)??;

        let (commands, inbox) = mpsc::channel(COMMAND_BACKLOG);
        let (tells, told) = mpsc::channel(COMMAND_BACKLOG);
        let (events, heard) = mpsc::channel(EVENT_BACKLOG);
        let (ending, ended) = oneshot::channel();
        let (pace, holds) = watch::channel(0);
        let (leave, leaving) = watch::channel(false);
        let (measured, round_trip) = watch::channel(None);
        let pulse = Pulse { holds, measured, leaving };
        let (offered_now, offered) = watch::channel(offered);
        tokio::spawn(run(socket, inbox, told, events, ending, pulse, offered_now));
        Ok((
            Self {
                commands,
                tells,
                offered: offered.clone(),
                greeted,
                pace: Arc::new(pace),
                leave: Arc::new(leave),
                round_trip,
                vouched,
                handshake_kind,
                token: Arc::default(),
                lane: Arc::new(files::Lane::following(url.clone(), trust.clone(), offered)),
                build,
                protocol,
                peer,
            },
            Events { events: heard, ended: Some(ended) },
        ))
    }

    /// The addresses [`Address`] says are worth trying, opened in order until one greets as a Chatkord server; that
    /// socket, which proved the address, is handed over. With none, the last failure: for a loopback host the plain
    /// address is tried last, so one that reaches nothing reads as nothing answering, not as encryption refused. A
    /// failure that lasts (about the server's key, its TLS or its protocol major) is about a server that answered, and outlives
    /// the failures after it.
    pub async fn resolve(
        found: &Address,
        trust: &Trust,
        client_build: &str,
        platform: &str,
    ) -> Result<Resolved, LinkError> {
        let mut failure = LinkError::Connect;
        for url in found.tried() {
            match Self::connect(url, trust, client_build, platform).await {
                Ok((link, events)) => {
                    return Ok(Resolved { link, events, url: url.clone() });
                }
                Err(failed) => {
                    if failure.kind() != terms::ErrorKind::Lasting {
                        failure = failed;
                    }
                }
            }
        }
        Err(failure)
    }

    /// Sends an ask and waits up to [`ASK_TIMEOUT`] for the op the schema says answers it. A
    /// refusal is an error carrying its code; an answer of another op is [`LinkError::Malformed`].
    pub async fn ask<A: Ask>(&self, ask: A) -> Result<A::Reply, LinkError> {
        self.ask_within(ask, ASK_TIMEOUT).await
    }

    /// [`Link::ask`] with its own bound, for an op the server takes longer to answer. A late
    /// reply is dropped; the connection stays open.
    pub async fn ask_within<A: Ask>(&self, ask: A, within: Duration) -> Result<A::Reply, LinkError> {
        let answered = self.ask_body_within(ask.into(), within).await?;
        A::Reply::of(answered).ok_or(LinkError::Malformed)
    }

    /// An ask whose type was erased on its way here (a screen's `Asked`): the op that answers it, for the caller to
    /// read as the reply it expects. Bounded as [`Link::ask`] is.
    pub async fn ask_body(&self, body: Body) -> Result<Body, LinkError> {
        self.ask_body_within(body, ASK_TIMEOUT).await
    }

    async fn ask_body_within(&self, body: Body, within: Duration) -> Result<Body, LinkError> {
        let (reply, answer) = oneshot::channel();
        let asked = async {
            self.commands.send(Command { body, reply }).await.map_err(|_| gone())?;
            answer.await.map_err(|_| gone())?
        };
        timeout(within, asked).await.map_err(|_| LinkError::Timeout)?
    }

    /// Sends an op nothing answers. It waits for no slot of the asks in flight and for no answer: it is written once
    /// the socket is free, or dropped with the connection.
    pub async fn tell<T: Tell>(&self, tell: T) -> Result<(), LinkError> {
        self.tells.send(tell.into()).await.map_err(|_| gone())
    }

    /// Resolves once this connection is over, closed by either end or found dead by its heartbeat, without reading
    /// its events.
    pub async fn closed(&self) {
        self.commands.closed().await;
    }

    /// Ends this connection as the client leaves, saying why where the server hears it, whoever else holds it; resolves
    /// once it is over, or after `PARTING`. A process that ends without awaiting it drops the goodbye unwritten.
    pub async fn part(&self) {
        self.leave.send_replace(true);
        let parted = self.closed();
        let _ = timeout(PARTING, parted).await;
    }

    /// Whether this connection is over.
    pub fn is_closed(&self) -> bool {
        self.commands.is_closed()
    }

    /// How long the server took to answer the last heartbeat; none before the first answer.
    pub fn round_trip(&self) -> Option<Duration> {
        *self.round_trip.borrow()
    }

    /// Beats every [`IN_CALL_HEARTBEAT_SECONDS`] and not every [`HEARTBEAT_SECONDS`] until the returned hold is dropped,
    /// and beats at once.
    pub fn quicken(&self) -> Quickened {
        self.pace.send_modify(|holds| *holds += 1);
        Quickened { pace: self.pace.clone() }
    }

    /// Whether `switch` is on now (its greeting's, as a later `ServerChanged` changed it).
    pub fn switched_on(&self, switch: Switch) -> bool {
        !self.offered.borrow().switched_off.contains(&switch)
    }

    /// Every switch an administrator has set off now.
    pub fn switched_off(&self) -> Vec<Switch> {
        self.offered.borrow().switched_off.clone()
    }

    /// The name and colour the server gave itself in its greeting.
    pub fn greeted(&self) -> &ServerIdentity {
        &self.greeted
    }

    /// The release the server named in its greeting; none when it named none.
    pub fn server_build(&self) -> Option<&ServerBuild> {
        self.build.as_ref()
    }

    /// The protocol version the server spoke in its greeting.
    pub fn server_protocol(&self) -> ProtoVersion {
        self.protocol
    }

    /// What the server takes in files now.
    pub fn file_limits(&self) -> FileLimits {
        self.offered.borrow().limits
    }

    /// What vouched for the server at the handshake: the system's verifier, the key this device trusts for it, or
    /// nothing at an unencrypted address.
    pub fn vouched(&self) -> Vouched {
        self.vouched
    }

    /// Whether the TLS handshake was whole or resumed a session this device kept for the server; none at an
    /// unencrypted address.
    pub fn handshake_kind(&self) -> Option<HandshakeKind> {
        self.handshake_kind
    }

    /// Asks to be told the server-wide events since `cursor` (`Ended::cursor` of the connection before), after signing
    /// in again; they come as events. Answers whether the server told the state whole instead.
    pub async fn resume(&self, cursor: &Cursor) -> Result<bool, LinkError> {
        {
            let resumed = self.ask(Resume { cursor: Some(cursor.bytes().to_vec()) }).await?;
            Ok(resumed.whole)
        }
    }

    /// The server's debug report as plain text: the ask is answered with a one-use secret id, and the text is
    /// downloaded with it beside this connection as a file is, at most `report::MAX_REPORT_BYTES` bytes and refused,
    /// never cut, past them. An admin's; one ask in ten seconds is `RATE_LIMITED`. The text is read here, once, as a
    /// report listed with the `newest` lines of each log, and refused `Malformed` where it is not one.
    pub async fn server_report(&self, newest: usize) -> Result<ArrivedReport, ReportError> {
        use proto::chatkord_v1::GetServerReport;
        let token = self.token.get().ok_or(ReportError::Refused(FileRefusal::Unauthenticated))?;
        let answer = self.ask(GetServerReport {}).await.map_err(ReportError::Reach)?;
        let id = ReportId::parse(answer.id).map_err(|_| ReportError::Reach(LinkError::Malformed))?;
        let text = self.lane.report(token, &id).await?;
        // Megabytes read on the pool, not on a task that serves sockets; the pool fails only a runtime that is ending.
        tokio::task::spawn_blocking(move || ArrivedReport::read(&text, newest))
            .await
            .unwrap_or(Err(ReportError::Reach(LinkError::Malformed)))
    }

    /// Every member, page after page, for an administrator; at most `MAX_MEMBER_PAGES` pages, so a server that never
    /// stops paging does not keep this asking.
    pub async fn list_members(&self) -> Result<Vec<proto::chatkord_v1::MemberEntry>, LinkError> {
        use proto::chatkord_v1::ListMembers;
        let (mut members, mut after) = (Vec::new(), None);
        for _ in 0..MAX_MEMBER_PAGES {
            let page = self.ask(ListMembers { after }).await?;
            members.extend(page.entries);
            match page.next {
                Some(next) => after = Some(next),
                None => return Ok(members),
            }
        }
        Err(LinkError::Malformed)
    }

    /// Signs in as `identity` with its password; the token issued is kept for this connection's files.
    pub async fn sign_in_with_password(&self, identity: &str, password: &Secret) -> Result<LoginAck, LinkError> {
        use proto::chatkord_v1::login::Credential;
        use proto::chatkord_v1::{Login, PasswordCredential};
        let login = Login {
            identity: identity.to_string(),
            credential: Some(Credential::Password(PasswordCredential { password: password.expose().to_string() })),
        };
        self.issued(self.ask(login).await)
    }

    /// Signs in as `identity` with a token saved earlier, kept for this connection's files.
    pub async fn sign_in_with_token(&self, identity: &str, token: &Secret) -> Result<LoginAck, LinkError> {
        use proto::chatkord_v1::login::Credential;
        use proto::chatkord_v1::{Login, TokenCredential};
        let login = Login {
            identity: identity.to_string(),
            credential: Some(Credential::Token(TokenCredential { token: token.expose().to_string() })),
        };
        let ack = self.ask(login).await?;
        let _ = self.token.set(token.clone());
        Ok(ack)
    }

    /// The token a sign-in or a registration was issued, kept for the files this connection asks for.
    fn issued(&self, ack: Result<LoginAck, LinkError>) -> Result<LoginAck, LinkError> {
        if let Some(token) = ack.as_ref().ok().and_then(|ack| ack.session_token.clone()) {
            let _ = self.token.set(Secret::new(token));
        }
        ack
    }

    /// The token a file's request shows, or the refusal of a file of `kind` asked before any sign-in.
    fn file_token(&self, kind: FileKind) -> Result<&Secret, FileError> {
        self.token.get().ok_or(FileError::Refused { kind, refusal: FileRefusal::Unauthenticated })
    }

    /// Sends a file of `kind` beside this connection, signed in as it is; the server's id for it.
    pub async fn upload(&self, kind: FileKind, bytes: Vec<u8>) -> Result<FileId, FileError> {
        self.lane.upload(self.file_token(kind)?, kind, bytes).await
    }

    /// Sends `bytes` as an attachment named `name`, as `upload_attachment` sends a file; the server's id.
    pub async fn upload_attachment_bytes(&self, name: &AttachmentName, bytes: Vec<u8>) -> Result<FileId, FileError> {
        let kind = FileKind::Attachment;
        self.lane.upload_named(self.file_token(kind)?, kind, Some(name), bytes).await
    }

    /// The bytes of file `id`, a file of `kind`, fetched beside this connection.
    pub async fn fetch(&self, kind: FileKind, id: FileId) -> Result<Vec<u8>, FileError> {
        self.lane.fetch(self.file_token(kind)?, kind, id).await
    }

    /// Sends the file at `path` as an attachment named `name`, read a chunk at a time; `progress` is told the bytes
    /// sent so far. The server's id.
    pub async fn upload_attachment(
        &self,
        name: &AttachmentName,
        path: &Path,
        progress: watch::Sender<u64>,
    ) -> Result<FileId, FileError> {
        let kind = FileKind::Attachment;
        let token = self.file_token(kind)?;
        self.lane.upload_path(token, kind, Some(name), path, progress).await.map_err(on_disk)
    }

    /// Fetches file `id`, a file of `kind`, into `path`, put in place only once whole.
    pub async fn fetch_to(&self, kind: FileKind, id: FileId, path: &Path) -> Result<(), FileError> {
        self.lane.fetch_to(self.file_token(kind)?, kind, id, path).await.map_err(on_disk)
    }

    /// Makes an account with an invite; with no display name, others see the username.
    pub async fn register(
        &self,
        identity: &str,
        display_name: Option<&str>,
        password: &Secret,
        invite_code: &Secret,
    ) -> Result<LoginAck, LinkError> {
        let register = proto::chatkord_v1::Register {
            identity: identity.to_string(),
            display_name: display_name.map(str::to_string),
            password: password.expose().to_string(),
            invite_code: invite_code.expose().to_string(),
        };
        self.issued(self.ask(register).await)
    }
}

fn gone() -> LinkError {
    LinkError::Closed(Ended::default())
}

/// Opens the connection a socket rides on. The name is looked up here: inside the socket library one that leads
/// nowhere is an unsorted I/O error, at an encrypted address read as an untrusted certificate.
pub(crate) async fn reach(uri: &Uri) -> Result<TcpStream, LinkError> {
    let host =
        uri.host().map(|host| host.trim_start_matches('[').trim_end_matches(']')).ok_or(LinkError::NoSuchHost)?;
    let port = uri.port_u16().unwrap_or(if matches!(uri.scheme_str(), Some("wss" | "https")) { 443 } else { 80 });
    let found: Vec<SocketAddr> =
        tokio::net::lookup_host((host, port)).await.map_err(|_| LinkError::NoSuchHost)?.collect();
    if found.is_empty() {
        return Err(LinkError::NoSuchHost);
    }
    connect(found).await
}

/// A connection to the first of `found` to answer.
pub(crate) async fn connect(found: Vec<SocketAddr>) -> Result<TcpStream, LinkError> {
    let stream = crate::race::first_to_answer(found, TcpStream::connect).await.ok_or(LinkError::Connect)?;
    stream.set_nodelay(true).map_err(|_| LinkError::Connect)?;
    Ok(stream)
}

/// Why opening a socket failed. The library reports an unfinished TLS handshake as an unreadable stream, not as
/// `Tls`, so at an encrypted address a failure that is not the network's is the encryption's. Its error is not
/// exhaustive: a kind it adds is nothing answering until a row here names it.
fn opening(handshake: Option<&Handshake>, failed: SocketError) -> LinkError {
    match (failed, handshake) {
        (SocketError::Tls(..), _) => LinkError::Untrusted,
        // The socket closed before any answer (a server over its accept rate drops it unread): the network's failure,
        // as the same close is when it comes as an I/O error.
        (SocketError::Protocol(ProtocolError::HandshakeIncomplete), _) => LinkError::Connect,
        (SocketError::Http(..) | SocketError::HttpFormat(..) | SocketError::Protocol(..), _) => LinkError::NotChatkord,
        (SocketError::Io(io), _) if about_the_network_io(&io) => LinkError::Connect,
        (SocketError::Io(io), Some(handshake)) => handshake.refused(Some(&io)),
        (_, Some(handshake)) => handshake.refused(None),
        (_, None) => LinkError::Connect,
    }
}

/// Whether a connection's I/O failed on the way in (nothing listening, the connection dead before any answer), not
/// for what came back: "try the other address", not "this address is not to be trusted".
pub(crate) fn about_the_network_io(io: &std::io::Error) -> bool {
    matches!(
        io.kind(),
        ErrorKind::ConnectionRefused
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkUnreachable
            | ErrorKind::TimedOut
            | ErrorKind::BrokenPipe
            | ErrorKind::ConnectionReset
            | ErrorKind::UnexpectedEof,
    )
}

/// What a server offers: the switches set off and what it takes in files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Offered {
    /// The switches set off.
    pub(crate) switched_off: Vec<Switch>,
    /// What it takes in files.
    pub(crate) limits: FileLimits,
}

impl Offered {
    /// What `told` says the server offers now.
    fn changed(told: &ServerChanged) -> Offered {
        Offered {
            switched_off: Switch::off(&told.switched_off),
            limits: FileLimits::of_wire(told.attachment_max_bytes, told.account_files_max_bytes),
        }
    }
}

/// What a server's greeting said that a client keeps.
struct Greeting {
    offered: Offered,
    greeted: ServerIdentity,
    build: Option<ServerBuild>,
    protocol: ProtoVersion,
}

/// The switches the server has off, the name and colour it gave itself and its release. A name, a colour or a release
/// out of shape is a server not keeping the protocol, refused rather than cut.
async fn greet(socket: &mut Socket, client_build: &str, platform: &str) -> Result<Greeting, LinkError> {
    let hello = proto::chatkord_v1::Handshake {
        proto_version: Some(PROTO),
        client_build: client_build.to_string(),
        platform: platform.to_string(),
    };
    send(socket, WireMessage { seq: HELLO_SEQ, ack_of: None, body: Some(Body::Handshake(hello)) })
        .await
        .map_err(|()| LinkError::Lost)?;
    loop {
        let Some(Ok(message)) = socket.next().await else { return Err(LinkError::Lost) };
        let Message::Binary(bytes) = message else { continue };
        let Frame::Op(op) = read_frame(&bytes) else { return Err(LinkError::Malformed) };
        return match op.body {
            Some(Body::ServerHello(hello)) => match hello.proto_version {
                Some(version) if !speaks_this_protocol(version) => {
                    Err(LinkError::OtherMajor(if version.major > PROTO.major {
                        ServerIs::Newer
                    } else {
                        ServerIs::Older
                    }))
                }
                Some(version) if speaks_this_protocol(version) => {
                    let identity = ServerIdentity::read(
                        hello.server_name.as_deref(),
                        hello.colour.as_deref(),
                        hello.picture.as_deref(),
                    );
                    match (identity, ServerBuild::read(&hello.server_build)) {
                        (Some(greeted), Ok(build)) => Ok(Greeting {
                            offered: Offered {
                                switched_off: Switch::off(&hello.switched_off),
                                limits: FileLimits::of_wire(hello.attachment_max_bytes, hello.account_files_max_bytes),
                            },
                            greeted,
                            build,
                            protocol: version,
                        }),
                        (None, _) | (_, Err(_)) => Err(LinkError::Malformed),
                    }
                }
                _ => Err(LinkError::Malformed),
            },
            Some(Body::Error(error)) if code_of(&error) == Some(ErrorCode::ProtoVersionMismatch) => {
                Err(LinkError::OtherMajor(ServerIs::Unsaid))
            }
            _ => Err(LinkError::Malformed),
        };
    }
}

async fn send(socket: &mut Socket, message: WireMessage) -> Result<(), ()> {
    write(socket, write_frame(&message).map_err(|_| ())?).await
}

async fn write(socket: &mut Socket, frame: Vec<u8>) -> Result<(), ()> {
    timeout(WRITE, socket.send(Message::Binary(frame.into()))).await.map_err(|_| ())?.map_err(|_| ())
}

/// What the task shares with its [`Link`]s about the heartbeat.
struct Pulse {
    /// How many [`Quickened`] are held.
    holds: watch::Receiver<usize>,
    /// Where the round trip of an answered heartbeat is put.
    measured: watch::Sender<Option<Duration>>,
    /// Set when this client leaves.
    leaving: watch::Receiver<bool>,
}

/// Who waits for the answer to a request the server has not answered. The server counts the request until it answers
/// it, so its place is kept here as long, whoever still waits.
enum Waiting {
    /// Its caller.
    Caller(Reply),
    /// Nobody: its caller stopped waiting.
    Nobody,
}

impl Waiting {
    fn answer(self, answer: Result<Body, LinkError>) {
        if let Self::Caller(reply) = self {
            let _ = reply.send(answer);
        }
    }
}

/// Resolves once a caller of a request still waiting has stopped waiting for it, and marks every such request.
async fn gave_up(pending: &mut HashMap<u64, Waiting>) {
    std::future::poll_fn(|context| {
        let mut any = false;
        for waiting in pending.values_mut() {
            if let Waiting::Caller(reply) = waiting
                && reply.poll_closed(context).is_ready()
            {
                *waiting = Waiting::Nobody;
                any = true;
            }
        }
        if any { std::task::Poll::Ready(()) } else { std::task::Poll::Pending }
    })
    .await;
}

/// The heartbeat's timer: the quick one beats at once, the slow one first after its interval.
async fn beating(quick: bool) -> tokio::time::Interval {
    let seconds = if quick { IN_CALL_HEARTBEAT_SECONDS } else { HEARTBEAT_SECONDS };
    let mut timer = interval(Duration::from_secs(seconds));
    if !quick {
        timer.tick().await;
    }
    timer
}

/// Whether the reader is told each event, or has fallen behind and is told nothing until it has made room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    Live,
    Behind,
}

/// The task that owns the socket until it ends.
async fn run(
    mut socket: Socket,
    mut inbox: mpsc::Receiver<Command>,
    mut told: mpsc::Receiver<Body>,
    events: mpsc::Sender<Queued>,
    ending: oneshot::Sender<Ended>,
    mut pulse: Pulse,
    offered: watch::Sender<Offered>,
) {
    let mut pending: HashMap<u64, Waiting> = HashMap::new();
    let mut seq = HELLO_SEQ;
    let mut quick = *pulse.holds.borrow_and_update() > 0;
    let mut heartbeat = beating(quick).await;
    // The beat sent and not yet answered: its nonce, and when it went.
    let mut flying: Option<(u64, Instant)> = None;
    let mut last_answered = Instant::now();
    let mut ended = Ended::default();
    let mut reading = Reading::Live;
    // The `Resume` of the state whole sent and not yet answered: a refusal to it is the end, an answer is nothing to
    // anyone.
    let mut resyncing: Option<u64> = None;

    // Why this client leaves, when it is this client's own choice.
    let leaving: Option<CloseReason> = loop {
        tokio::select! {
            _ = pulse.leaving.changed() => break Some(CloseReason::ClientShutdown),
            // Past the asks a server counts as unanswered, the next waits here, its caller's bound running.
            command = inbox.recv(), if pending.len() < ASKS_IN_FLIGHT => {
                let Some(Command { body, reply }) = command else { break Some(CloseReason::ClientShutdown) };
                if reply.is_closed() {
                    continue;
                }
                let Ok(frame) = write_frame(&WireMessage { seq: seq + 1, ack_of: None, body: Some(body) }) else {
                    let _ = reply.send(Err(LinkError::TooLarge));
                    continue;
                };
                seq += 1;
                if write(&mut socket, frame).await.is_err() {
                    let _ = reply.send(Err(gone()));
                    break None;
                }
                pending.insert(seq, Waiting::Caller(reply));
            }
            Some(body) = told.recv() => {
                seq += 1;
                if send(&mut socket, WireMessage { seq, ack_of: None, body: Some(body) }).await.is_err() {
                    break None;
                }
            }
            held = pulse.holds.changed() => {
                if held.is_err() {
                    break Some(CloseReason::ClientShutdown);
                }
                // A second hold, or one of two let go, leaves the beat as it is.
                let held_now = *pulse.holds.borrow_and_update() > 0;
                if held_now != quick {
                    quick = held_now;
                    heartbeat = beating(quick).await;
                }
            }
            () = gave_up(&mut pending), if !pending.is_empty() => {}
            room = events.reserve_many(CATCH_UP_ROOM), if reading == Reading::Behind => {
                let Some(permit) = room.ok().and_then(|mut room| room.next()) else {
                    break Some(CloseReason::FellBehind);
                };
                permit.send(Queued::Behind);
                reading = Reading::Live;
                seq += 1;
                resyncing = Some(seq);
                if send(&mut socket, WireMessage { seq, ack_of: None, body: Some(Body::Resume(Resume { cursor: None })) }).await.is_err() {
                    break None;
                }
            }
            _ = heartbeat.tick() => {
                if last_answered.elapsed() > Duration::from_secs(SILENT_SECONDS) {
                    ended.silent = true;
                    break None;
                }
                seq += 1;
                let ping = WireMessage { seq, ack_of: None, body: Some(Body::Heartbeat(Heartbeat { nonce: seq })) };
                flying = Some((seq, Instant::now()));
                if send(&mut socket, ping).await.is_err() {
                    break None;
                }
            }
            message = socket.next() => {
                let Some(Ok(message)) = message else { break None };
                match message {
                    Message::Binary(bytes) => {
                        last_answered = Instant::now();
                        let op = match read_frame(&bytes) {
                            Frame::Op(op) => op,
                            // An op a newer minor added: nothing here can read it and nothing here
                            // asked for it, so it is passed over rather than taken for a broken peer.
                            Frame::Refused(Refused { code: ErrorCode::UnknownOp, .. }) => continue,
                            Frame::Refused(_) => break None,
                        };
                        // Kept before the reader's room is weighed: an event dropped for a reader that is behind still
                        // moves what this link offers.
                        if let (None, Some(Body::Event(Event { kind: Some(event::Kind::ServerChanged(told)), .. }))) =
                            (&op.ack_of, &op.body)
                        {
                            offered.send_modify(|was| *was = Offered::changed(told));
                        }
                        match (op.ack_of, op.body) {
                            (_, Some(Body::Heartbeat(Heartbeat { nonce }))) => {
                                // An answer to a beat that was overtaken by the next is no measure of the link now.
                                if let Some((_, at)) = flying.take_if(|(sent, _)| *sent == nonce) {
                                    pulse.measured.send_replace(Some(last_answered.duration_since(at)));
                                }
                            }
                            // Dropped: the state whole that the reader's room asks for carries it.
                            (None, Some(Body::Event(_))) if reading == Reading::Behind => {}
                            (None, Some(Body::Event(event))) => {
                                let place = Cursor::of(&event);
                                match events.try_send(Queued::Event(event)) {
                                    // Kept once handed on: one dropped is told again, from the place before it.
                                    Ok(()) => ended.cursor = place.or(ended.cursor.take()),
                                    Err(TrySendError::Full(_)) => {
                                        report::note!(
                                            debug,
                                            "events fell behind; dropping them until the state is asked again"
                                        );
                                        reading = Reading::Behind;
                                    }
                                    Err(_) => {
                                        report::note!(warn, "events fell behind; ending the connection");
                                        break Some(CloseReason::FellBehind);
                                    }
                                }
                            }

                            (Some(to), body) if resyncing == Some(to) => {
                                resyncing = None;
                                if let Some(Body::Error(_)) = body {
                                    report::note!(warn, "the state could not be asked again; ending the connection");
                                    break Some(CloseReason::FellBehind);
                                }
                            }
                            (_, Some(Body::Error(error))) => {
                                let code = code_of(&error).unwrap_or(ErrorCode::Unspecified);
                                match op.ack_of {
                                    // An answer, kept only if its ask still waits: a late one is no end.
                                    Some(to) => {
                                        if let Some(waiting) = pending.remove(&to) {
                                            waiting.answer(Err(LinkError::Refused(code)));
                                        }
                                    }
                                    None => ended.refusal = Some(code),
                                }
                            }
                            (_, Some(Body::Close(close))) => {
                                ended.reason = proto::envelope::close_reason(close);
                                break None;
                            }
                            (Some(to), Some(body)) => {
                                if let Some(waiting) = pending.remove(&to) {
                                    waiting.answer(Ok(body));
                                }
                            }
                            _ => {}
                        }
                    }
                    Message::Close(frame) => {
                        if let Some(frame) = frame {
                            ended.reason = ended.reason.or_else(|| close_reason_of_code(u16::from(frame.code)));
                        }
                        break None;
                    }
                    _ => {}
                }
            }
        }
    };

    for (_, waiting) in pending.drain() {
        waiting.answer(Err(LinkError::Closed(ended.clone())));
    }
    if let Some(reason) = leaving {
        let goodbye = WireMessage { seq: seq + 1, ack_of: None, body: Some(Body::Close(close_frame(reason))) };
        let _ = timeout(WRITE, send(&mut socket, goodbye)).await;
    }
    let _ = timeout(WRITE, socket.close(None)).await;
    drop(events);
    let _ = ending.send(ended);
}

/// What a file sent from a path or fetched into one came to, as a file's failure.
fn on_disk(failed: files::PathError) -> FileError {
    match failed {
        files::PathError::File(failed) => failed,
        files::PathError::Disk(failed) => FileError::Disk(failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_end_waited_for_and_given_up_on_is_still_read_by_the_next_call() {
        let (queue, events) = mpsc::channel::<Queued>(1);
        let (ending, ended) = oneshot::channel();
        let mut events = Events { events, ended: Some(ended) };
        drop(queue);
        let given_up = timeout(Duration::from_millis(20), events.next()).await;
        assert!(given_up.is_err(), "the end had not come yet");
        let kicked = Ended { reason: Some(CloseReason::Kicked), ..Ended::default() };
        ending.send(kicked).expect("the reader is still there");
        let Some(Heard::Ended(ended)) = events.next().await else { panic!("the end was lost") };
        assert_eq!(ended.reason, Some(CloseReason::Kicked));
        assert!(events.next().await.is_none(), "the end is read once");
    }
}
