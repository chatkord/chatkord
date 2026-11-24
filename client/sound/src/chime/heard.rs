//! How a sound came out of a virtual speaker against what was sent: how late it started, and what broke in it.

use std::ops::Range;

/// Samples a second, of what is sent and what is recorded.
pub(crate) const RATE: usize = crate::speaker::RATE as usize;
/// A sample this loud, of full scale, is sound; the recorder's silence is zeros.
const LOUD: f32 = 0.01;
/// The stretch a gap is looked for in: 5 ms.
const WINDOW: usize = RATE / 200;
/// A window of the recording this much quieter than the same window sent is a gap.
const GAP: f32 = 0.1;
/// A jump in the recording's slope this many times the largest the sound sent has is a click.
const CLICK: f32 = 2.0;

/// One play of a sound, as the speaker was sent it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Heard {
    /// From the ask to the sound's first loud sample, in milliseconds; the recorder's own quantum is in it, the same
    /// for every way a sound is played.
    pub(crate) delay_ms: f64,
    /// Windows of [`WINDOW`] where the sound sent is loud and the speaker was sent next to nothing.
    pub(crate) gaps: usize,
    /// Samples whose slope jumps by more than [`CLICK`] times the sound's largest, scaled to the level heard.
    pub(crate) clicks: usize,
}

/// `recorded` (full-scale samples) against `sent`, asked for when the recording held `asked` samples. None when
/// nothing loud came in `asked`, or the recording ends before the sound would.
pub(crate) fn compare(recorded: &[f32], asked: Range<usize>, sent: &[f32]) -> Option<Heard> {
    let sent_on = sent.iter().position(|sample| sample.abs() > LOUD)?;
    let asked_at = asked.start;
    let awaited = recorded.get(asked_at..asked.end.min(recorded.len()))?;
    let heard_on = asked_at + awaited.iter().position(|sample| sample.abs() > LOUD)?;
    let start = heard_on.checked_sub(sent_on)?;
    let heard = recorded.get(start..start + sent.len())?;
    let level = peak(heard) / peak(sent);
    let gaps = sent
        .as_chunks::<WINDOW>()
        .0
        .iter()
        .zip(heard.as_chunks::<WINDOW>().0)
        .filter(|(sent, heard)| rms(*sent) > LOUD && rms(*heard) < GAP * level * rms(*sent))
        .count();
    let bound = slope_jumps(sent).fold(0.0, f32::max) * level * CLICK;
    let clicks = slope_jumps(heard).filter(|jump| *jump > bound).count();
    #[expect(clippy::cast_precision_loss, reason = "a delay is a few thousand samples")]
    let delay_ms = (heard_on as f64 - asked_at as f64 - sent_on as f64) * 1000.0 / RATE as f64;
    Some(Heard { delay_ms, gaps, clicks })
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample.abs()).fold(f32::MIN_POSITIVE, f32::max)
}

fn rms(samples: &[f32]) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "a window is a few hundred samples")]
    let count = samples.len() as f32;
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / count).sqrt()
}

/// How much each sample's slope differs from the one before it.
fn slope_jumps(samples: &[f32]) -> impl Iterator<Item = f32> + '_ {
    samples.windows(3).map(|three| (three[2] - 2.0 * three[1] + three[0]).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, length: usize) -> Vec<f32> {
        #[expect(clippy::cast_precision_loss, reason = "test lengths are small")]
        (0..length).map(|n| 0.3 * (std::f32::consts::TAU * hz * n as f32 / RATE as f32).sin()).collect()
    }

    fn recorded(silence: usize, sound: &[f32]) -> Vec<f32> {
        let mut recorded = vec![0.0; silence];
        recorded.extend_from_slice(sound);
        recorded.extend(std::iter::repeat_n(0.0, RATE / 10));
        recorded
    }

    #[test]
    fn a_whole_sound_is_late_by_the_silence_before_it_and_unbroken() {
        let sent = tone(660.0, RATE / 4);
        let heard = compare(&recorded(4_800, &sent), 2_400..RATE, &sent);
        assert_eq!(heard, Some(Heard { delay_ms: 50.0, gaps: 0, clicks: 0 }));
    }

    #[test]
    fn a_quieter_copy_is_not_a_gap() {
        let sent = tone(660.0, RATE / 4);
        let quiet: Vec<f32> = sent.iter().map(|sample| sample * 0.5).collect();
        let heard = compare(&recorded(0, &quiet), 0..RATE, &sent).map(|heard| (heard.gaps, heard.clicks));
        assert_eq!(heard, Some((0, 0)));
    }

    #[test]
    fn a_lost_stretch_is_a_gap_and_its_edges_click() {
        let sent = tone(660.0, RATE / 4);
        let mut broken = sent.clone();
        broken[RATE / 10..RATE / 10 + RATE / 100].fill(0.0);
        let heard = compare(&recorded(0, &broken), 0..RATE, &sent).expect("heard");
        assert_eq!(heard.gaps, 2, "10 ms is two windows: {heard:?}");
        assert!(heard.clicks >= 2, "{heard:?}");
    }

    #[test]
    fn nothing_loud_while_it_is_awaited_is_not_heard() {
        let sent = tone(660.0, RATE / 4);
        assert_eq!(compare(&recorded(RATE / 10, &sent), 0..RATE / 20, &sent), None);
    }
}
