//! What the client's tests start outside their own process: a real `chatkord-server` on a temporary store, a container
//! of the server's test image (`scripts/test-image.sh`) on this machine's network, the same on Linux and Windows; and
//! `Undo` (Unix), a step that is undone even when its test crashes; [`sound`], a microphone and a speaker made for a
//! test. Only ever a dev-dependency: a failure here is the failing test's, so it panics with what went wrong.
#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic, reason = "a test harness: a server that does not start fails the test")]

mod control;
mod folder;
mod forward;
mod impair;
mod line;
mod relay;
pub mod sound;
#[cfg(unix)]
mod undo;
pub use impair::{Impaired, Loss, Lost, Schedule, Way};
pub use line::Line;
pub use relay::Relay;

/// What the relay's own process (`chatkord-test-relay`) is made of; a test holds a [`Relay`] instead.
pub mod relay_process {
    pub use crate::control::{Command, Reply};
    pub use crate::forward::Forwarder;
}
#[cfg(unix)]
pub use undo::Undo;

use folder::Folder;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

/// The voice channel every server here begins with, beside the one a new store makes itself; a listing
/// finds it by this name and not by the kind alone.
pub const BOOTH: &str = "Booth";

/// The text channel every server here begins with, found by this name.
pub const GENERAL: &str = "general";

/// The category [`BOOTH`] and [`GENERAL`] are made in, found by this name.
pub const CATEGORY: &str = "Lobby";

/// A limit on a server's connections that a test raises through the server's environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// How many connections one address may hold open.
    MaxPerAddress,
    /// How many connections one address may open each second.
    AcceptsPerSecond,
}

impl Limit {
    /// The variable the server reads it from.
    const fn variable(self) -> &'static str {
        match self {
            Limit::MaxPerAddress => "CHATKORD_MAX_PER_ADDRESS",
            Limit::AcceptsPerSecond => "CHATKORD_ACCEPTS_PER_SECOND",
        }
    }
}

/// How long a server is given to say it listens.
const WAIT: Duration = Duration::from_secs(60);

/// How long a server is given to end once asked.
const END: Duration = Duration::from_secs(5);

/// The store inside the container (`chatkord.db` in the image's `CHATKORD_DATA`).
const STORE: &str = "/data/chatkord.db";

/// The image `scripts/test-image.sh` builds from this tree's relay and the server repository's base;
/// `CHATKORD_TEST_IMAGE` names another.
fn image() -> String {
    std::env::var("CHATKORD_TEST_IMAGE").unwrap_or_else(|_| "localhost/chatkord-server:test".to_string())
}

/// A server container on its own store, removed with its local folder when dropped (the folder kept if its test failed).
pub struct Server {
    name: String,
    child: Child,
    /// Closed to end the server: the container's shell then stops it, and `--rm` removes the container. A test
    /// binary that dies closes it too, so its server never outlives it.
    stdin: Option<ChildStdin>,
    dir: Folder,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let asked = Instant::now();
        while matches!(self.child.try_wait(), Ok(None)) && asked.elapsed() < END {
            std::thread::sleep(Duration::from_millis(20));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = runtime(&["kill", &self.name]).output();
            let _ = self.child.wait();
        }
    }
}

/// What the container says on its output that a test needs.
enum Said {
    Invite(String),
    Listening(u16),
}

impl Server {
    /// Starts a server with the voice channel [`BOOTH`]; returns it and `invites` single-use codes. `tag`
    /// names its container and folder, so a run's leftovers say which test made them.
    pub async fn start(tag: &str, invites: usize) -> (Self, Vec<String>) {
        Self::start_with(tag, invites, &[]).await
    }

    /// The same, the server started with each of `limits` set in its environment, as an operator would.
    pub async fn start_with(tag: &str, invites: usize, limits: &[(Limit, u32)]) -> (Self, Vec<String>) {
        // Port 0: one picked here and released could go to another test's server first. Media is on this machine's
        // own address: Windows does not route a packet from a network's address to loopback, and the engine sends from one.
        // A server announces media nowhere until told where, so the fixture says it is that address.
        let at = media_address();
        let media = format!("CHATKORD_MEDIA_IP={at} CHATKORD_MEDIA_PORT=0 CHATKORD_MEDIA_ANNOUNCED={at}");
        Self::start_on(tag, invites, limits, &media).await
    }

    /// The same, its media reached only through a [`Relay`] a test can cut while the server's socket keeps working.
    pub async fn start_relayed(tag: &str, invites: usize) -> (Self, Relay, Vec<String>) {
        let name = named(&format!("{tag}-relay"));
        let relay = tokio::task::spawn_blocking(move || Relay::start(name, media_address())).await.expect("it started");
        let at = relay.address();
        let media = format!(
            "CHATKORD_MEDIA_IP=127.0.0.1 CHATKORD_MEDIA_PORT={} CHATKORD_MEDIA_ANNOUNCED={}",
            at.port(),
            at.ip()
        );
        let (server, codes) = Self::start_on(tag, invites, &[], &media).await;
        (server, relay, codes)
    }

    /// The same, its media listening on IPv4's loopback only and announced as `name`, as an operator who names the
    /// host rather than its address would.
    pub async fn start_announcing(tag: &str, invites: usize, name: &str) -> (Self, Vec<String>) {
        let media = format!("CHATKORD_MEDIA_IP=127.0.0.1 CHATKORD_MEDIA_PORT=0 CHATKORD_MEDIA_ANNOUNCED={name}");
        Self::start_on(tag, invites, &[], &media).await
    }

    /// A server whose media is placed by `media`, its `CHATKORD_MEDIA_*` variables.
    async fn start_on(tag: &str, invites: usize, limits: &[(Limit, u32)], media: &str) -> (Self, Vec<String>) {
        tier::server();
        let name = named(tag);
        let dir = Folder::make(std::env::temp_dir().join(&name));
        let limited: String = limits.iter().map(|(limit, value)| format!("{}={value} ", limit.variable())).collect();
        let script = format!(
            "chatkord-server create-category {CATEGORY} >/dev/null || exit 1
             chatkord-server create-channel voice {BOOTH} --category {CATEGORY} >/dev/null || exit 1
             chatkord-server create-channel text {GENERAL} --category {CATEGORY} >/dev/null || exit 1
             for _ in $(seq {invites}); do echo \"invite $(chatkord-server create-invite --uses 1 --days 1)\"; done
             sqlite3 {STORE} \"INSERT INTO server (name) SELECT NULL WHERE NOT EXISTS (SELECT 1 FROM server);\" || exit 1
             {limited}CHATKORD_BIND=127.0.0.1:0 CHATKORD_ADMIN_ADDR=127.0.0.1:0 {media} NO_COLOR=1 chatkord-server 2>&1 &
             cat >/dev/null; kill $!; wait"
        );
        let mut child = runtime(&["run", "--rm", "--interactive", "--network", "host", "--label", "chatkord-test"])
            .args(["--name", &name, "--entrypoint", "sh", &image(), "-c", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(dir.path().join("runtime.log")).expect("a log file"))
            .spawn()
            .expect("the container runtime started");
        let said = child.stdout.take().expect("its output");
        let mut log = std::fs::File::create(dir.path().join("server.log")).expect("a log file");
        let (tell, told) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(said).lines().map_while(Result::ok) {
                let _ = writeln!(log, "{line}");
                let heard = match line.strip_prefix("invite ") {
                    Some(code) => Some(Said::Invite(code.trim().to_string())),
                    None => listening_port(&line).map(Said::Listening),
                };
                if let Some(heard) = heard {
                    let _ = tell.send(heard);
                }
            }
        });
        let stdin = child.stdin.take();
        let mut server = Self { name, child, stdin, dir, port: 0 };
        let log = server.dir.path().join("server.log");
        let heard = tokio::task::spawn_blocking(move || {
            let mut codes = Vec::new();
            loop {
                match told.recv_timeout(WAIT) {
                    Ok(Said::Invite(code)) => codes.push(code),
                    Ok(Said::Listening(port)) => return Some((port, codes)),
                    Err(_) => return None,
                }
            }
        })
        .await
        .expect("the wait ran");
        let Some((port, codes)) = heard else { panic!("the server did not listen: {}", log.display()) };
        assert_eq!(codes.len(), invites, "every invite was made");
        server.port = port;
        (server, codes)
    }

    /// The folder this server's files live in, gone when it is dropped: a client run in-process keeps what it
    /// remembers between runs here, never in a person's configuration directory.
    pub fn folder(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// A command in the running container; what it printed.
    fn operate(&self, command: &[&str]) -> String {
        let out = runtime(&["exec", &self.name]).args(command).output().expect("the runtime ran");
        assert!(out.status.success(), "{command:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A single-use owner code: whoever registers with it owns the server and holds every grant word.
    pub fn owner_code(&self) -> String {
        self.operate(&["chatkord-server", "create-owner-code"])
    }

    /// Takes `word` from the default role and gives it only through a new role `role`, which `identity` holds, in the
    /// running server's store, as an operator's SQL would.
    pub fn give_only(&self, word: &str, role: &str, identity: &str) {
        for value in [word, role, identity] {
            assert!(value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{value} is a plain word");
        }
        let held = self.operate(&[
            "sqlite3",
            "-cmd",
            ".timeout 30000",
            STORE,
            &format!(
                "BEGIN; \
                 DELETE FROM role_permissions WHERE permission = '{word}' \
                 AND role_id = (SELECT id FROM roles WHERE is_default = 1); \
                 INSERT INTO roles (name, position, created_at) VALUES ('{role}', (SELECT MAX(position) + 1 FROM roles WHERE is_default = 0), 0); \
                 INSERT INTO role_permissions (role_id, permission) SELECT id, '{word}' FROM roles WHERE name = '{role}'; \
                 INSERT INTO user_roles (user_id, role_id) SELECT u.id, r.id FROM users u, roles r \
                 WHERE u.identity = '{identity}' AND r.name = '{role}'; \
                 SELECT changes(); COMMIT;"
            ),
        ]);
        assert_eq!(held, "1", "{identity} is an account here");
    }

    /// Stops the server's process where it is, media and all, until [`Server::resume`]: a network that stopped
    /// carrying anything, with every socket still open.
    pub fn pause(&self) {
        self.container("pause");
    }

    /// Lets a [`Server::pause`]d server run on.
    pub fn resume(&self) {
        self.container("unpause");
    }

    fn container(&self, verb: &str) {
        let out = runtime(&[verb, &self.name]).output().expect("the runtime ran");
        assert!(out.status.success(), "{verb}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Its socket's address on this machine, unencrypted.
    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/v1", self.port)
    }

    /// The same server as a person types it: host and port, nothing about encryption.
    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// A container's name, `tag` saying which test made it and a count telling this binary's apart.
fn named(tag: &str) -> String {
    static STARTED: AtomicUsize = AtomicUsize::new(0);
    let started = STARTED.fetch_add(1, Ordering::Relaxed);
    format!("chatkord-test-{}-{tag}-{started}", std::process::id())
}

/// The container runtime as `scripts/test-image.sh` chooses it: `CHATKORD_CONTAINER`, else podman, else docker.
fn runtime(args: &[&str]) -> Command {
    static CHOSEN: OnceLock<String> = OnceLock::new();
    let chosen = CHOSEN.get_or_init(|| {
        std::env::var("CHATKORD_CONTAINER").unwrap_or_else(|_| {
            let podman = Command::new("podman").arg("--version").output().is_ok_and(|out| out.status.success());
            if podman { "podman" } else { "docker" }.to_string()
        })
    });
    let mut runtime = Command::new(chosen);
    runtime.args(args);
    runtime
}

/// This machine's address on the network its default route leaves by (no packet is sent), or loopback without one.
fn media_address() -> IpAddr {
    let routed = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).and(socket.local_addr()));
    routed.map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |local| local.ip())
}

/// The port in a server's `listening addr=127.0.0.1:PORT` line.
fn listening_port(line: &str) -> Option<u16> {
    line.split("listening addr=").nth(1)?.trim().rsplit(':').next()?.parse().ok()
}
