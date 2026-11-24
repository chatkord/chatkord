//! How a measured number reads on screen: one method per kind on [`Words`], so every front end writes them alike and
//! each language's catalog sets its own separators, units and order.

use crate::words::{
    FigureClockValues, FigureDimensionsValues, FigureOfMostValues, Phrase, Text, Words, without_isolation,
};
use std::time::Duration;

/// A figure is one run of digits and a unit, so it carries no marks isolating its parts' direction.
fn bare(text: &str) -> String {
    without_isolation(text)
}

impl Words {
    /// "950 bit/s", "48 kbit/s", "8 Mbit/s", "8.4 Mbit/s": decimal units, whole where it is whole.
    pub fn rate(&self, bits_per_second: u64) -> String {
        bare(&match bits_per_second {
            0..1_000 => self.figure_bits(bits_per_second.to_string()),
            1_000..1_000_000 => self.figure_kbits(self.digits((bits_per_second + 500) / 1_000)),
            _ => self.figure_mbits(self.tenths(bits_per_second, 1_000_000)),
        })
    }

    /// "4", "2.5": a rate in Mbit/s without its unit, as a row whose heading carries the unit labels it, to one
    /// decimal with this language's separator.
    pub fn mbits(&self, bits_per_second: u64) -> String {
        bare(&self.tenths(bits_per_second, 1_000_000))
    }

    /// "12 B", "640 kB", "12.4 MB", "1.2 GB".
    pub fn bytes(&self, bytes: u64) -> String {
        bare(&match bytes {
            0..1_000 => self.figure_b(bytes.to_string()),
            1_000..1_000_000 => self.figure_kb(self.digits((bytes + 500) / 1_000)),
            1_000_000..1_000_000_000 => self.figure_mb(self.tenths(bytes, 1_000_000)),
            _ => self.figure_gb(self.tenths(bytes, 1_000_000_000)),
        })
    }

    /// "1920 × 1080": a picture's size.
    pub fn dimensions(&self, width: u32, height: u32) -> String {
        bare(
            &self.figure_dimensions(FigureDimensionsValues {
                w: width.to_string().into(),
                h: height.to_string().into(),
            }),
        )
    }

    /// "150 %", a zoom.
    pub fn percent(&self, percent: u16) -> String {
        bare(&self.figure_percent(percent.to_string()))
    }

    /// "12,408": a count with its language's group separator.
    pub fn count(&self, count: u64) -> String {
        self.digits(count)
    }

    /// "21:09": a message's time, as the language writes the clock.
    pub fn clock(&self, at: chat::DateTime) -> String {
        bare(&self.figure_clock(FigureClockValues {
            hour: format!("{:02}", at.hour()).into(),
            minute: format!("{:02}", at.minute()).into(),
        }))
    }

    /// "4,012 / 4,000": how much a draft holds of the most a message may.
    pub fn of_most(&self, count: u64, most: u64) -> String {
        bare(
            &self.figure_of_most(FigureOfMostValues {
                count: self.digits(count).into(),
                most: self.digits(most).into(),
            }),
        )
    }

    /// "38 ms", "under 1 ms" on a near route (the engine reports whole milliseconds), or a dash where nothing was
    /// measured.
    pub fn millis(&self, round_trip: Option<Duration>) -> String {
        match round_trip.map(|time| time.as_millis()) {
            None => self.phrase(Phrase::FigureNone).into_owned(),
            Some(0) => self.phrase(Phrase::FigureUnderMs).into_owned(),
            Some(millis) => bare(&self.figure_ms(self.digits(u64::try_from(millis).unwrap_or(u64::MAX)))),
        }
    }

    /// "2.0", "0.9": a time in seconds to one decimal with this language's separator, always with the decimal.
    pub fn seconds(&self, time: Duration) -> String {
        let tenths = u64::try_from((time.as_millis() + 50) / 100).unwrap_or(u64::MAX);
        bare(&format!("{}{}{}", self.digits(tenths / 10), self.text(Text::FigureDecimal), tenths % 10))
    }

    /// "30 fps": pictures a second.
    pub fn fps(&self, per_second: u32) -> String {
        bare(&self.figure_fps(per_second.to_string()))
    }

    /// "1080p": a picture's short side.
    pub fn p(&self, side: u32) -> String {
        bare(&self.figure_p(side.to_string()))
    }

    /// A server's limit as its host sets it, in whole binary units under the catalog's names, as hosts write them:
    /// "50 MB" for 50 MiB, "1 GB" for 1 GiB.
    pub fn limit(&self, bytes: u64) -> String {
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * KIB;
        const GIB: u64 = 1024 * MIB;
        bare(&if bytes >= GIB && bytes.is_multiple_of(GIB) {
            self.figure_gb(self.digits(bytes / GIB))
        } else if bytes >= MIB {
            self.figure_mb(self.digits(bytes / MIB))
        } else {
            self.figure_kb(self.digits(bytes / KIB))
        })
    }

    /// `number` with this language's group separator between every three digits.
    fn digits(&self, number: u64) -> String {
        let group = self.text(Text::FigureGroup);
        let digits = number.to_string();
        let first = digits.len() % 3;
        let mut out = String::with_capacity(digits.len() + digits.len() / 3 * group.len());
        for (at, digit) in digits.chars().enumerate() {
            if at > 0 && at % 3 == first {
                out.push_str(&group);
            }
            out.push(digit);
        }
        out
    }

    /// `value / unit` to one decimal with this language's separator, and none on a whole number.
    fn tenths(&self, value: u64, unit: u64) -> String {
        let tenths = (value + unit / 20) / (unit / 10);
        if tenths.is_multiple_of(10) {
            self.digits(tenths / 10)
        } else {
            format!("{}{}{}", self.digits(tenths / 10), self.text(Text::FigureDecimal), tenths % 10)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    const SEPARATORS: &str = "language-name = Test\n\
        figure-decimal = {\",\"}\n\
        figure-group = {\".\"}\n\
        figure-mbits = { $n } Mbit/s\n\
        figure-kb = { $n } kB\n\
        figure-ms = { $n } ms\n\
        figure-under-ms = under 1 ms\n\
        figure-none = none\n";

    #[test]
    fn figures_follow_the_catalog_s_separators() {
        let words = Words::new(Language::given("nl", "Test", SEPARATORS));
        assert_eq!(words.count(999), "999");
        assert_eq!(words.count(1_234), "1.234");
        assert_eq!(words.count(1_234_567), "1.234.567");
        assert_eq!(words.rate(8_440_000), "8,4 Mbit/s");
        assert_eq!(words.rate(1_234_500_000), "1.234,5 Mbit/s");
        assert_eq!(words.rate(8_000_000), "8 Mbit/s");
        assert_eq!(words.bytes(640_000), "640 kB");
        assert_eq!(words.millis(Some(Duration::from_micros(38_400))), "38 ms");
        assert_eq!(words.millis(None), "none");
    }

    #[test]
    fn seconds_take_the_catalog_s_decimal() {
        let comma = Words::new(Language::given("nl", "Test", SEPARATORS));
        assert_eq!(comma.seconds(Duration::from_millis(2_040)), "2,0");
        assert_eq!(comma.seconds(Duration::from_millis(940)), "0,9");
        assert_eq!(Words::english().seconds(Duration::from_millis(12_340)), "12.3");
    }

    #[test]
    fn a_rate_reads_in_the_unit_it_is_nearest_and_is_whole_where_it_is_whole() {
        let words = Words::english();
        assert_eq!(words.rate(0), "0 bit/s");
        assert_eq!(words.rate(950), "950 bit/s");
        assert_eq!(words.rate(48_000), "48 kbit/s");
        assert_eq!(words.rate(48_499), "48 kbit/s");
        assert_eq!(words.rate(48_500), "49 kbit/s");
        assert_eq!(words.rate(8_000_000), "8 Mbit/s");
        assert_eq!(words.rate(8_440_000), "8.4 Mbit/s");
    }

    #[test]
    fn bytes_read_decimal_and_a_number_never_names_the_unit_above_it() {
        let words = Words::english();
        assert_eq!(words.bytes(0), "0 B");
        assert_eq!(words.bytes(999), "999 B");
        assert_eq!(words.bytes(640_000), "640 kB");
        assert_eq!(words.bytes(12_400_000), "12.4 MB");
        assert_eq!(words.bytes(48_100_000), "48.1 MB");
        assert_eq!(words.bytes(1_200_000_000), "1.2 GB");
    }

    #[test]
    fn a_count_has_a_comma_every_three_digits() {
        let words = Words::english();
        assert_eq!(words.count(0), "0");
        assert_eq!(words.count(999), "999");
        assert_eq!(words.count(1_000), "1,000");
        assert_eq!(words.count(12_408), "12,408");
        assert_eq!(words.count(1_234_567), "1,234,567");
    }

    #[test]
    fn a_round_trip_is_whole_milliseconds_and_none_is_a_dash() {
        let words = Words::english();
        assert_eq!(words.millis(Some(Duration::from_micros(38_400))), "38 ms");
        assert_eq!(words.millis(Some(Duration::from_micros(1_000))), "1 ms");
        assert_eq!(words.millis(Some(Duration::ZERO)), "under 1 ms", "measured, below what the engine resolves");
        assert_eq!(words.millis(Some(Duration::from_micros(400))), "under 1 ms");
        assert_eq!(words.millis(None), "\u{2014}");
    }

    #[test]
    fn a_frame_rate_a_height_and_a_size_read_as_the_catalog_writes_them() {
        let words = Words::english();
        assert_eq!(words.fps(30), "30 fps");
        assert_eq!(words.p(1080), "1080p");
        assert_eq!(words.dimensions(1920, 1080), "1920 \u{d7} 1080");
        assert_eq!(words.percent(150), "150 %");
        assert_eq!(words.of_most(4_012, 4_000), "4,012 / 4,000");
    }

    #[test]
    fn a_host_s_limit_reads_in_whole_binary_units() {
        let words = Words::english();
        assert_eq!(words.limit(50 * 1024 * 1024), "50 MB");
        assert_eq!(words.limit(1024 * 1024 * 1024), "1 GB");
        assert_eq!(words.limit(1536 * 1024 * 1024), "1,536 MB");
        assert_eq!(words.limit(512 * 1024), "512 kB");
    }
}
