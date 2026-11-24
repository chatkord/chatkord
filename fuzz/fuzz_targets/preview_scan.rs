//! A web page as a link's host sends it, which the sender's client scans for a preview.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| fetch::fuzz::preview_scan(data));
