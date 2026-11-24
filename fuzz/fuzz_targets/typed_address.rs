//! A server's address as a person types it, into the client's reader.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| terms::fuzz::typed_address(data));
