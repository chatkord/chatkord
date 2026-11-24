//! A system with no capture to send from yet: `begin` refuses, so no share it would hold can exist.

use crate::Slot;
use crate::screen::Pick;
use choices::ShareQuality;
use link::Link;
use screens::{Outgoing, SoundSaid};
use std::convert::Infallible;
use webrtc::Voice;

/// What a share's sound is asked to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundAsked {
    /// What was picked has.
    Follow,
}

/// A screen ready to send; none can exist here.
pub struct Begun {
    never: Infallible,
}

/// A screen being sent; none can exist here.
pub struct Broadcast {
    pub sound_refused: Option<words::Failure>,
    pub sound_said: Option<SoundSaid>,
    pub width: u32,
    pub height: u32,
    pub encoder: screens::Route,
    never: Infallible,
}

/// A share that ended by itself; none can exist here.
pub struct Ended {
    never: Infallible,
}

/// Refuses: this system has no capture yet. A pick the picker gave is given back.
pub async fn begin(
    pick: Pick,
    _: ShareQuality,
    _: SoundAsked,
    _: std::sync::Arc<media::Survey>,
) -> Result<Begun, words::Failure> {
    if let Pick::Portal(selection) = pick {
        selection.close().await;
    }
    Err(crate::failure::picked(::share::Refused::Unavailable))
}

impl Begun {
    pub async fn abandon(self) {
        match self.never {}
    }

    pub async fn send(self, _: &Voice, _: &Link, _: Option<Slot>) -> Result<Broadcast, words::Failure> {
        match self.never {}
    }
}

impl Ended {
    pub async fn close(self, _: &Link) -> words::Failure {
        match self.never {}
    }
}

impl Broadcast {
    /// Never resolves: nothing is ever sent here.
    pub async fn ended(this: &mut Option<Self>) -> Ended {
        match this {
            Some(broadcast) => match broadcast.never {},
            None => std::future::pending().await,
        }
    }

    pub fn look_ask(&self) -> (u64, std::future::Ready<webrtc::SendStats>) {
        match self.never {}
    }

    pub fn looked(&mut self, _of: u64, _engine: webrtc::SendStats, _watched: bool) -> Option<Outgoing> {
        match self.never {}
    }

    pub fn first_unshown(&mut self) -> bool {
        match self.never {}
    }

    pub fn handed_over(&mut self) -> Option<screens::Route> {
        match self.never {}
    }

    pub async fn stop(self, _: &Link) {
        match self.never {}
    }
}
