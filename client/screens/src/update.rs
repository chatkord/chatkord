//! Chatkord's own update: whether this build and run can have one, when it is checked for by itself, fetched and
//! put in place, and what the side pane's notice, About Chatkord and the tray say of it. A restart never ends a call.

use choices::AutoUpdate;
use std::time::{Duration, SystemTime};
use words::{Phrase, Sentence};

/// How long after the start the first check is made by itself: the window and its servers come first.
pub const FIRST_CHECK: Duration = Duration::from_secs(15);

/// How long after a check the next is made by itself, by the wall clock, so a device woken from sleep checks soon.
pub const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);

/// How often the host reads the wall clock against the schedule while nothing is due sooner: how long a wake from
/// sleep goes unnoticed. A reading that finds nothing due is the host's alone and never reaches the window.
pub const CLOCK_EVERY: Duration = Duration::from_secs(60);

/// A share of a download, 0 to 100; more is clamped, never carried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Percent(u8);

impl Percent {
    /// `value`, more than 100 taken as 100.
    pub fn new(value: u8) -> Percent {
        Percent(value.min(100))
    }

    /// The share, 0 to 100.
    pub fn get(self) -> u8 {
        self.0
    }
}

/// What did not go through, tried again at the next check made by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unfinished {
    /// The check: the feed could not be read, or what it said was refused.
    Check,
    /// The download, or putting the new version in place.
    Download,
}

/// Where this run's update stands while this build and run can have one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStage {
    /// Not checked yet this run.
    Unchecked,
    /// A check is under way.
    Checking,
    /// The last check found no newer version.
    UpToDate,
    /// A newer version is being fetched and put in place.
    Downloading {
        /// The newer version, as its release names it.
        version: String,
        /// How much of it has arrived.
        progress: Percent,
    },
    /// The newer version is in place: a restart runs it.
    Ready {
        /// The newer version, as its release names it.
        version: String,
    },
    /// A check or a download did not go through.
    Failed(Unfinished),
}

impl UpdateStage {
    /// Whether a check may start: none is under way and nothing newer is fetched or in place.
    fn idle(&self) -> bool {
        match self {
            UpdateStage::Unchecked | UpdateStage::UpToDate | UpdateStage::Failed(_) => true,
            UpdateStage::Checking | UpdateStage::Downloading { .. } | UpdateStage::Ready { .. } => false,
        }
    }
}

/// Whether this build and run can update themselves, and where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Updates {
    /// This build has no update channel, or this run is not from a package an update replaces: updates come with the
    /// installed package.
    Unoffered,
    /// They can.
    Offered {
        /// Where the update stands.
        stage: UpdateStage,
        /// When the next check by itself is due, by the wall clock.
        due: SystemTime,
    },
}

/// What the host tells the window of its update, or a person asks of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateMessage {
    /// This build has an update channel and this run a package an update replaces, found at the start; the wall
    /// clock then.
    Offered(SystemTime),
    /// The wall clock, read once the check scheduled was due.
    Clock(SystemTime),
    /// "Check for updates" on About Chatkord.
    Check,
    /// What a check found, and the wall clock when it ended.
    Checked {
        /// When the check ended.
        at: SystemTime,
        /// What it found.
        found: UpdateFound,
    },
    /// How much of the newer version has arrived.
    Progress(Percent),
    /// The newer version was fetched whole, checked and put in place.
    Placed,
    /// It was not.
    NotPlaced,
    /// "Restart", on the notice, About Chatkord or the tray.
    Restart,
    /// About Chatkord's "Download updates automatically" switched.
    ToggleAutomatic,
}

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateFound {
    /// No newer version for this system.
    Nothing,
    /// A newer version, as its release names it.
    Newer(String),
    /// Nothing could be told: the feed was not read, or what it said was refused.
    Failed,
}

/// What the window asks of the host for its update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAsk {
    /// Report [`UpdateMessage::Clock`] once the wall clock reaches this time ([`is_due`]); none while no check is to be
    /// made by itself. Asked only when it changes, each replacing the one before.
    Schedule(Option<SystemTime>),
    /// Check the update channel; report [`UpdateMessage::Checked`].
    Check,
    /// Fetch the newer version the last check found and put it in place; report [`UpdateMessage::Progress`] as it
    /// arrives, then [`UpdateMessage::Placed`] or [`UpdateMessage::NotPlaced`].
    Download,
    /// End this run as a quit does, every server parted, then start the version in place.
    Restart,
}

/// What a restart is offered as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartOffer {
    /// A restart button.
    Offered,
    /// The update is ready but a call is running, which a restart would end.
    DeferredUntilVoiceEnds,
    /// There is nothing to restart into.
    NotApplicable,
}

/// Whether a call runs in this window, which a restart would end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InVoice {
    /// One does.
    Yes,
    /// None does.
    No,
}

/// Whether this window sends a screen or a camera, which a check made by itself waits out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendsVideo {
    /// It sends one or both.
    Yes,
    /// It sends neither.
    No,
}

/// What the side pane's notice shows of the update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateNotice {
    /// The newer version is arriving.
    Downloading(Percent),
    /// It is in place, and whether a restart is offered or waits for the call.
    Ready(RestartOffer),
    /// The download failed; the next check tries again.
    Failed,
}

impl UpdateNotice {
    /// What the notice says.
    pub fn phrase(self) -> Phrase {
        match self {
            UpdateNotice::Downloading(_) => Phrase::UpdateDownloading,
            UpdateNotice::Ready(RestartOffer::DeferredUntilVoiceEnds) => Phrase::UpdateWaitsForCall,
            UpdateNotice::Ready(RestartOffer::Offered | RestartOffer::NotApplicable) => Phrase::UpdateReady,
            UpdateNotice::Failed => Phrase::UpdateFailed,
        }
    }
}

/// The button beside About Chatkord's line on the update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateButton {
    /// "Check for updates".
    Check,
    /// "Restart".
    Restart,
}

/// What About Chatkord's Updates card shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCard {
    /// The line on where the update stands.
    pub line: Sentence,
    /// The button beside it, if one is offered.
    pub button: Option<UpdateButton>,
    /// "Download updates automatically" as switched; none where updates are not offered, drawn off and unusable.
    pub automatic: Option<AutoUpdate>,
}

impl Updates {
    /// Whether a restart is offered: only once the newer version is in place, and never while a call runs.
    pub fn restart_offer(&self, in_voice: InVoice) -> RestartOffer {
        match (self, in_voice) {
            (Updates::Offered { stage: UpdateStage::Ready { .. }, .. }, InVoice::Yes) => {
                RestartOffer::DeferredUntilVoiceEnds
            }
            (Updates::Offered { stage: UpdateStage::Ready { .. }, .. }, InVoice::No) => RestartOffer::Offered,
            (Updates::Unoffered | Updates::Offered { .. }, _) => RestartOffer::NotApplicable,
        }
    }

    /// The side pane's notice: while a version downloads or is ready, and a failed download where checks are made by
    /// themselves, since only then is it tried again.
    pub fn notice(&self, in_voice: InVoice, automatic: AutoUpdate) -> Option<UpdateNotice> {
        let Updates::Offered { stage, .. } = self else { return None };
        match (stage, automatic) {
            (UpdateStage::Downloading { progress, .. }, _) => Some(UpdateNotice::Downloading(*progress)),
            (UpdateStage::Ready { .. }, _) => Some(UpdateNotice::Ready(self.restart_offer(in_voice))),
            (UpdateStage::Failed(Unfinished::Download), AutoUpdate::On) => Some(UpdateNotice::Failed),
            (UpdateStage::Failed(Unfinished::Download), AutoUpdate::Off)
            | (
                UpdateStage::Unchecked
                | UpdateStage::Checking
                | UpdateStage::UpToDate
                | UpdateStage::Failed(Unfinished::Check),
                _,
            ) => None,
        }
    }

    /// What About Chatkord's Updates card shows.
    pub fn card(&self, in_voice: InVoice, automatic: AutoUpdate) -> UpdateCard {
        let said = |line, button| UpdateCard { line: Sentence::Phrase(line), button, automatic: Some(automatic) };
        let Updates::Offered { stage, .. } = self else {
            return UpdateCard { line: Sentence::Phrase(Phrase::UpdatesInPackage), button: None, automatic: None };
        };
        match (stage, in_voice) {
            (UpdateStage::Unchecked, _) => said(Phrase::UpdateUnchecked, Some(UpdateButton::Check)),
            (UpdateStage::Checking, _) => said(Phrase::UpdateChecking, None),
            (UpdateStage::UpToDate, _) => said(Phrase::UpdateUpToDate, Some(UpdateButton::Check)),
            (UpdateStage::Downloading { .. }, _) => said(Phrase::UpdateDownloading, None),
            (UpdateStage::Failed(_), _) => said(Phrase::UpdateNotChecked, Some(UpdateButton::Check)),
            (UpdateStage::Ready { version }, InVoice::No) => UpdateCard {
                line: Sentence::VersionReady(version.clone()),
                button: Some(UpdateButton::Restart),
                automatic: Some(automatic),
            },
            (UpdateStage::Ready { version }, InVoice::Yes) => {
                UpdateCard { line: Sentence::VersionWaits(version.clone()), button: None, automatic: Some(automatic) }
            }
        }
    }

    /// When the next check by itself is to be made: while updates are offered, checks are made by themselves, nothing
    /// is sent, and no check is under way and nothing newer fetched.
    pub(crate) fn schedule(&self, automatic: AutoUpdate, sending: SendsVideo) -> Option<SystemTime> {
        match self {
            Updates::Offered { stage, due }
                if automatic == AutoUpdate::On && sending == SendsVideo::No && stage.idle() =>
            {
                Some(*due)
            }
            Updates::Offered { .. } | Updates::Unoffered => None,
        }
    }

    /// Takes `message`, given whether checks are made by themselves, whether a call runs and whether this window
    /// sends a screen or a camera, and returns what the host is to do.
    pub(crate) fn update(
        &mut self,
        message: UpdateMessage,
        automatic: AutoUpdate,
        in_voice: InVoice,
        sending: SendsVideo,
    ) -> Vec<UpdateAsk> {
        let restart = self.restart_offer(in_voice);
        let scheduled = self.schedule(automatic, sending);
        match (message, &mut *self) {
            (UpdateMessage::Offered(now), Updates::Unoffered) => {
                let due = now.checked_add(FIRST_CHECK).unwrap_or(now);
                *self = Updates::Offered { stage: UpdateStage::Unchecked, due };
                Vec::new()
            }
            // A reading that came as the schedule changed checks only if it is still due.
            (UpdateMessage::Clock(now), Updates::Offered { stage, .. })
                if scheduled.is_some_and(|due| is_due(due, now)) =>
            {
                *stage = UpdateStage::Checking;
                vec![UpdateAsk::Check]
            }
            (UpdateMessage::Check, Updates::Offered { stage, .. }) if stage.idle() => {
                *stage = UpdateStage::Checking;
                vec![UpdateAsk::Check]
            }
            (UpdateMessage::Checked { at, found }, Updates::Offered { stage: stage @ UpdateStage::Checking, due }) => {
                *due = at.checked_add(CHECK_EVERY).unwrap_or(at);
                let (now, asked) = match found {
                    UpdateFound::Nothing => (UpdateStage::UpToDate, None),
                    UpdateFound::Newer(version) => {
                        (UpdateStage::Downloading { version, progress: Percent::default() }, Some(UpdateAsk::Download))
                    }
                    UpdateFound::Failed => (UpdateStage::Failed(Unfinished::Check), None),
                };
                *stage = now;
                asked.into_iter().collect()
            }
            (
                UpdateMessage::Progress(arrived),
                Updates::Offered { stage: UpdateStage::Downloading { progress, .. }, .. },
            ) => {
                *progress = arrived;
                Vec::new()
            }
            (UpdateMessage::Placed, Updates::Offered { stage, .. }) => {
                if let UpdateStage::Downloading { version, .. } = stage {
                    *stage = UpdateStage::Ready { version: std::mem::take(version) };
                }
                Vec::new()
            }
            (UpdateMessage::NotPlaced, Updates::Offered { stage: stage @ UpdateStage::Downloading { .. }, .. }) => {
                *stage = UpdateStage::Failed(Unfinished::Download);
                Vec::new()
            }
            (UpdateMessage::Restart, Updates::Offered { .. }) if restart == RestartOffer::Offered => {
                vec![UpdateAsk::Restart]
            }
            // A second start, a clock or an answer not asked for, a check while one runs or a restart not offered.
            (
                UpdateMessage::Offered(_)
                | UpdateMessage::Clock(_)
                | UpdateMessage::Check
                | UpdateMessage::Checked { .. }
                | UpdateMessage::Progress(_)
                | UpdateMessage::Placed
                | UpdateMessage::NotPlaced
                | UpdateMessage::Restart
                | UpdateMessage::ToggleAutomatic,
                _,
            ) => Vec::new(),
        }
    }
}

/// Whether a check scheduled for `due` is due at `now`: its time has come, or the wall clock was set back past a whole
/// interval.
pub fn is_due(due: SystemTime, now: SystemTime) -> bool {
    due.duration_since(now).ok().is_none_or(|left| left.is_zero() || left > CHECK_EVERY)
}

/// How long until the wall clock is read again against `due`: at that time, or a minute on, whichever comes first.
pub fn clock_in(due: SystemTime, now: SystemTime) -> Duration {
    due.duration_since(now).ok().filter(|left| !left.is_zero()).map_or(CLOCK_EVERY, |left| left.min(CLOCK_EVERY))
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: Duration = Duration::from_secs(1_800_000_000);
    const VERSION: &str = "0.9.1";

    fn wall(after: Duration) -> SystemTime {
        SystemTime::UNIX_EPOCH + START + after
    }

    /// The window's updates, with the switch, the call and what it sends as a test sets them.
    struct Run {
        updates: Updates,
        automatic: AutoUpdate,
        in_voice: InVoice,
        sending: SendsVideo,
    }

    impl Run {
        fn offered() -> Run {
            let mut run = Run {
                updates: Updates::Unoffered,
                automatic: AutoUpdate::On,
                in_voice: InVoice::No,
                sending: SendsVideo::No,
            };
            assert_eq!(run.schedule(), None);
            assert_eq!(run.tell(UpdateMessage::Offered(wall(Duration::ZERO))), []);
            run
        }

        fn tell(&mut self, message: UpdateMessage) -> Vec<UpdateAsk> {
            self.updates.update(message, self.automatic, self.in_voice, self.sending)
        }

        fn schedule(&self) -> Option<SystemTime> {
            self.updates.schedule(self.automatic, self.sending)
        }

        fn stage(&self) -> &UpdateStage {
            match &self.updates {
                Updates::Offered { stage, .. } => stage,
                Updates::Unoffered => panic!("not offered"),
            }
        }

        fn checked(&mut self, at: Duration, found: UpdateFound) -> Vec<UpdateAsk> {
            self.tell(UpdateMessage::Checked { at: wall(at), found })
        }
    }

    #[test]
    fn the_first_check_is_scheduled_fifteen_seconds_after_the_start_then_four_hours_after_each() {
        let mut run = Run::offered();
        assert_eq!(run.schedule(), Some(wall(FIRST_CHECK)));
        assert_eq!(run.tell(UpdateMessage::Clock(wall(FIRST_CHECK))), [UpdateAsk::Check]);
        assert_eq!((run.stage(), run.schedule()), (&UpdateStage::Checking, None), "none while one is under way");
        assert_eq!(run.checked(FIRST_CHECK + Duration::from_secs(2), UpdateFound::Nothing), []);
        assert_eq!(run.stage(), &UpdateStage::UpToDate);
        assert_eq!(run.schedule(), Some(wall(FIRST_CHECK + Duration::from_secs(2) + CHECK_EVERY)));
    }

    #[test]
    fn the_clock_is_read_at_the_time_due_or_a_minute_on_and_a_wake_from_sleep_is_due() {
        let due = wall(CHECK_EVERY);
        assert_eq!(clock_in(due, wall(CHECK_EVERY - Duration::from_secs(30))), Duration::from_secs(30));
        assert_eq!(clock_in(due, wall(Duration::ZERO)), CLOCK_EVERY);
        assert_eq!(clock_in(due, wall(CHECK_EVERY + Duration::from_secs(9))), CLOCK_EVERY, "past due: a minute on");
        assert!(!is_due(due, wall(CHECK_EVERY - Duration::from_secs(1))));
        assert!(is_due(due, wall(CHECK_EVERY)));
        // Asleep for a night: the next reading is a night later by the wall clock.
        assert!(is_due(due, wall(Duration::from_secs(13 * 60 * 60))));
        let back = SystemTime::UNIX_EPOCH + START - Duration::from_secs(24 * 60 * 60);
        assert!(is_due(due, back), "a clock set back past a whole interval does not put the check off");
    }

    #[test]
    fn a_reading_before_its_time_changes_nothing() {
        let mut run = Run::offered();
        assert_eq!(run.tell(UpdateMessage::Clock(wall(Duration::from_secs(14)))), []);
        assert_eq!(run.stage(), &UpdateStage::Unchecked);
    }

    #[test]
    fn no_check_is_scheduled_while_a_screen_or_camera_is_sent_and_one_is_once_it_is_not() {
        let mut run = Run::offered();
        run.sending = SendsVideo::Yes;
        assert_eq!(run.schedule(), None, "postponed");
        assert_eq!(run.tell(UpdateMessage::Clock(wall(FIRST_CHECK))), [], "a reading that crossed the change");
        assert_eq!(run.stage(), &UpdateStage::Unchecked);
        run.sending = SendsVideo::No;
        assert_eq!(run.schedule(), Some(wall(FIRST_CHECK)), "due at once");
        assert_eq!(run.tell(UpdateMessage::Clock(wall(FIRST_CHECK + CLOCK_EVERY))), [UpdateAsk::Check]);
    }

    #[test]
    fn with_the_switch_off_no_check_is_scheduled_but_one_asked_for_is_made_and_fetches_what_it_finds() {
        let mut run = Run::offered();
        run.automatic = AutoUpdate::Off;
        assert_eq!(run.schedule(), None);
        assert_eq!(run.tell(UpdateMessage::Clock(wall(FIRST_CHECK + CHECK_EVERY))), []);
        assert_eq!(run.stage(), &UpdateStage::Unchecked);
        run.sending = SendsVideo::Yes;
        assert_eq!(run.tell(UpdateMessage::Check), [UpdateAsk::Check], "asked for, it is made at once");
        assert_eq!(run.tell(UpdateMessage::Check), [], "one check at a time");
        assert_eq!(run.checked(CHECK_EVERY, UpdateFound::Newer(VERSION.into())), [UpdateAsk::Download]);
        let downloading = UpdateStage::Downloading { version: VERSION.into(), progress: Percent::default() };
        assert_eq!(run.stage(), &downloading);
        run.automatic = AutoUpdate::On;
        run.sending = SendsVideo::No;
        assert_eq!(run.schedule(), None, "switched on, nothing is checked while a version downloads");
    }

    #[test]
    fn a_version_found_downloads_and_is_ready_and_a_restart_waits_for_the_call_to_end() {
        let mut run = Run::offered();
        run.tell(UpdateMessage::Clock(wall(FIRST_CHECK)));
        run.checked(FIRST_CHECK, UpdateFound::Newer(VERSION.into()));
        run.tell(UpdateMessage::Progress(Percent::new(64)));
        assert_eq!(run.updates.notice(InVoice::No, AutoUpdate::On), Some(UpdateNotice::Downloading(Percent::new(64))));
        assert_eq!(run.tell(UpdateMessage::Restart), [], "nothing to restart into yet");
        run.tell(UpdateMessage::Placed);
        assert_eq!(run.stage(), &UpdateStage::Ready { version: VERSION.into() });
        run.in_voice = InVoice::Yes;
        assert_eq!(run.tell(UpdateMessage::Restart), [], "a restart never ends a call");
        let waits = UpdateNotice::Ready(RestartOffer::DeferredUntilVoiceEnds);
        assert_eq!(run.updates.notice(InVoice::Yes, AutoUpdate::On), Some(waits));
        assert_eq!(waits.phrase(), Phrase::UpdateWaitsForCall);
        let card = run.updates.card(InVoice::Yes, AutoUpdate::On);
        assert_eq!((card.line, card.button), (Sentence::VersionWaits(VERSION.into()), None));
        run.in_voice = InVoice::No;
        let card = run.updates.card(InVoice::No, AutoUpdate::On);
        assert_eq!((card.line, card.button), (Sentence::VersionReady(VERSION.into()), Some(UpdateButton::Restart)));
        assert_eq!(run.tell(UpdateMessage::Restart), [UpdateAsk::Restart]);
        assert_eq!(run.schedule(), None, "ready, nothing is checked again");
        assert_eq!(run.tell(UpdateMessage::Check), []);
    }

    #[test]
    fn a_failed_download_is_noticed_and_tried_again_at_the_next_check() {
        let mut run = Run::offered();
        run.tell(UpdateMessage::Clock(wall(FIRST_CHECK)));
        run.checked(FIRST_CHECK, UpdateFound::Newer(VERSION.into()));
        run.tell(UpdateMessage::NotPlaced);
        assert_eq!(run.stage(), &UpdateStage::Failed(Unfinished::Download));
        assert_eq!(run.updates.notice(InVoice::No, AutoUpdate::On), Some(UpdateNotice::Failed));
        assert_eq!(run.updates.notice(InVoice::No, AutoUpdate::Off), None, "no later try to promise");
        assert_eq!(run.updates.card(InVoice::No, AutoUpdate::On).line, Sentence::Phrase(Phrase::UpdateNotChecked));
        assert_eq!(run.schedule(), Some(wall(FIRST_CHECK + CHECK_EVERY)));
        assert_eq!(run.tell(UpdateMessage::Clock(wall(FIRST_CHECK + CHECK_EVERY))), [UpdateAsk::Check]);
        assert_eq!(run.checked(FIRST_CHECK + CHECK_EVERY, UpdateFound::Newer(VERSION.into())), [UpdateAsk::Download]);
    }

    #[test]
    fn a_failed_check_is_said_on_the_page_alone_and_can_be_asked_again() {
        let mut run = Run::offered();
        run.tell(UpdateMessage::Clock(wall(FIRST_CHECK)));
        run.checked(FIRST_CHECK, UpdateFound::Failed);
        assert_eq!(run.updates.notice(InVoice::No, AutoUpdate::On), None);
        let card = run.updates.card(InVoice::No, AutoUpdate::On);
        assert_eq!((card.line, card.button), (Sentence::Phrase(Phrase::UpdateNotChecked), Some(UpdateButton::Check)));
        assert_eq!(run.tell(UpdateMessage::Check), [UpdateAsk::Check]);
    }

    #[test]
    fn an_answer_nothing_asked_for_changes_nothing() {
        let mut run = Run::offered();
        assert_eq!(run.checked(FIRST_CHECK, UpdateFound::Newer(VERSION.into())), []);
        run.tell(UpdateMessage::Progress(Percent::new(50)));
        run.tell(UpdateMessage::Placed);
        run.tell(UpdateMessage::NotPlaced);
        assert_eq!(run.stage(), &UpdateStage::Unchecked);
        assert_eq!(run.tell(UpdateMessage::Offered(wall(CHECK_EVERY))), [], "found once, at the start");
        assert_eq!(run.schedule(), Some(wall(FIRST_CHECK)));
    }

    #[test]
    fn a_build_with_no_channel_says_updates_come_with_the_package_and_asks_for_nothing() {
        let mut run = Run::offered();
        run.updates = Updates::Unoffered;
        for message in [UpdateMessage::Clock(wall(CHECK_EVERY)), UpdateMessage::Check, UpdateMessage::Restart] {
            assert_eq!(run.tell(message), []);
        }
        assert_eq!(run.schedule(), None);
        let card = run.updates.card(InVoice::No, AutoUpdate::On);
        assert_eq!(
            card,
            UpdateCard { line: Sentence::Phrase(Phrase::UpdatesInPackage), button: None, automatic: None }
        );
        assert_eq!(run.updates.notice(InVoice::No, AutoUpdate::On), None);
        assert_eq!(run.updates.restart_offer(InVoice::No), RestartOffer::NotApplicable);
    }

    #[test]
    fn the_card_offers_a_check_only_while_none_is_under_way_and_nothing_is_fetched() {
        let card = |stage| Updates::Offered { stage, due: wall(CHECK_EVERY) }.card(InVoice::No, AutoUpdate::Off);
        let shown = |stage| {
            let card = card(stage);
            (card.line, card.button, card.automatic)
        };
        let off = Some(AutoUpdate::Off);
        let check = Some(UpdateButton::Check);
        assert_eq!(shown(UpdateStage::Unchecked), (Sentence::Phrase(Phrase::UpdateUnchecked), check, off));
        assert_eq!(shown(UpdateStage::Checking), (Sentence::Phrase(Phrase::UpdateChecking), None, off));
        assert_eq!(shown(UpdateStage::UpToDate), (Sentence::Phrase(Phrase::UpdateUpToDate), check, off));
        let downloading = UpdateStage::Downloading { version: VERSION.into(), progress: Percent::new(3) };
        assert_eq!(shown(downloading), (Sentence::Phrase(Phrase::UpdateDownloading), None, off));
    }

    #[test]
    fn a_percent_is_clamped() {
        assert_eq!(Percent::new(250).get(), 100);
        assert_eq!(Percent::new(7).get(), 7);
    }
}
