//! A server's mediasoup parameters into the client's offer, the SDP its engine reads.
#![no_main]
#![forbid(unsafe_code)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rtc::fuzz::sdp_from_server(data));
