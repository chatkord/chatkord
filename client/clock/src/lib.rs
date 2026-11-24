//! The system's performance counter, the clock Windows stamps what it captures with: a picture's
//! `SystemRelativeTime`, a sound packet's position. A tick of it is read here as an `Instant` of this process's clock.
#![cfg(windows)]

use std::time::{Duration, Instant};

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// The instant on this process's clock that `ticks` (100 ns of the performance counter) names; none where the
/// counter cannot be read.
pub fn instant_of(ticks: i64) -> Option<Instant> {
    let now = Instant::now();
    let (mut counter, mut frequency) = (0i64, 0i64);
    // SAFETY: each call writes one i64 through a pointer to a live local.
    unsafe { QueryPerformanceCounter(&raw mut counter) }
        // SAFETY: as above.
        .and_then(|()| unsafe { QueryPerformanceFrequency(&raw mut frequency) })
        .ok()?;
    match from_now(counter, frequency, ticks)? {
        Offset::Ahead(by) => now.checked_add(by),
        Offset::Behind(by) => now.checked_sub(by),
    }
}

/// Where a time lies from now.
#[derive(Debug, PartialEq, Eq)]
enum Offset {
    Ahead(Duration),
    Behind(Duration),
}

/// Where `ticks` (100 ns of the performance counter) lies from now, the counter reading `counter` at `frequency` a
/// second; none for a counter with no frequency.
fn from_now(counter: i64, frequency: i64, ticks: i64) -> Option<Offset> {
    let frequency = i128::from(frequency);
    if frequency <= 0 {
        return None;
    }
    let ticks = i128::from(ticks) - i128::from(counter) * 10_000_000 / frequency;
    let by = Duration::from_nanos(u64::try_from(ticks.unsigned_abs()).ok()?.checked_mul(100)?);
    Some(if ticks >= 0 { Offset::Ahead(by) } else { Offset::Behind(by) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_read_against_the_performance_counter_in_100_ns_ticks() {
        // A 10 MHz counter at 0.3 s, a picture drawn 10 ms before.
        assert_eq!(from_now(3_000_000, 10_000_000, 2_900_000), Some(Offset::Behind(Duration::from_millis(10))));
        // A 24 MHz counter reads the same instant in its own units.
        assert_eq!(from_now(7_200_000, 24_000_000, 2_900_000), Some(Offset::Behind(Duration::from_millis(10))));
        // The composition a picture belongs to can be after it arrives.
        assert_eq!(from_now(3_000_000, 10_000_000, 3_120_000), Some(Offset::Ahead(Duration::from_millis(12))));
        assert_eq!(from_now(3_000_000, 0, 2_900_000), None, "no frequency");
    }

    #[test]
    fn now_on_the_counter_is_now_on_this_clock() {
        let mut counter = 0i64;
        let mut frequency = 0i64;
        // SAFETY: as in `instant_of`.
        unsafe { QueryPerformanceCounter(&raw mut counter) }.unwrap();
        // SAFETY: as above.
        unsafe { QueryPerformanceFrequency(&raw mut frequency) }.unwrap();
        let ticks = i64::try_from(i128::from(counter) * 10_000_000 / i128::from(frequency)).unwrap();
        let read = instant_of(ticks).unwrap();
        assert!(Instant::now().duration_since(read) < Duration::from_millis(50), "{:?}", read.elapsed());
    }
}
