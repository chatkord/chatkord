//! What the window's other threads tell it, drained even while hidden: a bounded queue, and each loudness kept apart,
//! newest only, since it is told ten times a second and only the last is worth showing.

use std::sync::{Arc, Mutex};

use screens::{PlaceId, PlaceMessage, ProbeHeard, RoomEvent, WindowMessage};
use terms::MemberId;
use tokio::sync::mpsc;

/// Wakes the host's loop to drain what it was told; called from any thread, so it must not block.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// The most messages the window may be behind by. A call that would pass it is left, as a connection whose own queue
/// is full is.
pub const MAILBOX: usize = 256;

/// The loudness last told, and the server whose call it is.
type Levels = (PlaceId, Vec<(MemberId, u8)>);

/// The telling side, for the threads that carry out what the screens ask.
#[derive(Clone)]
pub struct Mailbox {
    queue: mpsc::Sender<WindowMessage>,
    levels: Arc<Mutex<Option<Levels>>>,
    probe_level: Arc<Mutex<Option<i16>>>,
    wake: Wake,
}

/// The window's side.
pub struct Inbox {
    queue: mpsc::Receiver<WindowMessage>,
    levels: Arc<Mutex<Option<Levels>>>,
    probe_level: Arc<Mutex<Option<i16>>>,
}

/// A mailbox whose every message wakes the window with `wake`.
pub fn mailbox(wake: Wake) -> (Mailbox, Inbox) {
    let (queue, received) = mpsc::channel(MAILBOX);
    let levels = Arc::default();
    let probe_level = Arc::default();
    (
        Mailbox { queue, levels: Arc::clone(&levels), probe_level: Arc::clone(&probe_level), wake },
        Inbox { queue: received, levels, probe_level },
    )
}

impl Mailbox {
    /// Tells the window, waiting while it is [`MAILBOX`] messages behind.
    pub async fn tell(&self, message: WindowMessage) {
        if let Some(message) = self.keep_levels(message) {
            // A window that is gone has nobody left to tell.
            let _ = self.queue.send(message).await;
        }
        (self.wake)();
    }

    /// Tells the window without waiting; false when it is [`MAILBOX`] messages behind and nothing was told.
    pub fn try_tell(&self, message: WindowMessage) -> bool {
        let told = match self.keep_levels(message) {
            None => true,
            Some(message) => !matches!(self.queue.try_send(message), Err(mpsc::error::TrySendError::Full(_))),
        };
        (self.wake)();
        told
    }

    /// Keeps the newest loudness in its own place; anything else is for the queue.
    fn keep_levels(&self, message: WindowMessage) -> Option<WindowMessage> {
        match message {
            WindowMessage::Probed(ProbeHeard::Loudness(level)) => {
                if let Ok(mut newest) = self.probe_level.lock() {
                    *newest = Some(level);
                }
                None
            }
            WindowMessage::At(id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Levels(levels)))) => {
                if let Ok(mut newest) = self.levels.lock() {
                    *newest = Some((id, levels));
                }
                None
            }
            other => Some(other),
        }
    }
}

impl Inbox {
    /// Everything told since the last drain: the newest loudness first, then the queue in the order it was told, then
    /// the microphone's level (after the queue, so a probe that ended in it has no level after its end).
    pub fn drain(&mut self) -> Vec<WindowMessage> {
        let levels = self.levels.lock().ok().and_then(|mut newest| newest.take());
        let mut told: Vec<WindowMessage> = levels
            .map(|(id, levels)| {
                WindowMessage::At(id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Levels(levels))))
            })
            .into_iter()
            .collect();
        while let Ok(message) = self.queue.try_recv() {
            told.push(message);
        }
        let heard = self.probe_level.lock().ok().and_then(|mut newest| newest.take());
        told.extend(heard.map(|level| WindowMessage::Probed(ProbeHeard::Loudness(level))));
        told
    }
}

#[cfg(test)]
mod tests {
    use super::{MAILBOX, mailbox};
    use screens::{Opening, PlaceId, PlaceMessage, ProbeHeard, RoomEvent, Server, Window, WindowMessage};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use terms::{MemberId, ServerUrl};

    const ADA: &str = "ada";

    fn ada() -> MemberId {
        MemberId::parse(ADA).expect("a member id")
    }

    /// The id a window gives its one server.
    fn place() -> PlaceId {
        let server = Server {
            url: ServerUrl::parse("wss://example.invalid/v1").unwrap(),
            name: None,
            colour: None,
            picture: None,
        };
        Window::launch(vec![Opening::Server(server)], None, Instant::now()).0.places()[0].id()
    }

    fn at(message: PlaceMessage) -> WindowMessage {
        WindowMessage::At(place(), message)
    }

    fn levels(level: u8) -> WindowMessage {
        at(PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Levels(vec![(ada(), level)]))))
    }

    #[test]
    fn loudness_is_kept_newest_only_and_comes_before_the_queue_in_order() {
        let woken = Arc::new(AtomicUsize::new(0));
        let (told, mut inbox) = mailbox({
            let woken = Arc::clone(&woken);
            Arc::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            })
        });
        for level in 0..50 {
            assert!(told.try_tell(levels(level)));
        }
        assert!(told.try_tell(at(PlaceMessage::Call(screens::CallMessage::CallOver(None)))));
        assert!(
            told.try_tell(at(PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::MemberLeft { user_id: ada() }))))
        );
        let drained = inbox.drain();
        assert_eq!(drained.len(), 3, "fifty loudness reports are one: {drained:?}");
        assert!(
            matches!(&drained[0], WindowMessage::At(id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Levels(heard)))) if *id == place() && heard[..] == [(ada(), 49)])
        );
        assert!(matches!(drained[1], WindowMessage::At(_, PlaceMessage::Call(screens::CallMessage::CallOver(None)))));
        assert!(
            matches!(&drained[2], WindowMessage::At(_, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::MemberLeft { user_id }))) if user_id.as_str() == ADA)
        );
        assert_eq!(woken.load(Ordering::Relaxed), 52, "every message wakes the window");
        assert!(inbox.drain().is_empty());
    }

    #[test]
    fn the_microphones_level_is_kept_newest_only_and_comes_after_the_queue() {
        let (told, mut inbox) = mailbox(Arc::new(|| {}));
        for level in -70..-20 {
            assert!(told.try_tell(WindowMessage::Probed(ProbeHeard::Loudness(level))));
        }
        assert!(told.try_tell(WindowMessage::Probed(ProbeHeard::Failed(words::Failure::VoiceEngine))));
        let drained = inbox.drain();
        assert_eq!(drained.len(), 2, "fifty readings are one: {drained:?}");
        assert!(matches!(&drained[0], WindowMessage::Probed(ProbeHeard::Failed(words::Failure::VoiceEngine))));
        assert!(matches!(drained[1], WindowMessage::Probed(ProbeHeard::Loudness(-21))));
        assert!(inbox.drain().is_empty());
    }

    #[test]
    fn a_window_that_far_behind_is_told_nothing_more_without_waiting() {
        let (told, mut inbox) = mailbox(Arc::new(|| {}));
        for _ in 0..MAILBOX {
            assert!(told.try_tell(at(PlaceMessage::Call(screens::CallMessage::CallOver(None)))));
        }
        assert!(!told.try_tell(at(PlaceMessage::Call(screens::CallMessage::CallOver(None)))), "the queue is full");
        assert!(told.try_tell(levels(1)), "loudness has its own place and never fills the queue");
        assert_eq!(inbox.drain().len(), MAILBOX + 1);
        assert!(
            told.try_tell(at(PlaceMessage::Call(screens::CallMessage::CallOver(None)))),
            "a drained queue takes messages again"
        );
    }
}
