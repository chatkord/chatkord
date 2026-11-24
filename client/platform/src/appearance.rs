//! What the system says about how a screen should look, its colour scheme and whether it asks for less motion, as a
//! `design::Appearance`. A system that does not answer, or has no preference, leaves the core's default.

use design::{Appearance, MotionPreference, Scheme};
use mundy::{ColorScheme, Interest, Preferences, ReducedMotion, Subscription};
use std::time::Duration;

fn interest() -> Interest {
    Interest::ColorScheme | Interest::ReducedMotion
}

/// How long the first read waits for the system before giving up and saying nothing.
pub const FIRST_READ: Duration = Duration::from_millis(500);

/// The appearance a set of the system's preferences amounts to.
pub fn appearance_of(preferences: &Preferences) -> Appearance {
    Appearance {
        scheme: match preferences.color_scheme {
            ColorScheme::Light => Some(Scheme::Light),
            ColorScheme::Dark => Some(Scheme::Dark),
            ColorScheme::NoPreference => None,
        },
        motion: match preferences.reduced_motion {
            ReducedMotion::Reduce => MotionPreference::Reduced,
            ReducedMotion::NoPreference => MotionPreference::Full,
        },
    }
}

/// The system's appearance now, waiting at most [`FIRST_READ`]. An unanswered system is the default.
pub fn read() -> Appearance {
    Preferences::once_blocking(interest(), FIRST_READ)
        .map(|preferences| appearance_of(&preferences))
        .unwrap_or_default()
}

/// A running watch on the system's appearance; it ends when dropped.
pub struct Watch {
    _subscription: Subscription,
}

/// Calls `changed` with the appearance now and each time the person changes it, from another thread.
pub fn watch(changed: impl Fn(Appearance) + Send + Sync + 'static) -> Watch {
    Watch { _subscription: Preferences::subscribe(interest(), move |preferences| changed(appearance_of(&preferences))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_system_prefers_becomes_the_cores_appearance() {
        let mut dark_and_calm = Preferences::default();
        dark_and_calm.color_scheme = ColorScheme::Dark;
        dark_and_calm.reduced_motion = ReducedMotion::Reduce;
        assert_eq!(
            appearance_of(&dark_and_calm),
            Appearance { scheme: Some(Scheme::Dark), motion: MotionPreference::Reduced }
        );
        let mut light = Preferences::default();
        light.color_scheme = ColorScheme::Light;
        assert_eq!(appearance_of(&light), Appearance { scheme: Some(Scheme::Light), motion: MotionPreference::Full });
    }

    #[test]
    fn no_preference_is_no_scheme_and_full_motion() {
        assert_eq!(appearance_of(&Preferences::default()), Appearance { scheme: None, motion: MotionPreference::Full });
    }
}
