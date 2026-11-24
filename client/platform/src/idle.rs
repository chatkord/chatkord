//! Whether the person is at the machine: told as transitions, [`State::Idle`] after no input for [`IDLE_AFTER`] and
//! [`State::Active`] on the first input after. Where the system tells it (Wayland's `ext-idle-notify-v1`) the
//! transition is exact; elsewhere the idle time is asked every [`POLL_EVERY`], so a transition is told up to that late.
//! Nothing watches until the caller starts it, and dropping the [`Watch`] stops it.

#[cfg(target_os = "linux")]
mod mutter;
mod poll;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

use crate::Unavailable;
use std::time::Duration;
use tokio::sync::mpsc;

pub use screens::IDLE_AFTER;
/// How often the idle time is asked where the system does not tell it: input does not wake the program, so this is
/// the cost of watching.
pub const POLL_EVERY: Duration = Duration::from_secs(30);

/// Whether the person has used the machine within [`IDLE_AFTER`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum State {
    /// No input for [`IDLE_AFTER`].
    Idle,
    /// Input since.
    Active,
}

/// The state `idle_for` of no input moves `current` to, or `None` where it stays.
#[must_use]
pub fn moved(current: State, idle_for: Duration) -> Option<State> {
    match current {
        State::Active if idle_for >= IDLE_AFTER => Some(State::Idle),
        State::Idle if idle_for < IDLE_AFTER => Some(State::Active),
        State::Active | State::Idle => None,
    }
}

/// The sources a Linux session may answer by.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `ext-idle-notify-v1` on the compositor's socket.
    Wayland,
    /// GNOME's shell, over the session bus.
    Mutter,
    /// The X server's screen-saver extension.
    X11,
}

/// The sources to try, in order, for a session that names a Wayland display or an X display. The shell's answer needs
/// neither: a window started without the compositor's name still has the bus.
#[cfg(target_os = "linux")]
fn kinds(wayland: bool, x11: bool) -> Vec<Kind> {
    let mut kinds = Vec::new();
    if wayland {
        kinds.push(Kind::Wayland);
    }
    kinds.push(Kind::Mutter);
    if x11 {
        kinds.push(Kind::X11);
    }
    kinds
}

/// Where the transitions come from on this system.
enum Feed {
    #[cfg(target_os = "linux")]
    Wayland(wayland::Feed),
    Poll(poll::Poll),
}

impl Feed {
    /// The best source this session offers, or why there is none.
    async fn open() -> Result<Self, Unavailable> {
        #[cfg(target_os = "linux")]
        {
            let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
            for kind in kinds(wayland, std::env::var_os("DISPLAY").is_some()) {
                let opened = match kind {
                    Kind::Wayland => wayland::Feed::open().await.map(Feed::Wayland),
                    Kind::Mutter => poll::Poll::mutter().await.map(Feed::Poll),
                    Kind::X11 => poll::Poll::x11().await.map(Feed::Poll),
                };
                if let Ok(feed) = opened {
                    return Ok(feed);
                }
            }
            Err(Unavailable)
        }
        #[cfg(windows)]
        {
            poll::Poll::windows().await.map(Feed::Poll)
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            Err(Unavailable)
        }
    }

    /// The next transition, or `None` when the source has gone for good.
    async fn next(&mut self) -> Option<State> {
        match self {
            #[cfg(target_os = "linux")]
            Feed::Wayland(feed) => feed.next().await,
            Feed::Poll(poll) => poll.next().await,
        }
    }
}

/// The watching, until this is dropped.
pub struct Watch {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Starts watching, sending what `wrap` makes of each transition to `to` until `to` is closed or the [`Watch`] dropped.
/// The person is taken to be active when it starts. A session that tells no idle time (no protocol, no screen-saver
/// extension, no shell that answers) or a system with none here yet is [`Unavailable`].
pub async fn start<T: Send + 'static>(
    to: mpsc::Sender<T>,
    wrap: impl Fn(State) -> T + Send + 'static,
) -> Result<Watch, Unavailable> {
    let mut feed = Feed::open().await?;
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                state = feed.next() => {
                    let Some(state) = state else { break };
                    if to.send(wrap(state)).await.is_err() {
                        break;
                    }
                }
                () = to.closed() => break,
            }
        }
    });
    Ok(Watch { task })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn an_active_person_goes_idle_at_the_bound_and_not_before() {
        assert_eq!(moved(State::Active, IDLE_AFTER - SECOND), None);
        assert_eq!(moved(State::Active, IDLE_AFTER), Some(State::Idle));
    }

    #[test]
    fn an_idle_person_is_active_on_input_and_not_before() {
        assert_eq!(moved(State::Idle, IDLE_AFTER), None);
        assert_eq!(moved(State::Idle, IDLE_AFTER - SECOND), Some(State::Active));
        assert_eq!(moved(State::Idle, Duration::ZERO), Some(State::Active));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_source_that_fails_leaves_the_next_the_session_may_offer() {
        // GNOME answers by its shell whichever display server it runs; Xwayland has no screen-saver extension.
        assert_eq!(kinds(true, true), [Kind::Wayland, Kind::Mutter, Kind::X11]);
        assert_eq!(kinds(false, true), [Kind::Mutter, Kind::X11]);
        assert_eq!(kinds(true, false), [Kind::Wayland, Kind::Mutter]);
        assert_eq!(kinds(false, false), [Kind::Mutter]);
    }

    #[test]
    fn a_state_that_holds_is_not_told_again() {
        assert_eq!(moved(State::Idle, IDLE_AFTER * 5), None);
        assert_eq!(moved(State::Active, Duration::ZERO), None);
    }
}
