//! The server's debug report on Server settings › About: Download asks the server for it, the dialog the client's
//! report uses lists it, and Download there saves its whole text. The text is the server's, so it is bounded and read
//! as a report where it arrives, refused past the bound or out of shape, never cut; the dialog lists a log's newest
//! lines only.

use crate::client_report::{Body, Download, Listing, ReportMessage, Shown};
use report::Side;
use terms::{ArrivedReport, ReportError};
use words::{Sentence, failure};

/// Where the server's report stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Stage {
    /// None is asked for.
    #[default]
    Idle,
    /// The server was asked and has not answered.
    Asked,
    /// The dialog lists the answer.
    Open(Shown),
}

/// What the card says under its title.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ReportLine {
    /// What the report holds.
    #[default]
    Contains,
    /// The file it was last saved under.
    Downloaded(String),
    /// Why the server's report was not had: said as an alert.
    Refused(Sentence),
}

/// What the report asks of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// The server's report.
    Report,
    /// Where to save this file, and write it there.
    Save(Download),
}

/// The server's debug report, asked for on the About page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerReport {
    stage: Stage,
    line: ReportLine,
}

impl ServerReport {
    /// Whether the server was asked and the report is awaited: the card says so in place of its button.
    pub fn preparing(&self) -> bool {
        self.stage == Stage::Asked
    }

    /// The report the dialog shows, while it is open.
    pub fn shown(&self) -> Option<&Shown> {
        match &self.stage {
            Stage::Open(shown) => Some(shown),
            Stage::Idle | Stage::Asked => None,
        }
    }

    /// What the card says under its title.
    pub fn line(&self) -> &ReportLine {
        &self.line
    }

    pub(crate) fn update(&mut self, message: ReportMessage) -> Option<Ask> {
        match (message, &mut self.stage) {
            (ReportMessage::Open, stage @ Stage::Idle) => {
                *stage = Stage::Asked;
                Some(Ask::Report)
            }
            (ReportMessage::Cancel, stage @ Stage::Open(_)) => {
                *stage = Stage::Idle;
                None
            }
            (ReportMessage::Download, Stage::Open(shown)) => shown.download_pressed().map(Ask::Save),
            (ReportMessage::Saved(saved), Stage::Open(shown)) => {
                if let Some(name) = shown.saved(saved) {
                    self.line = ReportLine::Downloaded(name);
                    self.stage = Stage::Idle;
                }
                None
            }
            // The log is the client's own report's.
            (ReportMessage::Open | ReportMessage::Read(_) | ReportMessage::Cancel, _)
            | (ReportMessage::Download | ReportMessage::Saved(_), Stage::Idle | Stage::Asked) => None,
        }
    }

    /// The server's answer, at `wall` seconds since the epoch: the dialog opens on it, or the card says why not.
    pub(crate) fn replied(&mut self, answer: Result<ArrivedReport, ReportError>, wall: i64) {
        if self.stage != Stage::Asked {
            return;
        }
        match answer.map(ArrivedReport::into_parts) {
            Ok((text, listing)) => {
                let name = report::file_name(Side::Server, u64::try_from(wall).unwrap_or_default());
                self.stage = Stage::Open(Shown::new(Body::Text(text), Listing::listed(listing), name));
                self.line = ReportLine::Contains;
            }
            Err(why) => {
                self.stage = Stage::Idle;
                self.line = ReportLine::Refused(failure::report(&why));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_report::Saved;
    use crate::harness::{Clock, WALL, about_open, every_switch_on, tell};
    use crate::place;
    use crate::{Ask as Asked, server_about};
    use proto::chatkord_v1::ErrorCode;
    use proto::files::FileRefusal;
    use report::MAX_REPORT_BYTES;
    use std::time::Instant;
    use terms::LinkError;
    use words::Phrase;

    const TEXT: &str = "Chatkord debug report: server\nCreated now\n\nStore\n  users: 3\n";

    /// `text` read as the link reads a reply where it arrives.
    fn arrived(text: &str) -> Result<ArrivedReport, ReportError> {
        ArrivedReport::read(text, crate::client_report::LISTED_LOG_LINES)
    }

    fn page(message: ReportMessage) -> place::Message {
        place::Message::Page(place::PageMessage::About(server_about::Message::Report(message)))
    }

    fn reply(answer: Result<ArrivedReport, ReportError>) -> place::Message {
        place::Message::Page(place::PageMessage::ServerReported(answer))
    }

    fn open_page(clock: &Clock) -> (crate::Window, crate::PlaceId) {
        let (mut window, owls) = about_open(clock);
        every_switch_on(&mut window, clock, owls);
        (window, owls)
    }

    fn report_of(window: &crate::Window, owls: crate::PlaceId) -> &ServerReport {
        window.place(owls).and_then(|place| place.server_report()).expect("the report of an open About")
    }

    #[test]
    fn download_asks_the_server_once_and_the_card_says_it_is_preparing() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = open_page(&clock);
        let todo = tell(&mut window, &clock, owls, page(ReportMessage::Open));
        assert!(matches!(&todo[..], [Asked::At(at, place::Ask::GetServerReport)] if *at == owls), "{todo:?}");
        assert!(report_of(&window, owls).preparing());
        assert!(tell(&mut window, &clock, owls, page(ReportMessage::Open)).is_empty(), "asked once");
    }

    #[test]
    fn the_reply_opens_the_dialog_with_exactly_its_text() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = open_page(&clock);
        tell(&mut window, &clock, owls, page(ReportMessage::Open));
        tell(&mut window, &clock, owls, reply(arrived(TEXT)));
        let report = report_of(&window, owls);
        assert!(!report.preparing());
        assert_eq!(report.shown().map(|shown| shown.text().into_owned()).as_deref(), Some(TEXT));
        assert_eq!(window.report_dialog().map(|dialog| dialog.shown.text().into_owned()).as_deref(), Some(TEXT));
        let todo = tell(&mut window, &clock, owls, page(ReportMessage::Download));
        let name = report::file_name(Side::Server, u64::try_from(WALL).unwrap());
        let [Asked::At(_, place::Ask::SaveReport(download))] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(download.name, name);
        assert_eq!(download.bytes(), TEXT.as_bytes());
    }

    #[test]
    fn saving_closes_the_dialog_and_the_card_names_the_file() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = open_page(&clock);
        tell(&mut window, &clock, owls, page(ReportMessage::Open));
        tell(&mut window, &clock, owls, reply(arrived(TEXT)));
        tell(&mut window, &clock, owls, page(ReportMessage::Download));
        tell(&mut window, &clock, owls, page(ReportMessage::Saved(Saved::Failed)));
        assert!(report_of(&window, owls).shown().is_some_and(|shown| shown.failed() && !shown.saving()));
        tell(&mut window, &clock, owls, page(ReportMessage::Download));
        tell(&mut window, &clock, owls, page(ReportMessage::Saved(Saved::Declined)));
        assert!(report_of(&window, owls).shown().is_some(), "declined: the dialog stays");
        tell(&mut window, &clock, owls, page(ReportMessage::Download));
        let name = "chatkord-server-report-1.txt";
        tell(&mut window, &clock, owls, page(ReportMessage::Saved(Saved::Written(name.into()))));
        let report = report_of(&window, owls);
        assert!(report.shown().is_none());
        assert_eq!(report.line(), &ReportLine::Downloaded(name.into()));
    }

    #[test]
    fn cancel_closes_the_dialog_and_asks_nothing() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = open_page(&clock);
        tell(&mut window, &clock, owls, page(ReportMessage::Open));
        tell(&mut window, &clock, owls, reply(arrived(TEXT)));
        assert!(tell(&mut window, &clock, owls, page(ReportMessage::Cancel)).is_empty());
        assert!(window.report_dialog().is_none());
        assert_eq!(report_of(&window, owls).line(), &ReportLine::Contains);
    }

    #[test]
    fn a_rate_limited_ask_says_so_and_gives_the_button_back() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = open_page(&clock);
        tell(&mut window, &clock, owls, page(ReportMessage::Open));
        tell(&mut window, &clock, owls, reply(Err(ReportError::Reach(LinkError::Refused(ErrorCode::RateLimited)))));
        let report = report_of(&window, owls);
        assert!(!report.preparing() && report.shown().is_none());
        assert_eq!(report.line(), &ReportLine::Refused(Sentence::Phrase(Phrase::ReportTooMany)));
        assert_eq!(Phrase::ReportTooMany.in_english(), "Too many requests. Try again shortly.");
        assert!(matches!(
            &tell(&mut window, &clock, owls, page(ReportMessage::Open))[..],
            [Asked::At(_, place::Ask::GetServerReport)]
        ));
    }

    #[test]
    fn a_download_refused_says_the_words_of_its_status() {
        let mut report = ServerReport::default();
        let refused = |refusal| ReportError::Refused(refusal);
        let cases = [
            (refused(FileRefusal::TooMany), Phrase::ReportTooMany),
            (refused(FileRefusal::NotFound), Phrase::ReportGone),
            (refused(FileRefusal::Unauthenticated), Phrase::FileSignedOut),
        ];
        for (why, phrase) in cases {
            report.update(ReportMessage::Open);
            report.replied(Err(why.clone()), WALL);
            assert_eq!(report.line(), &ReportLine::Refused(Sentence::Phrase(phrase)), "{why}");
        }
        assert_eq!(Phrase::ReportGone.in_english(), "The report is no longer there. Ask for it again.");
    }

    #[test]
    fn any_other_failure_says_the_words_of_the_wires_code() {
        let mut report = ServerReport::default();
        for why in [LinkError::Timeout, LinkError::Refused(ErrorCode::NotAuthorized), LinkError::Offline] {
            report.update(ReportMessage::Open);
            report.replied(Err(ReportError::Reach(why.clone())), WALL);
            assert_eq!(report.line(), &ReportLine::Refused(failure::said(&why)), "{why}");
        }
    }

    #[test]
    fn a_reply_over_the_bound_is_refused_and_never_cut() {
        let mut report = ServerReport::default();
        report.update(ReportMessage::Open);
        report.replied(arrived(&"x".repeat(MAX_REPORT_BYTES + 1)), WALL);
        assert!(report.shown().is_none());
        assert_eq!(report.line(), &ReportLine::Refused(failure::said(&LinkError::Malformed)));
        report.update(ReportMessage::Open);
        let head = "Chatkord debug report: server\nCreated now\n\n";
        let mut text = format!("{head}Recent log, newest last\n  notes too long to keep: 0\n");
        let longest = 2 + report::MAX_LINE + 1;
        while text.len() < MAX_REPORT_BYTES {
            let rest = MAX_REPORT_BYTES - text.len();
            let mut take = rest.min(longest);
            if (1..3).contains(&(rest - take)) {
                take -= 3;
            }
            text.push_str(&format!("  {}\n", "x".repeat(take - 3)));
        }
        report.replied(arrived(&text), WALL);
        assert_eq!(report.shown().map(|shown| shown.text().len()), Some(MAX_REPORT_BYTES), "at the bound it is kept");
    }

    #[test]
    fn a_reply_of_more_parts_than_a_report_has_is_refused() {
        let mut report = ServerReport::default();
        report.update(ReportMessage::Open);
        let sections = "\nStore\n  users: 3\n".repeat(report::MAX_SECTIONS * 2);
        report.replied(arrived(&format!("Chatkord debug report: server\nCreated now\n{sections}")), WALL);
        assert!(report.shown().is_none());
        assert_eq!(report.line(), &ReportLine::Refused(failure::said(&LinkError::Malformed)));
    }

    #[test]
    fn a_log_of_thousands_of_lines_lists_the_newest_and_the_file_keeps_them_all() {
        let lines: String = (0..20_000).map(|at| format!("  12:00:00 info  voice a note  at={at}\n")).collect();
        let text = format!(
            "Chatkord debug report: server\nCreated now\n\nRecent log, newest last\n  notes too long to keep: 0\n{lines}"
        );
        let mut report = ServerReport::default();
        report.update(ReportMessage::Open);
        report.replied(arrived(&text), WALL);
        let shown = report.shown().expect("the report is shown");
        assert_eq!(shown.text(), text.as_str(), "the file holds every line");
        let listed = shown.listing().text(|sentence| words::Words::english().said(sentence));
        assert_eq!(listed.lines().count(), 2 + 3 + 1 + crate::client_report::LISTED_LOG_LINES, "{listed}");
        assert!(listed.contains("  Showing the last 200 of 20000 lines; the file holds them all.\n"), "{listed}");
        assert!(listed.ends_with("at=19999\n") && listed.contains("at=19800\n") && !listed.contains("at=19799\n"));
        assert!(listed.starts_with("Chatkord debug report: server\nCreated now\n\nRecent log, newest last\n"));
    }

    #[test]
    fn a_reply_nobody_asked_for_is_dropped() {
        let mut report = ServerReport::default();
        report.replied(arrived(TEXT), WALL);
        assert_eq!(report, ServerReport::default());
    }
}
