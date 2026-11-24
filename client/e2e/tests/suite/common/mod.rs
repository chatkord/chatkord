//! What the end-to-end tests share: a real `chatkord-server` process, a member who has joined its voice channel, and
//! driving a call through the product.

pub(crate) mod looks;
#[cfg(target_os = "linux")]
pub(crate) mod members;
#[cfg(target_os = "linux")]
pub(crate) mod onsets;
pub(crate) mod running;

use cli::{Ask, Terminal};
use link::{Link, Trust};
use proto::chatkord_v1::event;
use proto::chatkord_v1::{ChannelsList, VoiceJoin, VoiceState};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use terms::{Heard, MemberId, Secret, ServerUrl};
use tokio::sync::{oneshot, watch};
use tokio::time::timeout;
use webrtc::{Engine, Hearing, Voice};

pub(crate) use testserver::{BOOTH, Server};

pub(crate) const PASSWORD: &str = "correct horse battery staple";

pub(crate) const WAIT: Duration = Duration::from_secs(30);

/// Registers with an invite and finds the voice channel, without joining it: the signed-in connection and the id of
/// the channel.
pub(crate) async fn registered(url: &str, name: &str, invite: &str) -> (Link, link::Events, String) {
    let (link, events, booth, _) = registered_as(url, name, invite).await;
    (link, events, booth)
}

/// The same, and the account's own id, as the server says it in the reply to registering.
pub(crate) async fn registered_as(url: &str, name: &str, invite: &str) -> (Link, link::Events, String, MemberId) {
    let url = ServerUrl::parse(url).expect("a loopback address");
    let (link, events) = Link::connect(&url, &Trust::system(), "e2e", "test").await.expect("connected");
    let (password, invite) = (Secret::new(PASSWORD.into()), Secret::new(invite.into()));
    let ack = link.register(name, Some(&name.to_uppercase()), &password, &invite).await.expect("registered");
    let Ok(channels) = link.ask(ChannelsList {}).await else { panic!("no channels") };
    let booth =
        channels.channels.iter().find(|channel| channel.name == BOOTH).expect("the voice channel this fixture made");
    (link, events, booth.channel_id.clone(), MemberId::parse(&ack.user_id).expect("the server names the account"))
}

/// Registers with an invite, finds the voice channel and joins it.
pub(crate) async fn join_voice(url: &str, name: &str, invite: &str) -> (Link, link::Events, VoiceState) {
    let (link, events, booth) = registered(url, name, invite).await;
    let Ok(state) = link.ask(VoiceJoin { channel_id: booth, ..Default::default() }).await else {
        panic!("not a voice state")
    };
    (link, events, state)
}

/// The account id the tests' calls name as their own; the server's tests do not look at it.
pub(crate) const ME: &str = "me";

/// What the window joins with: listening only, neither muted nor deafened, on the default devices.
#[expect(clippy::needless_pass_by_value, reason = "a test hands over the id the server gave, as it gave it")]
pub(crate) fn listening(channel_id: String, level_every: Duration) -> call::room::Join {
    call::room::Join {
        channel_id: terms::ChannelId::parse(&channel_id).expect("a channel id"),
        me: MemberId::parse(ME),
        source: call::room::Source::Nothing,
        microphone: None,
        speaker: None,
        processing: choices::VoiceProcessing::default(),
        level_every,
        screens: None,
        own_screen: None,
        own_camera: None,
        painting: None,
        jitter: even(),
        encoders: Arc::new(media::Survey::system()),
        chain: call::ChainWatch::default(),
    }
}

/// A jitter of one, the middle of its range, so a test's waits are the ones the call names.
pub(crate) fn even() -> screens::Jitter {
    screens::Jitter::from_bits(1 << 63)
}

/// A value a callback updates and a test waits on: every update tells the waiters, so nothing is polled.
pub(crate) struct Shared<T> {
    value: Mutex<T>,
    updates: watch::Sender<u64>,
}

impl<T> Shared<T> {
    pub(crate) fn new(value: T) -> Arc<Self> {
        Arc::new(Self { value: Mutex::new(value), updates: watch::channel(0).0 })
    }

    /// The value, to read; it is changed only by [`Shared::change`].
    pub(crate) fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, T>> {
        self.value.lock()
    }

    /// Changes the value, and tells whoever waits.
    pub(crate) fn change<R>(&self, change: impl FnOnce(&mut T) -> R) -> R {
        let made = change(&mut self.value.lock().unwrap());
        self.updates.send_modify(|updates| *updates += 1);
        made
    }

    /// What `found` finds in the value once it does, woken by each change, for no longer than `within`.
    pub(crate) async fn wait<R>(&self, within: Duration, mut found: impl FnMut(&T) -> Option<R>) -> Option<R> {
        let mut updates = self.updates.subscribe();
        timeout(within, async {
            loop {
                if let Some(found) = found(&self.value.lock().unwrap()) {
                    return found;
                }
                if updates.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        })
        .await
        .ok()
    }
}

/// What `step` comes to, or a failure naming `what` once it has run past [`WAIT`]: unbounded, a step that never ends
/// would hold the test until nextest ends it, saying nothing of where.
pub(crate) async fn within<T>(what: &str, step: impl Future<Output = T>) -> T {
    timeout(WAIT, step).await.unwrap_or_else(|_| panic!("{what}: not within {WAIT:?}"))
}

/// Waits until `holds` is true of the room's tiles, or the wait runs out; each event the room is told wakes it.
pub(crate) async fn until(room: &Arc<Shared<screens::Room>>, what: &str, holds: impl Fn(&[screens::Tile]) -> bool) {
    let found = room.wait(WAIT, |room| holds(&room.tiles(std::time::Instant::now())).then_some(())).await;
    let names: Vec<String> =
        room.lock().unwrap().tiles(std::time::Instant::now()).into_iter().map(|tile| format!("{tile:?}")).collect();
    assert!(found.is_some(), "never saw {what}; the room was {names:?}");
}

/// The clock of a made-up source that hands the engine a chunk every `period`: a tick each, the ones missed skipped.
pub(crate) fn chunk_clock(period: Duration) -> tokio::time::Interval {
    let mut clock = tokio::time::interval(period);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    clock
}

/// The newest picture `slot` holds, read in memory: its size, luma plane and stride. One left on the GPU is refused, as
/// a window that cannot import it refuses it, so the decoder hands the next ones over in memory.
pub(crate) fn luma_in(slot: &call::Slot) -> Option<((u32, u32), Vec<u8>, usize)> {
    let frame = slot.take()?;
    let read = luma_of(&frame);
    if read.is_none() {
        slot.refuse_imports();
    }
    read
}

/// A taken picture's size, its luma plane and that plane's stride, for the two kinds a CPU decoder gives; none for
/// a picture left on the GPU.
pub(crate) fn luma_of(frame: &picture::Frame) -> Option<((u32, u32), Vec<u8>, usize)> {
    match frame {
        picture::Frame::Planes(planes) => {
            Some(((planes.width, planes.height), planes.luma.clone(), planes.width as usize))
        }
        picture::Frame::InMemory(picture) => {
            let (luma, stride) = picture.samples().luma();
            Some((picture.size(), luma.to_vec(), stride as usize))
        }
        _ => None,
    }
}

/// One beat between two tries of a command-line tool (`pw-link`) that says nothing when it can succeed.
#[cfg(target_os = "linux")]
#[expect(clippy::disallowed_methods, reason = "retrying an external tool that offers no event to wait on")]
pub(crate) fn retry_beat(length: Duration) {
    std::thread::sleep(length);
}

/// The engine the calls of this test process run on, built once: each test is a call, as each call of a window is.
pub(crate) fn engine() -> &'static Engine {
    static ENGINE: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(call::devices::engine)
}

/// A member's media for a join: both connections, made from the server's reply.
pub(crate) fn voice_of(joined: &VoiceState) -> (Voice, Hearing) {
    let located = webrtc::Located::of(joined, &webrtc::Resolved::new(), webrtc::Scope::Loopback).expect("addresses");
    Voice::new(engine(), joined, located).expect("both connections")
}

/// Reads `hearing` until it reports a stream loud enough to be a tone.
pub(crate) async fn hears_a_tone(hearing: &Voice) -> u8 {
    looks::Looks::levels()
        .until(WAIT, || hearing.levels().into_iter().map(|(_, level)| level).find(|level| *level > 5))
        .await
        .expect("a tone was heard in time")
}

/// The next event of `events` that `wanted` says yes to.
pub(crate) async fn told(events: &mut link::Events, wanted: impl Fn(&event::Kind) -> bool) -> event::Kind {
    timeout(WAIT, async {
        loop {
            if let Some(Heard::Event(event)) = events.next().await
                && let Some(kind) = event.kind
                && wanted(&kind)
            {
                return kind;
            }
        }
    })
    .await
    .expect("the room said it in time")
}

/// The person at the terminal for the command line: answers the password, and stops when told.
pub(crate) struct Person {
    pub invite: String,
    pub said: Arc<Shared<Vec<String>>>,
    pub stop: Option<oneshot::Receiver<()>>,
}

impl Terminal for Person {
    fn secret(&mut self, ask: Ask) -> Pin<Box<dyn Future<Output = Result<Secret, String>> + Send>> {
        Box::pin(std::future::ready(Ok(Secret::new(match ask {
            Ask::Invite => self.invite.clone(),
            Ask::Password | Ask::PasswordAgain | Ask::NewPassword | Ask::NewPasswordAgain => PASSWORD.to_string(),
        }))))
    }

    fn say(&mut self, line: &str) {
        self.said.change(|said| said.push(line.to_string()));
    }

    fn stop(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let stop = self.stop.take();
        Box::pin(async move {
            if let Some(stop) = stop {
                let _ = stop.await;
            }
        })
    }
}
