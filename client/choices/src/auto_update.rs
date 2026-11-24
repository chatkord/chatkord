//! Whether Chatkord downloads its own updates: one switch, kept across runs.

/// Whether Chatkord checks for a newer version by itself and downloads it, kept across runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum AutoUpdate {
    /// It checks when it starts and every few hours, and downloads what it finds.
    #[default]
    On,
    /// It checks only when asked.
    Off,
}

impl AutoUpdate {
    /// The other one, as the switch flips it.
    pub fn toggled(self) -> AutoUpdate {
        match self {
            AutoUpdate::On => AutoUpdate::Off,
            AutoUpdate::Off => AutoUpdate::On,
        }
    }
}
