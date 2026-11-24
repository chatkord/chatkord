//! The client's debug report: what the machine, the choices, the servers' states and the last calls came to, as
//! public facts only, with the recent log, shown in a dialog and saved on request. The report is built from facts by
//! type (`report::Public`), so a name, an address or a message cannot reach it; ids stand as `server-1`.

mod build;
mod devices;
mod finished;
mod listing;

pub(crate) use build::{Assembled, Inputs, assemble};
pub(crate) use devices::Counted;
pub(crate) use finished::Finished;
pub use listing::{LISTED_LOG_LINES, Listing};

use report::{Previous, Recent, Report, Section};
use std::borrow::Cow;
use std::sync::Arc;

/// What the host reads of this machine once, when the window starts: the operating system and the graphics adapter,
/// each already a section of public facts.
#[derive(Clone, Debug)]
pub struct Machine {
    /// The operating system, its version and the kind of desktop session.
    pub system: Section,
    /// The graphics adapter, its driver and the graphics API.
    pub graphics: Section,
}

/// The logs a report holds.
#[derive(Clone, Debug, Default)]
pub struct Logs {
    /// This run's recent notes, oldest first, and how many were too long to keep.
    pub recent: Recent,
    /// The newest earlier run's notes, none when no run left a log.
    pub previous: Option<Previous>,
}

/// What was done with the Download button and the dialog it opens.
#[derive(Clone, Debug)]
pub enum ReportMessage {
    /// The page's Download: the report is made and shown.
    Open,
    /// What the host read of the logs, the answer to the one ask.
    Read(Logs),
    /// Cancel, the dialog's ✕, Escape or a press outside it.
    Cancel,
    /// The dialog's Download: the system's Save dialog is asked for.
    Download,
    /// What the Save dialog came to.
    Saved(Saved),
}

/// What asking the system where to save the report came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Saved {
    /// It was written, under this file name.
    Written(String),
    /// The person closed the Save dialog.
    Declined,
    /// The system has no Save dialog, or the file could not be written.
    Failed,
}

/// What a report's file holds: the text as it arrived, or the client's report, whose text is written only when the
/// file is, so the window never writes megabytes of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Body {
    Text(Arc<str>),
    Report(Arc<Report>),
}

impl Body {
    fn text(&self) -> Cow<'_, str> {
        match self {
            Self::Text(text) => Cow::Borrowed(text),
            Self::Report(report) => Cow::Owned(report.text()),
        }
    }
}

/// The file the host is to write: the report's whole text, under the name offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    /// The name offered in the Save dialog.
    pub name: String,
    body: Body,
}

impl Download {
    /// A file of `text` under `name`.
    pub fn of_text(name: String, text: &str) -> Download {
        Download { name, body: Body::Text(text.into()) }
    }

    /// What the file holds: the report's text, byte for byte. A client's report is written here, so this is called
    /// where the file is, off the window's thread.
    pub fn bytes(&self) -> Cow<'_, [u8]> {
        match self.body.text() {
            Cow::Borrowed(text) => Cow::Borrowed(text.as_bytes()),
            Cow::Owned(text) => Cow::Owned(text.into_bytes()),
        }
    }
}

/// The report the dialog shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub(crate) body: Body,
    pub(crate) listing: Listing,
    pub(crate) name: String,
    pub(crate) saving: bool,
    pub(crate) failed: bool,
}

impl Shown {
    /// A report to save as `name` whole and list as `listing`, of either side.
    pub(crate) fn new(body: Body, listing: Listing, name: String) -> Shown {
        Shown { body, listing, name, saving: false, failed: false }
    }

    /// Download pressed: the file to save, none while a Save dialog is already open.
    pub(crate) fn download_pressed(&mut self) -> Option<Download> {
        if self.saving {
            return None;
        }
        self.saving = true;
        self.failed = false;
        Some(self.download())
    }

    /// What the Save dialog came to: the file name when it was written (the dialog is done), else the dialog stays,
    /// saying so where the save failed.
    pub(crate) fn saved(&mut self, saved: Saved) -> Option<String> {
        if !self.saving {
            return None;
        }
        self.saving = false;
        match saved {
            Saved::Written(name) => Some(name),
            Saved::Declined => None,
            Saved::Failed => {
                self.failed = true;
                None
            }
        }
    }

    /// The report's whole text, which Download saves as it is. A client's report writes it here: for a test, not for
    /// a frame.
    pub fn text(&self) -> Cow<'_, str> {
        self.body.text()
    }

    /// What the dialog lists of it: every section, and of a log only its newest lines.
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// Whether the Save dialog is open and its answer awaited.
    pub fn saving(&self) -> bool {
        self.saving
    }

    /// Whether the last save did not happen: the dialog says so.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// What the Download button asks for.
    pub(crate) fn download(&self) -> Download {
        Download { name: self.name.clone(), body: self.body.clone() }
    }
}
