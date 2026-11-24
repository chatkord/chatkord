//! An event a server tells, into the client's screens.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| screens::fuzz::event_into_place(data));
