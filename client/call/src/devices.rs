//! The devices a call's voice plays on: ours ([`webrtc::chain`]) on the system's sound server through
//! `sound::voice`, each opened by its id (PipeWire's `node.name`, WASAPI's endpoint id). They are listed by the process's
//! sound ([`::sound::Sound`]), which answers from a thread of its own.

use crate::sound::LISTING_WITHIN;
use std::sync::Arc;
use webrtc::chain::Sound;
use webrtc::{Device, Engine, VoiceRefused};
use words::Failure;

/// What an engine's voice is built on: ours.
fn sound() -> Arc<dyn Sound> {
    Arc::new(ours::Ours(::sound::Sound::shared()))
}

/// An engine on this system's devices, its microphone processed as a new device's until a call or the probe hands
/// it the person's choice. It blocks, up to a second or two.
pub fn engine() -> Engine {
    Engine::start(sound(), crate::processing::of(&choices::VoiceProcessing::default()))
}

/// [`engine`] on the blocking pool.
pub async fn build() -> Result<Engine, VoiceRefused> {
    Engine::build(sound(), crate::processing::of(&choices::VoiceProcessing::default())).await
}

/// The microphones, then the speakers, the process's sound lists, asked of its thread for no longer than
/// [`LISTING_WITHIN`]: a sound server that does not answer in it is said, and whoever asked goes on. A sound server
/// that is not there lists nothing.
pub async fn listed() -> Result<(Vec<Device>, Vec<Device>), Failure> {
    listed_by(&::sound::Sound::shared()).await
}

/// The same, asked of `sound`.
pub async fn listed_by(sound: &::sound::Sound) -> Result<(Vec<Device>, Vec<Device>), Failure> {
    match tokio::time::timeout(LISTING_WITHIN, sound.listing()).await {
        Ok(listing) => Ok(webrtc::lists(listing.unwrap_or_default())),
        Err(_) => {
            report::note!(
                warn,
                "the sound server did not list its devices within its bound",
                bound_ms = report::millis(LISTING_WITHIN),
            );
            Err(Failure::EngineStalled)
        }
    }
}

mod ours {
    use ::sound::voice::{self, VoiceError};
    use std::thread::Thread;
    use std::time::{Duration, Instant};
    use webrtc::DevicesRefused;
    use webrtc::chain::{self, Listing, Mono, Sound, Stereo};

    // Both ends cut sound into the same 10 ms frames.
    const _: () = assert!(voice::FRAME == chain::FRAME && voice::RATE == chain::RATE_HZ);

    fn refused(why: VoiceError) -> DevicesRefused {
        match why {
            VoiceError::Unavailable => DevicesRefused::Unavailable,
            VoiceError::Stream => DevicesRefused::Refused,
        }
    }

    /// The sound server's devices, opened through `sound::voice` and listed by the process's sound.
    pub(super) struct Ours(pub(super) ::sound::Sound);

    impl Sound for Ours {
        fn microphone(&self, device: Option<&str>, wake: Thread) -> chain::MicrophoneOpened {
            let (microphone, frames) = voice::Microphone::open(device, Some(wake)).map_err(refused)?;
            Ok((Box::new(Microphone(microphone)), Box::new(Heard(frames))))
        }

        fn speaker(&self, device: Option<&str>, now: Instant) -> chain::SpeakerOpened {
            let (speaker, voiced) = voice::Speaker::open(device, now).map_err(refused)?;
            Ok((Box::new(Speaker(speaker)), Box::new(Playing(voiced))))
        }

        fn listing(&self) -> Result<Listing, DevicesRefused> {
            self.0.listing_blocking().map_err(refused)
        }

        fn ahead(&self) -> Option<Box<dyn std::any::Any>> {
            Some(Box::new(voice::realtime()))
        }
    }

    struct Microphone(voice::Microphone);

    impl chain::Microphone for Microphone {
        fn keep(&mut self, now: Instant) {
            self.0.keep(now);
        }

        fn choose(&mut self, device: Option<&str>) -> Result<(), DevicesRefused> {
            self.0.choose(device).map_err(refused)
        }

        fn is_open(&self) -> bool {
            self.0.health() != voice::Health::Stopped
        }

        fn dropped(&self) -> u64 {
            self.0.dropped()
        }
    }

    struct Heard(voice::Frames);

    impl chain::MicrophoneEnd for Heard {
        fn frame(&mut self) -> Option<Mono> {
            self.0.frame()
        }

        fn delay(&self) -> Duration {
            self.0.delay()
        }
    }

    struct Speaker(voice::Speaker);

    impl chain::Speaker for Speaker {
        fn keep(&mut self, now: Instant) {
            self.0.keep(now);
        }

        fn choose(&mut self, device: Option<&str>) -> Result<(), DevicesRefused> {
            self.0.choose(device).map_err(refused)
        }

        fn is_open(&self) -> bool {
            self.0.health() != voice::Health::Stopped
        }

        fn losses(&self) -> (u64, u64) {
            self.0.losses()
        }
    }

    struct Playing(voice::Voiced);

    impl chain::SpeakerEnd for Playing {
        fn play(&mut self, frame: &Stereo) {
            self.0.play(frame);
        }

        fn delay(&self) -> Duration {
            self.0.delay()
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use testserver::sound::Lab;
    use tier::skipped;
    use webrtc::DevicesRefused;

    #[test]
    fn the_devices_are_listed_while_the_blocking_pool_is_full() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .expect("a runtime");
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (running, started) = tokio::sync::oneshot::channel();
        let pool = runtime.spawn_blocking(move || {
            let _ = running.send(());
            let _ = held.recv();
        });
        let listing = ::sound::voice::Listing {
            microphones: vec![::sound::voice::Listed { id: "mic".into(), label: "Microphone".into() }],
            ..Default::default()
        };
        let sound = ::sound::Sound::stand_in(move || Ok(listing.clone()));
        let listed = runtime.block_on(async {
            started.await.expect("the pool's only thread is held");
            tokio::time::timeout(LISTING_WITHIN, listed_by(&sound)).await
        });
        drop(release);
        runtime.block_on(pool).expect("the held thread ends");
        let (inputs, outputs) = listed.expect("answered within its bound").expect("a listing");
        assert_eq!(inputs.iter().filter_map(Device::asked_as).collect::<Vec<_>>(), ["mic"]);
        assert!(outputs.first().is_some_and(Device::is_default), "the default is first");
    }

    mod lab {
        use super::*;

        #[test]
        fn every_device_is_listed_by_the_name_it_is_opened_by_and_the_default_first() {
            let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
            let Some(made) = lab.make("listed_ours") else { return skipped("no sound server to make devices on") };
            let engine = engine();
            let devices = engine.devices().expect("ours");
            for (listed, name) in [(devices.outputs(), &made.sink), (devices.inputs(), &made.source)] {
                let first = listed.first().expect("at least the default and the one made");
                assert!(first.is_default(), "the default first: {listed:?}");
                assert!(!first.label().is_empty(), "named by the device it follows: {first:?}");
                assert_eq!(listed.iter().filter(|device| device.is_default()).count(), 1);
                assert!(listed.iter().any(|device| device.asked_as() == Some(name.as_str())), "{listed:?}");
            }
            assert_eq!(devices.choose_output(Some(&made.sink)), Ok(()));
            assert_eq!(devices.choose_output(None), Ok(()), "the default is chosen without a name");
            assert_eq!(devices.choose_output(Some("nobody's")), Err(DevicesRefused::Missing));
        }
    }
}
