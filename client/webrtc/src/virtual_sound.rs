//! The listed name of a virtual device (`testserver::sound`), for a test that chooses it.

use crate::devices::Device;

/// The device of `devices` whose name holds `description`, by the id it is chosen by.
pub fn listed(devices: &[Device], description: &str) -> Option<String> {
    devices.iter().find(|device| device.name.contains(description)).and_then(|d| d.asked_as().map(str::to_string))
}
