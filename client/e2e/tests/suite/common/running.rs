//! One account's call, as the window runs it, for the tests that drive whole calls: the slot each picture sent is put
//! in, the reports it made, and what it is asked. A test waits for the call's next report, never for a time.

use call::room::{Command, Join, Leaving, Report, Screens, Source, Wants, run};
use link::{Events, Link};
use proto::video::Video;
use screens::{RoomEvent, Track};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use terms::MemberId;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

/// One account's call, as the window runs it.
pub(crate) struct Running {
    /// What the person wants of the call, changed as the window would.
    pub(crate) wants: watch::Sender<Wants>,
    commands: mpsc::Sender<Command>,
    leaving: Leaving,
    pub(crate) reports: Arc<Mutex<Vec<Report>>>,
    /// How many reports the call has made, told as each is: what `until` and `looks` wait on.
    told: watch::Receiver<usize>,
    /// The slot each picture sent is put in, by its sender's id and kind, as the window makes them.
    pictures: Arc<Mutex<HashMap<(MemberId, Video), call::Slot>>>,
    /// Where this account's own screen is lent, as for its card.
    #[cfg_attr(windows, expect(dead_code, reason = "only Linux's share tests read it"))]
    pub(crate) own: call::Slot,
    /// Where this account's own camera's pictures are put, as for its card.
    pub(crate) own_camera: call::Slot,
    pub(crate) task: JoinHandle<Result<(), call::room::Stopped>>,
    _link: Link,
}

/// Runs `me`'s call in `channel_id` over `link`, showing every picture sent.
pub(crate) fn start(link: Link, events: Events, channel_id: String, me: MemberId) -> Running {
    start_hearing_on(link, events, channel_id, me, None)
}

/// The same, playing the call on `speaker`, or the default for none.
#[expect(clippy::needless_pass_by_value, reason = "a test hands over the id the server gave, as it gave it")]
pub(crate) fn start_hearing_on(
    link: Link,
    mut events: Events,
    channel_id: String,
    me: MemberId,
    speaker: Option<choices::Picked>,
) -> Running {
    let (commands, queued) = mpsc::channel(8);
    let (wants, wanted) = watch::channel(Wants::default());
    let leaving = Leaving::default();
    let left = leaving.clone();
    let reports = Arc::new(Mutex::new(Vec::new()));
    let (count, told) = watch::channel(0);
    let pictures: Arc<Mutex<HashMap<(MemberId, Video), call::Slot>>> = Arc::default();
    let (own, own_camera) = (call::Slot::new(), call::Slot::new());
    let made = Arc::clone(&pictures);
    let slot_of = move |user_id: &MemberId, video: Video| {
        made.lock().unwrap().entry((user_id.clone(), video)).or_default().clone()
    };
    let join = Join {
        channel_id: terms::ChannelId::parse(&channel_id).expect("a channel id"),
        me: Some(me),
        source: Source::Nothing,
        microphone: None,
        speaker,
        processing: choices::VoiceProcessing::default(),
        level_every: super::looks::LEVELS_EVERY,
        screens: Some(Screens::new(slot_of)),
        own_screen: Some(own.clone()),
        own_camera: Some(own_camera.clone()),
        painting: None,
        jitter: super::even(),
        encoders: Arc::new(media::Survey::system()),
        chain: call::ChainWatch::default(),
    };
    let (kept, made) = (link.clone(), Arc::clone(&reports));
    let task = tokio::spawn(async move {
        let asked = (wanted, queued);
        let report = move |report| {
            made.lock().unwrap().push(report);
            count.send_modify(|reports| *reports += 1);
        };
        run(&link, super::engine(), &mut events, join, asked, left, report).await
    });
    Running { wants, commands, leaving, reports, told, pictures, own, own_camera, task, _link: kept }
}

impl Running {
    /// The slot `user_id`'s screen is put in.
    pub(crate) fn screen(&self, user_id: &MemberId) -> call::Slot {
        self.picture(user_id, Video::Screen)
    }

    /// When the next picture of `user_id`'s `video` is put in its slot, told as the window is woken for it; a wake that
    /// put no picture in (the slot emptied as a watch starts) is not one.
    #[cfg(target_os = "linux")]
    pub(crate) fn next_picture(
        &self,
        user_id: &MemberId,
        video: Video,
    ) -> tokio::sync::oneshot::Receiver<std::time::Instant> {
        let (put, next) = tokio::sync::oneshot::channel();
        let put = Mutex::new(Some(put));
        let slot = self.picture(user_id, video);
        let (seen, before) = (slot.clone(), slot.stats().received);
        slot.wake_with(move || {
            let received = seen.stats().received;
            if received > 0
                && received != before
                && let Some(put) = put.lock().unwrap().take()
            {
                let _ = put.send(std::time::Instant::now());
            }
        });
        next
    }

    /// The slot `user_id`'s picture of `video` is put in.
    pub(crate) fn picture(&self, user_id: &MemberId, video: Video) -> call::Slot {
        self.pictures.lock().unwrap().entry((user_id.clone(), video)).or_default().clone()
    }

    pub(crate) async fn ask(&self, command: Command) {
        self.commands.send(command).await.expect("the call is running");
    }

    /// Waits until a report satisfies `holds`, woken by each report the call makes, for no longer than `within`.
    pub(crate) async fn until(&self, what: &str, within: Duration, holds: impl Fn(&Report) -> bool) -> Duration {
        let begun = Instant::now();
        let mut told = self.told.clone();
        drop(told.borrow_and_update());
        loop {
            if self.reports.lock().unwrap().iter().any(&holds) {
                return begun.elapsed();
            }
            let next = timeout_at(begun + within, told.changed()).await;
            assert!(
                matches!(next, Ok(Ok(()))),
                "{what}: not within {within:?}; the reports were {:#?}",
                self.reports.lock().unwrap()
            );
        }
    }

    /// The member whose `track` the call was told is live, from the room's own events.
    pub(crate) fn sender_of(&self, wanted: Track) -> Option<MemberId> {
        self.reports.lock().unwrap().iter().find_map(|report| match report {
            Report::Room(RoomEvent::Started { user_id, track }) if *track == wanted => Some(user_id.clone()),
            Report::Room(RoomEvent::Joined { streams, .. }) => {
                streams.iter().find(|(_, track)| *track == wanted).map(|(user_id, _)| user_id.clone())
            }
            _ => None,
        })
    }

    /// Waits for the call's next report, for no longer than `within`; false when none came.
    #[cfg(target_os = "linux")]
    pub(crate) async fn next(&self, within: Duration) -> bool {
        let mut told = self.told.clone();
        drop(told.borrow_and_update());
        matches!(timeout_at(Instant::now() + within, told.changed()).await, Ok(Ok(())))
    }

    /// Waits until `check` finds something, looking again at each report the call makes (its levels come every
    /// 100 ms, its looks every `LOOK_EVERY`), for no longer than `within`: for a fact the call keeps in a slot that
    /// is not itself a report.
    pub(crate) async fn when<R>(&self, what: &str, within: Duration, mut check: impl FnMut() -> Option<R>) -> R {
        let begun = Instant::now();
        let mut told = self.told.clone();
        drop(told.borrow_and_update());
        loop {
            if let Some(found) = check() {
                return found;
            }
            let next = timeout_at(begun + within, told.changed()).await;
            assert!(matches!(next, Ok(Ok(()))), "{what}: not within {within:?}");
        }
    }

    /// Waits until the call has looked at its connection `looks` more times (a look every `screens::LOOK_EVERY`), the
    /// clock a measuring window counts in. It returns how long that took.
    pub(crate) async fn looks(&self, looks: usize, within: Duration) -> Duration {
        let seen = |reports: &[Report]| reports.iter().filter(|report| matches!(report, Report::Connection(_))).count();
        let already = seen(&self.reports.lock().unwrap());
        let begun = Instant::now();
        let mut told = self.told.clone();
        drop(told.borrow_and_update());
        loop {
            if seen(&self.reports.lock().unwrap()) >= already + looks {
                return begun.elapsed();
            }
            let next = timeout_at(begun + within, told.changed()).await;
            assert!(matches!(next, Ok(Ok(()))), "{looks} looks of the connection: not within {within:?}");
        }
    }

    pub(crate) async fn leave(self) {
        self.leaving.leave();
        let ended = tokio::time::timeout(Duration::from_secs(30), self.task).await;
        assert!(matches!(ended, Ok(Ok(Ok(())))), "the call ended by being left");
    }
}
