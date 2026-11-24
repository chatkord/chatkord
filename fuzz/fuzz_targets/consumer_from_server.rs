//! The consumer a server tells a client to build, into the client's reader.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rtc::fuzz::consumer(data));
