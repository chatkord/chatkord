//! FFmpeg's key-value dictionary, for the options a device is made with.

use super::error::{AvError, check};
use ffmpeg_sys_next as ff;
use std::ffi::CStr;
use std::ptr;

/// A dictionary of options, freed when dropped.
pub(super) struct Dict(*mut ff::AVDictionary);

impl Dict {
    /// An empty dictionary.
    pub(super) fn new() -> Self {
        Self(ptr::null_mut())
    }

    /// Sets `key` to `value`.
    pub(super) fn set(&mut self, key: &CStr, value: &CStr) -> Result<(), AvError> {
        // SAFETY: `self.0` is null or a dictionary this value owns, and both strings are NUL-terminated; FFmpeg copies
        // them.
        check(unsafe { ff::av_dict_set(&raw mut self.0, key.as_ptr(), value.as_ptr(), 0) }).map(drop)
    }

    /// The dictionary, or null when empty, for a call that only reads it.
    pub(super) fn as_ptr(&self) -> *mut ff::AVDictionary {
        self.0
    }
}

impl Drop for Dict {
    fn drop(&mut self) {
        // SAFETY: null or the dictionary this value owns, freed once; `av_dict_free` nulls it.
        unsafe { ff::av_dict_free(&raw mut self.0) };
    }
}
