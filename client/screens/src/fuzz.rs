//! The fuzz target's entry for what the client reads of the events a server sends.

use crate::PlaceMessage;
use crate::harness::{self, Clock};
use proto::chatkord_v1::wire_message::Body;
use proto::envelope::{Frame, read_frame};
use proto::rules::Grants;
use std::time::Instant;

/// A server's frame as the client's edge reads it: an event it can make a message of is handed to a signed-in
/// place, and one it cannot (an id it cannot parse, a shape out of the protocol) leaves the place as it was.
pub fn event_into_place(data: &[u8]) {
    let Frame::Op(op) = read_frame(data) else { return };
    let Some(Body::Event(event)) = op.body else { return };
    let clock = Clock(Instant::now());
    let (mut window, id) = harness::holding(&clock, Grants::default());
    let shown = |window: &crate::Window| {
        let place = &window.places()[0];
        (place.panel(), place.rail(), place.grants())
    };
    let before = shown(&window);
    match PlaceMessage::heard(&event) {
        Some(message) => {
            harness::tell(&mut window, &clock, id, message);
        }
        None => assert_eq!(shown(&window), before, "a place that changed for an event nothing was made of"),
    }
}

#[cfg(test)]
mod tests {
    use super::event_into_place;
    use std::path::{Path, PathBuf};

    fn seeds(target: &str) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
        proto::fuzz::seeds(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds"), target)
    }

    #[test]
    fn every_kept_event_is_read_into_a_place_or_passed_over() {
        for (_, input) in seeds("event_into_place").expect("the seeds") {
            event_into_place(&input);
        }
    }
}
