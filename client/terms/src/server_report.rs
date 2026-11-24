//! The server's debug report as it arrives: read once where it arrives, so the window is handed what it lists.

use crate::error::LinkError;
use crate::files::ReportError;
use std::sync::Arc;

/// A server's debug report: its text, saved as it is, and what its dialog lists of it, made only by reading the text
/// as a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrivedReport {
    text: Arc<str>,
    listing: report::Listing,
}

impl ArrivedReport {
    /// `text` as the server sent it, listed with the `newest` lines of each log. Linear in the text, which may be
    /// megabytes: not for the window's thread.
    ///
    /// # Errors
    ///
    /// [`LinkError::Malformed`] for a text over [`report::MAX_REPORT_BYTES`] or not in the shape a report is written in.
    pub fn read(text: &str, newest: usize) -> Result<Self, ReportError> {
        let listing = report::Listing::read(text, newest).map_err(|_| ReportError::Reach(LinkError::Malformed))?;
        Ok(Self { text: text.into(), listing })
    }

    /// The text, byte for byte as it arrived, and what the dialog lists of it.
    #[must_use]
    pub fn into_parts(self) -> (Arc<str>, report::Listing) {
        (self.text, self.listing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_keeps_its_text_whole_and_a_text_that_is_not_one_is_malformed() {
        let text = "Chatkord debug report: server\nCreated now\n\nStore\n  users: 3\n";
        let (kept, listing) = ArrivedReport::read(text, 5).expect("a report").into_parts();
        assert_eq!((&*kept, listing.parts().len()), (text, 1));
        for refused in ["", "Chatkord debug report: server\n", "a name\n"] {
            assert_eq!(ArrivedReport::read(refused, 5), Err(ReportError::Reach(LinkError::Malformed)), "{refused:?}");
        }
    }
}
