//! A member in a voice channel as the engine sees it: a connection to send over and one to receive over, both made
//! from one join reply. What the member sends are tracks that end apart; what they take is named by (member, kind).

use crate::codec::Codec;
use crate::devices::{Devices, DevicesRefused, Playout};
use crate::engine::Engine;
use crate::located::Located;
use crate::reach::Reach;
use crate::receive::{Encoded, Gauge, KeyFrames, Picture, Receiving, Start, WatchStats, Watching};
use crate::sending::Sending;
use crate::sharing::{Feeder, SentVideo, Sharing};
use crate::speaking::Speaking;
use crate::volume::{Mix, Volumes};
use crate::within::Step;
use link::Link;
use proto::chatkord_v1::{TrackKind, VoiceState};
use proto::video::Video;
use terms::LinkError;

/// Why a voice step did not happen.
#[derive(Debug)]
pub enum VoiceRefused {
    /// The server's offer could not be read.
    Offer,
    /// The engine would not build or set a description or a header extension, or share a codec with the router.
    Engine,
    /// That kind of track is already being sent over this connection.
    AlreadySending,
    /// The device's sound could not be used.
    Sound(DevicesRefused),
    /// The server refused a step.
    Link(LinkError),
    /// The engine did not answer this step within its bound.
    Stalled(Step),
    /// No address the server announced for its media can be sent to from here.
    Unreached {
        /// The first address announced, as the server wrote it: a name or an address.
        host: String,
        /// Its port.
        port: u16,
    },
}

/// Reads one mediasoup JSON field the server sent.
pub(crate) fn sfu_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, VoiceRefused> {
    proto::sdp::sfu_json(bytes).map_err(|_| VoiceRefused::Offer)
}

impl From<LinkError> for VoiceRefused {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

/// What the engine says of a call's connection at one moment. Every count only grows; the caller takes differences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    /// The send route's round trip, else the receive route's; none until the engine has measured one.
    pub round_trip: Option<std::time::Duration>,
    /// Audio packets received over the call so far; with `lost`, what its quality is judged on.
    pub heard: u64,
    /// Audio packets lost over the call so far.
    pub lost: u64,
    /// Packets of every stream received, audio and video.
    pub received: u64,
    /// Payload bytes of every stream received.
    pub received_bytes: u64,
    /// Packets of every stream sent.
    pub sent: u64,
    /// Payload bytes of every stream sent.
    pub sent_bytes: u64,
    /// Everything both transports sent, headers and control included.
    pub up_bytes: u64,
    /// Everything both transports received, headers and control included.
    pub down_bytes: u64,
}

/// A member's media in one voice channel: what it sends and plays, and what can be read of what it takes while
/// [`Hearing`] changes that. Dropping it closes the send connection.
pub struct Voice {
    /// The engine both connections are made from, so they share one chain.
    engine: Engine,
    sending: Sending,
    gauge: Gauge,
    /// The speaker running while the call is played.
    playing: Option<Playout>,
    /// How loud each sound [`Hearing`] takes is mixed.
    volumes: Volumes,
}

/// What a member takes of the others in one voice channel, owned apart from [`Voice`] so taking a stream never
/// waits on, or holds up, what is sent and played. Dropping it closes the receive connection.
pub struct Hearing {
    receiving: Receiving,
}

impl Voice {
    /// Prepares both directions from the reply to a join, on `engine`'s factory, over the transports `located`
    /// reached. The server's two offers are read here, once.
    pub fn new(engine: &Engine, joined: &VoiceState, located: Located) -> Result<(Self, Hearing), VoiceRefused> {
        let (Some(send), Some(receive)) = (&joined.send, &joined.receive) else {
            return Err(VoiceRefused::Offer);
        };
        let volumes = Volumes::default();
        let chain = engine.chain().ok().cloned();
        let receiving = Receiving::new(&engine.factory, receive, located.receive, volumes.clone(), chain)?;
        let voice = Self {
            sending: Sending::new(engine.factory.clone(), send, located.send)?,
            gauge: receiving.gauge(),
            engine: engine.clone(),
            playing: None,
            volumes,
        };
        Ok((voice, Hearing { receiving }))
    }

    /// The audio devices of this call.
    pub fn devices(&self) -> Result<Devices, DevicesRefused> {
        self.engine.devices()
    }

    /// Asks for what the call receives, mixed by our chain, to be played to the speakers until this voice is
    /// dropped; whether the speaker opened is in [`Devices::sounding`]. The loudness [`Voice::levels`] reports is
    /// unaffected. Playing twice does nothing.
    pub fn play(&mut self) -> Result<(), DevicesRefused> {
        if self.playing.is_none() {
            self.playing = Some(self.devices()?.play()?);
        }
        Ok(())
    }

    /// Keeps the call from the speakers, or lets it be heard again. The speaker plays on, so the window's own sounds
    /// are heard; what arrives is still pulled and measured, so [`Voice::levels`] goes on.
    pub fn set_deafened(&mut self, deafened: bool) {
        self.volumes.hear(if deafened { Mix::Deafened } else { Mix::Heard });
    }

    /// Mixes `user_id`'s microphone or share audio at `gain`, from 0.0 (silent) to 1.0 (as sent), at once and
    /// whenever it is taken again, without waiting on a take. [`Voice::levels`] still reads it as sent.
    pub fn set_volume(&self, user_id: &str, kind: TrackKind, gain: f64) {
        self.volumes.set(user_id, kind, gain);
    }

    /// Whether the call is being played to the speakers: false until the speaker asked for has opened.
    pub fn is_playing(&self) -> bool {
        self.playing.as_ref().is_some_and(Playout::is_active)
    }

    /// Sends the chosen microphone through our chain's echo cancellation, noise suppression and gain control.
    /// Ending it leaves whatever else is being sent. The send borrows nothing of this voice.
    pub fn speak(&self, link: &Link) -> impl Future<Output = Result<Speaking, VoiceRefused>> + Send + use<> {
        let (devices, sending, link) = (self.devices(), self.sending.clone(), link.clone());
        async move { Speaking::microphone(&link, &sending, devices.map_err(VoiceRefused::Sound)?).await }
    }

    /// Sends a generated tone instead of a microphone, for a device or a test with none.
    pub fn speak_tone(&self, link: &Link) -> impl Future<Output = Result<Speaking, VoiceRefused>> + Send + use<> {
        let (sending, link) = (self.sending.clone(), link.clone());
        async move { Speaking::tone(&link, &sending).await }
    }

    /// Sends a screen of `width` by `height` in `codec`; the [`Feeder`] is how its encoded units reach the engine.
    /// Ending it leaves whatever else is being sent.
    pub fn share(
        &self,
        link: &Link,
        codec: Codec,
        width: u32,
        height: u32,
    ) -> impl Future<Output = Result<(Sharing, Feeder), VoiceRefused>> + Send + use<> {
        let (sending, link) = (self.sending.clone(), link.clone());
        async move { Sharing::start(&link, &sending, codec, width, height).await }
    }

    /// Starts sending one's camera of `width` by `height` in `codec`, beside whatever else is sent, at rates of its own.
    pub fn camera(
        &self,
        link: &Link,
        codec: Codec,
        width: u32,
        height: u32,
    ) -> impl Future<Output = Result<(SentVideo, Feeder), VoiceRefused>> + Send + use<> {
        let (sending, link) = (self.sending.clone(), link.clone());
        async move { SentVideo::start(&link, &sending, Video::Camera, codec, width, height).await }
    }

    /// How the engine is taking each picture watched, by its own report, asked off the caller's loop.
    pub fn watch_stats_ask(&self) -> impl Future<Output = Result<Watching, VoiceRefused>> + Send + use<> {
        let gauge = self.gauge.clone();
        async move { gauge.watch_stats().await }
    }

    /// How the engine is taking `user_id`'s picture of `video`, by its own report; the default until it reports one.
    pub async fn watch_stats_of(&self, user_id: &str, video: Video) -> Result<WatchStats, VoiceRefused> {
        Ok(self.gauge.watch_stats().await?.remove(&(user_id.to_owned(), video)).unwrap_or_default())
    }

    /// Whether the receive connection, which carries every stream taken, carries media now.
    pub fn receive_reach(&self) -> Reach {
        self.gauge.reach()
    }

    /// Whether the send connection, which carries every track sent, carries media now.
    pub fn send_reach(&self) -> Reach {
        self.sending.reach()
    }

    /// Starts the send connection's ICE again under new credentials after its route
    /// was lost; what is sent carries on once the route is back. It borrows
    /// nothing of this voice.
    pub fn restart_send(&self, link: &Link) -> impl Future<Output = Result<(), VoiceRefused>> + Send + use<> {
        let (sending, link) = (self.sending.clone(), link.clone());
        async move { sending.restart(&link).await }
    }

    /// How the connection is doing, by the engine's own report of both connections of the call.
    pub async fn connection(&self) -> Result<Connection, VoiceRefused> {
        self.connection_ask().await
    }

    /// The same ask, on handles of its own: whoever awaits it holds nothing of this voice.
    pub fn connection_ask(&self) -> impl Future<Output = Result<Connection, VoiceRefused>> + Send + use<> {
        let (sending, gauge) = (self.sending.clone(), self.gauge.clone());
        async move { connection(&sending, &gauge).await }
    }

    /// Each microphone's loudest sample since the last ask, as a share of full scale from 0 to 100.
    pub fn levels(&self) -> Vec<(String, u8)> {
        self.gauge.levels()
    }
}

/// Both connections of a call, by the engine's own report.
async fn connection(sending: &Sending, gauge: &Gauge) -> Result<Connection, VoiceRefused> {
    let (sent, received) = tokio::join!(sending.stats(), gauge.stats());
    let (sent, received) = (sent?, received?);
    let (heard, every) = (crate::stats::inbound_audio(&received), crate::stats::inbound_every(&received));
    let (sent_packets, sent_bytes) = crate::stats::outbound_every(&sent);
    let ((up_a, down_a), (up_b, down_b)) =
        (crate::stats::transport_bytes(&sent), crate::stats::transport_bytes(&received));
    Ok(Connection {
        round_trip: crate::stats::round_trip(&sent).or_else(|| crate::stats::round_trip(&received)),
        heard: heard.packets,
        lost: heard.lost,
        received: every.packets,
        received_bytes: every.bytes,
        sent: sent_packets,
        sent_bytes,
        up_bytes: up_a.saturating_add(up_b),
        down_bytes: down_a.saturating_add(down_b),
    })
}

impl Hearing {
    /// Starts the receive connection's ICE again under new credentials after its route
    /// was lost; every stream taken carries on once the route is back.
    pub async fn restart(&mut self, link: &Link) -> Result<(), VoiceRefused> {
        self.receiving.restart(link).await
    }

    /// Takes `user_id`'s microphone or share audio, replacing one already taken. Only a microphone is metered.
    pub async fn take(&mut self, link: &Link, user_id: &str, kind: TrackKind) -> Result<(), VoiceRefused> {
        self.receiving.take(link, user_id, kind).await
    }

    /// Takes `user_id`'s picture of `video`, lending each decoded picture to `sink` before the next is taken: a slow
    /// sink makes the engine drop pictures, never queue them.
    pub async fn watch(
        &mut self,
        link: &Link,
        user_id: &str,
        video: Video,
        sink: impl FnMut(Picture<'_>) + Send + 'static,
    ) -> Result<(), VoiceRefused> {
        self.receiving.watch(link, user_id, video, sink).await
    }

    /// Takes `user_id`'s picture of `video` undecoded, each frame lent to `sink` on an engine thread (copy and return).
    /// From then on the process's engine decodes no H.264 or AV1, so a picture [`Hearing::watch`] takes after this
    /// shows nothing.
    pub async fn watch_encoded(
        &mut self,
        link: &Link,
        user_id: &str,
        video: Video,
        sink: impl Fn(Encoded<'_>) + Send + Sync + 'static,
    ) -> Result<KeyFrames, VoiceRefused> {
        self.watch_encoded_from(link, user_id, video, Start::Playing, sink).await
    }

    /// [`Hearing::watch_encoded`], the stream left held at the server when it `start`s so, until [`crate::resume`].
    pub async fn watch_encoded_from(
        &mut self,
        link: &Link,
        user_id: &str,
        video: Video,
        start: Start,
        sink: impl Fn(Encoded<'_>) + Send + Sync + 'static,
    ) -> Result<KeyFrames, VoiceRefused> {
        self.receiving.watch_encoded(link, user_id, video, start, sink).await
    }

    /// The loudest sample of `user_id`'s sound of `kind` our chain mixed since this was last asked, after its gain;
    /// none while it is not taken.
    #[cfg(any(test, feature = "testing"))]
    pub fn mixed(&self, user_id: &str, kind: TrackKind) -> Option<u32> {
        self.receiving.mixed(user_id, kind)
    }

    /// How many sections the receive connection's offer holds.
    #[cfg(any(test, feature = "testing"))]
    pub fn receive_sections(&self) -> usize {
        self.receiving.sections()
    }

    /// Stops decoding a member's stream the server has closed; true when it was being taken.
    pub fn forget(&mut self, user_id: &str, kind: TrackKind) -> bool {
        self.receiving.forget(user_id, kind)
    }

    /// Stops taking a member's stream that goes on, given back to a server that offers that so it stops sending it and
    /// the stream may be taken again.
    pub async fn release(&mut self, link: &Link, user_id: &str, kind: TrackKind) {
        self.receiving.release(link, user_id, kind).await;
    }

    /// Whose streams of `kind` are taken now.
    pub fn taken(&self, kind: TrackKind) -> Vec<String> {
        self.receiving.taken(kind)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::chain::{Sounding, Standing};
    use proto::chatkord_v1::SfuOffer;

    #[test]
    fn an_sfu_field_over_the_bound_is_refused() {
        let too_long = vec![b' '; proto::envelope::MAX_SFU_JSON_BYTES + 1];
        assert!(matches!(sfu_json::<serde_json::Value>(&too_long), Err(VoiceRefused::Offer)));
        assert!(sfu_json::<serde_json::Value>(b"{}").is_ok());
    }

    pub(crate) fn offer() -> SfuOffer {
        let transport = br#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "iceParameters": {"usernameFragment": "serverufrag", "password": "serverpassword0123456789ab", "iceLite": true},
            "iceCandidates": [{"foundation": "udpcandidate", "priority": 1076302079, "address": "127.0.0.1", "protocol": "udp", "port": 40001, "type": "host"}],
            "dtlsParameters": {"role": "auto", "fingerprints": [{"algorithm": "sha-256", "value": "AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89"}]}
        }"#;
        SfuOffer { transport: transport.to_vec(), router_rtp: br#"{"codecs": [], "headerExtensions": []}"#.to_vec() }
    }

    pub(crate) fn joined() -> VoiceState {
        VoiceState { send: Some(offer()), receive: Some(offer()), ..Default::default() }
    }

    /// The server's transport of [`offer`], reached as a server on this machine is.
    pub(crate) fn reached() -> rtc::Reached {
        let transport = rtc::Transport::parse(&offer().transport).expect("a transport");
        rtc::Reached::of(&transport, &rtc::Resolved::new(), rtc::Scope::Loopback).expect("an address")
    }

    fn located() -> Located {
        Located::of(&joined(), &rtc::Resolved::new(), rtc::Scope::Loopback).expect("located")
    }

    use crate::chain::tests::eventually;

    fn engine() -> Engine {
        Engine::start(crate::chain::tests::silent(), crate::processing::Processing::PROCESSOR)
    }

    #[test]
    fn calls_made_from_one_engine_share_its_chain() {
        let engine = engine();
        let (first, second) = (
            Voice::new(&engine, &joined(), located()).expect("a voice").0,
            Voice::new(&engine, &joined(), located()).expect("a voice").0,
        );
        let chains = (first.devices().expect("ours").chain(), second.devices().expect("ours").chain());
        assert!(std::sync::Arc::ptr_eq(&chains.0, &chains.1), "two calls, one chain");
    }

    /// A window runs call after call on one engine: the speaker of the second must start as the first's did.
    #[test]
    fn a_second_call_on_one_engine_plays_after_the_first_has_ended() {
        let engine = engine();
        for round in 0..3 {
            let mut voice = Voice::new(&engine, &joined(), located()).expect("a voice").0;
            voice.play().expect("the speaker is asked for");
            eventually(|| voice.is_playing(), &format!("call {round} plays"));
        }
    }

    #[test]
    fn letting_go_of_a_hold_leaves_the_call_playing() {
        let mut voice = Voice::new(&engine(), &joined(), located()).expect("a voice").0;
        voice.play().expect("the speaker is asked for");
        eventually(|| voice.is_playing(), "the speaker opens");
        drop(voice.devices().expect("a second hold"));
        assert!(voice.is_playing(), "a listing that ends must not stop the call's speaker");
    }

    #[test]
    fn a_microphone_stopped_leaves_the_speaker_and_the_other_microphone_running() {
        let mut voice = Voice::new(&engine(), &joined(), located()).expect("a voice").0;
        let devices = voice.devices().expect("ours");
        let (first, second) = (devices.capture().expect("a microphone"), devices.capture().expect("again"));
        voice.play().expect("the speaker is asked for");
        eventually(|| second.is_active() && voice.is_playing(), "both open");
        drop(first);
        assert!(second.is_active(), "one microphone ending leaves the other");
        assert!(voice.is_playing(), "and the speaker");
        drop(second);
        assert!(!devices.is_capturing(), "the last one closes it");
        assert!(voice.is_playing(), "the last microphone ending leaves the speaker");
    }

    #[test]
    fn a_device_chosen_while_it_runs_goes_on_running_and_a_missing_one_is_refused() {
        let mut voice = Voice::new(&engine(), &joined(), located()).expect("a voice").0;
        let devices = voice.devices().expect("ours");
        let capture = devices.capture().expect("a microphone");
        voice.play().expect("the speaker is asked for");
        eventually(|| capture.is_active() && voice.is_playing(), "both open");
        let open = Sounding { microphone: Standing::Open, speaker: Standing::Open };
        assert_eq!(devices.choose_output(Some("left")), Ok(()));
        assert_eq!(devices.choose_input(Some("mic")), Ok(()));
        eventually(|| *devices.sounding().borrow() == open, "both are moved");
        assert!(voice.is_playing(), "the speaker runs on the device it was moved to");
        assert!(capture.is_active(), "and the microphone");
        assert_eq!(devices.choose_output(Some("nobody's")), Err(DevicesRefused::Missing));
    }
}
