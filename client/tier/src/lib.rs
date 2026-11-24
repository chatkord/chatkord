//! Which machine a test needs: the tier is the module the test is in (`server`, `lab`), and a test nextest ran
//! outside that tier's group fails, so one nobody placed cannot pass unseen. Outside nextest nothing is checked.
#![forbid(unsafe_code)]
#![allow(clippy::panic, reason = "a test harness: a test in the wrong tier fails")]

use std::fmt::Display;

/// What a test needs beyond its own process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    /// A server container.
    Server,
    /// Hardware or a desktop: a GPU, an encoder, a sound server or device, PipeWire, a session. A server too.
    Lab,
}

impl Tier {
    /// Its module, and the nextest group that module's tests run in.
    fn name(self) -> &'static str {
        match self {
            Tier::Server => "server",
            Tier::Lab => "lab",
        }
    }

    fn covers(self, wanted: Tier) -> bool {
        self == wanted || self == Tier::Lab
    }
}

/// Fails the test unless it runs where a server may be started.
pub fn server() {
    check(Tier::Server);
}

/// Fails the test unless it runs where hardware is; every probe of hardware calls this first.
pub fn lab() {
    check(Tier::Lab);
}

/// Says the hardware this test needs is absent, which proves nothing, so it is said rather than passed quietly.
#[expect(clippy::print_stderr, reason = "a skip is said in the test's own output")]
pub fn skipped(why: impl Display) {
    lab();
    eprintln!("SKIPPED: {why}");
}

/// Whether this test runs under Khronos' validation layer, so a validation error ends it: `scripts/vulkan-validation.sh`
/// turns the layer on for the tests `client/.config/nextest.toml` names, where the Vulkan loader finds it. Where it
/// does not, the test is skipped, saying how to install it.
pub fn validated() -> bool {
    lab();
    let on = std::env::var("VK_INSTANCE_LAYERS").is_ok_and(|layers| layers.contains(VALIDATION));
    if !on {
        skipped(format!(
            "{VALIDATION} is not installed (Fedora: vulkan-validation-layers; Windows: the LunarG Vulkan SDK)"
        ));
    }
    on
}

const VALIDATION: &str = "VK_LAYER_KHRONOS_validation";

/// Whether this test opens its D3D devices under the D3D debug layer and ends on what it reports:
/// `scripts/d3d-validation.sh` says so for the tests `client/.config/nextest.toml` names, where Windows' Graphics Tools
/// are installed; elsewhere the test runs as a person's machine would.
pub fn d3d_validated() -> bool {
    lab();
    std::env::var_os("D3D_VALIDATION").is_some()
}

/// The ids of the D3D debug layer's messages a validated test passes over, each a known issue of another project's
/// that the setup script names.
pub fn d3d_known() -> Vec<i32> {
    std::env::var("D3D_VALIDATION").unwrap_or_default().split(',').filter_map(|id| id.trim().parse().ok()).collect()
}

fn check(wanted: Tier) {
    if let Err(placed) = verdict(std::env::var("NEXTEST_TEST_GROUP").ok().as_deref(), wanted) {
        panic!("{placed}");
    }
}

/// Whether a test that needs `wanted` may run in nextest's `group` (`None`: not under nextest).
fn verdict(group: Option<&str>, wanted: Tier) -> Result<(), String> {
    let Some(group) = group else { return Ok(()) };
    let ran = [Tier::Server, Tier::Lab].into_iter().find(|tier| tier.name() == group);
    match ran {
        Some(ran) if ran.covers(wanted) => Ok(()),
        _ => Err(format!(
            "this test needs the {0} tier: put it in a `{0}` module; nextest ran it in {group}",
            wanted.name()
        )),
    }
}

/// A device of the machine, held by one test at a time across every test process: what one test makes, unloads or
/// opens at a mode of its own, another would take or refuse. Released when dropped.
pub struct Turn {
    _held: std::fs::File,
}

/// Waits for the machine's turn at `device`; none when its lock file cannot be opened.
pub fn turn(device: &str) -> Option<Turn> {
    let path = std::env::temp_dir().join(format!("chatkord_{device}.lock"));
    let file = std::fs::File::options().create(true).truncate(false).write(true).open(path).ok()?;
    file.lock().ok()?;
    Some(Turn { _held: file })
}

#[cfg(test)]
mod tests {
    use super::{Tier, verdict};

    #[test]
    fn a_test_runs_in_its_own_tier_and_a_server_test_in_the_lab_too() {
        assert_eq!(verdict(Some("server"), Tier::Server), Ok(()));
        assert_eq!(verdict(Some("lab"), Tier::Lab), Ok(()));
        assert_eq!(verdict(Some("lab"), Tier::Server), Ok(()));
    }

    #[test]
    fn a_test_outside_its_tier_is_told_where_it_belongs() {
        let refused = verdict(Some("@global"), Tier::Lab).unwrap_err();
        assert!(refused.contains("put it in a `lab` module") && refused.ends_with("@global"), "{refused}");
        assert!(verdict(Some("server"), Tier::Lab).is_err());
        assert!(verdict(Some("@global"), Tier::Server).is_err());
    }

    #[test]
    fn outside_nextest_nothing_is_checked() {
        assert_eq!(verdict(None, Tier::Lab), Ok(()));
    }
}
