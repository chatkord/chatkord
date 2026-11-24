#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test crate: a failed step is the test failing"
)]
//! The sound lab leaves nothing running after a test, whichever way the test ends.
#![cfg(target_os = "linux")]

use std::time::Duration;

use testserver::sound::Lab;
use tier::skipped;

const WAIT: Duration = Duration::from_secs(10);

/// The recorders running now that record the speaker `sink`, found by their command line.
fn recorders_of(sink: &str) -> usize {
    let Ok(processes) = std::fs::read_dir("/proc") else { return 0 };
    processes
        .filter_map(Result::ok)
        .filter_map(|process| std::fs::read(process.path().join("cmdline")).ok())
        .filter(|cmdline| {
            let args: Vec<&[u8]> = cmdline.split(|byte| *byte == 0).collect();
            args.first().is_some_and(|program| program.ends_with(b"pw-record")) && args.contains(&sink.as_bytes())
        })
        .count()
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_recording_let_go_unfinished_ends_its_recorder() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(sound) = lab.make("let_go") else { return skipped("no sound server to make devices on") };
        let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
        assert_eq!(recorders_of(&sound.sink), 1, "recording");
        drop(recording);
        assert_eq!(recorders_of(&sound.sink), 0, "a test that returns without finishing leaves no recorder");
    }
}
