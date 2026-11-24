//! A screen that changes once and goes still shows that change soon, though its encoder keeps a picture in flight:
//! the share repeats it at its next wait rather than at the still screen's slow repeat.

use crate::screen_support;

use call::screen::{Fail, Frames, Progress, begin_into};
use capture::{CaptureError, Frame, Pixels};
use choices::ShareQuality;
use screen_support::{candidates_of, layout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tier::skipped;

/// A screen that shows one picture and then nothing new.
struct Once {
    surface: screen_support::Surface,
    shown: AtomicBool,
}

// SAFETY: one thread at a time uses the surface's FFmpeg objects: the one that encodes, after the share began.
unsafe impl Send for Once {}

impl Frames for Once {
    fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        if self.shown.swap(true, Ordering::Relaxed) {
            crate::clock::frame_clock(wait);
            return Ok(None);
        }
        Ok(Some(self.surface.frame(Pixels::Bgrx)))
    }

    fn dropped(&self) -> u64 {
        0
    }
}

mod lab {
    use super::*;

    /// Under half the still screen's repeat (500 ms): the change waits one share wait (100 ms), not a repeat.
    const SOON: Duration = Duration::from_millis(250);

    #[tokio::test(flavor = "multi_thread")]
    async fn a_screen_gone_still_shows_its_last_change_at_the_next_wait() {
        let mut shared = 0;
        let hardware = candidates_of(media::Codec::H264).into_iter().filter(|(e, _)| e.path() != media::Path::Software);
        for (encoder, canvas) in hardware {
            let surveyed = screen_support::kept(media::Report { encoders: vec![encoder.clone()], unavailable: vec![] });
            let (width, height) = (1280, 720);
            let out = tokio::task::spawn_blocking(move || {
                let screen = |session: &media::Session| match canvas.surface(session, width, height) {
                    Some(mut surface) => {
                        surface.paint_rgb(&vec![[200, 60, 30]; width * height]);
                        Ok(Box::new(Once { surface, shown: AtomicBool::new(false) }) as Box<dyn Frames>)
                    }
                    None => Err(Fail::Capture(CaptureError::Format)),
                };
                let started = match begin_into(layout(width, height), &ShareQuality::default(), &surveyed, screen) {
                    Ok(started) => started,
                    Err(Fail::Capture(CaptureError::Format)) => return None,
                    Err(_) => panic!("the share begins"),
                };
                let progress = Progress::default();
                let begun = Instant::now();
                let mut first = None;
                let stop = || {
                    if first.is_none() && progress.bytes.load(Ordering::Relaxed) > 0 {
                        first = Some(begun.elapsed());
                    }
                    begun.elapsed() > Duration::from_millis(700)
                };
                started.run(None, None, &progress, stop, || true).expect("it runs");
                Some(first)
            })
            .await
            .unwrap();
            let Some(first) = out else {
                skipped(format!("{encoder} takes no 720p surface here"));
                continue;
            };
            let first = first.unwrap_or_else(|| panic!("{encoder}: the change was never sent while the share ran"));
            eprintln!("{encoder}: the one change sent {first:?} after the share began");
            assert!(first < SOON, "{encoder}: the change was sent after {first:?}, not within {SOON:?}");
            shared += 1;
        }
        if shared == 0 {
            skipped("no hardware H.264 encoder shares a screen here");
        }
    }
}
