//! A voice thread's standing with the system: asked to run ahead of ordinary work, so a machine busy with something
//! else does not keep a call's sound waiting for a core. The system decides; a thread it refuses runs on as before.

use std::sync::Once;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
use self::linux as system;
#[cfg(windows)]
use self::windows as system;

/// The calling thread's standing ahead of ordinary work, for as long as this is kept. It is let go on the thread
/// that asked, so it does not move to another.
pub struct Realtime {
    _held: Option<system::Held>,
}

/// Asks the system to run the calling thread, one that carries a call's sound, ahead of ordinary work: MMCSS's
/// "Pro Audio" task on Windows, the portal's real-time on Linux, which answers after this returns. A refusal is said
/// once in the log and changes nothing else.
pub fn realtime() -> Realtime {
    Realtime { _held: system::ask().map_err(|why| say(&why)).ok() }
}

/// Whether the system runs the calling thread ahead of ordinary work, for a test of a thread that asked.
#[cfg(test)]
pub(crate) fn ahead() -> bool {
    system::ahead()
}

/// Says the first refusal of a run: every later one has the same cause.
fn say(why: &system::Refused) {
    static SAID: Once = Once::new();
    SAID.call_once(|| report::note!(warn, "a call's sound threads run at ordinary priority", why = why));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    mod lab {
        use super::*;

        #[test]
        fn a_thread_that_asks_is_run_ahead_of_ordinary_work() {
            let raised = std::thread::spawn(|| {
                assert!(!system::ahead(), "a thread starts at ordinary priority");
                let _held = realtime();
                // The portal answers from a thread of its own; MMCSS has answered already.
                let asked = Instant::now();
                while !system::ahead() && asked.elapsed() < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                system::ahead()
            })
            .join()
            .expect("the thread ran");
            // Every Windows runs MMCSS; a Linux session may have no portal or RTKit to grant it (a remote one has not).
            if cfg!(windows) {
                assert!(raised, "MMCSS did not take the thread");
            } else if !raised {
                tier::skipped("this session is granted no real-time priority (no portal or RTKit, or a remote one)");
            }
        }
    }
}
