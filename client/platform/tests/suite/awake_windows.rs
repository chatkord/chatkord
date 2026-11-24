//! The hold on the display and the machine, taken and released as a power request, as the system reports the execution
//! state it makes of every request.

use platform::awake::Hold;
use screens::Awake;
use std::time::Duration;
use tier::skipped;
use windows_sys::Win32::System::Power::{
    CallNtPowerInformation, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SystemExecutionState,
};

/// The execution state the system holds now: the bits of what is required, from every request made.
fn required() -> u32 {
    let mut state = 0u32;
    // SAFETY: no input; `state` is a live u32 and the output length says so.
    let status = unsafe {
        CallNtPowerInformation(
            SystemExecutionState,
            std::ptr::null(),
            0,
            (&raw mut state).cast(),
            u32::try_from(size_of::<u32>()).unwrap(),
        )
    };
    assert_eq!(status, 0, "the system tells its execution state");
    state & (ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)
}

/// Waits until the system requires exactly `want`, or says what it required last.
async fn becomes(want: u32) -> Result<(), u32> {
    let mut seen = required();
    for _ in 0..100 {
        seen = required();
        if seen == want {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(seen)
}

mod lab {
    use super::*;

    #[tokio::test]
    async fn a_display_hold_requires_both_a_system_hold_only_the_machine_and_free_releases_both() {
        if becomes(0).await.is_err() {
            return skipped("something else holds the machine awake");
        }
        let (hold, task) = Hold::new();
        let task = tokio::spawn(task);
        hold.keep(Awake::Display, "In a call");
        assert_eq!(becomes(ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED).await, Ok(()));
        hold.keep(Awake::System, "In a call");
        assert_eq!(becomes(ES_SYSTEM_REQUIRED).await, Ok(()));
        hold.keep(Awake::Free, "In a call");
        assert_eq!(becomes(0).await, Ok(()));
        hold.keep(Awake::Display, "In a call");
        assert_eq!(becomes(ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED).await, Ok(()));
        drop(hold);
        assert!(task.await.unwrap().is_ok());
        assert_eq!(becomes(0).await, Ok(()), "a dropped hold keeps nothing");
    }
}
