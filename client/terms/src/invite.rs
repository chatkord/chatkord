//! An invite link in any shape a person passes it on: `host[:port]/join#<code>` or `/join/<code>`, with or without
//! the `http://` or `https://` a browser writes. The server half is read as a typed address is, so a link names
//! nothing an address could not.

use crate::address::{Address, AddressError};
use crate::secret::Secret;
use proto::rules::token_shaped;

/// The schemes a link may start with, in lower case, which an address is typed with.
const SCHEMES: [&str; 2] = ["https://", "http://"];

/// The word after the host that makes a link an invite, then a `/` or a `#` before the code.
const JOIN: &str = "join";

/// The most bytes a pasted link may have: more than any host and code together, so a longer one is refused.
const MAX_LINK_BYTES: usize = 512;

/// A parsed invite link. The code is a secret: it admits whoever holds it.
#[derive(Debug)]
pub struct InviteLink {
    /// The server it admits to, as the host it names would be typed.
    pub address: Address,
    /// The `host[:port]` the link names, as a person reads it.
    pub host: String,
    /// The invite's code, token-shaped.
    pub code: Secret,
}

/// The three parts of text shaped like an invite, none of them checked beyond that shape.
struct Shape<'a> {
    /// The scheme as an address is typed with it, lower case, or none.
    scheme: &'static str,
    /// What stands before the first `/`, never empty.
    host: &'a str,
    /// What follows `/join/` or `/join#`, without one trailing `/`.
    code: &'a str,
}

impl<'a> Shape<'a> {
    /// `text` read as `[scheme]host/join/code` or `[scheme]host/join#code`, a trailing slash allowed, or none when
    /// it is not shaped so.
    fn of(text: &'a str) -> Option<Self> {
        let (scheme, rest) = SCHEMES
            .iter()
            .find_map(|scheme| {
                let head = text.get(..scheme.len())?;
                head.eq_ignore_ascii_case(scheme).then(|| (*scheme, text.get(scheme.len()..).unwrap_or_default()))
            })
            .unwrap_or(("", text));
        let (host, path) = rest.split_once('/')?;
        let code = path.strip_prefix(JOIN)?.strip_prefix(['/', '#'])?;
        (!host.is_empty()).then(|| Shape { scheme, host, code: code.strip_suffix('/').unwrap_or(code) })
    }
}

impl InviteLink {
    /// Whether pasted text means to be an invite link, so a malformed one is said to be one and not an address.
    pub fn claimed(text: &str) -> bool {
        Shape::of(text.trim()).is_some()
    }

    /// Reads `[http://|https://]HOST[:PORT]/join/CODE` or `…/join#CODE`, a trailing slash allowed, at most 512
    /// bytes; any other shape is refused, as is a host that is not an address. A scheme means what it does to a
    /// typed address: `https` protected only, `http` or none protected, then plain for this machine.
    pub fn parse(text: &str) -> Result<Self, AddressError> {
        let text = text.trim();
        let shape = Shape::of(text).filter(|_| text.len() <= MAX_LINK_BYTES).ok_or(AddressError::Shape)?;
        if !token_shaped(shape.code) {
            return Err(AddressError::Shape);
        }
        let address = Address::typed(&format!("{}{}", shape.scheme, shape.host))?;
        Ok(InviteLink { address, host: shape.host.to_string(), code: Secret::new(shape.code.to_string()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code() -> String {
        "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6)
    }

    fn tried(link: &InviteLink) -> Vec<&str> {
        link.address.tried().iter().map(|url| url.as_str()).collect()
    }

    #[test]
    fn a_link_names_the_server_to_try_and_holds_the_code_as_a_secret() {
        let link = InviteLink::parse(&format!("  https://books.example.invalid:8443/join#{}  ", code())).unwrap();
        assert_eq!(link.host, "books.example.invalid:8443");
        assert_eq!(tried(&link), ["wss://books.example.invalid:8443/v1"], "a host elsewhere is tried encrypted only");
        assert_eq!(link.code.expose(), code());
        assert_eq!(format!("{link:?}").matches(&code()).count(), 0, "the code never prints");
    }

    #[test]
    fn an_invite_is_known_by_its_shape_whatever_the_scheme_path_or_spacing_around_it() {
        let code = code();
        let elsewhere = ["wss://books.example.invalid/v1"];
        for (text, host, tries) in [
            (format!("books.example.invalid/join#{code}"), "books.example.invalid", &elsewhere[..]),
            (format!("books.example.invalid/join/{code}"), "books.example.invalid", &elsewhere[..]),
            (format!("https://books.example.invalid/join#{code}"), "books.example.invalid", &elsewhere[..]),
            (format!("HTTPS://books.example.invalid/join/{code}"), "books.example.invalid", &elsewhere[..]),
            (format!("http://books.example.invalid/join#{code}"), "books.example.invalid", &elsewhere[..]),
            (format!("Http://books.example.invalid/join/{code}/"), "books.example.invalid", &elsewhere[..]),
            (format!("books.example.invalid/join#{code}/"), "books.example.invalid", &elsewhere[..]),
            (format!(" \t books.example.invalid/join/{code} \n"), "books.example.invalid", &elsewhere[..]),
            (
                format!("books.example.invalid:8443/join#{code}"),
                "books.example.invalid:8443",
                &["wss://books.example.invalid:8443/v1"][..],
            ),
            (
                format!("münchen.example.invalid/join#{code}"),
                "münchen.example.invalid",
                &["wss://xn--mnchen-3ya.example.invalid/v1"][..],
            ),
            (
                format!("127.0.0.1:18443/join#{code}"),
                "127.0.0.1:18443",
                &["wss://127.0.0.1:18443/v1", "ws://127.0.0.1:18443/v1"][..],
            ),
            (
                format!("http://127.0.0.1:18443/join/{code}"),
                "127.0.0.1:18443",
                &["wss://127.0.0.1:18443/v1", "ws://127.0.0.1:18443/v1"][..],
            ),
            (format!("https://127.0.0.1:18443/join#{code}"), "127.0.0.1:18443", &["wss://127.0.0.1:18443/v1"][..]),
        ] {
            let link = InviteLink::parse(&text).unwrap_or_else(|refused| panic!("{text}: {refused:?}"));
            assert_eq!((link.host.as_str(), tried(&link)), (host, tries.to_vec()), "{text}");
            assert_eq!(link.code.expose(), code, "{text}");
            assert!(InviteLink::claimed(&text), "{text}");
        }
    }

    #[test]
    fn the_link_a_server_writes_reads_back_tried_as_its_address_would_be() {
        let code = code();
        let elsewhere = InviteLink::parse(&proto::envelope::join_link("books.example.invalid:8443", &code)).unwrap();
        assert_eq!(tried(&elsewhere), ["wss://books.example.invalid:8443/v1"], "protected only");
        let here = InviteLink::parse(&proto::envelope::join_link("127.0.0.1:18443", &code)).unwrap();
        assert_eq!(tried(&here), ["wss://127.0.0.1:18443/v1", "ws://127.0.0.1:18443/v1"], "plainly too, here only");
        assert_eq!((here.host.as_str(), here.code.expose()), ("127.0.0.1:18443", code.as_str()));
    }

    #[test]
    fn anything_else_is_refused_rather_than_guessed_at() {
        let good = code();
        for bad in [
            format!("books.example.invalid/join/{}", good.get(1..).expect("a code is ASCII")),
            format!("books.example.invalid/join/{good}x"),
            format!("books.example.invalid/invite/{good}"),
            format!("books.example.invalid/v1/join/{good}"),
            format!("books.example.invalid/x/join#{good}"),
            format!("us@books.example.invalid/join/{good}"),
            format!("https://us@books.example.invalid/join#{good}"),
            format!("/join/{good}"),
            format!("https:///join#{good}"),
            format!("ws://books.example.invalid/join/{good}"),
            format!("wss://books.example.invalid/join#{good}"),
            format!("chatkord://books.example.invalid/join/{good}"),
            format!("books.example.invalid/join/{good}?x=1"),
            format!("books.example.invalid/join#{good}#x"),
            format!("books.example.invalid/joined/{good}"),
            format!("books.example.invalid/join{good}"),
            format!("books.example.invalid/join/{}", "+".repeat(proto::rules::TOKEN_LEN)),
            format!("{}/join/{good}", "a".repeat(MAX_LINK_BYTES)),
            format!("http://http://books.example.invalid/join/{good}"),
        ] {
            assert!(InviteLink::parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_host_idna_refuses_is_refused() {
        assert!(InviteLink::parse(&format!("\u{0301}a.example.invalid/join#{}", code())).is_err());
    }

    #[test]
    fn a_paste_says_it_is_a_link_by_the_join_after_a_host_and_a_malformed_one_is_still_claimed() {
        assert!(InviteLink::claimed(" HTTPS://x/join/y"));
        assert!(InviteLink::claimed("x/join#y"));
        assert!(InviteLink::claimed(&format!("us@x/join#{}", code())), "refused by parse, so said to be a bad invite");
        assert!(!InviteLink::claimed("books.example.invalid"));
        assert!(!InviteLink::claimed("books.example.invalid/v1"));
        assert!(!InviteLink::claimed("/join/y"));
        assert!(!InviteLink::claimed("chatkord://x/join/y"));
    }
}
