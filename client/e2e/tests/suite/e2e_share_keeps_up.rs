//! A share whose encoder cannot keep up with its screen comes down to a rate it can, on each system's own screen:
//! a DMA-BUF on Linux, a texture on the encoder's own D3D11 device on Windows, made once the encoder is open.

use crate::screen_support;

use call::screen::{Fail, Frames, Progress, begin_into};
use capture::{CaptureError, Frame, Pixels};
use choices::ShareQuality;
use media::Session;
use screen_support::{Canvas, candidates_of, layout};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tier::skipped;

/// A busy screen: eight pictures by turns, a block moving over fine lines, at 60 a second on its own clock, as a
/// compositor sends them however long the last one took to encode.
pub(crate) struct Busy {
    surfaces: Vec<screen_support::Surface>,
    next: AtomicUsize,
    began: Instant,
}

// SAFETY: one thread at a time uses the surfaces' FFmpeg objects: the one that encodes, after the share began.
unsafe impl Send for Busy {}

impl Frames for Busy {
    fn next(&self, _wait: Duration) -> Result<Option<Frame>, CaptureError> {
        let sixtieth = Duration::from_micros(16_667);
        let now = self.began.elapsed();
        let due = sixtieth * (now.as_micros() / sixtieth.as_micros() + 1) as u32;
        crate::clock::frame_clock(due - now);
        let at = self.next.fetch_add(1, Ordering::Relaxed);
        Ok(Some(self.surfaces[at % self.surfaces.len()].frame(Pixels::Bgrx)))
    }

    fn dropped(&self) -> u64 {
        0
    }
}

/// The busy screen on `canvas`, its surfaces ones `session`'s encoder takes; none when the device makes none so big.
pub(crate) fn busy(canvas: &Canvas, session: &Session, width: usize, height: usize) -> Option<Busy> {
    let pictures = (0..8).map(|n| {
        (0..width * height)
            .map(|i| {
                let (x, y) = (i % width, i / width);
                if (x + n * width / 8) % width < width / 6 && y < height / 3 {
                    [235, 235, 235]
                } else if (x / 2 + y) % 9 < 2 {
                    [40, 40, 40]
                } else {
                    [200, 200, 200]
                }
            })
            .collect()
    });
    painted(canvas, session, width, height, pictures)
}

/// A screen showing `pictures` in turn at 60 a second, on surfaces `session`'s encoder takes; none when the device
/// makes none so big.
pub(crate) fn painted(
    canvas: &Canvas,
    session: &Session,
    width: usize,
    height: usize,
    pictures: impl Iterator<Item = Vec<[u8; 3]>>,
) -> Option<Busy> {
    let surfaces = pictures
        .map(|picture| {
            let mut surface = canvas.surface(session, width, height)?;
            surface.paint_rgb(&picture);
            Some(surface)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Busy { surfaces, next: AtomicUsize::new(0), began: Instant::now() })
}

mod lab {
    use super::*;

    /// Each asserts the rate this machine's encoder keeps: beside other tests on a shared GPU and cores it keeps less.
    mod timing {
        use super::*;

        /// SVT-AV1 on the Ryzen 5 5600 encodes at most about 21 pictures a second of a busy 4K screen under Linux, 19
        /// under Windows: a share of one at 60 comes down to a rate it keeps up with, its size kept until the rate is 15.
        #[tokio::test(flavor = "multi_thread")]
        async fn a_software_share_its_processor_cannot_keep_up_with_comes_down_to_a_rate_it_can() {
            let software = candidates_of(media::Codec::Av1).into_iter().find(|(e, _)| e.name == media::Name::SvtAv1);
            let Some((software, canvas)) = software else {
                return skipped("no device converts a screen for SVT-AV1 here");
            };
            let surveyed = screen_support::kept(media::Report { encoders: vec![software], unavailable: vec![] });
            let quality = ShareQuality { codec: Some(choices::Codec::Av1), ..ShareQuality::default() };
            let (width, height) = (3840, 2160);
            let report = tokio::task::spawn_blocking(move || {
                let screen = |session: &Session| match busy(&canvas, session, width, height) {
                    Some(screen) => Ok(Box::new(screen) as Box<dyn Frames>),
                    None => Err(Fail::Capture(CaptureError::Format)),
                };
                let started = match begin_into(layout(width, height), &quality, &surveyed, screen) {
                    Ok(started) => started,
                    Err(Fail::Capture(CaptureError::Format)) => return None,
                    Err(Fail::Encode(why)) => panic!("the share begins: {why}"),
                    Err(_) => panic!("the share begins"),
                };
                let until = Instant::now() + Duration::from_secs(12);
                Some(
                    started.run(None, None, &Progress::default(), || Instant::now() > until, || true).expect("it runs"),
                )
            })
            .await
            .unwrap();
            let Some(report) = report else {
                return skipped("the device makes no 4K BGRx surface");
            };
            let encoded = report.frames as f64 / report.span.as_secs_f64();
            eprintln!(
                "{}: a busy 4K screen at 60 came down to {} a second, {encoded:.1} encoded a second over the run, {}x{}",
                report.encoder, report.final_frames_per_second, report.layout.width, report.layout.height
            );
            assert!(report.final_frames_per_second.get() < 60, "it came down from 60");
            // Under other load it may come down further: the size only once the frame rate is at its least.
            let kept = (report.layout.width, report.layout.height) == (width as u32, height as u32);
            assert!(kept || report.final_frames_per_second.get() == 15, "the frame rate first, then the size");
        }

        /// A hardware encoder's encode is inside the time `KeepUp` reads: in the send, or for VA-API and Vulkan, whose one
        /// picture in flight overlaps it, as the wait of a send that finds it unfinished. A busy 4K screen at 60 still keeps
        /// its rate and size on every hardware encoder of each codec.
        #[tokio::test(flavor = "multi_thread")]
        async fn a_hardware_share_of_a_busy_4k_screen_keeps_60_a_second() {
            let mut shared = 0;
            for codec in [(media::Codec::H264, choices::Codec::H264), (media::Codec::Av1, choices::Codec::Av1)] {
                let hardware: Vec<_> =
                    candidates_of(codec.0).into_iter().filter(|(e, _)| e.path() != media::Path::Software).collect();
                if hardware.is_empty() {
                    skipped(format!("no hardware {:?} encoder converts a screen here", codec.0));
                }
                for (encoder, canvas) in hardware {
                    let surveyed = screen_support::kept(media::Report { encoders: vec![encoder], unavailable: vec![] });
                    let quality = ShareQuality { codec: Some(codec.1), ..ShareQuality::default() };
                    let (width, height) = (3840, 2160);
                    let report = tokio::task::spawn_blocking(move || {
                        let screen = |session: &Session| match busy(&canvas, session, width, height) {
                            Some(screen) => Ok(Box::new(screen) as Box<dyn Frames>),
                            None => Err(Fail::Capture(CaptureError::Format)),
                        };
                        let started = match begin_into(layout(width, height), &quality, &surveyed, screen) {
                            Ok(started) => started,
                            Err(Fail::Capture(CaptureError::Format)) => return None,
                            Err(Fail::Encode(why)) => panic!("the share begins: {why}"),
                            Err(_) => panic!("the share begins"),
                        };
                        let until = Instant::now() + Duration::from_secs(12);
                        Some(
                            started
                                .run(None, None, &Progress::default(), || Instant::now() > until, || true)
                                .expect("it runs"),
                        )
                    })
                    .await
                    .unwrap();
                    let Some(report) = report else {
                        skipped("the device makes no 4K BGRx surface");
                        continue;
                    };
                    eprintln!("{}", report.line());
                    assert_eq!(report.final_frames_per_second.get(), 60, "{}: kept 60 a second", report.encoder);
                    assert_eq!(
                        (report.layout.width, report.layout.height),
                        (3840, 2160),
                        "{}: kept its size",
                        report.encoder
                    );
                    shared += 1;
                }
            }
            if shared == 0 {
                skipped("no hardware encoder shares a 4K screen here");
            }
        }
    }
}
