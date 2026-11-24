//! An invite link a person pastes, into the client's reader.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| terms::fuzz::invite_link(data));
