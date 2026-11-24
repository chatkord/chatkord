//! The operating system a debug report states: its name and version, the kind of desktop session, and the processor
//! family. Only a product name that passes [`report::Product::parse`] or a word written here is ever stated, never a host,
//! user or folder name.

use report::{Fact, Full, Section, word};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// The kind of desktop session Chatkord runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Session {
    /// A Wayland compositor.
    Wayland,
    /// An X11 server.
    X11,
    /// Windows' own desktop.
    Windows,
    /// The system names none.
    Unknown,
}

/// What this system states of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System {
    /// The system's name and release, as it names itself: a distribution's pretty name, or Windows' product name.
    pub name: Fact,
    /// The kernel's or build's version.
    pub version: Fact,
    /// The kind of desktop session.
    pub session: Session,
}

/// This system, read now: a few small files or registry values.
#[must_use]
pub fn read() -> System {
    #[cfg(target_os = "linux")]
    return linux::read();
    #[cfg(windows)]
    return windows::read();
    #[cfg(not(any(target_os = "linux", windows)))]
    return System { name: Fact::Absent, version: Fact::Absent, session: Session::Unknown };
}

/// The report's "System" section for `system`.
///
/// # Errors
///
/// [`Full`] when a row does not fit, which its fixed rows never do.
pub fn section(system: &System) -> Result<Section, Full> {
    let System { name, version, session } = system;
    let mut rows = Section::new("System");
    rows.push("system", name)?;
    rows.push("version", version)?;
    rows.push("session", session)?;
    rows.push("processor family", &family(std::env::consts::ARCH))?;
    Ok(rows)
}

/// The processor family as a word, or withheld for one this build has no word for.
fn family(arch: &str) -> Fact {
    match arch {
        "x86_64" => Fact::Word(word!("x86_64")),
        "aarch64" => Fact::Word(word!("aarch64")),
        _ => Fact::Withheld,
    }
}

/// A pretty name read from `os-release`'s text: its `PRETTY_NAME`, quotes off, as a product name.
#[cfg(any(target_os = "linux", test))]
fn pretty_name(os_release: &str) -> Fact {
    let value = os_release.lines().find_map(|line| line.strip_prefix("PRETTY_NAME="));
    value.map_or(Fact::Absent, |value| report::Product::fact_of(value.trim().trim_matches(['"', '\''])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pretty_name_is_stated_when_it_is_a_product_name_and_withheld_otherwise() {
        let said = |text: &str| pretty_name(text).to_string();
        assert_eq!(
            said("NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 43 (Workstation Edition)\"\n"),
            "Fedora Linux 43 (Workstation Edition)"
        );
        assert_eq!(said("PRETTY_NAME=Arch Linux"), "Arch Linux");
        assert_eq!(said("PRETTY_NAME=\"home @ ada\""), "withheld");
        assert_eq!(said("NAME=x"), "none");
    }

    #[test]
    fn a_section_states_the_session_by_its_word() {
        let system = System { name: Fact::Absent, version: Fact::Absent, session: Session::Wayland };
        let section = section(&system).expect("fixed rows fit");
        let build = report::Build { program: "Chatkord", version: "0", source: "test" };
        let mut report = report::Report::new(report::Side::Client, build, 0);
        report.section(section).expect("one section fits");
        assert!(report.text().contains("session: Wayland"));
    }
}
