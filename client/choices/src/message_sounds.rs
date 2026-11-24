//! Whether a message that would alert plays a sound: one switch, kept across runs.

/// Whether a message that would alert this account plays a sound, kept across runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum MessageSounds {
    /// The sound plays.
    #[default]
    On,
    /// It does not.
    Off,
}

impl MessageSounds {
    /// The other one, as the switch flips it.
    pub fn toggled(self) -> MessageSounds {
        match self {
            MessageSounds::On => MessageSounds::Off,
            MessageSounds::Off => MessageSounds::On,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_sounds_start_on_and_the_switch_flips_back() {
        assert_eq!(MessageSounds::default(), MessageSounds::On);
        assert_eq!(MessageSounds::On.toggled(), MessageSounds::Off);
        assert_eq!(MessageSounds::On.toggled().toggled(), MessageSounds::On);
    }
}
