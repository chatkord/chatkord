//! A lane that never connects ends the call: each of the call's two connections is timed from its first ask to
//! connect until the engine first says it is up, and the call ends with where the server said its media was when
//! the bound passes, or when the server says it could not reach this end either. A lane never asked is never timed.

use proto::chatkord_v1::CloseReason;
use proto::envelope::MEDIA_CONNECT_SECONDS;
use std::time::Duration;
use tokio::time::Instant;
use webrtc::Reach;
use words::Failure;

/// How long a connection may stay starting after its first ask: the server's bound for the same wait.
const CONNECT_WITHIN: Duration = Duration::from_secs(MEDIA_CONNECT_SECONDS);

/// Which of the call's two connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Which {
    /// The one this account's streams are sent over.
    Send,
    /// The one other members' streams are taken over.
    Receive,
}

/// Where one connection is in its first connecting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Timing {
    /// Nothing was asked of it: it has no DTLS parameters sent, so the server waits for none.
    #[default]
    Unasked,
    /// Asked at this time, and not yet connected.
    Waiting(Instant),
    /// It connected once: its first connecting is over.
    Connected,
}

impl Timing {
    fn ask(&mut self, now: Instant) {
        if *self == Self::Unasked {
            *self = Self::Waiting(now);
        }
    }

    /// Reads the engine's word on the route at `now`: whether the wait is past its bound.
    fn read(&mut self, reach: Reach, now: Instant) -> bool {
        match (*self, reach) {
            (Self::Waiting(_), Reach::Up) => {
                *self = Self::Connected;
                false
            }
            // The engine gives up a route it never had well before the bound, and a restart cannot find one either.
            (Self::Waiting(asked), Reach::Starting | Reach::Down | Reach::Lost) => {
                now.saturating_duration_since(asked) >= CONNECT_WITHIN
            }
            (Self::Unasked | Self::Connected, _) => false,
        }
    }
}

/// Both connections' first connecting, and the address the server said its media was at.
#[derive(Debug)]
pub(super) struct Unreached {
    host: String,
    port: u16,
    send: Timing,
    receive: Timing,
}

impl Unreached {
    /// Nothing asked yet, the server's media announced at `host` and `port`.
    pub(super) fn new(host: String, port: u16) -> Self {
        Self { host, port, send: Timing::Unasked, receive: Timing::Unasked }
    }

    /// The first ask to connect `lane` came at `now`; later ones change nothing.
    pub(super) fn asked(&mut self, lane: Which, now: Instant) {
        match lane {
            Which::Send => self.send.ask(now),
            Which::Receive => self.receive.ask(now),
        }
    }

    /// What the engine says of both routes at `now`: the failure once a connection has stayed starting past its bound.
    pub(super) fn read(&mut self, send: Reach, receive: Reach, now: Instant) -> Option<Failure> {
        let late = [self.send.read(send, now), self.receive.read(receive, now)];
        late.contains(&true).then(|| self.failure())
    }

    /// The server closed the place for `reason`: when it could not reach this end and a connection was still
    /// starting, the same failure, which names the address a person can act on.
    pub(super) fn closed(&self, reason: Option<CloseReason>) -> Option<Failure> {
        let waiting = [self.send, self.receive].iter().any(|timing| matches!(timing, Timing::Waiting(_)));
        (reason == Some(CloseReason::Unreachable) && waiting).then(|| self.failure())
    }

    fn failure(&self) -> Failure {
        Failure::VoiceUnreached { host: self.host.clone(), port: self.port }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "voice.example.invalid";
    const PORT: u16 = 40_000;

    fn unreached() -> Unreached {
        Unreached::new(HOST.into(), PORT)
    }

    fn said() -> Failure {
        Failure::VoiceUnreached { host: HOST.into(), port: PORT }
    }

    #[tokio::test(start_paused = true)]
    async fn a_lane_still_starting_when_its_deadline_passes_ends_the_call_with_the_address() {
        let mut lanes = unreached();
        let asked = Instant::now();
        lanes.asked(Which::Send, asked);
        let almost = asked + CONNECT_WITHIN - Duration::from_millis(1);
        assert_eq!(lanes.read(Reach::Starting, Reach::Starting, almost), None);
        assert_eq!(lanes.read(Reach::Starting, Reach::Starting, asked + CONNECT_WITHIN), Some(said()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lane_connected_before_its_deadline_never_trips_it() {
        let mut lanes = unreached();
        let asked = Instant::now();
        lanes.asked(Which::Send, asked);
        assert_eq!(lanes.read(Reach::Up, Reach::Starting, asked + Duration::from_secs(1)), None);
        let long_after = asked + CONNECT_WITHIN * 10;
        assert_eq!(lanes.read(Reach::Starting, Reach::Starting, long_after), None, "its first connecting is over");
        assert_eq!(lanes.closed(Some(CloseReason::Unreachable)), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_lane_the_engine_gave_up_before_it_ever_connected_still_trips() {
        let mut lanes = unreached();
        let asked = Instant::now();
        lanes.asked(Which::Send, asked);
        assert_eq!(lanes.read(Reach::Lost, Reach::Starting, asked + Duration::from_secs(8)), None);
        assert_eq!(lanes.read(Reach::Lost, Reach::Starting, asked + CONNECT_WITHIN), Some(said()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lane_never_asked_never_trips() {
        let mut lanes = unreached();
        let long_after = Instant::now() + CONNECT_WITHIN * 10;
        assert_eq!(lanes.read(Reach::Starting, Reach::Starting, long_after), None);
        assert_eq!(lanes.closed(Some(CloseReason::Unreachable)), None, "the server waits for none either");
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_ask_starts_the_clock_and_a_later_one_does_not_move_it() {
        let mut lanes = unreached();
        let asked = Instant::now();
        lanes.asked(Which::Receive, asked);
        lanes.asked(Which::Receive, asked + Duration::from_secs(10));
        assert_eq!(lanes.read(Reach::Starting, Reach::Starting, asked + CONNECT_WITHIN), Some(said()));
    }

    #[tokio::test(start_paused = true)]
    async fn an_unreachable_close_while_a_lane_starts_is_the_same_failure_and_any_other_reason_is_not() {
        let mut lanes = unreached();
        lanes.asked(Which::Send, Instant::now());
        assert_eq!(lanes.closed(Some(CloseReason::Unreachable)), Some(said()));
        assert_eq!(lanes.closed(Some(CloseReason::Moved)), None);
        assert_eq!(lanes.closed(None), None);
    }
}
