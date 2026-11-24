//! A share's sound: what the picked screen or window is heard as, heard here and fed to the engine beside the share's
//! picture.

use crate::failure;
use ::share::{Kind, Owner};
#[cfg(any(target_os = "linux", windows))]
use ::sound::share::App;
pub(crate) use ::sound::share::Hears;
use ::sound::share::{CHANNELS, Capture};
use choices::SoundSwitches;
use link::Link;
use screens::SoundSaid;
use std::sync::{Arc, OnceLock};
use webrtc::{Sharing, SoundFeeder};
use words::Failure;

/// What a share's sound is asked to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundAsked {
    /// What was picked has: a whole screen every app's but ours, a window its own app's, as the switches allow.
    Follow,
    /// These applications, by name; none is no sound.
    #[cfg(target_os = "linux")]
    Apps(Vec<App>),
    /// This application, which Windows hears as one process tree.
    #[cfg(windows)]
    App(App),
}

/// What a share hears and is said as (none for named apps); `owner` is asked only for a window whose sound is on. A
/// window whose owner's processes `holds_us` (this process, or one that launched it) goes without sound, so a call
/// never hears itself; an unknown kind is a screen.
pub(crate) fn decide(
    sound: SoundAsked,
    kind: Option<Kind>,
    switches: SoundSwitches,
    owner: impl FnOnce() -> Option<Owner>,
    holds_us: impl FnOnce(&[u32]) -> bool,
) -> (Hears, Option<SoundSaid>) {
    match (sound, kind) {
        #[cfg(target_os = "linux")]
        (SoundAsked::Apps(apps), _) if apps.is_empty() => (Hears::Nothing, None),
        #[cfg(target_os = "linux")]
        (SoundAsked::Apps(apps), _) => (Hears::Apps(apps.into_iter().collect()), None),
        #[cfg(windows)]
        (SoundAsked::App(App { name, root }), _) => (Hears::Tree { roots: vec![root], app_id: name }, None),
        (SoundAsked::Follow, Some(Kind::Window)) if !switches.window => (Hears::Nothing, Some(SoundSaid::Off)),
        (SoundAsked::Follow, Some(Kind::Window)) => match owner() {
            Some(Owner { pids, .. }) if holds_us(&pids) => (Hears::Nothing, Some(SoundSaid::Own)),
            Some(Owner { app_id, pids, name }) => (Hears::Tree { roots: pids, app_id }, Some(SoundSaid::App(name))),
            // On Linux the mix starts linked to nothing, so an application the sharer picks joins it live.
            #[cfg(target_os = "linux")]
            None => (Hears::Apps(std::collections::HashSet::new()), Some(SoundSaid::NotFound)),
            #[cfg(not(target_os = "linux"))]
            None => (Hears::Nothing, Some(SoundSaid::NotFound)),
        },
        (SoundAsked::Follow, Some(Kind::Monitor | Kind::Virtual) | None) if !switches.screen => {
            (Hears::Nothing, Some(SoundSaid::Off))
        }
        (SoundAsked::Follow, Some(Kind::Monitor | Kind::Virtual) | None) => {
            (Hears::AllButOurs, Some(SoundSaid::Everything))
        }
    }
}

/// The sound of a share being sent: its capture, which hands every 10 ms to the engine from a real-time thread of its
/// own. Dropping it stops the feeding and unlinks the applications.
pub(crate) struct Heard {
    capture: Capture,
}

impl Heard {
    /// Hears what `hears` names from now on instead.
    #[cfg(target_os = "linux")]
    pub(crate) fn choose(&self, hears: Hears) {
        self.capture.choose(hears);
    }

    /// Whether the sound has stayed stopped past its wait, its capture tried again meanwhile.
    pub(crate) fn stopped(&self) -> bool {
        self.capture.stopped()
    }
}

/// Hears what `hears` names and sends it as the share's sound; nothing heard sends no sound. A refusal leaves the
/// picture going on without it.
pub(crate) async fn start(sharing: &Sharing, link: &Link, hears: Hears) -> Result<Option<Heard>, Failure> {
    if hears == Hears::Nothing {
        return Ok(None);
    }
    let channels = u32::try_from(CHANNELS).map_err(|_| failure::sound(::sound::share::SoundError::Stream))?;
    // The capture opens first, so a refusal leaves no track on the share; what it hears before the track is made
    // goes nowhere.
    let feeder = Arc::new(OnceLock::<SoundFeeder>::new());
    let sink = Arc::clone(&feeder);
    let feed = move |chunk: &[i16; ::sound::share::CHUNK]| {
        if let Some(feeder) = sink.get() {
            // Refused only for a wrong length, which a chunk never has.
            let _ = feeder.push_now(chunk);
        }
    };
    // The sound server or Windows' activation may be slow to answer, which no worker of the runtime waits on.
    let capture = tokio::task::spawn_blocking(move || Capture::open(hears, feed))
        .await
        .map_err(|_| failure::sound(::sound::share::SoundError::Stream))?
        .map_err(failure::sound)?;
    let track = sharing.sound(link, ::sound::share::RATE, channels).await.map_err(failure::voice)?;
    let _ = feeder.set(track);
    Ok(Some(Heard { capture }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: SoundSwitches = SoundSwitches { screen: true, window: true };
    const OFF: SoundSwitches = SoundSwitches { screen: false, window: false };
    const OURS: u32 = 400;
    const SHELL: u32 = 4;

    /// What the sound crate answers on a machine where `OURS` runs under `SHELL`.
    fn holds_us(pids: &[u32]) -> bool {
        pids.iter().any(|pid| [OURS, SHELL].contains(pid))
    }

    fn brave() -> Option<Owner> {
        Some(Owner { app_id: "brave-browser".into(), pids: vec![500], name: "Brave".into() })
    }

    fn unasked() -> Option<Owner> {
        panic!("the window's owner is asked for only for a window whose sound is on")
    }

    fn chatkord() -> Option<Owner> {
        Some(Owner { app_id: "chatkord".into(), pids: vec![OURS], name: "Chatkord".into() })
    }

    #[test]
    fn a_whole_screen_hears_every_app_but_ours_and_a_window_its_owner() {
        let screen = decide(SoundAsked::Follow, Some(Kind::Monitor), ON, unasked, holds_us);
        assert_eq!(screen, (Hears::AllButOurs, Some(SoundSaid::Everything)));
        let unknown = decide(SoundAsked::Follow, None, ON, unasked, holds_us).0;
        assert_eq!(unknown, Hears::AllButOurs, "an unknown kind is a screen");
        let window = decide(SoundAsked::Follow, Some(Kind::Window), ON, brave, holds_us);
        let tree = Hears::Tree { roots: vec![500], app_id: "brave-browser".into() };
        assert_eq!(window, (tree, Some(SoundSaid::App("Brave".into()))));
    }

    #[test]
    fn a_window_whose_app_is_not_found_waits_for_a_pick_and_says_so() {
        let lost = decide(SoundAsked::Follow, Some(Kind::Window), ON, || None, holds_us);
        #[cfg(target_os = "linux")]
        let waiting = Hears::Apps(std::collections::HashSet::new());
        #[cfg(not(target_os = "linux"))]
        let waiting = Hears::Nothing;
        assert_eq!(lost, (waiting, Some(SoundSaid::NotFound)), "on Linux a mix linked to nothing, for a pick");
    }

    #[test]
    fn chatkords_own_window_is_shared_without_sound_and_says_so() {
        let own = decide(SoundAsked::Follow, Some(Kind::Window), ON, chatkord, holds_us);
        assert_eq!(own, (Hears::Nothing, Some(SoundSaid::Own)));
    }

    #[test]
    fn a_window_whose_owner_holds_this_process_is_shared_without_sound() {
        let launcher = || Some(Owner { app_id: "explorer".into(), pids: vec![SHELL], name: "Explorer".into() });
        let held = decide(SoundAsked::Follow, Some(Kind::Window), ON, launcher, holds_us);
        assert_eq!(held, (Hears::Nothing, Some(SoundSaid::Own)), "a window of what launched this process");
        let sibling = decide(SoundAsked::Follow, Some(Kind::Window), ON, brave, holds_us);
        assert_ne!(sibling.0, Hears::Nothing, "a window of an unrelated application is heard");
    }

    #[test]
    fn a_switch_off_is_no_sound() {
        for kind in [Some(Kind::Monitor), Some(Kind::Window), None] {
            assert_eq!(
                decide(SoundAsked::Follow, kind, OFF, unasked, holds_us),
                (Hears::Nothing, Some(SoundSaid::Off))
            );
        }
        let only_window = SoundSwitches { screen: false, window: true };
        assert_eq!(decide(SoundAsked::Follow, Some(Kind::Monitor), only_window, unasked, holds_us).0, Hears::Nothing);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn named_apps_are_heard_as_named() {
        let game = App { name: "Game".into(), binary: None };
        let named = decide(SoundAsked::Apps(vec![game.clone()]), Some(Kind::Window), OFF, unasked, holds_us);
        assert_eq!(named, (Hears::Apps([game].into()), None));
        assert_eq!(decide(SoundAsked::Apps(Vec::new()), None, ON, unasked, holds_us), (Hears::Nothing, None));
    }

    #[cfg(windows)]
    #[test]
    fn a_named_app_is_heard_as_its_tree_whatever_is_picked() {
        let game = App { name: "game".into(), root: 700 };
        let tree = (Hears::Tree { roots: vec![700], app_id: "game".into() }, None);
        assert_eq!(decide(SoundAsked::App(game.clone()), Some(Kind::Window), OFF, unasked, holds_us), tree);
        assert_eq!(decide(SoundAsked::App(game), Some(Kind::Monitor), ON, unasked, holds_us), tree);
    }
}
