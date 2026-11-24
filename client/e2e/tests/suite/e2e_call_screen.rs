//! A screen shared and watched from inside the window's own call: one account shares a painted DMA-BUF, and a picture
//! of it reaches the slot each other call paints from without asking. Only the portal needs a person.

use crate::common;
use crate::screen_support;

// A screen painted from a VA-API device or a DMA-BUF, which only Linux has.

use call::room::{Command, Report, SoundAsked};
use call::screen::{Frames, Pick};
use capture::{CaptureError, Frame, Pixels};
use choices::{Ceiling, Codec, FrameRate, Resolution, ShareQuality};
use common::looks::Looks;
use common::running::{Running, start};
use common::{Server, registered_as};
use proto::video::Video;
use screen_support::{candidates, quadrants};
use screens::{RoomEvent, ShareState, Shared, Watched};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};
use terms::MemberId;
use tier::skipped;
use tokio::time::timeout;

/// How often a share's counts are read where it reports nothing.
const COUNTS_EVERY: Duration = Duration::from_millis(50);

/// A painted screen, as a compositor lends one: it sends pictures steadily, or one and then nothing, as a still screen
/// does.
struct Painted {
    surface: screen_support::Surface,
    still: bool,
    sent: AtomicBool,
    /// How long the screen takes between pictures while it moves.
    every: Duration,
}

// SAFETY: the surface's FFmpeg objects are used by one thread at a time: the one that encodes, after the share began.
unsafe impl Send for Painted {}

impl Frames for Painted {
    fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        if self.still && self.sent.swap(true, Ordering::Relaxed) {
            crate::clock::frame_clock(wait);
            return Ok(None);
        }
        if !self.still {
            crate::clock::frame_clock(self.every);
        }
        Ok(Some(self.surface.frame(Pixels::Bgrx)))
    }

    fn dropped(&self) -> u64 {
        0
    }
}

fn painted(width: usize, height: usize, still: bool) -> Option<Pick> {
    painted_every(width, height, still, Duration::from_millis(33))
}

fn painted_every(width: usize, height: usize, still: bool, every: Duration) -> Option<Pick> {
    let (_, node) = candidates().into_iter().next()?;
    let mut surface = screen_support::Surface::new(node.render_node(), width, height)?;
    surface.paint_rgb(&quadrants(width, height));
    Some(Pick::Frames(Box::new(Painted { surface, still, sent: AtomicBool::new(false), every })))
}

/// A shared window that, once `ending` is set, is closed (`resized` is `None`) or takes another size.
struct Ending {
    surface: screen_support::Surface,
    resized: Option<screen_support::Surface>,
    ending: Arc<AtomicBool>,
}

// SAFETY: the surfaces' FFmpeg objects are used by one thread at a time: the one that encodes, after the share began.
unsafe impl Send for Ending {}

impl Frames for Ending {
    fn next(&self, _wait: Duration) -> Result<Option<Frame>, CaptureError> {
        crate::clock::frame_clock(Duration::from_millis(33));
        match (self.ending.load(Ordering::Relaxed), &self.resized) {
            (false, _) => Ok(Some(self.surface.frame(Pixels::Bgrx))),
            (true, Some(resized)) => Ok(Some(resized.frame(Pixels::Bgrx))),
            (true, None) => Err(CaptureError::Ended),
        }
    }

    fn dropped(&self) -> u64 {
        0
    }
}

/// A painted window that, once the flag is set, is closed (`to` is `None`) or takes the size `to`.
fn ending(to: Option<(usize, usize)>) -> Option<(Pick, Arc<AtomicBool>)> {
    let (width, height) = (screen_support::WIDTH, screen_support::HEIGHT);
    let (_, node) = candidates().into_iter().next()?;
    let mut surface = screen_support::Surface::new(node.render_node(), width, height)?;
    surface.paint_rgb(&quadrants(width, height));
    let resized = match to {
        Some((width, height)) => {
            let mut resized = screen_support::Surface::new(node.render_node(), width, height)?;
            resized.paint_rgb(&quadrants(width, height));
            Some(resized)
        }
        None => None,
    };
    let ending = Arc::new(AtomicBool::new(false));
    Some((Pick::Frames(Box::new(Ending { surface, resized, ending: Arc::clone(&ending) })), ending))
}

/// A painted screen whose every picture counts its buffer out and home, as a compositor's pool would see them.
struct Counted {
    surface: screen_support::Surface,
    out: Arc<AtomicI64>,
    /// The most pictures ever out at once, seen as each new one is lent.
    most: Arc<AtomicI64>,
}

// SAFETY: the surface's FFmpeg objects are used by one thread at a time: the one that encodes, after the share began.
unsafe impl Send for Counted {}

/// One lent picture's buffer: home when it drops.
struct Lent(Arc<AtomicI64>);

impl Drop for Lent {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Frames for Counted {
    fn next(&self, _wait: Duration) -> Result<Option<Frame>, CaptureError> {
        crate::clock::frame_clock(Duration::from_millis(16));
        self.most.fetch_max(self.out.load(Ordering::SeqCst), Ordering::SeqCst);
        self.out.fetch_add(1, Ordering::SeqCst);
        Ok(Some(self.surface.frame_lent(Pixels::Bgrx, Lent(Arc::clone(&self.out)))))
    }

    fn dropped(&self) -> u64 {
        0
    }
}

/// Two accounts in one call: the first shares `pick` and the second is told when it is live.
async fn two_in_a_call(pick: Pick) -> (Server, Running, Running, MemberId) {
    let (server, ada, bea, ada_id, _) = in_a_call(pick, 0).await;
    (server, ada, bea, ada_id)
}

/// As [`two_in_a_call`], with `more` invites left over for others.
async fn in_a_call(pick: Pick, more: usize) -> (Server, Running, Running, MemberId, Vec<String>) {
    in_a_call_as(pick, more, ShareQuality::default()).await
}

/// As [`in_a_call`], the share sent as `quality` says.
async fn in_a_call_as(
    pick: Pick,
    more: usize,
    quality: ShareQuality,
) -> (Server, Running, Running, MemberId, Vec<String>) {
    let (server, mut invites) = Server::start("e2e_call_screen", 2 + more).await;
    let (ada_link, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
    let (bea_link, bea_events, _, bea_id) = registered_as(&server.url(), "bea", &invites[1]).await;
    let ada = start(ada_link, ada_events, booth.clone(), ada_id.clone());
    let bea = start(bea_link, bea_events, booth, bea_id);
    ada.until("Ada is in the room", Duration::from_secs(30), |report| {
        matches!(report, Report::Room(RoomEvent::Joined { .. }))
    })
    .await;
    ada.ask(Command::Share(pick, quality, SoundAsked::Apps(Vec::new()))).await;
    ada.until("the share is live", Duration::from_secs(30), |report| {
        matches!(report, Report::Share(Shared::Started { .. }))
    })
    .await;
    let others = invites.split_off(2);
    (server, ada, bea, ada_id, others)
}

/// Bea sees `user_id` sharing, and pictures of `what` reach her slot for it.
async fn watched(bea: &Running, user_id: &MemberId, what: &str) {
    bea.until("Bea sees the share", Duration::from_secs(30), |report| {
        matches!(report, Report::Room(RoomEvent::Started { track: screens::Track::ScreenVideo, .. }))
    })
    .await;
    bea.until(&format!("a picture of {what}"), Duration::from_secs(20), |report| {
        matches!(report, Report::Watch(Watched::Card(_)))
    })
    .await;
    assert!(bea.screen(user_id).stats().received > 0, "pictures of {what} reached Bea");
}

/// A share whose window takes the size `to` goes on, sent at `sent` as `quality` asks (`scaled_from` the window's size
/// where it is scaled), and the viewer follows it with no end in between.
async fn a_share_that_changes_size(
    quality: ShareQuality,
    to: (usize, usize),
    sent: (u32, u32),
    scaled_from: Option<(u32, u32)>,
) {
    let Some((pick, resizing)) = ending(Some(to)) else {
        return skipped("no VA-API device to paint a screen on");
    };
    let (_server, ada, bea, ada_id, _) = in_a_call_as(pick, 0, quality).await;
    bea.until("a picture of the card", Duration::from_secs(30), |report| {
        matches!(report, Report::Watch(Watched::Card(_)))
    })
    .await;

    resizing.store(true, Ordering::Relaxed);
    ada.until("the share goes out at its new size", Duration::from_secs(15), |report| {
        assert!(
            !matches!(report, Report::Share(Shared::Failed(_) | Shared::Stopped)),
            "a new size ended the share: {report:?}"
        );
        matches!(report, Report::Share(Shared::Looked(outgoing))
            if (outgoing.width, outgoing.height) == sent && outgoing.scaled_from == scaled_from)
    })
    .await;
    bea.until("the viewer follows the new size", Duration::from_secs(15), |report| {
        assert!(!matches!(report, Report::Watch(Watched::Ended { .. })), "the stream ended: {report:?}");
        matches!(report, Report::Watch(Watched::Card(card))
            if card.who == ada_id && (card.width, card.height) == sent)
    })
    .await;
    bea.leave().await;
    ada.leave().await;
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_screen_shared_in_one_call_plays_in_another_and_ends_with_the_share() {
        let Some(pick) = painted(screen_support::WIDTH, screen_support::HEIGHT, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, ada_id) = two_in_a_call(pick).await;
        bea.until("Bea sees Ada sharing", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Started { track: screens::Track::ScreenVideo, .. }))
        })
        .await;
        bea.until(
            "a picture of the card",
            Duration::from_secs(20),
            |report| matches!(report, Report::Watch(Watched::Card(card)) if (card.width, card.height) == (1280, 720)),
        )
        .await;
        assert!(bea.screen(&ada_id).stats().received > 0, "pictures reached the slot the window paints");
        assert_eq!(bea.screen(&ada_id).size(), Some((1280, 720)));
        assert!(!ada.task.is_finished(), "sharing did not end Ada's call: it is one seat");

        ada.ask(Command::StopShare).await;
        ada.until("the share is told stopped", Duration::from_secs(10), |report| {
            matches!(report, Report::Share(Shared::Stopped))
        })
        .await;
        bea.until(
            "the watch ends with the share",
            Duration::from_secs(10),
            |report| matches!(report, Report::Watch(Watched::Ended { user_id, video: Video::Screen }) if *user_id == ada_id),
        )
        .await;
        // Or the next share would paint this one's last picture.
        assert_eq!(bea.screen(&ada_id).size(), None, "the share's pictures are gone with it");
        assert!(bea.screen(&ada_id).take().is_none(), "nothing of it waits to be painted");
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_share_nobody_watches_encodes_nothing_and_encodes_again_once_watched() {
        let Some(Pick::Frames(frames)) = painted(640, 360, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let quality = ShareQuality::default();
        let started = tokio::task::spawn_blocking(move || {
            call::screen::begin_with(frames, &quality, &Arc::new(media::Survey::system())).ok()
        })
        .await
        .unwrap();
        let started = started.expect("the share begins");
        let (watched, stopped) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)));
        let progress = Arc::new(call::screen::Progress::default());
        let run = {
            let (watched, stopped, progress) = (Arc::clone(&watched), Arc::clone(&stopped), Arc::clone(&progress));
            tokio::task::spawn_blocking(move || {
                let (stop, watching) = (|| stopped.load(Ordering::Relaxed), || watched.load(Ordering::Relaxed));
                started.run(None, None, &progress, stop, watching)
            })
        };
        // A failed assertion still ends the run, or the runtime would wait for its thread for ever.
        struct Stops(Arc<AtomicBool>);
        impl Drop for Stops {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let stops = Stops(Arc::clone(&stopped));
        let counts = || (progress.frames.load(Ordering::Relaxed), progress.key_frames.load(Ordering::Relaxed));
        let mut looks = Looks::every(COUNTS_EVERY);
        let (frames, keys) = looks
            .until(Duration::from_secs(10), || Some(counts()).filter(|(frames, keys)| *frames > 0 && *keys >= 1))
            .await
            .expect("a watched share encodes, from a key frame");
        eprintln!("watched, the share encoded {frames} pictures from {keys} key frames");

        watched.store(false, Ordering::Relaxed);
        // The picture the encoder holds comes out first, then nothing does.
        looks.over(Duration::from_millis(200)).await;
        let resting = counts();
        looks.over(Duration::from_millis(600)).await;
        assert_eq!(counts(), resting, "nobody watches: the encoder rests");

        watched.store(true, Ordering::Relaxed);
        // Its key frame is the server's to ask, as `a_held_watch_taken_again_costs_its_sharer_one_key_frame_and_no_reopen`
        // shows: nothing here asks.
        looks
            .until(Duration::from_secs(10), || (counts().0 > resting.0).then_some(()))
            .await
            .expect("watched again, it encodes again");
        looks.over(Duration::from_millis(600)).await;
        let (_, keys) = counts();
        assert_eq!(keys, resting.1, "and makes no key frame nobody asked for: {keys} key frames");
        drop(stops);
        assert!(run.await.unwrap().is_ok(), "the run ends as asked");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_ceiling_chosen_caps_the_rates_a_share_states() {
        let Some(pick) = painted(1920, 1080, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let Pick::Frames(frames) = pick else { panic!("painted") };
        // At 60 a second this screen alone would be given a little over 5 Mbit/s.
        let quality = ShareQuality { ceiling: Ceiling::Mbit4, ..ShareQuality::default() };
        let started = tokio::task::spawn_blocking(move || {
            call::screen::begin_with(frames, &quality, &Arc::new(media::Survey::system())).ok()
        })
        .await
        .unwrap();
        let started = started.expect("the share begins");
        let open = net::Envelope::for_screen(started.layout.width, started.layout.height, 60, None);
        assert!(open.max > 4_000_000, "the screen alone would be given more: {open:?}");
        assert_eq!(
            started.envelope,
            net::Envelope::for_screen(started.layout.width, started.layout.height, 60, Some(4_000_000))
        );
        assert_eq!(started.envelope.max, 4_000_000);
    }

    /// The server watches what a member sends against the most a share is sent at: an honest 4K share, at its own
    /// size and with no ceiling, stays under it and keeps its call.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_honest_4k_share_stays_under_the_most_a_share_is_sent_at() {
        let Some(pick) = painted(3840, 2160, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, ada_id) = two_in_a_call(pick).await;
        watched(&bea, &ada_id, "Ada's screen").await;
        ada.looks(10, Duration::from_secs(60)).await;
        let sent: Vec<u64> = ada
            .reports
            .lock()
            .unwrap()
            .iter()
            .filter_map(|report| match report {
                Report::Share(Shared::Looked(outgoing)) => Some(outgoing.bits_per_second),
                _ => None,
            })
            .collect();
        let most = sent.iter().copied().max().unwrap_or(0);
        println!("the share's highest second: {most} bits a second, over {} looks", sent.len());
        assert!(sent.len() >= 5, "the share was looked at each second: {sent:?}");
        assert!(most <= proto::rules::SHARE_MAX_BITS_PER_SECOND, "sent past the most: {sent:?}");
        assert!(!ada.task.is_finished(), "the server kept Ada's place");
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_share_goes_out_at_the_resolution_chosen_and_plays_at_it() {
        let Some(pick) = painted(1920, 1080, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let quality = ShareQuality { resolution: Resolution::P720, ..ShareQuality::default() };
        let (_server, ada, bea, _, _) = in_a_call_as(pick, 0, quality).await;
        ada.until("the share is told scaled from the screen", Duration::from_secs(10), |report| {
            matches!(report, Report::Share(Shared::Looked(outgoing))
            if (outgoing.width, outgoing.height, outgoing.scaled_from) == (1280, 720, Some((1920, 1080))))
        })
        .await;
        bea.until(
            "a picture at the size chosen",
            Duration::from_secs(20),
            |report| matches!(report, Report::Watch(Watched::Card(card)) if (card.width, card.height) == (1280, 720)),
        )
        .await;
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_resolution_chosen_sizes_the_stream_and_its_rates() {
        let Some(Pick::Frames(frames)) = painted(1920, 1080, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let quality = ShareQuality { resolution: Resolution::P720, ..ShareQuality::default() };
        let started = tokio::task::spawn_blocking(move || {
            call::screen::begin_with(frames, &quality, &Arc::new(media::Survey::system())).ok()
        })
        .await
        .unwrap();
        let started = started.expect("the share begins");
        assert_eq!((started.layout.width, started.layout.height, started.scaled_from), (1280, 720, Some((1920, 1080))));
        assert_eq!(
            started.envelope,
            net::Envelope::for_screen(1280, 720, 60, None),
            "the rates follow the stream's size"
        );
    }

    /// A sharer whose roles lose `SHARE_SCREEN` is told, and only the share ends: the call, and its seat, go on.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_share_denied_stops_only_the_share() {
        let Some(pick) = painted(screen_support::WIDTH, screen_support::HEIGHT, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (server, ada, bea, ada_id) = two_in_a_call(pick).await;
        watched(&bea, &ada_id, "Ada's screen").await;
        server.give_only("SHARE_SCREEN", "sharers", "ada");
        let (cy, _cy_events, _, _) = registered_as(&server.url(), "cy", &server.owner_code()).await;
        let roles = cy.ask(proto::chatkord_v1::ListRoles {}).await.expect("the roles").entries;
        let sharers = roles.into_iter().find(|role| role.name == "sharers").expect("listed").role_id;
        let take = proto::chatkord_v1::SetRole { user_id: ada_id.as_str().to_owned(), role_id: sharers, held: false };
        cy.ask(take).await.expect("Cy, who owns the server, took the role");
        ada.until("Ada is told her share was denied", Duration::from_secs(10), |report| {
            matches!(report, Report::Share(Shared::Denied))
        })
        .await;
        bea.until(
            "the watch ends with the share",
            Duration::from_secs(10),
            |report| matches!(report, Report::Watch(Watched::Ended { user_id, video: Video::Screen }) if *user_id == ada_id),
        )
        .await;
        assert!(!ada.task.is_finished(), "the call goes on");
        assert!(
            !bea.reports
                .lock()
                .unwrap()
                .iter()
                .any(|report| matches!(report, Report::Room(RoomEvent::MemberLeft { .. }))),
            "Ada keeps her seat"
        );
        bea.leave().await;
        ada.leave().await;
    }

    /// A watched screen whose media stops says it is reconnecting, and is live again when the media comes back. The
    /// server is paused for less than the link's silence allowance, so the call itself goes on.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_watched_screen_whose_server_pauses_says_it_is_reconnecting_and_then_live() {
        let Some(pick) = painted(screen_support::WIDTH, screen_support::HEIGHT, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (server, ada, bea, ada_id) = two_in_a_call(pick).await;
        watched(&bea, &ada_id, "Ada's screen").await;
        server.pause();
        let reconnecting = |report: &Report| matches!(report, Report::Watch(Watched::Card(card)) if card.state == ShareState::Reconnecting);
        let took = bea.until("the watch says it is reconnecting", Duration::from_secs(12), reconnecting).await;
        eprintln!("reconnecting told {took:?} after the pause");
        bea.reports.lock().unwrap().clear();
        server.resume();
        let back = bea
        .until("the watch is live again", Duration::from_secs(20), |report| {
            matches!(report, Report::Watch(Watched::Card(card)) if card.state == ShareState::Live && card.frame_rate.is_some())
        })
        .await;
        eprintln!("live again {back:?} after the resume");
        assert!(!bea.task.is_finished() && !ada.task.is_finished(), "both calls went on");
        bea.leave().await;
        ada.leave().await;
    }

    /// Two screens shared at once both play in a third call, each in its own slot, and one ending leaves the other
    /// playing.
    #[tokio::test(flavor = "multi_thread")]
    async fn two_screens_shared_in_one_call_both_arrive_in_a_third() {
        let (Some(ada_pick), Some(cy_pick)) = (
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
        ) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (server, ada, bea, ada_id, invites) = in_a_call(ada_pick, 1).await;
        let (cy_link, cy_events, booth, cy_id) = registered_as(&server.url(), "cy", &invites[0]).await;
        let cy = start(cy_link, cy_events, booth, cy_id.clone());
        cy.ask(Command::Share(cy_pick, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        cy.until("Cy's share is live", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Started { .. }))
        })
        .await;
        for (who, whose) in [(&ada_id, "Ada's"), (&cy_id, "Cy's")] {
            bea.until(
                &format!("a picture of {whose} screen"),
                Duration::from_secs(20),
                |report| matches!(report, Report::Watch(Watched::Card(card)) if card.who == *who),
            )
            .await;
            assert!(bea.screen(who).stats().received > 0, "{whose} pictures reached their own slot");
        }
        cy.until(
            "a sharer sees the other's screen too",
            Duration::from_secs(20),
            |report| matches!(report, Report::Watch(Watched::Card(card)) if card.who == ada_id),
        )
        .await;
        assert!(
            !cy.reports
                .lock()
                .unwrap()
                .iter()
                .any(|report| matches!(report, Report::Watch(Watched::Card(card)) if card.who == cy_id)),
            "and never decodes its own"
        );

        ada.ask(Command::StopShare).await;
        bea.until(
            "Ada's screen ends with her share",
            Duration::from_secs(10),
            |report| matches!(report, Report::Watch(Watched::Ended { user_id, video: Video::Screen }) if *user_id == ada_id),
        )
        .await;
        bea.reports.lock().unwrap().clear();
        bea.until("Cy's screen plays on", Duration::from_secs(5), |report| {
        matches!(report, Report::Watch(Watched::Card(card)) if card.who == cy_id && card.state == ShareState::Live)
    })
    .await;
        cy.leave().await;
        bea.leave().await;
        ada.leave().await;
    }

    /// With no microphone the screen is the send connection's first section, the one its bundle hangs on: stopping it
    /// must not close the connection, or a second share sends nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_screen_shared_again_with_no_microphone_reaches_the_watcher() {
        let (Some(first), Some(second)) = (
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
        ) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, ada_id) = two_in_a_call(first).await;
        watched(&bea, &ada_id, "the first share").await;
        ada.ask(Command::StopShare).await;
        bea.until(
            "the watch ends with the share",
            Duration::from_secs(10),
            |report| matches!(report, Report::Watch(Watched::Ended { user_id, video: Video::Screen }) if *user_id == ada_id),
        )
        .await;
        ada.reports.lock().unwrap().clear();
        bea.reports.lock().unwrap().clear();
        ada.ask(Command::Share(second, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.until("the second share is live", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Started { .. }))
        })
        .await;
        watched(&bea, &ada_id, "the second share").await;
        // The sender's own numbers are the new section's, not the resting first one's.
        ada.until(
            "the second share is counted going out",
            Duration::from_secs(10),
            |report| matches!(report, Report::Share(Shared::Looked(outgoing)) if outgoing.bits_per_second > 0),
        )
        .await;
        assert!(!ada.task.is_finished(), "Ada is still in the call");
        bea.leave().await;
        ada.leave().await;
    }

    /// A screen picked while the one before it is still being stopped begins once that stop ends, never dropped unsaid.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_screen_picked_while_the_last_one_stops_is_shared_after_it() {
        let (Some(first), Some(second)) = (
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
        ) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, ada_id) = two_in_a_call(first).await;
        watched(&bea, &ada_id, "the first share").await;
        ada.reports.lock().unwrap().clear();
        // Sent together, both reach the call before its stop has gone to the server.
        ada.ask(Command::StopShare).await;
        ada.ask(Command::Share(second, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.until("the first share is over", Duration::from_secs(10), |report| {
            matches!(report, Report::Share(Shared::Stopped))
        })
        .await;
        ada.until("the second share is live", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Started { .. }))
        })
        .await;
        bea.leave().await;
        ada.leave().await;
    }

    /// A screen picked while another is live replaces it: the first is let go unsaid, the room hears it end, and the
    /// second is sent after it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_screen_picked_while_another_is_live_replaces_it_unsaid() {
        let (Some(first), Some(second)) = (
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
        ) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, ada_id) = two_in_a_call(first).await;
        watched(&bea, &ada_id, "the first share").await;
        ada.reports.lock().unwrap().clear();
        ada.ask(Command::Share(second, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.until("the room hears the first share end", Duration::from_secs(10), |report| {
            matches!(report, Report::Room(RoomEvent::Stopped { track: screens::Track::ScreenVideo, .. }))
        })
        .await;
        ada.until("the second share is live", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Started { .. }))
        })
        .await;
        let said = ada.reports.lock().unwrap().iter().any(|report| matches!(report, Report::Share(Shared::Stopped)));
        assert!(!said, "the share replaced is never said stopped");
        bea.leave().await;
        ada.leave().await;
    }

    /// A share that ends by itself is told as a failure, and the call it was in goes on.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_shared_window_that_closes_ends_the_share_with_a_sentence_and_not_the_call() {
        let Some((pick, ending)) = ending(None) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, ada_id) = two_in_a_call(pick).await;
        bea.until("Bea sees Ada sharing", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Started { track: screens::Track::ScreenVideo, .. }))
        })
        .await;
        bea.until("a picture of the card", Duration::from_secs(20), |report| {
            matches!(report, Report::Watch(Watched::Card(_)))
        })
        .await;

        ending.store(true, Ordering::Relaxed);
        ada.until("the share is told failed", Duration::from_secs(10), |report| {
            matches!(report, Report::Share(Shared::Failed(_)))
        })
        .await;
        ada.until("Ada's own tile stops sharing", Duration::from_secs(5), |report| {
            matches!(report, Report::Room(RoomEvent::Stopped { track: screens::Track::ScreenVideo, .. }))
        })
        .await;
        bea.until(
            "the watch ends with the share",
            Duration::from_secs(10),
            |report| matches!(report, Report::Watch(Watched::Ended { user_id, video: Video::Screen }) if *user_id == ada_id),
        )
        .await;
        assert!(!ada.task.is_finished(), "the share's end did not end Ada's call");
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_shared_window_that_shrinks_goes_on_at_its_new_size() {
        a_share_that_changes_size(ShareQuality::default(), (960, 540), (960, 540), None).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_shared_window_that_grows_goes_on_at_the_resolution_chosen() {
        let quality = ShareQuality { resolution: Resolution::P720, ..ShareQuality::default() };
        a_share_that_changes_size(quality, (1920, 1080), (1280, 720), Some((1920, 1080))).await;
    }

    /// The sharer's newest look: its key frames and reopens so far.
    fn sent_so_far(ada: &Running) -> Option<(u64, u64, u64)> {
        ada.reports.lock().unwrap().iter().rev().find_map(|report| match report {
            Report::Share(Shared::Looked(outgoing)) => {
                Some((outgoing.key_frames_made, outgoing.restarts, outgoing.bits_per_second))
            }
            _ => None,
        })
    }

    /// A watched screen held and taken again costs its sharer one key frame and no reopen: the server's resume, made
    /// once the viewer's decoder opened again, asks for the key frame, and the rate the link carried before the hold
    /// is the rate it carries after, the engine's estimate made over the silence not taken.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_held_watch_taken_again_costs_its_sharer_one_key_frame_and_no_reopen() {
        held_and_taken_again(Codec::H264).await;
    }

    /// The same in AV1, whose key frame the server tells only by the dependency descriptor: without it, it asks again a
    /// second later and the sharer makes two.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_av1_watch_held_and_taken_again_costs_its_sharer_one_key_frame() {
        held_and_taken_again(Codec::Av1).await;
    }

    #[expect(clippy::print_stderr, reason = "the key frames counted and the encoder are read in the test's output")]
    async fn held_and_taken_again(codec: Codec) {
        let Some(pick) = painted(3840, 2160, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let quality = ShareQuality { codec: Some(codec), ..ShareQuality::default() };
        let (_server, ada, bea, ada_id, _) = in_a_call_as(pick, 0, quality).await;
        watched(&bea, &ada_id, "Ada's screen").await;
        // The share's start climbs from its start rate: settled before the hold.
        bea.looks(6, Duration::from_secs(60)).await;
        bea.wants.send_modify(|wants| wants.show_none_of(vec![(ada_id.clone(), Video::Screen)]));
        bea.looks(
            usize::try_from((call::UNSEEN + Duration::from_secs(4)).as_secs()).unwrap_or(10),
            Duration::from_secs(90),
        )
        .await;
        let held = sent_so_far(&ada).expect("looked at while held");
        bea.wants.send_modify(|wants| wants.show_none_of(Vec::new()));
        ada.when("the sharer made the key frame", Duration::from_secs(30), || {
            sent_so_far(&ada).filter(|now| now.0 > held.0)
        })
        .await;
        // Long enough for a second ask to be sent on a second later, inside the gate's wait for a new measurement,
        // which a real climb of the link may follow.
        bea.looks(2, Duration::from_secs(30)).await;
        let after = sent_so_far(&ada).expect("looked at after");
        let route = ada.reports.lock().unwrap().iter().rev().find_map(|report| match report {
            Report::Share(Shared::Looked(outgoing)) => Some(outgoing.encoder.clone()),
            _ => None,
        });
        eprintln!("key frames, reopens, bits a second: held {held:?}, after {after:?}, encoded by {route:?}");
        assert_eq!(held.2, 0, "nobody watched: nothing was sent");
        assert_eq!(after.0 - held.0, 1, "taken again on one key frame: {held:?} {after:?}");
        assert_eq!(after.1, held.1, "and no reopen: {held:?} {after:?}");
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_watched_screens_card_plays_at_its_first_picture_not_at_the_next_look() {
        let Some(pick) = painted(screen_support::WIDTH, screen_support::HEIGHT, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (server, invites) = Server::start("e2e_call_screen", 2).await;
        let (ada_link, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea_link, bea_events, _, bea_id) = registered_as(&server.url(), "bea", &invites[1]).await;
        let ada = start(ada_link, ada_events, booth.clone(), ada_id.clone());
        ada.until("Ada is in the room", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Joined { .. }))
        })
        .await;
        ada.ask(Command::Share(pick, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.until("the share is live", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Started { .. }))
        })
        .await;
        let bea = start(bea_link, bea_events, booth, bea_id);
        let pictured = bea.next_picture(&ada_id, Video::Screen);
        bea.until(
            "the screen's card plays",
            Duration::from_secs(30),
            |report| matches!(report, Report::Watch(Watched::Card(card)) if card.who == ada_id),
        )
        .await;
        let carded = Instant::now();
        let pictured = timeout(Duration::from_secs(30), pictured).await.expect("a picture came").expect("told");
        let after = carded.saturating_duration_since(pictured);
        eprintln!("the card played {after:?} after the first picture");
        // The look that reports every card comes once a second: a card left to it waits up to that long.
        assert!(after < Duration::from_millis(300), "the card waited {after:?} for the next look");
        bea.leave().await;
        ada.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_still_screen_gives_a_late_joiner_a_picture_within_two_seconds() {
        let Some(pick) = painted(screen_support::WIDTH, screen_support::HEIGHT, true) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (server, invites) = Server::start("e2e_call_screen", 2).await;
        let (ada_link, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea_link, bea_events, _, bea_id) = registered_as(&server.url(), "bea", &invites[1]).await;
        let ada = start(ada_link, ada_events, booth.clone(), ada_id.clone());
        ada.until("Ada is in the room", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Joined { .. }))
        })
        .await;
        ada.ask(Command::Share(pick, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.until("the share is live", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Started { .. }))
        })
        .await;
        // The share's first key frame is long gone, and nothing is sent while the screen does not change.
        ada.looks(4, Duration::from_secs(60)).await;
        let bea = start(bea_link, bea_events, booth, bea_id);
        let pictured = bea.next_picture(&ada_id, Video::Screen);
        bea.until("Bea is in the room", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Joined { .. }))
        })
        .await;
        let joined = Instant::now();
        let pictured = timeout(Duration::from_secs(10), pictured).await.expect("a picture came").expect("told");
        let took = pictured.saturating_duration_since(joined);
        eprintln!("a late joiner to a still screen had its picture {took:?} after joining");
        assert!(took < Duration::from_secs(2), "the picture took {took:?}");
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_window_of_odd_size_is_shared_without_its_last_column_and_row() {
        let Some(pick) = painted(1281, 721, false) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (_server, ada, bea, _) = two_in_a_call(pick).await;
        assert!(
            ada.reports
                .lock()
                .unwrap()
                .iter()
                .any(|report| matches!(report, Report::Share(Shared::Started { width: 1280, height: 720, .. }))),
            "the share says the even size it sends"
        );
        bea.until(
            "an even picture",
            Duration::from_secs(20),
            |report| matches!(report, Report::Watch(Watched::Card(card)) if (card.width, card.height) == (1280, 720)),
        )
        .await;
        bea.leave().await;
        ada.leave().await;
    }

    /// This account's own card is lent what is sent beside the encoder and keeps no buffer: one is lent only when the
    /// last was taken, so at most the one waiting and the one painted are out, and all are home once the share stops.
    #[tokio::test(flavor = "multi_thread")]
    async fn one_s_own_screen_is_lent_to_its_card_and_every_buffer_comes_home() {
        let (width, height) = (screen_support::WIDTH, screen_support::HEIGHT);
        let Some((_, node)) = candidates().into_iter().next() else {
            return skipped("no VA-API device to paint a screen on");
        };
        let Some(mut surface) = screen_support::Surface::new(node.render_node(), width, height) else {
            return skipped("no VA-API device to paint a screen on");
        };
        surface.paint_rgb(&quadrants(width, height));
        let (out, most) = (Arc::new(AtomicI64::new(0)), Arc::new(AtomicI64::new(0)));
        let counted = Counted { surface, out: Arc::clone(&out), most: Arc::clone(&most) };
        let (_server, ada, bea, _) = two_in_a_call(Pick::Frames(Box::new(counted))).await;
        // The window: it takes a picture, paints it for a frame, and lets the one before go.
        let (painting, own) = (Arc::new(AtomicBool::new(true)), ada.own.clone());
        let window = {
            let painting = Arc::clone(&painting);
            std::thread::spawn(move || {
                let mut shown = None;
                while painting.load(Ordering::Relaxed) {
                    if let Some(picture) = own.take() {
                        shown = Some(picture);
                    }
                    crate::clock::frame_clock(Duration::from_millis(16));
                }
                drop(shown);
            })
        };
        ada.looks(2, Duration::from_secs(30)).await;
        assert!(ada.own.stats().received > 30, "the card was lent pictures: {:?}", ada.own.stats());
        // The window stops painting first (the card went), so a picture waits in the slot when the share stops.
        painting.store(false, Ordering::Relaxed);
        window.join().unwrap();
        // Under a loaded suite a picture in flight comes home late, and the next reaches the slot late: both are
        // waited for, not slept on.
        ada.when("only the picture waiting for a window is held", Duration::from_secs(2), || {
            (out.load(Ordering::SeqCst) == 1).then_some(())
        })
        .await;
        assert_eq!(out.load(Ordering::SeqCst), 1, "only the picture waiting for a window that went is held");
        ada.ask(Command::StopShare).await;
        ada.until("the share is told stopped", Duration::from_secs(10), |report| {
            matches!(report, Report::Share(Shared::Stopped))
        })
        .await;
        assert!(ada.own.take().is_none(), "the card holds nothing once the share stopped");
        assert_eq!(out.load(Ordering::SeqCst), 0, "every buffer came home");
        let most = most.load(Ordering::SeqCst);
        assert!(most <= 2, "at most the one waiting and the one painted were out beside a new one, not {most}");
        bea.leave().await;
        ada.leave().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_share_stopped_while_it_begins_is_never_sent_and_the_next_one_is() {
        let (Some(first), Some(second)) = (
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
            painted(screen_support::WIDTH, screen_support::HEIGHT, false),
        ) else {
            return skipped("no VA-API device to paint a screen on");
        };
        let (server, invites) = Server::start("e2e_call_screen_let_go", 2).await;
        let (ada_link, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea_link, bea_events, _, bea_id) = registered_as(&server.url(), "bea", &invites[1]).await;
        let ada = start(ada_link, ada_events, booth.clone(), ada_id.clone());
        let bea = start(bea_link, bea_events, booth, bea_id);
        ada.until("Ada is in the room", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Joined { .. }))
        })
        .await;

        // Stopped before its first picture: it is let go when that comes, and nobody is told it started.
        ada.ask(Command::Share(first, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.ask(Command::StopShare).await;
        ada.until("the share is told stopped", Duration::from_secs(30), |report| {
            matches!(report, Report::Share(Shared::Stopped))
        })
        .await;
        let started = |report: &Report| matches!(report, Report::Share(Shared::Started { .. }));
        assert!(!ada.reports.lock().unwrap().iter().any(started), "a share stopped while it began was sent");

        ada.ask(Command::Share(second, ShareQuality::default(), SoundAsked::Apps(Vec::new()))).await;
        ada.until("the next share is live", Duration::from_secs(30), started).await;
        let shared = |report: &Report| matches!(report, Report::Room(RoomEvent::Started { user_id, track: screens::Track::ScreenVideo }) if *user_id == ada_id);
        bea.until("Bea sees the next share", Duration::from_secs(30), shared).await;
        let told = bea.reports.lock().unwrap().iter().filter(|report| shared(report)).count();
        assert_eq!(told, 1, "the share let go never reached the room");
        bea.leave().await;
        ada.leave().await;
    }

    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread")]
        async fn a_share_goes_out_at_the_frame_rate_chosen_from_a_faster_screen() {
            let Some(pick) =
                painted_every(screen_support::WIDTH, screen_support::HEIGHT, false, Duration::from_millis(16))
            else {
                return skipped("no VA-API device to paint a screen on");
            };
            let quality = ShareQuality { frame_rate: FrameRate::Thirty, ..ShareQuality::default() };
            let (_server, ada, bea, _, _) = in_a_call_as(pick, 0, quality).await;
            ada.looks(6, Duration::from_secs(60)).await;
            let rates: Vec<u32> = ada
                .reports
                .lock()
                .unwrap()
                .iter()
                .filter_map(|report| match report {
                    Report::Share(Shared::Looked(outgoing)) => outgoing.frame_rate,
                    _ => None,
                })
                .collect();
            eprintln!("a screen of about 60 a second, shared at 30: {rates:?} a second");
            let settled = &rates[rates.len().saturating_sub(3)..];
            assert!(settled.len() == 3 && settled.iter().all(|rate| (26..=33).contains(rate)), "about 30: {rates:?}");
            bea.leave().await;
            ada.leave().await;
        }
    }
}
