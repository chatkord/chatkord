//! A share paced below its screen's rate encodes each picture it takes once: a picture the pace leaves out is never
//! encoded again as a repeat, and the last one it leaves out, the screen's last change, is encoded once nothing newer
//! comes.

use crate::screen_support;

use call::screen::{Fail, Frames, Progress, Report, begin_into};
use capture::{CaptureError, Frame, Pixels};
use choices::{FrameRate, ShareQuality};
use media::Session;
use screen_support::{Canvas, candidates_of, layout};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tier::skipped;

const SIXTIETH: Duration = Duration::from_micros(16_667);

/// A screen that sends a picture at each of `due` (from its first, on its own clock as a compositor does), then
/// nothing new.
struct Timed {
    surfaces: [screen_support::Surface; 2],
    due: Vec<Duration>,
    sent: AtomicUsize,
    began: OnceLock<Instant>,
}

// SAFETY: one thread at a time uses the surfaces' FFmpeg objects: the one that encodes, after the share began.
unsafe impl Send for Timed {}

impl Frames for Timed {
    fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        let at = self.sent.fetch_add(1, Ordering::Relaxed);
        let Some(due) = self.due.get(at) else {
            crate::clock::frame_clock(wait);
            return Ok(None);
        };
        crate::clock::frame_clock(due.saturating_sub(self.began.get_or_init(Instant::now).elapsed()));
        Ok(Some(self.surfaces[at % 2].frame(Pixels::Bgrx)))
    }

    fn dropped(&self) -> u64 {
        0
    }
}

/// Shares `gaps` at 30 a second for `running` on every hardware H.264 encoder here, a report each.
async fn share(gaps: Vec<Duration>, running: Duration) -> Vec<Report> {
    let mut reports = Vec::new();
    let hardware = candidates_of(media::Codec::H264).into_iter().filter(|(e, _)| e.path() != media::Path::Software);
    for (encoder, canvas) in hardware {
        let surveyed = screen_support::kept(media::Report { encoders: vec![encoder.clone()], unavailable: vec![] });
        let gaps = gaps.clone();
        let report = tokio::task::spawn_blocking(move || run(&canvas, &surveyed, &gaps, running)).await.unwrap();
        match report {
            Some(report) => reports.push(report),
            None => skipped(format!("{encoder} takes no 720p surface here")),
        }
    }
    if reports.is_empty() {
        skipped("no hardware H.264 encoder shares a screen here");
    }
    reports
}

fn run(
    canvas: &Canvas,
    surveyed: &std::sync::Arc<media::Survey>,
    gaps: &[Duration],
    running: Duration,
) -> Option<Report> {
    let (width, height) = (1280, 720);
    let screen = |session: &Session| {
        let painted = |colour| {
            let mut surface = canvas.surface(session, width, height)?;
            surface.paint_rgb(&vec![colour; width * height]);
            Some(surface)
        };
        match (painted([200, 60, 30]), painted([30, 60, 200])) {
            (Some(one), Some(other)) => {
                let due = gaps.iter().scan(Duration::ZERO, |at, gap| {
                    *at += *gap;
                    Some(*at)
                });
                let due = due.collect();
                let timed = Timed { surfaces: [one, other], due, sent: AtomicUsize::new(0), began: OnceLock::new() };
                Ok(Box::new(timed) as Box<dyn Frames>)
            }
            _ => Err(Fail::Capture(CaptureError::Format)),
        }
    };
    let quality = ShareQuality { frame_rate: FrameRate::Thirty, ..ShareQuality::default() };
    let started = match begin_into(layout(width, height), &quality, surveyed, screen) {
        Ok(started) => started,
        Err(Fail::Capture(CaptureError::Format)) => return None,
        Err(Fail::Encode(why)) => panic!("the share begins: {why}"),
        Err(_) => panic!("the share begins"),
    };
    let until = Instant::now() + running;
    Some(started.run(None, None, &Progress::default(), || Instant::now() > until, || true).expect("it runs"))
}

mod lab {
    use super::*;

    /// Six seconds of a 60 Hz screen, then one still: about 180 pictures taken, and the still second's few repeats.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_share_at_30_of_a_60_hz_screen_sends_its_encoder_30_a_second() {
        let gaps = vec![SIXTIETH; 6 * 60];
        for report in share(gaps, Duration::from_secs(7)).await {
            eprintln!("{}: {} pictures encoded, {} sends", report.encoder, report.frames, report.sends);
            assert!((170..=185).contains(&report.frames), "{}: {} taken of 360", report.encoder, report.frames);
            assert!(
                report.sends <= report.frames + 4,
                "{}: {} sends for {} pictures",
                report.encoder,
                report.sends,
                report.frames
            );
        }
    }

    /// The first picture and one long after it are taken; the third, 5 ms after the second, is left out, and is the
    /// screen's last change.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_last_picture_the_pace_left_out_is_encoded_once_the_screen_is_still() {
        let gaps = vec![Duration::ZERO, Duration::from_millis(100), Duration::from_millis(5)];
        for report in share(gaps, Duration::from_millis(800)).await {
            assert_eq!(report.frames, 3, "{}: the last change encoded", report.encoder);
            assert_eq!(report.skipped, 0, "{}: nothing left out for good", report.encoder);
        }
    }
}
