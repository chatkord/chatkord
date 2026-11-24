//! The token's life in a keyring: none, saved, read back, forgotten, and forgetting twice. The
//! library's in-memory store stands in for the system's, set once for each test, which runs in
//! a process of its own (nextest), because the default store is process-wide.

use keyring_core::{CredentialStore, Entry, Error, mock};
use platform::Unavailable;
use platform::keyring::{Keyring, SERVICE, forget, load, store};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};
use terms::{Secret, ServerUrl};
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn a_token_is_saved_per_account_and_server_and_forgotten() {
    keyring_core::set_default_store(keyring_core::mock::Store::new().expect("a store"));
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");
    let there = ServerUrl::parse("wss://example.invalid:8443").expect("an address");

    assert!(load(&here, "ada").expect("usable").is_none(), "nothing saved yet");
    store(&here, "ada", None, &Secret::new("token".into())).expect("saved");
    assert_eq!(load(&here, "ada").expect("usable").map(|t| t.expose().to_string()), Some("token".into()));
    assert!(load(&here, "grace").expect("usable").is_none(), "another account has its own");
    assert!(load(&there, "ada").expect("usable").is_none(), "another server has its own");

    forget(&here, "ada").expect("forgotten");
    assert!(load(&here, "ada").expect("usable").is_none());
    forget(&here, "ada").expect("forgetting what is not saved is not an error");
}

fn mock_store() -> keyring_core::Result<Arc<CredentialStore>> {
    let store: Arc<CredentialStore> = mock::Store::new()?;
    Ok(store)
}

fn gone() -> Error {
    Error::PlatformFailure("the service stopped answering".into())
}

/// A keyring whose maker fails for its first `failures` tries, then makes a fresh mock store each time, and how many
/// times it was asked.
fn keyring_ready_after(failures: usize) -> (Keyring, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    let keyring = Keyring::new();
    keyring.use_maker(Box::new(move || {
        if counted.fetch_add(1, Ordering::SeqCst) < failures { Err(gone()) } else { mock_store() }
    }));
    (keyring, asked)
}

/// Makes the next call on `identity`'s entry fail with `error`, as a store whose backend failed would.
fn fail_next_call(url: &ServerUrl, identity: &str, error: Error) {
    let entry = Entry::new(SERVICE, &format!("{identity}@{}", url.as_str())).expect("an entry");
    entry.as_any().downcast_ref::<mock::Cred>().expect("a mock credential").set_error(error);
}

fn noted<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let ring = report::Ring::new(SystemTime::now);
    let out = tracing::subscriber::with_default(tracing_subscriber::registry().with(ring.layer()), run);
    (out, ring.lines().iter().map(|line| line.as_str().to_string()).collect())
}

#[test]
fn a_keyring_that_was_not_ready_at_start_is_used_once_it_is() {
    let (keyring, asked) = keyring_ready_after(1);
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");

    assert_eq!(keyring.load(&here, "ada").unwrap_err(), Unavailable, "absent at first");
    keyring.store(&here, "ada", None, &Secret::new("token".into())).expect("present later");
    assert_eq!(keyring.load(&here, "ada").expect("usable").map(|t| t.expose().to_string()), Some("token".into()));
    assert_eq!(asked.load(Ordering::SeqCst), 2, "made again once, then kept");
}

#[test]
fn a_call_that_finds_the_backend_gone_makes_the_store_again_on_the_next() {
    let (keyring, asked) = keyring_ready_after(0);
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");
    keyring.store(&here, "ada", None, &Secret::new("token".into())).expect("saved");

    fail_next_call(&here, "ada", gone());
    assert_eq!(keyring.load(&here, "ada").unwrap_err(), Unavailable);
    assert!(keyring_core::get_default_store().is_none(), "the dead store is let go");

    assert!(keyring.load(&here, "ada").expect("a new store answers").is_none(), "the new store holds nothing");
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

#[test]
fn a_locked_keyring_keeps_its_store_for_the_person_to_unlock() {
    let (keyring, asked) = keyring_ready_after(0);
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");
    keyring.store(&here, "ada", None, &Secret::new("token".into())).expect("saved");

    fail_next_call(&here, "ada", Error::NoStorageAccess("locked".into()));
    assert_eq!(keyring.load(&here, "ada").unwrap_err(), Unavailable);
    assert!(keyring.load(&here, "ada").expect("unlocked").is_some(), "the same store, its token still there");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failing_keyring_is_noted_once_when_it_turns_and_again_only_after_it_worked() {
    let (keyring, _) = keyring_ready_after(3);
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");

    let (_, lines) = noted(|| {
        for _ in 0..3 {
            assert!(keyring.load(&here, "ada").is_err());
        }
        assert!(keyring.load(&here, "ada").expect("made at last").is_none());
        fail_next_call(&here, "ada", gone());
        assert!(keyring.load(&here, "ada").is_err());
    });
    let failed: Vec<_> = lines.iter().filter(|line| line.contains("the keyring failed")).collect();
    assert_eq!(failed.len(), 2, "once for each turn to failing, not once a call: {lines:?}");
    assert!(failed.iter().all(|line| line.contains("operation=Load  kind=PlatformFailure")), "{failed:?}");
    assert!(failed.iter().all(|line| !line.contains("example.invalid") && !line.contains("ada")), "{failed:?}");
}

#[test]
fn a_keyring_that_never_answers_holds_up_no_start_and_is_unavailable_within_its_deadline() {
    let keyring = Keyring::answering_within(Duration::from_millis(200));
    keyring.use_maker(Box::new(|| {
        loop {
            std::thread::park();
        }
    }));
    let here = ServerUrl::parse("wss://example.invalid").expect("an address");

    let started = Instant::now();
    keyring.start();
    assert!(started.elapsed() < Duration::from_millis(100), "the start waited {:?}", started.elapsed());
    let asked = Instant::now();
    assert_eq!(keyring.load(&here, "ada").unwrap_err(), Unavailable, "no answer is no keyring");
    assert!(asked.elapsed() < Duration::from_secs(5), "the call waited {:?}", asked.elapsed());

    let again = Instant::now();
    assert_eq!(keyring.store(&here, "ada", None, &Secret::new("token".into())).unwrap_err(), Unavailable);
    assert!(again.elapsed() < Duration::from_millis(100), "a later call waited {:?}", again.elapsed());
}
