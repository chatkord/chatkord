//! A preview against a server on this machine's loopback, reached through a [`Reach`] the test supplies: names
//! resolve to addresses the test chooses and every connection goes to the server, so what the guard refused is read
//! from what was never connected, and nothing reaches the internet.

use crate::get::{GetError, Stream};
use crate::guard::Vetted;
use crate::net::Reach;
use crate::preview::{MAX_PAGE_BYTES, MAX_PICTURE_BYTES, picture, preview};
use crate::url::Url;
use crate::{MediaType, PreviewError};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, channel::Channel};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terms::LinkError;
use tokio::net::{TcpListener, TcpStream};

const SITE: &str = "https://site.example.invalid";
const TEN: Duration = Duration::from_secs(10);
const PNG: &[u8] = b"\x89PNG not really";

/// What the server sends for a path.
#[derive(Clone)]
struct Reply {
    status: u16,
    content_type: Option<&'static str>,
    location: Option<String>,
    stated: Option<u64>,
    body: Body,
}

#[derive(Clone)]
enum Body {
    /// All of it, then the end.
    Whole(Vec<u8>),
    /// All of it in parts of 16 KiB, no length stated, then the end.
    Parts(Vec<u8>),
    /// These bytes, then the connection held open with nothing more.
    Hang(Vec<u8>),
}

impl Reply {
    fn of(content_type: &'static str, body: Body) -> Reply {
        Reply { status: 200, content_type: Some(content_type), location: None, stated: None, body }
    }

    fn html(page: &str) -> Reply {
        Reply::of("text/html; charset=utf-8", Body::Whole(page.as_bytes().to_vec()))
    }

    fn image(content_type: &'static str, bytes: &[u8]) -> Reply {
        Reply::of(content_type, Body::Whole(bytes.to_vec()))
    }

    fn redirect(location: &str) -> Reply {
        Reply {
            status: 302,
            content_type: None,
            location: Some(location.to_string()),
            stated: None,
            body: Body::Whole(vec![]),
        }
    }

    fn stating(self, length: u64) -> Reply {
        Reply { stated: Some(length), ..self }
    }
}

/// A request as the server saw it.
#[derive(Clone, Debug)]
struct Seen {
    path: String,
    headers: Vec<(String, String)>,
}

#[derive(Clone, Default)]
struct Server {
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    fn route(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_string(), reply);
    }

    fn asked(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|seen| seen.path.clone()).collect()
    }

    async fn serve(&self) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let server = server.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let server = server.clone();
                        async move { Ok::<_, Infallible>(server.answer(&request)) }
                    });
                    let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                });
            }
        });
        address
    }

    fn answer(&self, request: &Request<Incoming>) -> Response<BoxBody<Bytes, Infallible>> {
        let path = request.uri().path().to_string();
        let headers = request
            .headers()
            .iter()
            .map(|(name, value)| (name.to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
            .collect();
        self.seen.lock().unwrap().push(Seen { path: path.clone(), headers });
        let reply = self.routes.lock().unwrap().get(&path).cloned();
        let Some(reply) = reply else {
            let mut missing = Response::new(Full::new(Bytes::new()).boxed());
            *missing.status_mut() = StatusCode::NOT_FOUND;
            return missing;
        };
        let hang = matches!(reply.body, Body::Hang(_)) || reply.stated.is_some();
        let body = match reply.body {
            Body::Whole(bytes) if reply.stated.is_none() => Full::new(Bytes::from(bytes)).boxed(),
            Body::Whole(bytes) | Body::Hang(bytes) => {
                let (mut sender, body) = Channel::<Bytes, Infallible>::new(4);
                tokio::spawn(async move {
                    for part in bytes.chunks(16 * 1024) {
                        if sender.send_data(Bytes::copy_from_slice(part)).await.is_err() {
                            return;
                        }
                    }
                    if hang {
                        let _held = sender;
                        std::future::pending::<()>().await;
                    }
                });
                body.boxed()
            }
            Body::Parts(bytes) => {
                let (mut sender, body) = Channel::<Bytes, Infallible>::new(4);
                tokio::spawn(async move {
                    for part in bytes.chunks(16 * 1024) {
                        if sender.send_data(Bytes::copy_from_slice(part)).await.is_err() {
                            return;
                        }
                    }
                });
                body.boxed()
            }
        };
        let mut response = Response::new(body);
        *response.status_mut() = StatusCode::from_u16(reply.status).unwrap();
        if let Some(content_type) = reply.content_type {
            response.headers_mut().insert(CONTENT_TYPE, content_type.parse().unwrap());
        }
        if let Some(location) = reply.location {
            response.headers_mut().insert(LOCATION, location.parse().unwrap());
        }
        if let Some(stated) = reply.stated {
            response.headers_mut().insert(CONTENT_LENGTH, stated.into());
        }
        response
    }
}

/// Names the test chose, and the server every connection goes to.
struct Lab {
    names: HashMap<&'static str, Vec<IpAddr>>,
    server: SocketAddr,
    resolved: Mutex<Vec<String>>,
    connected: Mutex<Vec<Vec<SocketAddr>>>,
}

impl Reach for Lab {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, GetError> {
        self.resolved.lock().unwrap().push(host.to_string());
        let found = self.names.get(host).ok_or(GetError::Reach(LinkError::NoSuchHost))?;
        Ok(found.iter().map(|ip| SocketAddr::new(*ip, port)).collect())
    }

    async fn connect(&self, to: &Vetted, _host: &str) -> Result<Stream, GetError> {
        self.connected.lock().unwrap().push(to.addresses().to_vec());
        let stream = TcpStream::connect(self.server).await.map_err(|_| GetError::Reach(LinkError::Connect))?;
        Ok(Box::new(stream))
    }
}

impl Lab {
    async fn new() -> (Lab, Server) {
        let server = Server::default();
        let at = server.serve().await;
        let ip = |text: &str| vec![text.parse::<IpAddr>().unwrap()];
        let names = HashMap::from([
            ("site.example.invalid", ip("93.184.216.34")),
            ("cdn.example.invalid", ip("8.8.8.8")),
            ("inside.example.invalid", ip("10.0.0.5")),
            ("loop.example.invalid", ip("127.0.0.1")),
            ("mixed.example.invalid", vec!["8.8.8.8".parse().unwrap(), "192.168.0.1".parse().unwrap()]),
            ("mapped.example.invalid", ip("::ffff:10.0.0.1")),
        ]);
        (Lab { names, server: at, resolved: Mutex::default(), connected: Mutex::default() }, server)
    }

    fn connections(&self) -> usize {
        self.connected.lock().unwrap().len()
    }
}

fn page(head: &str) -> String {
    format!("<!doctype html><html><head>{head}</head><body>x</body></html>")
}

#[tokio::test]
async fn a_page_gives_its_title_description_and_picture() {
    let (lab, server) = Lab::new().await;
    server.route("/", Reply::html(&page(r#"<meta property="og:title" content="Hello &amp; welcome"><meta property="og:description" content="A page"><meta property="og:image" content="https://cdn.example.invalid/p.png">"#)));
    server.route("/p.png", Reply::image("image/png", PNG));
    let link = format!("{SITE}/#frag");
    let made = preview(&lab, &link, TEN).await.unwrap();
    assert_eq!(made.url, link);
    assert_eq!(made.title, "Hello & welcome");
    assert_eq!(made.description.as_deref(), Some("A page"));
    let picture = made.picture.unwrap();
    assert_eq!((picture.bytes.as_slice(), picture.media_type), (PNG, MediaType::Png));
    assert_eq!(lab.connections(), 2);
    assert_eq!(*lab.resolved.lock().unwrap(), ["site.example.invalid", "cdn.example.invalid"]);
}

#[tokio::test]
async fn a_request_carries_no_identity_beyond_a_generic_agent() {
    let (lab, server) = Lab::new().await;
    server.route("/a", Reply::html(&page("<title>T</title>")));
    preview(&lab, &format!("{SITE}/a?q=1"), TEN).await.unwrap();
    let seen = server.seen.lock().unwrap().clone();
    let mut names: Vec<_> = seen[0].headers.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["accept", "host", "user-agent"]);
    let value = |wanted: &str| seen[0].headers.iter().find(|(name, _)| name == wanted).map(|(_, value)| value.as_str());
    assert_eq!(value("host"), Some("site.example.invalid"));
    assert_eq!(value("user-agent"), Some("Mozilla/5.0 (compatible; Chatkord link preview)"));
}

#[tokio::test]
async fn every_address_not_global_is_refused_before_a_connection() {
    let (lab, server) = Lab::new().await;
    for host in ["inside", "loop", "mixed", "mapped"] {
        let refused = preview(&lab, &format!("https://{host}.example.invalid/"), TEN).await;
        assert_eq!(refused, Err(PreviewError::Refused), "{host}");
    }
    assert_eq!(lab.connections(), 0);
    assert!(server.asked().is_empty());
}

#[tokio::test]
async fn a_literal_address_is_judged_without_a_lookup() {
    let (lab, server) = Lab::new().await;
    for literal in [
        "127.0.0.1",
        "10.1.2.3",
        "169.254.169.254",
        "100.64.0.1",
        "[::1]",
        "[::ffff:10.0.0.1]",
        "[fd00::1]",
        "[64:ff9b::7f00:1]",
    ] {
        let refused = preview(&lab, &format!("https://{literal}/"), TEN).await;
        assert_eq!(refused, Err(PreviewError::Refused), "{literal}");
    }
    assert!(lab.resolved.lock().unwrap().is_empty());
    assert_eq!(lab.connections(), 0);
    assert!(server.asked().is_empty());
    server.route("/", Reply::html(&page("<title>Global</title>")));
    assert_eq!(preview(&lab, "https://8.8.8.8/", TEN).await.unwrap().title, "Global");
    assert_eq!(*lab.connected.lock().unwrap(), [vec!["8.8.8.8:443".parse::<SocketAddr>().unwrap()]]);
}

#[tokio::test]
async fn a_name_that_does_not_resolve_is_not_connected() {
    let (lab, _) = Lab::new().await;
    let failed = preview(&lab, "https://nowhere.example.invalid/", TEN).await;
    assert_eq!(failed, Err(PreviewError::Fetch(GetError::Reach(LinkError::NoSuchHost))));
    assert_eq!(lab.connections(), 0);
}

#[tokio::test]
async fn an_address_that_is_not_https_on_443_is_never_asked() {
    let (lab, server) = Lab::new().await;
    for link in [
        "http://site.example.invalid/",
        "https://site.example.invalid:8443/",
        "https://user@site.example.invalid/",
        "ftp://site.example.invalid/",
        "site.example.invalid",
    ] {
        assert_eq!(preview(&lab, link, TEN).await, Err(PreviewError::BadUrl), "{link}");
    }
    assert!(lab.resolved.lock().unwrap().is_empty());
    assert!(server.asked().is_empty());
}

#[tokio::test]
async fn a_redirect_to_an_address_that_is_not_global_is_refused_unconnected() {
    let (lab, server) = Lab::new().await;
    server.route("/go", Reply::redirect("https://inside.example.invalid/secret"));
    server.route("/secret", Reply::html(&page("<title>Inside</title>")));
    let refused = preview(&lab, &format!("{SITE}/go"), TEN).await;
    assert_eq!(refused, Err(PreviewError::Refused));
    assert_eq!(lab.connections(), 1, "only the first hop was connected");
    assert_eq!(server.asked(), ["/go"]);
    server.route("/lit", Reply::redirect("https://[::1]/x"));
    assert_eq!(preview(&lab, &format!("{SITE}/lit"), TEN).await, Err(PreviewError::Refused));
    assert_eq!(server.asked(), ["/go", "/lit"]);
}

#[tokio::test]
async fn a_redirect_is_resolved_and_vetted_again() {
    let (lab, server) = Lab::new().await;
    server.route("/go", Reply::redirect("https://cdn.example.invalid/there"));
    server.route("/there", Reply::redirect("/here?x=1"));
    server.route("/here", Reply::html(&page("<title>Arrived</title>")));
    assert_eq!(preview(&lab, &format!("{SITE}/go"), TEN).await.unwrap().title, "Arrived");
    assert_eq!(*lab.resolved.lock().unwrap(), ["site.example.invalid", "cdn.example.invalid", "cdn.example.invalid"]);
    assert_eq!(lab.connections(), 3);
}

#[tokio::test]
async fn three_redirects_are_followed_and_a_fourth_is_not() {
    let (lab, server) = Lab::new().await;
    for (from, to) in [("/a", "/b"), ("/b", "/c"), ("/c", "/d"), ("/d", "/e")] {
        server.route(from, Reply::redirect(to));
    }
    server.route("/e", Reply::html(&page("<title>End</title>")));
    assert_eq!(preview(&lab, &format!("{SITE}/b"), TEN).await.unwrap().title, "End");
    assert_eq!(preview(&lab, &format!("{SITE}/a"), TEN).await, Err(PreviewError::Fetch(GetError::TooManyRedirects)));
}

#[tokio::test]
async fn a_redirect_to_http_or_another_port_or_nothing_is_not_followed() {
    let (lab, server) = Lab::new().await;
    for (path, location) in [
        ("/h", "http://site.example.invalid/x"),
        ("/p", "https://site.example.invalid:8443/x"),
        ("/s", "javascript:alert(1)"),
        ("/u", "https://u@site.example.invalid/x"),
    ] {
        server.route(path, Reply::redirect(location));
        assert_eq!(
            preview(&lab, &format!("{SITE}{path}"), TEN).await,
            Err(PreviewError::Fetch(GetError::BadRedirect)),
            "{location}"
        );
    }
    server.route("/n", Reply { location: None, ..Reply::redirect("") });
    assert_eq!(preview(&lab, &format!("{SITE}/n"), TEN).await, Err(PreviewError::Fetch(GetError::BadRedirect)));
}

#[tokio::test]
async fn a_page_is_read_to_its_cap_and_no_further() {
    let (lab, server) = Lab::new().await;
    let padding = " ".repeat(MAX_PAGE_BYTES);
    server.route("/late", Reply::html(&format!("<html><head>{padding}<title>Past the cap</title>")));
    assert_eq!(preview(&lab, &format!("{SITE}/late"), TEN).await, Err(PreviewError::NoTitle));
    let mut early = format!("<html><head><title>Inside the cap</title>{padding}").into_bytes();
    early.truncate(MAX_PAGE_BYTES);
    server.route("/early", Reply::of("text/html", Body::Hang(early)));
    let started = Instant::now();
    assert_eq!(preview(&lab, &format!("{SITE}/early"), TEN).await.unwrap().title, "Inside the cap");
    assert!(started.elapsed() < Duration::from_secs(4), "reading stopped at the cap, not at the end");
}

#[tokio::test]
async fn a_title_far_into_a_page_behind_its_scripts_is_found() {
    let (lab, server) = Lab::new().await;
    let before = "<!doctype html><html><head><script>";
    let after = "</script>";
    let mut page = String::from(before);
    page.push_str(&"x".repeat(850_078 - before.len() - after.len()));
    page.push_str(after);
    assert_eq!(page.len(), 850_078);
    page.push_str(r#"<meta property="og:title" content="Far in"><script>"#);
    page.push_str(&"y".repeat(830_000));
    page.push_str("</script></head><body>x</body></html>");
    assert!(page.len() > 1_680_000);
    server.route("/deep", Reply::of("text/html; charset=utf-8", Body::Parts(page.into_bytes())));
    assert_eq!(preview(&lab, &format!("{SITE}/deep"), TEN).await.unwrap().title, "Far in");
}

#[tokio::test]
async fn a_page_is_read_no_further_than_the_end_of_its_head() {
    let (lab, server) = Lab::new().await;
    let mut head = format!("<html><head><title>Short</title></head>{}", "z".repeat(1024)).into_bytes();
    head.extend_from_slice(b"\xff not even read");
    server.route("/short", Reply::of("text/html", Body::Hang(head)));
    let started = Instant::now();
    assert_eq!(preview(&lab, &format!("{SITE}/short"), TEN).await.unwrap().title, "Short");
    assert!(started.elapsed() < Duration::from_secs(4), "reading stopped at the head, not at the end");
}

#[tokio::test]
async fn a_picture_over_its_cap_is_refused_by_its_length_and_while_it_arrives() {
    let (lab, server) = Lab::new().await;
    let over = MAX_PICTURE_BYTES + 1;
    let url = |path: &str| Url::parse(&format!("{SITE}{path}")).unwrap();
    server.route("/stated", Reply::of("image/png", Body::Hang(vec![0; 10])).stating(u64::try_from(over).unwrap()));
    server.route("/unstated", Reply::of("image/png", Body::Parts(vec![0; over])));
    server.route("/exact", Reply::of("image/png", Body::Parts(vec![0; MAX_PICTURE_BYTES])));
    let started = Instant::now();
    assert_eq!(picture(&lab, url("/stated")).await.err(), Some(GetError::TooLarge));
    assert!(started.elapsed() < Duration::from_secs(4), "refused by its stated length, before its body");
    assert_eq!(picture(&lab, url("/unstated")).await.err(), Some(GetError::TooLarge));
    assert_eq!(picture(&lab, url("/exact")).await.unwrap().bytes.len(), MAX_PICTURE_BYTES);
    server.route("/", Reply::html(&page(r#"<title>T</title><meta property="og:image" content="/unstated">"#)));
    let made = preview(&lab, &format!("{SITE}/"), TEN).await.unwrap();
    assert_eq!((made.title.as_str(), made.picture), ("T", None));
}

#[tokio::test]
async fn the_total_deadline_ends_a_page_that_never_arrives() {
    let (lab, server) = Lab::new().await;
    server.route("/slow", Reply::of("text/html", Body::Hang(b"<html><head>".to_vec())));
    let started = Instant::now();
    let late = preview(&lab, &format!("{SITE}/slow"), Duration::from_millis(300)).await;
    assert_eq!(late, Err(PreviewError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn a_picture_that_never_arrives_costs_the_picture_and_not_the_preview() {
    let (lab, server) = Lab::new().await;
    server.route("/", Reply::html(&page(r#"<title>T</title><meta property="og:image" content="/slow.png">"#)));
    server.route("/slow.png", Reply::of("image/png", Body::Hang(vec![1, 2, 3])));
    let started = Instant::now();
    let made = preview(&lab, &format!("{SITE}/"), Duration::from_millis(500)).await.unwrap();
    assert_eq!((made.title.as_str(), made.picture), ("T", None));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn only_a_web_page_is_a_page_and_only_png_jpeg_or_webp_is_a_picture() {
    let (lab, server) = Lab::new().await;
    let body = page("<title>T</title>").into_bytes();
    for (path, content_type) in [
        ("/png", Some("image/png")),
        ("/text", Some("text/plain")),
        ("/json", Some("application/json")),
        ("/xhtml", Some("application/xhtml+xml")),
    ] {
        server.route(path, Reply { content_type, ..Reply::image("x", &body) });
        assert_eq!(preview(&lab, &format!("{SITE}{path}"), TEN).await, Err(PreviewError::NotHtml), "{path}");
    }
    server.route("/none", Reply { content_type: None, ..Reply::image("x", &body) });
    assert_eq!(preview(&lab, &format!("{SITE}/none"), TEN).await, Err(PreviewError::NotHtml));
    server.route("/upper", Reply::of("TEXT/HTML ; Charset=\"UTF-8\"", Body::Whole(body.clone())));
    assert!(preview(&lab, &format!("{SITE}/upper"), TEN).await.is_ok());
    server.route("/latin", Reply::of("text/html; charset=iso-8859-1", Body::Whole(body.clone())));
    assert_eq!(preview(&lab, &format!("{SITE}/latin"), TEN).await, Err(PreviewError::NotUtf8));
    server.route("/404", Reply { status: 404, ..Reply::html("") });
    assert_eq!(preview(&lab, &format!("{SITE}/404"), TEN).await, Err(PreviewError::Fetch(GetError::Status(404))));

    for (path, content_type, media) in [
        ("/p", "image/png", Some(MediaType::Png)),
        ("/j", "image/jpeg", Some(MediaType::Jpeg)),
        ("/w", "image/webp", Some(MediaType::Webp)),
        ("/g", "image/gif", None),
        ("/s", "image/svg+xml", None),
        ("/h", "text/html", None),
    ] {
        server.route(path, Reply::image(content_type, PNG));
        server.route(
            &format!("/page{path}"),
            Reply::html(&page(&format!(r#"<title>T</title><meta property="og:image" content="{path}">"#))),
        );
        let made = preview(&lab, &format!("{SITE}/page{path}"), TEN).await.unwrap();
        assert_eq!(made.picture.map(|picture| picture.media_type), media, "{content_type}");
    }
}

#[tokio::test]
async fn a_picture_goes_through_the_same_guard_and_https_only() {
    let (lab, server) = Lab::new().await;
    for (path, image) in [
        ("/a", "https://inside.example.invalid/p.png"),
        ("/b", "http://cdn.example.invalid/p.png"),
        ("/c", "https://127.0.0.1/p.png"),
        ("/d", "data:image/png;base64,AAAA"),
    ] {
        server.route(
            path,
            Reply::html(&page(&format!(r#"<title>T</title><meta property="og:image" content="{image}">"#))),
        );
        let made = preview(&lab, &format!("{SITE}{path}"), TEN).await.unwrap();
        assert_eq!(made.picture, None, "{image}");
    }
    assert_eq!(lab.connections(), 4, "one for each page, none for a picture");
    assert_eq!(server.asked(), ["/a", "/b", "/c", "/d"]);
}

#[tokio::test]
async fn a_page_with_no_title_has_no_preview() {
    let (lab, server) = Lab::new().await;
    server.route("/", Reply::html(&page(r#"<meta property="og:description" content="only this">"#)));
    assert_eq!(preview(&lab, &format!("{SITE}/"), TEN).await, Err(PreviewError::NoTitle));
}
