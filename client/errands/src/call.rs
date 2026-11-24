//! One call, run on the runner: what a person's controls reach it through, and how it reports to the window and
//! paints into the host's slots.

use crate::Kept;
use crate::host::Host;
use crate::mailbox::Mailbox;
use link::Events;
use screens::{CallReturn, End, PlaceId, PlaceMessage, RoomEvent, WindowMessage};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use terms::Heard;
use tokio::sync::{mpsc, watch};

/// How often the loudness of the people heard is read, so a tile's ring follows the voice.
pub(crate) const LEVEL_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// The call being run: what the person wants of it, where to send what they ask of it, and how to leave it.
pub(crate) type Running = Mutex<Option<(PlaceId, Asks, call::room::Leaving)>>;

/// What a person's controls reach a running call through: what they want, kept whole, and what they ask done.
#[derive(Clone)]
pub(crate) struct Asks {
    pub(crate) wants: watch::Sender<call::room::Wants>,
    pub(crate) commands: mpsc::Sender<call::room::Command>,
}

impl Asks {
    /// Sends the call `act`, in order with the acts before it: one that finds the queue full waits its turn on
    /// `runtime`, never dropped.
    pub(crate) fn act(&self, runtime: &tokio::runtime::Runtime, act: call::room::Command) {
        if let Err(mpsc::error::TrySendError::Full(act)) = self.commands.try_send(act) {
            let commands = self.commands.clone();
            crate::errand::spawn(runtime, report::word!("queue a call act"), async move {
                let _ = commands.send(act).await;
            });
        }
    }
}

/// How to reach the call held at `id`, if one is.
pub(crate) fn held_asks(running: &Running, id: PlaceId) -> Option<Asks> {
    running
        .lock()
        .ok()
        .and_then(|running| running.as_ref().filter(|(at, ..)| *at == id).map(|(_, asks, _)| asks.clone()))
}

/// Ends the call held at `id`, if one is; its loop reports how it ended.
pub(crate) fn stop_call(running: &Running, id: PlaceId) {
    if let Some((_, _, leaving)) = running.lock().ok().and_then(|mut running| running.take_if(|(at, ..)| *at == id)) {
        leaving.leave();
    }
}

/// How a call is told what the person wants and asks: what they want, the commands (the sender kept for the devices'
/// watch) and the leave.
pub(crate) struct Controls {
    pub(crate) wanted: watch::Receiver<call::room::Wants>,
    /// The sender of the same, kept to forget what was wanted of a picture that ended.
    pub(crate) wants: watch::Sender<call::room::Wants>,
    pub(crate) commands: mpsc::Sender<call::room::Command>,
    pub(crate) queued: mpsc::Receiver<call::room::Command>,
    pub(crate) leaving: call::room::Leaving,
}

/// Where a call reports to and paints into: the window's mailbox and the host's slots, and the folders it reads the
/// person's choices from.
pub(crate) struct Outlet<H> {
    pub(crate) mailbox: Mailbox,
    pub(crate) host: Arc<H>,
    pub(crate) platform: platform::Platform,
}

/// One call, from the engine to the end of its loop. The connection's events are lent to it and given back when it ends,
/// so the window is told them again.
pub(crate) async fn run_call<H: Host>(
    id: PlaceId,
    channel_id: terms::ChannelId,
    engines: &call::Engines,
    kept: &Mutex<Option<Kept>>,
    controls: Controls,
    outlet: Outlet<H>,
) -> CallReturn {
    let Outlet { mailbox, host, platform } = outlet;
    let Controls { wanted, wants, commands, queued, leaving } = controls;
    let tell = move |message: PlaceMessage| WindowMessage::At(id, message);
    let engine = match engines.engine().await {
        Ok(engine) => engine,
        Err(refused) => return CallReturn::Stopped(Some(End::Failed(call::failure::voice(refused)))),
    };
    let signed = kept.lock().ok().and_then(|kept| kept.clone());
    let lent = match &signed {
        Some(signed) => signed.lend().await,
        None => None,
    };
    let (Some(signed), Some(mut lent)) = (signed, lent) else {
        return CallReturn::Stopped(Some(End::Unreached(terms::LinkError::Offline)));
    };
    // The call's events are told without waiting (the loop that reads them reads the connection too); a window that
    // far behind ends the call rather than miss what the room became.
    let fell_behind = AtomicBool::new(false);
    let ended: Mutex<Option<End>> = Mutex::new(None);
    // A device that comes or goes is the call's to settle; a device that cannot say so is not told.
    if let Ok(changes) = sound::Sound::shared().watch().await {
        changes.forward(commands.clone(), || call::room::Command::DevicesChanged);
    }
    let chosen = platform::voice::choice(&platform).await;
    let join = call::room::Join {
        channel_id,
        me: crate::signin::me(&signed.ack),
        source: call::room::Source::Microphone,
        microphone: chosen.microphone,
        speaker: chosen.speaker,
        processing: chosen.processing,
        level_every: LEVEL_EVERY,
        screens: Some({
            let host = Arc::clone(&host);
            call::room::Screens::new(move |user_id, video| host.slot(user_id, video))
        }),
        own_screen: Some(host.own_screen()),
        own_camera: Some(host.own_camera()),
        painting: Some({
            let host = Arc::clone(&host);
            call::room::Paints::new(move || host.painting())
        }),
        jitter: crate::random::jitter(),
        encoders: engines.encoders(),
        chain: engines.chain_watch(),
    };
    let asked = (wanted, queued);
    let outcome = call::room::run(&signed.link, &engine, &mut lent.events, join, asked, leaving.clone(), |report| {
        let told = match report {
            call::room::Report::Room(event) => {
                if let RoomEvent::Ended(end) = &event
                    && let Ok(mut kept) = ended.lock()
                {
                    *kept = Some(end.clone());
                }
                PlaceMessage::Call(screens::CallMessage::Room(event))
            }
            call::room::Report::Heard(event) => match PlaceMessage::heard(&event) {
                Some(heard) => heard,
                None => return,
            },
            call::room::Report::Notice(subject, said) => {
                PlaceMessage::Call(screens::CallMessage::Notice(subject, said))
            }
            call::room::Report::Sound(sound) => PlaceMessage::Call(screens::CallMessage::Sound(sound)),
            call::room::Report::Connection(health) => PlaceMessage::Call(screens::CallMessage::Health(health)),
            call::room::Report::Share(shared) => PlaceMessage::Call(screens::CallMessage::Shared(shared)),
            call::room::Report::Watch(watched) => {
                if let screens::Watched::Ended { user_id, video } = &watched {
                    wants.send_if_modified(|wants| wants.ended(user_id, *video));
                }
                PlaceMessage::Call(screens::CallMessage::Watched(watched))
            }
            call::room::Report::OwnWatched { video, watched } => {
                PlaceMessage::Call(screens::CallMessage::OwnWatched { video, watched })
            }
            call::room::Report::Camera(sent) => PlaceMessage::Call(screens::CallMessage::Camera(sent)),
            call::room::Report::Behind => PlaceMessage::Session(screens::SessionMessage::Behind),
            call::room::Report::Answered(subject) => PlaceMessage::Call(screens::CallMessage::Answered(subject)),
            call::room::Report::Said(_) | call::room::Report::Left(_) => return,
        };
        if !mailbox.try_tell(tell(told)) {
            fell_behind.store(true, Ordering::Relaxed);
            leaving.leave();
        }
    })
    .await;
    host.clear_slots();
    // A connection that ended during the call is not given back: the window makes it again, with the way the call
    // ended.
    let link = signed.link.clone();
    // Dropped first, so a listener this connection was replaced in meanwhile ends rather than tell what follows.
    drop(signed);
    if let Err(call::room::Stopped::Fault) = outcome {
        // What the failed step held is unknown, the server's place in voice among it: the connection goes with it.
        link.part().await;
        return CallReturn::Faulted;
    }
    if link.is_closed() {
        let ended = match outcome {
            Err(call::room::Stopped::Disconnected(ended)) => ended,
            Err(call::room::Stopped::Ended(_) | call::room::Stopped::Fault) | Ok(()) => how_ended(lent.events).await,
        };
        return CallReturn::Disconnected(ended);
    }
    let _ = lent.back.send(lent.events);
    match outcome {
        Ok(()) if fell_behind.load(Ordering::Relaxed) => CallReturn::FellBehind,
        Ok(()) => CallReturn::Returned,
        // The loop said how it ended, as the room's own event; this one cannot be dropped as full.
        Err(_) => CallReturn::Stopped(ended.lock().ok().and_then(|mut kept| kept.take())),
    }
}

/// Runs `call` as a task of its own to its end. One that panicked took the connection's lent events with it, so
/// `link` is closed rather than kept with no listener, and the window says so and makes it again.
pub(crate) async fn kept_call<H: Host>(
    call: impl Future<Output = CallReturn> + Send + 'static,
    host: &H,
    link: Option<link::Link>,
) -> CallReturn {
    match crate::errand::spawn_here(report::word!("run a call"), call).await {
        Ok(finished) => finished,
        Err(_) => {
            host.clear_slots();
            if let Some(link) = link {
                link.part().await;
            }
            CallReturn::Faulted
        }
    }
}

/// How a connection ended, read from the end of its events.
async fn how_ended(mut events: Events) -> terms::Ended {
    while let Some(heard) = events.next().await {
        if let Heard::Ended(ended) = heard {
            return ended;
        }
    }
    terms::Ended::default()
}
