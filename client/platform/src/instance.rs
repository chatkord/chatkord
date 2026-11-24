//! One window per person on this device. The first run holds a lock for as long as it lives and
//! listens beside it; a later run finds the lock taken, asks the first to come forward with the token its desktop
//! launched it with, and waits for its reply that it has: a run no one answers ends with failure rather than hang
//! the person's launch.
//! Desktop only: a phone's system runs one copy of an app by itself.

use crate::activation::{self, Activation};
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// The lock and the place a later run knocks, in this program's folder.
const LOCK: &str = "instance.lock";
#[cfg(unix)]
const KNOCK: &str = "instance.sock";
/// The first run's process and claim, which name the pipe a later run knocks on.
#[cfg(windows)]
const KNOCK: &str = "instance.pipe";
/// The one request a later run makes, followed by a space and its activation token where it has one.
const COME_FORWARD: &[u8] = b"activate";
/// What ends a request, so the one who knocks can wait for the answer on the same connection.
const END: u8 = b'\n';
/// The longest request read, its end included: one more byte than the longest that parses, so a longer one is refused.
const REQUEST_MAX: usize = COME_FORWARD.len() + 1 + activation::MAX + 1 + 1;
/// The one byte the first run answers with once it has handled the request.
const REPLIED: u8 = 1;
/// How long a later run waits for that byte, and the first run for its window to handle the request.
const REPLY_WITHIN: Duration = Duration::from_secs(5);

/// This run is the one: the lock is held until the process ends.
#[derive(Debug)]
pub struct Instance {
    _lock: File,
    #[cfg(unix)]
    knocks: std::os::unix::net::UnixListener,
    /// Taken by [`Instance::answer`]: the pipe and the runtime it is served on.
    #[cfg(windows)]
    knocks: Option<pipe::Knocks>,
}

/// Another run holds the lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elsewhere {
    /// It was asked to come forward.
    Told,
    /// It could not be reached.
    Silent,
    /// It took the request and did not say, in the time a reply is waited for, that its window handled it.
    Unanswered,
}

/// What a run's window says once it has handled a request to come forward; one that is dropped unsaid, or never said,
/// leaves the later run to find the request unanswered. Clones share the one answer.
#[derive(Clone, Debug)]
pub struct Reply(Arc<Mutex<Option<mpsc::Sender<()>>>>);

impl Reply {
    fn pair() -> (Self, mpsc::Receiver<()>) {
        let (said, heard) = mpsc::channel();
        (Self(Arc::new(Mutex::new(Some(said)))), heard)
    }

    /// Says the request was handled; a second say, from a clone, does nothing.
    pub fn done(&self) {
        let said = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(said) = said {
            let _ = said.send(());
        }
    }
}

/// Claims this device's one window for this run; where another run holds it, that run is asked to come forward with
/// `activation`. `Ok(None)` is a system with no folder for the lock, where a run goes on without one.
pub fn claim(platform: &crate::Platform, activation: Option<&Activation>) -> Result<Option<Instance>, Elsewhere> {
    match folder(platform) {
        Some(here) => claim_in(&here, activation).map(Some),
        None => Ok(None),
    }
}

/// [`claim`] in a named folder, created private if missing.
pub fn claim_in(folder: &Path, activation: Option<&Activation>) -> Result<Instance, Elsewhere> {
    if private_folder(folder).is_err() {
        return Err(Elsewhere::Silent);
    }
    let lock = match OpenOptions::new().create(true).write(true).truncate(false).open(folder.join(LOCK)) {
        Ok(file) => file,
        Err(_) => return Err(Elsewhere::Silent),
    };
    match lock.try_lock() {
        Ok(()) => held(lock, folder),
        Err(TryLockError::WouldBlock) => Err(knock(folder, &request(activation))),
        Err(TryLockError::Error(_)) => Err(Elsewhere::Silent),
    }
}

/// Where the lock lives: the session's own runtime folder on Linux, else this program's folder.
fn folder(platform: &crate::Platform) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|path| path.is_absolute()) {
        return Some(runtime.join(crate::state::FOLDER));
    }
    platform.folder()
}

#[cfg(unix)]
fn private_folder(folder: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder)
}

#[cfg(not(unix))]
fn private_folder(folder: &Path) -> io::Result<()> {
    std::fs::create_dir_all(folder)
}

#[cfg(unix)]
fn held(lock: File, folder: &Path) -> Result<Instance, Elsewhere> {
    let path = folder.join(KNOCK);
    // The lock is ours, so a socket file left here belongs to a run that is gone.
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(gone) if gone.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(Elsewhere::Silent),
    }
    let knocks = std::os::unix::net::UnixListener::bind(&path).map_err(|_| Elsewhere::Silent)?;
    Ok(Instance { _lock: lock, knocks })
}

#[cfg(windows)]
fn held(lock: File, folder: &Path) -> Result<Instance, Elsewhere> {
    let knocks = pipe::open().map_err(|_| Elsewhere::Silent)?;
    std::fs::write(folder.join(KNOCK), knocks.at.to_string()).map_err(|_| Elsewhere::Silent)?;
    Ok(Instance { _lock: lock, knocks: Some(knocks) })
}

/// The request that asks the first run to come forward, with `activation` where there is one.
fn request(activation: Option<&Activation>) -> Vec<u8> {
    let mut said = COME_FORWARD.to_vec();
    if let Some(activation) = activation {
        said.push(b' ');
        said.extend_from_slice(activation.token().as_bytes());
    }
    said.push(END);
    said
}

/// The request in what was read up to its end, which is dropped; none when no end came.
fn request_read(mut said: Vec<u8>) -> Option<Option<Activation>> {
    (said.pop()? == END).then(|| parse(&said)).flatten()
}

/// What a request asks: `Some` with the token it carries, where it carries one; `None` is not the request.
fn parse(said: &[u8]) -> Option<Option<Activation>> {
    let rest = said.strip_prefix(COME_FORWARD)?;
    if rest.is_empty() {
        return Some(None);
    }
    let token = std::str::from_utf8(rest.strip_prefix(b" ")?).ok()?;
    Activation::new(token.to_owned()).ok().map(Some)
}

#[cfg(unix)]
fn knock(folder: &Path, request: &[u8]) -> Elsewhere {
    use std::io::{Read, Write};
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(folder.join(KNOCK)) else { return Elsewhere::Silent };
    if stream.set_read_timeout(Some(REPLY_WITHIN)).is_err() || stream.write_all(request).is_err() {
        return Elsewhere::Silent;
    }
    let mut reply = [0u8; 1];
    match stream.read_exact(&mut reply) {
        Ok(()) if reply == [REPLIED] => Elsewhere::Told,
        Ok(()) | Err(_) => Elsewhere::Unanswered,
    }
}

#[cfg(windows)]
fn knock(folder: &Path, request: &[u8]) -> Elsewhere {
    let first = std::fs::read_to_string(folder.join(KNOCK)).ok().and_then(|said| said.parse::<pipe::At>().ok());
    match first.map(|first| pipe::knock(first, request)) {
        Some(Ok(true)) => Elsewhere::Told,
        Some(Ok(false)) => Elsewhere::Unanswered,
        Some(Err(_)) | None => Elsewhere::Silent,
    }
}

impl Instance {
    /// Lets this run's claim go, then calls `next`: a run `next` starts claims the one window, where while this run
    /// held it the run would ask this one to come forward and end.
    pub fn hand_over<T>(self, next: impl FnOnce() -> T) -> T {
        drop(self);
        next()
    }

    /// Calls `come_forward` with the later run's token and a [`Reply`] on a thread of its own each time a later run
    /// asks, for as long as the process runs; the later run is told once the reply says the request was handled.
    /// Anything but the one request is ignored.
    #[cfg(unix)]
    pub fn answer(&mut self, come_forward: impl Fn(Option<Activation>, Reply) + Send + 'static) -> io::Result<()> {
        use std::io::{BufRead, BufReader, Read, Write};
        let knocks = self.knocks.try_clone()?;
        std::thread::Builder::new().name("instance".into()).spawn(move || {
            for stream in knocks.incoming() {
                let Ok(mut stream) = stream else { continue };
                // A run that connects and says nothing does not hold this thread.
                if stream.set_read_timeout(Some(Duration::from_secs(1))).is_err() {
                    continue;
                }
                let mut said = Vec::with_capacity(REQUEST_MAX);
                let bound = u64::try_from(REQUEST_MAX).unwrap_or(u64::MAX);
                if BufReader::new((&stream).take(bound)).read_until(END, &mut said).is_ok()
                    && let Some(activation) = request_read(said)
                {
                    let (reply, handled) = Reply::pair();
                    come_forward(activation, reply);
                    if handled.recv_timeout(REPLY_WITHIN).is_ok() {
                        let _ = stream.write_all(&[REPLIED]);
                    }
                }
            }
        })?;
        Ok(())
    }

    /// Calls `come_forward` with the later run's token and a [`Reply`] on a thread of its own each time a later run
    /// asks, for as long as the process runs; the later run is told once the reply says the request was handled.
    /// Anything but the one request is ignored; a second call is refused.
    #[cfg(windows)]
    pub fn answer(&mut self, come_forward: impl Fn(Option<Activation>, Reply) + Send + 'static) -> io::Result<()> {
        let knocks = self.knocks.take().ok_or_else(|| io::Error::other("already answering"))?;
        std::thread::Builder::new().name("instance".into()).spawn(move || knocks.serve(come_forward))?;
        Ok(())
    }
}

/// The knock on Windows: a local named pipe, named by the first run's process and claim, that reads one request.
#[cfg(windows)]
mod pipe {
    use super::{Activation, END, REPLIED, REPLY_WITHIN, REQUEST_MAX, Reply, request_read};
    use std::io;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    /// Which pipe: the first run's process, and which claim in it (a test claims more than once).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct At {
        process: u32,
        claim: u32,
    }

    impl At {
        pub(super) fn name(self) -> String {
            format!(r"\\.\pipe\chatkord-instance-{}-{}", self.process, self.claim)
        }
    }

    impl std::fmt::Display for At {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "{} {}", self.process, self.claim)
        }
    }

    impl std::str::FromStr for At {
        type Err = std::num::ParseIntError;

        fn from_str(said: &str) -> Result<Self, Self::Err> {
            let (process, claim) = said.split_once(' ').unwrap_or((said, ""));
            Ok(Self { process: process.parse()?, claim: claim.parse()? })
        }
    }

    fn create(at: At, first: bool) -> io::Result<NamedPipeServer> {
        // Local only, the request read and its one byte of reply written; the system's default access lets no other
        // account write to it.
        ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create(at.name())
    }

    /// The pipe and the runtime it is served on.
    pub(super) struct Knocks {
        pub(super) at: At,
        runtime: tokio::runtime::Runtime,
        pipe: NamedPipeServer,
    }

    impl std::fmt::Debug for Knocks {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.debug_struct("Knocks").field("at", &self.at).finish_non_exhaustive()
        }
    }

    /// The pipe, made now so a later run that knocks before [`Knocks::serve`] waits rather than fails.
    pub(super) fn open() -> io::Result<Knocks> {
        static CLAIMS: AtomicU32 = AtomicU32::new(0);
        let at = At { process: std::process::id(), claim: CLAIMS.fetch_add(1, Ordering::Relaxed) };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build()?;
        let pipe = runtime.block_on(async { create(at, true) })?;
        Ok(Knocks { at, runtime, pipe })
    }

    impl Knocks {
        /// Reads each knock in turn until the pipe cannot be made again.
        pub(super) fn serve(self, come_forward: impl Fn(Option<Activation>, Reply)) {
            let Self { at, runtime, mut pipe } = self;
            runtime.block_on(async move {
                loop {
                    let connected = pipe.connect().await;
                    // The next instance first, so a knock while this one is read finds the pipe.
                    let Ok(next) = create(at, false) else { return };
                    let current = std::mem::replace(&mut pipe, next);
                    if connected.is_err() {
                        continue;
                    }
                    let mut current = current;
                    let mut said = Vec::with_capacity(REQUEST_MAX);
                    let bound = u64::try_from(REQUEST_MAX).unwrap_or(u64::MAX);
                    // A run that connects and says nothing does not hold this thread.
                    let mut reader = tokio::io::BufReader::new((&mut current).take(bound));
                    let read = tokio::time::timeout(Duration::from_secs(1), reader.read_until(END, &mut said)).await;
                    drop(reader);
                    if matches!(read, Ok(Ok(_)))
                        && let Some(activation) = request_read(said)
                    {
                        let (reply, handled) = Reply::pair();
                        come_forward(activation, reply);
                        if handled.recv_timeout(REPLY_WITHIN).is_ok() && current.write_all(&[REPLIED]).await.is_ok() {
                            // The pipe is let go once the later run has read its byte and closed its end.
                            let _ = tokio::time::timeout(Duration::from_secs(1), current.read(&mut [0u8; 1])).await;
                        }
                    }
                }
            });
        }
    }

    /// Asks the run at `first` to come forward, and lets it take the foreground, which Windows gives a process
    /// only when the one in front allows it; whether it said, within [`REPLY_WITHIN`], that it handled the request.
    pub(super) fn knock(first: At, request: &[u8]) -> io::Result<bool> {
        use std::io::{Read, Write};
        // SAFETY: a plain call that takes a process id and touches no memory of ours.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(first.process) };
        let mut pipe = connect(first)?;
        pipe.write_all(request)?;
        // A read on a pipe has no timeout, so it is waited for on a thread of its own.
        let (told, heard) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("instance-reply".into()).spawn(move || {
            let mut reply = [0u8; 1];
            let _ = told.send(pipe.read_exact(&mut reply).map(|()| reply[0]));
        })?;
        Ok(matches!(heard.recv_timeout(REPLY_WITHIN), Ok(Ok(REPLIED))))
    }

    /// The pipe at `at`, opened for writing. Between one knock and the next instance the pipe is busy, which is a
    /// wait, not a refusal (the client's half of the named-pipe contract).
    pub(super) fn connect(at: At) -> io::Result<std::fs::File> {
        let busy = windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
        let until = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(at.name()) {
                Err(error) if error.raw_os_error() == i32::try_from(busy).ok() && std::time::Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                opened => return opened,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chatkord-instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_second_run_asks_the_first_to_come_forward_and_does_not_start() {
        let here = scratch("second");
        let mut first = claim_in(&here, None).expect("the first run claims");
        let (told, heard) = mpsc::channel();
        first
            .answer(move |activation, reply| {
                told.send(activation).expect("heard");
                reply.done();
            })
            .expect("answering");
        assert_eq!(claim_in(&here, None).unwrap_err(), Elsewhere::Told);
        let told = heard.recv_timeout(Duration::from_secs(5)).expect("the first run was asked to come forward");
        assert_eq!(told, None, "a run launched without a token hands none over");
        drop(first);
        claim_in(&here, None).expect("once the first is gone, a new run claims");
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_knock_the_running_window_never_answers_is_said_unanswered() {
        let here = scratch("unanswered");
        let mut first = claim_in(&here, None).expect("the first run claims");
        let (told, heard) = mpsc::channel();
        first
            .answer(move |activation, reply| {
                // Dropped unsaid: the window never handled it.
                told.send(activation).expect("heard");
                drop(reply);
            })
            .expect("answering");
        assert_eq!(claim_in(&here, None).unwrap_err(), Elsewhere::Unanswered);
        assert!(heard.recv_timeout(Duration::from_secs(5)).is_ok(), "the request itself was read");
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_later_run_hands_the_first_its_activation_token() {
        let here = scratch("token");
        let mut first = claim_in(&here, None).expect("the first run claims");
        let (told, heard) = mpsc::channel();
        first
            .answer(move |activation, reply| {
                told.send(activation).expect("heard");
                reply.done();
            })
            .expect("answering");
        let token = Activation::new("k".repeat(activation::MAX)).unwrap();
        assert_eq!(claim_in(&here, Some(&token)).unwrap_err(), Elsewhere::Told);
        let told = heard.recv_timeout(Duration::from_secs(5)).expect("the first run was asked to come forward");
        assert_eq!(told, Some(token), "the first run takes the focus with the later run's token");
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_run_handed_over_to_claims_the_window_though_the_first_still_answers() {
        let here = scratch("handed");
        let mut first = claim_in(&here, None).expect("the first run claims");
        first.answer(|_, reply| reply.done()).expect("answering");
        let next = first.hand_over(|| claim_in(&here, None));
        assert!(next.is_ok(), "the next run knocked instead: {next:?}");
        drop(next);
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_socket_left_by_a_run_that_is_gone_does_not_stop_the_next() {
        let here = scratch("left");
        private_folder(&here).unwrap();
        std::fs::write(here.join(KNOCK), b"").unwrap();
        claim_in(&here, None).expect("a stale socket file is not a run");
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn only_the_one_request_is_answered() {
        use std::io::Write;
        let here = scratch("request");
        let mut first = claim_in(&here, None).expect("claimed");
        let (told, heard) = mpsc::channel();
        first
            .answer(move |activation, reply| {
                told.send(activation).expect("heard");
                reply.done();
            })
            .expect("answering");
        let over = [COME_FORWARD, b" ", &[b'k'; activation::MAX + 1]].concat();
        let spaced = [COME_FORWARD, b" a b"].concat();
        for said in [b"activate-and-more".as_slice(), &[b'a'; 1024], b"act", b"activate ", &over, &spaced] {
            #[cfg(unix)]
            let mut stream = std::os::unix::net::UnixStream::connect(here.join(KNOCK)).unwrap();
            #[cfg(windows)]
            let mut stream = {
                let at: pipe::At = std::fs::read_to_string(here.join(KNOCK)).unwrap().parse().unwrap();
                pipe::connect(at).unwrap()
            };
            stream.write_all(said).unwrap();
        }
        assert!(heard.recv_timeout(Duration::from_millis(500)).is_err(), "nothing but the request comes forward");
        let _ = std::fs::remove_dir_all(&here);
    }
}
