//! The link against a scripted server: each test is one thing the real server
//! does or might do, written as the frames it would send.

use futures_util::{SinkExt, StreamExt};
use link::{Link, Trust};
use proto::chatkord_v1::wire_message::Body;
use proto::chatkord_v1::{
    ChannelsList, CloseReason, Error, ErrorCode, Event, Heartbeat, LoginAck, Ok as Done, ProtoVersion, ServerHello,
    SetDisplayName, VoiceLeave, WireMessage, event,
};
use proto::envelope::{
    ASKS_IN_FLIGHT, Frame, HEARTBEAT_SECONDS, IN_CALL_HEARTBEAT_SECONDS, MAX_FRAME_BYTES, PROTO, SILENT_SECONDS,
    close_code, close_frame, read_frame, write_frame,
};
use std::future::Future;
use std::time::Duration;
use terms::{Address, AddressError, Heard, LinkError, Secret, ServerIs, ServerUrl};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

type Server = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

const PHRASE: &str = "correct horse battery staple";

/// A server on a loopback port that runs `script` for the one connection it takes.
async fn serve<F, Fut>(script: F) -> String
where
    F: FnOnce(Server) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let socket = tokio_tungstenite::accept_async(stream).await.expect("upgrade");
        script(socket).await;
    });
    format!("ws://{address}")
}

/// A server on a loopback port that says nothing to a connection it cannot upgrade, as the encrypted address, tried
/// first, finds with no certificate on offer.
async fn serve_untrusted<F, Fut>(script: F) -> String
where
    F: FnOnce(Server) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("accept");
            match tokio_tungstenite::accept_async(stream).await {
                Ok(socket) => {
                    script(socket).await;
                    return;
                }
                Err(_) => continue,
            }
        }
    });
    format!("ws://{address}")
}

/// Something answering at a loopback port as a plain HTTP server: a web server
/// or a proxy behind that name, which never offers this product's socket.
async fn answering_http() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address").to_string();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.expect("accept");
            // As a web server does: the request is read before the answer, and the answer ends with this side's close.
            // A socket dropped with the request unread is reset, and on Windows the reset can beat the answer there.
            tokio::spawn(async move {
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).await;
                let answer = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(answer.as_bytes()).await;
                let _ = stream.shutdown().await;
                let _ = stream.read_to_end(&mut Vec::new()).await;
            });
        }
    });
    address
}

async fn read(socket: &mut Server) -> WireMessage {
    loop {
        if let Message::Binary(bytes) = socket.next().await.expect("the client sent something").expect("a frame") {
            match read_frame(&bytes) {
                Frame::Op(op) => return op,
                Frame::Refused(refused) => panic!("the client sent a bad frame: {refused:?}"),
            }
        }
    }
}

async fn write(socket: &mut Server, message: WireMessage) {
    let bytes = write_frame(&message).expect("fits");
    socket.send(Message::Binary(bytes.into())).await.expect("sent");
}

fn reply(to: &WireMessage, body: Body) -> WireMessage {
    WireMessage { seq: 0, ack_of: Some(to.seq), body: Some(body) }
}

/// The server's half of the hello.
async fn greet(socket: &mut Server, version: ProtoVersion) {
    greeting_with(socket, version, vec![]).await;
}

/// The same hello with the switches this server says are off, by their numbers.
async fn greeting_with(socket: &mut Server, version: ProtoVersion, switched_off: Vec<i32>) {
    let hello = read(socket).await;
    assert!(matches!(hello.body, Some(Body::Handshake(_))));
    let greeting = ServerHello {
        attachment_max_bytes: 0,
        account_files_max_bytes: None,
        proto_version: Some(version),
        server_build: "test".into(),
        features: Vec::new(),
        switched_off,
        server_name: None,
        colour: None,
        picture: None,
    };
    write(socket, reply(&hello, Body::ServerHello(greeting))).await;
}

async fn connect(url: &str) -> Result<(Link, link::Events), LinkError> {
    let url = ServerUrl::parse(url).expect("a loopback address");
    Link::connect(&url, &Trust::system(), "test", "test").await
}

fn phrase() -> Secret {
    Secret::new(PHRASE.into())
}

/// An ask the schema answers with `Ok`, which these servers answer.
fn an_ask() -> VoiceLeave {
    VoiceLeave {}
}

#[tokio::test]
async fn a_client_greets_and_signs_in() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let login = read(&mut socket).await;
        assert!(matches!(login.body, Some(Body::Login(_))));
        let ack = LoginAck {
            session_token: Some("token".into()),
            display_name: "Ada".into(),
            identity: "ada".into(),
            place: proto::rules::Place::BOTTOM.wire(),
            must_change_password: false,
            grants: Vec::new(),
            user_id: "ada".into(),
            colour: None,
        };
        write(&mut socket, reply(&login, Body::LoginAck(ack))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let ack = link.sign_in_with_password("ada", &phrase()).await.expect("signed in");
    assert_eq!(ack.display_name, "Ada");
}

#[tokio::test]
async fn out_of_order_replies_reach_the_request_that_asked() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let first = read(&mut socket).await;
        let second = read(&mut socket).await;
        write(&mut socket, reply(&second, Body::Ok(Done {}))).await;
        write(&mut socket, reply(&first, Body::ChannelsSummary(Default::default()))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let (summary, done) = tokio::join!(link.ask(ChannelsList {}), link.ask(an_ask()));
    assert!(summary.is_ok() && done.is_ok(), "{summary:?} {done:?}");
}

#[tokio::test]
async fn a_refusal_is_an_error_with_its_code_and_the_link_stays_usable() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let asked = read(&mut socket).await;
        let refusal = Error { code: ErrorCode::NotFound.into() };
        write(&mut socket, reply(&asked, Body::Error(refusal))).await;
        let again = read(&mut socket).await;
        write(&mut socket, reply(&again, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let refused = link.ask(an_ask()).await.unwrap_err();
    assert_eq!(refused, LinkError::Refused(ErrorCode::NotFound));
    assert!(link.ask(an_ask()).await.is_ok());
}

#[tokio::test]
async fn a_host_learns_the_connection_ended_without_reading_its_events() {
    // A window showing the channels reads no room's events, and still has to know the connection
    // is gone rather than find out at the next press.
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let _ = socket.close(None).await;
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    tokio::time::timeout(Duration::from_secs(5), link.closed()).await.expect("the end is noticed");
    assert!(link.is_closed());
}

#[tokio::test]
async fn a_refusal_with_a_code_from_a_newer_minor_fails_that_ask_and_nothing_else() {
    // A server on a newer minor may answer with a code this build has never heard of: the ask fails as refused
    // without a known cause, and the connection goes on.
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let asked = read(&mut socket).await;
        let refusal = Error { code: 9_999 };
        write(&mut socket, reply(&asked, Body::Error(refusal))).await;
        let again = read(&mut socket).await;
        write(&mut socket, reply(&again, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let refused = link.ask(an_ask()).await.unwrap_err();
    assert_eq!(refused, LinkError::Refused(ErrorCode::Unspecified));
    assert!(link.ask(an_ask()).await.is_ok(), "the connection outlived a code it could not name");
}

#[tokio::test]
async fn a_frame_carrying_an_op_from_a_newer_minor_is_passed_over() {
    // A body a newer minor added: nothing here asked for it, so it is skipped, not taken for a broken connection.
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let asked = read(&mut socket).await;
        let mut newer = write_frame(&WireMessage { seq: 7, ack_of: None, body: None }).expect("fits");
        newer.extend_from_slice(&[0xb8, 0x3e, 0x01]); // field 999, varint 1
        socket.send(Message::Binary(newer.into())).await.expect("sent");
        write(&mut socket, reply(&asked, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    assert!(link.ask(an_ask()).await.is_ok(), "the answer after the unreadable frame still arrives");
}

#[tokio::test]
async fn events_arrive_in_order_and_the_end_is_read_with_the_servers_reason() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        for n in 1..=3 {
            let left = event::Kind::MemberLeft(proto::chatkord_v1::MemberLeft {
                user_id: n.to_string(),
                channel_id: "c".into(),
            });
            let message = WireMessage {
                seq: 0,
                ack_of: None,
                body: Some(Body::Event(Event { cursor: Vec::new(), kind: Some(left) })),
            };
            write(&mut socket, message).await;
        }
        let close = WireMessage { seq: 0, ack_of: None, body: Some(Body::Close(close_frame(CloseReason::Kicked))) };
        write(&mut socket, close).await;
        let frame = CloseFrame { code: CloseCode::from(close_code(CloseReason::Kicked)), reason: "".into() };
        let _ = socket.close(Some(frame)).await;
    })
    .await;
    let (_link, mut events) = connect(&url).await.expect("connected");
    for n in 1..=3 {
        let Some(Heard::Event(event)) = events.next().await else { panic!("no event {n}") };
        let Some(event::Kind::MemberLeft(left)) = event.kind else { panic!("not event {n}") };
        assert_eq!(left.user_id, n.to_string(), "in order");
    }
    let Some(Heard::Ended(ended)) = events.next().await else { panic!("no end") };
    assert_eq!(ended.reason, Some(CloseReason::Kicked));
    assert!(events.next().await.is_none());
}

/// `count` events, then the answer to the ask that follows them.
async fn events_then_answer(socket: &mut Server, count: u64) {
    let asked = read(socket).await;
    for n in 1..=count {
        let left =
            event::Kind::MemberLeft(proto::chatkord_v1::MemberLeft { user_id: n.to_string(), channel_id: "c".into() });
        write(
            socket,
            WireMessage {
                seq: 0,
                ack_of: None,
                body: Some(Body::Event(Event { cursor: Vec::new(), kind: Some(left) })),
            },
        )
        .await;
    }
    write(socket, reply(&asked, Body::Ok(Done {}))).await;
}

#[tokio::test]
async fn events_read_between_asks_never_fill_the_queue_and_the_end_is_still_read() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        events_then_answer(&mut socket, 200).await;
        events_then_answer(&mut socket, 200).await;
        let close = WireMessage { seq: 0, ack_of: None, body: Some(Body::Close(close_frame(CloseReason::Kicked))) };
        write(&mut socket, close).await;
        let frame = CloseFrame { code: CloseCode::from(close_code(CloseReason::Kicked)), reason: "".into() };
        let _ = socket.close(Some(frame)).await;
    })
    .await;
    let (link, mut events) = connect(&url).await.expect("connected");
    // Each answer comes after its events, so they are queued by the time it is read: 400 in all, past the queue.
    for _ in 0..2 {
        link.ask(an_ask()).await.expect("answered");
        for _ in 0..200 {
            let read = tokio::time::timeout(Duration::from_secs(1), events.next()).await;
            assert!(matches!(read, Ok(Some(Heard::Event(_)))), "every event before the answer: {read:?}");
        }
    }
    let Some(Heard::Ended(ended)) = events.next().await else { panic!("no end") };
    assert_eq!(ended.reason, Some(CloseReason::Kicked), "the server ended it, not a full queue");
}

#[tokio::test]
async fn a_session_ended_elsewhere_is_read_as_signed_out_in_its_own_sentence() {
    // What a socket whose session another ask ended is sent: the refusal a dead token gets, then the close.
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let refusal = Error { code: ErrorCode::Unauthenticated.into() };
        write(&mut socket, WireMessage { seq: 0, ack_of: None, body: Some(Body::Error(refusal)) }).await;
        let close = WireMessage { seq: 0, ack_of: None, body: Some(Body::Close(close_frame(CloseReason::SignedOut))) };
        write(&mut socket, close).await;
        let frame = CloseFrame { code: CloseCode::from(close_code(CloseReason::SignedOut)), reason: "".into() };
        let _ = socket.close(Some(frame)).await;
    })
    .await;
    let (_link, mut events) = connect(&url).await.expect("connected");
    let Some(Heard::Ended(ended)) = events.next().await else { panic!("no end") };
    assert_eq!(ended.reason, Some(CloseReason::SignedOut));
    assert!(matches!(ended.refusal, Some(ErrorCode::Unauthenticated)), "the socket was told before it was closed",);
    assert!(events.next().await.is_none(), "the end is the last thing the link says");
}

#[tokio::test]
async fn a_request_waiting_when_the_server_goes_is_answered_with_the_end() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let asked = read(&mut socket).await;
        let refusal = Error { code: ErrorCode::RateLimited.into() };
        write(&mut socket, WireMessage { seq: 0, ack_of: None, body: Some(Body::Error(refusal)) }).await;
        let _ = asked;
        let close = WireMessage { seq: 0, ack_of: None, body: Some(Body::Close(close_frame(CloseReason::Normal))) };
        write(&mut socket, close).await;
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let ended = link.ask(an_ask()).await.unwrap_err();
    assert!(matches!(&ended, LinkError::Closed(e) if e.refusal == Some(ErrorCode::RateLimited)));
}

/// What a server of another major answers: its own version, then the refusal and the close.
async fn refuse_as(socket: &mut Server, version: Option<ProtoVersion>) {
    let hello = read(socket).await;
    if let Some(version) = version {
        let ours = ServerHello { proto_version: Some(version), ..ServerHello::default() };
        write(socket, reply(&hello, Body::ServerHello(ours))).await;
    }
    let refusal = Error { code: ErrorCode::ProtoVersionMismatch.into() };
    write(socket, reply(&hello, Body::Error(refusal))).await;
    write(socket, reply(&hello, Body::Close(close_frame(CloseReason::VersionMismatch)))).await;
}

/// Another major is its own failure, never a server not reached.
#[tokio::test]
async fn a_server_on_another_major_says_which_end_is_behind() {
    let newer = ProtoVersion { major: PROTO.major + 1, minor: 0 };
    let newer = serve(move |mut socket| async move { refuse_as(&mut socket, Some(newer)).await }).await;
    assert_eq!(connect(&newer).await.err(), Some(LinkError::OtherMajor(ServerIs::Newer)));
    let older = ProtoVersion { major: PROTO.major - 1, minor: 7 };
    let older = serve(move |mut socket| async move { refuse_as(&mut socket, Some(older)).await }).await;
    assert_eq!(connect(&older).await.err(), Some(LinkError::OtherMajor(ServerIs::Older)));
    let unsaid = serve(|mut socket| async move { refuse_as(&mut socket, None).await }).await;
    assert_eq!(connect(&unsaid).await.err(), Some(LinkError::OtherMajor(ServerIs::Unsaid)));
}

#[tokio::test]
async fn a_server_that_does_not_greet_is_refused() {
    let silent = serve(|mut socket| async move {
        let _ = read(&mut socket).await;
        write(&mut socket, WireMessage { seq: 0, ack_of: None, body: Some(Body::Ok(Done {})) }).await;
    })
    .await;
    assert_eq!(connect(&silent).await.err(), Some(LinkError::Malformed));
    let newer_minor = serve(|mut socket| async move {
        greet(&mut socket, ProtoVersion { major: PROTO.major, minor: PROTO.minor + 3 }).await;
        while socket.next().await.is_some() {}
    })
    .await;
    assert!(connect(&newer_minor).await.is_ok(), "a different minor is tolerated");
}

#[tokio::test]
async fn nothing_listening_is_a_connect_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    drop(listener);
    assert_eq!(connect(&format!("ws://{address}")).await.err(), Some(LinkError::Connect));
    // Text that is not an address never reaches the network: it is refused where it is parsed.
    assert_eq!(ServerUrl::parse("not a url"), Err(AddressError::Shape));
}

/// The next ask the client sends, passing over its heartbeats; none if it sends none within `within`.
async fn asked_within(socket: &mut Server, within: Duration) -> Option<WireMessage> {
    tokio::time::timeout(within, async {
        loop {
            let asked = read(socket).await;
            if !matches!(asked.body, Some(Body::Heartbeat(_))) {
                return asked;
            }
        }
    })
    .await
    .ok()
}

#[tokio::test]
async fn the_ninth_ask_waits_for_a_place_and_goes_when_a_reply_frees_one() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let mut held = Vec::new();
        for _ in 0..ASKS_IN_FLIGHT {
            held.push(asked_within(&mut socket, Duration::from_secs(5)).await.expect("an ask"));
        }
        // An ask past the bound is refused here, so the client's ninth fails if it is sent before a place frees.
        let early = asked_within(&mut socket, Duration::from_millis(300)).await;
        let refusal = Error { code: ErrorCode::RateLimited.into() };
        if let Some(early) = &early {
            write(&mut socket, reply(early, Body::Error(refusal))).await;
        }
        write(&mut socket, reply(&held[0], Body::Ok(Done {}))).await;
        if early.is_none() {
            let ninth = asked_within(&mut socket, Duration::from_secs(5)).await.expect("the ninth");
            write(&mut socket, reply(&ninth, Body::Ok(Done {}))).await;
        }
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let waiting: Vec<_> = (0..ASKS_IN_FLIGHT)
        .map(|_| {
            let link = link.clone();
            tokio::spawn(async move { link.ask(an_ask()).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(link.ask(an_ask()).await.is_ok(), "sent once a place was free, and answered");
    for task in waiting {
        task.abort();
    }
}

#[tokio::test]
async fn an_ask_over_a_frames_size_is_refused_alone_and_the_asks_around_it_are_answered() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let before = read(&mut socket).await;
        let after = read(&mut socket).await;
        assert_eq!(after.seq, before.seq + 1, "the ask never sent took no number");
        write(&mut socket, reply(&after, Body::Ok(Done {}))).await;
        write(&mut socket, reply(&before, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let before = tokio::spawn({
        let link = link.clone();
        async move { link.ask(an_ask()).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let large = SetDisplayName { display_name: Some("a".repeat(MAX_FRAME_BYTES)) };
    assert_eq!(link.ask(large).await.unwrap_err(), LinkError::TooLarge);
    assert!(link.ask(an_ask()).await.is_ok(), "the link goes on");
    assert!(before.await.expect("joined").is_ok(), "the ask already waiting is answered");
}

#[tokio::test]
async fn a_client_with_every_place_taken_still_says_goodbye() {
    let (said, heard) = tokio::sync::oneshot::channel();
    let url = serve(|mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![]).await;
        loop {
            let message = read(&mut socket).await;
            if let Some(Body::Close(close)) = message.body {
                let _ = said.send(close.reason());
                return;
            }
        }
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let waiting: Vec<_> = (0..=ASKS_IN_FLIGHT)
        .map(|_| {
            let link = link.clone();
            tokio::spawn(async move { link.ask(an_ask()).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(100)).await;
    link.part().await;
    let reason = tokio::time::timeout(Duration::from_secs(1), heard).await.expect("said in time").expect("said");
    assert_eq!(reason, CloseReason::ClientShutdown);
    for task in waiting {
        task.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn an_ask_the_server_never_answers_times_out_and_the_link_stays_usable() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let ignored = read(&mut socket).await;
        assert!(matches!(ignored.body, Some(Body::VoiceLeave(_))));
        loop {
            let asked = read(&mut socket).await;
            match asked.body {
                Some(Body::Heartbeat(beat)) => write(&mut socket, reply(&asked, Body::Heartbeat(beat))).await,
                _ => write(&mut socket, reply(&asked, Body::Ok(Done {}))).await,
            }
        }
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let started = tokio::time::Instant::now();
    assert_eq!(link.ask(an_ask()).await.unwrap_err(), LinkError::Timeout);
    assert_eq!(started.elapsed(), link::ASK_TIMEOUT, "given up at the bound, not before or long after");
    assert!(link.ask(an_ask()).await.is_ok(), "the next ask on the same link is answered");
}

#[tokio::test(start_paused = true)]
async fn a_refusal_that_comes_after_its_ask_gave_up_is_not_the_connection_s_end() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let abandoned = read(&mut socket).await;
        // The next ask, sent once the first gave up, makes the link forget the first.
        let next = read(&mut socket).await;
        write(&mut socket, reply(&next, Body::Ok(Done {}))).await;
        let refusal = Error { code: ErrorCode::Unauthenticated.into() };
        write(&mut socket, reply(&abandoned, Body::Error(refusal))).await;
        // The network goes: no close frame, no reason.
    })
    .await;
    let (link, mut events) = connect(&url).await.expect("connected");
    assert_eq!(link.ask(an_ask()).await.unwrap_err(), LinkError::Timeout);
    assert!(link.ask(an_ask()).await.is_ok());
    let Some(Heard::Ended(ended)) = events.next().await else { panic!("no end") };
    assert_eq!(ended.refusal, None, "an answer nobody waited for is dropped, so the end is a drop that reconnects");
}

#[tokio::test(start_paused = true)]
async fn an_ask_that_timed_out_keeps_its_place_until_the_server_answers_it() {
    let (answer, mut answering) = mpsc::channel::<()>(1);
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        // The first asks, as many as may wait, are held; every one after them is answered at once.
        let mut held = Vec::new();
        loop {
            tokio::select! {
                asked = read(&mut socket) => match asked.body {
                    Some(Body::Heartbeat(beat)) => write(&mut socket, reply(&asked, Body::Heartbeat(beat))).await,
                    _ if held.len() < ASKS_IN_FLIGHT => held.push(asked),
                    _ => write(&mut socket, reply(&asked, Body::Ok(Done {}))).await,
                },
                Some(()) = answering.recv() => write(&mut socket, reply(&held[0], Body::Ok(Done {}))).await,
            }
        }
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let waiting: Vec<_> = (0..ASKS_IN_FLIGHT)
        .map(|_| {
            let link = link.clone();
            tokio::spawn(async move { link.ask(an_ask()).await })
        })
        .collect();
    for task in waiting {
        assert_eq!(task.await.expect("joined").unwrap_err(), LinkError::Timeout);
    }
    let ninth = link.ask(an_ask()).await.unwrap_err();
    assert_eq!(ninth, LinkError::Timeout, "never sent while the server still counts eight: it answers a ninth at once");
    answer.send(()).await.expect("the server runs");
    // The paused clock jumps to the nearest timer while a reply is still on the socket: a bound past the heartbeat's.
    let tenth = link.ask_within(an_ask(), Duration::from_secs(3600)).await;
    assert!(tenth.is_ok(), "the late answer freed a place: {tenth:?}");
}

#[tokio::test(start_paused = true)]
async fn a_server_that_answers_no_heartbeat_is_declared_gone_by_the_client() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        // Reads, never answers.
        while socket.next().await.is_some() {}
    })
    .await;
    let (_link, mut events) = connect(&url).await.expect("connected");
    let ended =
        tokio::time::timeout(Duration::from_secs(10 * 60), events.next()).await.expect("the client gave up in time");
    let Some(Heard::Ended(ended)) = ended else { panic!("no end") };
    assert!(ended.silent);
}

#[tokio::test(start_paused = true)]
async fn a_server_sending_only_ops_from_a_newer_minor_is_still_heard() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        // Never answers a beat; says something this build cannot read every ten seconds.
        loop {
            let mut newer = write_frame(&WireMessage { seq: 7, ack_of: None, body: None }).expect("fits");
            newer.extend_from_slice(&[0xb8, 0x3e, 0x01]); // field 999, varint 1
            socket.send(Message::Binary(newer.into())).await.expect("sent");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    })
    .await;
    let (_link, mut events) = connect(&url).await.expect("connected");
    let waited = tokio::time::timeout(Duration::from_secs(SILENT_SECONDS * 3), events.next()).await;
    assert!(waited.is_err(), "a frame nothing here can read is still the server being alive");
}

#[tokio::test(start_paused = true)]
async fn an_answered_heartbeat_keeps_the_connection() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(bytes) = message
                && let Frame::Op(op) = read_frame(&bytes)
                && let Some(Body::Heartbeat(beat)) = op.body.clone()
            {
                write(&mut socket, reply(&op, Body::Heartbeat(beat))).await;
            }
        }
    })
    .await;
    let (_link, mut events) = connect(&url).await.expect("connected");
    let waited = tokio::time::timeout(Duration::from_secs(10 * 60), events.next()).await;
    assert!(waited.is_err(), "ten minutes of answered beats and the connection is still up");
}

/// A server that answers each heartbeat after `delay`, with the nonce `shift` further on, and tells when each came.
async fn beating(delay: Duration, shift: u64) -> (String, mpsc::UnboundedReceiver<Instant>) {
    let (told, beats) = mpsc::unbounded_channel();
    let url = serve(move |mut socket| async move {
        greet(&mut socket, PROTO).await;
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(bytes) = message
                && let Frame::Op(op) = read_frame(&bytes)
                && let Some(Body::Heartbeat(mut beat)) = op.body.clone()
            {
                let _ = told.send(Instant::now());
                tokio::time::sleep(delay).await;
                beat.nonce += shift;
                write(&mut socket, reply(&op, Body::Heartbeat(beat))).await;
            }
        }
    })
    .await;
    (url, beats)
}

#[tokio::test(start_paused = true)]
async fn the_round_trip_is_the_time_the_server_took_to_answer_a_beat() {
    let (url, mut beats) = beating(Duration::from_millis(40), 0).await;
    let (link, _events) = connect(&url).await.expect("connected");
    assert_eq!(link.round_trip(), None, "nothing is measured before the first beat is answered");
    beats.recv().await.expect("the first beat");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(link.round_trip(), Some(Duration::from_millis(40)));
}

#[tokio::test(start_paused = true)]
async fn an_answer_to_another_beat_is_no_measure() {
    let (url, mut beats) = beating(Duration::from_millis(40), 100).await;
    let (link, _events) = connect(&url).await.expect("connected");
    beats.recv().await.expect("the first beat");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(link.round_trip(), None);
}

#[tokio::test(start_paused = true)]
async fn a_held_call_beats_every_five_seconds_at_once_and_a_dropped_one_every_twenty() {
    let (url, mut beats) = beating(Duration::ZERO, 0).await;
    let (link, _events) = connect(&url).await.expect("connected");
    let hold = link.quicken();
    let mut at = Vec::new();
    for _ in 0..3 {
        at.push(beats.recv().await.expect("a beat"));
    }
    let quick = Duration::from_secs(IN_CALL_HEARTBEAT_SECONDS);
    assert_eq!(at[1] - at[0], quick);
    assert_eq!(at[2] - at[1], quick);
    drop(hold);
    for _ in 0..2 {
        at.push(beats.recv().await.expect("a beat"));
    }
    let slow = Duration::from_secs(HEARTBEAT_SECONDS);
    assert_eq!(at[3] - at[2], slow, "counted from the hold's end, not from the beat before it");
    assert_eq!(at[4] - at[3], slow);
}

#[tokio::test(start_paused = true)]
async fn the_beat_stays_quick_until_the_last_hold_is_dropped() {
    let (url, mut beats) = beating(Duration::ZERO, 0).await;
    let (link, _events) = connect(&url).await.expect("connected");
    let (first, second) = (link.quicken(), link.quicken());
    let mut at = vec![beats.recv().await.expect("a beat")];
    drop(first);
    at.push(beats.recv().await.expect("a beat"));
    drop(second);
    assert_eq!(at[1] - at[0], Duration::from_secs(IN_CALL_HEARTBEAT_SECONDS), "one hold is still held");
}

#[tokio::test]
async fn an_op_that_ends_the_session_is_answered_and_then_the_socket_is_over() {
    let url = serve(|mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![]).await;
        let asked = read(&mut socket).await;
        assert!(matches!(asked.body, Some(Body::SignOut(_))), "the client asked to end its own session");
        write(&mut socket, reply(&asked, Body::Ok(Done {}))).await;
        // The server ends the socket whose session it ended, at the reason the client asked for.
        let closing =
            WireMessage { seq: 0, ack_of: None, body: Some(Body::Close(close_frame(CloseReason::ClientShutdown))) };
        write(&mut socket, closing).await;
        let frame = CloseFrame { code: CloseCode::from(close_code(CloseReason::ClientShutdown)), reason: "".into() };
        let _ = socket.close(Some(frame)).await;
    })
    .await;
    let (link, mut events) = connect(&url).await.expect("connected");
    link.ask(proto::chatkord_v1::SignOut {}).await.expect("the server says it is done");
    let Some(Heard::Ended(ended)) = events.next().await else {
        panic!("the socket did not end after its own sign-out");
    };
    assert_eq!(ended.reason, Some(CloseReason::ClientShutdown));
    assert!(matches!(link.ask(proto::chatkord_v1::SignOut {}).await, Err(LinkError::Closed(_))), "the link is over");
}

/// `count` events told without being asked, as a busy room tells them.
async fn burst(socket: &mut Server, count: usize) {
    for n in 1..=count {
        let left =
            event::Kind::MemberLeft(proto::chatkord_v1::MemberLeft { user_id: n.to_string(), channel_id: "c".into() });
        write(
            socket,
            WireMessage {
                seq: 0,
                ack_of: None,
                body: Some(Body::Event(Event { cursor: Vec::new(), kind: Some(left) })),
            },
        )
        .await;
    }
}

/// A server that sends 300 events at once, then answers each ask, and a `Resume` of the state whole with the channels
/// or, where `refuse`, with a refusal. It returns the `Close` reasons the client sent.
async fn behind_on(refuse: bool) -> (String, tokio::sync::oneshot::Receiver<Vec<CloseReason>>) {
    let (said, heard) = tokio::sync::oneshot::channel();
    let url = serve(move |mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![]).await;
        burst(&mut socket, 300).await;
        let mut reasons = Vec::new();
        while let Some(Ok(message)) = socket.next().await {
            let asked = match message {
                Message::Binary(bytes) => match read_frame(&bytes) {
                    Frame::Op(asked) => asked,
                    Frame::Refused(refused) => panic!("the client sent a bad frame: {refused:?}"),
                },
                _ => WireMessage::default(),
            };
            match &asked.body {
                Some(Body::Resume(proto::chatkord_v1::Resume { cursor: None })) if refuse => {
                    let refusal = Error { code: ErrorCode::RateLimited.into() };
                    write(&mut socket, reply(&asked, Body::Error(refusal))).await;
                }
                Some(Body::Resume(proto::chatkord_v1::Resume { cursor: None })) => {
                    let whole = event::Kind::ServerChanged(proto::chatkord_v1::ServerChanged::default());
                    let told = WireMessage {
                        seq: 0,
                        ack_of: None,
                        body: Some(Body::Event(Event { cursor: Vec::new(), kind: Some(whole) })),
                    };
                    write(&mut socket, told).await;
                    let resumed = proto::chatkord_v1::Resumed { whole: true };
                    write(&mut socket, reply(&asked, Body::Resumed(resumed))).await;
                }
                Some(Body::Close(close)) => reasons.extend(proto::envelope::close_reason(*close)),
                Some(Body::Heartbeat(_)) | None => {}
                Some(_) => write(&mut socket, reply(&asked, Body::Ok(Done {}))).await,
            }
        }
        let _ = said.send(reasons);
    })
    .await;
    (url, heard)
}

/// A reader past the queue drops what it cannot hold, is told so once it has made room, and is sent the state
/// whole; the connection stays, and answers meanwhile.
#[tokio::test]
async fn a_reader_behind_is_told_so_and_sent_the_state_whole_and_stays_connected() {
    let (url, _said) = behind_on(false).await;
    let (link, mut events) = connect(&url).await.expect("connected");
    tokio::time::sleep(Duration::from_millis(500)).await;
    link.ask(an_ask()).await.expect("answered while behind");
    for n in 0..256 {
        assert!(matches!(events.next().await, Some(Heard::Event(_))), "event {n}");
    }
    assert!(matches!(events.next().await, Some(Heard::Behind)));
    let whole = tokio::time::timeout(Duration::from_secs(2), events.next()).await.expect("the state came");
    assert!(matches!(&whole, Some(Heard::Event(Event { kind: Some(event::Kind::ServerChanged(_)), .. }))), "{whole:?}");
    link.ask(an_ask()).await.expect("still connected");
    assert!(!link.is_closed());
}

/// A server that refuses to send the state again leaves the reader's state stale: the client ends, saying why.
#[tokio::test]
async fn a_refused_resume_of_the_state_whole_ends_the_link_as_fallen_behind() {
    let (url, said) = behind_on(true).await;
    let (link, mut events) = connect(&url).await.expect("connected");
    tokio::time::sleep(Duration::from_millis(500)).await;
    while let Some(Heard::Event(_) | Heard::Behind) = events.next().await {}
    assert!(link.is_closed());
    let reasons = tokio::time::timeout(Duration::from_secs(5), said).await.expect("the socket ended").expect("said");
    assert_eq!(reasons, [CloseReason::FellBehind]);
}

/// What a client said as it left a server greeted with `serving`: the reasons of the `Close` ops it sent before its
/// socket ended. `burst` events are sent first, unread by the test.
async fn goodbye_after(serving: Vec<i32>, burst: usize, leave: impl FnOnce(Link, link::Events)) -> Vec<CloseReason> {
    let (said, heard) = tokio::sync::oneshot::channel();
    let url = serve(move |mut socket| async move {
        greeting_with(&mut socket, PROTO, serving).await;
        for n in 1..=burst {
            let left = event::Kind::MemberLeft(proto::chatkord_v1::MemberLeft {
                user_id: n.to_string(),
                channel_id: "c".into(),
            });
            write(
                &mut socket,
                WireMessage {
                    seq: 0,
                    ack_of: None,
                    body: Some(Body::Event(Event { cursor: Vec::new(), kind: Some(left) })),
                },
            )
            .await;
        }
        let mut reasons = Vec::new();
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(bytes) = message
                && let Frame::Op(WireMessage { body: Some(Body::Close(close)), .. }) = read_frame(&bytes)
            {
                reasons.extend(proto::envelope::close_reason(close));
            }
        }
        let _ = said.send(reasons);
    })
    .await;
    let (link, events) = connect(&url).await.expect("connected");
    leave(link, events);
    tokio::time::timeout(Duration::from_secs(5), heard).await.expect("the socket ended").expect("the server's word")
}

/// A client says why it leaves where the server hears it (`client-close`), and only there.
#[tokio::test]
async fn a_client_says_why_it_leaves_where_the_server_hears_it() {
    // Every handle on it dropped: it quit.
    assert_eq!(goodbye_after(Vec::new(), 0, |_, _| {}).await, [CloseReason::ClientShutdown]);
}

/// A process that ends drops its runtime and the link's task with it; parted first, the goodbye is written,
/// whoever else held the link.
#[tokio::test]
async fn a_link_parted_before_its_runtime_ends_says_goodbye() {
    let (said, heard) = tokio::sync::oneshot::channel();
    let url = serve(move |mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![]).await;
        let mut reasons = Vec::new();
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(bytes) = message
                && let Frame::Op(WireMessage { body: Some(Body::Close(close)), .. }) = read_frame(&bytes)
            {
                reasons.extend(proto::envelope::close_reason(close));
            }
        }
        let _ = said.send(reasons);
    })
    .await;
    tokio::task::spawn_blocking(move || {
        let process = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        process.block_on(async {
            let (link, _events) = connect(&url).await.expect("connected");
            let _held_elsewhere = link.clone();
            link.part().await;
            assert!(link.is_closed());
        });
    })
    .await
    .expect("the client ran");
    let reasons = tokio::time::timeout(Duration::from_secs(5), heard).await.expect("the socket ended").expect("said");
    assert_eq!(reasons, [CloseReason::ClientShutdown]);
}

#[tokio::test]
async fn a_frame_over_the_bound_ends_the_link() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let oversized = vec![0_u8; proto::envelope::MAX_FRAME_BYTES + 1];
        let _ = socket.send(Message::Binary(oversized.into())).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (_link, mut events) = connect(&url).await.expect("connected");
    let heard = tokio::time::timeout(Duration::from_secs(5), events.next()).await.expect("the link ended");
    assert!(matches!(heard, Some(Heard::Ended(_))));
}

#[tokio::test]
async fn a_bare_address_on_this_machine_reaches_the_plain_socket_that_greets() {
    let url = serve_untrusted(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let login = read(&mut socket).await;
        assert!(matches!(login.body, Some(Body::Login(_))));
        let ack = LoginAck {
            session_token: Some("token".into()),
            display_name: "Ada".into(),
            identity: "ada".into(),
            place: proto::rules::Place::BOTTOM.wire(),
            must_change_password: false,
            grants: Vec::new(),
            user_id: "ada".into(),
            colour: None,
        };
        write(&mut socket, reply(&login, Body::LoginAck(ack))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    // The host and port alone: the encrypted address is tried first and cannot greet here, so the plain one answers.
    let asked = Address::typed(url.strip_prefix("ws://").expect("a loopback address")).expect("worth trying");
    let resolved = Link::resolve(&asked, &Trust::system(), "test", "test").await.expect("greeted");
    let ack = resolved.link.sign_in_with_password("ada", &phrase()).await.expect("signed in");
    assert_eq!(ack.display_name, "Ada", "the connection that greeted is usable as it is");
    assert!(!resolved.url.encrypted(), "the route won plainly, which is what a window warns about");
}

#[tokio::test]
async fn a_bare_address_that_reaches_nothing_says_nothing_answers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address").to_string();
    drop(listener);
    let asked = Address::typed(&address).expect("worth trying");
    assert_eq!(
        Link::resolve(&asked, &Trust::system(), "test", "test").await.err(),
        Some(LinkError::Connect),
        "the plain address is the last one worth trying, so a person hears about what they typed",
    );
}

#[tokio::test]
async fn a_name_that_leads_nowhere_is_not_blamed_on_a_certificate() {
    // `.invalid` never resolves (RFC 6761): no handshake is attempted, and the name is what was wrong.
    let asked = Address::typed("chat.example.invalid").expect("worth trying");
    assert_eq!(Link::resolve(&asked, &Trust::system(), "test", "test").await.err(), Some(LinkError::NoSuchHost));
}

/// A server over its rate of accepts drops a socket before reading a byte; the client learns nothing about it, so
/// neither "not a Chatkord server" nor "unreadable" may be said.
#[tokio::test]
async fn a_socket_dropped_unread_is_said_as_nothing_answering() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            drop(socket);
        }
    });
    let url = ServerUrl::parse(&format!("ws://{address}/v1")).expect("a loopback address");
    for _ in 0..20 {
        let failed = Link::connect(&url, &Trust::system(), "test", "test").await.err();
        assert_eq!(
            failed,
            Some(LinkError::Connect),
            "a dropped socket is the network's failure, not the server's kind"
        );
    }
}

#[tokio::test]
async fn something_answering_as_plain_http_is_not_a_chatkord_server() {
    let asked = Address::typed(&answering_http().await).expect("worth trying");
    assert_eq!(
        Link::resolve(&asked, &Trust::system(), "test", "test").await.err(),
        Some(LinkError::NotChatkord),
        "the person has to stop typing that name, so the failure says something answered",
    );
}

#[tokio::test]
async fn a_claimed_secure_address_this_device_cannot_verify_is_named_as_such() {
    let address = answering_http().await;
    let asked = Address::typed(&format!("https://{address}", address = address)).expect("a claim");
    // Only the claimed address is tried and no certificate is on offer, so the handshake was refused and nothing
    // about the server was learned.
    assert_eq!(
        Link::resolve(&asked, &Trust::system(), "test", "test").await.err(),
        Some(LinkError::Untrusted),
        "the one failure a person can act on by naming a certificate",
    );
}

/// Answers every `ListMembers` with one member after the one asked for, and a next page while `pages` lasts.
async fn members_for(socket: &mut Server, pages: Option<u32>) -> u32 {
    use proto::chatkord_v1::{MemberEntry, Members};
    let mut served = 0;
    while let Some(Ok(Message::Binary(bytes))) = socket.next().await {
        let Frame::Op(asked) = read_frame(&bytes) else { panic!("not an op") };
        let Some(Body::ListMembers(list)) = asked.body.clone() else { return served };
        served += 1;
        let at: u32 = list.after.as_deref().map_or(0, |after| after.parse().expect("an id this server gave"));
        let id = (at + 1).to_string();
        let next = pages.is_none_or(|pages| served < pages).then(|| id.clone());
        let entry = MemberEntry { user_id: id, handle: "m".into(), display_name: "m".into(), ..Default::default() };
        write(socket, reply(&asked, Body::Members(Members { entries: vec![entry], next }))).await;
    }
    served
}

#[tokio::test]
async fn the_member_list_follows_each_next_page_to_the_end() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        members_for(&mut socket, Some(3)).await;
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let members = link.list_members().await.expect("listed");
    assert_eq!(members.iter().map(|m| m.user_id.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
}

#[tokio::test]
async fn a_server_that_never_stops_paging_is_not_asked_without_end() {
    let (told, served) = tokio::sync::oneshot::channel();
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let _ = told.send(members_for(&mut socket, None).await);
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    assert_eq!(link.list_members().await.unwrap_err(), LinkError::Malformed);
    drop(link);
    assert_eq!(served.await.expect("counted"), 100);
}

#[tokio::test]
async fn an_invite_is_made_listed_and_stopped_with_what_was_asked() {
    use proto::chatkord_v1::{CreateInvite, ListInvites, RevokeInvite};
    use proto::chatkord_v1::{InviteCreated, InviteSummary, Invites, Ok as Done};
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let made = read(&mut socket).await;
        let Some(Body::CreateInvite(ref create)) = made.body else { panic!("not a create") };
        assert_eq!(
            (create.uses, create.days, create.role_id.as_deref(), create.note.as_deref()),
            (5, 7, None, Some("for Bo"))
        );
        let code = "c".repeat(proto::rules::TOKEN_LEN);
        write(&mut socket, reply(&made, Body::InviteCreated(InviteCreated { code, expires_at: 42 }))).await;
        let listed = read(&mut socket).await;
        let entry = InviteSummary { invite_id: "7".into(), uses: 1, max_uses: 5, expires_at: 42, ..Default::default() };
        write(&mut socket, reply(&listed, Body::Invites(Invites { entries: vec![entry] }))).await;
        let stopped = read(&mut socket).await;
        let Some(Body::RevokeInvite(ref revoke)) = stopped.body else { panic!("not a revoke") };
        assert_eq!(revoke.invite_id, "7");
        write(&mut socket, reply(&stopped, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let create = CreateInvite { uses: 5, days: 7, role_id: None, note: Some("for Bo".into()) };
    let made = link.ask(create).await.expect("made");
    assert_eq!((made.code.len(), made.expires_at), (proto::rules::TOKEN_LEN, 42));
    assert!(!format!("{made:?}").contains(&made.code), "the code is a secret");
    let listed = link.ask(ListInvites {}).await.expect("listed").entries;
    assert_eq!(listed.iter().map(|i| (i.invite_id.as_str(), i.uses, i.max_uses)).collect::<Vec<_>>(), [("7", 1, 5)]);
    link.ask(RevokeInvite { invite_id: "7".into() }).await.expect("stopped");
}

#[tokio::test]
async fn a_channel_is_made_renamed_and_deleted_with_what_was_asked() {
    use proto::chatkord_v1::{ChannelAccess, ChannelKind, ChannelSummary, ChannelsSummary, Ok as Done};
    use proto::chatkord_v1::{ChannelCreate, ChannelDelete, ChannelRename};
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let made = read(&mut socket).await;
        let Some(Body::ChannelCreate(ref create)) = made.body else { panic!("not a create") };
        assert_eq!((create.kind, create.name.as_str()), (i32::from(ChannelKind::Voice), "Movie night"));
        assert_eq!(
            (create.category_id.as_str(), create.access, create.role_id.as_deref()),
            ("4", Some(i32::from(ChannelAccess::Private)), Some("6")),
        );
        let channel = ChannelSummary {
            channel_id: "9".into(),
            name: "Movie night".into(),
            kind: create.kind,
            category_id: create.category_id.clone(),
            ..Default::default()
        };
        write(
            &mut socket,
            reply(
                &made,
                Body::ChannelsSummary(ChannelsSummary {
                    channels: vec![channel],
                    version: 1,
                    standings: Vec::new(),
                    categories: Vec::new(),
                }),
            ),
        )
        .await;
        let renamed = read(&mut socket).await;
        let Some(Body::ChannelRename(ref rename)) = renamed.body else { panic!("not a rename") };
        assert_eq!((rename.channel_id.as_str(), rename.name.as_str()), ("9", "Big room"));
        write(&mut socket, reply(&renamed, Body::Ok(Done {}))).await;
        let deleted = read(&mut socket).await;
        let Some(Body::ChannelDelete(ref delete)) = deleted.body else { panic!("not a delete") };
        assert_eq!(delete.channel_id, "9");
        write(&mut socket, reply(&deleted, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let create = ChannelCreate {
        kind: ChannelKind::Voice.into(),
        name: "Movie night".into(),
        category_id: "4".into(),
        access: Some(ChannelAccess::Private.into()),
        role_id: Some("6".into()),
    };
    let made = link.ask(create).await.expect("made");
    assert_eq!(made.channels.iter().map(|c| c.channel_id.as_str()).collect::<Vec<_>>(), ["9"]);
    link.ask(ChannelRename { channel_id: "9".into(), name: "Big room".into() }).await.expect("renamed");
    link.ask(ChannelDelete { channel_id: "9".into() }).await.expect("deleted");
}

#[tokio::test]
async fn a_member_is_muted_taken_out_and_overwritten_by_whom_was_asked() {
    use proto::chatkord_v1::{Ok as Done, OverwriteState, set_channel_overwrite::Whom};
    use proto::chatkord_v1::{SetChannelOverwrite, VoiceMuteMember, VoiceTakeOut};
    const BO: &str = "bo";
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let muted = read(&mut socket).await;
        let Some(Body::VoiceMuteMember(ref mute)) = muted.body else { panic!("not a mute") };
        assert_eq!((mute.user_id.as_str(), mute.muted), (BO, true));
        write(&mut socket, reply(&muted, Body::Ok(Done {}))).await;
        let taken = read(&mut socket).await;
        let Some(Body::VoiceTakeOut(ref out)) = taken.body else { panic!("not a take out") };
        assert_eq!(out.user_id, BO);
        write(&mut socket, reply(&taken, Body::Ok(Done {}))).await;
        let set = read(&mut socket).await;
        let Some(Body::SetChannelOverwrite(ref overwrite)) = set.body else { panic!("not an overwrite") };
        assert_eq!(
            (overwrite.channel_id.as_str(), overwrite.whom.clone(), overwrite.word.as_str(), overwrite.state),
            ("9", Some(Whom::UserId(BO.into())), "SEND_MESSAGE", i32::from(OverwriteState::Deny)),
        );
        write(&mut socket, reply(&set, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    link.ask(VoiceMuteMember { user_id: BO.into(), muted: true }).await.expect("muted");
    link.ask(VoiceTakeOut { user_id: BO.into() }).await.expect("taken out");
    let overwrite = SetChannelOverwrite {
        channel_id: "9".into(),
        whom: Some(Whom::UserId(BO.into())),
        word: proto::rules::Grant::SendMessage.word().into(),
        state: OverwriteState::Deny.into(),
    };
    link.ask(overwrite).await.expect("overwritten");
}

/// A server that greets with `name` and then waits for the client to go.
async fn named(name: Option<String>) -> String {
    greeting_as(name, None).await
}

/// A server that greets with `name` and `colour` and then waits for the client to go.
async fn greeting_as(name: Option<String>, colour: Option<String>) -> String {
    serve_untrusted(move |mut socket| async move {
        let hello = read(&mut socket).await;
        let greeting = ServerHello {
            attachment_max_bytes: 0,
            account_files_max_bytes: None,
            proto_version: Some(proto::envelope::PROTO),
            server_build: "test".into(),
            features: Vec::new(),
            switched_off: Vec::new(),
            server_name: name,
            colour,
            picture: None,
        };
        write(&mut socket, reply(&hello, Body::ServerHello(greeting))).await;
        while socket.next().await.is_some() {}
    })
    .await
}

async fn resolved(url: String) -> Result<link::Resolved, LinkError> {
    let typed = Address::typed(url.trim_start_matches("ws://")).expect("an address");
    Link::resolve(&typed, &Trust::system(), "test", "test").await
}

#[tokio::test]
async fn the_name_a_server_greets_with_is_kept_and_a_blank_one_is_none() {
    let found = resolved(named(Some("Night Owls".into())).await).await.expect("greeted");
    assert_eq!(found.server_name(), Some("Night Owls"));
    let found = resolved(named(Some("  ".into())).await).await.expect("greeted");
    assert_eq!(found.server_name(), None);
    let found = resolved(named(None).await).await.expect("greeted");
    assert_eq!(found.server_name(), None);
}

#[tokio::test]
async fn a_name_longer_than_any_name_is_a_server_not_keeping_the_protocol() {
    let long = "N".repeat(proto::rules::MAX_NAME_CHARS + 1);
    assert!(matches!(resolved(named(Some(long)).await).await, Err(LinkError::Malformed)));
    let longest = "N".repeat(proto::rules::MAX_NAME_CHARS);
    assert!(resolved(named(Some(longest)).await).await.is_ok());
    let reordering = "Night\u{202E}Owls".to_string();
    assert!(matches!(resolved(named(Some(reordering)).await).await, Err(LinkError::Malformed)), "a direction control");
}

#[tokio::test]
async fn the_colour_a_server_greets_with_is_kept_in_its_one_spelling_and_a_bad_one_is_refused() {
    let found = resolved(greeting_as(None, Some("#3d7fd6".into())).await).await.expect("greeted");
    assert_eq!(found.link.greeted().colour.map(|colour| colour.to_string()).as_deref(), Some("#3D7FD6"));
    let plaid = resolved(greeting_as(Some("Night Owls".into()), Some("plaid".into())).await).await;
    assert!(matches!(plaid, Err(LinkError::Malformed)));
}

/// A server that greets with `build` and then waits for the client to go.
async fn built(build: String) -> String {
    serve_untrusted(move |mut socket| async move {
        let hello = read(&mut socket).await;
        let greeting =
            ServerHello { proto_version: Some(proto::envelope::PROTO), server_build: build, ..ServerHello::default() };
        write(&mut socket, reply(&hello, Body::ServerHello(greeting))).await;
        while socket.next().await.is_some() {}
    })
    .await
}

#[tokio::test]
async fn the_build_a_server_greets_with_is_kept_and_one_past_the_bound_is_refused() {
    let found = resolved(built(" 0.9.0 ".into()).await).await.expect("greeted");
    assert_eq!(found.link.server_build().map(terms::ServerBuild::as_str), Some("0.9.0"));
    let found = resolved(built(String::new()).await).await.expect("greeted");
    assert_eq!(found.link.server_build(), None, "an empty build is none");
    let longest = "9".repeat(proto::envelope::MAX_HANDSHAKE_TEXT);
    assert!(resolved(built(longest).await).await.is_ok());
    let long = "9".repeat(proto::envelope::MAX_HANDSHAKE_TEXT + 1);
    assert!(matches!(resolved(built(long).await).await, Err(LinkError::Malformed)));
}

#[tokio::test]
async fn a_reconnect_sends_resume_with_the_last_cursor() {
    const PLACE: [u8; 36] = [9; 36];
    let first = serve(|mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![]).await;
        let renamed = event::Kind::MemberRenamed(proto::chatkord_v1::MemberRenamed {
            user_id: "1".into(),
            display_name: "Grace".into(),
        });
        let told = Event { cursor: PLACE.to_vec(), kind: Some(renamed) };
        write(&mut socket, WireMessage { seq: 0, ack_of: None, body: Some(Body::Event(told)) }).await;
        let room = event::Kind::VoiceOccupancy(proto::chatkord_v1::VoiceOccupancy::default());
        let unplaced = Event { cursor: Vec::new(), kind: Some(room) };
        write(&mut socket, WireMessage { seq: 0, ack_of: None, body: Some(Body::Event(unplaced)) }).await;
        let _ = socket.close(None).await;
    })
    .await;
    let (_link, mut events) = connect(&first).await.expect("connected");
    let mut ended = None;
    while let Some(heard) = events.next().await {
        if let Heard::Ended(end) = heard {
            ended = Some(end);
        }
    }
    let cursor = ended.and_then(|ended| ended.cursor).expect("the place of the last event told in order");
    let second = serve(|mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![]).await;
        let asked = read(&mut socket).await;
        let Some(Body::Resume(resume)) = &asked.body else { panic!("not a resume: {asked:?}") };
        assert_eq!(resume.cursor.as_deref(), Some(PLACE.as_slice()), "handed back as the server sent it");
        write(&mut socket, reply(&asked, Body::Resumed(proto::chatkord_v1::Resumed { whole: false }))).await;
    })
    .await;
    let (link, _events) = connect(&second).await.expect("connected again");
    assert!(!link.resume(&cursor).await.expect("answered"), "told what it missed, not the state whole");
}

#[tokio::test]
async fn an_answer_of_another_op_fails_that_ask_alone() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let wrong = read(&mut socket).await;
        write(&mut socket, reply(&wrong, Body::ChannelsSummary(Default::default()))).await;
        let right = read(&mut socket).await;
        write(&mut socket, reply(&right, Body::Ok(Done {}))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    assert_eq!(link.ask(an_ask()).await.unwrap_err(), LinkError::Malformed, "the schema answers it with Ok");
    assert!(link.ask(an_ask()).await.is_ok(), "and the link goes on");
}

#[tokio::test]
async fn a_tell_is_sent_past_every_ask_in_flight_and_waits_for_nothing() {
    let (heard, hearing) = oneshot::channel();
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        for _ in 0..proto::envelope::ASKS_IN_FLIGHT {
            read(&mut socket).await;
        }
        let _ = heard.send(read(&mut socket).await.body);
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let asks: Vec<_> = (0..proto::envelope::ASKS_IN_FLIGHT)
        .map(|_| {
            let link = link.clone();
            tokio::spawn(async move { link.ask(an_ask()).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(100)).await;
    link.tell(Heartbeat { nonce: 7 }).await.expect("told");
    let told =
        tokio::time::timeout(Duration::from_secs(2), hearing).await.expect("the tell went though no slot was free");
    assert_eq!(told.expect("heard"), Some(Body::Heartbeat(Heartbeat { nonce: 7 })));
    for ask in asks {
        ask.abort();
    }
}

#[tokio::test]
async fn a_server_that_drops_the_connection_while_greeting_is_lost_not_another_version() {
    let dropped = serve(|mut socket| async move {
        let _ = read(&mut socket).await;
        let _ = socket.close(None).await;
    })
    .await;
    assert_eq!(connect(&dropped).await.err(), Some(LinkError::Lost));
}

#[tokio::test]
async fn a_switch_this_build_does_not_know_is_passed_over() {
    let url = serve(|mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![99]).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("a switch it does not know is no reason to refuse");
    assert!(link.switched_off().is_empty());
}

#[tokio::test]
async fn each_talk_op_goes_out_as_its_own_frame_and_is_answered_by_the_reply_it_names() {
    use proto::chatkord_v1::{
        Activity, Conversation, Conversations, FollowThread, FollowedThreads, ListConversations, ListFollowed,
        OpenConversation,
    };
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let asked = read(&mut socket).await;
        assert!(matches!(&asked.body, Some(Body::OpenConversation(OpenConversation { user_id })) if user_id == "bo"));
        let conversation = Conversation {
            conversation_id: "c1".into(),
            timeline_id: "t1".into(),
            user_ids: vec!["ada".into(), "bo".into()],
            revision: 1,
            standing: None,
            last: None,
            sendable: true,
        };
        write(&mut socket, reply(&asked, Body::Conversation(conversation))).await;
        let asked = read(&mut socket).await;
        assert!(matches!(asked.body, Some(Body::ListConversations(ListConversations {}))));
        write(&mut socket, reply(&asked, Body::Conversations(Conversations::default()))).await;
        let asked = read(&mut socket).await;
        assert!(matches!(
            &asked.body,
            Some(Body::FollowThread(FollowThread { timeline_id, follow: true })) if timeline_id == "t2"
        ));
        write(&mut socket, reply(&asked, Body::Ok(Done {}))).await;
        let asked = read(&mut socket).await;
        assert!(matches!(asked.body, Some(Body::ListFollowed(ListFollowed {}))));
        write(&mut socket, reply(&asked, Body::FollowedThreads(FollowedThreads::default()))).await;
        let told = read(&mut socket).await;
        assert!(matches!(told.body, Some(Body::Activity(Activity { idle: true }))), "a tell: nothing answers");
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let bo = terms::MemberId::parse("bo").unwrap();
    let opened = link.ask(OpenConversation { user_id: bo.as_str().to_owned() }).await.expect("opened");
    assert_eq!(opened.timeline_id, "t1");
    assert!(link.ask(ListConversations {}).await.expect("listed").conversations.is_empty());
    let timeline = terms::TimelineId::parse("t2").unwrap();
    link.ask(FollowThread { timeline_id: timeline.as_str().to_owned(), follow: true }).await.expect("followed");
    assert!(link.ask(ListFollowed {}).await.expect("listed").threads.is_empty());
    link.tell(Activity { idle: true }).await.expect("told");
}

#[tokio::test]
async fn a_server_changed_replaces_the_switches_and_caps_the_link_holds() {
    use proto::chatkord_v1::{ServerChanged, Switch as WireSwitch};
    use proto::rules::Switch;
    let url = serve(|mut socket| async move {
        greeting_with(&mut socket, PROTO, vec![WireSwitch::Threads as i32]).await;
        let now = ServerChanged {
            switched_off: vec![WireSwitch::Search as i32],
            attachment_max_bytes: 9,
            ..ServerChanged::default()
        };
        let changed = |told| {
            let body = Body::Event(Event { cursor: Vec::new(), kind: Some(event::Kind::ServerChanged(told)) });
            WireMessage { seq: 0, ack_of: None, body: Some(body) }
        };
        write(&mut socket, changed(now)).await;
        // The second change waits for the client's tell, so the first is read before the second replaces it.
        let _ = read(&mut socket).await;
        write(&mut socket, changed(ServerChanged::default())).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, mut events) = connect(&url).await.expect("connected");
    assert!(link.switched_on(Switch::Search) && !link.switched_on(Switch::Threads));
    assert!(matches!(events.next().await, Some(Heard::Event(_))));
    assert!(link.switched_on(Switch::Threads) && !link.switched_on(Switch::Search), "replaced by the first");
    assert_eq!(link.file_limits().attachment, Some(9), "and its caps");
    link.tell(proto::chatkord_v1::Activity { idle: true }).await.expect("told");
    assert!(matches!(events.next().await, Some(Heard::Event(_))));
    assert!(link.switched_off().is_empty(), "the second turned everything on");
}

#[tokio::test]
async fn a_search_and_a_preview_go_out_as_the_schema_names_them_and_are_answered_by_the_reply_it_names() {
    use proto::chatkord_v1::{Found, LinkPreview, Message, Search, SearchPage};
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        let asked = read(&mut socket).await;
        let Some(Body::Search(Search { query, timeline_id: None, before: None })) = &asked.body else {
            panic!("not an open search: {asked:?}")
        };
        assert_eq!(query, "light house", "the words typed");
        let found = Found {
            message: Some(Box::new(Message::default())),
            channel_id: None,
            root_message_id: None,
            file_ids: Vec::new(),
            in_body: true,
        };
        write(&mut socket, reply(&asked, Body::SearchPage(SearchPage { found: vec![found], older: true }))).await;
        let asked = read(&mut socket).await;
        let Some(Body::Search(Search { timeline_id: Some(timeline), before: Some(before), .. })) = &asked.body else {
            panic!("not a scoped search: {asked:?}")
        };
        assert_eq!((timeline.as_str(), before.message_id.as_str()), ("t1", "m9"));
        write(&mut socket, reply(&asked, Body::SearchPage(SearchPage::default()))).await;
        let asked = read(&mut socket).await;
        let Some(Body::SendMessage(send)) = &asked.body else { panic!("not a send: {asked:?}") };
        assert_eq!(send.preview.as_deref().map(|preview| preview.title.as_str()), Some("A page"));
        write(&mut socket, reply(&asked, Body::Message(Message::default()))).await;
        let asked = read(&mut socket).await;
        let Some(Body::EditMessage(edit)) = &asked.body else { panic!("not an edit: {asked:?}") };
        assert_eq!(edit.preview, None, "none drops it");
        write(&mut socket, reply(&asked, Body::Message(Message::default()))).await;
        let asked = read(&mut socket).await;
        let Some(Body::SendMessage(send)) = &asked.body else { panic!("not a send: {asked:?}") };
        assert!(send.preview.is_none() && send.thread_of.is_none(), "a plain send carries neither");
        write(&mut socket, reply(&asked, Body::Message(Message::default()))).await;
        let asked = read(&mut socket).await;
        let Some(Body::SendMessage(send)) = &asked.body else { panic!("not a send: {asked:?}") };
        assert_eq!(send.thread_of.as_deref(), Some("m4"), "the first reply of a thread names its root");
        write(&mut socket, reply(&asked, Body::Message(Message::default()))).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    let query = proto::rules::SearchQuery::parse("  light   house ").expect("a query");
    let words = query.words().join(" ");
    let page = link.ask(Search { query: words.clone(), timeline_id: None, before: None }).await.expect("searched");
    assert_eq!((page.found.len(), page.older), (1, true));
    let (timeline, before) = (terms::TimelineId::parse("t1").unwrap(), terms::MessageId::parse("m9").unwrap());
    let within = Search {
        query: words,
        timeline_id: Some(timeline.as_str().to_owned()),
        before: Some(proto::chatkord_v1::SearchBefore { message_id: before.as_str().to_owned() }),
    };
    assert!(link.ask(within).await.expect("searched").found.is_empty());
    let sending = || link::Sending {
        timeline: &timeline,
        thread_of: None,
        body: "see https://example.invalid/a",
        spans: &[],
        files: &[],
        reply: None,
        nonce: terms::Nonce::new([1; 16]),
    };
    let preview =
        LinkPreview { url: "https://example.invalid/a".into(), title: "A page".into(), ..LinkPreview::default() };
    link.send_with_preview(sending(), Some(preview)).await.expect("sent");
    link.edit_with_preview(&terms::MessageId::parse("m1").unwrap(), "see https://example.invalid/a", &[], None)
        .await
        .expect("edited");
    link.send_with_preview(sending(), None).await.expect("sent");
    let root = terms::MessageId::parse("m4").unwrap();
    link.send_with_preview(link::Sending { thread_of: Some(&root), ..sending() }, None).await.expect("sent");
}

/// What a test server's file route saw: the path and the `Authorization` header of each GET.
type Gets = std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>;

/// A server on a loopback port whose first connection is the socket `script` runs and whose later ones are HTTP,
/// answering `GET /v1/files/reports/<id>` with `text` for `id` and `404` for any other, and noting each request.
async fn serve_with_reports<F, Fut>(script: F, id: &'static str, text: Vec<u8>) -> (String, Gets)
where
    F: FnOnce(Server) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    use hyper::service::service_fn;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let gets: Gets = Gets::default();
    let seen = gets.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let socket = tokio_tungstenite::accept_async(stream).await.expect("upgrade");
        tokio::spawn(script(socket));
        while let Ok((stream, _)) = listener.accept().await {
            let (seen, text) = (seen.clone(), text.clone());
            let answer = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                let (seen, text) = (seen.clone(), text.clone());
                async move {
                    let bearer = request.headers().get("authorization").and_then(|value| value.to_str().ok());
                    let path = request.uri().path().to_owned();
                    seen.lock().unwrap().push((path.clone(), bearer.map(str::to_owned)));
                    let found = path == format!("/v1/files/reports/{id}");
                    let mut response =
                        hyper::Response::new(http_body_util::Full::new(hyper::body::Bytes::from(if found {
                            text
                        } else {
                            Vec::new()
                        })));
                    *response.status_mut() = if found { hyper::StatusCode::OK } else { hyper::StatusCode::NOT_FOUND };
                    Ok::<_, std::convert::Infallible>(response)
                }
            });
            let connection = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), answer);
            tokio::spawn(async move {
                let _ = connection.await;
            });
        }
    });
    (format!("ws://{address}"), gets)
}

#[tokio::test]
async fn the_servers_report_is_asked_then_downloaded_with_its_id_and_the_session_token() {
    use proto::chatkord_v1::{GetServerReport, ServerReport};
    const REPORT: &str = "Chatkord debug report: server\nCreated now\n\nStore\n  users: 3\n";
    const LISTED: usize = 5;
    const ID: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";
    const GONE: &str = "ZyXwVuTsRqPoNmLkJiHgFeDcBa9876543210_-ZyXwV";
    let (url, gets) = serve_with_reports(
        |mut socket| async move {
            greet(&mut socket, PROTO).await;
            let login = read(&mut socket).await;
            let ack = LoginAck {
                session_token: Some("token".into()),
                display_name: "Ada".into(),
                identity: "ada".into(),
                place: proto::rules::Place::BOTTOM.wire(),
                must_change_password: false,
                grants: Vec::new(),
                user_id: "ada".into(),
                colour: None,
            };
            write(&mut socket, reply(&login, Body::LoginAck(ack))).await;
            for id in [ID, GONE, "short"] {
                let asked = read(&mut socket).await;
                let Some(Body::GetServerReport(GetServerReport {})) = &asked.body else {
                    panic!("not a report ask: {asked:?}")
                };
                write(&mut socket, reply(&asked, Body::ServerReport(ServerReport { id: id.into() }))).await;
            }
            let asked = read(&mut socket).await;
            let refusal = Error { code: ErrorCode::RateLimited.into() };
            write(&mut socket, reply(&asked, Body::Error(refusal))).await;
            while socket.next().await.is_some() {}
        },
        ID,
        REPORT.as_bytes().to_vec(),
    )
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    assert_eq!(
        link.server_report(LISTED).await,
        Err(terms::ReportError::Refused(proto::files::FileRefusal::Unauthenticated)),
        "before sign-in there is no token to show, and nothing is asked"
    );
    link.sign_in_with_password("ada", &phrase()).await.expect("signed in");
    let arrived = link.server_report(LISTED).await.map(terms::ArrivedReport::into_parts);
    assert_eq!(arrived.as_ref().map(|(text, _)| &**text), Ok(REPORT));
    assert_eq!(arrived.map(|(_, listing)| listing), Ok(report::Listing::read(REPORT, LISTED).expect("a report")));
    assert_eq!(*gets.lock().unwrap(), [(format!("/v1/files/reports/{ID}"), Some("Bearer token".to_owned()))]);
    assert_eq!(
        link.server_report(LISTED).await,
        Err(terms::ReportError::Refused(proto::files::FileRefusal::NotFound)),
        "an id the server does not know is a 404"
    );
    assert_eq!(
        link.server_report(LISTED).await,
        Err(terms::ReportError::Reach(LinkError::Malformed)),
        "an id that is not 43 characters of base64url is never put in a path"
    );
    assert_eq!(gets.lock().unwrap().len(), 2, "the malformed id asked nothing");
    assert_eq!(
        link.server_report(LISTED).await,
        Err(terms::ReportError::Reach(LinkError::Refused(ErrorCode::RateLimited)))
    );
}

#[tokio::test]
async fn a_server_that_says_no_switch_is_off_has_every_switch_on() {
    let url = serve(|mut socket| async move {
        greet(&mut socket, PROTO).await;
        while socket.next().await.is_some() {}
    })
    .await;
    let (link, _events) = connect(&url).await.expect("connected");
    assert!(proto::rules::Switch::ALL.into_iter().all(|switch| link.switched_on(switch)));
}
