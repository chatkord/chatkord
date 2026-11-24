//! Where a feed's file is: an `https` address, or for a check by hand a plain one on this machine's loopback.

use crate::error::UpdateError;
use hyper::Uri;
use std::net::{IpAddr, SocketAddr};

/// The longest address followed: a release host's redirect carries a signed query of about a kilobyte.
pub(crate) const MAX_ADDRESS: usize = 4096;

/// How an address is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reached {
    /// TLS 1.3, its host vouched for by the system's verifier.
    Https,
    /// Plain HTTP to a loopback address of this machine, which no other machine can stand in for.
    Loopback(SocketAddr),
}

/// An absolute address with a host, no user, no fragment, within [`MAX_ADDRESS`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Address {
    uri: Uri,
    reached: Reached,
}

/// Why an address was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AddressRefused;

impl Address {
    /// A feed's base, where `latest.json` and the packages are: `https`, or plain `http` only to a loopback IP when
    /// `loopback` allows it; its path a directory (ending in `/`) with no query.
    pub(crate) fn base(text: &str, loopback: Loopback) -> Result<Address, AddressRefused> {
        let address = Address::parse(text, loopback)?;
        if !address.uri.path().ends_with('/') || address.uri.query().is_some() {
            return Err(AddressRefused);
        }
        Ok(address)
    }

    /// The file `name` in this base, a [`plain_segment`].
    pub(crate) fn join(&self, name: &str) -> Result<Address, UpdateError> {
        if !plain_segment(name) {
            return Err(UpdateError::Malformed);
        }
        let mut parts = self.uri.clone().into_parts();
        let path = format!("{}{name}", self.uri.path());
        parts.path_and_query = Some(path.parse().map_err(|_| UpdateError::Malformed)?);
        let uri = Uri::from_parts(parts).map_err(|_| UpdateError::Malformed)?;
        Ok(Address { uri, reached: self.reached })
    }

    /// Where a redirect's `location` leads from here: an `https` address, or a path on this one's host when this
    /// one is `https`. Anything else is refused, a loopback address included.
    pub(crate) fn redirected(&self, location: &str) -> Result<Address, UpdateError> {
        let text = match (location.starts_with('/') && !location.starts_with("//"), self.reached) {
            (true, Reached::Https) => format!("https://{}{location}", self.authority()),
            (true, Reached::Loopback(_)) => return Err(UpdateError::BadRedirect),
            (false, _) => location.to_string(),
        };
        Address::parse(&text, Loopback::Refused).map_err(|_| UpdateError::BadRedirect)
    }

    fn parse(text: &str, loopback: Loopback) -> Result<Address, AddressRefused> {
        if text.len() > MAX_ADDRESS || text.contains('#') {
            return Err(AddressRefused);
        }
        let uri: Uri = text.parse().map_err(|_| AddressRefused)?;
        let authority = uri.authority().ok_or(AddressRefused)?;
        if authority.as_str().contains('@') || authority.host().is_empty() {
            return Err(AddressRefused);
        }
        let reached = match (uri.scheme_str(), loopback) {
            (Some("https"), _) => Reached::Https,
            (Some("http"), Loopback::Allowed) => {
                let host = authority.host().trim_start_matches('[').trim_end_matches(']');
                let ip = host.parse::<IpAddr>().ok().filter(IpAddr::is_loopback).ok_or(AddressRefused)?;
                Reached::Loopback(SocketAddr::new(ip, authority.port_u16().unwrap_or(80)))
            }
            _ => return Err(AddressRefused),
        };
        Ok(Address { uri, reached })
    }

    /// The address as a request's target.
    pub(crate) fn uri(&self) -> &Uri {
        &self.uri
    }

    /// How it is reached.
    pub(crate) fn reached(&self) -> Reached {
        self.reached
    }

    /// Its host and port as written, a request's `Host`.
    pub(crate) fn authority(&self) -> &str {
        self.uri.authority().map_or("", |authority| authority.as_str())
    }

    /// Its path and query, a request's target in origin form.
    pub(crate) fn origin_form(&self) -> &str {
        self.uri.path_and_query().map_or("/", |path| path.as_str())
    }
}

/// Whether `name` is one segment of letters, digits, `.`, `_` and `-`, not starting with a dot: the same in a URL's
/// path and a directory as it is written, and never `.` or `..`.
pub(crate) fn plain_segment(name: &str) -> bool {
    let allowed = name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    allowed && !name.is_empty() && !name.starts_with('.')
}

/// Whether a plain address on this machine's loopback is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Loopback {
    /// Taken: the feed a person names for a check by hand.
    Allowed,
    /// Refused: the address a build was stamped with, and every redirect.
    Refused,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base_is_an_https_directory() {
        let base = Address::base("https://example.invalid/o/r/releases/latest/download/", Loopback::Refused).unwrap();
        assert_eq!(base.reached(), Reached::Https);
        let file = base.join("latest.json").unwrap();
        assert_eq!(file.uri().to_string(), "https://example.invalid/o/r/releases/latest/download/latest.json");
        assert_eq!(file.authority(), "example.invalid");
        assert_eq!(file.origin_form(), "/o/r/releases/latest/download/latest.json");
        for text in [
            "http://example.invalid/",
            "https://example.invalid/download",
            "https://example.invalid/?q=1",
            "https://example.invalid/#top",
            "https://user@example.invalid/",
            "https:///",
            "ftp://example.invalid/",
            "example.invalid/",
            "/download/",
            "",
            "http://127.0.0.1/",
        ] {
            assert_eq!(Address::base(text, Loopback::Refused), Err(AddressRefused), "{text:?}");
        }
        let long = format!("https://example.invalid/{}/", "a".repeat(MAX_ADDRESS));
        assert_eq!(Address::base(&long, Loopback::Refused), Err(AddressRefused));
    }

    #[test]
    fn a_plain_base_is_taken_only_on_a_loopback_ip_and_only_when_allowed() {
        for (text, at) in [("http://127.0.0.1:8080/feed/", "127.0.0.1:8080"), ("http://[::1]/", "[::1]:80")] {
            let base = Address::base(text, Loopback::Allowed).unwrap();
            assert_eq!(base.reached(), Reached::Loopback(at.parse().unwrap()), "{text}");
        }
        for text in ["http://localhost/", "http://example.invalid/", "http://192.0.2.1/", "http://[::2]/"] {
            assert_eq!(Address::base(text, Loopback::Allowed), Err(AddressRefused), "{text:?}");
        }
        assert!(Address::base("https://example.invalid/", Loopback::Allowed).is_ok());
    }

    #[test]
    fn a_name_joined_is_one_plain_segment() {
        let base = Address::base("https://example.invalid/d/", Loopback::Refused).unwrap();
        for name in ["", "a/b", "..", ".", ".a", "a?b", "a b", "%2e", "a#b"] {
            assert_eq!(base.join(name), Err(UpdateError::Malformed), "{name:?}");
        }
    }

    #[test]
    fn a_redirect_is_followed_only_to_https() {
        let here = Address::base("https://example.invalid/d/", Loopback::Refused).unwrap().join("a.AppImage").unwrap();
        let there = here.redirected("https://cdn.example.invalid/x?sig=1").unwrap();
        assert_eq!(there.uri().to_string(), "https://cdn.example.invalid/x?sig=1");
        assert_eq!(here.redirected("/y").unwrap().uri().to_string(), "https://example.invalid/y");
        for location in ["http://cdn.example.invalid/x", "http://127.0.0.1/x", "//cdn.example.invalid/x", "x", ""] {
            assert_eq!(here.redirected(location), Err(UpdateError::BadRedirect), "{location:?}");
        }
        let plain = Address::base("http://127.0.0.1:1/", Loopback::Allowed).unwrap();
        assert_eq!(plain.redirected("/y"), Err(UpdateError::BadRedirect));
        assert!(plain.redirected("https://example.invalid/y").is_ok());
    }
}
