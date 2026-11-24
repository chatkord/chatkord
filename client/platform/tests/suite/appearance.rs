//! Reading the system's appearance never hangs and never fails: on a machine with no settings service
//! (a CI runner, a bare session) it is the default, and on one with it, whatever the person set.

use platform::appearance::{FIRST_READ, read};
use std::time::{Duration, Instant};

#[test]
fn reading_the_appearance_returns_within_its_timeout_whatever_the_system_says() {
    let started = Instant::now();
    let _appearance = read();
    assert!(started.elapsed() < FIRST_READ + Duration::from_secs(5), "took {:?}", started.elapsed());
}
