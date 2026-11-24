//! What a person types about where a server is. Nobody types a scheme, so what is typed becomes the addresses worth
//! trying, protected first and plainly only to this machine; a scheme is never shown to a person.

use proto::envelope::is_local_authority;
use std::fmt;

/// A server's `/v1` socket address: the host and port it names, the path, and the
/// scheme this client reaches it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerUrl(String);

/// Why a typed address cannot be tried at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressError {
    /// Not something that names a host, so there is nothing to try anywhere.
    Shape,
    /// Plain text to a host that is not this machine, where a password would
    /// cross in the open.
    Unprotected,
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape => f.write_str("Type a server address like example.invalid, or example.invalid:8443."),
            Self::Unprotected => f.write_str(
                "A server on another machine is reached over an encrypted address, so your password is not sent in the open.",
            ),
        }
    }
}

/// How long an address's host, port and path may be: a host is at most 253 characters, a port and the path a few
/// more, so anything longer is not an address.
const MAX_ADDRESS_BYTES: usize = 512;

/// What a typed address claimed about protection, which decides how many
/// addresses are worth trying and what order they go in.
enum Asked {
    /// The person named the scheme: an override, tried as exactly that one address.
    Named(bool),
    /// The person typed a secure address: a positive claim about that server.
    Protected,
    /// The person said nothing either way.
    Unasked,
}

struct Parts {
    authority: String,
    host: String,
}

/// The host and port one typed address names, the host in its ASCII form (UTS #46), refused with an `@` (another
/// product's shape), a query or a fragment (a web page's), whitespace, a stray bracket, no host, a host that is not a
/// name, past the bound, a port that is not one, or a path other than the socket's `v1`.
fn parts_of(typed: &str) -> Result<Parts, AddressError> {
    if typed.len() > MAX_ADDRESS_BYTES
        || typed.contains('@')
        || typed.contains('?')
        || typed.contains('#')
        || typed.chars().any(|c| c.is_ascii_whitespace())
    {
        return Err(AddressError::Shape);
    }
    let (authority, path) = typed.split_once('/').unwrap_or((typed, ""));
    let (host, port) = host_and_port(authority)?;
    if host.is_empty() || (!port.is_empty() && !names_a_port(port)) || authority.ends_with(':') {
        return Err(AddressError::Shape);
    }
    if !matches!(path.trim_matches('/'), "" | "v1") {
        return Err(AddressError::Shape);
    }
    let host = if authority.starts_with('[') || host.contains(':') {
        host.parse::<std::net::Ipv6Addr>().map_err(|_| AddressError::Shape)?.to_string()
    } else {
        // Strict: a DNS name's letters, digits and hyphens; the lenient form lets a control character through.
        idna::domain_to_ascii_strict(host).map_err(|_| AddressError::Shape)?
    };
    let bracketed = if host.contains(':') { format!("[{host}]") } else { host.clone() };
    let authority = if port.is_empty() { bracketed } else { format!("{bracketed}:{port}") };
    Ok(Parts { authority, host })
}

/// The host and port an authority names; an IPv6 host is spelled in brackets, `[::1]:8443`.
fn host_and_port(authority: &str) -> Result<(&str, &str), AddressError> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']').ok_or(AddressError::Shape)?;
        return match after.strip_prefix(':') {
            Some(port) => Ok((host, port)),
            None if after.is_empty() => Ok((host, "")),
            None => Err(AddressError::Shape),
        };
    }
    if authority.contains(['[', ']']) {
        return Err(AddressError::Shape);
    }
    Ok(authority.split_once(':').unwrap_or((authority, "")))
}

/// Digits only, no sign, 1 to 65535.
fn names_a_port(port: &str) -> bool {
    port.chars().all(|c| c.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|number| number != 0)
}

/// One address from the parts a typed address names, always the socket's `/v1`.
fn address(secure: bool, parts: &Parts) -> ServerUrl {
    let scheme = if secure { "wss" } else { "ws" };
    ServerUrl(format!("{scheme}://{authority}/v1", authority = parts.authority))
}

impl ServerUrl {
    /// An address that already names its socket, scheme and all: what a script, a test or a resolved route writes.
    /// Plain text is refused to a host that is not this machine.
    pub fn parse(text: &str) -> Result<Self, AddressError> {
        let typed = text.trim();
        let (secure, rest) = match (typed.strip_prefix("wss://"), typed.strip_prefix("ws://")) {
            (Some(rest), _) => (true, rest),
            (None, Some(rest)) => (false, rest),
            (None, None) => return Err(AddressError::Shape),
        };
        let parts = parts_of(rest)?;
        if !secure && !is_local_authority(&parts.host) {
            return Err(AddressError::Unprotected);
        }
        Ok(address(secure, &parts))
    }

    /// The address as it goes to the socket library.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Where the address is, with neither a scheme nor a path: what a person reads of a route.
    pub fn authority(&self) -> &str {
        let after_scheme = match self.0.split_once("://") {
            Some((_, after)) => after,
            None => &self.0,
        };
        match after_scheme.split_once("/") {
            Some((authority, _)) => authority,
            None => after_scheme,
        }
    }

    /// Whether reaching this address is encrypted: the one test of its scheme.
    pub fn encrypted(&self) -> bool {
        self.0.starts_with("wss://")
    }
}

/// What a person types about a server: the addresses worth trying, in order, never empty. What is typed proves
/// nothing about the server, so the one that answers is chosen by `link::Link::resolve`.
///
/// | Typed | Tried, in order |
/// |---|---|
/// | `example.invalid`, `host:port`, `host/v1` | protected; then plainly, here only |
/// | `https://host` | protected only: a positive claim about that server |
/// | `http://host` | protected; then plainly, here only: no claim either way |
/// | `wss://host`, `ws://host` | exactly that address, and nothing else |
///
/// A port that is not typed stays untyped: the scheme's convention, never one guessed here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address(Vec<ServerUrl>);

impl Address {
    /// What a person types about where a server is: a name, a name and a port,
    /// a pasted `https://` or `http://`, or the address a script writes in full.
    pub fn typed(text: &str) -> Result<Self, AddressError> {
        let typed = text.trim();
        let (asked, rest) = match (
            typed.strip_prefix("wss://"),
            typed.strip_prefix("ws://"),
            typed.strip_prefix("https://"),
            typed.strip_prefix("http://"),
        ) {
            (Some(rest), _, _, _) => (Asked::Named(true), rest),
            (_, Some(rest), _, _) => (Asked::Named(false), rest),
            (_, _, Some(rest), _) => (Asked::Protected, rest),
            (_, _, _, Some(rest)) => (Asked::Unasked, rest),
            (None, None, None, None) => {
                // Any other scheme is another product's address, not this one's.
                if typed.contains("://") {
                    return Err(AddressError::Shape);
                }
                (Asked::Unasked, typed)
            }
        };
        let parts = parts_of(rest)?;
        let options = match asked {
            Asked::Named(secure) => {
                if !secure && !is_local_authority(&parts.host) {
                    return Err(AddressError::Unprotected);
                }
                vec![address(secure, &parts)]
            }
            Asked::Protected => vec![address(true, &parts)],
            Asked::Unasked => {
                let mut both = vec![address(true, &parts)];
                if is_local_authority(&parts.host) {
                    both.push(address(false, &parts));
                }
                both
            }
        };
        Ok(Address(options))
    }

    /// The addresses, in the order worth trying them.
    pub fn tried(&self) -> &[ServerUrl] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secure_address_gets_the_v1_path_when_it_has_none() {
        assert_eq!(ServerUrl::parse("wss://example.invalid").unwrap().as_str(), "wss://example.invalid/v1");
        assert_eq!(
            ServerUrl::parse("wss://example.invalid:8443/v1").unwrap().as_str(),
            "wss://example.invalid:8443/v1"
        );
    }

    #[test]
    fn plain_text_is_for_this_machine_only() {
        assert!(ServerUrl::parse("ws://127.0.0.1:8443").is_ok());
        assert!(ServerUrl::parse("ws://localhost:8443/v1").is_ok());
        assert!(ServerUrl::parse("ws://[::1]:8443").is_ok());
        assert_eq!(ServerUrl::parse("ws://example.invalid"), Err(AddressError::Unprotected));
        // A person reads where a server is, not which socket it means.
        assert_eq!(ServerUrl::parse("wss://example.invalid/v1").unwrap().authority(), "example.invalid");
        assert_eq!(ServerUrl::parse("wss://example.invalid:8443/v1").unwrap().authority(), "example.invalid:8443");
        assert_eq!(ServerUrl::parse("ws://[::1]:8443/v1").unwrap().authority(), "[::1]:8443");
        assert_eq!(ServerUrl::parse("ws://127.0.0.1.example.invalid"), Err(AddressError::Unprotected));
    }

    #[test]
    fn a_typed_path_other_than_the_sockets_is_refused() {
        for path in ["screens/v1", "v2", "v1/x", "x"] {
            assert_eq!(ServerUrl::parse(&format!("wss://example.invalid/{path}")), Err(AddressError::Shape), "{path}");
            assert_eq!(Address::typed(&format!("example.invalid/{path}")), Err(AddressError::Shape), "{path}");
        }
        assert!(Address::typed("example.invalid/v1/").is_ok());
    }

    #[test]
    fn a_host_is_tried_in_its_ascii_form() {
        assert_eq!(
            Address::typed("münchen.example.invalid").unwrap().tried(),
            vec![ServerUrl::parse("wss://xn--mnchen-3ya.example.invalid/v1").unwrap()]
        );
        assert_eq!(
            Address::typed("Example.Invalid:8443").unwrap().tried()[0].as_str(),
            "wss://example.invalid:8443/v1"
        );
        assert_eq!(Address::typed("[::1]:8443").unwrap().tried()[0].as_str(), "wss://[::1]:8443/v1");
    }

    #[test]
    fn a_host_with_a_character_no_dns_name_holds_is_refused() {
        for bad in ["example.inv\u{f}lid", "`oli\u{16}\0.example.invalid", "under_score.example.invalid"] {
            assert_eq!(Address::typed(bad), Err(AddressError::Shape), "{bad:?}");
        }
    }

    #[test]
    fn a_bracketed_host_is_an_ipv6_address_or_refused() {
        assert_eq!(Address::typed("[::1]").unwrap().tried()[0].as_str(), "wss://[::1]/v1");
        for bad in ["[\u{f}]:8443", "[example.invalid]", "[::1::2]"] {
            assert_eq!(Address::typed(bad), Err(AddressError::Shape), "{bad:?}");
        }
    }

    #[test]
    fn a_host_idna_refuses_is_refused() {
        assert_eq!(Address::typed("\u{0301}a.example.invalid"), Err(AddressError::Shape));
    }

    #[test]
    fn what_is_not_an_address_is_refused() {
        for bad in
            ["", "example.invalid", "https://example.invalid", "wss://", "wss://:8443", "wss://user@example.invalid"]
        {
            assert_eq!(ServerUrl::parse(bad), Err(AddressError::Shape), "{bad}");
        }
    }

    #[test]
    fn a_typed_name_is_tried_protected_first_and_never_in_the_open() {
        assert_eq!(
            Address::typed("example.invalid").unwrap().tried(),
            vec![ServerUrl::parse("wss://example.invalid/v1").unwrap()],
            "nothing typed about a name says it is not a TLS server"
        );
        assert_eq!(
            Address::typed("example.invalid:8443").unwrap().tried(),
            vec![ServerUrl::parse("wss://example.invalid:8443/v1").unwrap()]
        );
    }

    #[test]
    fn a_host_on_this_machine_is_also_worth_trying_plainly_second() {
        assert_eq!(
            Address::typed("127.0.0.1:8443").unwrap().tried(),
            vec![
                ServerUrl::parse("wss://127.0.0.1:8443/v1").unwrap(),
                ServerUrl::parse("ws://127.0.0.1:8443/v1").unwrap(),
            ],
            "protected first: what a person types may well be a TLS server"
        );
        for here in ["[::1]:8443", "localhost:8443", "127.0.0.1"] {
            assert_eq!(Address::typed(here).unwrap().tried().len(), 2, "{here}");
        }
        assert_eq!(
            Address::typed("example.invalid").unwrap().tried().len(),
            1,
            "a name elsewhere never gets a plain candidate"
        );
    }

    #[test]
    fn typing_a_secure_address_claims_it_and_offers_nothing_plain() {
        assert_eq!(
            Address::typed("https://127.0.0.1:8443").unwrap().tried(),
            vec![ServerUrl::parse("wss://127.0.0.1:8443/v1").unwrap()],
            "https is a claim about that server, so no plain fallback"
        );
        assert_eq!(
            Address::typed("wss://example.invalid").unwrap().tried(),
            vec![ServerUrl::parse("wss://example.invalid/v1").unwrap()]
        );
    }

    #[test]
    fn typing_a_scheme_overrides_and_asks_for_exactly_that_address() {
        assert_eq!(
            Address::typed("ws://127.0.0.1:8443").unwrap().tried(),
            vec![ServerUrl::parse("ws://127.0.0.1:8443/v1").unwrap()],
            "a script's override gets no extra candidate"
        );
        assert_eq!(Address::typed("ws://example.invalid"), Err(AddressError::Unprotected));
    }

    #[test]
    fn typing_http_claims_nothing_about_protection() {
        assert_eq!(
            Address::typed("http://127.0.0.1:8443").unwrap().tried(),
            vec![
                ServerUrl::parse("wss://127.0.0.1:8443/v1").unwrap(),
                ServerUrl::parse("ws://127.0.0.1:8443/v1").unwrap(),
            ]
        );
        assert_eq!(
            Address::typed("http://example.invalid").unwrap().tried(),
            vec![ServerUrl::parse("wss://example.invalid/v1").unwrap()],
            "elsewhere, http still cannot mean the open"
        );
    }

    #[test]
    fn a_paste_is_cleaned_up_before_it_is_tried() {
        let spaced = Address::typed("  example.invalid  ").unwrap();
        let barred = Address::typed("example.invalid/").unwrap();
        let named = Address::typed("example.invalid/v1").unwrap();
        assert_eq!(spaced.tried(), named.tried());
        assert_eq!(barred.tried(), named.tried());
        assert_eq!(
            Address::typed(" 127.0.0.1:8443/v1 ").unwrap().tried(),
            vec![
                ServerUrl::parse("wss://127.0.0.1:8443/v1").unwrap(),
                ServerUrl::parse("ws://127.0.0.1:8443/v1").unwrap(),
            ]
        );
    }

    #[test]
    fn what_a_person_cannot_mean_is_refused() {
        for bad in [
            "",
            "   ",
            "example.invalid:84xx",
            "user@",
            "@example.invalid",
            "wss://user@127.0.0.1",
            "ftp://example.invalid",
            "example.invalid?q=1",
            "example.invalid#top",
            "a b",
            "example.invalid:",
            "wss://",
            "https://",
            "://example.invalid",
            "example.invalid]",
            "example.invalid:99999",
            "example.invalid:65536",
            "example.invalid:0",
            "[::1]:70000",
        ] {
            assert_eq!(Address::typed(bad), Err(AddressError::Shape), "{bad}");
        }
    }

    #[test]
    fn an_address_longer_than_a_host_can_be_is_refused_not_cut() {
        // The longest DNS name, 253 bytes, and slashes up to the bound, which the path reads as none.
        let name =
            [&"a".repeat(63), &"a".repeat(63), &"a".repeat(63), &"a".repeat(61)].map(|label| label.as_str()).join(".");
        let long = format!("{name}{}", "/".repeat(MAX_ADDRESS_BYTES - name.len()));
        assert!(Address::typed(&long).is_ok(), "the bound itself is an address");
        assert_eq!(Address::typed(&format!("{long}/")), Err(AddressError::Shape));
    }

    #[test]
    fn no_address_a_person_reads_shows_this_crates_own_scheme() {
        for said in [AddressError::Shape.to_string(), AddressError::Unprotected.to_string()] {
            assert!(!said.contains("://"), "the scheme is internal: {said}");
        }
        assert!(
            AddressError::Shape.to_string().contains("example.invalid"),
            "and the refusal says what to type instead"
        );
    }
}
