//! Members already in a call, for the measurements that put many of them in a room: each its own connection, engine and
//! media, sending a tone, a camera of one made-up picture, or both; and the encoder that makes the cameras' pictures.

use crate::common::registered_as;
use crate::screen_support::{self, layout, quadrants, settings};
use capture::Pixels;
use media::Session;
use proto::chatkord_v1::VoiceJoin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::watch;

/// The size and rate of the cameras a measurement sends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Camera {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) frames_per_second: u32,
}

/// A member already in the call: its connection, its media and what it sends, on an engine of its own as each person's
/// client is.
pub(crate) struct Member {
    _link: link::Link,
    _events: link::Events,
    pub(crate) id: String,
    _voice: webrtc::Voice,
    _hearing: webrtc::Hearing,
    _camera: Option<webrtc::SentVideo>,
    _speaking: Option<webrtc::Speaking>,
    _engine: webrtc::Engine,
}

/// Joins; with `camera`, a camera begins first, so the room lists it before the voice, and with `tone` a tone is spoken.
pub(crate) async fn joined(
    url: &str,
    name: &str,
    invite: &str,
    camera: Option<Camera>,
    tone: bool,
) -> (Member, Option<webrtc::Feeder>) {
    let (link, events, booth, id) = registered_as(url, name, invite).await;
    let Ok(state) = link.ask(VoiceJoin { channel_id: booth, ..Default::default() }).await else {
        panic!("not a voice state")
    };
    let engine = tokio::task::spawn_blocking(call::devices::engine).await.expect("an engine");
    let (voice, hearing) = webrtc::Voice::new(
        &engine,
        &state,
        webrtc::Located::of(&state, &webrtc::Resolved::new(), webrtc::Scope::Loopback).expect("addresses"),
    )
    .expect("both connections");
    let (sent, feeder) = match camera {
        Some(Camera { width, height, frames_per_second }) => {
            let (width, height) = (u32::try_from(width).unwrap(), u32::try_from(height).unwrap());
            let (sent, feeder) =
                voice.camera(&link, webrtc::Codec::H264, width, height).await.expect("the camera starts");
            let envelope = net::Envelope::for_camera(width, height, frames_per_second, None);
            sent.set_rates(envelope.min, envelope.start, envelope.max).expect("the camera states its rates");
            (Some(sent), Some(feeder))
        }
        None => (None, None),
    };
    let speaking = if tone { Some(voice.speak_tone(&link).await.expect("speaks")) } else { None };
    let member = Member {
        _link: link,
        _events: events,
        id: id.as_str().to_owned(),
        _voice: voice,
        _hearing: hearing,
        _camera: sent,
        _speaking: speaking,
        _engine: engine,
    };
    (member, feeder)
}

/// Raises its flag when dropped, so cameras fed on the blocking pool stop however the test ends: a blocking task the
/// runtime waits for would hold a failed test open.
pub(crate) struct Stops(pub(crate) std::sync::Arc<AtomicBool>);

impl Drop for Stops {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Encodes one painted picture at the camera's rate and pushes each unit to every feeder until `stop`, counting the
/// pictures made in `fed`.
pub(crate) fn send(
    encoder: &media::Encoder,
    canvas: &screen_support::Canvas,
    camera: Camera,
    feeders: &[webrtc::Feeder],
    stop: &AtomicBool,
    fed: &watch::Sender<usize>,
) {
    let Camera { width: w, height: h, frames_per_second } = camera;
    let (width, height) = (u32::try_from(w).unwrap(), u32::try_from(h).unwrap());
    let wanted = media::Settings { frames_per_second, ..settings(w, h) };
    let mut session = Session::open_screen(encoder, wanted, layout(w, h)).expect("the encoder opens");
    let mut surface = canvas.surface(&session, w, h).expect("a surface");
    surface.paint_rgb(&quadrants(w, h));
    let mut n = 0;
    while !stop.load(Ordering::Relaxed) {
        if feeders.iter().any(webrtc::Feeder::key_frame_wanted) {
            session.request_key_frame();
        }
        for unit in session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encodes") {
            for feeder in feeders {
                feeder.push(&unit.data, unit.key_frame, width, height, unit.captured);
            }
        }
        n += 1;
        fed.send_replace(n);
        crate::clock::frame_clock(Duration::from_secs(1) / frames_per_second);
    }
}
