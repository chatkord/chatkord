//! Where the call's media is sent: both of its transports' candidates as addresses, looked up once at the join. The
//! engine resolves one family of a name and prefers IPv6, so a server announced by name is reached by every address
//! the name has, none of them nearer this device than the server itself is.

use crate::voice::VoiceRefused;
use proto::chatkord_v1::VoiceState;
use rtc::{NotReached, Reached, Resolved, Scope, Transport};
use std::net::IpAddr;
use std::time::Duration;

/// How long one candidate's name may take to look up: the join waits for it.
const LOOKUP_WITHIN: Duration = Duration::from_secs(5);

/// Both transports of a call, reached.
#[derive(Clone, Debug)]
pub struct Located {
    pub(crate) send: Reached,
    pub(crate) receive: Reached,
    host: String,
    port: u16,
}

/// The server's two transports, parsed.
fn parsed(joined: &VoiceState) -> Result<(Transport, Transport), VoiceRefused> {
    let (Some(send), Some(receive)) = (&joined.send, &joined.receive) else { return Err(VoiceRefused::Offer) };
    let read =
        |offer: &proto::chatkord_v1::SfuOffer| Transport::parse(&offer.transport).map_err(|_| VoiceRefused::Offer);
    Ok((read(send)?, read(receive)?))
}

/// Both transports reached, remembering where the server said its media was.
fn both(send: &Transport, receive: &Transport, resolved: &Resolved, server: Scope) -> Result<Located, VoiceRefused> {
    let (host, port) = send.announced().map(|(host, port)| (host.to_string(), port)).ok_or(VoiceRefused::Offer)?;
    Ok(Located { send: reach(send, resolved, server)?, receive: reach(receive, resolved, server)?, host, port })
}

/// `transport` reached, or where the server said it was when nothing of it can be.
fn reach(transport: &Transport, resolved: &Resolved, server: Scope) -> Result<Reached, VoiceRefused> {
    Reached::of(transport, resolved, server).map_err(|why| match (why, transport.announced()) {
        (NotReached::Nothing | NotReached::TooMany, Some((host, port))) => {
            VoiceRefused::Unreached { host: host.to_string(), port }
        }
        (NotReached::Nothing | NotReached::TooMany, None) => VoiceRefused::Offer,
    })
}

impl Located {
    /// The two transports of `joined` as `resolved` says the names among their candidates are, held to `server`'s
    /// scope. Where the lookup is not wanted, a server that announces addresses only is reached with none.
    pub fn of(joined: &VoiceState, resolved: &Resolved, server: Scope) -> Result<Self, VoiceRefused> {
        let (send, receive) = parsed(joined)?;
        both(&send, &receive, resolved, server)
    }

    /// The address and port the server announced its media at, as it wrote them: a name or an address.
    pub fn announced(&self) -> (&str, u16) {
        (&self.host, self.port)
    }
}

/// Looks up the names among the candidates of `joined`, then reaches them: the server is at `peer`, the address the
/// link connected to. A name that does not resolve in time is left out of the call, not waited for.
pub async fn locate(joined: &VoiceState, peer: IpAddr) -> Result<Located, VoiceRefused> {
    let (send, receive) = parsed(joined)?;
    let mut names = send.names();
    for name in receive.names() {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let looked = names.into_iter().map(|name| async move {
        let found = tokio::time::timeout(LOOKUP_WITHIN, tokio::net::lookup_host((name, 0))).await;
        let addresses = match found {
            Ok(Ok(found)) => found.map(|address| address.ip()).collect(),
            Ok(Err(_)) | Err(_) => Vec::new(),
        };
        (name.to_string(), addresses)
    });
    let resolved: Resolved = futures_util::future::join_all(looked).await.into_iter().collect();
    both(&send, &receive, &resolved, Scope::of_server(peer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tests::joined;

    #[test]
    fn a_server_that_announces_addresses_is_reached_without_a_lookup() {
        assert!(Located::of(&joined(), &Resolved::new(), Scope::Loopback).is_ok());
    }

    #[test]
    fn a_candidate_nearer_than_the_server_is_told_as_where_the_server_said_it_was() {
        let refused = Located::of(&joined(), &Resolved::new(), Scope::Global).expect_err("a loopback candidate");
        assert!(
            matches!(&refused, VoiceRefused::Unreached { host, port } if host == "127.0.0.1" && *port == 40001),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_server_on_this_machine_is_reached_by_its_loopback_address() {
        let reached = locate(&joined(), IpAddr::from([127, 0, 0, 1])).await.expect("reached");
        assert_eq!(reached.announced(), ("127.0.0.1", 40001));
    }
}
