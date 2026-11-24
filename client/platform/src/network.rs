//! Whether this device has any way to reach a network, as the system's routing table says: GLib's network monitor
//! calls the device offline when no default route is up. Reading it is cheap but blocks, so async code calls it from
//! the blocking pool.

/// Whether the system has a default route up, for IPv4 or for IPv6. A system this module does not read says yes, never
/// "offline" on no evidence.
pub fn has_default_route() -> bool {
    routes::has_default_route()
}

#[cfg(target_os = "linux")]
mod routes {
    /// The route flag that says a route is up.
    const RTF_UP: u32 = 0x1;
    /// The route flag that says a packet for it is refused, which the kernel keeps as a default on a device with no
    /// route.
    const RTF_REJECT: u32 = 0x200;
    /// The loopback device, which no packet leaves the machine by.
    const LOOPBACK: &str = "lo";

    pub(super) fn has_default_route() -> bool {
        let read = |path| std::fs::read_to_string(path).unwrap_or_default();
        default_in_ipv4(&read("/proc/net/route")) || default_in_ipv6(&read("/proc/net/ipv6_route"))
    }

    /// Whether `/proc/net/route`'s text holds a default route that is up: destination and mask both `00000000`, on a
    /// device other than loopback. Its columns are tab-separated, the numbers hexadecimal.
    pub(super) fn default_in_ipv4(table: &str) -> bool {
        table.lines().skip(1).any(|line| {
            let mut columns = line.split_whitespace();
            let (Some(device), Some(destination), Some(flags), Some(mask)) =
                (columns.next(), columns.next(), columns.nth(1), columns.nth(3))
            else {
                return false;
            };
            let up = u32::from_str_radix(flags, 16).is_ok_and(|flags| flags & RTF_UP != 0);
            device != LOOPBACK && destination == "00000000" && mask == "00000000" && up
        })
    }

    /// Whether `/proc/net/ipv6_route`'s text holds a default route that is up and not a refusal: destination `::` with
    /// prefix length 0, on a device other than loopback. Its columns are destination, prefix length, source, source
    /// prefix length, next hop, metric, reference count, use, flags and device.
    pub(super) fn default_in_ipv6(table: &str) -> bool {
        table.lines().any(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let [destination, prefix, _, _, _, _, _, _, flags, device] = columns[..] else { return false };
            let flags = u32::from_str_radix(flags, 16).unwrap_or(0);
            device != LOOPBACK
                && destination.bytes().all(|digit| digit == b'0')
                && prefix == "00"
                && flags & RTF_UP != 0
                && flags & RTF_REJECT == 0
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{default_in_ipv4, default_in_ipv6};

        const HEADER: &str = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT";

        fn ipv4(rows: &[&str]) -> String {
            std::iter::once(HEADER).chain(rows.iter().copied()).collect::<Vec<_>>().join("\n")
        }

        #[test]
        fn an_ipv4_default_route_that_is_up_is_found_and_no_other_row_is_one() {
            let home = "eth0\t00000000\t0100A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0";
            let local = "eth0\t0000A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0";
            assert!(default_in_ipv4(&ipv4(&[local, home])));
            assert!(!default_in_ipv4(&ipv4(&[local])), "a local network is no way out");
            assert!(!default_in_ipv4(&ipv4(&[])), "a table of only its header");
            assert!(!default_in_ipv4(""), "no table at all");
            let down = "eth0\t00000000\t0100A8C0\t0002\t0\t0\t100\t00000000\t0\t0\t0";
            assert!(!default_in_ipv4(&ipv4(&[down])), "a route that is not up");
            let loopback = "lo\t00000000\t00000000\t0001\t0\t0\t0\t00000000\t0\t0\t0";
            assert!(!default_in_ipv4(&ipv4(&[loopback])), "nothing leaves by loopback");
            let narrow = "eth0\t00000000\t0100A8C0\t0003\t0\t0\t100\t000000FF\t0\t0\t0";
            assert!(!default_in_ipv4(&ipv4(&[narrow])), "a destination of zero under a mask is not everywhere");
            assert!(!default_in_ipv4(&ipv4(&["eth0\t00000000"])), "a row cut short");
        }

        #[test]
        fn an_ipv6_default_route_that_is_up_is_found_and_no_other_row_is_one() {
            let zero = "0".repeat(32);
            let row = |destination: &str, prefix: &str, flags: &str, device: &str| {
                format!("{destination} {prefix} {zero} 00 {zero} 00000400 00000001 00000000 {flags} {device}")
            };
            let default = row(&zero, "00", "00000003", "wlan0");
            let local = row("fe800000000000000000000000000000", "40", "00000001", "wlan0");
            assert!(default_in_ipv6(&format!("{local}\n{default}\n")));
            assert!(!default_in_ipv6(&format!("{local}\n")));
            assert!(!default_in_ipv6(""), "no table at all");
            assert!(!default_in_ipv6(&row(&zero, "00", "00000002", "wlan0")), "a route that is not up");
            assert!(!default_in_ipv6(&row(&zero, "00", "00200201", "wlan0")), "a refusal");
            assert!(!default_in_ipv6(&row(&zero, "00", "00000001", "lo")), "nothing leaves by loopback");
            assert!(!default_in_ipv6(&row(&zero, "01", "00000001", "wlan0")), "half the addresses is not all");
        }
    }
}

#[cfg(windows)]
mod routes {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::NetworkManagement::IpHelper::{GetBestRoute2, MIB_IPFORWARD_ROW2};
    use windows::Win32::Networking::WinSock::{ADDRESS_FAMILY, AF_INET, AF_INET6, SOCKADDR_INET};

    pub(super) fn has_default_route() -> bool {
        [AF_INET, AF_INET6].into_iter().any(best_route_is_default)
    }

    /// Whether the system's best route to the unspecified address of `family` is a default one: no prefix, and not
    /// the loopback's.
    fn best_route_is_default(family: ADDRESS_FAMILY) -> bool {
        // Zeroed whole, the unspecified address of either family, before its family is named.
        let mut destination = SOCKADDR_INET::default();
        destination.si_family = family;
        let mut route = MIB_IPFORWARD_ROW2::default();
        let mut source = SOCKADDR_INET::default();
        // SAFETY: `destination`, `route` and `source` are live locals of the types the call names, written only
        // through the pointers it is given, and the optional interface and source arguments are absent.
        let found = unsafe {
            GetBestRoute2(
                None,
                0,
                None,
                std::ptr::from_ref(&destination),
                0,
                std::ptr::from_mut(&mut route),
                std::ptr::from_mut(&mut source),
            )
        };
        found == ERROR_SUCCESS && route.DestinationPrefix.PrefixLength == 0 && !route.Loopback
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod routes {
    pub(super) fn has_default_route() -> bool {
        true
    }
}
