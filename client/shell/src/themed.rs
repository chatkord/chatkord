//! The theme a scene draws with, built once per scheme, accent and contrast: a `ui::Theme` is a palette search, too dear to
//! run on every turn of the loop. The motion the system asks for changes it without a search.

use design::{Accent, Contrast, MotionPreference, Scheme};

/// A `ui::Theme` and what it was built for, so it is built again only when that changes.
pub struct Themed {
    built_for: (Scheme, Accent, Contrast),
    theme: ui::Theme,
}

impl Themed {
    /// The theme of `scheme`, `accent` and `contrast`, built now, with full motion.
    pub fn new(scheme: Scheme, accent: Accent, contrast: Contrast) -> Self {
        Self { built_for: (scheme, accent, contrast), theme: ui::Theme::with_contrast(scheme, accent, contrast) }
    }

    /// Follows the window into `scheme` with `accent` at `contrast` and `motion`, building the theme again only when one
    /// of the first three changed; true when anything did.
    pub fn follow(&mut self, scheme: Scheme, accent: Accent, contrast: Contrast, motion: MotionPreference) -> bool {
        let searched = self.built_for != (scheme, accent, contrast);
        if searched {
            *self = Self::new(scheme, accent, contrast);
        }
        let moved = self.theme.motion() != motion;
        if moved {
            self.theme = self.theme.clone().with_motion(motion);
        }
        searched || moved
    }

    /// The theme to draw with now.
    pub fn theme(&self) -> &ui::Theme {
        &self.theme
    }
}

#[cfg(test)]
mod tests {
    use super::Themed;
    use design::{Accent, Contrast, MotionPreference, Scheme};
    use iced_winit::core::theme::{Base, Mode};

    #[test]
    fn a_theme_is_built_again_only_when_the_scheme_the_accent_or_the_contrast_changes() {
        let (standard, full) = (Contrast::Standard, MotionPreference::Full);
        let mut themed = Themed::new(Scheme::Dark, Accent::DEFAULT, standard);
        assert!(!themed.follow(Scheme::Dark, Accent::DEFAULT, standard, full), "the same scheme builds nothing");
        assert!(themed.follow(Scheme::Light, Accent::DEFAULT, standard, full), "another scheme builds the theme again");
        assert_eq!(themed.theme().mode(), Mode::Light);
        assert!(!themed.follow(Scheme::Light, Accent::DEFAULT, standard, full));
        assert!(themed.follow(Scheme::Light, Accent::DEFAULT, Contrast::More, full), "and another contrast");
        assert_eq!(themed.theme().palette().contrast(), Contrast::More);
        assert!(themed.follow(Scheme::Light, Accent::DEFAULT, Contrast::More, MotionPreference::Reduced), "and motion");
        assert_eq!(themed.theme().motion(), MotionPreference::Reduced);
        assert_eq!(themed.theme().mode(), Mode::Light, "the palette stays");
    }
}
