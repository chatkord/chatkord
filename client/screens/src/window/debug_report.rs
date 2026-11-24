//! The client's debug report in the window: Download on About Chatkord makes it, the dialog shows it, and Download in
//! the dialog saves the report's whole text.

use super::*;
use crate::client_report::{self, Inputs, Machine, ReportMessage, Shown};

/// Where the report stands.
#[derive(Debug, Default)]
pub(super) enum Reporting {
    /// No report is asked for.
    #[default]
    Closed,
    /// The host was asked for the log and the moment.
    Reading,
    /// The log is read; the package's notices, which state the build's source, are still being read.
    Logged(client_report::Logs),
    /// The dialog shows the report.
    Open(Shown),
    /// The report could not be made, from its limits or for want of the time: About Chatkord says so until the next ask.
    Refused,
}

/// The debug report's dialog: whose report it lists, and what its Cancel and Download send. One dialog, either side's.
#[derive(Debug)]
pub struct ReportDialog<'a> {
    /// Whose report it is.
    pub side: report::Side,
    /// The report it lists, and saves as it is.
    pub shown: &'a Shown,
    /// What Cancel, the dialog's close, Escape and a press outside it send.
    pub cancel: Message,
    /// What its Download sends.
    pub download: Message,
}

/// What the debug report and About Chatkord state of this build and machine.
#[derive(Debug)]
pub(super) struct Reports {
    /// The build About Chatkord states.
    pub(super) build: Build,
    /// The package's notices.
    pub(super) carried: Carried,
    /// What the host read of this machine at the start, which the debug report states.
    pub(super) machine: Option<Machine>,
    /// How many devices of each kind were listed so far.
    pub(super) counted: Counted,
    /// The last calls that ended.
    pub(super) finished: Finished,
    /// Where the debug report stands.
    pub(super) reporting: Reporting,
    /// The file name the debug report was last saved under.
    pub(super) downloaded: Option<String>,
}

impl Default for Reports {
    fn default() -> Reports {
        Reports {
            build: Build::THIS,
            carried: Carried::Unasked,
            machine: None,
            counted: Counted::default(),
            finished: Finished::default(),
            reporting: Reporting::default(),
            downloaded: None,
        }
    }
}

impl Window {
    /// The report the client's dialog shows, while it is open.
    pub fn report_shown(&self) -> Option<&Shown> {
        match &self.report.reporting {
            Reporting::Open(shown) => Some(shown),
            Reporting::Closed | Reporting::Reading | Reporting::Logged(_) | Reporting::Refused => None,
        }
    }

    /// Whether the last report asked for could not be made.
    pub fn report_refused(&self) -> bool {
        matches!(self.report.reporting, Reporting::Refused)
    }

    /// The dialog that lists a report, the client's or a server's, while one is open.
    pub fn report_dialog(&self) -> Option<ReportDialog<'_>> {
        if let Some(shown) = self.report_shown() {
            return Some(ReportDialog {
                side: report::Side::Client,
                shown,
                cancel: Message::Report(ReportMessage::Cancel),
                download: Message::Report(ReportMessage::Download),
            });
        }
        self.places.iter().find_map(|place| {
            let shown = place.server_report()?.shown()?;
            let to = |message| {
                let page = place::PageMessage::About(crate::ServerAboutMessage::Report(message));
                Message::At(place.id(), place::Message::Page(page))
            };
            Some(ReportDialog {
                side: report::Side::Server,
                shown,
                cancel: to(ReportMessage::Cancel),
                download: to(ReportMessage::Download),
            })
        })
    }

    /// The file name the report was last saved under, which About Chatkord states.
    pub fn downloaded(&self) -> Option<&str> {
        self.report.downloaded.as_deref()
    }

    /// The machine told: the operating system and the graphics adapter, as sections of public facts.
    pub(super) fn machine_read(&mut self, machine: Machine) -> Vec<Ask> {
        self.report.machine = Some(machine);
        Vec::new()
    }

    /// The calls that ended since the last update, kept for the report.
    pub(super) fn keep_finished_calls(&mut self) {
        for place in &mut self.places {
            let address = place.server().url.as_str().to_owned();
            for figures in place.take_finished() {
                self.report.finished.push(&address, figures);
            }
        }
    }

    /// Something done about the report.
    pub(super) fn report_asked(&mut self, message: ReportMessage) -> Vec<Ask> {
        match (message, &mut self.report.reporting) {
            (ReportMessage::Open, reporting @ (Reporting::Closed | Reporting::Refused)) => {
                *reporting = Reporting::Reading;
                std::iter::once(Ask::ReadLog).chain(self.ask_notices()).collect()
            }
            (ReportMessage::Read(logs), Reporting::Reading) => {
                self.report.reporting =
                    if self.notices_pending() { Reporting::Logged(logs) } else { self.assembled(logs) };
                Vec::new()
            }
            (ReportMessage::Cancel, reporting) => {
                *reporting = Reporting::Closed;
                Vec::new()
            }
            (ReportMessage::Download, Reporting::Open(shown)) => {
                shown.download_pressed().map(Ask::SaveReport).into_iter().collect()
            }
            (ReportMessage::Saved(saved), Reporting::Open(shown)) => {
                if let Some(name) = shown.saved(saved) {
                    self.report.downloaded = Some(name);
                    self.report.reporting = Reporting::Closed;
                }
                Vec::new()
            }
            (ReportMessage::Open | ReportMessage::Read(_) | ReportMessage::Download | ReportMessage::Saved(_), _) => {
                Vec::new()
            }
        }
    }

    /// The notices are read: a report waiting on them is made.
    pub(super) fn report_stated(&mut self) {
        self.report.reporting = match std::mem::take(&mut self.report.reporting) {
            Reporting::Logged(logs) => self.assembled(logs),
            other => other,
        };
    }

    /// The report with `log`, open; refused where the wall clock is not known yet, which the file's name and date
    /// would otherwise state as 1970, or where the report does not fit its own limits.
    fn assembled(&self, logs: client_report::Logs) -> Reporting {
        let Some(created) = self.times.wall.and_then(|wall| u64::try_from(wall).ok()) else {
            return Reporting::Refused;
        };
        let stated = self.stated();
        let inputs = Inputs {
            stated: &stated,
            version: self.report.build.version,
            machine: self.report.machine.as_ref(),
            encoders: self.hardware.encoders.as_deref(),
            devices: self.report.counted,
            remembered: &self.voice,
            servers: self.places.iter().map(|place| (place.server().url.as_str(), place.rail())).collect(),
            calls: &self.report.finished,
            logs,
            created,
        };
        match client_report::assemble(inputs) {
            Ok(client_report::Assembled { report, name, listing }) => {
                Reporting::Open(Shown::new(client_report::Body::Report(report.into()), listing, name))
            }
            Err(_) => Reporting::Refused,
        }
    }
}
