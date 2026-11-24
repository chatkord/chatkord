//! Linux: the distribution's `os-release`, the kernel's release and which display server the session talks to.

use super::{Session, System, pretty_name};
use report::{Fact, Product};

pub(super) fn read() -> System {
    let name = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .map_or(Fact::Absent, |text| pretty_name(&text));
    let version = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map_or(Fact::Absent, |release| Product::fact_of(release.trim()));
    System { name, version, session: session(|variable| std::env::var_os(variable).is_some_and(|set| !set.is_empty())) }
}

/// Wayland when the session names a Wayland display, else X11 when it names an X display.
fn session(is_set: impl Fn(&str) -> bool) -> Session {
    if is_set("WAYLAND_DISPLAY") {
        Session::Wayland
    } else if is_set("DISPLAY") {
        Session::X11
    } else {
        Session::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wayland_display_wins_over_an_x_display_and_none_is_unknown() {
        assert_eq!(session(|_| true), Session::Wayland);
        assert_eq!(session(|name| name == "DISPLAY"), Session::X11);
        assert_eq!(session(|_| false), Session::Unknown);
    }
}
