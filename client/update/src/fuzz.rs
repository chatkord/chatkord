//! The fuzz target's entry for the one address a feed is read from.

use crate::address::{Address, Loopback, MAX_ADDRESS, Reached};

/// A typed feed address, whatever the bytes: one taken is `https`, or plain only to a loopback IP of this machine,
/// has no user and no fragment and is within the bound; and when loopback is refused it is `https` alone.
pub fn feed_address(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    for loopback in [Loopback::Refused, Loopback::Allowed] {
        let Ok(address) = Address::base(&text, loopback) else { continue };
        assert!(text.len() <= MAX_ADDRESS && !text.contains('#'), "an address past the bound or with a fragment");
        assert!(!address.authority().contains('@'), "an address with a user");
        match address.reached() {
            Reached::Https => assert_eq!(address.uri().scheme_str(), Some("https")),
            Reached::Loopback(socket) => {
                assert!(loopback == Loopback::Allowed, "a plain address taken where it is refused");
                assert!(socket.ip().is_loopback(), "a plain address that is not this machine");
                assert_eq!(address.uri().scheme_str(), Some("http"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::feed_address;
    use std::path::{Path, PathBuf};

    fn seeds(target: &str) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
        proto::fuzz::seeds(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds"), target)
    }

    #[test]
    fn every_kept_feed_address_is_taken_only_as_the_rules_allow() {
        for (_, input) in seeds("feed_address").expect("the seeds") {
            feed_address(&input);
        }
    }
}
