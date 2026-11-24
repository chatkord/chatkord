//! How Chatkord starts with this device and what closing its window does: the three switches of Settings › Startup
//! and window, and whether the desktop shows a tray for them. Where and how each is kept is the host's.

/// Whether Chatkord opens when the person signs in to this device. The desktop's own entry is the fact: the host
/// reads it and writes it, and keeps no copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum AtSignIn {
    /// It opens at sign-in.
    Opens,
    /// It does not.
    #[default]
    Not,
}

/// Where a start at sign-in opens. Any other start opens the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum StartsIn {
    /// The window, as any start does.
    #[default]
    Window,
    /// The tray alone, where the desktop shows one.
    Tray,
}

/// What closing the window does where the desktop shows a tray; where it shows none, closing quits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum CloseTo {
    /// The window goes and Chatkord keeps running in the tray, a call with it.
    #[default]
    Tray,
    /// Chatkord quits, asking first during a call.
    Quit,
}

/// Whether the desktop shows Chatkord's tray icon now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tray {
    /// It does: the window can go to it.
    Shown,
    /// It shows none, or not yet.
    #[default]
    Absent,
}

/// How Chatkord starts and closes on this device, on a system that offers the choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Startup {
    /// Whether it opens at sign-in.
    pub at_sign_in: AtSignIn,
    /// Where such a start opens.
    pub starts_in: StartsIn,
    /// What closing the window does.
    pub close_to: CloseTo,
}

impl Startup {
    /// The first switch flipped; where a start at sign-in opens is kept either way.
    pub fn toggled_at_sign_in(self) -> Startup {
        let at_sign_in = match self.at_sign_in {
            AtSignIn::Opens => AtSignIn::Not,
            AtSignIn::Not => AtSignIn::Opens,
        };
        Startup { at_sign_in, ..self }
    }

    /// The second switch flipped, which is offered only while the first is on and holds its value while it is off.
    pub fn toggled_starts_in(self) -> Startup {
        let starts_in = match (self.at_sign_in, self.starts_in) {
            (AtSignIn::Opens, StartsIn::Window) => StartsIn::Tray,
            (AtSignIn::Opens, StartsIn::Tray) => StartsIn::Window,
            (AtSignIn::Not, held) => held,
        };
        Startup { starts_in, ..self }
    }

    /// The third switch flipped.
    pub fn toggled_close_to(self) -> Startup {
        let close_to = match self.close_to {
            CloseTo::Tray => CloseTo::Quit,
            CloseTo::Quit => CloseTo::Tray,
        };
        Startup { close_to, ..self }
    }

    /// Whether a close goes to the tray rather than quitting.
    pub fn closes_to_tray(self, tray: Tray) -> bool {
        self.close_to == CloseTo::Tray && tray == Tray::Shown
    }

    /// Whether a start at sign-in opens in the tray alone, with no window.
    pub fn starts_hidden(self, tray: Tray) -> bool {
        self.at_sign_in == AtSignIn::Opens && self.starts_in == StartsIn::Tray && tray == Tray::Shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_off_the_start_at_sign_in_keeps_its_start_in_the_tray_for_the_next_time_it_is_on() {
        let off = Startup::default();
        assert_eq!(off.toggled_starts_in(), off, "offered only with the first on");
        let on = off.toggled_at_sign_in().toggled_starts_in();
        assert_eq!((on.at_sign_in, on.starts_in), (AtSignIn::Opens, StartsIn::Tray));
        assert!(on.starts_hidden(Tray::Shown));
        assert!(!on.starts_hidden(Tray::Absent), "no tray, so the window opens");
        let held = on.toggled_at_sign_in();
        assert_eq!((held.at_sign_in, held.starts_in), (AtSignIn::Not, StartsIn::Tray));
        assert!(!held.starts_hidden(Tray::Shown), "it is a start at sign-in's alone");
        assert_eq!(held.toggled_starts_in(), held, "held, not flipped, while the first is off");
        assert_eq!(held.toggled_at_sign_in(), on);
    }

    #[test]
    fn a_close_goes_to_the_tray_only_where_one_is_shown_and_it_is_chosen() {
        let startup = Startup::default();
        assert!(startup.closes_to_tray(Tray::Shown), "on by default");
        assert!(!startup.closes_to_tray(Tray::Absent));
        assert!(!startup.toggled_close_to().closes_to_tray(Tray::Shown));
    }
}
