//! What the tests share: keys made for the test, packages, and a feed on this machine's loopback.

use crate::key::SigningKey;
use crate::package::{Digest, Package, PackageName, Platform};
use data_encoding::BASE64;
use http_body_util::channel::Channel as Streamed;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_LENGTH, LOCATION};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use link::Trust;
use proto::pin::KeyPin;
use rcgen::{CertifiedKey, PublicKeyData};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// A release key made for one test.
pub(crate) fn signing_key() -> SigningKey {
    let der = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    SigningKey::from_base64(&BASE64.encode(der.as_ref())).unwrap()
}

/// `platform`'s package `file` holding `bytes`.
pub(crate) fn package(platform: Platform, file: &str, bytes: &[u8]) -> Package {
    let digest = Digest::of(&ring::digest::digest(&ring::digest::SHA256, bytes)).unwrap();
    let size = u64::try_from(bytes.len()).unwrap();
    Package::new(PackageName::parse(file, platform).unwrap(), size, digest).unwrap()
}

/// How the feed answers one path.
#[derive(Clone, Debug)]
pub(crate) enum Reply {
    /// 200 with these bytes, their length stated.
    Bytes(Vec<u8>),
    /// 200 with these bytes in parts, no length stated.
    Parts(Vec<u8>),
    /// 200 stating this length, then these bytes.
    Stating(u64, Vec<u8>),
    /// 200 and a head, then nothing, the connection held open.
    Stall,
    /// A redirect to this location.
    Redirect(String),
    /// This status and no body.
    Status(u16),
}

/// A feed on a loopback port: its routes, settable while it runs, and the paths it was asked for.
#[derive(Clone, Default)]
pub(crate) struct Feed {
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Feed {
    /// `path` answered with `reply`.
    pub(crate) fn route(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_string(), reply);
    }

    /// The paths asked for, in order.
    pub(crate) fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    /// Serves this feed over TLS 1.3 with `certificate` on a new loopback port; its address.
    pub(crate) async fn serve_tls(&self, certificate: &Certificate) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (feed, acceptor) = (self.clone(), certificate.acceptor.clone());
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (feed, acceptor) = (feed.clone(), acceptor.clone());
                tokio::spawn(async move {
                    if let Ok(secured) = acceptor.accept(stream).await {
                        feed.answer(TokioIo::new(secured)).await;
                    }
                });
            }
        });
        address
    }

    /// Serves this feed in plain HTTP on a new loopback port; its address.
    pub(crate) async fn serve_plain(&self) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let feed = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let feed = feed.clone();
                tokio::spawn(async move { feed.answer(TokioIo::new(stream)).await });
            }
        });
        address
    }

    async fn answer<T>(&self, io: TokioIo<T>)
    where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let feed = self.clone();
        let service = service_fn(move |request: Request<Incoming>| {
            let feed = feed.clone();
            async move { Ok::<_, Infallible>(feed.reply(request.uri().path())) }
        });
        let _ = http1::Builder::new().serve_connection(io, service).await;
    }

    fn reply(&self, path: &str) -> Response<BoxBody<Bytes, Infallible>> {
        self.asked.lock().unwrap().push(path.to_string());
        let reply = self.routes.lock().unwrap().get(path).cloned().unwrap_or(Reply::Status(404));
        let full = |bytes: Vec<u8>| Full::new(Bytes::from(bytes)).boxed();
        match reply {
            Reply::Bytes(bytes) => Response::new(full(bytes)),
            Reply::Parts(bytes) => {
                let (mut sender, body) = Streamed::<Bytes, Infallible>::new(4);
                tokio::spawn(async move {
                    for part in bytes.chunks(1000) {
                        if sender.send_data(Bytes::copy_from_slice(part)).await.is_err() {
                            return;
                        }
                    }
                });
                Response::new(body.boxed())
            }
            Reply::Stating(length, bytes) => {
                let (mut sender, body) = Streamed::<Bytes, Infallible>::new(4);
                tokio::spawn(async move { drop(sender.send_data(Bytes::from(bytes)).await) });
                let mut response = Response::new(body.boxed());
                response.headers_mut().insert(CONTENT_LENGTH, length.into());
                response
            }
            Reply::Stall => {
                let (sender, body) = Streamed::<Bytes, Infallible>::new(1);
                tokio::spawn(async move {
                    let _held = sender;
                    std::future::pending::<()>().await;
                });
                Response::new(body.boxed())
            }
            Reply::Redirect(location) => {
                let mut response = Response::new(full(Vec::new()));
                *response.status_mut() = StatusCode::FOUND;
                response.headers_mut().insert(LOCATION, location.parse().unwrap());
                response
            }
            Reply::Status(status) => {
                let mut response = Response::new(full(Vec::new()));
                *response.status_mut() = StatusCode::from_u16(status).unwrap();
                response
            }
        }
    }
}

/// A self-signed certificate for the feed, and a trust that takes its key and nothing else the system would not.
pub(crate) struct Certificate {
    acceptor: TlsAcceptor,
    pin: KeyPin,
}

impl Certificate {
    pub(crate) fn new() -> Certificate {
        let CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let digest = ring::digest::digest(&ring::digest::SHA256, &signing_key.subject_public_key_info());
        let pin = KeyPin::from_digest(digest.as_ref().try_into().unwrap());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.der().clone()], key)
            .unwrap();
        Certificate { acceptor: TlsAcceptor::from(Arc::new(config)), pin }
    }

    /// The system's verifier, and this certificate's key as one this device was told to trust.
    pub(crate) fn trust(&self) -> Trust {
        Trust::system().for_server(Some(self.pin))
    }
}
