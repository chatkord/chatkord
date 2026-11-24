//! The sound server's list names each device by what it is opened by, and says which of each is the default.

use testserver::sound::Lab;
use tier::skipped;

mod lab {
    use super::*;

    #[test]
    fn a_device_is_listed_by_its_node_name_and_the_defaults_are_among_those_listed() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(sound) = lab.make("listed") else { return skipped("no sound server to make devices on") };
        let listing = match pw::devices() {
            Ok(listing) => listing,
            Err(why) => return skipped(why.to_string()),
        };
        let named = |listed: &[pw::Listed], name: &str| listed.iter().any(|device| device.name == name);
        assert!(named(&listing.speakers, &sound.sink), "{listing:?}");
        assert!(named(&listing.microphones, &sound.source), "{listing:?}");
        let default = listing.default_speaker.as_deref().expect("a server with a speaker says its default");
        assert!(named(&listing.speakers, default), "the default speaker is one listed: {default}");
        if let Some(default) = listing.default_microphone.as_deref() {
            println!("default microphone {default}");
        }
    }
}
