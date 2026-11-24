//! Server settings › About, an admin's: the server's release and protocol, which the place holds from its greeting,
//! and the server's debug report, asked for here.

use crate::client_report::{Download, ReportMessage};
use crate::server_report::{self, ServerReport};

/// The page, open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerAbout {
    pub(crate) report: ServerReport,
}

/// What a person does on the page.
#[derive(Clone, Debug)]
pub enum Message {
    /// Something done with the server's debug report: its Download, and the dialog that lists it.
    Report(ReportMessage),
}

/// What the page asks: of the server its debug report, and of the system where to save one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    ServerReport,
    SaveReport(Download),
}

impl ServerAbout {
    /// The server's debug report: the card's line and button, and the dialog that lists it.
    pub fn report(&self) -> &ServerReport {
        &self.report
    }

    pub(crate) fn update(&mut self, message: Message) -> Option<Ask> {
        match message {
            Message::Report(message) => self.report.update(message).map(|asked| match asked {
                server_report::Ask::Report => Ask::ServerReport,
                server_report::Ask::Save(download) => Ask::SaveReport(download),
            }),
        }
    }
}
