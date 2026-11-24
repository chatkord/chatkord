//! An encoded unit's bytes in FFmpeg's reference-counted packet: made once, from the encoder's own buffer or one copy
//! of a received frame, then shared by reference with the decoder and a replay, never copied again.

use crate::av::{AvError, Packet};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// An encoded unit's bytes; cloning shares them.
#[derive(Clone)]
pub struct Coded(Arc<Held>);

/// The packet a [`Coded`] holds, read and referenced from any thread, never written.
struct Held(Packet);

// SAFETY: the packet's buffer is reference-counted with atomic counts and is never written once held: every use reads
// it or takes another reference to it.
unsafe impl Send for Held {}
// SAFETY: as above; `&Held` only reads.
unsafe impl Sync for Held {}

/// Why a unit's bytes could not be held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodedError {
    /// No bytes, or more than a packet holds.
    Size,
    /// FFmpeg could not allocate the packet.
    Memory,
}

impl fmt::Display for CodedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Size => "an encoded unit is empty or larger than a packet holds",
            Self::Memory => "no memory for an encoded unit",
        })
    }
}

impl std::error::Error for CodedError {}

impl Coded {
    /// The bytes of `unit`, copied once into a packet of their own.
    pub fn copied(unit: &[u8]) -> Result<Self, CodedError> {
        match Packet::copied(unit) {
            Ok(packet) => Ok(Self(Arc::new(Held(packet)))),
            Err(AvError::NoMemory) => Err(CodedError::Memory),
            Err(_) => Err(CodedError::Size),
        }
    }

    /// What `packet` holds, its buffer moved here and `packet` left empty; none when it holds nothing.
    pub(crate) fn taken(packet: &mut Packet) -> Result<Self, CodedError> {
        let coded = Self(Arc::new(Held(packet.take().map_err(|_| CodedError::Memory)?)));
        if coded.is_empty() { Err(CodedError::Size) } else { Ok(coded) }
    }

    /// A new packet referring to these bytes, stamped `pts`, for a decoder to be sent.
    pub(crate) fn stamped(&self, pts: i64) -> Result<Packet, CodedError> {
        self.0.0.referenced(pts).map_err(|_| CodedError::Memory)
    }
}

impl Deref for Coded {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.0.0.bytes()
    }
}

impl AsRef<[u8]> for Coded {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl fmt::Debug for Coded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Coded({} bytes)", self.len())
    }
}

impl PartialEq for Coded {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Coded {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_holds_the_bytes_and_a_clone_or_a_stamped_packet_shares_them() {
        let unit = Coded::copied(&[0, 0, 1, 0x65, 7]).unwrap();
        assert_eq!(&*unit, &[0, 0, 1, 0x65, 7]);
        let again = unit.clone();
        assert_eq!(again.as_ptr(), unit.as_ptr(), "a clone copies nothing");
        let stamped = unit.stamped(42).unwrap();
        assert_eq!(stamped.bytes().as_ptr(), unit.as_ptr(), "the decoder is sent the same bytes");
        assert_eq!(stamped.pts(), 42);
    }

    #[test]
    fn nothing_is_not_a_unit() {
        assert_eq!(Coded::copied(&[]).err(), Some(CodedError::Size));
    }

    #[test]
    fn a_taken_packet_keeps_the_buffer_it_had_and_leaves_the_source_empty() {
        let mut source = Coded::copied(&[1, 2, 3]).unwrap().stamped(0).unwrap();
        let before = source.bytes().as_ptr();
        let taken = Coded::taken(&mut source).unwrap();
        assert_eq!(taken.as_ptr(), before);
        assert!(source.bytes().is_empty());
    }
}
