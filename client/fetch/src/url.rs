//! The address a preview fetches: a link as `proto::rules` reads one, narrowed to `https`, port 443, no user, a
//! host that is a public-looking name or a literal address, and no fragment.

use proto::rules::LinkTarget;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The longest address followed, a page's or a redirect's.
const MAX_URL: usize = 2048;

/// The only port a preview connects to.
pub(crate) const PORT: u16 = 443;

/// Why an address was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UrlRefused;

/// Whom an address names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Host {
    /// A name, lower case, ASCII, every label non-empty, its last one starting with a letter, so no number a
    /// resolver might read as an address passes for a name.
    Name(String),
    /// A literal address.
    Ip(IpAddr),
}

/// An `https` address on port 443.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Url {
    host: Host,
    target: String,
}

impl Url {
    /// `text` as an address: `https://` in any case, then a host, then optionally `:443`, a path and a query, within
    /// [`MAX_URL`]; the fragment is dropped. A user, another port, a space, a control or non-ASCII byte is refused.
    pub(crate) fn parse(text: &str) -> Result<Url, UrlRefused> {
        if text.len() > MAX_URL {
            return Err(UrlRefused);
        }
        let link = LinkTarget::parse(text).ok_or(UrlRefused)?;
        if !link.is_https() || link.has_userinfo() || link.port().is_some_and(|port| port != PORT) {
            return Err(UrlRefused);
        }
        let host = Url::host(link.host())?;
        let tail = link.rest().split_once('#').map_or(link.rest(), |(before, _)| before);
        let target = match tail {
            "" => "/".to_string(),
            tail if tail.starts_with('?') => format!("/{tail}"),
            tail => tail.to_string(),
        };
        if !target.bytes().all(|byte| (0x21..0x7f).contains(&byte)) {
            return Err(UrlRefused);
        }
        Ok(Url { host, target })
    }

    fn host(host: &str) -> Result<Host, UrlRefused> {
        if let Some(inside) = host.strip_prefix('[').and_then(|host| host.strip_suffix(']')) {
            return inside.parse::<Ipv6Addr>().map(|ip| Host::Ip(IpAddr::V6(ip))).map_err(|_| UrlRefused);
        }
        if let Ok(ip) = host.parse::<Ipv4Addr>() {
            return Ok(Host::Ip(IpAddr::V4(ip)));
        }
        let name = host.to_ascii_lowercase();
        let labels_ok = name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        });
        let last_ok =
            name.rsplit('.').next().and_then(|label| label.bytes().next()).is_some_and(|b| b.is_ascii_alphabetic());
        if name.len() > 253 || !labels_ok || !last_ok {
            return Err(UrlRefused);
        }
        Ok(Host::Name(name))
    }

    /// Where `reference` leads from here: an absolute `https` address, one without a scheme (`//host/x`), a path on
    /// this host (`/x`), a query or a path beside this one. Any other scheme is refused.
    pub(crate) fn join(&self, reference: &str) -> Result<Url, UrlRefused> {
        let reference = reference.trim_matches(|c: char| c.is_ascii_whitespace());
        let first_segment = reference.split(['/', '?', '#']).next().unwrap_or_default();
        if reference.get(..8).is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://")) {
            return Url::parse(reference);
        }
        if first_segment.contains(':') {
            return Err(UrlRefused);
        }
        let authority = self.authority();
        let (path, _) = self.target.split_once('?').unwrap_or((&self.target, ""));
        let text = if reference.starts_with("//") {
            format!("https:{reference}")
        } else if reference.starts_with('/') {
            format!("https://{authority}{reference}")
        } else if reference.starts_with('?') {
            format!("https://{authority}{path}{reference}")
        } else if reference.is_empty() || reference.starts_with('#') {
            format!("https://{authority}{}", self.target)
        } else {
            let directory = path.rsplit_once('/').map_or("/", |(before, _)| before);
            format!("https://{authority}{directory}/{reference}")
        };
        Url::parse(&text)
    }

    /// Whom it names.
    pub(crate) fn host_kind(&self) -> &Host {
        &self.host
    }

    /// The host as a request's `Host` and a certificate's name carry it: a literal IPv6 address in brackets.
    pub(crate) fn authority(&self) -> String {
        match &self.host {
            Host::Name(name) => name.clone(),
            Host::Ip(IpAddr::V4(ip)) => ip.to_string(),
            Host::Ip(IpAddr::V6(ip)) => format!("[{ip}]"),
        }
    }

    /// The host as a name for a certificate: a literal IPv6 address without its brackets.
    pub(crate) fn server_name(&self) -> String {
        match &self.host {
            Host::Name(name) => name.clone(),
            Host::Ip(ip) => ip.to_string(),
        }
    }

    /// Path and query, as a request's target.
    pub(crate) fn target(&self) -> &str {
        &self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(text: &str) -> String {
        let url = Url::parse(text).unwrap();
        format!("{}{}", url.authority(), url.target())
    }

    #[test]
    fn an_https_address_on_port_443_is_taken() {
        assert_eq!(target("https://Example.INVALID"), "example.invalid/");
        assert_eq!(target("HTTPS://example.invalid:443/a/b?c=d#top"), "example.invalid/a/b?c=d");
        assert_eq!(target("https://example.invalid?q=1"), "example.invalid/?q=1");
        assert_eq!(target("https://[2001:db8::1]/x"), "[2001:db8::1]/x");
        assert_eq!(Url::parse("https://8.8.8.8/").unwrap().host_kind(), &Host::Ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn anything_else_is_refused() {
        let long = format!("https://example.invalid/{}", "a".repeat(MAX_URL));
        for text in [
            "",
            "http://example.invalid/",
            "ftp://example.invalid/",
            "example.invalid/",
            "//example.invalid/",
            "https://",
            "https:///x",
            "https://user@example.invalid/",
            "https://user:pw@example.invalid/",
            "https://example.invalid:8443/",
            "https://example.invalid:/",
            "https://example.invalid:443x/",
            "https://exa mple.invalid/",
            "https://example.invalid/a b",
            "https://example.invalid/a\tb",
            "https://example.invalid/\u{e9}",
            "https://\u{e9}xample.invalid/",
            "https://example..invalid/",
            "https://.example.invalid/",
            "https://[::1/",
            "https://[not-an-address]/",
            "https://2130706433/",
            "https://127.1/",
            "https://0x7f.1/",
            "https://999.1.1.1/",
            "https://example.invalid@evil.invalid/",
            "https://[::1]:80/",
            "https://[::1]x/",
            long.as_str(),
        ] {
            assert_eq!(Url::parse(text), Err(UrlRefused), "{text:?}");
        }
    }

    #[test]
    fn a_reference_is_resolved_against_the_page() {
        let page = Url::parse("https://example.invalid/a/b?x=1").unwrap();
        let at = |reference: &str| page.join(reference).map(|url| format!("{}{}", url.authority(), url.target()));
        assert_eq!(at("https://cdn.example.invalid/i.png").unwrap(), "cdn.example.invalid/i.png");
        assert_eq!(at("//cdn.example.invalid/i.png").unwrap(), "cdn.example.invalid/i.png");
        assert_eq!(at("/i.png").unwrap(), "example.invalid/i.png");
        assert_eq!(at("i.png").unwrap(), "example.invalid/a/i.png");
        assert_eq!(at("?y=2").unwrap(), "example.invalid/a/b?y=2");
        assert_eq!(at("").unwrap(), "example.invalid/a/b?x=1");
        for refused in [
            "http://example.invalid/i.png",
            "//example.invalid:8443/i.png",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            "ftp://example.invalid/i.png",
            "/a b",
        ] {
            assert_eq!(page.join(refused), Err(UrlRefused), "{refused:?}");
        }
    }
}
