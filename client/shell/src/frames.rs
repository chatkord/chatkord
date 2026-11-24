//! The window's frames, tallied a second at a time for a measurement to read.

use std::time::{Duration, Instant};

/// The frames drawn since the first one counted, and their CPU time; nothing before the first frame.
#[derive(Default)]
pub(crate) struct Tally(Option<Counted>);

struct Counted {
    since: Instant,
    frames: u32,
    /// Of `frames`, those that laid the interface out again.
    laid_out: u32,
    cpu: Duration,
    worst: Duration,
}

impl Counted {
    fn first(cpu: Duration, laid_out: bool, now: Instant) -> Self {
        Self { since: now, frames: 1, laid_out: u32::from(laid_out), cpu, worst: cpu }
    }
}

/// The frames of a second, or of longer when the window drew nothing between two: how many were drawn, over how long,
/// and the CPU time of each, mean and worst.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Second {
    frames: u32,
    laid_out: u32,
    over: Duration,
    mean: Duration,
    worst: Duration,
}

impl Tally {
    /// Counts a frame that took `cpu`, drawn at `now` (`laid_out` when it laid the interface out again); once a second
    /// has passed since the first frame counted, the frames before this one, and this one starts the next tally. A
    /// window drawing nothing counts nothing.
    pub(crate) fn count(&mut self, cpu: Duration, laid_out: bool, now: Instant) -> Option<Second> {
        let Some(counted) = &mut self.0 else {
            self.0 = Some(Counted::first(cpu, laid_out, now));
            return None;
        };
        let over = now.saturating_duration_since(counted.since);
        if over < Duration::from_secs(1) {
            counted.frames = counted.frames.saturating_add(1);
            counted.laid_out = counted.laid_out.saturating_add(u32::from(laid_out));
            counted.cpu = counted.cpu.saturating_add(cpu);
            counted.worst = counted.worst.max(cpu);
            return None;
        }
        let second = Second {
            frames: counted.frames,
            laid_out: counted.laid_out,
            over,
            mean: counted.cpu / counted.frames,
            worst: counted.worst,
        };
        *counted = Counted::first(cpu, laid_out, now);
        Some(second)
    }
}

impl Second {
    /// Logs the second: how many frames and how many of them laid out, over how long, and the CPU time of each, mean
    /// and worst.
    pub(crate) fn note(&self) {
        report::note!(
            debug,
            "frames drawn",
            frames = self.frames,
            laid_out = self.laid_out,
            over_ms = report::millis(self.over),
            mean_us = micros(self.mean),
            worst_us = micros(self.worst)
        );
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_second_of_frames_is_told_once_with_its_mean_and_worst() {
        let start = Instant::now();
        let mut tally = Tally::default();
        assert_eq!(tally.count(2 * MS, true, start), None);
        assert_eq!(tally.count(4 * MS, false, start + 500 * MS), None);
        assert_eq!(tally.count(MS, true, start + 999 * MS), None);
        let told = tally.count(9 * MS, false, start + 1000 * MS);
        let second = Second { frames: 3, laid_out: 2, over: 1000 * MS, mean: 7 * MS / 3, worst: 4 * MS };
        assert_eq!(told, Some(second));
        assert_eq!(tally.count(MS, true, start + 1500 * MS), None);
        let next = tally.count(MS, false, start + 2100 * MS);
        assert_eq!(next, Some(Second { frames: 2, laid_out: 1, over: 1100 * MS, mean: 5 * MS, worst: 9 * MS }));
    }

    #[test]
    fn a_frame_after_an_idle_spell_tells_the_spell_it_ends() {
        let start = Instant::now();
        let mut tally = Tally::default();
        assert_eq!(tally.count(3 * MS, true, start), None);
        let told = tally.count(MS, true, start + 30_000 * MS);
        assert_eq!(told, Some(Second { frames: 1, laid_out: 1, over: 30_000 * MS, mean: 3 * MS, worst: 3 * MS }));
    }
}
