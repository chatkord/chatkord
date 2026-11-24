//! An encoded unit in FFmpeg's reference-counted packet.

use super::error::{AvError, check};
use ffmpeg_sys_next as ff;
use std::ffi::c_int;
use std::ptr;

/// An FFmpeg packet, freed when dropped.
pub struct Packet(pub(super) *mut ff::AVPacket);

impl Packet {
    /// An empty packet.
    pub fn new() -> Option<Self> {
        // SAFETY: allocation has no preconditions; null is handled.
        let raw = unsafe { ff::av_packet_alloc() };
        (!raw.is_null()).then_some(Self(raw))
    }

    /// A packet holding a copy of `bytes`; refused as invalid when it is empty or more than a packet holds.
    pub fn copied(bytes: &[u8]) -> Result<Self, AvError> {
        let size = c_int::try_from(bytes.len()).ok().filter(|size| *size > 0).ok_or(AvError::Invalid)?;
        let packet = Self::new().ok_or(AvError::NoMemory)?;
        // SAFETY: the packet is owned and empty.
        check(unsafe { ff::av_new_packet(packet.0, size) })?;
        // SAFETY: the packet is owned and was just given its buffer.
        let data = unsafe { (*packet.0).data };
        // SAFETY: `av_new_packet` gave `data` `size` writable bytes (plus padding), a new buffer apart from `bytes`.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len()) };
        Ok(packet)
    }

    /// The bytes it holds, none when it holds nothing.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the packet is live, and its `data` holds `size` bytes while it holds a buffer.
        let packet = unsafe { &*self.0 };
        let size = usize::try_from(packet.size).unwrap_or(0);
        if packet.data.is_null() || size == 0 {
            return &[];
        }
        // SAFETY: as above.
        unsafe { std::slice::from_raw_parts(packet.data, size) }
    }

    /// The timestamp it carries.
    pub fn pts(&self) -> i64 {
        // SAFETY: the packet is live; a plain field is read.
        unsafe { (*self.0).pts }
    }

    /// Whether a receiver can start decoding at it.
    pub fn is_key(&self) -> bool {
        // SAFETY: the packet is live; a plain field is read.
        unsafe { (*self.0).flags & ff::AV_PKT_FLAG_KEY as c_int != 0 }
    }

    /// What this packet holds moved to a new packet of its own, this one left empty, and the bytes made the new one's
    /// own: an encoder's packets are reference-counted already, and then this copies nothing.
    pub fn take(&mut self) -> Result<Self, AvError> {
        let into = Self::new().ok_or(AvError::NoMemory)?;
        // SAFETY: both packets are owned and live; the move leaves this one blank.
        unsafe { ff::av_packet_move_ref(into.0, self.0) };
        // SAFETY: `into` is owned and holds what was moved into it.
        check(unsafe { ff::av_packet_make_refcounted(into.0) })?;
        Ok(into)
    }

    /// A new packet referring to the same bytes, stamped `pts`: no byte is copied.
    pub fn referenced(&self, pts: i64) -> Result<Self, AvError> {
        let packet = Self::new().ok_or(AvError::NoMemory)?;
        // SAFETY: `packet` is owned and blank; the source is a live, reference-counted packet, so this takes a
        // reference to its buffer and copies no bytes.
        check(unsafe { ff::av_packet_ref(packet.0, self.0) })?;
        // SAFETY: the packet is owned, and nothing else reaches its fields.
        unsafe { (*packet.0).pts = pts };
        Ok(packet)
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `av_packet_alloc` and is freed once.
        unsafe { ff::av_packet_free(&raw mut self.0) };
    }
}
