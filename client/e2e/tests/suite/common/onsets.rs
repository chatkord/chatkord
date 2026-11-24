//! Where loud sounds start in a recording, which the measurements of a voice's trip and of a face against its voice
//! both read.

/// The samples a second every recording here is taken at.
pub(crate) const RATE_HZ: usize = 48_000;
/// A sound starts where a sample passes this after at least [`QUIET`] below it.
pub(crate) const LOUD: i16 = 2_000;
/// A fifth of a second.
pub(crate) const QUIET: usize = RATE_HZ / 5;

/// Where each loud sound starts in `samples`, by sample.
pub(crate) fn onsets(samples: impl Iterator<Item = i16>) -> Vec<usize> {
    let (mut found, mut quiet) = (Vec::new(), QUIET);
    for (at, sample) in samples.enumerate() {
        if sample.saturating_abs() > LOUD {
            if quiet >= QUIET {
                found.push(at);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    found
}

/// The middle of `values`, the higher of the two middles of an even count.
pub(crate) fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
