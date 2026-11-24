//! What follows a close reason, decided once: retrying, the door, or a call's end. Retrying, the panel and a call's
//! end all read this table, and its words are `words::failure::closed`'s, so a new reason stops the build in both.

use crate::call::room::Why;
use proto::chatkord_v1::CloseReason;

/// What follows a close reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    /// The connection went and the session did not: worth trying again.
    TryAgain,
    /// The session is over; signing in again is the way back.
    SignIn,
    /// The account is gone: the door offers no password.
    AccountGone,
    /// This build cannot speak to the server: its refusal says so, and nothing retries.
    OtherVersion,
    /// The call left voice; the session goes on.
    LeftVoice(Why),
}

impl Then {
    pub(crate) fn of(reason: CloseReason) -> Self {
        match reason {
            CloseReason::SignedOut
            | CloseReason::SessionExpired
            | CloseReason::Kicked
            | CloseReason::PasswordChanged
            | CloseReason::PasswordReset
            | CloseReason::Banned => Then::SignIn,
            CloseReason::AccountDeleted => Then::AccountGone,
            CloseReason::VersionMismatch => Then::OtherVersion,
            CloseReason::Moved => Then::LeftVoice(Why::Moved),
            CloseReason::NotAllowed => Then::LeftVoice(Why::RolesChanged),
            CloseReason::ChannelDeleted => Then::LeftVoice(Why::Deleted),
            CloseReason::TakenOut => Then::LeftVoice(Why::TakenOut),
            CloseReason::OverRate => Then::LeftVoice(Why::Closed),
            CloseReason::Unreachable
            | CloseReason::ClientShutdown
            | CloseReason::ServerStopping
            | CloseReason::Unspecified
            | CloseReason::Normal
            | CloseReason::ServerError
            | CloseReason::FellBehind => Then::TryAgain,
        }
    }

    /// Whether a connection that ended this way is worth signing in to again by itself.
    pub(crate) fn retry(self) -> bool {
        match self {
            Then::TryAgain | Then::LeftVoice(_) => true,
            Then::SignIn | Then::AccountGone | Then::OtherVersion => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_reason() -> impl Iterator<Item = CloseReason> {
        (0..=i32::from(u8::MAX)).filter_map(|n| CloseReason::try_from(n).ok())
    }

    #[test]
    fn a_session_that_is_over_is_always_said() {
        for reason in every_reason() {
            if matches!(Then::of(reason), Then::SignIn | Then::AccountGone) {
                assert!(words::failure::closed(reason).is_some(), "{reason:?}");
            }
        }
    }

    #[test]
    fn only_a_session_that_goes_on_is_tried_again() {
        let tried: Vec<_> = every_reason().filter(|reason| Then::of(*reason).retry()).collect();
        let not: Vec<_> = every_reason().filter(|reason| !Then::of(*reason).retry()).collect();
        assert!(not.contains(&CloseReason::Kicked) && not.contains(&CloseReason::VersionMismatch), "{not:?}");
        assert!(not.contains(&CloseReason::Banned), "a banned account signs in only once unbanned");
        assert!(tried.contains(&CloseReason::ServerStopping) && tried.contains(&CloseReason::Moved), "{tried:?}");
        assert_eq!(tried.len() + not.len(), every_reason().count());
    }
}
