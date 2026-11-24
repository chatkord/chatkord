//! What a server says of itself: its name, its picture and that picture's colour, read where they arrive (the
//! greeting, a `ServerChanged`) and shaped once, as `proto::rules` says a name others read, a colour and a file are.

use proto::files::FileId;
use proto::rules::{Colour, MAX_NAME_CHARS, TextRefused, text_shape};

/// A server's name, colour and picture, shaped; none of each is "its address", "Chatkord's colour" and "its initial".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerIdentity {
    /// Its name, shaped as a name others read.
    pub name: Option<String>,
    /// The colour it is shown in.
    pub colour: Option<Colour>,
    /// Its picture: a file fetched over HTTP once signed in, kept by its id.
    pub picture: Option<FileId>,
}

impl ServerIdentity {
    /// The name, colour and picture as a server wrote them. A blank name is none; any of them out of shape is a server
    /// not keeping the protocol, so none of it is taken.
    pub fn read(name: Option<&str>, colour: Option<&str>, picture: Option<&str>) -> Option<ServerIdentity> {
        let name = match name.map(|name| text_shape(name, MAX_NAME_CHARS)) {
            None | Some(Err(TextRefused::Empty)) => None,
            Some(Ok(name)) => Some(name.to_string()),
            Some(Err(TextRefused::TooLong | TextRefused::ControlCharacter)) => return None,
        };
        let colour = colour.map(Colour::parse).transpose().ok()?;
        let picture = picture.map(FileId::parse).transpose().ok()?;
        Some(ServerIdentity { name, colour, picture })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_a_colour_and_a_picture_are_read_in_shape_or_not_at_all() {
        let picture = "0123456789abcdef0123456789abcdef";
        let read = ServerIdentity::read(Some(" Server S "), Some("#3d7fd6"), Some(picture));
        let expected = ServerIdentity {
            name: Some("Server S".into()),
            colour: Colour::parse("#3D7FD6").ok(),
            picture: FileId::parse(picture).ok(),
        };
        assert_eq!(read, Some(expected));
        assert_eq!(ServerIdentity::read(Some("  "), None, None), Some(ServerIdentity::default()), "blank is none");
        assert_eq!(ServerIdentity::read(None, None, None), Some(ServerIdentity::default()));
        for bad in [&"n".repeat(MAX_NAME_CHARS + 1), "a\u{202E}b", "a\u{0007}b"] {
            assert_eq!(ServerIdentity::read(Some(bad), None, None), None, "{bad:?}");
        }
        assert_eq!(ServerIdentity::read(Some("Server S"), Some("plaid"), None), None);
        for bad in ["../../etc/passwd", "0123456789ABCDEF0123456789ABCDEF", "abc"] {
            assert_eq!(ServerIdentity::read(None, None, Some(bad)), None, "{bad:?}");
        }
    }
}
