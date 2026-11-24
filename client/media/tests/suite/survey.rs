//! `survey` from several threads at once, each making devices in the Vulkan loader, which two threads in it at the
//! same moment crash.

use crate::common;

use common::skipped;
use media::{Encoder, survey};
use std::sync::Barrier;
use std::thread;

const THREADS: usize = 16;
const ROUNDS: usize = 20;

fn names(encoders: &[Encoder]) -> Vec<(media::Codec, media::Path)> {
    encoders.iter().map(|e| (e.codec(), e.path())).collect()
}

mod lab {
    use super::*;

    #[test]
    fn surveys_from_many_threads_at_once_agree_and_do_not_crash() {
        let alone = names(&survey().encoders);
        if alone.is_empty() {
            skipped("no hardware encoder opens on this machine, so there is no device to open twice");
            return;
        }
        for _ in 0..ROUNDS {
            let start = Barrier::new(THREADS);
            let answers: Vec<_> = thread::scope(|scope| {
                let threads: Vec<_> = (0..THREADS)
                    .map(|_| {
                        scope.spawn(|| {
                            start.wait();
                            names(&survey().encoders)
                        })
                    })
                    .collect();
                threads.into_iter().map(|t| t.join().expect("a survey does not panic")).collect()
            });
            assert!(
                answers.iter().all(|a| *a == alone),
                "every thread finds the same encoders: {answers:?} against {alone:?}"
            );
        }
    }
}
