//! The engine a host's calls and probe run on: built at the first voice use, on this system's devices
//! ([`crate::devices::sound`]), whose owner opens them again, then kept for the process, so it never needs building
//! again. A process that never uses its voice builds none.

use crate::ChainWatch;
use std::future::Future;
use std::sync::{Arc, PoisonError};
use tokio::sync::Mutex;
use webrtc::{Engine, VoiceRefused};

/// What a host does at its first voice use with the encoder survey, which costs nothing until then: surveys it and
/// tells its window what it found.
type FirstUse = Box<dyn FnOnce(Arc<media::Survey>) + Send>;

/// One value built on first use and kept.
struct Slot<T> {
    held: Mutex<Option<T>>,
}

impl<T: Clone> Slot<T> {
    fn new() -> Self {
        Self { held: Mutex::new(None) }
    }

    /// The value, built when there is none.
    async fn get<E, F: Future<Output = Result<T, E>>>(&self, build: impl FnOnce() -> F) -> Result<T, E> {
        let mut held = self.held.lock().await;
        if let Some(kept) = held.as_ref() {
            return Ok(kept.clone());
        }
        let built = build().await?;
        *held = Some(built.clone());
        Ok(built)
    }
}

/// The engine, built the first time it is asked for, the one watch on its voice chain, and the encoder survey, kept
/// for the process.
pub struct Engines {
    engine: Slot<Engine>,
    chain: ChainWatch,
    encoders: Arc<media::Survey>,
    /// Taken by the first ask for the engine, which every voice use makes.
    first_use: std::sync::Mutex<Option<FirstUse>>,
}

impl Default for Engines {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        let encoders = media::Survey::painting(|| picture::Painting::found().render_node().map(str::to_string));
        #[cfg(not(target_os = "linux"))]
        // Windows' own start, as the window's adapter is not known yet: a host with a window uses `painting_on`.
        let encoders = media::Survey::system();
        Self::surveying(encoders)
    }
}

impl Engines {
    /// Engines whose encoder survey is of the adapter that paints the window, as `painting` says it now: the encoder,
    /// the capture and a share's card are then on one GPU, as on Linux, and a new adapter after a driver's reset is
    /// followed. A window that paints through Vulkan, or none yet, leaves the default adapter.
    #[cfg(windows)]
    pub fn painting_on(painting: impl Fn() -> picture::Painting + Send + Sync + 'static) -> Self {
        let encoders = media::Survey::painting(move || painting().device().map(|device| device.adapter().to_string()));
        Self::surveying(encoders)
    }

    fn surveying(encoders: media::Survey) -> Self {
        Self {
            engine: Slot::new(),
            chain: ChainWatch::default(),
            encoders: Arc::new(encoders),
            first_use: std::sync::Mutex::new(None),
        }
    }

    /// These engines, doing `first_use` once, with the encoder survey, at the first ask for the engine: every voice
    /// use (a call, the voice settings' probe) asks for it, so nothing of the voice runs before one.
    #[must_use]
    pub fn at_first_use(self, first_use: impl FnOnce(Arc<media::Survey>) + Send + 'static) -> Self {
        *self.first_use.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(first_use));
        self
    }

    /// Does what the first voice use does, if it is this one.
    fn first_used(&self) {
        let first = self.first_use.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(first) = first {
            first(self.encoders());
        }
    }

    /// The engine, built the first time and off the runtime's threads: it takes a second or more. The first ask is
    /// the first voice use ([`Engines::at_first_use`]).
    pub async fn engine(&self) -> Result<Engine, VoiceRefused> {
        self.first_used();
        self.engine.get(crate::devices::build).await
    }

    /// The watch on the engine's voice chain, the same for every call and probe on it.
    pub fn chain_watch(&self) -> ChainWatch {
        self.chain.clone()
    }

    /// The encoder survey every share, camera and the window's encoder list reads: made once, and again only when the
    /// devices change or an encoder fails.
    pub fn encoders(&self) -> Arc<media::Survey> {
        Arc::clone(&self.encoders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_first_voice_use_does_is_done_at_the_first_ask_and_never_again() {
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&done);
        let engines = Engines::default().at_first_use(move |_| {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let times = || done.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(times(), 0, "made, not used: nothing is surveyed");
        engines.first_used();
        assert_eq!(times(), 1, "the first use");
        engines.first_used();
        assert_eq!(times(), 1, "done once for the process");
    }

    #[tokio::test]
    async fn a_value_is_built_at_first_use_and_kept() {
        let slot = Slot::new();
        let mut builds = 0;
        let mut build = |value: u32| {
            builds += 1;
            std::future::ready(Ok::<u32, ()>(value))
        };
        assert_eq!(slot.get(|| build(1)).await, Ok(1), "built on first use");
        assert_eq!(slot.get(|| build(2)).await, Ok(1), "kept after");
        assert_eq!(builds, 1, "the kept one was never built again");
    }

    #[tokio::test]
    async fn a_build_that_fails_keeps_nothing_and_the_next_ask_builds() {
        let slot = Slot::<u32>::new();
        assert_eq!(slot.get(|| std::future::ready(Err("no engine"))).await, Err("no engine"));
        assert_eq!(slot.get(|| std::future::ready(Ok::<u32, &str>(7))).await, Ok(7));
    }
}
