//! Other members' streams, all received over one connection renegotiated as they come. An ended stream's section is
//! taken by the next of its media kind, so the offer stays as long as the most streams taken at once.

use crate::chain::{Chain, Heard};
use crate::codec::Codec;
use crate::reach::Reach;
use crate::voice::VoiceRefused;
use crate::volume::Volumes;
use crate::within::{Half, Step, within};
use futures_util::StreamExt;
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::native::encoded_frame_tap::{EncodedFrameTap, decode_in_host};
use libwebrtc::peer_connection::PeerConnection;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::rtp_receiver::RtpReceiver;
use libwebrtc::session_description::{SdpType, SessionDescription};
use libwebrtc::stats::RtcStats;
use libwebrtc::video_frame::VideoBuffer;
use libwebrtc::video_stream::native::NativeVideoStream;
use link::Link;
use proto::chatkord_v1::{
    SfuAnswer, SfuIce, SfuLane, SfuOffer, SfuRestartIce, TrackKind, VoiceConsume, VoicePause, VoiceRelease, VoiceResume,
};
use proto::mediasoup_types::data_structures::IceParameters;
use proto::mediasoup_types::rtp_parameters::RtpParameters;
use proto::video::Video;
use rtc::{Media, Offered, Reached, RemoteOffer, dtls_from_sdp};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

const FULL_SCALE: u32 = i16::MAX as u32 + 1;
/// Tracks the engine has handed over and no request has taken yet.
const ARRIVALS_KEPT: usize = 16;

/// One decoded picture of a screen, lent for the length of the call it is given to. Each plane is
/// `stride` bytes a row; chroma planes are half the width and height, rounded up.
pub struct Picture<'a> {
    /// In pixels, as decoded.
    pub width: u32,
    /// In pixels, as decoded.
    pub height: u32,
    /// The Y plane.
    pub luma: &'a [u8],
    /// The U plane.
    pub blue: &'a [u8],
    /// The V plane.
    pub red: &'a [u8],
    /// Bytes a row of the luma, blue and red planes.
    pub strides: (u32, u32, u32),
}

/// One encoded frame of a screen as the sender made it, lent for the length of the call it is given to.
pub struct Encoded<'a> {
    /// Annex B for H.264, with the parameter sets a key frame needs; OBUs for AV1.
    pub data: &'a [u8],
    /// When the engine's timing shows it, in microseconds of its clock: rising in decode order.
    pub render_time_us: i64,
    /// Whether a decoder can start from this frame.
    pub key_frame: bool,
    /// The codec `data` is in, as the screen's consumer was given it.
    pub codec: Codec,
}

/// What the engine says of how a watched screen is being decoded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchStats {
    /// The decoder the engine chose, by its own name; empty until the first picture.
    pub decoder: String,
    /// Whether the engine counts that decoder as power efficient (a hardware decoder is).
    pub power_efficient: bool,
    /// Pictures decoded since the screen was taken.
    pub frames_decoded: u32,
    /// Time spent decoding every picture so far, in seconds.
    pub decode_seconds: f64,
    /// Pictures the engine decoded and dropped before they were shown.
    pub frames_dropped: u32,
    /// Time every picture so far was held between arriving whole and being let out, in seconds: what the engine waits
    /// for jitter and to hold the picture to its sound.
    pub held_seconds: f64,
    /// The pictures `held_seconds` is summed over.
    pub held: u64,
}

impl WatchStats {
    /// The mean time a picture was held by the engine, `seconds` summed over `pictures` of them, as two looks' differences
    /// give it; none before one was let out.
    #[expect(clippy::cast_precision_loss, reason = "a count of pictures, far under 2^52")]
    pub fn mean_held(seconds: f64, pictures: u64) -> Option<std::time::Duration> {
        let mean = seconds / pictures as f64;
        (pictures > 0).then(|| std::time::Duration::try_from_secs_f64(mean).ok()).flatten()
    }
}

/// A screen's tap, shared with its [`KeyFrames`]: the stream's end takes it out, so no handle keeps it past the
/// connection it must not outlive.
type Tapped = Arc<Mutex<Option<EncodedFrameTap>>>;

/// One stream being decoded; dropping it stops the decoding.
struct Decoding {
    task: Option<JoinHandle<()>>,
    /// The track in our chain's mix: dropped, it leaves the mix.
    _heard: Option<Heard>,
    /// Shows a screen's encoded frames to the caller instead of decoding them here.
    tap: Option<Tapped>,
}

impl Drop for Decoding {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
        if let Some(tap) = &self.tap {
            drop(tap.lock().unwrap_or_else(PoisonError::into_inner).take());
        }
    }
}

/// Asks the sender of one screen taken encoded for a key frame, for a decoder that lost its place; and says the
/// codec its frames are in.
#[derive(Clone)]
pub struct KeyFrames {
    tapped: Tapped,
    codec: Codec,
}

impl KeyFrames {
    /// False once the screen is no longer taken.
    pub fn ask(&self) -> bool {
        let tap = self.tapped.lock().unwrap_or_else(PoisonError::into_inner);
        tap.as_ref().inspect(|tap| tap.request_key_frame()).is_some()
    }

    /// The codec the screen's frames are in, as its consumer was given it.
    pub fn codec(&self) -> Codec {
        self.codec
    }
}

/// The loudest sample of each microphone taken since the last look, by its member.
type Meters = BTreeMap<String, Arc<AtomicU32>>;

/// Whose picture each offered section's mid carries, as the engine's report names a stream by its mid.
type Mids = HashMap<String, (String, Video)>;

/// What the engine says of each picture taken, by whose it is.
pub type Watching = HashMap<(String, Video), WatchStats>;

/// What can be read of the receive connection while its streams change: how it is doing and how loud each
/// microphone is. Cloning it is cheap.
#[derive(Clone)]
pub(crate) struct Gauge {
    connection: PeerConnection,
    reach: watch::Receiver<Reach>,
    meters: watch::Receiver<Meters>,
    mids: watch::Receiver<Mids>,
}

impl Gauge {
    /// Whether the connection carries media now.
    pub(crate) fn reach(&self) -> Reach {
        *self.reach.borrow()
    }

    pub(crate) fn levels(&self) -> Vec<(String, u8)> {
        self.meters
            .borrow()
            .iter()
            .map(|(user_id, loudest)| {
                let peak = loudest.swap(0, Ordering::Relaxed);
                (user_id.clone(), u8::try_from(peak * 100 / FULL_SCALE).unwrap_or(100))
            })
            .collect()
    }

    /// What the engine says of the connection at one moment.
    pub(crate) async fn stats(&self) -> Result<Vec<RtcStats>, VoiceRefused> {
        within(Step::Statistics(Half::Receive), self.connection.get_stats()).await?.map_err(|_| VoiceRefused::Engine)
    }

    /// What the engine says of each picture taken, keyed by whose it is through its section's mid.
    pub(crate) async fn watch_stats(&self) -> Result<Watching, VoiceRefused> {
        let stats = self.stats().await?;
        let mids = self.mids.borrow().clone();
        Ok(stats
            .iter()
            .filter_map(|one| match one {
                RtcStats::InboundRtp(inbound) if inbound.stream.kind == "video" => {
                    let whose = mids.get(&inbound.inbound.mid)?.clone();
                    Some((
                        whose,
                        WatchStats {
                            decoder: inbound.inbound.decoder_implementation.clone(),
                            power_efficient: inbound.inbound.power_efficient_decoder,
                            frames_decoded: inbound.inbound.frames_decoded,
                            decode_seconds: inbound.inbound.total_decode_time,
                            frames_dropped: inbound.inbound.frames_dropped,
                            held_seconds: inbound.inbound.jitter_buffer_delay,
                            held: inbound.inbound.jitter_buffer_emitted_count,
                        },
                    ))
                }
                _ => None,
            })
            .collect())
    }
}

/// A track the engine handed over, with the stream and the track id the offer named it by.
struct Arrival {
    stream_id: String,
    track_id: String,
    track: MediaStreamTrack,
    receiver: RtpReceiver,
}

/// One section the engine has been offered, and what it holds now.
#[derive(Debug, Clone)]
struct Section {
    media: Media,
    held: Held,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Held {
    /// The stream of (member, kind) arrives on it.
    Stream(String, TrackKind),
    /// Its stream ended; the next offer closes it.
    Ending,
    /// Closed in an offer the engine took, so a stream of the same media kind may take the slot.
    Closed,
    /// The first section, whose stream ended: it carries the bundle, so it stays open and idle and is never
    /// taken again.
    Resting,
}

#[cfg(test)]
impl From<(String, TrackKind)> for Held {
    fn from((user_id, kind): (String, TrackKind)) -> Self {
        Held::Stream(user_id, kind)
    }
}

impl Section {
    fn offered(&self) -> Offered {
        match self.held {
            Held::Stream(..) => Offered::Sending,
            Held::Resting => Offered::Resting,
            Held::Ending | Held::Closed => Offered::Closed,
        }
    }
}

/// The streams of other members being received over one connection. Dropping it closes the connection.
pub(crate) struct Receiving {
    connection: PeerConnection,
    transport: Reached,
    router_rtp: Vec<u8>,
    /// Every section the engine has been offered, in order; a slot is never removed from an offer.
    sections: Vec<Section>,
    /// How many sections have been made, so a mid is never used twice.
    made: u64,
    arrivals: mpsc::Receiver<Arrival>,
    reach: watch::Receiver<Reach>,
    connected: bool,
    streams: HashMap<(String, TrackKind), Decoding>,
    /// The one writer of the microphones' meters a [`Gauge`] reads.
    meters: watch::Sender<Meters>,
    /// The one writer of whose picture each mid carries, which a [`Gauge`] reads.
    mids: watch::Sender<Mids>,
    /// Each sound taken, mixed at the gain the call asks.
    volumes: Volumes,
    /// Our chain, which plays what is taken, when the voice is ours.
    chain: Option<Arc<Chain>>,
}

impl Drop for Receiving {
    fn drop(&mut self) {
        // A tap holds the engine's receiver, which must be released while the connection is still alive.
        self.streams.clear();
        self.volumes.release();
        self.connection.close();
    }
}

impl Receiving {
    /// Prepares to receive over the receive transport `offer` describes.
    pub(crate) fn new(
        factory: &PeerConnectionFactory,
        offer: &SfuOffer,
        transport: Reached,
        volumes: Volumes,
        chain: Option<Arc<Chain>>,
    ) -> Result<Self, VoiceRefused> {
        let connection = factory.create_peer_connection(Default::default()).map_err(|_| VoiceRefused::Engine)?;
        let (arrived, arrivals) = mpsc::channel(ARRIVALS_KEPT);
        let reach = crate::reach::watched(&connection);
        connection.on_track(Some(Box::new(move |event| {
            if let Some(stream) = event.streams.first() {
                // A full queue holds arrivals nobody is waiting for any more; this one is dropped like them.
                let _ = arrived.try_send(Arrival {
                    stream_id: stream.id(),
                    track_id: event.track.id(),
                    track: event.track,
                    receiver: event.receiver,
                });
            }
        })));
        Ok(Self {
            connection,
            transport,
            router_rtp: offer.router_rtp.clone(),
            sections: Vec::new(),
            made: 0,
            arrivals,
            reach,
            connected: false,
            streams: HashMap::new(),
            meters: watch::Sender::new(Meters::new()),
            mids: watch::Sender::new(Mids::new()),
            volumes,
            chain,
        })
    }

    /// What can be read of this connection while its streams change.
    pub(crate) fn gauge(&self) -> Gauge {
        Gauge {
            connection: self.connection.clone(),
            reach: self.reach.clone(),
            meters: self.meters.subscribe(),
            mids: self.mids.subscribe(),
        }
    }

    pub(crate) async fn take(&mut self, link: &Link, user_id: &str, kind: TrackKind) -> Result<(), VoiceRefused> {
        let (MediaStreamTrack::Audio(track), _, _) = self.negotiate(link, user_id, kind, Start::Playing).await? else {
            return Err(VoiceRefused::Engine);
        };
        let gain = self.volumes.taken(user_id, kind);
        let loudest = Arc::new(AtomicU32::new(0));
        let channels = if kind == TrackKind::Microphone { 1 } else { 2 };
        let heard = self.chain.as_ref().map(|chain| chain.hear(track, channels, gain, Arc::clone(&loudest)));
        self.streams.insert((user_id.to_string(), kind), Decoding { task: None, _heard: heard, tap: None });
        if kind == TrackKind::Microphone {
            self.meters.send_modify(|meters| {
                meters.insert(user_id.to_string(), loudest);
            });
        }
        Ok(())
    }

    pub(crate) async fn watch(
        &mut self,
        link: &Link,
        user_id: &str,
        video: Video,
        mut sink: impl FnMut(Picture<'_>) + Send + 'static,
    ) -> Result<(), VoiceRefused> {
        let (MediaStreamTrack::Video(track), _, _) =
            self.negotiate(link, user_id, video.kind(), Start::Playing).await?
        else {
            return Err(VoiceRefused::Engine);
        };
        let mut pictures = NativeVideoStream::new(track);
        let task = tokio::spawn(async move {
            while let Some(frame) = pictures.next().await {
                let i420 = frame.buffer.to_i420();
                let (luma, blue, red) = i420.data();
                sink(Picture { width: i420.width(), height: i420.height(), luma, blue, red, strides: i420.strides() });
            }
        });
        self.streams
            .insert((user_id.to_string(), video.kind()), Decoding { task: Some(task), _heard: None, tap: None });
        Ok(())
    }

    pub(crate) async fn watch_encoded(
        &mut self,
        link: &Link,
        user_id: &str,
        video: Video,
        start: Start,
        sink: impl Fn(Encoded<'_>) + Send + Sync + 'static,
    ) -> Result<KeyFrames, VoiceRefused> {
        decode_in_host(true);
        let (MediaStreamTrack::Video(_), receiver, Some(codec)) =
            self.negotiate(link, user_id, video.kind(), start).await?
        else {
            return Err(VoiceRefused::Engine);
        };
        let tap = EncodedFrameTap::new(&receiver, move |frame| {
            sink(Encoded { data: frame.data, render_time_us: frame.render_time_us, key_frame: frame.key_frame, codec })
        });
        let tapped = Arc::new(Mutex::new(Some(tap)));
        let decoding = Decoding { task: None, _heard: None, tap: Some(Arc::clone(&tapped)) };
        self.streams.insert((user_id.to_string(), video.kind()), decoding);
        Ok(KeyFrames { tapped, codec })
    }

    /// Asks the server for the consumer, renegotiates and resumes it unless it `start`s held, and returns the track the
    /// engine hands over, its receiver and the video codec it carries (none for sound).
    async fn negotiate(
        &mut self,
        link: &Link,
        user_id: &str,
        kind: TrackKind,
        start: Start,
    ) -> Result<(MediaStreamTrack, RtpReceiver, Option<Codec>), VoiceRefused> {
        let stream_id = stream_id(user_id, kind).ok_or(VoiceRefused::Engine)?;
        let state = link
            .ask(VoiceConsume {
                user_id: user_id.to_string(),
                kind: kind.into(),
                rtp_capabilities: self.router_rtp.clone(),
            })
            .await?;
        let consumer = rtc::consumed(&state.consumer).map_err(|_| VoiceRefused::Offer)?;
        let codec = Codec::carried_by(&consumer.rtp_parameters);
        self.forget(user_id, kind);
        let (answer, track_id) =
            self.offer(consumer.rtp_parameters, stream_id.clone(), (user_id.to_string(), kind)).await?;
        let arrived = self.arrive(link, user_id, kind, start, (&stream_id, &track_id), &answer).await;
        if arrived.is_err() {
            // The section was offered for a stream that never came: the next offer closes it.
            self.forget(user_id, kind);
        }
        arrived.map(|(track, receiver)| (track, receiver, codec))
    }

    /// Connects on the first answer, resumes the stream unless it `start`s held, and waits for the engine to hand over
    /// its track.
    async fn arrive(
        &mut self,
        link: &Link,
        user_id: &str,
        kind: TrackKind,
        start: Start,
        (stream_id, track_id): (&str, &str),
        answer: &SessionDescription,
    ) -> Result<(MediaStreamTrack, RtpReceiver), VoiceRefused> {
        if !self.connected {
            let dtls = serde_json::to_vec(&dtls_from_sdp(&answer.to_string()).map_err(|_| VoiceRefused::Engine)?)
                .map_err(|_| VoiceRefused::Engine)?;
            link.ask(SfuAnswer { dtls, lane: SfuLane::Receive.into() }).await?;
            self.connected = true;
        }
        if start == Start::Playing {
            link.ask(VoiceResume { user_id: user_id.to_string(), kind: kind.into() }).await?;
        }
        self.handed(stream_id, track_id).await
    }

    /// Waits for the engine to hand over the track of `stream_id` named `track_id`; any other arrival is dropped.
    async fn handed(
        &mut self,
        stream_id: &str,
        track_id: &str,
    ) -> Result<(MediaStreamTrack, RtpReceiver), VoiceRefused> {
        let handed = async {
            loop {
                let arrival = self.arrivals.recv().await.ok_or(VoiceRefused::Engine)?;
                if arrival.stream_id == stream_id && arrival.track_id == track_id {
                    return Ok((arrival.track, arrival.receiver));
                }
            }
        };
        within(Step::Arrival, handed).await?
    }

    /// Offers every section so far, the ended ones closed and `held`'s stream in a slot, and answers with the id its
    /// track arrives under. Sections are kept once the engine takes the offer, so a refused one is never offered again.
    async fn offer(
        &mut self,
        rtp_parameters: RtpParameters,
        stream_id: String,
        (user_id, kind): (String, TrackKind),
    ) -> Result<(SessionDescription, String), VoiceRefused> {
        let mid = self.made.to_string();
        // The engine refuses two sections with one `msid`, and a stream taken again may still rest in its old one.
        let track_id = format!("{kind:?}-{mid}");
        let media = Media { mid: mid.clone(), rtp_parameters, stream_id, track_id: track_id.clone() };
        let held = Section { media, held: Held::Stream(user_id, kind) };
        let mut sections = self.sections.clone();
        // A slot closed in an offer the engine took, of the same media kind (mediasoup-client's rule).
        match sections.iter_mut().find(|section| {
            section.held == Held::Closed && section.media.kind().is_some() && section.media.kind() == held.media.kind()
        }) {
            Some(slot) => *slot = held,
            None => sections.push(held),
        }
        let answer = self.negotiate_sections(&sections).await?;
        for section in &mut sections {
            if section.held == Held::Ending {
                section.held = Held::Closed;
            }
        }
        self.sections = sections;
        self.publish_mids();
        self.made += 1;
        Ok((answer, track_id))
    }

    /// Has the engine take `sections` offered in the server's name, under the transport's current credentials, and
    /// answer them; the answer is what it took.
    async fn negotiate_sections(&self, sections: &[Section]) -> Result<SessionDescription, VoiceRefused> {
        let offer = sections.iter().fold(RemoteOffer::new(self.transport.clone()), |offer, section| {
            offer.offered(section.media.clone(), section.offered())
        });
        let offered = SessionDescription::parse(&offer.sdp().map_err(|_| VoiceRefused::Offer)?, SdpType::Offer)
            .map_err(|_| VoiceRefused::Engine)?;
        within(Step::SetRemote(Half::Receive), self.connection.set_remote_description(offered))
            .await?
            .map_err(|_| VoiceRefused::Engine)?;
        let answer = within(Step::CreateAnswer, self.connection.create_answer(Default::default()))
            .await?
            .map_err(|_| VoiceRefused::Engine)?;
        let stereo = rtc::heard_in_stereo(&answer.to_string(), sections.iter().map(|section| &section.media));
        let answer = SessionDescription::parse(&stereo, SdpType::Answer).map_err(|_| VoiceRefused::Engine)?;
        within(Step::SetLocal(Half::Receive), self.connection.set_local_description(answer.clone()))
            .await?
            .map_err(|_| VoiceRefused::Engine)?;
        Ok(answer)
    }

    /// Starts the connection's ICE again under new credentials from the server, after its route was lost: the same
    /// sections are offered under them, and the engine checks the route anew with its DTLS session kept. Before any
    /// section the credentials are only kept, for the first offer.
    pub(crate) async fn restart(&mut self, link: &Link) -> Result<(), VoiceRefused> {
        let ice_parameters = restarted(link, SfuLane::Receive).await?;
        self.transport.restarted(ice_parameters);
        if self.sections.is_empty() {
            return Ok(());
        }
        let sections = self.sections.clone();
        self.negotiate_sections(&sections).await.map(drop)
    }

    /// The stream's section is closed by the next offer, or rests if it carries the bundle.
    pub(crate) fn forget(&mut self, user_id: &str, kind: TrackKind) -> bool {
        let ended = Held::Stream(user_id.to_string(), kind);
        for (index, section) in self.sections.iter_mut().enumerate() {
            if section.held == ended {
                section.held = if index == 0 { Held::Resting } else { Held::Ending };
            }
        }
        self.publish_mids();
        if kind == TrackKind::Microphone {
            self.meters.send_if_modified(|meters| meters.remove(user_id).is_some());
        }
        self.volumes.gone(user_id, kind);
        self.streams.remove(&(user_id.to_string(), kind)).is_some()
    }

    /// Tells the gauge whose picture each section carries now.
    fn publish_mids(&self) {
        let mids = self
            .sections
            .iter()
            .filter_map(|section| match &section.held {
                Held::Stream(user_id, kind) => {
                    let video = picture_of(*kind)?;
                    Some((section.media.mid.clone(), (user_id.clone(), video)))
                }
                Held::Ending | Held::Closed | Held::Resting => None,
            })
            .collect();
        self.mids.send_replace(mids);
    }

    pub(crate) async fn release(&mut self, link: &Link, user_id: &str, kind: TrackKind) {
        self.forget(user_id, kind);
        // A stream that ended meanwhile has nothing to give back: the outcome is the same.
        let _ = link.ask(VoiceRelease { user_id: user_id.to_string(), kind: kind.into() }).await;
    }

    /// The loudest sample of `user_id`'s sound of `kind` our chain mixed since this was last asked, after its gain;
    /// none while it is not taken into a chain.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn mixed(&self, user_id: &str, kind: TrackKind) -> Option<u32> {
        self.streams.get(&(user_id.to_string(), kind))?._heard.as_ref().map(Heard::mixed)
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn sections(&self) -> usize {
        self.sections.len()
    }

    /// Whose streams of `kind` are taken now.
    pub(crate) fn taken(&self, kind: TrackKind) -> Vec<String> {
        self.streams.keys().filter(|(_, taken)| *taken == kind).map(|(user_id, _)| user_id.clone()).collect()
    }
}

/// New ICE credentials for `lane`'s transport, from the server.
pub(crate) async fn restarted(link: &Link, lane: SfuLane) -> Result<IceParameters, VoiceRefused> {
    let SfuIce { ice_parameters } = link.ask(SfuRestartIce { lane: lane.into() }).await?;
    crate::voice::sfu_json(&ice_parameters)
}

/// The picture a track of `kind` carries; none for sound.
fn picture_of(kind: TrackKind) -> Option<Video> {
    match kind {
        TrackKind::ScreenVideo => Some(Video::Screen),
        TrackKind::Camera => Some(Video::Camera),
        TrackKind::Microphone | TrackKind::ScreenAudio | TrackKind::Unspecified => None,
    }
}

/// The engine keeps one stream's audio and video in step (lip-sync), so a share's sound and picture are one stream
/// and the microphone another, never held back to match a screen's or a camera's decode; a camera is its own.
fn stream_id(user_id: &str, kind: TrackKind) -> Option<String> {
    let group = match kind {
        TrackKind::Microphone => "voice",
        TrackKind::ScreenVideo | TrackKind::ScreenAudio => "share",
        TrackKind::Camera => "camera",
        TrackKind::Unspecified => return None,
    };
    Some(format!("{user_id}:{group}"))
}

/// Whether a stream taken plays from its start or is left held at the server until [`resume`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Start {
    /// The consumer is resumed as the stream arrives.
    #[default]
    Playing,
    /// The consumer stays as the server made it, held: nothing of the stream comes.
    Held,
}

/// Holds `user_id`'s stream of `kind`, taken already, at a server that offers that: nothing of it comes
/// until [`resume`]. Whether the server holds it.
pub async fn pause(link: &Link, user_id: &str, kind: TrackKind) -> bool {
    let asked = VoicePause { user_id: user_id.to_string(), kind: kind.into() };
    link.ask(asked).await.is_ok()
}

/// Starts again a stream [`pause`] held; the server asks its sender for a key frame. Whether it did.
pub async fn resume(link: &Link, user_id: &str, kind: TrackKind) -> bool {
    let asked = VoiceResume { user_id: user_id.to_string(), kind: kind.into() };
    link.ask(asked).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mean_a_picture_was_held_is_the_seconds_over_the_pictures_let_out() {
        assert_eq!(WatchStats::mean_held(1.5, 30), Some(std::time::Duration::from_millis(50)));
        assert_eq!(WatchStats::mean_held(0.0, 0), None, "none let out: no mean");
        assert_eq!(WatchStats::mean_held(f64::NAN, 3), None);
        assert_eq!(WatchStats::mean_held(-1.0, 3), None, "a clock that ran back is no hold");
    }

    const ADA: &str = "ada";
    const BO: &str = "bo";

    #[test]
    fn a_shares_audio_and_picture_are_one_stream_and_the_microphone_is_another() {
        let share_video = stream_id(ADA, TrackKind::ScreenVideo).expect("named");
        assert_eq!(stream_id(ADA, TrackKind::ScreenAudio), Some(share_video.clone()), "kept in step with each other");
        assert_ne!(stream_id(ADA, TrackKind::Microphone), Some(share_video), "not with the microphone");
    }

    #[test]
    fn a_camera_is_a_stream_of_its_own_so_no_voice_waits_for_it() {
        let camera = stream_id(ADA, TrackKind::Camera).expect("named");
        for kind in [TrackKind::Microphone, TrackKind::ScreenVideo] {
            assert_ne!(stream_id(ADA, kind), Some(camera.clone()), "{kind:?}");
        }
    }

    #[test]
    fn two_members_never_share_a_stream() {
        for kind in [TrackKind::Microphone, TrackKind::ScreenVideo, TrackKind::ScreenAudio, TrackKind::Camera] {
            assert_ne!(stream_id(ADA, kind), stream_id(BO, kind), "{kind:?}");
        }
    }

    #[test]
    fn a_track_with_no_kind_has_no_stream() {
        assert_eq!(stream_id(ADA, TrackKind::Unspecified), None);
    }

    /// A consumer as the server makes one: each with an SSRC of its own.
    fn opus_on(cname: &str, ssrc: u32) -> RtpParameters {
        serde_json::from_str(&format!(
            r#"{{"codecs": [{{"mimeType": "audio/opus", "payloadType": 100, "clockRate": 48000, "channels": 2,
                "parameters": {{"useinbandfec": 1}}, "rtcpFeedback": []}}],
                "headerExtensions": [], "encodings": [{{"ssrc": {ssrc}}}], "rtcp": {{"cname": "{cname}", "reducedSize": true}}}}"#
        ))
        .expect("parameters a server could send")
    }

    /// A screen's consumer as the server makes one.
    fn h264_on(cname: &str, ssrc: u32) -> RtpParameters {
        serde_json::from_str(&format!(
            r#"{{"codecs": [{{"mimeType": "video/H264", "payloadType": 102, "clockRate": 90000,
                "parameters": {{"packetization-mode": 1, "profile-level-id": "42e01f", "level-asymmetry-allowed": 1}},
                "rtcpFeedback": [{{"type": "nack"}}]}}],
                "headerExtensions": [], "encodings": [{{"ssrc": {ssrc}}}], "rtcp": {{"cname": "{cname}", "reducedSize": true}}}}"#
        ))
        .expect("parameters a server could send")
    }

    #[tokio::test]
    async fn a_share_picture_waited_for_is_never_handed_its_sound() {
        let factory = PeerConnectionFactory::default();
        let mut receiving = Receiving::new(
            &factory,
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
            Volumes::default(),
            None,
        )
        .expect("receiving");
        let share = stream_id(ADA, TrackKind::ScreenVideo).expect("named");
        let (_, _sound) = receiving
            .offer(opus_on(ADA, 1), share.clone(), (ADA.into(), TrackKind::ScreenAudio))
            .await
            .expect("the sound offered");
        let (_, picture) = receiving
            .offer(h264_on(ADA, 2), share.clone(), (ADA.into(), TrackKind::ScreenVideo))
            .await
            .expect("the picture offered");
        // Both arrived under one stream, the sound first; the picture's wait must not take it.
        let (track, _) = receiving.handed(&share, &picture).await.expect("handed over");
        assert!(matches!(track, MediaStreamTrack::Video(_)), "the wait for the picture got {}", track.id());
    }

    #[tokio::test]
    async fn a_sound_sent_in_stereo_is_decoded_in_stereo_and_a_microphone_is_not() {
        let factory = PeerConnectionFactory::default();
        let mut receiving = Receiving::new(
            &factory,
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
            Volumes::default(),
            None,
        )
        .expect("receiving");
        receiving.offer(opus_on(BO, 1), format!("{BO}:voice"), mine(BO)).await.expect("the microphone offered");
        let mut stereo = opus_on(ADA, 2);
        crate::sharing::stereo(&mut stereo);
        let share = stream_id(ADA, TrackKind::ScreenAudio).expect("named");
        receiving.offer(stereo, share, (ADA.into(), TrackKind::ScreenAudio)).await.expect("the sound offered");
        let taken = receiving.connection.current_local_description().expect("answered").to_string();
        let fmtp: Vec<bool> = taken
            .lines()
            .filter(|line| line.starts_with("a=fmtp:100 "))
            .map(|line| line.contains("stereo=1"))
            .collect();
        assert_eq!(fmtp, [false, true], "the engine took an answer asking two channels of the share's sound alone");
    }

    #[test]
    fn a_microphone_forgotten_is_gone_from_the_gauge_and_its_members_share_sound_never_was_in_it() {
        let factory = PeerConnectionFactory::default();
        let mut receiving = Receiving::new(
            &factory,
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
            Volumes::default(),
            None,
        )
        .expect("receiving");
        let gauge = receiving.gauge();
        let loudest = Arc::new(AtomicU32::new(FULL_SCALE / 2));
        receiving.meters.send_modify(|meters| {
            meters.insert(ADA.into(), loudest);
        });
        receiving
            .streams
            .insert((ADA.into(), TrackKind::ScreenAudio), Decoding { task: None, _heard: None, tap: None });
        assert_eq!(gauge.levels(), [(ADA.to_string(), 50)], "read through the gauge, while the stream is taken");
        receiving.forget(ADA, TrackKind::ScreenAudio);
        assert_eq!(gauge.levels(), [(ADA.to_string(), 0)], "the share's sound is not the microphone");
        receiving.forget(ADA, TrackKind::Microphone);
        assert_eq!(gauge.levels(), [], "a microphone forgotten is no level");
    }

    fn mine(user_id: &str) -> (String, TrackKind) {
        (user_id.to_string(), TrackKind::Microphone)
    }

    #[tokio::test]
    async fn a_stream_the_offer_refuses_is_left_out_and_the_next_one_is_still_taken() {
        let factory = PeerConnectionFactory::default();
        let mut receiving = Receiving::new(
            &factory,
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
            Volumes::default(),
            None,
        )
        .expect("receiving");
        let mut ssrc = 0;
        let mut offer = |cname: &str| {
            ssrc += 1;
            (opus_on(cname, ssrc), format!("{cname}:voice"))
        };

        let (rtp, stream) = offer("a b");
        assert!(matches!(receiving.offer(rtp, stream, mine("a b")).await, Err(VoiceRefused::Offer)));
        assert!(receiving.sections.is_empty(), "a refused stream is not kept");
        let (rtp, stream) = offer(ADA);
        receiving.offer(rtp, stream, mine(ADA)).await.expect("the engine takes a valid stream");
        let (rtp, stream) = offer("x;y");
        assert!(receiving.offer(rtp, stream, mine("x;y")).await.is_err());
        let (rtp, stream) = offer(BO);
        receiving.offer(rtp, stream, mine(BO)).await.expect("a later valid stream after a refused one");
        assert_eq!(receiving.sections.len(), 2);
    }

    #[tokio::test]
    async fn a_stream_taken_and_forgotten_again_and_again_keeps_the_offer_to_three_sections() {
        let factory = PeerConnectionFactory::default();
        let mut receiving = Receiving::new(
            &factory,
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
            Volumes::default(),
            None,
        )
        .expect("receiving");
        for round in 0..200 {
            receiving.offer(opus_on(ADA, 1_000 + round), format!("{ADA}:voice"), mine(ADA)).await.expect("taken");
            receiving.forget(ADA, TrackKind::Microphone);
            assert!(receiving.sections.len() <= 3, "round {round}: {} sections", receiving.sections.len());
        }
        assert_eq!(receiving.sections[0].held, Held::Resting, "the section that carries the bundle rests");
    }

    #[tokio::test]
    async fn a_closed_slot_is_taken_by_the_next_stream_of_its_kind_under_a_new_mid() {
        let factory = PeerConnectionFactory::default();
        let mut receiving = Receiving::new(
            &factory,
            &crate::voice::tests::offer(),
            crate::voice::tests::reached(),
            Volumes::default(),
            None,
        )
        .expect("receiving");
        for (user_id, ssrc) in [(ADA, 1), (BO, 2)] {
            receiving.offer(opus_on(user_id, ssrc), format!("{user_id}:voice"), mine(user_id)).await.expect("taken");
        }
        receiving.forget(BO, TrackKind::Microphone);
        receiving.offer(opus_on("cy", 3), "cy:voice".into(), mine("cy")).await.expect("taken");
        assert_eq!(receiving.sections.len(), 3, "bo's section was still open when cy's was offered");
        assert_eq!(receiving.sections[1].held, Held::Closed);

        receiving.offer(opus_on("di", 4), "di:voice".into(), mine("di")).await.expect("taken");
        let mids: Vec<&str> = receiving.sections.iter().map(|section| section.media.mid.as_str()).collect();
        assert_eq!(mids, ["0", "3", "2"], "di took bo's closed slot under a mid of its own");
        assert_eq!(receiving.sections[1].held, mine("di").into());
    }
}
