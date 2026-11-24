//! A bitstream filter that rewrites an encoder's units.

use super::codec::CodecContext;
use super::error::{AvError, check};
use super::packet::Packet;
use ffmpeg_sys_next as ff;
use std::ffi::CStr;
use std::ptr;

/// FFmpeg's `filter_units`, which drops the units of the types it is told, freed when dropped.
pub struct UnitFilter(*mut ff::AVBSFContext);

impl UnitFilter {
    /// The filter for the stream that the open encoder `context` writes, dropping the unit type `remove`.
    pub fn new(context: &CodecContext, remove: &CStr) -> Result<Self, AvError> {
        // SAFETY: the name is NUL terminated; a null result is handled.
        let kind = unsafe { ff::av_bsf_get_by_name(c"filter_units".as_ptr()) };
        if kind.is_null() {
            return Err(AvError::Unsupported);
        }
        let mut raw = ptr::null_mut();
        // SAFETY: `kind` is a filter FFmpeg found; `raw` receives a context the value made below frees.
        let status = unsafe { ff::av_bsf_alloc(kind, &raw mut raw) };
        let filter = Self(raw);
        check(status)?;
        if raw.is_null() {
            return Err(AvError::NoMemory);
        }
        // SAFETY: `raw` is an allocated, not yet initialised filter context, which nothing else reaches yet.
        let filtering = unsafe { &mut *raw };
        // SAFETY: `par_in` is the filter's own parameters, allocated with it; the context is a live encoder.
        check(unsafe { ff::avcodec_parameters_from_context(filtering.par_in, context.raw()) })?;
        filtering.time_base_in = context.time_base();
        // SAFETY: `remove_types` is the filter's own option, set on its private data.
        check(unsafe { ff::av_opt_set(filtering.priv_data, c"remove_types".as_ptr(), remove.as_ptr(), 0) })?;
        // SAFETY: the filter is allocated and told its input, and not yet initialised.
        check(unsafe { ff::av_bsf_init(raw) })?;
        Ok(filter)
    }

    /// Sends `packet`, whose reference the filter takes, leaving it empty.
    pub fn send(&mut self, packet: &mut Packet) -> Result<(), AvError> {
        // SAFETY: the filter is initialised and `packet` holds an encoded unit.
        check(unsafe { ff::av_bsf_send_packet(self.0, packet.0) }).map(drop)
    }

    /// Receives what is left of a sent packet into `packet`, which was emptied by the send.
    pub fn receive(&mut self, packet: &mut Packet) -> Result<(), AvError> {
        // SAFETY: the filter is initialised and `packet` is owned and empty.
        check(unsafe { ff::av_bsf_receive_packet(self.0, packet.0) }).map(drop)
    }
}

impl Drop for UnitFilter {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `av_bsf_alloc` (or is null, which `av_bsf_free` takes) and is freed once.
        unsafe { ff::av_bsf_free(&raw mut self.0) };
    }
}
