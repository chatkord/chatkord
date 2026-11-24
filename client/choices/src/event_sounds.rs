//! Whether the window's own sounds play: one switch, kept across runs.

/// Whether the window's own sounds play, kept across runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum EventSounds {
    /// Every one plays.
    #[default]
    On,
    /// None does.
    Off,
}

impl EventSounds {
    /// The other one, as the switch flips it.
    pub fn toggled(self) -> EventSounds {
        match self {
            EventSounds::On => EventSounds::Off,
            EventSounds::Off => EventSounds::On,
        }
    }
}
