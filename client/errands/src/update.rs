//! Chatkord's own update for the window: whether this build and run can have one, its checks and downloads carried
//! out one after another, the wall clock read against the check scheduled, and the run a restart hands the window
//! over to.

use crate::mailbox::Mailbox;
use link::Trust;
use screens::{Percent, UpdateFound, UpdateMessage, WindowMessage};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;
use tokio::sync::{mpsc, watch};
use update::{Channel, NoChannel, Offer, Target, UpdateError, Version, VersionRefused};

/// The work on the update that waits its turn: a download needs the offer the check before it found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Work {
    Check,
    Download,
}

/// What this build and run update from and replace.
pub(crate) struct Found {
    channel: Channel,
    target: Target,
    running: Version,
}

/// Why this run offers no update: it is said in the log, and the window says updates come with the package.
#[derive(Debug, report::Public)]
pub(crate) enum Unoffered {
    Channel(NoChannel),
    Version(VersionRefused),
    Target(UpdateError),
}

impl fmt::Display for Unoffered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unoffered::Channel(why) => write!(f, "{why}"),
            Unoffered::Version(why) => write!(f, "this build's version: {why}"),
            Unoffered::Target(why) => write!(f, "{why}"),
        }
    }
}

/// This build's update channel, its version and the package this run replaces, checked under the system's verifier.
pub(crate) async fn find(trust: &Trust) -> Result<Found, Unoffered> {
    let channel = Channel::of_this_build(trust).map_err(Unoffered::Channel)?;
    let running = Version::of_this_build().map_err(Unoffered::Version)?;
    let target = Target::of_this_run().await.map_err(Unoffered::Target)?;
    Ok(Found { channel, target, running })
}

impl Found {
    /// The package a restart starts.
    pub(crate) fn target(&self) -> &Target {
        &self.target
    }
}

/// Carries out the window's checks and downloads in the order asked, each answer told to the window, until the
/// window is gone.
pub(crate) async fn serve(found: Found, mut asks: mpsc::UnboundedReceiver<Work>, mailbox: Mailbox) {
    let Found { channel, target, running } = found;
    let mut offer: Option<Offer> = None;
    while let Some(work) = asks.recv().await {
        let told = match work {
            Work::Check => {
                let found = match update::check(&channel, running).await {
                    Ok(None) => UpdateFound::Nothing,
                    Ok(Some(newer)) => {
                        let version = newer.version().to_string();
                        offer = Some(newer);
                        UpdateFound::Newer(version)
                    }
                    Err(why) => {
                        report::note!(warn, "the update was not checked", why = why);
                        UpdateFound::Failed
                    }
                };
                UpdateMessage::Checked { at: SystemTime::now(), found }
            }
            Work::Download => match download(&channel, offer.as_ref(), &target, &mailbox).await {
                Ok(()) => UpdateMessage::Placed,
                Err(why) => {
                    report::note!(warn, "the update was not put in place", why = why);
                    UpdateMessage::NotPlaced
                }
            },
        };
        mailbox.tell(WindowMessage::Update(told)).await;
    }
}

/// `offer`'s package fetched beside `target`, each new percent told to the window, then put in its place.
async fn download(
    channel: &Channel,
    offer: Option<&Offer>,
    target: &Target,
    mailbox: &Mailbox,
) -> Result<(), UpdateError> {
    // The window asks for a download only after a check found one.
    let offer = offer.ok_or(UpdateError::NotOffered)?;
    let mut told = None;
    let fetched = update::fetch(channel, offer, target, |progress| {
        let percent = progress.percent();
        if told != Some(percent) {
            told = Some(percent);
            // A share missed while the window is behind is the next one's to tell.
            mailbox.try_tell(WindowMessage::Update(UpdateMessage::Progress(Percent::new(percent))));
        }
    })
    .await?;
    update::place(fetched).await
}

/// Reads `clock` against the check `schedule` holds, at its time and every minute, and tells the window only once it
/// is due: a reading that finds nothing due costs the window nothing. Each change of the schedule is read at once;
/// none schedules nothing. Ends when the window is gone.
pub(crate) async fn tell_when_due(
    mut schedule: watch::Receiver<Option<SystemTime>>,
    clock: impl Fn() -> SystemTime + Send,
    mailbox: Mailbox,
) {
    loop {
        let scheduled = *schedule.borrow_and_update();
        let wait = match scheduled {
            None => None,
            Some(due) => {
                let now = clock();
                if screens::is_due(due, now) {
                    mailbox.tell(WindowMessage::Update(UpdateMessage::Clock(now))).await;
                    // The window's answer schedules the next; told once a minute until it does.
                    Some(screens::CLOCK_EVERY)
                } else {
                    Some(screens::clock_in(due, now))
                }
            }
        };
        let changed = match wait {
            None => schedule.changed().await.is_ok(),
            Some(wait) => tokio::select! {
                changed = schedule.changed() => changed.is_ok(),
                () = tokio::time::sleep(wait) => true,
            },
        };
        if !changed {
            return;
        }
    }
}

/// The run to start once this one has let the window go: the package a restart was asked into, none until then.
#[derive(Clone, Debug, Default)]
pub struct Handover(Arc<Mutex<Option<Target>>>);

impl Handover {
    /// `target` is the run to start.
    pub(crate) fn hand(&self, target: Target) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(target);
    }

    /// The run to start, if a restart was asked for.
    pub fn take(&self) -> Option<Target> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}
