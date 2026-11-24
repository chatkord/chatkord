//! What the call decides about the voice: the person's commands and wants, the place's state, the microphone and the
//! devices.

use super::own::{OwnAct, own_started};
use super::{Act, Mic, State};
use crate::failure;
use crate::room::place::Hold;
use crate::room::{Command, Muting, Over, Report, Source, Wants, heard_on, heed};
use crate::send::own::Asked;
use crate::send::sender;
use crate::sound::{Chosen, Looking, Plan, Told};
use crate::watch::viewing::Viewed;
use screens::{RoomEvent, Subject, Track};
use terms::{LinkError, MemberId};
use tokio::time::Instant;
use webrtc::{Sounding, VoiceRefused};

/// What the voice hears: the person's asks, the place's answers, the microphone and the devices.
pub(in crate::room) enum VoiceInput {
    /// The person's acts, in the order asked.
    Commands(Vec<Command>),
    /// What the person wants changed from `was` to `is`.
    Wants { was: Wants, is: Wants },
    /// The server answered the place's state.
    Asked(Result<(), LinkError>),
    /// The microphone opened, and its track is the driver's now, or would not.
    MicOpened(Result<(), VoiceRefused>),
    /// The microphone was set to the place's state.
    MicSynced(Synced),
    /// Look at the devices.
    Settle(Looking),
    /// The look at the devices answered.
    Settled(Option<(Plan, Chosen)>),
    /// The devices cannot be looked at.
    SettleFailed,
    /// The chain told what became of the devices.
    Sounded(Sounded),
    /// The loudness tick.
    Levels(Levels),
    /// The call is ending: nothing more is sent or played.
    Hush,
}

/// What the driver carries out for the voice.
pub(in crate::room) enum VoiceAct {
    /// Play the call to the speaker, or hold it back.
    Deafen(bool),
    /// Play this sound at this gain.
    Volume { user_id: MemberId, kind: proto::chatkord_v1::TrackKind, gain: f64 },
    /// Process the microphone so.
    Process(choices::VoiceProcessing),
    /// Mute the microphone track.
    MuteMic,
    /// Set the microphone track to the place's state, then say how it went with [`VoiceInput::MicSynced`].
    SyncMic { silenced: bool },
    /// Open the microphone, or a tone.
    OpenMic(Source),
    /// Let go of the microphone being opened.
    DropOpening,
    /// Look at the devices for `sounds`.
    Settle { looking: Looking, sounds: crate::sound::Sounds },
    /// Let go of the look at the devices.
    DropGlance,
    /// Ask the server for this state of the place.
    AskState(choices::State),
    /// Tick for the loudness of who is heard while the window shows (true), not at all while it does not.
    Levels(bool),
}

impl From<VoiceAct> for Act {
    fn from(act: VoiceAct) -> Self {
        Self::Voice(act)
    }
}

/// What `input` changes in `state` at `now`, and the acts that follow.
pub(super) fn decide(state: &mut State, input: VoiceInput, now: Instant, acts: &mut Vec<Act>) {
    match input {
        VoiceInput::Commands(commands) => state.commands(commands, now, acts),
        VoiceInput::Wants { was, is } => state.want(&was, &is, now, acts),
        VoiceInput::Asked(told) => state.answered(told, now, acts),
        VoiceInput::MicOpened(opened) => state.opened(opened, acts),
        VoiceInput::MicSynced(synced) => state.synced(synced, acts),
        VoiceInput::Settle(looking) => state.settle(looking, now, acts),
        VoiceInput::Settled(looked) => state.settled(looked, now, acts),
        VoiceInput::SettleFailed => state.settle_failed(acts),
        VoiceInput::Sounded(sounded) => state.sounded(&sounded, acts),
        VoiceInput::Levels(levels) => state.levelled(levels, acts),
        VoiceInput::Hush => state.hush(acts),
    }
}

/// Whether the microphone took the state the place is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::room) enum Synced {
    Set,
    /// It would not open again.
    Refused,
}

/// What the chain told of the devices, and what the driver reads of them.
pub(in crate::room) struct Sounded {
    pub(in crate::room) now: Sounding,
    /// Whether the chain's thread runs.
    pub(in crate::room) alive: bool,
    /// Whether the microphone is held and open.
    pub(in crate::room) capturing: bool,
}

/// How loud each person heard is, and this account's own microphone.
pub(in crate::room) struct Levels {
    pub(in crate::room) heard: Vec<(MemberId, u8)>,
    /// The microphone's loudness from the chain's own samples, where one is open.
    pub(in crate::room) microphone: Option<u8>,
}

impl State {
    /// Carries out the person's acts, in order, and then what they want.
    fn commands(&mut self, commands: Vec<Command>, now: Instant, acts: &mut Vec<Act>) {
        let mut devices = false;
        for command in commands {
            match command {
                Command::Microphone(asked) => {
                    self.sounds.ask_microphone(asked);
                    devices = true;
                }
                Command::Speaker(asked) => {
                    self.sounds.ask_speaker(asked);
                    devices = true;
                }
                Command::DevicesChanged => devices = true,
                Command::Processing(asked) => acts.push(VoiceAct::Process(asked).into()),
                Command::Share(pick, quality, sound) => {
                    acts.push(OwnAct::ShareAsk(Asked { pick, quality, sound }).into())
                }
                Command::StopShare => acts.push(OwnAct::ShareStop.into()),
                Command::Camera(Some(ask)) => acts.push(OwnAct::CameraAsk(ask).into()),
                Command::Camera(None) => acts.push(OwnAct::CameraStop.into()),
                Command::ShareSoundOf(app) => acts.push(OwnAct::ShareSoundOf(app).into()),
                Command::Levels(shown) => acts.push(VoiceAct::Levels(shown).into()),
            }
        }
        if devices {
            self.settle(Looking::Moved, now, acts);
        }
        self.apply(acts);
    }

    /// What the person wants `is` that differs from what was last read (`was`).
    fn want(&mut self, was: &Wants, is: &Wants, now: Instant, acts: &mut Vec<Act>) {
        if is.muted().times != was.muted().times {
            self.place.mute(is.muted().what);
        }
        if is.deafened().times != was.deafened().times {
            self.place.deafen(is.deafened().what);
        }
        for (user_id, audio, volume) in is.played_otherwise_than(was) {
            acts.push(VoiceAct::Volume { user_id, kind: heard_on(audio), gain: volume.fraction() }.into());
        }
        if is.unseen() != was.unseen() {
            self.unseen = is.unseen().to_vec();
            let viewed: Vec<Viewed> = self.watching.keys().cloned().collect();
            for each in &viewed {
                self.see(each, now, acts);
            }
        }
        if is.paused() != was.paused() || is.resumed() != was.resumed() {
            self.paused = is.paused();
            for each in self.watching.keys().cloned().collect::<Vec<Viewed>>() {
                let (paused, had) = (is.paused().contains(&each), was.paused().contains(&each));
                let started_again = is.resumed().contains(&each) && !was.resumed().contains(&each);
                if paused && !had {
                    self.pause(&each, acts);
                } else if started_again {
                    self.resume(&each, acts);
                }
            }
        }
        self.apply(acts);
    }

    /// Carries out on this device what the person wants (nothing is sent on a deafened or muted device, whichever was
    /// asked for) and, once the microphone is set, shows it as theirs and asks the server for it.
    fn apply(&mut self, acts: &mut Vec<Act>) {
        acts.push(VoiceAct::Deafen(self.place.want().deafened).into());
        if self.mic == Mic::Open {
            acts.push(VoiceAct::SyncMic { silenced: self.place.silenced() }.into());
            return;
        }
        self.applied(acts);
    }

    /// The microphone is set: the person's state is shown as theirs, and asked of the server unless an ask is in
    /// flight; its answer asks again what changed meanwhile.
    fn applied(&mut self, acts: &mut Vec<Act>) {
        if let Some(user_id) = &self.me {
            acts.push(Act::Report(Report::Room(RoomEvent::State {
                user_id: user_id.clone(),
                state: self.place.want(),
            })));
        }
        if let Some(state) = self.place.ask() {
            acts.push(VoiceAct::AskState(state).into());
        }
    }

    fn synced(&mut self, synced: Synced, acts: &mut Vec<Act>) {
        if synced == Synced::Refused {
            // Refused here only with the chain's thread gone, which was said when it went.
            self.place.mic_refused();
        }
        self.applied(acts);
    }

    /// The server answered the place's state.
    fn answered(&mut self, told: Result<(), LinkError>, now: Instant, acts: &mut Vec<Act>) {
        match told {
            Ok(()) => {
                self.place.agreed();
                // The place is told first, so a stream started now is born unpaused.
                self.open_mic(now, acts);
                self.apply(acts);
            }
            Err(LinkError::Closed(ended)) => acts.push(Act::End(Over::Link(ended))),
            Err(other) => {
                self.place.refused();
                acts.push(Act::Report(Report::Notice(Subject::VoiceState, words::failure::said(&other))));
                self.apply(acts);
            }
        }
    }

    /// Opens the source once the server has the place heard. A place that joined muted opens no device until it is
    /// first unmuted.
    pub(super) fn open_mic(&mut self, now: Instant, acts: &mut Vec<Act>) {
        if self.mic != Mic::Closed || !self.place.may_speak() || self.source == Source::Nothing {
            return;
        }
        let within = sender::start_within();
        self.mic = Mic::Opening { due: now + within, within };
        acts.push(VoiceAct::OpenMic(self.source).into());
    }

    /// The microphone opened, or would not.
    fn opened(&mut self, opened: Result<(), VoiceRefused>, acts: &mut Vec<Act>) {
        match opened {
            Ok(()) => {
                // Silenced while it opened: it is sent paused, as the server is told.
                if self.place.silenced() {
                    acts.push(VoiceAct::MuteMic.into());
                }
                self.mic = Mic::Open;
                own_started(self.me.as_ref(), Track::Microphone, acts);
            }
            Err(refused) => {
                self.mic = Mic::Closed;
                let said = words::failure::of(&failure::voice(refused));
                acts.push(Act::Report(Report::Notice(Subject::Microphone, said)));
            }
        }
    }

    /// This account's mute for everyone began or ended: while it holds the microphone is off, as the server has it;
    /// lifted, it sends again unless the person's own mute or deafen holds it.
    pub(super) fn hold(&mut self, hold: Hold, now: Instant, acts: &mut Vec<Act>) {
        self.place.hold(hold);
        match (hold, &self.mic) {
            (Hold::Held, Mic::Open) => acts.push(VoiceAct::MuteMic.into()),
            (Hold::Free, Mic::Open) => acts.push(VoiceAct::SyncMic { silenced: self.place.silenced() }.into()),
            (Hold::Free, Mic::Closed) => self.open_mic(now, acts),
            (Hold::Held, Mic::Closed | Mic::Opening { .. }) | (Hold::Free, Mic::Opening { .. }) => {}
        }
    }

    /// Looks at the devices again to move each direction to the one it should use. [`Looking::Forced`] sets both
    /// whatever they were: the engine's chain remembers a device chosen in an earlier call.
    fn settle(&mut self, looking: Looking, now: Instant, acts: &mut Vec<Act>) {
        if !self.has_devices {
            self.sounds.unavailable(&mut |subject, said| acts.push(Act::Report(Report::Notice(subject, said))));
            return;
        }
        if self.settling.ask(looking, now) {
            acts.push(VoiceAct::Settle { looking, sounds: self.sounds.clone() }.into());
        }
    }

    /// A look at the devices could not be made.
    fn settle_failed(&mut self, acts: &mut Vec<Act>) {
        self.settling.answered();
        self.sounds.unavailable(&mut |subject, said| acts.push(Act::Report(Report::Notice(subject, said))));
    }

    /// A look at the devices answered: each direction is where it should be, said when it had to move, and the look
    /// asked for meanwhile is made.
    fn settled(&mut self, looked: Option<(Plan, Chosen)>, now: Instant, acts: &mut Vec<Act>) {
        self.settling.answered();
        if let Some((plan, chosen)) = looked {
            let sound = self.sounds.settled(plan, chosen, &mut |subject, said| {
                acts.push(Act::Report(Report::Notice(subject, said)));
            });
            acts.push(Act::Report(Report::Sound(sound)));
        }
        if let Some(looking) = self.settling.again() {
            self.settle(looking, now, acts);
        }
    }

    /// The chain told what became of the devices asked for, each heeded as [`heed`] says.
    fn sounded(&mut self, sounded: &Sounded, acts: &mut Vec<Act>) {
        for told in self.standings.hear(sounded.now, sounded.alive) {
            self.told(told, sounded.capturing, acts);
        }
    }

    /// Says what became of a device, or that the chain stopped, and mutes the place where [`heed`] says.
    pub(super) fn told(&mut self, told: Told, capturing: bool, acts: &mut Vec<Act>) {
        let sending = self.mic == Mic::Open && self.source == Source::Microphone && !self.place.silenced();
        let (said, muting) = heed(told, sending, &self.sounds);
        if let Some(said) = said {
            acts.push(Act::Report(said));
        }
        if muting == Muting::Silence {
            self.silence_unless_capturing(capturing, acts);
        }
    }

    /// The microphone would not open: the place is muted, as the server is told, unless the one before still runs.
    fn silence_unless_capturing(&mut self, capturing: bool, acts: &mut Vec<Act>) {
        if self.mic != Mic::Open || capturing {
            return;
        }
        acts.push(VoiceAct::MuteMic.into());
        self.place.mic_refused();
        self.apply(acts);
    }

    /// How loud everyone heard and this account's own microphone are, so their tiles ring when they speak: a
    /// microphone's from the chain's own samples, a tone's as the engine last reported it.
    fn levelled(&mut self, levels: Levels, acts: &mut Vec<Act>) {
        let mut heard = levels.heard;
        let own = match (self.mic, self.source) {
            (Mic::Open, Source::Microphone) => levels.microphone,
            (Mic::Open, Source::Tone) => self.tone,
            (Mic::Open, Source::Nothing) | (Mic::Closed | Mic::Opening { .. }, _) => None,
        };
        if let (Some(user_id), Some(level)) = (&self.me, own) {
            heard.push((user_id.clone(), level));
        }
        acts.push(Act::Report(Report::Room(RoomEvent::Levels(heard))));
    }

    /// The call is ending: nothing more is sent from the microphone or played, at once, whatever still has to end.
    fn hush(&mut self, acts: &mut Vec<Act>) {
        acts.push(VoiceAct::Deafen(true).into());
        match self.mic {
            Mic::Open => acts.push(VoiceAct::MuteMic.into()),
            Mic::Opening { .. } => {
                self.mic = Mic::Closed;
                acts.push(VoiceAct::DropOpening.into());
            }
            Mic::Closed => {}
        }
    }

    /// When the look at the devices is late, if one is out.
    pub(in crate::room) fn settling_due(&self) -> Option<Instant> {
        self.settling.due()
    }
}
