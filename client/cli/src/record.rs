//! The screens shared in a voice channel from the terminal: watched in the window's own call, which says each second
//! how every screen arrives (`watch`), or kept in a file as the sender encoded it (`watch_stream`).

use crate::run::Terminal;
use crate::spool::Spool;
use call::room::{self, Report, Source};
use link::{Events, Link};
use proto::chatkord_v1::{TrackKind, VoiceState};
use screens::{RoomEvent, ShareCard, ShareState, Watched};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use terms::{Ended, Heard, LinkError, MemberId};
use tokio::sync::mpsc;
use webrtc::{Engine, KeyFrames, Voice};

/// How often a running watch says how it is going.
const STATUS_EVERY: Duration = Duration::from_secs(1);

/// AV1's temporal delimiter OBU type: what starts each temporal unit of a stream a player reads. RTP never carries
/// one, so the engine's frames come without it.
const TEMPORAL_DELIMITER: u8 = 2;

/// Writes one frame as a player reads it: H.264 as it came, an AV1 frame after a temporal delimiter it lacks.
fn write_frame(out: &mut impl Write, frame: &webrtc::Encoded<'_>) -> io::Result<()> {
    let delimited = frame.data.first().is_some_and(|header| header >> 3 & 0x0f == TEMPORAL_DELIMITER);
    if frame.codec == webrtc::Codec::Av1 && !delimited {
        // The OBU's header with its size field flagged, then its size: none.
        out.write_all(&[TEMPORAL_DELIMITER << 3 | 0b10, 0])?;
    }
    out.write_all(frame.data)
}

/// A stream written from its first key frame, so a player can start where the file does: an AV1 consumer is not held
/// until one, and frames before it reference pictures the file never had.
struct FromKeyFrame<W> {
    out: W,
    started: bool,
}

impl<W: Write> FromKeyFrame<W> {
    fn new(out: W) -> Self {
        Self { out, started: false }
    }

    fn write(&mut self, frame: &webrtc::Encoded<'_>) -> io::Result<()> {
        self.started |= frame.key_frame;
        if self.started { write_frame(&mut self.out, frame) } else { Ok(()) }
    }
}

/// What a channel event says about a screen.
enum Screen {
    Shared(String),
    Stopped(String),
}

/// The screen a channel event starts or stops, if it is about one.
fn screen_change(kind: Option<proto::chatkord_v1::event::Kind>) -> Option<Screen> {
    use proto::chatkord_v1::event::Kind;
    let screen = i32::from(TrackKind::ScreenVideo);
    match kind {
        Some(Kind::ProducerJoined(joined)) if joined.kind == screen => Some(Screen::Shared(joined.user_id)),
        Some(Kind::ProducerClosed(closed)) if closed.kind == screen => Some(Screen::Stopped(closed.user_id)),
        _ => None,
    }
}

/// Joins the channel and returns what the server says of it.
async fn join(link: &Link, channel_id: &terms::ChannelId) -> Result<proto::chatkord_v1::VoiceState, String> {
    call::room::join(link, channel_id).await.map_err(|e| crate::said(&e))
}

/// How a recording that joined the channel stopped early.
enum Stopped {
    /// Something here failed: the place is given back.
    Failed(String),
    /// The connection ended, and the place with it.
    LinkEnded(String),
}

impl From<String> for Stopped {
    fn from(sentence: String) -> Self {
        Self::Failed(sentence)
    }
}

/// Leaves the channel unless the connection ended the place already, and says how the recording ended.
async fn over(link: &Link, terminal: &mut dyn Terminal, recorded: Result<(), Stopped>) -> Result<(), String> {
    match recorded {
        Ok(()) => {
            call::room::leave(link).await.map_err(|why| crate::said(&why))?;
            terminal.say("Left the voice channel.");
            Ok(())
        }
        Err(Stopped::Failed(sentence)) => {
            // The failure is what the person needs to hear; a refused leave ends with the connection anyway.
            let _ = call::room::leave(link).await;
            Err(sentence)
        }
        Err(Stopped::LinkEnded(sentence)) => Err(sentence),
    }
}

/// The sentence for a file the spool could not write: it fell behind, or the disk refused.
fn unwritten(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::WouldBlock => "The recording fell behind the stream.".to_string(),
        _ => "The file could not be written.".to_string(),
    }
}

/// Makes the file at `path` off the async task and starts its writer.
async fn spool(path: String) -> Result<Spool, Stopped> {
    let made = tokio::task::spawn_blocking(move || std::fs::File::create(path)).await;
    match made {
        Ok(Ok(file)) => Ok(Spool::start(file)),
        _ => Err(Stopped::Failed("The file could not be made.".to_string())),
    }
}

/// Hands the spool's last bytes to its writer and waits until the file is written, off the async task.
async fn close(spool: Spool) -> Result<(), Stopped> {
    match tokio::task::spawn_blocking(move || spool.close()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(Stopped::Failed(unwritten(&error))),
        Err(_) => Err(Stopped::Failed("The file could not be written.".to_string())),
    }
}

/// What a watched screen's card says of its state, in a word.
fn state_word(state: ShareState) -> &'static str {
    match state {
        ShareState::Live => "live",
        ShareState::Reconnecting => "reconnecting",
        ShareState::Behind => "this computer is behind it",
        ShareState::Paused => "paused",
    }
}

/// The line a screen's card makes: whose, its size and rate, how it is coded and decoded, and what was lost. Pictures
/// replaced unpainted are left out: nothing paints the command line's slot, so every one would count.
fn card_line(card: &ShareCard, words: &words::Words) -> String {
    let (freezes, longest) = card.freezes;
    format!(
        "{}: {}, {}, decoded by {}, {} refused, {freezes} freezes (longest {} ms), {}.",
        card.who.as_str(),
        card.size_and_rate(words),
        card.codec,
        screens::encoder_row(&card.decoder, words),
        card.refused,
        longest.as_millis(),
        state_word(card.state),
    )
}

/// Joins the channel as the window's call does, taking every screen shared in it, and says each second how each
/// arrives, until told to stop. The place is given back by the call.
pub(crate) async fn watch(
    link: &Link,
    engine: &Engine,
    mut events: Events,
    channel_id: terms::ChannelId,
    me: Option<MemberId>,
    terminal: &mut dyn Terminal,
) -> Result<(), String> {
    let stop = terminal.stop();
    let words = words::Words::english();
    let leaving = room::Leaving::default();
    // Held to the end: a closed command channel is a leave, and a dropped sender of wants is a call that wants nothing.
    let (_commands, queued) = mpsc::channel(1);
    let (_wants, wanted) = tokio::sync::watch::channel(room::Wants::default());
    let report = |report: Report| match report {
        Report::Said(line) => terminal.say(&line),
        Report::Left(Ok(())) => terminal.say("Left the voice channel."),
        Report::Left(Err(said)) | Report::Notice(_, said) => terminal.say(&crate::english(&said)),
        Report::Room(event) => {
            if matches!(event, RoomEvent::Joined { .. }) {
                terminal.say("Watching for shared screens. Press Ctrl-C to stop.");
            }
        }
        Report::Watch(Watched::Card(card)) => terminal.say(&card_line(&card, &words)),
        Report::Watch(Watched::Failed { user_id, failed, .. }) => {
            terminal.say(&format!("{}: {}", user_id.as_str(), crate::failed(&failed)));
        }
        Report::Watch(Watched::Ended { user_id, .. }) => {
            terminal.say(&format!("{} stopped sharing.", user_id.as_str()))
        }
        Report::Watch(Watched::HandedOver { user_id, route, .. }) => {
            terminal.say(&crate::english(&route.watched_handed_over(user_id.as_str().to_string())));
        }
        Report::Watch(Watched::Paused { .. } | Watched::Sound { .. })
        | Report::Heard(_)
        | Report::Behind
        | Report::Answered(_)
        | Report::Connection(_)
        | Report::Sound(_)
        | Report::Share(_)
        | Report::Camera(_)
        | Report::OwnWatched { .. } => {}
    };
    let screens = room::Screens::new(|_, _| call::Slot::new());
    let join = room::Join { screens: Some(screens), ..crate::run::joining(channel_id, me, Source::Nothing) };
    let running = room::run(link, engine, &mut events, join, (wanted, queued), leaving.clone(), report);
    tokio::pin!(running);
    tokio::pin!(stop);
    tokio::select! {
        done = &mut running => return done.map_err(|stopped| crate::english(&stopped.sentence())),
        () = &mut stop => {}
    }
    leaving.leave();
    running.await.map_err(|stopped| crate::english(&stopped.sentence()))
}

fn link_ended(ended: Ended) -> Stopped {
    Stopped::LinkEnded(crate::said(&LinkError::Closed(ended)))
}

/// What `watch --stream` has seen of the encoded frames, counted by the engine's thread and read by the status line.
#[derive(Default)]
struct Streamed {
    frames: AtomicU64,
    keys: AtomicU64,
    bytes: AtomicU64,
    failed: Mutex<Option<String>>,
}

/// Records a shared screen as the sender encoded it, saying each second the frames, key frames and bytes that came
/// and the engine's decoder's report; it asks for a key frame once, so the line shows a request answered.
pub(crate) async fn watch_stream(
    link: &Link,
    engine: &Engine,
    events: Events,
    channel_id: terms::ChannelId,
    path: String,
    terminal: &mut dyn Terminal,
) -> Result<(), String> {
    let state = join(link, &channel_id).await?;
    let recorded = record_encoded(link, engine, events, &state, path, terminal).await;
    over(link, terminal, recorded).await
}

async fn record_encoded(
    link: &Link,
    engine: &Engine,
    mut events: Events,
    state: &VoiceState,
    path: String,
    terminal: &mut dyn Terminal,
) -> Result<(), Stopped> {
    let refused = |why| crate::failed(&::call::failure::voice(why));
    let located = webrtc::locate(state, link.peer()).await.map_err(refused)?;
    let (voice, mut hearing) = Voice::new(engine, state, located).map_err(refused)?;
    // Shared with the engine's thread that fills it, so the bytes it holds are written out once the watch ends.
    let file = Arc::new(Mutex::new(Some(FromKeyFrame::new(spool(path).await?))));
    let streamed = Arc::new(Streamed::default());
    let screen_kind = i32::from(TrackKind::ScreenVideo);
    let mut pending: Vec<String> =
        state.streams.iter().filter(|stream| stream.kind == screen_kind).map(|stream| stream.user_id.clone()).collect();
    let mut sink_file = Some(Arc::clone(&file));
    let mut watching: Option<(String, KeyFrames)> = None;
    let mut seconds = 0u32;
    terminal.say("Watching for a shared screen. Press Ctrl-C to stop.");
    let stop = terminal.stop();
    tokio::pin!(stop);
    let mut tick = tokio::time::interval(STATUS_EVERY);
    let mut last = (0u64, 0u64, 0u64);
    loop {
        while watching.is_none() && sink_file.is_some() {
            let Some(user_id) = pending.pop() else { break };
            let (counts, out) = (Arc::clone(&streamed), sink_file.take());
            let Some(out) = out else { break };
            // On an engine thread: the spool copies the bytes and returns.
            let sink = move |frame: webrtc::Encoded<'_>| {
                let wrote = match out.lock().as_deref_mut() {
                    Ok(Some(stream)) => stream.write(&frame),
                    _ => Err(io::ErrorKind::BrokenPipe.into()),
                };
                if let Err(error) = wrote
                    && let Ok(mut failed) = counts.failed.lock()
                {
                    failed.get_or_insert_with(|| unwritten(&error));
                }
                counts.frames.fetch_add(1, Ordering::Relaxed);
                counts.keys.fetch_add(u64::from(frame.key_frame), Ordering::Relaxed);
                counts.bytes.fetch_add(frame.data.len() as u64, Ordering::Relaxed);
            };
            match hearing.watch_encoded(link, &user_id, webrtc::Video::Screen, sink).await {
                Ok(key_frames) => {
                    terminal.say(&format!("Recording {user_id}'s screen as it was sent."));
                    watching = Some((user_id, key_frames));
                }
                Err(refused) => terminal.say(&crate::failed(&::call::failure::voice(refused))),
            }
        }
        tokio::select! {
            () = &mut stop => break,
            _ = tick.tick(), if watching.is_some() => {
                if let Some(failed) = streamed.failed.lock().ok().and_then(|failed| failed.clone()) {
                    return Err(Stopped::Failed(failed));
                }
                seconds += 1;
                let now = (streamed.frames.load(Ordering::Relaxed), streamed.keys.load(Ordering::Relaxed), streamed.bytes.load(Ordering::Relaxed));
                let engine = match &watching {
                    Some((user_id, _)) => voice.watch_stats_of(user_id, webrtc::Video::Screen).await.unwrap_or_default(),
                    None => webrtc::WatchStats::default(),
                };
                terminal.say(&format!(
                    "{} frames, {} key frames, {} KB this second; engine decoded {}, dropped {}, decoder {}",
                    now.0 - last.0, now.1 - last.1, (now.2 - last.2) / 1000, engine.frames_decoded, engine.frames_dropped, if engine.decoder.is_empty() { "not named" } else { &engine.decoder },
                ));
                last = now;
                if let Some((user_id, key_frames)) = watching.as_ref().filter(|_| seconds == 5) {
                    terminal.say(&format!("Asked {user_id}'s sender for a key frame: {}.", key_frames.ask()));
                }
            }
            event = events.next() => match event {
                Some(Heard::Event(event)) => match screen_change(event.kind) {
                    Some(Screen::Shared(user_id)) => pending.push(user_id),
                    Some(Screen::Stopped(user_id)) => {
                        pending.retain(|waiting| *waiting != user_id);
                        if hearing.forget(&user_id, TrackKind::ScreenVideo) {
                            terminal.say(&format!("{user_id} stopped sharing."));
                            break;
                        }
                    }
                    None => {}
                },
                // A tool that waits on one share reads its start and end, never the room whole.
                Some(Heard::Behind) => {}
                Some(Heard::Ended(ended)) => return Err(link_ended(ended)),
                None => return Err(link_ended(Ended::default())),
            },
        }
    }
    drop((voice, hearing));
    let kept = file.lock().ok().and_then(|mut held| held.take());
    match kept {
        Some(stream) => close(stream.out).await,
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Ask;
    use link::Trust;
    use terms::{Secret, ServerUrl};

    use proto::chatkord_v1::{ChannelsList, VoiceJoin, event};
    use std::future::Future;
    use std::pin::Pin;

    /// A terminal nobody types into.
    struct Nobody;

    impl Terminal for Nobody {
        fn secret(&mut self, _: Ask) -> Pin<Box<dyn Future<Output = Result<terms::Secret, String>> + Send>> {
            Box::pin(std::future::ready(Err("nobody is there".to_string())))
        }
        fn say(&mut self, _: &str) {}
        fn stop(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            Box::pin(std::future::pending())
        }
    }

    /// A terminal that keeps what it is told and stops when asked.
    struct Kept {
        lines: Arc<Mutex<Vec<String>>>,
        stop: Arc<tokio::sync::Notify>,
    }

    impl Terminal for Kept {
        fn secret(&mut self, _: Ask) -> Pin<Box<dyn Future<Output = Result<terms::Secret, String>> + Send>> {
            Box::pin(std::future::ready(Err("nobody is there".to_string())))
        }
        fn say(&mut self, line: &str) {
            if let Ok(mut lines) = self.lines.lock() {
                lines.push(line.to_string());
            }
        }
        fn stop(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            let stop = Arc::clone(&self.stop);
            Box::pin(async move { stop.notified().await })
        }
    }

    async fn registered(server: &testserver::Server, name: &str, invite: &str) -> (Link, Events, String) {
        let url = ServerUrl::parse(&server.url()).expect("a loopback address");
        let (link, events) = Link::connect(&url, &Trust::system(), "test", "test").await.expect("connected");
        let password = Secret::new("correct horse battery staple".into());
        link.register(name, None, &password, &Secret::new(invite.into())).await.expect("registered");
        let Ok(channels) = link.ask(ChannelsList {}).await else { panic!("no channels") };
        let booth = channels.channels.into_iter().find(|channel| channel.name == testserver::BOOTH).expect("the booth");
        (link, events, booth.channel_id)
    }

    fn written(codec: webrtc::Codec, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        write_frame(&mut out, &webrtc::Encoded { data, render_time_us: 0, key_frame: false, codec }).expect("written");
        out
    }

    #[test]
    fn a_stream_is_written_from_its_first_key_frame() {
        let frame =
            |data, key_frame| webrtc::Encoded { data, render_time_us: 0, key_frame, codec: webrtc::Codec::H264 };
        let mut stream = FromKeyFrame::new(Vec::new());
        for (data, key_frame) in [(&[1][..], false), (&[2], false), (&[3], true), (&[4], false), (&[5], true)] {
            stream.write(&frame(data, key_frame)).expect("written");
        }
        assert_eq!(stream.out, [3, 4, 5]);
    }

    #[test]
    fn an_av1_frame_is_written_after_one_temporal_delimiter_and_h264_as_it_came() {
        let frame = [0x32, 0x01, 0xaa];
        assert_eq!(written(webrtc::Codec::Av1, &frame), [0x12, 0x00, 0x32, 0x01, 0xaa]);
        assert_eq!(written(webrtc::Codec::Av1, &[0x12, 0x00, 0x32, 0x01, 0xaa]), [0x12, 0x00, 0x32, 0x01, 0xaa]);
        assert_eq!(written(webrtc::Codec::H264, &[0, 0, 0, 1, 0x65]), [0, 0, 0, 1, 0x65]);
    }

    mod server {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_stream_watch_that_cannot_make_its_file_gives_its_place_back_on_a_connection_that_stays() {
            let (server, invites) = testserver::Server::start("cli_record", 2).await;
            let (bo, mut bo_events, booth) = registered(&server, "bo", &invites[0]).await;
            bo.ask(VoiceJoin { channel_id: booth.clone(), ..Default::default() }).await.expect("bo joins");
            let (ada, ada_events, _) = registered(&server, "ada", &invites[1]).await;

            let nowhere = std::env::temp_dir().join("chatkord-no-such-directory").join("watch.h264");
            let watched = watch_stream(
                &ada,
                &::call::devices::engine(),
                ada_events,
                terms::ChannelId::parse(&booth).expect("a channel id"),
                nowhere.display().to_string(),
                &mut Nobody,
            )
            .await;
            assert_eq!(watched, Err("The file could not be made.".to_string()));

            // Ada's connection is still open: only a leave gives the place back.
            let left = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Some(Heard::Event(event)) = bo_events.next().await
                        && let Some(event::Kind::MemberLeft(_)) = event.kind
                    {
                        return;
                    }
                }
            })
            .await;
            assert!(left.is_ok(), "ada's place was given back");
            drop(ada);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_watch_joins_says_it_waits_for_a_screen_and_gives_its_place_back_when_stopped() {
            let (server, invites) = testserver::Server::start("cli_watch", 2).await;
            let (bo, mut bo_events, booth) = registered(&server, "bo", &invites[0]).await;
            bo.ask(VoiceJoin { channel_id: booth.clone(), ..Default::default() }).await.expect("bo joins");
            let (ada, ada_events, _) = registered(&server, "ada", &invites[1]).await;

            let lines = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(tokio::sync::Notify::new());
            let mut terminal = Kept { lines: Arc::clone(&lines), stop: Arc::clone(&stop) };
            let driving = async {
                // Ada is in the place once bo hears her join; a line of hers says what she waits for.
                let joined = tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        if let Some(Heard::Event(event)) = bo_events.next().await
                            && let Some(event::Kind::MemberJoined(_)) = event.kind
                        {
                            return;
                        }
                    }
                })
                .await;
                let said = tokio::time::timeout(Duration::from_secs(10), async {
                    while lines.lock().map_or(true, |lines| lines.is_empty()) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                })
                .await;
                stop.notify_one();
                let left = tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        if let Some(Heard::Event(event)) = bo_events.next().await
                            && let Some(event::Kind::MemberLeft(_)) = event.kind
                        {
                            return;
                        }
                    }
                })
                .await;
                (joined.is_ok(), said.is_ok(), left.is_ok())
            };
            let engine = ::call::devices::engine();
            let (watched, (joined, said, left)) = tokio::join!(
                watch(
                    &ada,
                    &engine,
                    ada_events,
                    terms::ChannelId::parse(&booth).expect("a channel id"),
                    None,
                    &mut terminal
                ),
                driving
            );
            assert_eq!(watched, Ok(()), "stopped by the person: not a failure");
            assert!(joined && said, "ada joined and said what she waits for");
            assert!(left, "and her place was given back when she was stopped");
            let lines = lines.lock().expect("a lock").clone();
            let waits = "Watching for shared screens. Press Ctrl-C to stop.";
            assert!(lines.iter().any(|line| line == waits), "a device with no sound says so first: {lines:?}");
            drop(ada);
        }
    }
}
