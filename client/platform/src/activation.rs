//! The token a desktop hands a program so its window may take the focus (`xdg-activation`): given to a launched
//! process in its environment and to a clicked popup's owner by the notification server.

use std::fmt;

/// The longest token taken, in bytes; KWin's and Mutter's are a few dozen.
pub const MAX: usize = 256;

/// The environment variable a Wayland desktop hands a launched process its token in.
const ENV: &str = "XDG_ACTIVATION_TOKEN";

/// A token that lets this process's window take the focus; it is a capability, so it is never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Activation(String);

/// A token over [`MAX`] bytes, empty, or holding anything but printable ASCII.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

impl Activation {
    /// The token `given`, refused, never cut, unless it is 1 to [`MAX`] bytes of printable ASCII without a space.
    pub fn new(given: String) -> Result<Self, Refused> {
        let printable = given.bytes().all(|byte| byte.is_ascii_graphic());
        if given.is_empty() || given.len() > MAX || !printable {
            return Err(Refused);
        }
        Ok(Self(given))
    }

    /// The token the desktop launched this process with, where it gave one that [`Activation::new`] takes, taken out
    /// of the environment so no process this one starts is handed it spent.
    ///
    /// # Safety
    ///
    /// No other thread runs: the environment is changed.
    #[must_use]
    pub unsafe fn take_from_env() -> Option<Self> {
        let given = std::env::var(ENV).ok();
        // SAFETY: the caller promises no other thread reads or writes the environment.
        unsafe { std::env::remove_var(ENV) };
        given.and_then(|given| Self::new(given).ok())
    }

    /// The token as the windowing system takes it.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Activation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Activation([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_within_its_bound_is_taken() {
        let token = "k".repeat(MAX);
        assert_eq!(Activation::new(token.clone()).map(|taken| taken.token().to_owned()), Ok(token));
        assert!(Activation::new("kwin-12_ab.CD:9".into()).is_ok());
    }

    #[test]
    fn a_token_over_its_bound_or_not_printable_is_refused_not_cut() {
        assert_eq!(Activation::new("k".repeat(MAX + 1)), Err(Refused));
        assert_eq!(Activation::new(String::new()), Err(Refused));
        for wrong in ["a b", "a\nb", "a\u{7f}", "é"] {
            assert_eq!(Activation::new(wrong.into()), Err(Refused), "{wrong:?}");
        }
    }

    #[test]
    fn a_token_is_never_printed() {
        let token = Activation::new("secret-token".into()).unwrap();
        assert!(!format!("{token:?}").contains("secret-token"));
    }
}
