#![allow(clippy::unwrap_used, clippy::panic, reason = "a test crate: a failed step is the test failing")]
//! KWin, asked over the session bus, answers which processes show an app's windows. Runs only where KWin is.

#![cfg(target_os = "linux")]

use share::{NoAnswer, ask_kwin};

use tier::skipped;

mod lab {
    use super::*;

    #[test]
    fn kwin_answers_and_an_app_with_no_window_is_an_empty_list() {
        match ask_kwin("org.example.no-such-app") {
            Ok(pids) => assert!(pids.is_empty(), "{pids:?}"),
            Err(NoAnswer::Bus) => skipped("no KWin on the session bus"),
            Err(why) => panic!("KWin is here and did not answer: {why:?}"),
        }
        assert!(matches!(ask_kwin("a\"b"), Err(NoAnswer::Script)), "an app id that is not plain words is refused");
    }
}
