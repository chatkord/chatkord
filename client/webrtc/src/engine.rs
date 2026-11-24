//! The engine every call is made from. Building it takes a second or more the first time in a process and some
//! 160 ms each time after (threads, codec and device probing), so a host builds one at its first voice use and every
//! [`crate::Voice`] borrows it. Until a device is asked of it, its chain's threads sleep.

use crate::chain::{Chain, STILL_AFTER, Sound};
use crate::devices::{Devices, DevicesRefused};
use crate::processing::Processing;
use crate::voice::VoiceRefused;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use std::sync::Arc;
use std::time::Duration;

/// The engine's factory, shared by every call of the host that made it, and the voice's sound: ours
/// ([`crate::chain`]) on the devices the host handed in, refused when its thread would not start.
#[derive(Clone)]
pub struct Engine {
    pub(crate) factory: PeerConnectionFactory,
    pub(crate) chain: Result<Arc<Chain>, DevicesRefused>,
}

impl Engine {
    /// Builds the engine, its voice on `sound`'s devices through [`crate::chain`], the microphone through
    /// `processing`'s steps until told otherwise ([`crate::Devices::process`]). It blocks, up to a second or two: run
    /// it on the blocking pool.
    pub fn start(sound: Arc<dyn Sound>, processing: Processing) -> Self {
        Self::bounded(sound, processing, STILL_AFTER)
    }

    /// [`Engine::start`], with `give_up_after` in place of [`STILL_AFTER`] as how long a device's keeper may be in one
    /// call before a fresh one takes its place.
    pub(crate) fn bounded(sound: Arc<dyn Sound>, processing: Processing, give_up_after: Duration) -> Self {
        let factory = PeerConnectionFactory::default();
        let chain = Chain::start(sound, processing, factory.clone(), give_up_after).map(Arc::new);
        Self { factory, chain }
    }

    /// A hold on the audio devices, to list and choose them with or without a call.
    pub fn devices(&self) -> Result<Devices, DevicesRefused> {
        Devices::open(self)
    }

    /// [`Engine::start`] on the blocking pool, for a host that is async: the one place that hands it there.
    pub async fn build(sound: Arc<dyn Sound>, processing: Processing) -> Result<Self, VoiceRefused> {
        tokio::task::spawn_blocking(move || Self::start(sound, processing)).await.map_err(|_| VoiceRefused::Engine)
    }

    /// Our chain, or why it did not start.
    pub(crate) fn chain(&self) -> Result<&Arc<Chain>, DevicesRefused> {
        self.chain.as_ref().map_err(|refused| *refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_engine_is_built_on_the_blocking_pool_and_serves_a_call() {
        let engine = Engine::build(crate::chain::tests::silent(), Processing::PROCESSOR).await.expect("built");
        assert!(engine.factory.create_peer_connection(Default::default()).is_ok());
    }

    #[cfg(target_os = "linux")]
    mod lab {
        use super::*;
        use tier::skipped;

        /// Whether a shared object of the video drivers is mapped in this process.
        fn video_driver_loaded() -> Option<String> {
            let maps = std::fs::read_to_string("/proc/self/maps").expect("this process's maps read");
            maps.lines()
                .filter_map(|line| line.split_whitespace().nth(5))
                .find(|path| {
                    let name = path.rsplit('/').next().unwrap_or(path);
                    name.starts_with("libva.so") || name.starts_with("libva-drm.so") || name.contains("_drv_video.so")
                })
                .map(str::to_owned)
        }

        #[test]
        fn starting_the_engine_loads_no_video_driver() {
            tier::lab();
            let nodes = std::fs::read_dir("/dev/dri").map(|dir| {
                dir.filter_map(Result::ok).any(|node| node.file_name().to_string_lossy().starts_with("renderD"))
            });
            if !nodes.unwrap_or(false) {
                return skipped("no render node a video driver would open");
            }
            assert_eq!(video_driver_loaded(), None, "nothing loaded a video driver before the engine");
            let _engine = Engine::start(crate::chain::tests::silent(), Processing::PROCESSOR);
            assert_eq!(video_driver_loaded(), None, "the engine opened no video driver of its own");
        }
    }
}
