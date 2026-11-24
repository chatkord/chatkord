//! Which microphone and speaker a call or a probe uses: what a person asked for, what the devices were set to,
//! and what is said when a device goes or comes back. The devices are listed by the process's sound, which answers from
//! a thread of its own (the sound server may not answer), and what is decided from a listing is decided here, from
//! values alone.

use crate::bounded::{Panicked, Task};
use crate::failure;
use choices::Picked;
use screens::{Listed, Sound, Subject};
use std::time::Duration;
use tokio::time::Instant;
use webrtc::{Device, Devices, DevicesRefused, Sounding, Standing};
use words::{Failure, Sentence};

/// How long the sound server may take to list its devices and take a choice: some hundred times what a listing
/// takes on a running PipeWire (tens of milliseconds). A bound, not a measurement.
pub const LISTING_WITHIN: Duration = Duration::from_secs(5);

/// What one direction, the microphone or the speaker, is asked to use and what it uses.
#[derive(Clone, Debug, Default)]
struct Pick {
    /// What the person wants, or none for the system's default. Never changed by a device going.
    asked: Option<Picked>,
    /// What the devices were last set to, by id, none for the default.
    using: Option<String>,
    /// The device asked for is not here, and the default stands in for it.
    away: bool,
    /// What the sound server named the device in use when it last listed it; none before a look, or nameless.
    named: Option<String>,
}

/// What a direction does about the devices listed now, and what a person is told of it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Settled {
    /// The device to use: the one asked for if it is here, the default if not.
    target: Option<String>,
    away: bool,
    told: Option<Sentence>,
}

impl Pick {
    /// A device that went is said once, when it goes, and again when it is back.
    fn settled(&self, listed: &[Listed]) -> Settled {
        let here = self
            .asked
            .as_ref()
            .is_none_or(|picked| listed.iter().any(|device| device.asked.as_ref() == Some(&picked.id)));
        let told = match (&self.asked, here, self.away) {
            (Some(was), false, false) => {
                let now = listed
                    .iter()
                    .find(|device| device.asked.is_none())
                    .map(|device| device.label.clone())
                    .filter(|label| !label.is_empty());
                Some(Sentence::SoundGone { was: was.label.clone(), now })
            }
            (Some(back), true, true) => Some(Sentence::SoundBack(back.label.clone())),
            _ => None,
        };
        let target = self.asked.as_ref().filter(|_| here).map(|picked| picked.id.clone());
        Settled { target, away: !here, told }
    }
}

/// What `listed` names the device asked for by `using` (none: the default); none when it names it nothing.
fn named(listed: &[Listed], using: Option<&str>) -> Option<String> {
    listed
        .iter()
        .find(|device| device.asked.as_deref() == using)
        .map(|device| device.label.clone())
        .filter(|label| !label.is_empty())
}

fn listed(devices: &[Device]) -> Vec<Listed> {
    devices
        .iter()
        .map(|device| Listed { asked: device.asked_as().map(str::to_string), label: device.label().to_string() })
        .collect()
}

/// The devices the sound server lists at one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Listing {
    pub(crate) microphones: Vec<Listed>,
    pub(crate) speakers: Vec<Listed>,
}

impl Listing {
    /// What the sound server listed, microphones and speakers from the one listing.
    fn of(listing: ::sound::voice::Listing) -> Self {
        let (microphones, speakers) = webrtc::lists(listing);
        Self { microphones: listed(&microphones), speakers: listed(&speakers) }
    }
}

/// What a look does to the device one direction uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    /// It stays as it is.
    Keep,
    /// The device is asked for.
    Choose,
    /// Nothing can be asked for (the module lists no default): what is in use is written down as none.
    Note,
}

/// What a look decided for one direction, and whose ask it decided it for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Step {
    asked: Option<Picked>,
    settled: Settled,
    change: Change,
}

impl Pick {
    /// A module with no default entry follows the system's by itself; one with an entry is asked again while on the
    /// default, which may have moved, and the module skips a choice that changes nothing.
    fn step(&self, listed: &[Listed], forced: bool) -> Step {
        let settled = self.settled(listed);
        let default_listed = listed.iter().any(|device| device.asked.is_none());
        let on_default = settled.target.is_none() && default_listed;
        let change =
            match (forced || settled.target != self.using || on_default, settled.target.is_some() || default_listed) {
                (false, _) => Change::Keep,
                (true, true) => Change::Choose,
                (true, false) => Change::Note,
            };
        Step { asked: self.asked.clone(), settled, change }
    }

    /// Takes what a look decided and what the device answered; what a person is told of it. A device that refused
    /// leaves everything as it was, and what was decided for an ask the person has changed since is not said.
    fn take(&mut self, step: Step, chosen: Result<(), DevicesRefused>) -> Option<Sentence> {
        if let Err(refused) = chosen {
            return Some(words::failure::of(&failure::devices(&refused)));
        }
        if step.change != Change::Keep {
            self.using = step.settled.target;
        }
        if step.asked != self.asked {
            return None;
        }
        self.away = step.settled.away;
        step.settled.told
    }
}

/// What one look at the devices decided: what is listed, and what each direction does about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    listing: Listing,
    microphone: Step,
    speaker: Step,
}

/// What the devices answered to a plan's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chosen {
    microphone: Result<(), DevicesRefused>,
    speaker: Result<(), DevicesRefused>,
}

impl Plan {
    /// Asks `devices` for what was decided. A target was read from the listing this plan is of, so it is not listed
    /// again to be checked.
    fn choose(&self, devices: &Devices) -> Chosen {
        let ask = |step: &Step, choose: &dyn Fn(Option<&str>) -> Result<(), DevicesRefused>| match step.change {
            Change::Choose => choose(step.settled.target.as_deref()),
            Change::Keep | Change::Note => Ok(()),
        };
        Chosen {
            microphone: ask(&self.microphone, &|target| devices.choose_input_listed(target)),
            speaker: ask(&self.speaker, &|target| devices.choose_output_listed(target)),
        }
    }
}

/// The microphone and the speaker.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sounds {
    microphone: Pick,
    speaker: Pick,
}

impl Sounds {
    pub(crate) fn asking(microphone: Option<Picked>, speaker: Option<Picked>) -> Self {
        Self {
            microphone: Pick { asked: microphone, ..Pick::default() },
            speaker: Pick { asked: speaker, ..Pick::default() },
        }
    }

    pub(crate) fn ask_microphone(&mut self, asked: Option<Picked>) {
        self.microphone.asked = asked;
    }

    pub(crate) fn ask_speaker(&mut self, asked: Option<Picked>) {
        self.speaker.asked = asked;
    }

    /// The speaker in use, by its id at the sound server; none for the default.
    pub(crate) fn speaker(&self) -> Option<&str> {
        self.speaker.using.as_deref()
    }

    /// There are no devices to look at: said, when a person asked for one.
    pub(crate) fn unavailable(&self, said: &mut impl FnMut(Subject, Sentence)) {
        if self.microphone.asked.is_some() || self.speaker.asked.is_some() {
            said(Subject::Devices, words::failure::of(&failure::devices(&DevicesRefused::Unavailable)));
        }
    }

    /// What each direction does about the devices in `listing`. `forced` asks for both whatever they were: the
    /// engine's chain remembers a device chosen earlier.
    pub(crate) fn plan(&self, listing: Listing, forced: bool) -> Plan {
        Plan {
            microphone: self.microphone.step(&listing.microphones, forced),
            speaker: self.speaker.step(&listing.speakers, forced),
            listing,
        }
    }

    /// Takes a look's plan and what the devices answered to it, handing `said` what a person is told; the devices
    /// there are and the ones in use now.
    pub(crate) fn settled(&mut self, plan: Plan, chosen: Chosen, said: &mut impl FnMut(Subject, Sentence)) -> Sound {
        if let Some(told) = self.microphone.take(plan.microphone, chosen.microphone) {
            said(Subject::Microphone, told);
        }
        if let Some(told) = self.speaker.take(plan.speaker, chosen.speaker) {
            said(Subject::Speaker, told);
        }
        self.microphone.named = named(&plan.listing.microphones, self.microphone.using.as_deref());
        self.speaker.named = named(&plan.listing.speakers, self.speaker.using.as_deref());
        Sound {
            microphones: plan.listing.microphones,
            speakers: plan.listing.speakers,
            microphone: self.microphone.using.clone(),
            speaker: self.speaker.using.clone(),
        }
    }

    /// What is said once `direction`'s device answers after it did not: that it is back, by the name it was last
    /// listed by; none when it has none.
    pub(crate) fn answered(&self, direction: Direction) -> Option<Sentence> {
        let pick = match direction {
            Direction::Microphone => &self.microphone,
            Direction::Speaker => &self.speaker,
        };
        pick.named.clone().map(Sentence::SoundBack)
    }
}

#[cfg(test)]
impl Sounds {
    /// The default devices as a look that found `listing` and was taken whole leaves them.
    pub(crate) fn settled_on(listing: Listing) -> Self {
        let mut sounds = Self::default();
        let plan = sounds.plan(listing, true);
        sounds.settled(plan, Chosen { microphone: Ok(()), speaker: Ok(()) }, &mut |_, _| {});
        sounds
    }
}

/// How a look is made: whatever the devices were, or only where something moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Looking {
    Moved,
    Forced,
}

/// Which look at the devices is under way, as a call decides it: when it is late, and the look asked for while it is
/// out.
#[derive(Debug, Default)]
pub(crate) struct Looks {
    due: Option<Instant>,
    again: Option<Looking>,
}

impl Looks {
    /// Whether a look is to be made at `now`; while one is out the next is asked for once it answers
    /// ([`Looks::again`]).
    pub(crate) fn ask(&mut self, looking: Looking, now: Instant) -> bool {
        if self.due.is_some() {
            self.again = self.again.max(Some(looking));
            return false;
        }
        self.due = Some(now + LISTING_WITHIN);
        true
    }

    /// The look under way answered, or could not be made.
    pub(crate) fn answered(&mut self) {
        self.due = None;
    }

    /// The look asked for while the last was out, once.
    pub(crate) fn again(&mut self) -> Option<Looking> {
        self.again.take()
    }

    /// When the look under way is late.
    pub(crate) fn due(&self) -> Option<Instant> {
        self.due
    }

    /// Whether the look under way is still out at its bound at `now`: it is let go, and the next may be asked.
    pub(crate) fn stalled(&mut self, now: Instant) -> bool {
        let late = self.due.is_some_and(|due| now >= due);
        if late {
            self.due = None;
        }
        late
    }
}

/// A look at the devices running as a task, the listing asked of the process's sound once; letting go of it ends it.
#[derive(Debug)]
pub(crate) struct Glance(Task<(Plan, Chosen)>);

impl Glance {
    /// Looks at `devices` for `sounds` as it is now.
    pub(crate) fn start(sounds: Sounds, devices: Devices, looking: Looking) -> Self {
        Self::start_on(::sound::Sound::shared(), sounds, devices, looking)
    }

    /// The same, asking `sound` for the listing.
    pub(crate) fn start_on(sound: ::sound::Sound, sounds: Sounds, devices: Devices, looking: Looking) -> Self {
        Self(Task::spawn(async move {
            // A sound server that does not list has no devices: the look is of an empty listing.
            let listing = Listing::of(sound.listing().await.unwrap_or_default());
            let plan = sounds.plan(listing, looking == Looking::Forced);
            let chosen = plan.choose(&devices);
            (plan, chosen)
        }))
    }

    /// Resolves with the look, [`Panicked`] when it ended without an answer; cancel-safe.
    pub(crate) async fn answer(&mut self) -> Result<(Plan, Chosen), Panicked> {
        self.0.answer().await
    }
}

/// The look at the devices of a probe: [`Looks`] and the [`Glance`] they decide.
#[derive(Debug, Default)]
pub(crate) struct Settling {
    looks: Looks,
    glance: Option<Glance>,
}

impl Settling {
    /// Looks at `devices` for `sounds` as it is at `now`; while a look is out, the next one is asked for once it
    /// answers ([`Settling::again`]).
    pub(crate) fn ask(&mut self, sounds: &Sounds, devices: Devices, looking: Looking, now: Instant) {
        if self.looks.ask(looking, now) {
            self.glance = Some(Glance::start(sounds.clone(), devices, looking));
        }
    }

    /// Resolves with the look under way, [`Panicked`] when it ended without an answer; never, while none is out.
    /// Cancel-safe.
    pub(crate) async fn answer(&mut self) -> Result<(Plan, Chosen), Panicked> {
        let Some(glance) = &mut self.glance else { return std::future::pending().await };
        let answer = glance.answer().await;
        self.glance = None;
        self.looks.answered();
        answer
    }

    /// The look asked for while the last was out, once.
    pub(crate) fn again(&mut self) -> Option<Looking> {
        self.looks.again()
    }

    /// When the look under way is late.
    pub(crate) fn due(&self) -> Option<Instant> {
        self.looks.due()
    }

    /// Whether the look under way is still out at its bound at `now`: it is let go, and the next may be asked.
    pub(crate) fn stalled(&mut self, now: Instant) -> bool {
        let late = self.looks.stalled(now);
        if late {
            self.glance = None;
        }
        late
    }
}

/// Which device a standing is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Microphone,
    Speaker,
}

/// What a person is told of the chain's devices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Told {
    /// A device would not open, or would not take a choice.
    Refused(Direction, Failure),
    /// A device did not answer in time and is being tried again, so nothing follows from it yet.
    Unanswered(Direction, Failure),
    /// A device's retry did not answer either, and nothing more is tried of it until it is asked again.
    Hung(Direction, Failure),
    /// A device said not to answer opened since, its retry or another ask.
    Answered(Direction),
    /// The chain itself stopped: neither device sounds again in this run.
    Stopped,
}

/// Whether the chain's thread runs, as far as a person was told.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Chain {
    #[default]
    Running,
    /// It stopped, and that was said.
    SaidStopped,
}

/// Whether a device's opening is said back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Answer {
    /// It opened, or closed, since it was last said not to answer.
    #[default]
    Given,
    /// It was said not to answer, and has not opened or closed since.
    Owed,
}

/// What the chain last said became of the microphone and the speaker, so each refusal is said once, a device that
/// did not answer is said back once it opens, and a chain that stopped is said once in place of its devices'
/// refusals.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Standings {
    heard: Sounding,
    chain: Chain,
    microphone: Answer,
    speaker: Answer,
}

impl Standings {
    /// Takes what the chain says `now` and whether its thread is `alive`: each device newly refused, with why, newly
    /// unanswered or hung, or open after it was said not to answer, however asked since; or only that the chain
    /// stopped. Any other open, close or opening says nothing, and a standing that stands is not said again.
    pub(crate) fn hear(&mut self, now: Sounding, alive: bool) -> Vec<Told> {
        let was = std::mem::replace(&mut self.heard, now);
        if !alive {
            return self.stopped(alive).into_iter().collect();
        }
        [
            (Direction::Microphone, was.microphone, now.microphone, &mut self.microphone),
            (Direction::Speaker, was.speaker, now.speaker, &mut self.speaker),
        ]
        .into_iter()
        .filter(|(_, was, now, _)| was != now)
        .filter_map(|(direction, _, now, answer)| {
            let next = match now {
                Standing::Unanswered | Standing::Hung => Answer::Owed,
                Standing::Open | Standing::Closed => Answer::Given,
                Standing::Opening | Standing::Refused(_) => *answer,
            };
            let owed = std::mem::replace(answer, next) == Answer::Owed;
            match now {
                Standing::Refused(why) => Some(Told::Refused(direction, failure::devices(&why))),
                Standing::Unanswered => Some(Told::Unanswered(direction, Failure::EngineStalled)),
                Standing::Hung => Some(Told::Hung(direction, Failure::EngineStalled)),
                Standing::Open => owed.then_some(Told::Answered(direction)),
                Standing::Closed | Standing::Opening => None,
            }
        })
        .collect()
    }

    /// That the chain stopped, once, when a look finds its thread no longer `alive`.
    pub(crate) fn stopped(&mut self, alive: bool) -> Option<Told> {
        if alive || self.chain == Chain::SaidStopped {
            return None;
        }
        self.chain = Chain::SaidStopped;
        Some(Told::Stopped)
    }
}

/// Resolves with what the chain says became of its devices, each time it changes; never, with no devices or once
/// the chain is gone.
pub(crate) async fn sounding(told: &mut Option<tokio::sync::watch::Receiver<Sounding>>) -> Sounding {
    let Some(told) = told else { return std::future::pending().await };
    match told.changed().await {
        Ok(()) => *told.borrow_and_update(),
        Err(_) => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(label: &str, asked: Option<&str>) -> Listed {
        Listed { asked: asked.map(str::to_string), label: label.to_string() }
    }

    fn here() -> Vec<Listed> {
        vec![device("Laptop", None), device("Headset", Some("Headset")), device("Laptop", Some("Laptop"))]
    }

    fn picked(name: &str) -> Picked {
        Picked { id: name.into(), label: name.into() }
    }

    fn pick(asked: Option<&str>, using: Option<&str>, away: bool) -> Pick {
        Pick { asked: asked.map(picked), using: using.map(str::to_string), away, named: None }
    }

    #[test]
    fn the_default_is_always_here_and_never_told() {
        let settled = pick(None, None, false).settled(&here());
        assert_eq!(settled, Settled { target: None, away: false, told: None });
    }

    #[test]
    fn a_device_that_is_here_is_used_and_not_told() {
        let settled = pick(Some("Headset"), None, false).settled(&here());
        assert_eq!(settled, Settled { target: Some("Headset".into()), away: false, told: None });
    }

    #[test]
    fn a_device_that_goes_is_told_once_with_the_default_that_takes_its_place_and_stays_asked_for() {
        let gone = [device("Laptop", None), device("Laptop", Some("Laptop"))];
        let asked = pick(Some("Headset"), Some("Headset"), false);
        let settled = asked.settled(&gone);
        let told = Sentence::SoundGone { was: "Headset".into(), now: Some("Laptop".into()) };
        assert_eq!(settled, Settled { target: None, away: true, told: Some(told) });
        // The next look, with it still away, says nothing; and the choice itself was never changed.
        let again = pick(Some("Headset"), None, true).settled(&gone);
        assert_eq!(again, Settled { target: None, away: true, told: None });
        assert_eq!(asked.asked, Some(picked("Headset")));
    }

    #[test]
    fn a_device_that_returns_is_taken_again_and_told_once() {
        let settled = pick(Some("Headset"), None, true).settled(&here());
        assert_eq!(
            settled,
            Settled { target: Some("Headset".into()), away: false, told: Some(Sentence::SoundBack("Headset".into())) }
        );
        let after = pick(Some("Headset"), Some("Headset"), false).settled(&here());
        assert_eq!(after.told, None);
    }

    #[test]
    fn a_saved_device_missing_at_the_start_is_told_like_one_that_went() {
        let settled = pick(Some("Desk"), None, false).settled(&here());
        let told = Sentence::SoundGone { was: "Desk".into(), now: Some("Laptop".into()) };
        assert_eq!(settled, Settled { target: None, away: true, told: Some(told) });
    }

    #[test]
    fn a_module_with_no_default_entry_is_told_without_a_name_for_what_stands_in() {
        let no_default = [device("Laptop", Some("Laptop"))];
        let told = pick(Some("Headset"), None, false).settled(&no_default).told;
        assert_eq!(told, Some(Sentence::SoundGone { was: "Headset".into(), now: None }));
    }

    #[test]
    fn a_default_the_module_cannot_name_stands_in_without_a_name() {
        let nameless = [device("", None)];
        let told = pick(Some("Headset"), None, false).settled(&nameless).told;
        assert_eq!(told, Some(Sentence::SoundGone { was: "Headset".into(), now: None }));
    }

    #[test]
    fn with_no_audio_module_a_device_asked_for_is_refused_in_a_sentence_and_the_default_says_nothing() {
        let mut said = Vec::new();
        Sounds::asking(Some(picked("Headset")), None).unavailable(&mut |subject, text| said.push((subject, text)));
        assert_eq!(said, [(Subject::Devices, Sentence::Phrase(words::Phrase::SoundUnavailable))]);
        said.clear();
        Sounds::default().unavailable(&mut |subject, text| said.push((subject, text)));
        assert!(said.is_empty());
    }

    fn listing() -> Listing {
        Listing { microphones: here(), speakers: here() }
    }

    const CHOSEN: Chosen = Chosen { microphone: Ok(()), speaker: Ok(()) };

    #[test]
    fn a_look_asks_for_what_moved_and_a_forced_one_for_both() {
        let mut sounds = Sounds::asking(Some(picked("Headset")), None);
        let first = sounds.plan(listing(), true);
        assert_eq!((first.microphone.change, first.speaker.change), (Change::Choose, Change::Choose));
        let sound = sounds.settled(first, CHOSEN, &mut |_, _| panic!("nothing went or came back"));
        assert_eq!((sound.microphone.as_deref(), sound.speaker), (Some("Headset"), None));
        let next = sounds.plan(listing(), false);
        assert_eq!(next.microphone.change, Change::Keep, "the device asked for is the one in use");
        assert_eq!(next.speaker.change, Change::Choose, "the default may have moved: asked again");
        let nameless = Listing { microphones: here(), speakers: vec![device("Laptop", Some("Laptop"))] };
        assert_eq!(sounds.plan(nameless, true).speaker.change, Change::Note, "no default entry to ask for");
    }

    #[test]
    fn a_choice_the_device_refuses_is_said_and_leaves_what_is_in_use() {
        let mut sounds = Sounds::asking(Some(picked("Headset")), None);
        let plan = sounds.plan(listing(), true);
        let mut said = Vec::new();
        let refused = Chosen { microphone: Err(DevicesRefused::Missing), speaker: Ok(()) };
        let sound = sounds.settled(plan, refused, &mut |subject, text| said.push((subject, text)));
        assert_eq!(sound.microphone, None, "not moved");
        assert_eq!(said, [(Subject::Microphone, Sentence::Phrase(words::Phrase::DeviceMissing))]);
    }

    #[test]
    fn what_a_look_decided_for_an_ask_changed_since_is_not_said() {
        let gone = Listing { microphones: vec![device("Laptop", None), device("Laptop", Some("Laptop"))], ..listing() };
        let mut sounds = Sounds::asking(Some(picked("Headset")), None);
        let plan = sounds.plan(gone.clone(), true);
        sounds.ask_microphone(Some(picked("Laptop")));
        sounds.settled(plan, CHOSEN, &mut |_, said| panic!("{said:?} is of the ask before"));
        let mut said = Vec::new();
        let sound = sounds.settled(sounds.plan(gone, false), CHOSEN, &mut |subject, text| said.push((subject, text)));
        assert_eq!((sound.microphone.as_deref(), said.len()), (Some("Laptop"), 0), "and the new ask is here");
    }

    #[test]
    fn a_device_newly_refused_is_said_once_and_one_that_opens_says_nothing() {
        let standing = |microphone, speaker| Sounding { microphone, speaker };
        let mut standings = Standings::default();
        assert_eq!(standings.hear(standing(Standing::Opening, Standing::Opening), true), []);
        assert_eq!(standings.hear(standing(Standing::Open, Standing::Opening), true), []);
        let refused = standing(Standing::Refused(DevicesRefused::Refused), Standing::Open);
        let microphone = Told::Refused(Direction::Microphone, Failure::DeviceRefused);
        assert_eq!(standings.hear(refused, true), [microphone], "exactly one notice");
        assert_eq!(standings.hear(refused, true), [], "a refusal that stands is not said again");
        let both = standing(Standing::Refused(DevicesRefused::Refused), Standing::Refused(DevicesRefused::Unavailable));
        let speaker = Told::Refused(Direction::Speaker, Failure::SoundUnavailable);
        assert_eq!(standings.hear(both, true), [speaker], "only what changed");
        assert_eq!(standings.hear(standing(Standing::Opening, Standing::Closed), true), [], "asked again: nothing yet");
        let again = standing(Standing::Refused(DevicesRefused::Refused), Standing::Closed);
        assert_eq!(standings.hear(again, true).len(), 1, "refused again after a new ask is a new refusal");
    }

    #[test]
    fn a_device_that_does_not_answer_is_said_once_as_stalled_and_once_more_when_its_retry_opens() {
        let standing = |microphone, speaker| Sounding { microphone, speaker };
        let mut standings = Standings::default();
        assert_eq!(standings.hear(standing(Standing::Opening, Standing::Open), true), []);
        let stuck = standing(Standing::Unanswered, Standing::Open);
        let microphone = Told::Unanswered(Direction::Microphone, Failure::EngineStalled);
        assert_eq!(standings.hear(stuck, true), [microphone], "one sentence");
        assert_eq!(standings.hear(stuck, true), [], "not said again while it stands");
        let back = standing(Standing::Open, Standing::Open);
        assert_eq!(standings.hear(back, true), [Told::Answered(Direction::Microphone)], "the retry's answer");
        assert_eq!(standings.hear(back, true), [], "said once");
        let speaker = Told::Unanswered(Direction::Speaker, Failure::EngineStalled);
        assert_eq!(standings.hear(standing(Standing::Open, Standing::Unanswered), true), [speaker]);
        assert_eq!(standings.hear(standing(Standing::Closed, Standing::Closed), true), [], "let go: nothing answered");
    }

    #[test]
    fn a_device_said_not_to_answer_is_said_back_however_it_opens_and_not_once_let_go() {
        let microphone = |microphone| Sounding { microphone, speaker: Standing::Open };
        let mut standings = Standings::default();
        standings.hear(microphone(Standing::Opening), true);
        let stalled = Told::Unanswered(Direction::Microphone, Failure::EngineStalled);
        assert_eq!(standings.hear(microphone(Standing::Unanswered), true), [stalled]);
        let hung = Told::Hung(Direction::Microphone, Failure::EngineStalled);
        assert_eq!(
            standings.hear(microphone(Standing::Hung), true),
            std::slice::from_ref(&hung),
            "its retry is held too"
        );
        assert_eq!(standings.hear(microphone(Standing::Opening), true), [], "another microphone picked");
        assert_eq!(standings.hear(microphone(Standing::Hung), true), [hung], "a new ask held as well is said again");
        assert_eq!(standings.hear(microphone(Standing::Opening), true), []);
        let back = [Told::Answered(Direction::Microphone)];
        assert_eq!(standings.hear(microphone(Standing::Open), true), back, "opened at last, through another ask");
        standings.hear(microphone(Standing::Unanswered), true);
        standings.hear(microphone(Standing::Closed), true);
        assert_eq!(standings.hear(microphone(Standing::Open), true), [], "let go meanwhile: nothing owed");
    }

    #[test]
    fn a_device_whose_retry_opens_is_said_back_by_the_name_it_was_last_listed_by() {
        let mut sounds = Sounds::asking(Some(picked("Headset")), None);
        assert_eq!(sounds.answered(Direction::Microphone), None, "nothing listed yet: no name to say");
        let plan = sounds.plan(listing(), true);
        sounds.settled(plan, CHOSEN, &mut |_, said| panic!("{said:?}: nothing went"));
        let mut standings = Standings::default();
        let standing = |microphone| Sounding { microphone, speaker: Standing::Open };
        standings.hear(standing(Standing::Unanswered), true);
        let said: Vec<Sentence> = standings
            .hear(standing(Standing::Open), true)
            .into_iter()
            .filter_map(|told| match told {
                Told::Answered(direction) => sounds.answered(direction),
                Told::Refused(..) | Told::Unanswered(..) | Told::Hung(..) | Told::Stopped => None,
            })
            .collect();
        assert_eq!(said, [Sentence::SoundBack("Headset".into())], "what clears the microphone's mark");
        assert_eq!(sounds.answered(Direction::Speaker), Some(Sentence::SoundBack("Laptop".into())), "the default's");
        let nameless = Listing { microphones: here(), speakers: vec![device("", None)] };
        let plan = sounds.plan(nameless, false);
        sounds.settled(plan, CHOSEN, &mut |_, _| {});
        assert_eq!(sounds.answered(Direction::Speaker), None);
    }

    #[test]
    fn a_chain_that_stopped_is_said_once_in_place_of_its_devices_refusals() {
        let gone = Sounding {
            microphone: Standing::Refused(DevicesRefused::Unavailable),
            speaker: Standing::Refused(DevicesRefused::Unavailable),
        };
        let mut standings = Standings::default();
        assert_eq!(standings.stopped(true), None, "a chain that runs says nothing");
        assert_eq!(standings.hear(gone, false), [Told::Stopped], "one sentence, not one a device");
        assert_eq!(standings.hear(gone, false), [], "said once");
        assert_eq!(standings.stopped(false), None, "whoever finds it");
        let mut looked = Standings::default();
        assert_eq!(looked.stopped(false), Some(Told::Stopped), "a look finds it before the devices tell");
        assert_eq!(looked.hear(gone, false), []);
    }

    #[tokio::test(start_paused = true)]
    async fn a_look_asked_while_one_is_out_is_kept_for_after_and_the_stronger_ask_wins() {
        let mut settling = Settling::default();
        assert_eq!(settling.again(), None);
        let now = Instant::now();
        assert!(settling.looks.ask(Looking::Moved, now), "none out: made now");
        assert!(!settling.looks.ask(Looking::Moved, now));
        assert!(!settling.looks.ask(Looking::Forced, now));
        assert!(!settling.looks.ask(Looking::Moved, now));
        assert_eq!(settling.again(), Some(Looking::Forced));
        assert_eq!(settling.again(), None, "once");
        settling.looks.answered();
        assert_eq!(settling.due(), None);
        assert!(!settling.stalled(Instant::now() + LISTING_WITHIN * 2), "no look out is never late");
    }

    #[tokio::test(start_paused = true)]
    async fn a_look_the_sound_server_never_answers_is_let_go_at_its_bound() {
        let asked = Instant::now();
        let mut looks = Looks::default();
        assert!(looks.ask(Looking::Moved, asked));
        let mut settling = Settling {
            looks,
            glance: Some(Glance(Task::spawn(async {
                std::future::pending::<()>().await;
                (Sounds::default().plan(Listing::default(), false), CHOSEN)
            }))),
        };
        assert_eq!(settling.due(), Some(asked + LISTING_WITHIN));
        assert!(!settling.stalled(asked + LISTING_WITHIN / 2));
        assert!(settling.stalled(asked + LISTING_WITHIN), "late at its bound");
        assert_eq!(settling.due(), None, "and the next look may be asked");
    }
}
