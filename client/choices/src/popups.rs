//! Whether a message that would alert shows a desktop popup: a choice of this device, kept across runs. When nothing
//! alerts is the account's status, which its server keeps.

/// Whether a message that would alert, with Chatkord not in front, shows a desktop popup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum Popups {
    /// A popup shows.
    #[default]
    On,
    /// None does.
    Off,
}

impl Popups {
    /// The other one, as the switch flips it.
    pub fn toggled(self) -> Popups {
        match self {
            Popups::On => Popups::Off,
            Popups::Off => Popups::On,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popups_flip() {
        assert_eq!(Popups::On.toggled(), Popups::Off);
        assert_eq!(Popups::default(), Popups::On);
    }
}
