//! The connection a member sends over, one section a track. Each change is an offer the engine writes and an answer
//! in the server's name, under one lock so two tracks starting at once cannot interleave their offers. The lock
//! guards the engine's state only: it is let go before the server is asked anything, so an ask that waits holds up
//! no other track and no reading.

use crate::bounds::{Bounds, Rates};
use crate::reach::Reach;
use crate::voice::VoiceRefused;
use crate::within::{Half, Step, within};
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::peer_connection::{OfferOptions, PeerConnection};
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::rtp_sender::RtpSender;
use libwebrtc::rtp_transceiver::{RtpTransceiver, RtpTransceiverDirection};
use libwebrtc::session_description::{SdpType, SessionDescription};
use libwebrtc::stats::RtcStats;
use link::Link;
use proto::chatkord_v1::{SfuAnswer, SfuLane, SfuOffer, TrackKind, VoiceProduce, VoiceStop};
use proto::mediasoup_types::rtp_parameters::{RtpCapabilitiesFinalized, RtpHeaderExtensionUri, RtpParameters};
use rtc::{Answered, LocalSend, Reached, dtls_from_sdp};
use std::sync::{Arc, PoisonError};
use tokio::sync::{Mutex, OnceCell, watch};

/// A member's send connection. Cloning it shares the connection, which closes with the last clone.
#[derive(Clone)]
pub(crate) struct Sending {
    inner: Arc<Inner>,
}

struct Inner {
    factory: PeerConnectionFactory,
    connection: PeerConnection,
    reach: watch::Receiver<Reach>,
    router: RtpCapabilitiesFinalized,
    state: Mutex<State>,
    /// Set once the server's transport has been given this end's DTLS parameters, which it takes once: whoever
    /// starts a track meanwhile waits for the one giving them.
    connected: OnceCell<()>,
    bounds: std::sync::Mutex<Bounds>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.connection.close();
    }
}

struct State {
    /// The server's side, whose ICE credentials a restart replaces.
    transport: Reached,
    tracks: Vec<Sent>,
    /// The first section: the bundle hangs on it, so it is never closed (as mediasoup-client does).
    tag: Option<String>,
    /// What the first section last carried, while no track is on it.
    resting: Option<RtpParameters>,
}

/// A track being sent and the section it is in.
struct Sent {
    kind: TrackKind,
    mid: String,
    parameters: RtpParameters,
    phase: Phase,
}

/// Where a track is between its start and its end, kept because the lock is let go while the server is asked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Offered and answered; the server is being told of it, or has it.
    Sent,
    /// The server is being asked to stop it; whoever else ends it has nothing left to do.
    Ending,
}

impl Sending {
    /// Prepares to send over the transport `offer` describes.
    pub(crate) fn new(
        factory: PeerConnectionFactory,
        offer: &SfuOffer,
        transport: Reached,
    ) -> Result<Self, VoiceRefused> {
        let router = crate::voice::sfu_json(&offer.router_rtp)?;
        let connection = factory.create_peer_connection(Default::default()).map_err(|_| VoiceRefused::Engine)?;
        let reach = crate::reach::watched(&connection);
        let state = Mutex::new(State { transport, tracks: Vec::new(), tag: None, resting: None });
        let (connected, bounds) = (OnceCell::new(), std::sync::Mutex::default());
        Ok(Self { inner: Arc::new(Inner { factory, connection, reach, router, state, connected, bounds }) })
    }

    /// Whether the connection carries what is sent now.
    pub(crate) fn reach(&self) -> Reach {
        *self.inner.reach.borrow()
    }

    /// Starts the connection's ICE again under new credentials from the server, after its route was lost: the engine
    /// offers a restart and takes the server's answer under them, its DTLS session kept. Before any track the
    /// credentials are only kept, for the first offer.
    pub(crate) async fn restart(&self, link: &Link) -> Result<(), VoiceRefused> {
        let ice_parameters = crate::receive::restarted(link, SfuLane::Send).await?;
        let mut state = self.inner.state.lock().await;
        state.transport.restarted(ice_parameters);
        if !self.inner.connected.initialized() {
            return Ok(());
        }
        let (_, sections) = self.offer(OfferOptions { ice_restart: true, ..OfferOptions::default() }).await?;
        self.answer(&sections, &state).await
    }

    pub(crate) fn factory(&self) -> &PeerConnectionFactory {
        &self.inner.factory
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &PeerConnection {
        &self.inner.connection
    }

    /// What the engine says of this connection at one moment.
    pub(crate) async fn stats(&self) -> Result<Vec<RtcStats>, VoiceRefused> {
        within(Step::Statistics(Half::Send), self.inner.connection.get_stats()).await?.map_err(|_| VoiceRefused::Engine)
    }

    /// States the rates the picture sent as `kind` runs between, the estimate starting at `start` if it is the one
    /// picture; the connection's bounds are every picture's summed.
    pub(crate) fn bound(&self, kind: TrackKind, rates: Rates, start: Option<u64>) -> Result<(), VoiceRefused> {
        let settings = self.inner.bounds.lock().unwrap_or_else(PoisonError::into_inner).state(kind, rates, start);
        self.inner.connection.set_bitrate(settings).map_err(|_| VoiceRefused::Engine)
    }

    /// Starts sending `track` as `kind`. `prepare` sees the sender before the offer is made, and `tune` the producer's
    /// parameters before the server or the engine is answered with them; a failure after the track was added ends it.
    pub(crate) async fn start(
        &self,
        link: &Link,
        kind: TrackKind,
        track: MediaStreamTrack,
        stream: &str,
        prepare: impl FnOnce(&RtpSender),
        tune: impl FnOnce(&mut RtpParameters),
    ) -> Result<RtpSender, VoiceRefused> {
        let (sender, text, parameters) = {
            let mut state = self.inner.state.lock().await;
            if state.tracks.iter().any(|sent| sent.kind == kind) {
                return Err(VoiceRefused::AlreadySending);
            }
            let (sender, text, sections) = self.add(&state, track, stream, prepare).await?;
            match self.place(&mut state, kind, &sections, tune).await {
                Ok(parameters) => (sender, text, parameters),
                Err(refused) => {
                    let _ = self.close_section(&mut state, kind).await;
                    return Err(refused);
                }
            }
        };
        if let Err(refused) = self.produce(link, kind, &text, &parameters).await {
            let _ = self.close_section(&mut *self.inner.state.lock().await, kind).await;
            return Err(refused);
        }
        Ok(sender)
    }

    /// Finds the section the engine put the track just offered on, keeps it as `kind`'s and answers the offer in
    /// the server's name; the producer's parameters, as `tune` left them.
    async fn place(
        &self,
        state: &mut State,
        kind: TrackKind,
        sections: &[LocalSend],
        tune: impl FnOnce(&mut RtpParameters),
    ) -> Result<RtpParameters, VoiceRefused> {
        // The section the engine put this track on (the one sending that no track of ours is in), which may be
        // the resting first one.
        let mid = self
            .inner
            .connection
            .transceivers()
            .iter()
            .filter(|transceiver| sends(transceiver))
            .filter_map(RtpTransceiver::mid)
            .find(|mid| !state.tracks.iter().any(|sent| sent.mid == *mid))
            .ok_or(VoiceRefused::Engine)?;
        let section = sections
            .iter()
            .find(|section| section.mid() == mid && !section.is_rejected())
            .ok_or(VoiceRefused::Engine)?;
        let mut parameters = section.rtp_parameters(&self.inner.router).map_err(|_| VoiceRefused::Engine)?;
        tune(&mut parameters);
        state.tag.get_or_insert_with(|| sections.first().map_or_else(|| mid.clone(), |first| first.mid().into()));
        if state.tag.as_deref() == Some(mid.as_str()) {
            state.resting = None;
        }
        state.tracks.push(Sent { kind, mid, parameters: parameters.clone(), phase: Phase::Sent });
        self.answer(sections, state).await?;
        Ok(parameters)
    }

    /// Tells the server of the track offered in `text`: this end's DTLS parameters the first time, then the
    /// producer. Nothing of the connection is held meanwhile.
    async fn produce(
        &self,
        link: &Link,
        kind: TrackKind,
        text: &str,
        parameters: &RtpParameters,
    ) -> Result<(), VoiceRefused> {
        let connect = || async {
            let dtls = serde_json::to_vec(&dtls_from_sdp(text).map_err(|_| VoiceRefused::Engine)?)
                .map_err(|_| VoiceRefused::Engine)?;
            link.ask(SfuAnswer { dtls, lane: SfuLane::Send.into() }).await?;
            Ok::<(), VoiceRefused>(())
        };
        self.inner.connected.get_or_try_init(connect).await?;
        let rtp_parameters = serde_json::to_vec(parameters).map_err(|_| VoiceRefused::Engine)?;
        link.ask(VoiceProduce { kind: kind.into(), rtp_parameters }).await?;
        Ok(())
    }

    /// Adds `track` and has the engine offer it. An offer that fails stops the track's transceiver again.
    async fn add(
        &self,
        state: &State,
        track: MediaStreamTrack,
        stream: &str,
        prepare: impl FnOnce(&RtpSender),
    ) -> Result<(RtpSender, String, Vec<LocalSend>), VoiceRefused> {
        let sender = self.inner.connection.add_track(track, &[stream]).map_err(|_| VoiceRefused::Engine)?;
        prepare(&sender);
        if let Err(refused) = self.offer_dependency_descriptor(state) {
            self.stop_unowned(state);
            return Err(refused);
        }
        match self.offer(OfferOptions::default()).await {
            Ok((text, sections)) => Ok((sender, text, sections)),
            Err(refused) => {
                self.stop_unowned(state);
                Err(refused)
            }
        }
    }

    /// Has the engine offer the AV1 dependency descriptor on the section just added: it keeps it stopped unless told,
    /// and the router tells an AV1 key frame only by it. Only a video section lists it, a share's or a camera's.
    fn offer_dependency_descriptor(&self, state: &State) -> Result<(), VoiceRefused> {
        let uri = RtpHeaderExtensionUri::DependencyDescriptor.as_str();
        for transceiver in self.inner.connection.transceivers() {
            let owned = transceiver.mid().is_some_and(|mid| state.tracks.iter().any(|sent| sent.mid == mid));
            if !sends(&transceiver) || owned {
                continue;
            }
            let mut extensions = transceiver.header_extensions_to_negotiate();
            let Some(descriptor) = extensions.iter_mut().find(|extension| extension.uri == uri) else { continue };
            if descriptor.direction == RtpTransceiverDirection::SendRecv {
                continue;
            }
            descriptor.direction = RtpTransceiverDirection::SendRecv;
            transceiver.set_header_extensions_to_negotiate(extensions).map_err(|_| VoiceRefused::Engine)?;
        }
        Ok(())
    }

    /// The section `kind` is sent in, while it is.
    pub(crate) async fn mid_of(&self, kind: TrackKind) -> Option<String> {
        self.inner.state.lock().await.tracks.iter().find(|sent| sent.kind == kind).map(|sent| sent.mid.clone())
    }

    /// Ends the track sent as `kind`, at the server first; the other tracks carry on. Ending what is not being sent,
    /// or is being ended already, is not an error.
    pub(crate) async fn end(&self, link: &Link, kind: TrackKind) -> Result<(), VoiceRefused> {
        if !self.mark(kind, Phase::Sent, Phase::Ending).await {
            return Ok(());
        }
        if let Err(refused) = link.ask(VoiceStop { kind: kind.into() }).await {
            self.mark(kind, Phase::Ending, Phase::Sent).await;
            return Err(refused.into());
        }
        self.close_section(&mut *self.inner.state.lock().await, kind).await
    }

    /// Moves `kind`'s track from the phase `from` to `to`; false when it is not sent, or not in `from`.
    async fn mark(&self, kind: TrackKind, from: Phase, to: Phase) -> bool {
        let mut state = self.inner.state.lock().await;
        let sent = state.tracks.iter_mut().find(|sent| sent.kind == kind && sent.phase == from);
        sent.map(|sent| sent.phase = to).is_some()
    }

    /// Forgets the track sent as `kind` and renegotiates without it.
    async fn close_section(&self, state: &mut State, kind: TrackKind) -> Result<(), VoiceRefused> {
        if let Some(at) = state.tracks.iter().position(|sent| sent.kind == kind) {
            let ended = state.tracks.remove(at);
            if state.tag.as_deref() == Some(ended.mid.as_str()) {
                state.resting = Some(ended.parameters);
            }
        }
        let left = self.inner.bounds.lock().unwrap_or_else(PoisonError::into_inner).forget(kind);
        if let Some(settings) = left {
            self.inner.connection.set_bitrate(settings).map_err(|_| VoiceRefused::Engine)?;
        }
        self.close_unused(state).await
    }

    /// Closes every section nothing is sent in, for the engine to reuse the slot. The first's track is only taken off,
    /// so that section rests and the connection stays up.
    async fn close_unused(&self, state: &State) -> Result<(), VoiceRefused> {
        for transceiver in self.inner.connection.transceivers() {
            let Some(mid) = transceiver.mid() else { continue };
            if state.tracks.iter().any(|sent| sent.mid == mid) {
                continue;
            }
            if state.tag.as_deref() == Some(mid.as_str()) {
                if sends(&transceiver) {
                    self.inner.connection.remove_track(transceiver.sender()).map_err(|_| VoiceRefused::Engine)?;
                }
            } else {
                // A transceiver that is already stopped has nothing more to stop.
                let _ = transceiver.stop();
            }
        }
        let (_, sections) = self.offer(OfferOptions::default()).await?;
        self.answer(&sections, state).await
    }

    /// Stops every transceiver that sends a track no `Sent` owns: the one a start added before its offer failed,
    /// which has no mid yet, so `close_unused` would never close it and the next start could take it for its own.
    fn stop_unowned(&self, state: &State) {
        for transceiver in self.inner.connection.transceivers() {
            let owned = transceiver.mid().is_some_and(|mid| state.tracks.iter().any(|sent| sent.mid == mid));
            if sends(&transceiver) && !owned {
                let _ = transceiver.stop();
            }
        }
    }

    /// The engine's next offer, as text and as the sections it holds.
    async fn offer(&self, options: OfferOptions) -> Result<(String, Vec<LocalSend>), VoiceRefused> {
        let connection = &self.inner.connection;
        let offer =
            within(Step::CreateOffer, connection.create_offer(options)).await?.map_err(|_| VoiceRefused::Engine)?;
        within(Step::SetLocal(Half::Send), connection.set_local_description(offer.clone()))
            .await?
            .map_err(|_| VoiceRefused::Engine)?;
        let text = offer.to_string();
        let sections = LocalSend::parse_all(&text).map_err(|_| VoiceRefused::Engine)?;
        Ok((text, sections))
    }

    /// Gives the engine the server's answer to `sections`: the server receives the tracks it has a
    /// producer for, the first section rests while it has none, and every other section is answered closed.
    async fn answer(&self, sections: &[LocalSend], state: &State) -> Result<(), VoiceRefused> {
        let answered: Vec<_> = sections
            .iter()
            .map(|section| {
                let sent = state.tracks.iter().find(|sent| sent.mid == section.mid());
                let resting = state.resting.as_ref().filter(|_| state.tag.as_deref() == Some(section.mid()));
                let answered = match (sent, resting) {
                    _ if section.is_rejected() => Answered::Rejected,
                    (Some(sent), _) => Answered::Receiving(&sent.parameters),
                    (None, Some(parameters)) => Answered::Resting(parameters),
                    (None, None) => Answered::Rejected,
                };
                (section, answered)
            })
            .collect();
        let text = rtc::answer(&state.transport, &answered).map_err(|_| VoiceRefused::Offer)?;
        let answer = SessionDescription::parse(&text, SdpType::Answer).map_err(|_| VoiceRefused::Engine)?;
        within(Step::SetRemote(Half::Send), self.inner.connection.set_remote_description(answer))
            .await?
            .map_err(|_| VoiceRefused::Engine)
    }
}

/// Whether the engine sends on `transceiver`: adding a track sets it to send, removing it or stopping the transceiver
/// does not. Read from the direction because the binding crashes when asked for the track of a sender that has none.
fn sends(transceiver: &RtpTransceiver) -> bool {
    matches!(transceiver.direction(), RtpTransceiverDirection::SendRecv | RtpTransceiverDirection::SendOnly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libwebrtc::audio_source::AudioSourceOptions;
    use libwebrtc::audio_source::native::NativeAudioSource;
    use libwebrtc::data_channel::DataChannelInit;
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    use libwebrtc::rtp_sender::VideoEncoderBackend;
    use libwebrtc::video_source::VideoResolution;
    use libwebrtc::video_source::native::NativeVideoSource;

    #[tokio::test]
    async fn a_track_whose_first_offer_fails_leaves_no_transceiver_sending() {
        let sending = Sending::new(
            PeerConnectionFactory::default(),
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
        )
        .expect("sending");
        // A data section is no media the server takes, so the offer it is in is refused.
        let _data = sending.connection().create_data_channel("data", DataChannelInit::default()).expect("channel");
        let source = NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 1000);
        let track = sending.factory().create_audio_track("mic", source);
        let state = sending.inner.state.lock().await;
        assert!(sending.add(&state, track.into(), "member", |_| {}).await.is_err(), "the offer has a data section");
        let left = sending.connection().transceivers().iter().filter(|transceiver| sends(transceiver)).count();
        assert_eq!(left, 0, "the track's transceiver was stopped");
    }

    #[tokio::test]
    async fn the_offer_of_a_video_carries_the_dependency_descriptor_and_one_of_a_sound_does_not() {
        let uri = RtpHeaderExtensionUri::DependencyDescriptor.as_str();
        let carries = |text: &str| text.lines().any(|line| line.starts_with("a=extmap:") && line.contains(uri));
        let sending = Sending::new(
            PeerConnectionFactory::default(),
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
        )
        .expect("sending");
        let source = NativeVideoSource::new_encoded(VideoResolution { width: 1280, height: 720 });
        let track = sending.factory().create_video_track("screen", source);
        let state = sending.inner.state.lock().await;
        let prepared = |sender: &RtpSender| sender.set_video_encoder_backend(VideoEncoderBackend::PreEncoded);
        let (_, text, _) = sending.add(&state, track.into(), "member", prepared).await.expect("the video is offered");
        assert!(carries(&text), "the offer names the extension for the router to tell a key frame by: {text}");
        let other = Sending::new(
            PeerConnectionFactory::default(),
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
        )
        .expect("sending");
        let source = NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 1000);
        let track = other.factory().create_audio_track("mic", source);
        let state = other.inner.state.lock().await;
        let (_, text, _) = other.add(&state, track.into(), "member", |_| {}).await.expect("the sound is offered");
        assert!(!carries(&text), "a sound has no use for it");
    }
}
