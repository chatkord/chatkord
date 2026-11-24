//! What the report dialog draws of a report: every section in full, and of a log its heading, its count and only the
//! newest lines, with a sentence where lines are left out. The report crate makes the listing, so a report of megabytes
//! never makes the window slow; the file holds the whole text.

use std::fmt::Write as _;
use words::Sentence;

/// The most lines of a log the dialog lists: the newest.
pub const LISTED_LOG_LINES: usize = 200;

/// A report as the dialog lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing(report::Listing);

impl Listing {
    pub(crate) fn of(report: &report::Report) -> Listing {
        Listing(report.listing(LISTED_LOG_LINES))
    }

    /// A server's report as read where it arrived, with [`LISTED_LOG_LINES`] of each log.
    pub(crate) fn listed(listing: report::Listing) -> Listing {
        Listing(listing)
    }

    /// The text the dialog lists. A log cut to its newest lines says so in `said`'s words, in place of the lines left out.
    pub fn text(&self, said: impl Fn(&Sentence) -> String) -> String {
        let mut text = String::new();
        for line in self.0.head() {
            let _ = writeln!(text, "{line}");
        }
        for part in self.0.parts() {
            text.push('\n');
            for row in part.rows() {
                let _ = writeln!(text, "{row}");
            }
            if let Some(log) = part.log() {
                if log.total() > log.newest().len() {
                    let cut = Sentence::ReportLogCut {
                        shown: u64::try_from(log.newest().len()).unwrap_or(u64::MAX),
                        total: u64::try_from(log.total()).unwrap_or(u64::MAX),
                    };
                    let _ = writeln!(text, "  {}", said(&cut));
                }
                for line in log.newest() {
                    let _ = writeln!(text, "{line}");
                }
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(sentence: &Sentence) -> String {
        match sentence {
            Sentence::ReportLogCut { shown, total } => format!("last {shown} of {total}"),
            other => format!("{other:?}"),
        }
    }

    fn log_of(count: usize) -> String {
        let lines: String = (0..count).map(|at| format!("  line {at}\n")).collect();
        format!("\nRecent log, newest last\n  notes too long to keep: 0\n{lines}")
    }

    const HEAD: &str = "Chatkord debug report: server\nCreated now\n";

    fn read(text: &str) -> Listing {
        Listing::listed(report::Listing::read(text, LISTED_LOG_LINES).expect("a report"))
    }

    #[test]
    fn a_text_of_short_blocks_is_listed_as_it_is() {
        let text = format!("{HEAD}\nSystem\n  Linux\n\nStore\n  1 page\n");
        assert_eq!(read(&text).text(said), text);
    }

    #[test]
    fn a_log_of_a_server_is_cut_to_its_newest_lines_and_says_so() {
        let text = format!("{HEAD}{}", log_of(LISTED_LOG_LINES + 50));
        let listed = read(&text).text(said);
        assert!(
            listed.contains("Recent log, newest last\n  notes too long to keep: 0\n  last 200 of 250\n  line 50\n")
        );
        assert!(listed.ends_with("  line 249\n") && !listed.contains("line 49\n"), "{listed}");
        assert_eq!(listed.lines().count(), 2 + 3 + 1 + LISTED_LOG_LINES);
    }

    #[test]
    fn a_log_at_the_most_is_listed_whole_without_a_notice() {
        let text = format!("{HEAD}{}", log_of(LISTED_LOG_LINES));
        assert_eq!(read(&text).text(said), text);
    }
}
