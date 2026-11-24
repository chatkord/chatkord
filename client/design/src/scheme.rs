//! Light or dark, and what the operating system says a person prefers. The host reads the system
//! and hands the answer in as an [`Appearance`]; nothing here asks the system.

use crate::listed;

listed! {
    /// The two schemes every colour is defined for.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Scheme {
        /// Dark text on light grounds.
        Light,
        /// Light text on dark grounds.
        Dark,
    }
}

/// The person's own choice, which wins over the system's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preference {
    #[default]
    /// Whatever the operating system says.
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Whether movement on screen is welcome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionPreference {
    #[default]
    /// Movement is welcome.
    Full,
    /// Less movement is asked for.
    Reduced,
}

/// What the operating system says, as read by the host. A system that says nothing about its
/// scheme leaves `scheme` absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    /// The scheme the system prefers, if it says.
    pub scheme: Option<Scheme>,
    /// Whether the system asks for less movement.
    pub motion: MotionPreference,
}

impl Preference {
    /// The scheme to draw: the person's choice, else the system's, else dark, because a call is
    /// often watched in a dim room and a sudden white window is the worse surprise.
    pub fn resolve(self, appearance: Appearance) -> Scheme {
        match (self, appearance.scheme) {
            (Preference::Light, _) => Scheme::Light,
            (Preference::Dark, _) => Scheme::Dark,
            (Preference::System, Some(system)) => system,
            (Preference::System, None) => Scheme::Dark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_person_wins_then_the_system_then_dark() {
        let light_system = Appearance { scheme: Some(Scheme::Light), ..Appearance::default() };
        assert_eq!(Preference::Dark.resolve(light_system), Scheme::Dark);
        assert_eq!(Preference::System.resolve(light_system), Scheme::Light);
        assert_eq!(Preference::Light.resolve(Appearance::default()), Scheme::Light);
        assert_eq!(Preference::System.resolve(Appearance::default()), Scheme::Dark);
    }
}
