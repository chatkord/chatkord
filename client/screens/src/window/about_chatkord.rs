//! Settings › About Chatkord: this build's version and licence, and the notices the package carries, read once when
//! the page first opens, one group open at a time.

use super::*;

/// A build, as About Chatkord states it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    /// Its version, version.txt's.
    pub version: &'static str,
    /// The protocol it speaks, the one the protocol's `wire/PROTOCOL` names.
    pub protocol: proto::chatkord_v1::ProtoVersion,
    /// Its licence's id, as its manifest names it.
    pub licence: &'static str,
}

impl Build {
    /// The build running.
    pub const THIS: Build =
        Build { version: terms::BUILD, protocol: proto::envelope::PROTO, licence: env!("CARGO_PKG_LICENSE") };
}

/// The package's notices, as About Chatkord has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Carried {
    /// Not asked for: the page has not opened.
    Unasked,
    /// Asked for, not read yet.
    Reading,
    /// There are none to show: a build run from its tree, or a file that was refused.
    Absent,
    /// Read, with the group that is open.
    Read {
        /// Every group, in order.
        notices: notices::Notices,
        /// The group open, by its place among them.
        open: Option<usize>,
    },
}

/// What About Chatkord states of the build: the package's own version and source once its notices are read, else
/// the running tree's version and no source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stated {
    /// The version: the package's, or the tree's `version.txt`.
    pub version: String,
    /// The protocol the client speaks, said as [`crate::protocol_said`] says a server's.
    pub protocol: String,
    /// The licence's id, as the manifest names it.
    pub licence: &'static str,
    /// Where the source is; none for a build run from its own tree.
    pub source: Option<notices::Origin>,
}

impl Window {
    /// The build About Chatkord states.
    pub fn build(&self) -> Build {
        self.report.build
    }

    /// What About Chatkord states of the build.
    pub fn stated(&self) -> Stated {
        let packaged = match &self.report.carried {
            Carried::Read { notices, .. } => notices.built(),
            Carried::Unasked | Carried::Reading | Carried::Absent => None,
        };
        let (version, source) = match packaged {
            Some(built) => (built.version().to_string(), Some(built.from().clone())),
            None => (self.report.build.version.to_string(), None),
        };
        Stated {
            version,
            protocol: crate::protocol_said(self.report.build.protocol),
            licence: self.report.build.licence,
            source,
        }
    }

    /// Each signed-in server that named its release, by what a person calls it, in the rail's order.
    pub fn server_builds(&self) -> Vec<(&str, &terms::ServerBuild)> {
        self.places()
            .iter()
            .filter(|place| place.signed_in())
            .filter_map(|place| Some((place.server().shown(), place.build()?)))
            .collect()
    }

    /// The window stating `build` in place of the one running: a fixed version for a picture of the page.
    pub fn built_as(mut self, build: Build) -> Window {
        self.report.build = build;
        self
    }

    /// The package's notices, as About Chatkord shows them.
    pub fn carried(&self) -> &Carried {
        &self.report.carried
    }

    /// About Chatkord opened: every group closed, and the notices asked for the first time.
    pub(super) fn open_about(&mut self) -> Vec<Ask> {
        if let Carried::Read { open, .. } = &mut self.report.carried {
            *open = None;
        }
        self.ask_notices().into_iter().collect()
    }

    /// The notices asked for, the first time anything needs them: About Chatkord, or the debug report, which states
    /// the source they name.
    pub(super) fn ask_notices(&mut self) -> Option<Ask> {
        matches!(self.report.carried, Carried::Unasked).then(|| {
            self.report.carried = Carried::Reading;
            Ask::ReadNotices
        })
    }

    /// Whether the notices are asked for and not read yet.
    pub(super) fn notices_pending(&self) -> bool {
        matches!(self.report.carried, Carried::Reading)
    }

    /// What the host read of the notices; only the answer to the one ask counts.
    pub(super) fn notices_read(&mut self, read: Option<notices::Notices>) -> Vec<Ask> {
        if matches!(self.report.carried, Carried::Reading) {
            self.report.carried = read.map_or(Carried::Absent, |notices| Carried::Read { notices, open: None });
            self.report_stated();
        }
        Vec::new()
    }

    /// A group pressed: it opens and the one open closes, or it closes.
    pub(super) fn toggle_notice(&mut self, group: usize) -> Vec<Ask> {
        if let Carried::Read { notices, open } = &mut self.report.carried
            && group < notices.groups().len()
        {
            *open = (*open != Some(group)).then_some(group);
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, notices, two_signed_in};

    fn opened(window: &mut Window, clock: &Clock) -> Vec<Ask> {
        window.update(Message::OpenDevice(DevicePage::AboutChatkord), clock.at())
    }

    #[test]
    fn the_notices_are_asked_for_once_when_the_page_first_opens() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        assert_eq!(window.carried(), &Carried::Unasked, "nothing read before the page opens");
        assert!(opened(&mut window, &clock).iter().any(|ask| matches!(ask, Ask::ReadNotices)));
        assert_eq!((window.device(), window.carried()), (Some(DevicePage::AboutChatkord), &Carried::Reading));
        assert!(!opened(&mut window, &clock).iter().any(|ask| matches!(ask, Ask::ReadNotices)), "asked already");
        window.update(Message::NoticesRead(Some(notices())), clock.at());
        assert_eq!(window.carried(), &Carried::Read { notices: notices(), open: None });
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        assert!(!opened(&mut window, &clock).iter().any(|ask| matches!(ask, Ask::ReadNotices)), "read once a run");
    }

    #[test]
    fn the_page_states_the_packages_own_version_and_source_and_a_tree_its_version_alone() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock).built_as(built("0.1.0"));
        let tree =
            Stated { version: "0.1.0".into(), protocol: "1.12".into(), licence: "GPL-3.0-or-later", source: None };
        assert_eq!(window.stated(), tree, "before the notices are read: the tree's version, no source");
        opened(&mut window, &clock);
        let pre = br#"{"built": {"version": "0.1.1-dev.20261114", "from": {"commit": "1a2b3c4d"}}, "groups": []}"#;
        window.update(Message::NoticesRead(notices::Notices::parse(pre).ok()), clock.at());
        let packaged = Stated {
            version: "0.1.1-dev.20261114".into(),
            protocol: "1.12".into(),
            licence: "GPL-3.0-or-later",
            source: Some(Origin::Commit("1a2b3c4d".into())),
        };
        assert_eq!(window.stated(), packaged, "a pre-release off a tag says its own version and commit");
        let mut bare = two_signed_in(&clock).built_as(built("0.1.0"));
        opened(&mut bare, &clock);
        bare.update(Message::NoticesRead(None), clock.at());
        assert_eq!(bare.stated(), tree, "a build with no package");
    }

    #[test]
    fn a_report_asked_before_about_chatkord_ever_opened_waits_for_the_notices_and_states_their_source() {
        use crate::client_report::{Logs, ReportMessage};
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock).built_as(built("0.1.0"));
        let asked = window.update(Message::Report(ReportMessage::Open), clock.at());
        assert!(matches!(&asked[..], [Ask::ReadLog, Ask::ReadNotices]), "{asked:?}");
        window.update(Message::Report(ReportMessage::Read(Logs::default())), clock.at());
        assert!(window.report_shown().is_none(), "the log alone does not make it");
        let pre = br#"{"built": {"version": "0.1.1-dev.20261114", "from": {"commit": "1a2b3c4d"}}, "groups": []}"#;
        window.update(Message::NoticesRead(notices::Notices::parse(pre).ok()), clock.at());
        let text = window.report_shown().map(|shown| shown.text().into_owned()).unwrap_or_default();
        assert!(text.contains("(pre-release)") && text.contains("source commit: 1a2b3c4d"), "{text}");
        window.update(Message::Report(ReportMessage::Cancel), clock.at());
        let asked = window.update(Message::Report(ReportMessage::Open), clock.at());
        assert!(matches!(&asked[..], [Ask::ReadLog]), "read once a run: {asked:?}");
    }

    #[test]
    fn the_clients_protocol_is_the_one_the_wire_names() {
        let clock = Clock(Instant::now());
        let window = two_signed_in(&clock);
        assert_eq!(Build::THIS.protocol, proto::envelope::PROTO);
        assert_eq!(window.stated().protocol, crate::protocol_said(proto::envelope::PROTO));
    }

    fn built(version: &'static str) -> Build {
        let protocol = proto::chatkord_v1::ProtoVersion { major: 1, minor: 12 };
        Build { version, protocol, licence: "GPL-3.0-or-later" }
    }

    #[test]
    fn each_signed_in_server_that_named_its_release_is_listed_in_the_rails_order() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls, books) = (crate::harness::id(&window, 0), crate::harness::id(&window, 1));
        let build = |text: &str| terms::ServerBuild::read(text).ok().flatten();
        assert_eq!(window.server_builds(), [], "none has greeted a release");
        let told = |window: &mut Window, place, text: &str| {
            crate::harness::tell(
                window,
                &clock,
                place,
                place::Message::Server(place::ServerMessage::Built(build(text))),
            );
        };
        told(&mut window, books, "0.8.2");
        told(&mut window, owls, "0.9.0");
        let listed: Vec<_> = window.server_builds().into_iter().map(|(name, build)| (name, build.as_str())).collect();
        assert_eq!(listed, [("Night Owls", "0.9.0"), ("Book club", "0.8.2")]);
        told(&mut window, owls, "");
        let listed: Vec<_> = window.server_builds().into_iter().map(|(name, _)| name).collect();
        assert_eq!(listed, ["Book club"], "a server that names none is left out");
    }

    #[test]
    fn a_build_without_notices_says_so_and_never_asks_again() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::NoticesRead(Some(notices())), clock.at());
        assert_eq!(window.carried(), &Carried::Unasked, "an answer to no ask is not taken");
        opened(&mut window, &clock);
        window.update(Message::NoticesRead(None), clock.at());
        assert_eq!(window.carried(), &Carried::Absent);
        window.update(Message::NoticesRead(Some(notices())), clock.at());
        assert_eq!(window.carried(), &Carried::Absent, "a second answer is not taken either");
        assert!(!opened(&mut window, &clock).iter().any(|ask| matches!(ask, Ask::ReadNotices)));
    }

    #[test]
    fn one_group_is_open_at_a_time_and_the_page_opens_with_none() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        opened(&mut window, &clock);
        window.update(Message::NoticesRead(Some(notices())), clock.at());
        let open = |window: &Window| match window.carried() {
            Carried::Read { open, .. } => *open,
            Carried::Unasked | Carried::Reading | Carried::Absent => None,
        };
        window.update(Message::ToggleNotice(0), clock.at());
        assert_eq!(open(&window), Some(0));
        window.update(Message::ToggleNotice(1), clock.at());
        assert_eq!(open(&window), Some(1), "another closes the one open");
        window.update(Message::ToggleNotice(1), clock.at());
        assert_eq!(open(&window), None, "pressed again, it closes");
        window.update(Message::ToggleNotice(notices().groups().len()), clock.at());
        assert_eq!(open(&window), None, "no such group");
        window.update(Message::ToggleNotice(0), clock.at());
        window.update(Message::CloseDevice, clock.at());
        opened(&mut window, &clock);
        assert_eq!(open(&window), None, "opened again, every group is closed");
    }
}
