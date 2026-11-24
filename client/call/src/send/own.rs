//! This account's own screen as a thing sent: begun from the picker's pick, sent over the call's voice, said as a
//! share.

use super::sender::{End, Kind, start_within, stop_within};
use crate::Slot;
use crate::broadcast::{self, Begun, Broadcast, SoundAsked};
use crate::room::lane::Flight;
use crate::screen::Pick;
use choices::ShareQuality;
use link::Link;
use screens::Shared;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::task::JoinHandle;
use words::Failure;

/// A screen the person picked, as they asked for it.
pub(crate) struct Asked {
    pub(crate) pick: Pick,
    pub(crate) quality: ShareQuality,
    pub(crate) sound: SoundAsked,
}

/// The screen: the survey its encoder comes from, where it is lent for its card, and the link it is sent over.
pub(crate) struct Screen {
    pub(crate) encoders: Arc<media::Survey>,
    pub(crate) own: Option<Slot>,
    pub(crate) link: Link,
}

impl Kind for Screen {
    type Ask = Asked;
    type Begun = Begun;
    type Live = Broadcast;
    type Said = Shared;

    const WHAT: &'static str = "the screen";

    // Its picture and its sound are two tracks.
    fn send_within() -> Duration {
        start_within() * 2
    }

    fn stop_within() -> Duration {
        stop_within() * 2
    }

    /// A share reads whether anyone takes it once it is sent, from [`super::sender::Sender::watched_now`].
    fn begin(&self, ask: Asked, _: Arc<AtomicBool>) -> JoinHandle<Result<Begun, Failure>> {
        tokio::spawn(broadcast::begin(ask.pick, ask.quality, ask.sound, Arc::clone(&self.encoders)))
    }

    fn let_go(ask: Asked) {
        if let Pick::Portal(selection) = ask.pick {
            drop(tokio::spawn(selection.close()));
        }
    }

    fn abandon(begun: Begun) -> Flight<()> {
        Box::pin(begun.abandon())
    }

    fn stop(&self, live: Broadcast) -> Flight<()> {
        let link = self.link.clone();
        Box::pin(async move { live.stop(&link).await })
    }

    fn ended(live: &mut Broadcast) -> impl Future<Output = Failure> + Send {
        live.finished()
    }

    fn close(&self, live: Broadcast) -> Flight<()> {
        let (ended, link) = (live.ended(), self.link.clone());
        Box::pin(async move { ended.close(&link).await })
    }

    fn said(end: End) -> Shared {
        match end {
            End::Stopped => Shared::Stopped,
            End::Denied => Shared::Denied,
            End::Failed(failed) => Shared::Failed(failed),
        }
    }

    /// Its card lets go of the compositor's buffer.
    fn emptied(&self) {
        if let Some(own) = &self.own {
            own.empty();
        }
    }
}
