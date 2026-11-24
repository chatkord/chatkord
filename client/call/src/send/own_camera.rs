//! This account's own camera as a thing sent: opened from the person's pick, sent over the call's voice, said as a
//! camera.

use super::sender::{End, Kind, start_within, stop_within};
use crate::camera::{self, Begun, Live, Pick};
use crate::room::lane::Flight;
use link::Link;
use picture::Slot;
use screens::CameraSent;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::task::JoinHandle;
use words::Failure;

/// The camera: the survey its encoder comes from, where its pictures are put for one's own card, and the link it is
/// sent over.
pub(crate) struct Camera {
    pub(crate) encoders: Arc<media::Survey>,
    pub(crate) own: Option<Slot>,
    pub(crate) link: Link,
}

impl Kind for Camera {
    type Ask = Pick;
    type Begun = Begun;
    type Live = Live;
    type Said = CameraSent;

    const WHAT: &'static str = "the camera";

    fn send_within() -> Duration {
        start_within()
    }

    fn stop_within() -> Duration {
        stop_within()
    }

    fn begin(&self, pick: Pick, watched: Arc<AtomicBool>) -> JoinHandle<Result<Begun, Failure>> {
        tokio::spawn(camera::begin(pick, self.own.clone(), watched, Arc::clone(&self.encoders)))
    }

    fn let_go(pick: Pick) {
        drop(pick);
    }

    fn abandon(begun: Begun) -> Flight<()> {
        Box::pin(begun.abandon())
    }

    fn stop(&self, live: Live) -> Flight<()> {
        let link = self.link.clone();
        Box::pin(async move { live.stop(&link).await })
    }

    fn ended(live: &mut Live) -> impl Future<Output = Failure> + Send {
        live.ended()
    }

    fn close(&self, live: Live) -> Flight<()> {
        let link = self.link.clone();
        Box::pin(async move { live.close(&link).await })
    }

    fn said(end: End) -> CameraSent {
        match end {
            End::Stopped => CameraSent::Stopped,
            End::Denied => CameraSent::Denied,
            End::Failed(failed) => CameraSent::Failed(failed),
        }
    }

    fn emptied(&self) {
        if let Some(own) = &self.own {
            own.empty();
        }
    }
}
