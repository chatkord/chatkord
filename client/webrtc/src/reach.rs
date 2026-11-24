//! Whether a connection of the call carries media, as the engine says while it runs.

use libwebrtc::peer_connection::{PeerConnection, PeerConnectionState};
use tokio::sync::watch;

/// Whether a connection carries media: every stream sent or received goes over one of the call's two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Not connected yet: nothing was sent or taken over it, or its first checks still run.
    Starting,
    /// Connected.
    Up,
    /// The engine lost the route and is trying it again.
    Down,
    /// The engine gave up the route; only an ICE restart brings it back.
    Lost,
}

impl Reach {
    /// What the engine's connection state says of the route; none where it says nothing new.
    fn of(state: PeerConnectionState) -> Option<Reach> {
        match state {
            PeerConnectionState::Connected => Some(Reach::Up),
            PeerConnectionState::Disconnected => Some(Reach::Down),
            PeerConnectionState::Failed => Some(Reach::Lost),
            // Our own teardown is `Closed`, the first two come before the first connection, which stays
            // `Starting` through them, and a restart's checks are `Connecting` until the route is back.
            PeerConnectionState::New | PeerConnectionState::Connecting | PeerConnectionState::Closed => None,
        }
    }
}

/// The reach of `connection` from now on, as its state changes: [`Reach::Starting`] until the engine first says it
/// is connected.
pub(crate) fn watched(connection: &PeerConnection) -> watch::Receiver<Reach> {
    let (reached, reach) = watch::channel(Reach::Starting);
    connection.on_connection_state_change(Some(Box::new(move |state| {
        if let Some(now) = Reach::of(state) {
            reached.send_replace(now);
        }
    })));
    reach
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_is_down_while_the_engine_retries_it_and_lost_when_it_gives_up() {
        assert_eq!(Reach::of(PeerConnectionState::Connected), Some(Reach::Up));
        assert_eq!(Reach::of(PeerConnectionState::Disconnected), Some(Reach::Down));
        assert_eq!(Reach::of(PeerConnectionState::Failed), Some(Reach::Lost));
        for quiet in [PeerConnectionState::New, PeerConnectionState::Connecting, PeerConnectionState::Closed] {
            assert_eq!(
                Reach::of(quiet),
                None,
                "{quiet:?}: a connection not yet up stays starting, one lost stays lost"
            );
        }
    }
}
