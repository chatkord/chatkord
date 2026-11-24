//! The numbers `capture` reports about the encode it ran. Pure, so a test sets them.

use std::time::Duration;

/// How long the pictures took to encode, kept as a count, their sum and the slowest, so a run of any length holds
/// three numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timings {
    count: u64,
    total: Duration,
    slowest: Duration,
}

impl Timings {
    /// Counts one more picture's time.
    pub fn push(&mut self, took: Duration) {
        self.count += 1;
        self.total = self.total.saturating_add(took);
        self.slowest = self.slowest.max(took);
    }

    /// The average time, none before the first picture.
    pub fn mean(&self) -> Option<Duration> {
        let count = u32::try_from(self.count).ok().filter(|count| *count > 0)?;
        Some(self.total / count)
    }

    /// The slowest picture's time, none before the first picture.
    pub fn slowest(&self) -> Option<Duration> {
        (self.count > 0).then_some(self.slowest)
    }
}

/// Megabits a second that `bytes` made over `span`; none for an empty span.
pub(crate) fn megabits(bytes: u64, span: Duration) -> Option<f64> {
    (!span.is_zero()).then(|| bytes as f64 * 8.0 / span.as_secs_f64() / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timings(millis: &[u64]) -> Timings {
        let mut t = Timings::default();
        millis.iter().for_each(|m| t.push(Duration::from_millis(*m)));
        t
    }

    #[test]
    fn no_pictures_have_no_time() {
        let t = Timings::default();
        assert_eq!((t.mean(), t.slowest()), (None, None));
    }

    #[test]
    fn the_mean_is_the_sum_over_the_count_and_the_slowest_is_the_largest() {
        let t = timings(&[4, 10, 1, 5]);
        assert_eq!(t.mean(), Some(Duration::from_millis(5)));
        assert_eq!(t.slowest(), Some(Duration::from_millis(10)));
        assert_eq!(timings(&[7]).slowest(), Some(Duration::from_millis(7)));
    }

    #[test]
    fn a_long_run_keeps_its_mean_and_slowest_with_no_picture_kept() {
        let mut long = Timings::default();
        for _ in 0..100_000 {
            long.push(Duration::from_millis(2));
        }
        assert_eq!((long.mean(), long.slowest()), (Some(Duration::from_millis(2)), Some(Duration::from_millis(2))));
    }

    #[test]
    fn a_rate_is_bits_over_the_span_and_an_empty_span_has_none() {
        assert_eq!(megabits(1_250_000, Duration::from_secs(1)), Some(10.0));
        assert_eq!(megabits(1, Duration::ZERO), None);
    }
}
