//! The link's trust against a server on this machine that presents a certificate no authority signed, as a server
//! that made its own does: the key a person is asked to trust, and what each later handshake does with it.

use futures_util::{SinkExt, StreamExt};
use link::{HandshakeKind, Link, Trust, Vouched};
use proto::chatkord_v1::wire_message::Body;
use proto::chatkord_v1::{ServerHello, WireMessage};
use proto::envelope::{Frame, PROTO, read_frame, write_frame};
use proto::pin::KeyPin;
use rcgen::{CertifiedKey, PublicKeyData};
use rustls::SupportedProtocolVersion;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use terms::{Address, LinkError, ServerUrl};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

/// A server on a loopback port speaking `versions` of TLS with a self-signed certificate, greeting every connection
/// that gets through; its address and the pin of its key, hashed apart from the client's code.
async fn serve(versions: &[&'static SupportedProtocolVersion]) -> (SocketAddr, KeyPin) {
    let CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let digest = ring::digest::digest(&ring::digest::SHA256, &signing_key.subject_public_key_info());
    let pin = KeyPin::from_digest(digest.as_ref().try_into().unwrap());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(versions)
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(greet(acceptor.clone(), stream));
        }
    });
    (address, pin)
}

/// One connection's handshake and hello, as far as the client lets it get.
async fn greet(acceptor: TlsAcceptor, stream: TcpStream) -> Option<()> {
    let secured = acceptor.accept(stream).await.ok()?;
    let mut socket = tokio_tungstenite::accept_async(secured).await.ok()?;
    let hello = loop {
        if let Message::Binary(bytes) = socket.next().await?.ok()?
            && let Frame::Op(op) = read_frame(&bytes)
        {
            break op;
        }
    };
    let greeting = ServerHello {
        attachment_max_bytes: 0,
        account_files_max_bytes: None,
        proto_version: Some(PROTO),
        server_build: "test".into(),
        features: Vec::new(),
        switched_off: Vec::new(),
        server_name: None,
        colour: None,
        picture: None,
    };
    let answer = WireMessage { seq: 0, ack_of: Some(hello.seq), body: Some(Body::ServerHello(greeting)) };
    socket.send(Message::Binary(write_frame(&answer).ok()?.into())).await.ok()?;
    while socket.next().await.is_some() {}
    Some(())
}

async fn connect(address: SocketAddr, pin: Option<KeyPin>) -> Result<Link, LinkError> {
    let url = ServerUrl::parse(&format!("wss://{address}/v1")).unwrap();
    Link::connect(&url, &Trust::system().for_server(pin), "test", "test").await.map(|(link, _events)| link)
}

#[tokio::test]
async fn a_self_signed_server_is_refused_with_the_pin_of_its_own_key_then_greets_once_that_key_is_trusted() {
    let (address, pin) = serve(&[&rustls::version::TLS13]).await;
    assert_eq!(connect(address, None).await.err(), Some(LinkError::Unvouched(pin)), "the server's own SPKI hash");
    let link = connect(address, Some(pin)).await.expect("the trusted key greets");
    assert_eq!(link.vouched(), Vouched::Pin);
}

#[tokio::test]
async fn a_server_presenting_another_key_than_the_one_trusted_is_refused_naming_both() {
    let (address, presented) = serve(&[&rustls::version::TLS13]).await;
    let (_, trusted) = serve(&[&rustls::version::TLS13]).await;
    assert_eq!(connect(address, Some(trusted)).await.err(), Some(LinkError::KeyChanged { trusted, presented }));
}

#[tokio::test]
async fn a_server_speaking_only_tls_1_2_is_refused_as_too_old() {
    let (address, pin) = serve(&[&rustls::version::TLS12]).await;
    assert_eq!(connect(address, None).await.err(), Some(LinkError::OldEncryption));
    assert_eq!(connect(address, Some(pin)).await.err(), Some(LinkError::OldEncryption), "a trusted key too");
}

#[tokio::test]
async fn a_typed_address_of_a_self_signed_server_here_asks_for_trust_not_says_nothing_answers() {
    // The plain address, tried after the encrypted one, reaches a server speaking only TLS: its failure is not the
    // one to tell.
    let (address, pin) = serve(&[&rustls::version::TLS13]).await;
    let typed = Address::typed(&address.to_string()).unwrap();
    assert_eq!(typed.tried().len(), 2, "encrypted, then plainly");
    let found = Link::resolve(&typed, &Trust::system(), "test", "test").await;
    assert_eq!(found.err(), Some(LinkError::Unvouched(pin)));
    let found = Link::resolve(&typed, &Trust::system().for_server(Some(pin)), "test", "test").await.unwrap();
    assert!(found.url.encrypted() && found.link.vouched() == Vouched::Pin);
}

#[tokio::test]
async fn a_later_connect_with_the_same_trust_resumes_and_keeps_what_vouched_for_the_server() {
    let (address, pin) = serve(&[&rustls::version::TLS13]).await;
    let url = ServerUrl::parse(&format!("wss://{address}/v1")).unwrap();
    let trust = Trust::system().for_server(Some(pin));
    let (first, _events) = Link::connect(&url, &trust, "test", "test").await.unwrap();
    assert_eq!((first.handshake_kind(), first.vouched()), (Some(HandshakeKind::Full), Vouched::Pin));
    let (again, _events) = Link::connect(&url, &trust.clone(), "test", "test").await.unwrap();
    assert_eq!(again.handshake_kind(), Some(HandshakeKind::Resumed), "no second verify, no second chain check");
    assert_eq!(again.vouched(), Vouched::Pin, "the session the pin vouched for");
}
