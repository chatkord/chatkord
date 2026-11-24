//! The address guard: a connection for a preview goes only to addresses that are globally routable, so a link
//! pasted into a draft cannot make the sender's client knock on its own machine, its network or its router.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Why a set of addresses was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The name resolved to nothing.
    Unresolved,
    /// At least one address is not globally routable.
    NotGlobal,
}

/// Addresses every one of which is globally routable. Only [`vet`] makes one, and a connector takes nothing else, so
/// no connection is opened to an address nobody checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vetted(Vec<SocketAddr>);

impl Vetted {
    /// The addresses, in the order the resolver gave them; never none.
    pub fn addresses(&self) -> &[SocketAddr] {
        &self.0
    }
}

/// `found` as [`Vetted`], when it is not empty and every address is globally routable: one private address in a
/// name's answer refuses the name, as a rebinding host mixes them.
pub fn vet(found: Vec<SocketAddr>) -> Result<Vetted, Refused> {
    if found.is_empty() {
        return Err(Refused::Unresolved);
    }
    if !found.iter().all(|address| is_global(address.ip())) {
        return Err(Refused::NotGlobal);
    }
    Ok(Vetted(found))
}

/// Whether `ip` is a public unicast address, an embedded IPv4 address judged as itself.
pub(crate) fn is_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => global_v4(v4),
        IpAddr::V6(v6) => global_v6(v6),
    }
}

fn global_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let refused = a == 0 // "this network", the unspecified address included
        || a == 10
        || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // documentation
        || (a == 192 && b == 88 && c == 99) // 6to4 relay anycast
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113) // documentation
        || a >= 224; // multicast, reserved, and the broadcast address
    !refused
}

fn global_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    // ::ffff:a.b.c.d: an IPv4 host behind an IPv6 socket.
    if let Some(embedded) = ip.to_ipv4_mapped() {
        return global_v4(embedded);
    }
    // 64:ff9b::/96, NAT64: the gateway connects to the IPv4 address it carries.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return global_v4(v4_of(s[6], s[7]));
    }
    // 2002::/16, 6to4: the IPv4 address follows the prefix.
    if s[0] == 0x2002 {
        return global_v4(v4_of(s[1], s[2]));
    }
    // Only 2000::/3 is global unicast; this leaves out ::/96 (unspecified, loopback, IPv4-compatible), ULA, link-local,
    // site-local, multicast and 64:ff9b:1::/48.
    let unicast = s[0] & 0xe000 == 0x2000;
    let documentation = (s[0] == 0x2001 && s[1] == 0x0db8) || (s[0] == 0x3fff && s[1] & 0xf000 == 0);
    let protocol_assignments = s[0] == 0x2001 && s[1] & 0xfe00 == 0; // 2001::/23, Teredo among them
    unicast && !documentation && !protocol_assignments
}

fn v4_of(high: u16, low: u16) -> Ipv4Addr {
    let [a, b] = high.to_be_bytes();
    let [c, d] = low.to_be_bytes();
    Ipv4Addr::new(a, b, c, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(text: &str) -> SocketAddr {
        SocketAddr::new(text.parse().unwrap(), 443)
    }

    #[test]
    fn each_class_that_is_not_global_is_refused() {
        for text in [
            "0.0.0.0",
            "0.1.2.3",
            "10.1.2.3",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "127.255.0.9",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.1",
            "192.0.0.8",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.1",
            "198.51.100.7",
            "203.0.113.7",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "3fff::1",
            "2001::1",
            "2001:1::1",
        ] {
            assert_eq!(vet(vec![address(text)]), Err(Refused::NotGlobal), "{text}");
        }
    }

    #[test]
    fn the_neighbours_of_those_classes_are_global() {
        for text in [
            "1.1.1.1",
            "8.8.8.8",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.255.255",
            "172.32.0.1",
            "198.17.0.1",
            "198.20.0.1",
            "223.255.255.255",
            "2606:4700::1111",
            "2001:4860:4860::8888",
            "2001:200::1",
        ] {
            assert!(vet(vec![address(text)]).is_ok(), "{text}");
        }
    }

    #[test]
    fn an_ipv4_address_inside_an_ipv6_one_is_judged_as_itself() {
        for text in [
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:7f00:1",
            "::127.0.0.1",
            "::10.0.0.1",
            "::8.8.8.8",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "64:ff9b::192.168.0.1",
            "64:ff9b:1::1",
            "2002:7f00:1::",
            "2002:a00:1::1",
        ] {
            assert_eq!(vet(vec![address(text)]), Err(Refused::NotGlobal), "{text}");
        }
        for text in ["::ffff:8.8.8.8", "64:ff9b::808:808", "2002:808:808::1"] {
            assert!(vet(vec![address(text)]).is_ok(), "{text}");
        }
    }

    #[test]
    fn one_refused_address_among_global_ones_refuses_the_name() {
        let mixed = vec![address("8.8.8.8"), address("10.0.0.5"), address("2606:4700::1111")];
        assert_eq!(vet(mixed), Err(Refused::NotGlobal));
        let global = vec![address("8.8.8.8"), address("2606:4700::1111")];
        assert_eq!(vet(global.clone()).map(|vetted| vetted.addresses().to_vec()), Ok(global));
    }

    #[test]
    fn a_name_that_resolved_to_nothing_is_refused() {
        assert_eq!(vet(Vec::new()), Err(Refused::Unresolved));
    }
}
