//! An update feed's address, into the client's reader.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| update::fuzz::feed_address(data));
