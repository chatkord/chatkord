//! Pinned libwebrtc, patched to hand over a frame still encoded. This crate owns the one dependency on the engine;
//! nothing else in the client names it.

mod bounds;
pub mod chain;
mod codec;
mod devices;
mod engine;
mod keeper;
#[cfg(test)]
mod local;
mod located;
mod probe;
mod processing;
mod reach;
mod receive;
mod sending;
mod sharing;
mod speaking;
mod stats;
mod tone;
mod video;
#[cfg(any(test, feature = "testing"))]
pub mod virtual_sound;
mod voice;
mod volume;
mod within;

pub use chain::{ChainHealth, Sounding, Standing, Turn};
pub use codec::Codec;
pub use devices::{Capture, Device, Devices, DevicesRefused, Playout, Test, lists};
pub use engine::Engine;
pub use keeper::{Keeper, Keeping};
pub use located::{Located, locate};
pub use probe::Probe;
pub use processing::{Gain, Noise, Processing};
pub use proto::video::Video;
pub use reach::Reach;
pub use receive::{Encoded, KeyFrames, Picture, Start, WatchStats, Watching, pause, resume};
pub use rtc::{Resolved, Scope};
pub use sharing::{Feeder, SendStats, SentVideo, Sharing, SoundFeeder};
pub use speaking::Speaking;
pub use voice::{Connection, Hearing, Voice, VoiceRefused};
pub use within::{ENGINE_WITHIN, Half, Step};

#[cfg(test)]
mod loopback;

#[cfg(test)]
mod tests {
    use libwebrtc::audio_source::AudioSourceOptions;
    use libwebrtc::audio_source::native::NativeAudioSource;
    use libwebrtc::peer_connection_factory::PeerConnectionFactory;
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;

    #[test]
    fn the_engine_links_and_makes_a_peer_connection() {
        let factory = PeerConnectionFactory::default();
        let connection = factory.create_peer_connection(Default::default());
        assert!(connection.is_ok());
    }

    #[test]
    fn a_sender_whose_track_was_removed_has_none() {
        let factory = PeerConnectionFactory::default();
        let connection = factory.create_peer_connection(Default::default()).expect("a connection");
        let source = NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 1000);
        let track = factory.create_audio_track("mic", source);
        let sender = connection.add_track(track.into(), &["stream"]).expect("track added");
        connection.remove_track(sender.clone()).expect("track removed");
        // The binding before webrtc-sys 0.3.52 dereferenced the missing track: the process ended on SIGSEGV.
        assert!(sender.track().is_none());
    }
}
