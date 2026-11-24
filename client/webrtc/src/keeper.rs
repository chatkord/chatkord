//! The chain's two keepers: for each device, one thread that opens, chooses, keeps and drops its streams, so a call
//! into the sound server that does not return holds up that device's keeper and nothing else. The mix thread holds
//! only the device's end ([`crate::chain::MicrophoneEnd`], [`crate::chain::SpeakerEnd`]), never a stream.

use crate::chain::{
    Cadence, Counts, Microphone, MicrophoneEnd, RESTART_WITHIN, STOP_BOUND, Shared, Sound, Speaker, SpeakerEnd,
    Standing, Which, lock,
};
use crate::devices::DevicesRefused;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

/// What a keeper's thread is doing now, so a call that never returns is named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum Keeping {
    /// Waiting for an ask or its next look.
    #[default]
    Idle,
    /// Opening the device.
    Opening,
    /// Moving the device it holds to the one chosen.
    Choosing,
    /// Looking after the stream: handing over to a device chosen, opening a failed one again.
    #[expect(clippy::enum_variant_names, reason = "the step is named for what a keeper does")]
    Keeping,
    /// Letting go of the device.
    Closing,
}

impl Keeping {
    const ALL: [Self; 5] = [Self::Idle, Self::Opening, Self::Choosing, Self::Keeping, Self::Closing];

    fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Opening => 1,
            Self::Choosing => 2,
            Self::Keeping => 3,
            Self::Closing => 4,
        }
    }

    fn of(code: u8) -> Self {
        Self::ALL.into_iter().find(|doing| doing.code() == code).unwrap_or(Self::Idle)
    }
}

/// How one keeper is doing at one moment, by its own counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keeper {
    /// Looks the keeper has finished: one that stops growing is a thread held in [`Keeper::doing`].
    pub turns: u64,
    /// What the keeper is doing now.
    pub doing: Keeping,
}

/// What one keeper publishes, read without waiting on it.
#[derive(Default)]
pub(crate) struct Stage {
    doing: AtomicU8,
    turns: AtomicU64,
    /// When the look in hand began; none between looks.
    since: Mutex<Option<Instant>>,
}

impl Stage {
    fn doing(&self, doing: Keeping) {
        self.doing.store(doing.code(), Ordering::Relaxed);
    }

    fn began(&self, now: Instant) {
        *lock(&self.since) = Some(now);
    }

    fn finished(&self) {
        *lock(&self.since) = None;
        self.doing(Keeping::Idle);
        self.turns.fetch_add(1, Ordering::Relaxed);
    }

    fn stuck(&self, now: Instant, bound: Duration) -> bool {
        lock(&self.since).is_some_and(|since| now.saturating_duration_since(since) >= bound)
    }

    /// How long until the look in hand passes `bound`; none between looks or once it has.
    fn left(&self, now: Instant, bound: Duration) -> Option<Duration> {
        let since = (*lock(&self.since))?;
        bound.checked_sub(now.saturating_duration_since(since)).filter(|left| !left.is_zero())
    }

    /// The keeper in the look was given up on: the one in its place has begun none.
    fn given_up(&self) {
        *lock(&self.since) = None;
    }

    pub(crate) fn health(&self) -> Keeper {
        Keeper { turns: self.turns.load(Ordering::Relaxed), doing: Keeping::of(self.doing.load(Ordering::Relaxed)) }
    }
}

/// Whether a keeper was given up on: whatever its stuck call returns is dropped on its own thread, and it touches
/// nothing shared.
#[derive(Default)]
pub(crate) struct Lease(AtomicBool);

impl Lease {
    fn abandon(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn abandoned(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// What a keeper needs of either device.
pub(crate) trait Device {
    const WHICH: Which;
    /// What the mix thread holds of it.
    type End: ?Sized + Send + 'static;
    fn open(
        sound: &dyn Sound,
        device: Option<&str>,
        wake: &Thread,
        now: Instant,
    ) -> Result<(Box<Self>, Box<Self::End>), DevicesRefused>;
    fn moved(&mut self, device: Option<&str>) -> Result<(), DevicesRefused>;
    fn keep(&mut self, now: Instant);
    fn flows(&self) -> bool;
    /// Tells the counts it keeps.
    fn count(&self, counts: &Counts);
    /// A device that opened or closed leaves nothing of the last one's sound to read.
    fn changed(shared: &Shared);
}

impl Device for dyn Microphone {
    const WHICH: Which = Which::Microphone;
    type End = dyn MicrophoneEnd;

    fn open(
        sound: &dyn Sound,
        device: Option<&str>,
        wake: &Thread,
        _now: Instant,
    ) -> Result<(Box<Self>, Box<Self::End>), DevicesRefused> {
        sound.microphone(device, wake.clone())
    }

    fn moved(&mut self, device: Option<&str>) -> Result<(), DevicesRefused> {
        self.choose(device)
    }

    fn keep(&mut self, now: Instant) {
        Microphone::keep(self, now);
    }

    fn flows(&self) -> bool {
        self.is_open()
    }

    fn count(&self, counts: &Counts) {
        counts.microphone_dropped.store(self.dropped(), Ordering::Relaxed);
    }

    fn changed(shared: &Shared) {
        shared.level.store(0, Ordering::Relaxed);
        *lock(&shared.loudness) = None;
    }
}

impl Device for dyn Speaker {
    const WHICH: Which = Which::Speaker;
    type End = dyn SpeakerEnd;

    fn open(
        sound: &dyn Sound,
        device: Option<&str>,
        _wake: &Thread,
        now: Instant,
    ) -> Result<(Box<Self>, Box<Self::End>), DevicesRefused> {
        sound.speaker(device, now)
    }

    fn moved(&mut self, device: Option<&str>) -> Result<(), DevicesRefused> {
        self.choose(device)
    }

    fn keep(&mut self, now: Instant) {
        Speaker::keep(self, now);
    }

    fn flows(&self) -> bool {
        self.is_open()
    }

    fn count(&self, counts: &Counts) {
        let (dropped, underruns) = self.losses();
        counts.speaker_dropped.store(dropped, Ordering::Relaxed);
        counts.speaker_underruns.store(underruns, Ordering::Relaxed);
    }

    fn changed(_shared: &Shared) {}
}

/// A device as its keeper holds it, and the ask it last answered.
struct Held<D: ?Sized> {
    device: Option<Box<D>>,
    tried: Option<u64>,
}

impl<D: ?Sized> Default for Held<D> {
    fn default() -> Self {
        Self { device: None, tried: None }
    }
}

impl<D: ?Sized> Held<D> {
    /// How the keeper of `which` sleeps before its next look: parked while nothing is asked of the device and nothing
    /// of it is held.
    fn cadence(&self, which: Which, shared: &Shared) -> Cadence {
        let asked = which.asked(&lock(&shared.state)).holds > 0;
        if asked || self.device.is_some() || self.tried.is_some() { Cadence::Turning } else { Cadence::Parked }
    }
}

/// Makes `held` what is asked: closed with no holder, else opened by `open` or moved to the device chosen, once for
/// each ask. The lock is held only to read what is asked and to say what became of it, never while the host works.
/// Once `lease` is given up, nothing is said of what the host returned.
fn reconcile<D: Device + ?Sized>(
    shared: &Shared,
    lease: &Lease,
    held: &mut Held<D>,
    open: impl FnOnce(Option<&str>) -> Result<Box<D>, DevicesRefused>,
) {
    let which = D::WHICH;
    let stage = which.stage(shared);
    let (holds, asks) = {
        let state = lock(&shared.state);
        let asked = which.asked(&state);
        (asked.holds, asked.asks)
    };
    if holds == 0 {
        if held.device.is_some() || held.tried.is_some() {
            stage.doing(Keeping::Closing);
            *held = Held::default();
            if lease.abandoned() {
                return;
            }
            which.flowing(shared).store(false, Ordering::Relaxed);
            shared.tell(which, asks, Standing::Closed);
        }
        return;
    }
    if held.tried == Some(asks) {
        return;
    }
    let (asks, device) = {
        let state = lock(&shared.state);
        let asked = which.asked(&state);
        (asked.asks, asked.device.clone())
    };
    stage.doing(if held.device.is_some() { Keeping::Choosing } else { Keeping::Opening });
    shared.tell(which, asks, Standing::Opening);
    let done = if let Some(running) = held.device.as_deref_mut() {
        running.moved(device.as_deref())
    } else {
        open(device.as_deref()).map(|opened| held.device = Some(opened))
    };
    if lease.abandoned() {
        return;
    }
    held.tried = Some(asks);
    which.flowing(shared).store(held.device.as_deref().is_some_and(Device::flows), Ordering::Relaxed);
    shared.tell(which, asks, done.map_or_else(Standing::Refused, |()| Standing::Open));
}

/// What a keeper's thread shares with the mix thread and its owner.
struct Seat<'a, D: Device + ?Sized> {
    shared: &'a Shared,
    lease: &'a Lease,
    mix: &'a Thread,
    ends: &'a mpsc::Sender<Option<Box<D::End>>>,
}

impl<D: Device + ?Sized> Seat<'_, D> {
    /// Hands the mix thread `end` as the one in use, none as the device closed, and wakes it to take it up.
    fn hand(&self, end: Option<Box<D::End>>) {
        // The mix is gone only with the chain, which stops this thread next.
        let _ = self.ends.send(end);
        self.mix.unpark();
    }

    /// One look at `now`: the device made what is asked, then kept.
    fn look(&self, held: &mut Held<D>, epoch: &mut u32, now: Instant) {
        let (shared, lease, which) = (self.shared, self.lease, D::WHICH);
        let stage = which.stage(shared);
        let seen = shared.epoch.load(Ordering::Acquire);
        if std::mem::replace(epoch, seen) != seen {
            *held = Held::default();
            which.flowing(shared).store(false, Ordering::Relaxed);
            self.hand(None);
        }
        let was_open = held.device.is_some();
        reconcile(shared, lease, held, |device| {
            let (opened, end) = D::open(&*shared.sound, device, self.mix, now)?;
            if !lease.abandoned() {
                self.hand(Some(end));
            }
            Ok(opened)
        });
        if lease.abandoned() {
            return;
        }
        let is_open = held.device.is_some();
        if was_open != is_open {
            D::changed(shared);
            if !is_open {
                self.hand(None);
            }
        }
        let Some(device) = held.device.as_deref_mut() else { return };
        stage.doing(Keeping::Keeping);
        let flowing = which.flowing(shared);
        flowing.store(device.flows(), Ordering::Relaxed);
        device.keep(now);
        if lease.abandoned() {
            return;
        }
        flowing.store(device.flows(), Ordering::Relaxed);
        device.count(&shared.counts);
    }
}

/// The keeper's thread: looks after the device as often as it is asked, and every [`crate::chain::IDLE`] without
/// while the device is asked or held; with neither it sleeps until asked. It takes the time once per look. A look
/// that panics drops the device to be opened again; one that panics again within [`RESTART_WITHIN`] refuses what is
/// asked until it changes.
fn run<D: Device + ?Sized>(seat: &Seat<'_, D>) {
    let (shared, which) = (seat.shared, D::WHICH);
    let stage = which.stage(shared);
    let mut held = Held::<D>::default();
    let mut epoch = shared.epoch.load(Ordering::Acquire);
    let mut panicked: Option<Instant> = None;
    while !seat.lease.abandoned() && !shared.stop.load(Ordering::Acquire) && !shared.halted.load(Ordering::Acquire) {
        stage.doing(Keeping::Idle);
        held.cadence(which, shared).sleep();
        let now = Instant::now();
        stage.began(now);
        // Sound to go on after: what a look leaves half done is `held`, which is dropped and made anew below.
        let done = panic::catch_unwind(AssertUnwindSafe(|| seat.look(&mut held, &mut epoch, now)));
        if seat.lease.abandoned() {
            return;
        }
        if done.is_err() {
            held = Held::default();
            which.flowing(shared).store(false, Ordering::Relaxed);
            seat.hand(None);
            if panicked.is_some_and(|first| now.saturating_duration_since(first) < RESTART_WITHIN) {
                report::note!(
                    error,
                    "a voice device's keeper panicked again; what is asked of it is refused until it changes",
                    bound_ms = report::millis(RESTART_WITHIN),
                );
                let asks = which.asked(&lock(&shared.state)).asks;
                held.tried = Some(asks);
                shared.tell(which, asks, Standing::Refused(DevicesRefused::Unavailable));
            } else {
                report::note!(error, "a voice device's keeper panicked; its device is dropped and opened again");
                panicked = Some(now);
            }
        }
        stage.finished();
    }
}

/// A keeper's thread, and whether it has ended.
struct Live {
    lease: Arc<Lease>,
    thread: JoinHandle<()>,
    /// Closed by the thread as it ends.
    done: mpsc::Receiver<()>,
}

/// One device's keeper: the thread now looking after it, and the one given up on, if it has not ended.
pub(crate) struct Station<D: Device + ?Sized> {
    shared: Arc<Shared>,
    mix: Thread,
    ends: mpsc::Sender<Option<Box<D::End>>>,
    live: Option<Live>,
    abandoned: Option<JoinHandle<()>>,
}

impl<D: Device + ?Sized + 'static> Station<D> {
    pub(crate) fn new(shared: Arc<Shared>, mix: Thread, ends: mpsc::Sender<Option<Box<D::End>>>) -> Self {
        Self { shared, mix, ends, live: None, abandoned: None }
    }

    /// Starts a keeper for the device.
    pub(crate) fn start(&mut self) -> Result<(), DevicesRefused> {
        let lease = Arc::new(Lease::default());
        let (done, finished) = mpsc::channel();
        let (shared, mix, ends, running) =
            (Arc::clone(&self.shared), self.mix.clone(), self.ends.clone(), Arc::clone(&lease));
        let thread = thread::Builder::new()
            .name(D::WHICH.keeper().into())
            .spawn(move || {
                let _done = done;
                run::<D>(&Seat { shared: &shared, lease: &running, mix: &mix, ends: &ends });
            })
            .map_err(|_| DevicesRefused::Unavailable)?;
        self.live = Some(Live { lease, thread, done: finished });
        Ok(())
    }

    /// Wakes the keeper, as an ask changed.
    pub(crate) fn wake(&self) {
        if let Some(live) = &self.live {
            live.thread.thread().unpark();
        }
    }

    /// Replaces the keeper if it is stuck at `now`; how long until it next could be.
    pub(crate) fn watch(&mut self, now: Instant) -> Duration {
        self.replace_if_stuck(now);
        let bound = self.shared.give_up_after;
        D::WHICH.stage(&self.shared).left(now, bound).unwrap_or(bound)
    }

    /// A keeper that has been in one call for the chain's bound at `now` is said unanswered, given up on, and another
    /// started to open what is asked; while an earlier one given up on has not ended it is said hung and kept, so a
    /// device whose calls never return does not gather threads.
    fn replace_if_stuck(&mut self, now: Instant) {
        let (shared, which) = (&*self.shared, D::WHICH);
        let stage = which.stage(shared);
        if !stage.stuck(now, shared.give_up_after) {
            return;
        }
        let asks = which.asked(&lock(&shared.state)).asks;
        if self.abandoned.as_ref().is_some_and(|gone| !gone.is_finished()) {
            shared.tell(which, asks, Standing::Hung);
            return;
        }
        shared.tell(which, asks, Standing::Unanswered);
        let Some(stuck) = self.live.take() else { return };
        stuck.lease.abandon();
        stuck.thread.thread().unpark();
        stage.given_up();
        report::note!(warn, "a voice device's keeper did not return within its bound; a new one takes its place");
        self.abandoned = Some(stuck.thread);
        if self.start().is_err() {
            report::note!(error, "a voice device's keeper could not be started in place of the one given up on");
        }
    }

    /// Waits for the keeper to end until `deadline`; one that does not is left behind.
    pub(crate) fn stop(&mut self, deadline: Instant) {
        let Some(live) = self.live.take() else { return };
        live.thread.thread().unpark();
        match live.done.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = live.thread.join();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let doing = D::WHICH.stage(&self.shared).health().doing;
                report::note!(
                    warn,
                    "a voice device's keeper was still running after it was stopped; it is left behind",
                    doing = doing,
                    bound_ms = report::millis(STOP_BOUND),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keeper_s_step_is_read_back_as_it_was_set() {
        for step in Keeping::ALL {
            assert_eq!(Keeping::of(step.code()), step);
        }
    }
}
