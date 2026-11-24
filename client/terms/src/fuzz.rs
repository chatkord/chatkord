//! The fuzz targets' entries for what a person pastes or types about where a server is.

use crate::{Address, InviteLink, ServerUrl};
use proto::rules::token_shaped;

/// Whether every address `address` tries reads again, as a full address, to itself, with an authority a socket can
/// be opened to.
fn every_try_reads_again(address: &Address) {
    for url in address.tried() {
        assert_eq!(ServerUrl::parse(url.as_str()).as_ref(), Ok(url), "an address that does not read again");
        let authority = url.authority();
        assert!(
            !authority.is_empty() && !authority.chars().any(|c| c.is_whitespace() || c.is_control()),
            "an authority a socket cannot be opened to: {authority:?}"
        );
    }
}

/// A pasted invite link, whatever the bytes: one taken holds a token-shaped code and a host with no `/`, and each
/// address it tries reads again to itself.
pub fn invite_link(data: &[u8]) {
    let Ok(link) = InviteLink::parse(&String::from_utf8_lossy(data)) else { return };
    assert!(token_shaped(link.code.expose()), "a code that is not token-shaped");
    assert!(!link.host.contains('/'), "a host with a slash");
    every_try_reads_again(&link.address);
}

/// A typed server address, whatever the bytes: each address it tries reads again to itself, its authority non-empty
/// with no whitespace or control character.
pub fn typed_address(data: &[u8]) {
    if let Ok(address) = Address::typed(&String::from_utf8_lossy(data)) {
        every_try_reads_again(&address);
    }
}

#[cfg(test)]
mod tests {
    use super::{invite_link, typed_address};
    use std::path::{Path, PathBuf};

    fn seeds(target: &str) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
        proto::fuzz::seeds(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds"), target)
    }

    #[test]
    fn every_kept_invite_link_is_taken_only_whole() {
        for (_, input) in seeds("invite_link").expect("the seeds") {
            invite_link(&input);
        }
    }

    #[test]
    fn every_kept_typed_address_is_tried_only_as_an_address() {
        for (_, input) in seeds("typed_address").expect("the seeds") {
            typed_address(&input);
        }
    }
}
