//! The thread that speaks to the system's sound server for the process, and the handle that asks it. Listing the
//! devices and the applications, telling the devices' changes and playing a chime are asks to it: each is answered
//! on a `oneshot` (or, for the chain's synchronous listing, on a channel waited on), so an async caller never holds a
//! pool thread while the server is slow, and one slow ask delays only the asks behind it, which their askers bound.

use crate::host::{Call, Host, Served};
use crate::share::{App, Listen, SoundError};
use crate::speaker::{Chimes, PlayError};
use crate::voice::{Listing, VoiceError};
use crate::watch::{Changes, Listening, Unavailable, Wake};
use screens::Chime;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// How often a chime playing is looked at for having drained, to take its stream down.
const DRAINED_EVERY: Duration = Duration::from_millis(50);

/// Where an answer goes: to a task awaiting it, or to a thread waiting for it.
enum Reply<T> {
    Task(oneshot::Sender<T>),
    Thread(SyncSender<T>),
}

impl<T> Reply<T> {
    fn send(self, answer: T) {
        // An asker that stopped waiting is not told.
        match self {
            Self::Task(to) => drop(to.send(answer)),
            Self::Thread(to) => drop(to.send(answer)),
        }
    }
}

/// What the owner thread is asked.
enum Ask {
    /// The devices the sound server lists now.
    Listing(Reply<Result<Listing, VoiceError>>),
    /// Wake-ups for the devices coming and going.
    Watch(oneshot::Sender<Result<Changes, Unavailable>>),
    /// The chimes' player connected now.
    Ready(oneshot::Sender<Result<(), PlayError>>),
    /// One chime, on the speaker with the id `speaker`, or the default.
    Chime { chime: Chime, speaker: Option<String>, done: oneshot::Sender<Result<(), PlayError>> },
    /// The applications whose sound a share may carry, as the system lists them.
    Apps(oneshot::Sender<Result<Vec<App>, SoundError>>),
    /// The sound server's registry changed, or its connection ended: look at the listening.
    Registry,
    /// Every handle is gone.
    Stop,
}

/// The handles' shared end: the last one to go stops the thread.
struct Inner {
    asks: Sender<Ask>,
    #[cfg(any(test, feature = "testing"))]
    served: Served,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.asks.send(Ask::Stop);
    }
}

/// The process's sound: a handle to the one thread that lists the sound server's devices, tells their changes and
/// plays the window's chimes. Cloning it shares the thread; the thread ends with the last handle.
#[derive(Clone)]
pub struct Sound {
    inner: Arc<Inner>,
}

impl Sound {
    /// The process's one sound, started by the first to ask.
    pub fn shared() -> Self {
        static SHARED: OnceLock<Sound> = OnceLock::new();
        SHARED.get_or_init(Self::start).clone()
    }

    /// A sound of its own, on the system's sound server. A thread that cannot start is a sound that answers
    /// unavailable.
    pub fn start() -> Self {
        Self::serving(Host::listing)
    }

    /// A sound whose listing is `listing`, so a test needs no sound server.
    #[cfg(any(test, feature = "testing"))]
    pub fn stand_in(mut listing: impl FnMut() -> Result<Listing, VoiceError> + Send + 'static) -> Self {
        Self::serving(move |_| listing())
    }

    /// Which thread this sound's asks ran on.
    #[cfg(any(test, feature = "testing"))]
    pub fn served(&self) -> Served {
        self.inner.served.clone()
    }

    fn serving(listing: impl FnMut(&Host) -> Result<Listing, VoiceError> + Send + 'static) -> Self {
        let (asks, asked) = mpsc::channel();
        let wake: Wake = {
            let asks = asks.clone();
            Arc::new(move || drop(asks.send(Ask::Registry)))
        };
        let served = Served::default();
        let ran = served.clone();
        // Without a thread the channel is closed, and every ask finds the sound unavailable.
        let _ = thread::Builder::new().name("chatkord-sound".into()).spawn(move || serve(&asked, &wake, &ran, listing));
        Self {
            inner: Arc::new(Inner {
                asks,
                #[cfg(any(test, feature = "testing"))]
                served,
            }),
        }
    }

    /// The microphones and speakers the sound server lists now, and its defaults. Bound it by the time you can wait.
    pub async fn listing(&self) -> Result<Listing, VoiceError> {
        let (reply, answer) = oneshot::channel();
        self.inner.asks.send(Ask::Listing(Reply::Task(reply))).map_err(|_| VoiceError::Unavailable)?;
        answer.await.unwrap_or(Err(VoiceError::Unavailable))
    }

    /// The same for a thread that is not a task, such as the voice chain's: it waits for the answer.
    pub fn listing_blocking(&self) -> Result<Listing, VoiceError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.inner.asks.send(Ask::Listing(Reply::Thread(reply))).map_err(|_| VoiceError::Unavailable)?;
        answer.recv().unwrap_or(Err(VoiceError::Unavailable))
    }

    /// Wake-ups for this system's sound devices coming and going, or why there are none. The first holder after
    /// none is told once of the devices already there.
    pub async fn watch(&self) -> Result<Changes, Unavailable> {
        let (reply, answer) = oneshot::channel();
        self.inner.asks.send(Ask::Watch(reply)).map_err(|_| Unavailable)?;
        answer.await.unwrap_or(Err(Unavailable))
    }

    /// Connects the chimes' player to the sound server now, so the first chime plays at once.
    pub async fn ready(&self) -> Result<(), PlayError> {
        let (done, answer) = oneshot::channel();
        self.inner.asks.send(Ask::Ready(done)).map_err(|_| PlayError::Closed)?;
        answer.await.unwrap_or(Err(PlayError::Closed))
    }

    /// Plays `chime` on the speaker with the sound server's id `speaker`, or the default; one no longer there plays
    /// on the default. It resolves once the sound has started, the newest in place of one still playing.
    pub async fn chime(&self, chime: Chime, speaker: Option<String>) -> Result<(), PlayError> {
        let (done, answer) = oneshot::channel();
        self.inner.asks.send(Ask::Chime { chime, speaker, done }).map_err(|_| PlayError::Closed)?;
        answer.await.unwrap_or(Err(PlayError::Closed))
    }

    /// The applications playing now, whose sound a share may carry.
    pub async fn apps(&self) -> Result<Vec<App>, SoundError> {
        let (reply, answer) = oneshot::channel();
        self.inner.asks.send(Ask::Apps(reply)).map_err(|_| SoundError::Unavailable)?;
        answer.await.unwrap_or(Err(SoundError::Unavailable))
    }
}

/// The asks for the applications waiting on one listen, all answered with what it heard.
struct Gathering {
    listen: Listen,
    replies: Vec<oneshot::Sender<Result<Vec<App>, SoundError>>>,
}

impl Gathering {
    fn answer(self) {
        let apps = self.listen.heard();
        for reply in self.replies {
            // An asker that stopped waiting is not told.
            let _ = reply.send(Ok(apps.clone()));
        }
    }
}

/// Plays `chime`, rendered whole, from `chimes`.
fn chime_of(chimes: &mut Chimes<'_>, chime: Chime, speaker: Option<&str>) -> Result<(), PlayError> {
    chimes.play(crate::chime::sound(chime), speaker)
}

/// The listening for `Ask::Watch`: kept while a holder has it, started again by the next after none.
fn watching(listening: &mut Option<Listening>, host: &Host, wake: &Wake) -> Result<Changes, Unavailable> {
    let started = !listening.as_ref().is_some_and(Listening::held);
    if started {
        *listening = Some(Listening::start(host, Arc::clone(wake))?);
    }
    listening.as_ref().map(|listening| listening.changes(started)).ok_or(Unavailable)
}

/// The thread's whole life: asks answered one at a time, the chime playing looked at for having drained, the
/// registry looked at for having been lost, the applications' listen read once it has heard enough.
fn serve(
    asked: &Receiver<Ask>,
    wake: &Wake,
    served: &Served,
    mut listing: impl FnMut(&Host) -> Result<Listing, VoiceError>,
) {
    #[cfg(windows)]
    let Some(host) = Host::enter(served.clone()) else { return };
    #[cfg(target_os = "linux")]
    let host = Host::enter(served.clone());
    let mut chimes = Chimes::new(host.stage());
    let mut listening: Option<Listening> = None;
    let mut gathering: Option<Gathering> = None;
    loop {
        let drained = chimes.playing().then(|| Instant::now() + DRAINED_EVERY);
        let retry = listening.as_ref().and_then(Listening::due);
        let heard = gathering.as_ref().map(|gathering| gathering.listen.until());
        let ask = match drained.into_iter().chain(retry).chain(heard).min() {
            None => match asked.recv() {
                Ok(ask) => Some(ask),
                Err(_) => return,
            },
            Some(at) => match asked.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(ask) => Some(ask),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
        match ask {
            Some(Ask::Stop) => return,
            Some(Ask::Listing(reply)) => reply.send(listing(&host)),
            Some(Ask::Watch(reply)) => drop(reply.send(watching(&mut listening, &host, wake))),
            Some(Ask::Ready(done)) => drop(done.send(chimes.open())),
            Some(Ask::Chime { chime, speaker, done }) => {
                served.record(Call::Chime);
                // A caller that stopped waiting has no answer to take.
                let _ = done.send(chime_of(&mut chimes, chime, speaker.as_deref()));
            }
            Some(Ask::Apps(reply)) => match gathering.as_mut() {
                Some(gathering) => gathering.replies.push(reply),
                None => match host.apps(Instant::now()) {
                    Ok(listen) => gathering = Some(Gathering { listen, replies: vec![reply] }),
                    Err(refused) => drop(reply.send(Err(refused))),
                },
            },
            Some(Ask::Registry) | None => {}
        }
        if let Some(heard) = gathering.take_if(|gathering| gathering.listen.until() <= Instant::now()) {
            heard.answer();
        }
        chimes.settle();
        if let Some(kept) = listening.as_mut() {
            if kept.held() {
                kept.keep(Instant::now());
            } else {
                listening = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::Listed;

    fn listing() -> Listing {
        Listing {
            microphones: vec![Listed { id: "mic".into(), label: "Microphone".into() }],
            speakers: vec![Listed { id: "left".into(), label: "Speaker".into() }],
            default_microphone: Some("mic".into()),
            default_speaker: None,
        }
    }

    /// The listing is asked of the owner thread, never of the blocking pool, so a pool with no thread free still
    /// has its answer: the one thread it may have is held for the whole test.
    #[test]
    fn a_listing_is_answered_while_the_blocking_pool_is_full() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .expect("a runtime");
        let (held, release) = std::sync::mpsc::channel::<()>();
        let (running, started) = oneshot::channel();
        let pool = runtime.spawn_blocking(move || {
            let _ = running.send(());
            let _ = release.recv();
        });
        let sound = Sound::stand_in(|| Ok(listing()));
        let answer = runtime.block_on(async {
            started.await.expect("the pool's only thread is held");
            tokio::time::timeout(Duration::from_secs(5), sound.listing()).await
        });
        drop(held);
        runtime.block_on(pool).expect("the held thread ends");
        assert_eq!(answer, Ok(Ok(listing())));
    }

    #[test]
    fn a_thread_that_is_not_a_task_is_answered_too_and_a_dropped_sound_ends_the_thread() {
        let sound = Sound::stand_in(|| Ok(listing()));
        assert_eq!(sound.listing_blocking(), Ok(listing()));
        let clone = sound.clone();
        drop(sound);
        assert_eq!(clone.listing_blocking(), Ok(listing()), "a clone keeps the thread");
    }

    mod lab {
        use super::*;

        /// Windows' listing, endpoint registration, chimes and applications are COM objects of the owner's one
        /// apartment, so none runs on a thread of its own or of the caller.
        #[cfg(windows)]
        #[test]
        fn the_listing_the_watch_a_chime_and_the_applications_are_served_on_the_owners_thread() {
            tier::lab();
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
            let sound = Sound::start();
            let served = sound.served();
            runtime.block_on(async {
                let _ = sound.listing().await;
                let watch = sound.watch().await;
                let _ = sound.chime(Chime::Joined, None).await;
                let _ = sound.apps().await;
                drop(watch);
            });
            let owner = served.threads(Call::Owner);
            assert_eq!(owner.len(), 1, "the owner opened the system once");
            assert_ne!(owner[0], thread::current().id(), "and not on the caller's thread");
            for call in [Call::Listing, Call::Watch, Call::Chime, Call::Apps] {
                assert_eq!(served.threads(call), owner, "{call:?} was served on the owner's thread, once");
            }
        }

        /// Listening for the applications takes a second of the sound server's graph; the owner answers a listing
        /// asked meanwhile instead of waiting that second out.
        #[cfg(target_os = "linux")]
        #[test]
        fn a_listing_asked_while_the_applications_are_listened_for_is_answered_first() {
            tier::lab();
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
            let sound = Sound::start();
            let (listed_after, apps) = runtime.block_on(async {
                let apps = sound.apps();
                let listing = async {
                    let asked = Instant::now();
                    let _ = sound.listing().await;
                    asked.elapsed()
                };
                // Polled in order, so the applications are asked first.
                let (apps, listed_after) = tokio::join!(apps, listing);
                (listed_after, apps)
            });
            assert!(apps.is_ok(), "the applications are listed: {apps:?}");
            assert!(listed_after < Duration::from_millis(500), "the listing waited {listed_after:?}");
        }
    }

    #[test]
    fn a_refusal_of_the_sound_server_is_the_answer() {
        let sound = Sound::stand_in(|| Err(VoiceError::Unavailable));
        assert_eq!(sound.listing_blocking(), Err(VoiceError::Unavailable));
    }
}
