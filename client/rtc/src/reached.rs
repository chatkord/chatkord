//! Where the server's media is reached: its candidates as addresses this device may send to, never as names. A name
//! the server announced is looked up by the caller, and libwebrtc resolves one family only, so the SDP carries every
//! address the name has and no name at all.

use crate::transport::{MAX_ICE_CANDIDATES, Transport};
use proto::mediasoup_types::data_structures::{
    DtlsParameters, IceCandidate, IceCandidateTcpType, IceCandidateType, IceParameters, Protocol,
};
use std::collections::HashMap;
use std::net::IpAddr;

/// What each name among a transport's candidates resolved to, as the caller looked it up.
pub type Resolved = HashMap<String, Vec<IpAddr>>;

/// How near an address is, from this device's side: a server reached at one scope may announce candidates of that
/// scope or wider, never a narrower one, so a public server cannot send media to this device's own network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// This machine only.
    Loopback,
    /// The link this machine is on.
    LinkLocal,
    /// A private network: RFC 1918, carrier-grade NAT, or an IPv6 unique local address.
    Private,
    /// Anywhere on the internet.
    Global,
}

impl Scope {
    /// The scope of `address`; none for one nothing can be reached at (unspecified, multicast, broadcast).
    pub fn of(address: IpAddr) -> Option<Self> {
        match address.to_canonical() {
            IpAddr::V4(v4) => {
                let octets = v4.octets();
                if v4.is_unspecified() || v4.is_multicast() || v4.is_broadcast() {
                    None
                } else if v4.is_loopback() {
                    Some(Self::Loopback)
                } else if v4.is_link_local() {
                    Some(Self::LinkLocal)
                } else if v4.is_private() || (octets[0] == 100 && octets[1] & 0xc0 == 64) {
                    Some(Self::Private)
                } else {
                    Some(Self::Global)
                }
            }
            IpAddr::V6(v6) => {
                let first = v6.segments()[0];
                if v6.is_unspecified() || v6.is_multicast() {
                    None
                } else if v6.is_loopback() {
                    Some(Self::Loopback)
                } else if first & 0xffc0 == 0xfe80 {
                    Some(Self::LinkLocal)
                } else if first & 0xfe00 == 0xfc00 {
                    Some(Self::Private)
                } else {
                    Some(Self::Global)
                }
            }
        }
    }

    /// The scope of a server reached at `peer`. An address with none is held to the widest, the strictest reading.
    pub fn of_server(peer: IpAddr) -> Self {
        Self::of(peer).unwrap_or(Self::Global)
    }
}

/// Why no candidate of a transport could be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotReached {
    /// Every candidate named nothing that resolved, or an address narrower than the server's own.
    Nothing,
    /// The names, once looked up, make more candidates than any real server lists.
    TooMany,
}

/// One candidate whose address is an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) foundation: String,
    pub(crate) priority: u32,
    pub(crate) address: IpAddr,
    pub(crate) protocol: Protocol,
    pub(crate) port: u16,
    pub(crate) kind: IceCandidateType,
    pub(crate) tcp_type: Option<IceCandidateTcpType>,
}

impl Candidate {
    fn at(listed: &IceCandidate, address: IpAddr, nth: usize) -> Self {
        let foundation = if nth == 0 { listed.foundation.clone() } else { format!("{}{nth}", listed.foundation) };
        Self {
            foundation,
            priority: listed.priority,
            address,
            protocol: listed.protocol,
            port: listed.port,
            kind: listed.r#type,
            tcp_type: listed.tcp_type,
        }
    }
}

/// A member's transport as the SDP may state it: the server's credentials and certificate, and only candidates that
/// are addresses within the server's own scope.
#[derive(Clone, Debug)]
pub struct Reached {
    pub(crate) ice_parameters: IceParameters,
    pub(crate) ice_candidates: Vec<Candidate>,
    pub(crate) dtls_parameters: DtlsParameters,
}

impl Reached {
    /// The candidates of `transport` with each name turned, by `resolved`, into one candidate per address it has,
    /// every address narrower than the `server`'s scope left out. Nothing left, or more than the bound once expanded,
    /// is refused.
    pub fn of(transport: &Transport, resolved: &Resolved, server: Scope) -> Result<Self, NotReached> {
        let mut found: Vec<Candidate> = Vec::new();
        for listed in &transport.ice_candidates {
            let parsed = listed.address.parse::<IpAddr>().ok();
            let addresses = match &parsed {
                Some(address) => std::slice::from_ref(address),
                None => resolved.get(&listed.address).map_or(&[][..], Vec::as_slice),
            };
            let mut nth = 0;
            for &address in addresses {
                if !Scope::of(address).is_some_and(|scope| scope >= server) {
                    continue;
                }
                let candidate = Candidate::at(listed, address, nth);
                let twin = |other: &Candidate| {
                    (other.address, other.port, other.protocol) == (address, listed.port, listed.protocol)
                };
                if !found.iter().any(twin) {
                    if found.len() == MAX_ICE_CANDIDATES {
                        return Err(NotReached::TooMany);
                    }
                    found.push(candidate);
                    nth += 1;
                }
            }
        }
        match found.len() {
            0 => Err(NotReached::Nothing),
            _ => Ok(Self {
                ice_parameters: transport.ice_parameters.clone(),
                ice_candidates: found,
                dtls_parameters: transport.dtls_parameters.clone(),
            }),
        }
    }

    /// Replaces the server's ICE credentials, as an ICE restart gives them.
    pub fn restarted(&mut self, ice_parameters: IceParameters) {
        self.ice_parameters = ice_parameters;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport(addresses: &[&str]) -> Transport {
        let candidates: Vec<String> = addresses
            .iter()
            .enumerate()
            .map(|(nth, address)| {
                format!(
                    r#"{{"foundation":"f{nth}","priority":1,"address":"{address}","port":4000,"protocol":"udp","type":"host"}}"#
                )
            })
            .collect();
        let json = format!(
            r#"{{"iceParameters":{{"usernameFragment":"u","password":"p","iceLite":true}},"iceCandidates":[{}],
            "dtlsParameters":{{"role":"auto","fingerprints":[]}}}}"#,
            candidates.join(",")
        );
        Transport::parse(json.as_bytes()).expect("a transport")
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("an address")
    }

    fn addresses(reached: &Reached) -> Vec<IpAddr> {
        reached.ice_candidates.iter().map(|candidate| candidate.address).collect()
    }

    #[test]
    fn a_name_on_both_loopbacks_becomes_a_candidate_for_each() {
        let resolved = Resolved::from([("localhost".to_string(), vec![ip("::1"), ip("127.0.0.1")])]);
        let reached = Reached::of(&transport(&["localhost"]), &resolved, Scope::Loopback).expect("reached");
        assert_eq!(addresses(&reached), [ip("::1"), ip("127.0.0.1")]);
        assert_ne!(reached.ice_candidates[0].foundation, reached.ice_candidates[1].foundation);
    }

    #[test]
    fn a_private_candidate_of_a_global_server_is_refused() {
        let resolved = Resolved::new();
        let all = transport(&["10.0.0.5", "192.0.2.7", "fd00::1", "127.0.0.1", "fe80::1"]);
        let reached = Reached::of(&all, &resolved, Scope::Global).expect("the public one stays");
        assert_eq!(addresses(&reached), [ip("192.0.2.7")]);
        assert_eq!(Reached::of(&transport(&["10.0.0.5"]), &resolved, Scope::Global).unwrap_err(), NotReached::Nothing);
        assert!(Reached::of(&transport(&["10.0.0.5"]), &resolved, Scope::Private).is_ok(), "as near as the server");
    }

    #[test]
    fn a_name_that_resolved_to_nothing_leaves_nothing() {
        let resolved = Resolved::from([("gone.example.invalid".to_string(), Vec::new())]);
        assert_eq!(
            Reached::of(&transport(&["gone.example.invalid", "other.example.invalid"]), &resolved, Scope::Loopback)
                .unwrap_err(),
            NotReached::Nothing
        );
    }

    #[test]
    fn the_bound_holds_after_the_names_are_expanded() {
        let many: Vec<IpAddr> = (1..=MAX_ICE_CANDIDATES + 1).map(|last| ip(&format!("192.0.2.{last}"))).collect();
        let resolved = Resolved::from([("many.example.invalid".to_string(), many[..MAX_ICE_CANDIDATES].to_vec())]);
        let at_bound = Reached::of(&transport(&["many.example.invalid"]), &resolved, Scope::Global).expect("at bound");
        assert_eq!(at_bound.ice_candidates.len(), MAX_ICE_CANDIDATES);
        let resolved = Resolved::from([("many.example.invalid".to_string(), many)]);
        assert_eq!(
            Reached::of(&transport(&["many.example.invalid"]), &resolved, Scope::Global).unwrap_err(),
            NotReached::TooMany
        );
    }

    #[test]
    fn scopes_are_ordered_from_the_machine_to_the_internet() {
        let scope = |text: &str| Scope::of(ip(text));
        assert_eq!(scope("127.0.0.1"), Some(Scope::Loopback));
        assert_eq!(scope("::ffff:127.0.0.1"), Some(Scope::Loopback));
        assert_eq!(scope("169.254.1.1"), Some(Scope::LinkLocal));
        assert_eq!(scope("172.16.0.1"), Some(Scope::Private));
        assert_eq!(scope("100.64.0.1"), Some(Scope::Private));
        assert_eq!(scope("fc00::1"), Some(Scope::Private));
        assert_eq!(scope("2001:db8::1"), Some(Scope::Global));
        assert_eq!(scope("0.0.0.0"), None);
        assert_eq!(scope("224.0.0.1"), None);
        assert!(
            Scope::Loopback < Scope::LinkLocal && Scope::LinkLocal < Scope::Private && Scope::Private < Scope::Global
        );
    }
}
