//! An uninstall's clean slate: every remembered account's token, the cache and the folder go, and a keyring that
//! refuses stops it before anything goes. The default store is process-wide, so each test names its own (nextest runs each
//! test in a process of its own).

use std::path::{Path, PathBuf};

use platform::{Platform, Unavailable, forget, instance, keyring, state};
use terms::{Secret, ServerUrl};

const ADA: &str = "ada";
const BEA: &str = "bea";
const HERE: &str = "wss://example.invalid";
const THERE: &str = "wss://example.invalid:8443";

/// A device that remembers two accounts with their tokens and one cached file, its folders under a scratch one.
fn remembered(name: &str) -> (Platform, PathBuf, PathBuf, Vec<ServerUrl>) {
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    let scratch = std::env::temp_dir().join(format!("chatkord-forget-{name}-{}", std::process::id()));
    let (config, cached) = (scratch.join("config"), scratch.join("cache"));
    let platform = Platform::default().config_in(config.clone()).cache_in(cached.clone());
    let servers = vec![ServerUrl::parse(HERE).unwrap(), ServerUrl::parse(THERE).unwrap()];
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    for (server, identity) in servers.iter().zip([ADA, BEA]) {
        runtime.block_on(state::remember_account(&platform, server.clone(), identity.into())).unwrap();
        keyring::store(server, identity, None, &Secret::new(format!("token of {identity}"))).unwrap();
    }
    std::fs::create_dir_all(cached.join(channel::THIS.id).join("files")).unwrap();
    std::fs::write(cached.join(channel::THIS.id).join("files").join("kept"), b"a picture").unwrap();
    (platform, config, cached, servers)
}

fn held(config: &Path) -> instance::Instance {
    instance::claim_in(&config.join(channel::THIS.id), None).unwrap()
}

#[test]
fn every_token_the_cache_and_the_folder_go() {
    let (platform, config, cached, servers) = remembered("all");

    forget::everything(&platform, held(&config)).unwrap();

    for (server, identity) in servers.iter().zip([ADA, BEA]) {
        assert!(keyring::load(server, identity).unwrap().is_none(), "{identity}'s token stayed");
    }
    assert!(!config.join(channel::THIS.id).exists(), "the folder stayed");
    assert!(!cached.join(channel::THIS.id).exists(), "the cache stayed");
    let _ = std::fs::remove_dir_all(config.parent().unwrap());
}

#[test]
fn a_keyring_that_refuses_leaves_everything_in_place() {
    let (platform, config, cached, servers) = remembered("refused");
    let store = keyring_core::unset_default_store().unwrap();

    assert_eq!(forget::everything(&platform, held(&config)).unwrap_err(), Unavailable);

    keyring_core::set_default_store(store);
    for (server, identity) in servers.iter().zip([ADA, BEA]) {
        assert!(keyring::load(server, identity).unwrap().is_some(), "{identity}'s token went");
    }
    assert!(config.join(channel::THIS.id).join(platform::kept::FILE).exists(), "the folder went");
    assert!(cached.join(channel::THIS.id).exists(), "the cache went");
    let _ = std::fs::remove_dir_all(config.parent().unwrap());
}
