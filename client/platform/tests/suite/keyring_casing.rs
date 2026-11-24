//! One account gains one keyring entry, however its name was typed: the token is filed under the identity the
//! server named, since only the store knows how it folds a name. The library's in-memory store stands in for the
//! system's, in a process of its own (nextest) because that store is one global.

use platform::keyring::{forget, load, store};
use terms::{Secret, ServerUrl};

#[test]
fn a_name_typed_in_another_casing_files_the_token_under_the_name_the_server_named() {
    keyring_core::set_default_store(keyring_core::mock::Store::new().expect("a store"));
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");
    const NAMED: &str = "casing";
    const TYPED: &str = "CASING";

    assert!(load(&here, TYPED).expect("usable").is_none(), "the name typed has no entry of its own");
    store(&here, TYPED, Some(NAMED.into()), &Secret::new("token".into())).expect("saved");
    assert_eq!(
        load(&here, NAMED).expect("usable").map(|token| token.expose().to_string()),
        Some("token".into()),
        "the account signs in next time from the one entry the store named",
    );
    assert!(
        load(&here, TYPED).expect("usable").is_none(),
        "the name typed gained no second entry, so one account has one entry",
    );

    forget(&here, NAMED).expect("forgotten");
    assert!(load(&here, NAMED).expect("usable").is_none(), "and that one entry is what forget removes");
}
