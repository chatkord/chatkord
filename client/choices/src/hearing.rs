//! What a person has done about hearing and being heard: in a call, as the server keeps it, and for the next call,
//! as this device keeps it.

/// What a person has done about hearing and being heard, as the server keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// They muted their microphone.
    pub mic_muted: bool,
    /// They hear nothing; their microphone is off with it.
    pub deafened: bool,
}

impl State {
    /// After a press on mute. Deafened, it undoes both: a microphone is never on while nothing is heard.
    pub fn mute_pressed(self) -> State {
        if self.deafened { State::default() } else { State { mic_muted: !self.mic_muted, deafened: false } }
    }
}
