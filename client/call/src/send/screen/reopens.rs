//! Why a share's encoder was reopened, and what each reopen changed: counted by cause for the share's health line,
//! bounded, the last change kept whole.

use media::Changes;
use std::fmt;

/// What made the share ask its encoder for a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The screen took a new size and kept it.
    Resized,
    /// The encoder could not keep up: a step down.
    Overused,
    /// The pictures needed more bits than the rate gives them: a step down.
    Busy,
    /// The encoder had room to spare: a step back up.
    Roomy,
    /// The engine asked for another rate for the link.
    LinkRate,
    /// The encoder failed and another took over.
    EncoderFailed,
}

impl Cause {
    const ALL: [Cause; 6] =
        [Cause::Resized, Cause::Overused, Cause::Busy, Cause::Roomy, Cause::LinkRate, Cause::EncoderFailed];

    fn word(self) -> &'static str {
        match self {
            Cause::Resized => "resized",
            Cause::Overused => "overused",
            Cause::Busy => "busy",
            Cause::Roomy => "roomy",
            Cause::LinkRate => "link rate",
            Cause::EncoderFailed => "encoder failed",
        }
    }
}

/// The reopens of a share so far: how many for each cause, and the last with what it changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reopens {
    counts: [u64; Cause::ALL.len()],
    last: Option<(Cause, Changes)>,
}

impl Reopens {
    /// Takes in a reopen `cause` asked for, which changed what `changes` says.
    pub fn record(&mut self, cause: Cause, changes: Changes) {
        if let Some(index) = Cause::ALL.iter().position(|each| *each == cause) {
            self.counts[index] = self.counts[index].saturating_add(1);
        }
        self.last = Some((cause, changes));
    }

    /// How many reopens there were in all.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// The reopens since `earlier`, its last change dropped if nothing new came.
    pub fn since(&self, earlier: &Reopens) -> Reopens {
        let mut counts = self.counts;
        for (count, before) in counts.iter_mut().zip(earlier.counts) {
            *count = count.saturating_sub(before);
        }
        let last = if counts.iter().any(|count| *count > 0) { self.last } else { None };
        Reopens { counts, last }
    }
}

impl fmt::Display for Reopens {
    /// "2 resized, 1 overused; last overused: 3840x2160 -> 2560x1440, 20.0 -> 12.0 Mbit/s", or "none".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let counted: Vec<String> = Cause::ALL
            .iter()
            .zip(self.counts)
            .filter(|(_, count)| *count > 0)
            .map(|(cause, count)| format!("{count} {}", cause.word()))
            .collect();
        if counted.is_empty() {
            return f.write_str("none");
        }
        f.write_str(&counted.join(", "))?;
        let Some((cause, changes)) = self.last else { return Ok(()) };
        write!(f, "; last {}:", cause.word())?;
        let mut said = Vec::new();
        if let Some((from, to)) = changes.encoder {
            said.push(format!("{from} -> {to}"));
        }
        if let Some(((w0, h0), (w1, h1))) = changes.size {
            said.push(format!("{w0}x{h0} -> {w1}x{h1}"));
        }
        if let Some((from, to)) = changes.frames_per_second {
            said.push(format!("{from} -> {to} fps"));
        }
        if let Some((from, to)) = changes.bits_per_second {
            let mbit = |bits: u32| f64::from(bits) / 1_000_000.0;
            said.push(format!("{:.1} -> {:.1} Mbit/s", mbit(from), mbit(to)));
        }
        write!(f, " {}", said.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smaller() -> Changes {
        Changes {
            size: Some(((3840, 2160), (2560, 1440))),
            bits_per_second: Some((20_000_000, 12_000_000)),
            ..Changes::default()
        }
    }

    #[test]
    fn reopens_are_counted_by_cause_and_bounded() {
        let mut reopens = Reopens::default();
        assert_eq!(reopens.to_string(), "none");
        for _ in 0..2 {
            reopens.record(Cause::Resized, Changes::default());
        }
        reopens.record(Cause::Overused, smaller());
        assert_eq!(reopens.total(), 3);
        assert_eq!(
            reopens.to_string(),
            "2 resized, 1 overused; last overused: 3840x2160 -> 2560x1440, 20.0 -> 12.0 Mbit/s"
        );
        assert_eq!(std::mem::size_of::<Reopens>(), std::mem::size_of_val(&reopens), "a fixed size however many come");
    }

    #[test]
    fn the_reopens_since_a_look_are_its_counts_and_the_last_only_if_one_came() {
        let mut reopens = Reopens::default();
        reopens.record(Cause::LinkRate, smaller());
        let looked = reopens;
        assert_eq!(reopens.since(&looked).to_string(), "none", "nothing new since the look");
        reopens.record(
            Cause::EncoderFailed,
            Changes { encoder: Some((media::Name::H264Vulkan, media::Name::H264Vaapi)), ..Changes::default() },
        );
        assert_eq!(
            reopens.since(&looked).to_string(),
            "1 encoder failed; last encoder failed: h264_vulkan -> h264_vaapi"
        );
    }
}
