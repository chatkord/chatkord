//! The release a server runs, as its greeting names it, read where it arrives and bounded once.

use proto::envelope::MAX_HANDSHAKE_TEXT;
use proto::rules::DIRECTION_CONTROLS;
use std::fmt;

/// A server's release (`ServerHello.server_build`): trimmed, never empty, at most [`MAX_HANDSHAKE_TEXT`] bytes, with
/// no control or text-direction character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerBuild(String);

/// A build text a server wrote out of shape: a server not keeping the protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildRefused;

impl ServerBuild {
    /// The build as a server wrote it. Blank is none; over the bound or holding a control character is refused,
    /// never cut.
    pub fn read(text: &str) -> Result<Option<ServerBuild>, BuildRefused> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        if text.len() > MAX_HANDSHAKE_TEXT
            || text.chars().any(|character| character.is_control() || DIRECTION_CONTROLS.contains(&character))
        {
            return Err(BuildRefused);
        }
        Ok(Some(ServerBuild(text.to_string())))
    }

    /// The build as read.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerBuild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_is_trimmed_blank_is_none_and_out_of_shape_is_refused() {
        assert_eq!(ServerBuild::read(" 0.9.0 ").unwrap().unwrap().as_str(), "0.9.0");
        assert_eq!(ServerBuild::read("  "), Ok(None));
        assert!(ServerBuild::read(&"1".repeat(MAX_HANDSHAKE_TEXT)).unwrap().is_some());
        for bad in ["1".repeat(MAX_HANDSHAKE_TEXT + 1), "0.9\u{0007}".into(), "0.9\u{202E}".into()] {
            assert_eq!(ServerBuild::read(&bad), Err(BuildRefused), "{bad:?}");
        }
    }
}
