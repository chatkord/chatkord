use proto::chatkord_v1::{CloseReason, ErrorCode};
use proto::pin::KeyPin;
use std::fmt;

/// Why a request or the connection did not work. `Display` is for a log, not a person.
#[derive(Clone, Debug, PartialEq, Eq, report::Public)]
pub enum LinkError {
    /// Nothing answered at that address: the name did not resolve, nothing was
    /// listening, or the connection died before any answer reached us.
    Connect,
    /// The name in the address leads nowhere: it does not resolve.
    NoSuchHost,
    /// This device has no route to any network, so no address could be reached: told in place of [`LinkError::NoSuchHost`]
    /// where the name could not be asked for that reason.
    NoNetwork,
    /// This client holds no live connection to that server, so nothing was asked.
    Offline,
    /// The server took the connection and did not greet in time.
    Unanswered,
    /// The connection was lost before the server answered.
    Lost,
    /// The server sent what this build cannot read: a frame it cannot decode, a greeting of no shape, or a reply that
    /// is not the one the schema names.
    Malformed,
    /// The server speaks another major of this protocol: never tried again until one end is updated.
    OtherMajor(ServerIs),
    /// Something answered at that address, but not as a Chatkord server: it
    /// answered as plain HTTP, or refused the socket this product speaks.
    NotChatkord,
    /// The address is encrypted in a way this device does not trust: no
    /// certificate, one that is not trusted, or one that does not name that
    /// address.
    Untrusted,
    /// The server's certificate is vouched for by no authority this device trusts and by no key it was told to trust
    /// for that server: the key it presented, for a person to trust on first use.
    Unvouched(KeyPin),
    /// The server presented another key than the one this device trusts for it; nothing was sent.
    KeyChanged {
        /// The key this device trusts for that server.
        trusted: KeyPin,
        /// The key it presented now.
        presented: KeyPin,
    },
    /// The server speaks only a TLS older than 1.3, which this build does not.
    OldEncryption,
    /// The server answered the request with a refusal; its code says why, `Unspecified` for one this build does not
    /// know (a server on a newer minor).
    Refused(#[public] ErrorCode),
    /// The connection ended, and the server's reason if it gave one.
    Closed(Ended),
    /// The server did not answer the request in time; the connection stays open.
    Timeout,
    /// The request is larger than one frame carries, so it was never sent; the connection stays open.
    TooLarge,
}

/// What a failure is, for whoever decides what to say or do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The server answered the request with this refusal.
    Refused(ErrorCode),
    /// About a server that answered and will answer so again, whatever is tried: its key, its encryption or its
    /// protocol major. It stays until someone acts: an update, or a person's word on its key.
    Lasting,
    /// The address or the connection failed, or the server did not answer: trying again may help.
    Unreached,
}

impl LinkError {
    /// What this failure is: a refusal, a fact about the server that lasts, or no answer.
    pub const fn kind(&self) -> Kind {
        match self {
            Self::Refused(code) => Kind::Refused(*code),
            Self::OtherMajor(_) | Self::Unvouched(_) | Self::KeyChanged { .. } | Self::OldEncryption => Kind::Lasting,
            Self::Connect
            | Self::NoSuchHost
            | Self::NoNetwork
            | Self::Offline
            | Self::Unanswered
            | Self::Lost
            | Self::Malformed
            | Self::NotChatkord
            | Self::Untrusted
            | Self::Closed(_)
            | Self::Timeout
            | Self::TooLarge => Kind::Unreached,
        }
    }
}

/// A server of another major, against this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerIs {
    /// It speaks a later major: this build is the one to update.
    Newer,
    /// It speaks an earlier major: the server is the one to update.
    Older,
    /// It refused without saying which major it speaks.
    Unsaid,
}

/// How a connection ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ended {
    /// The reason the server closed with, when it said.
    pub reason: Option<CloseReason>,
    /// The refusal the server wrote just before closing, when it wrote one.
    pub refusal: Option<ErrorCode>,
    /// True when this end gave up because the server stopped answering heartbeats.
    pub silent: bool,
    /// Where the connection stood in what the whole server tells, for `Link::resume` once signed in again; none when
    /// it was handed no event the server tells in order.
    pub cursor: Option<crate::cursor::Cursor>,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect => f.write_str("connect"),
            Self::NoSuchHost => f.write_str("no such host"),
            Self::NoNetwork => f.write_str("no route to any network"),
            Self::Offline => f.write_str("no live connection"),
            Self::Unanswered => f.write_str("no greeting in time"),
            Self::Lost => f.write_str("connection lost"),
            Self::Malformed => f.write_str("malformed answer"),
            Self::OtherMajor(server) => write!(f, "server on another major ({server:?})"),
            Self::NotChatkord => f.write_str("not a chatkord server"),
            Self::Untrusted => f.write_str("untrusted address"),
            Self::Unvouched(pin) => write!(f, "a key nothing vouches for ({pin})"),
            Self::KeyChanged { trusted, presented } => write!(f, "key changed from {trusted} to {presented}"),
            Self::OldEncryption => f.write_str("TLS older than 1.3"),
            Self::Refused(code) => write!(f, "refused {code:?}"),
            Self::Closed(ended) => write!(f, "closed {:?}", ended.reason),
            Self::Timeout => f.write_str("timed out"),
            Self::TooLarge => f.write_str("request over a frame's size"),
        }
    }
}

impl std::error::Error for LinkError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin() -> KeyPin {
        KeyPin::from_digest([7; KeyPin::LEN])
    }

    /// Every variant, each written out: a new one stops the build here until its kind is pinned.
    fn every(error: &LinkError) -> Kind {
        match error {
            LinkError::Connect
            | LinkError::NoSuchHost
            | LinkError::NoNetwork
            | LinkError::Offline
            | LinkError::Unanswered
            | LinkError::Lost
            | LinkError::Malformed
            | LinkError::NotChatkord
            | LinkError::Untrusted
            | LinkError::Closed(_)
            | LinkError::Timeout
            | LinkError::TooLarge => Kind::Unreached,
            LinkError::OtherMajor(_)
            | LinkError::Unvouched(_)
            | LinkError::KeyChanged { .. }
            | LinkError::OldEncryption => Kind::Lasting,
            LinkError::Refused(code) => Kind::Refused(*code),
        }
    }

    #[test]
    fn every_failure_is_a_refusal_a_lasting_fact_about_the_server_or_no_answer() {
        let all = [
            LinkError::Connect,
            LinkError::NoSuchHost,
            LinkError::NoNetwork,
            LinkError::Offline,
            LinkError::Unanswered,
            LinkError::Lost,
            LinkError::Malformed,
            LinkError::OtherMajor(ServerIs::Newer),
            LinkError::OtherMajor(ServerIs::Older),
            LinkError::OtherMajor(ServerIs::Unsaid),
            LinkError::NotChatkord,
            LinkError::Untrusted,
            LinkError::Unvouched(pin()),
            LinkError::KeyChanged { trusted: pin(), presented: pin() },
            LinkError::OldEncryption,
            LinkError::Refused(ErrorCode::NotAuthorized),
            LinkError::Refused(ErrorCode::Unspecified),
            LinkError::Closed(Ended::default()),
            LinkError::Timeout,
            LinkError::TooLarge,
        ];
        for error in all {
            assert_eq!(error.kind(), every(&error), "{error}");
        }
        assert_eq!(LinkError::Refused(ErrorCode::RateLimited).kind(), Kind::Refused(ErrorCode::RateLimited));
        assert_eq!(LinkError::OldEncryption.kind(), Kind::Lasting);
        assert_eq!(LinkError::Timeout.kind(), Kind::Unreached);
    }
}
