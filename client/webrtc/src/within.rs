//! The bound on every call into the engine that is awaited: one that never answers is refused by its name, so
//! nothing that waits on the engine waits for ever.

use crate::voice::VoiceRefused;
use std::fmt;
use std::time::Duration;
use tokio::time::{Instant, timeout};

/// The longest an awaited engine call may take. A bound, not a measurement: it is chosen well above what a call
/// is expected to take, until each step's time has been measured from the log a bounded call writes.
pub const ENGINE_WITHIN: Duration = Duration::from_secs(5);
/// How long the engine may take to hand over a track after the server resumes it.
const ARRIVAL: Duration = Duration::from_secs(10);

/// Which of a call's two connections a step ran on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    /// The connection the member sends over.
    Send,
    /// The connection the member receives over.
    Receive,
}

/// An awaited step of the engine, each bounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Step {
    /// The engine writes the send connection's next offer.
    CreateOffer,
    /// The engine answers the receive connection's offer.
    CreateAnswer,
    /// The engine takes its own description: the offer when sending, the answer when receiving.
    SetLocal(Half),
    /// The engine takes the description made in the server's name: the answer when sending, the offer when
    /// receiving.
    SetRemote(Half),
    /// The engine reports a connection's statistics.
    Statistics(Half),
    /// The engine hands over a track the server resumed.
    Arrival,
}

impl Step {
    /// The longest this step may take.
    pub fn bound(self) -> Duration {
        match self {
            Self::CreateOffer | Self::CreateAnswer | Self::SetLocal(_) | Self::SetRemote(_) | Self::Statistics(_) => {
                ENGINE_WITHIN
            }
            Self::Arrival => ARRIVAL,
        }
    }
}

impl fmt::Display for Half {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Send => "send",
            Self::Receive => "receive",
        })
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateOffer => f.write_str("creating the send offer"),
            Self::CreateAnswer => f.write_str("creating the receive answer"),
            Self::SetLocal(half) => write!(f, "setting the {half} connection's local description"),
            Self::SetRemote(half) => write!(f, "setting the {half} connection's remote description"),
            Self::Statistics(half) => write!(f, "reading the {half} connection's statistics"),
            Self::Arrival => f.write_str("waiting for a resumed track to arrive"),
        }
    }
}

/// Awaits `call` no longer than `step`'s bound, and logs how long it took so each step's time can be measured.
pub(crate) async fn within<T>(step: Step, call: impl Future<Output = T>) -> Result<T, VoiceRefused> {
    let began = Instant::now();
    let done = timeout(step.bound(), call).await;
    let elapsed_ms = report::millis(began.elapsed());
    report::note!(debug, "engine call", step = step, elapsed_ms = elapsed_ms, stalled = done.is_err());
    done.map_err(|_| VoiceRefused::Stalled(step))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn an_engine_call_that_never_answers_is_refused_by_its_step_at_its_bound() {
        let began = Instant::now();
        for step in [Step::CreateOffer, Step::Statistics(Half::Receive), Step::Arrival] {
            let asked = Instant::now();
            let refused = within(step, std::future::pending::<()>()).await;
            assert!(matches!(refused, Err(VoiceRefused::Stalled(stalled)) if stalled == step), "{step}");
            assert_eq!(asked.elapsed(), step.bound(), "{step}: at its bound, on the paused clock");
        }
        assert_eq!(began.elapsed(), ENGINE_WITHIN * 2 + ARRIVAL);
        assert!(matches!(within(Step::CreateAnswer, async { 7 }).await, Ok(7)), "one that answers is handed back");
    }
}
