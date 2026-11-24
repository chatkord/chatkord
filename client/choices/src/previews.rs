//! Whether the links a person sends get a preview, which this device fetches: one choice of this device, kept across
//! runs.

/// Whether a link in a message about to be sent is opened from this device to read its title and picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum Previews {
    /// The first link of a message gets a preview.
    #[default]
    On,
    /// Nothing is fetched.
    Off,
}

impl Previews {
    /// The other one, as the switch flips it.
    pub fn toggled(self) -> Previews {
        match self {
            Previews::On => Previews::Off,
            Previews::Off => Previews::On,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_on_until_switched_off_and_the_switch_flips_both_ways() {
        assert_eq!(Previews::default(), Previews::On);
        assert_eq!(Previews::On.toggled(), Previews::Off);
        assert_eq!(Previews::Off.toggled(), Previews::On);
    }
}
