//! A password, an invite or a token held by the client: zeroed when dropped, never printed.

use std::fmt;
use zeroize::Zeroizing;

/// A secret's text, which `Debug` prints as `[redacted]`.
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Takes `value` over; its memory is zeroed when the secret is dropped.
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// The text, for the one place that sends or stores it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether nothing was typed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A copy is a secret of its own: it zeroes itself when dropped, and prints `[redacted]`. A message
/// that carries a secret through a toolkit that clones its messages needs this.
impl Clone for Secret {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn a_copy_is_equal_independent_and_still_redacted() {
        let secret = Secret::new("hunter2".to_string());
        let copy = secret.clone();
        assert_eq!(secret, copy);
        drop(secret);
        assert_eq!(copy.expose(), "hunter2", "the copy owns its text");
        assert_eq!(format!("{copy:?}"), "[redacted]");
    }
}
