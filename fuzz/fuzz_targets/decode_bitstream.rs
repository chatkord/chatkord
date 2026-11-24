//! A coded video a server relays, into the client's software decoder.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| media::fuzz::decode_bitstream(data));
