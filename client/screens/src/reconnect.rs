//! A dropped connection retried by itself: which ends are worth retrying and how long to wait before each try. The
//! tries go through the sign-in model's own kept token, go on for as long as the window runs (the other servers keep
//! working meanwhile), and a success goes back to the channels, never into a call.

use crate::closed::Then;
use net::Backoff;
use proto::chatkord_v1::ErrorCode;
use std::time::Duration;
use terms::Ended;

/// Whether a connection that ended this way is worth signing in to again by itself: not when the
/// session itself is over (ended elsewhere, expired, removed) or this build cannot speak to that server.
pub(crate) fn retryable(ended: &Ended) -> bool {
    if matches!(ended.refusal, Some(ErrorCode::Unauthenticated)) {
        return false;
    }
    ended.reason.is_none_or(|reason| Then::of(reason).retry())
}

/// The wait before one reconnect try: 1, 2, 4, 8 and 16 seconds, then 30, each times the host's jitter.
pub(crate) const BACKOFF: Backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));

#[cfg(test)]
mod tests {
    use super::*;
    use proto::chatkord_v1::CloseReason;

    fn ended(reason: Option<CloseReason>, code: Option<ErrorCode>) -> Ended {
        Ended { reason, refusal: code, silent: false, cursor: None }
    }

    #[test]
    fn a_session_that_is_over_is_not_retried() {
        for reason in [
            CloseReason::SignedOut,
            CloseReason::SessionExpired,
            CloseReason::Kicked,
            CloseReason::VersionMismatch,
            CloseReason::PasswordChanged,
            CloseReason::PasswordReset,
            CloseReason::AccountDeleted,
            CloseReason::Banned,
        ] {
            assert!(!retryable(&ended(Some(reason), None)), "{reason:?}");
        }
        assert!(!retryable(&ended(None, Some(ErrorCode::Unauthenticated))), "a dead token is not tried again");
    }

    #[test]
    fn a_server_that_went_away_is_retried() {
        for reason in [
            CloseReason::Unspecified,
            CloseReason::Normal,
            CloseReason::Unreachable,
            CloseReason::ServerError,
            CloseReason::ClientShutdown,
            CloseReason::ServerStopping,
        ] {
            assert!(retryable(&ended(Some(reason), None)), "{reason:?}");
        }
        assert!(retryable(&Ended { silent: true, ..Ended::default() }), "a server that stopped answering");
        assert!(retryable(&ended(None, Some(ErrorCode::ServerUnavailable))));
    }
}
