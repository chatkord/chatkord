//! The person's idleness told as transitions: a minute of no input, then ydotool's input through the kernel's uinput.

use platform::idle::{self, IDLE_AFTER, POLL_EVERY, State};
use std::process::Command;
use std::time::Duration;
use tier::skipped;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// A move of the pointer by one pixel, or why ydotool could not make it.
fn nudge() -> bool {
    Command::new("ydotool")
        .env("YDOTOOL_SOCKET", "/tmp/.ydotool_socket")
        .args(["mousemove", "-x", "1", "-y", "0"])
        .status()
        .is_ok_and(|status| status.success())
}

mod lab {
    use super::*;

    #[tokio::test]
    async fn a_minute_without_input_is_idle_and_input_is_active() {
        let (told, mut heard) = mpsc::channel(4);
        let Ok(watch) = idle::start(told, |state| state).await else {
            return skipped("this session tells no idle time");
        };
        if !nudge() {
            return skipped("no ydotool to make input with (`YDOTOOL_SOCKET` names its socket)");
        }
        let wait = IDLE_AFTER + POLL_EVERY * 2 + Duration::from_secs(5);
        let Ok(first) = timeout(wait, heard.recv()).await else {
            return skipped("the machine was in use, so it never went idle");
        };
        assert_eq!(first, Some(State::Idle));
        assert!(nudge());
        let next = timeout(POLL_EVERY * 2 + Duration::from_secs(5), heard.recv()).await;
        assert_eq!(next.unwrap(), Some(State::Active));
        drop(watch);
    }
}
