//! In a binary that links libwebrtc, the linked `pw_init` is its stub, a jump through a null pointer: a session opened
//! through the loaded library must not crash there.

use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

use webrtc as _;

use libspa_sys::{
    SPA_FORMAT_mediaSubtype, SPA_FORMAT_mediaType, SPA_MEDIA_SUBTYPE_raw, SPA_MEDIA_TYPE_video, SPA_PARAM_EnumFormat,
    SPA_TYPE_Id, SPA_TYPE_OBJECT_Format, SPA_TYPE_Object,
};
use tier::skipped;

struct Quiet;

impl pw::Handler for Quiet {
    fn state_changed(&mut self, _: pw::State) {}
    fn param_changed(&mut self, _: &pw::StreamRef<'_>, _: u32, _: Option<&[u8]>) {}
    fn process(&mut self, _: &pw::StreamRef<'_>) {}
}

/// A format pod naming video and raw and nothing more: what PipeWire 1.0 needs before it takes a stream (it refuses
/// a format with no media type, `ESRCH`, where later releases let it pass), and offers no size or rate.
fn offer() -> Vec<u8> {
    // An object of type Format for EnumFormat, then two properties: each a key, its flags and an Id pod.
    let mut pod = Vec::new();
    let mut word = |value: u32| pod.extend_from_slice(&value.to_ne_bytes());
    word(8 + 2 * 24); // body size: the object's type and id, then two properties of 24 bytes
    word(SPA_TYPE_Object);
    word(SPA_TYPE_OBJECT_Format);
    word(SPA_PARAM_EnumFormat);
    for (key, id) in [(SPA_FORMAT_mediaType, SPA_MEDIA_TYPE_video), (SPA_FORMAT_mediaSubtype, SPA_MEDIA_SUBTYPE_raw)] {
        word(key);
        word(0); // no flags
        word(4); // the value's size
        word(SPA_TYPE_Id);
        word(id);
        word(0); // padding to 8 bytes
    }
    pod
}

#[test]
fn a_session_opens_and_closes_in_a_binary_that_links_libwebrtc() {
    // Keeps the stub linked: its address is taken, never called.
    let stub = std::hint::black_box(pipewire_sys::pw_init as *const () as usize);
    assert_ne!(stub, 0);

    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        // A connection that never speaks: PipeWire takes it, and the stream stays connecting.
        let (ours, _theirs) = UnixStream::pair().expect("a socket pair");
        let outcome = pw::Session::open(ours.into(), 0, pw::Role::Screen, &[&offer()], Quiet);
        let _ = done.send(outcome.map(drop));
    });
    match finished.recv_timeout(Duration::from_secs(20)) {
        Ok(Err(pw::Error::Unavailable(_))) => skipped("libpipewire is not installed here"),
        // A refused context skips: the fault guarded is a null jump, a crash and never a refusal. A later refusal is
        // ours, since from "connect to the screen" on it would be this socket's, which never refuses.
        Ok(Err(pw::Error::Refused("make a context" | "make the context properties"))) => {
            skipped("libpipewire will not make a context here")
        }
        Ok(opened) => assert_eq!(opened, Ok(())),
        Err(_) => panic!("opening and closing a session did not finish"),
    }
}
