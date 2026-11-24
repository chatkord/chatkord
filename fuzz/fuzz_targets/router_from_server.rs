//! The router capabilities a server tells a client, into the client's reader.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rtc::fuzz::router(data));
