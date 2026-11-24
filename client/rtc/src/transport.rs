//! What the server sends about a member's transport, parsed once, and what the engine's own SDP says about this end's
//! DTLS identity.

use proto::mediasoup_types::data_structures::{DtlsFingerprint, DtlsParameters, DtlsRole, IceCandidate, IceParameters};
use proto::sdp::{SfuJsonRefused, sfu_json};
use serde::Deserialize;
use std::net::IpAddr;

/// The most ICE candidates a transport may list: a server offers a few addresses, never a crowd.
pub(crate) const MAX_ICE_CANDIDATES: usize = 8;

/// A member's transport as the server describes it (`SfuOffer.transport`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transport {
    /// The server's ICE credentials.
    pub ice_parameters: IceParameters,
    /// Where the server listens.
    pub ice_candidates: Vec<IceCandidate>,
    /// The server's DTLS role and certificate fingerprints.
    pub dtls_parameters: DtlsParameters,
}

/// Why server JSON or the engine's SDP could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportRefused {
    /// Past the bound on server JSON.
    TooLong,
    /// Not the JSON a transport is.
    NotJson,
    /// More ICE candidates than any real server lists.
    TooManyCandidates,
    /// The engine's SDP carries no fingerprint that can be read.
    NoFingerprint,
}

impl Transport {
    /// The candidates' addresses that are host names, each once, in the order listed: what the caller looks up. An
    /// address that is neither an IP address nor a host name names nothing and is left out.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for candidate in &self.ice_candidates {
            let name = candidate.address.as_str();
            if name.parse::<IpAddr>().is_err() && is_host_name(name) && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// The first candidate's address as the server announced it, and its port, for what a person is told: none when
    /// it is neither an IP address nor a host name.
    pub fn announced(&self) -> Option<(&str, u16)> {
        let first = self.ice_candidates.first()?;
        let address = first.address.as_str();
        (address.parse::<IpAddr>().is_ok() || is_host_name(address)).then_some((address, first.port))
    }

    /// Reads `SfuOffer.transport`.
    pub fn parse(bytes: &[u8]) -> Result<Self, TransportRefused> {
        let transport: Self = sfu_json(bytes).map_err(|refused| match refused {
            SfuJsonRefused::TooLong => TransportRefused::TooLong,
            SfuJsonRefused::NotJson | SfuJsonRefused::NotAscii => TransportRefused::NotJson,
        })?;
        if transport.ice_candidates.len() > MAX_ICE_CANDIDATES {
            return Err(TransportRefused::TooManyCandidates);
        }
        Ok(transport)
    }
}

/// Whether `name` is a DNS host name: dot-separated labels of ASCII letters, digits and hyphens, none empty, longer
/// than 63 bytes or starting or ending with a hyphen, at most 253 bytes in all.
fn is_host_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// This end's DTLS parameters, read from the SDP the engine wrote: the client is the DTLS client against an ICE-lite
/// server, and the fingerprint is the engine's.
pub fn dtls_from_sdp(sdp: &str) -> Result<DtlsParameters, TransportRefused> {
    let line = sdp
        .lines()
        .find_map(|line| line.trim().strip_prefix("a=fingerprint:"))
        .ok_or(TransportRefused::NoFingerprint)?;
    let (algorithm, value) = line.split_once(' ').ok_or(TransportRefused::NoFingerprint)?;
    let json = format!(r#"{{"algorithm":"{algorithm}","value":"{value}"}}"#);
    let fingerprint: DtlsFingerprint = sfu_json(json.as_bytes()).map_err(|_| TransportRefused::NoFingerprint)?;
    Ok(DtlsParameters { role: DtlsRole::Client, fingerprints: vec![fingerprint] })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINGERPRINT: &str =
        "AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89";

    #[test]
    fn the_engines_fingerprint_becomes_the_dtls_parameters_the_server_needs() {
        let sdp = format!("v=0\r\na=ice-ufrag:x\r\na=fingerprint:sha-256 {FINGERPRINT}\r\na=setup:actpass\r\n");
        let parameters = dtls_from_sdp(&sdp).expect("read");
        assert_eq!(parameters.role, DtlsRole::Client);
        assert_eq!(parameters.fingerprints.len(), 1);
        assert_eq!(parameters.fingerprints[0].value_string(), FINGERPRINT);
    }

    #[test]
    fn an_sdp_with_no_fingerprint_or_a_bad_one_is_refused() {
        assert_eq!(dtls_from_sdp("v=0\r\n"), Err(TransportRefused::NoFingerprint));
        assert_eq!(dtls_from_sdp("a=fingerprint:sha-256 nothex\r\n"), Err(TransportRefused::NoFingerprint));
        assert_eq!(dtls_from_sdp("a=fingerprint:md5 AB:CD\r\n"), Err(TransportRefused::NoFingerprint));
    }

    #[test]
    fn a_server_fingerprint_with_a_wide_character_is_refused_not_a_crash() {
        let value = format!("A\u{1b4}{}", "A".repeat(92));
        let json = format!(
            r#"{{"iceParameters":{{"usernameFragment":"u","password":"p","iceLite":true}},"iceCandidates":[],
            "dtlsParameters":{{"role":"auto","fingerprints":[{{"algorithm":"sha-256","value":"{value}"}}]}}}}"#
        );
        assert_eq!(Transport::parse(json.as_bytes()).unwrap_err(), TransportRefused::NotJson);
    }

    #[test]
    fn a_transport_listing_more_candidates_than_the_bound_is_refused() {
        let candidate =
            r#"{"foundation":"f","priority":1,"address":"192.0.2.1","port":4000,"protocol":"udp","type":"host"}"#;
        let transport = |count: usize| {
            let candidates = vec![candidate; count].join(",");
            format!(
                r#"{{"iceParameters":{{"usernameFragment":"u","password":"p","iceLite":true}},"iceCandidates":[{candidates}],
                "dtlsParameters":{{"role":"auto","fingerprints":[]}}}}"#
            )
        };
        assert!(Transport::parse(transport(MAX_ICE_CANDIDATES).as_bytes()).is_ok());
        assert_eq!(
            Transport::parse(transport(MAX_ICE_CANDIDATES + 1).as_bytes()).unwrap_err(),
            TransportRefused::TooManyCandidates
        );
    }

    #[test]
    fn only_a_host_name_is_looked_up_or_told() {
        let candidate = |address: &str| {
            format!(
                r#"{{"foundation":"f","priority":1,"address":"{address}","port":4000,"protocol":"udp","type":"host"}}"#
            )
        };
        let transport = |addresses: &[&str]| {
            let candidates: Vec<String> = addresses.iter().map(|address| candidate(address)).collect();
            let json = format!(
                r#"{{"iceParameters":{{"usernameFragment":"u","password":"p","iceLite":true}},"iceCandidates":[{}],
                "dtlsParameters":{{"role":"auto","fingerprints":[]}}}}"#,
                candidates.join(",")
            );
            Transport::parse(json.as_bytes()).expect("a transport")
        };
        let listed = transport(&["voice.example.invalid", "192.0.2.1", "a b", "bell\\u0007", "-x.example.invalid"]);
        assert_eq!(listed.names(), ["voice.example.invalid"]);
        assert_eq!(listed.announced(), Some(("voice.example.invalid", 4000)));
        assert_eq!(transport(&["::1"]).announced(), Some(("::1", 4000)));
        assert_eq!(transport(&["bell\\u0007"]).announced(), None, "a control is never put before a person");
        assert!(is_host_name("localhost") && is_host_name("example.invalid.") && !is_host_name(&"a".repeat(64)));
    }

    #[test]
    fn server_json_is_bounded_and_must_be_the_shape() {
        assert_eq!(
            Transport::parse(&vec![b' '; proto::envelope::MAX_SFU_JSON_BYTES + 1]).unwrap_err(),
            TransportRefused::TooLong
        );
        assert_eq!(Transport::parse(b"{}").unwrap_err(), TransportRefused::NotJson);
        assert_eq!(Transport::parse(b"nope").unwrap_err(), TransportRefused::NotJson);
    }
}
