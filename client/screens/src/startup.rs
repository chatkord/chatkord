//! The tray icon's name and menu, and what they say.

use crate::update::RestartOffer;
use choices::State;
use words::{Phrase, Words};

/// What the tray icon's menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayPick {
    /// Bring the window back.
    Open,
    /// Mute or unmute: the call's microphone in a call, the next call's outside one.
    Mute,
    /// Deafen or hear again, as for mute.
    Deafen,
    /// Quit at once, as the menu's own word says.
    Quit,
    /// Restart into the new version in place, offered only while nothing holds a restart off.
    Restart,
}

/// One entry of the tray icon's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    /// What picking it does.
    pub pick: TrayPick,
    /// What it says.
    pub label: String,
    /// Its tick, for an entry that is on or off; none for one that only acts.
    pub ticked: Option<bool>,
}

/// What the tray icon shows, in the window's language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenu {
    /// The icon's name, which a pointer over it and a screen reader say.
    pub title: String,
    /// Its menu's entries, in order.
    pub items: Vec<TrayItem>,
}

/// The tray icon's menu for a person whose microphone and hearing are `state`, with Restart to update while `restart`
/// offers one.
pub(crate) fn tray_menu(state: State, restart: RestartOffer, words: &Words) -> TrayMenu {
    let item = |pick, phrase, ticked| TrayItem { pick, label: words.phrase(phrase).into_owned(), ticked };
    let restart = (restart == RestartOffer::Offered).then(|| item(TrayPick::Restart, Phrase::TrayRestart, None));
    TrayMenu {
        title: words.phrase(Phrase::WindowTitle).into_owned(),
        items: [
            item(TrayPick::Open, Phrase::TrayOpen, None),
            item(TrayPick::Mute, Phrase::MuteMic, Some(state.mic_muted)),
            item(TrayPick::Deafen, Phrase::DeafenSelf, Some(state.deafened)),
        ]
        .into_iter()
        .chain(restart)
        .chain([item(TrayPick::Quit, Phrase::TrayQuit, None)])
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_menu_ticks_what_is_on() {
        let menu =
            tray_menu(State { mic_muted: true, deafened: false }, RestartOffer::NotApplicable, &Words::english());
        let said: Vec<(&str, Option<bool>)> =
            menu.items.iter().map(|item| (item.label.as_str(), item.ticked)).collect();
        assert_eq!(menu.title, "Chatkord");
        assert_eq!(
            said,
            [("Open Chatkord", None), ("Mute", Some(true)), ("Deafen", Some(false)), ("Quit Chatkord", None)]
        );
    }

    #[test]
    fn the_tray_menu_offers_a_restart_before_quit_only_when_one_is_offered() {
        let restart = |offer| {
            let menu = tray_menu(State::default(), offer, &Words::english());
            menu.items.iter().map(|item| item.label.clone()).collect::<Vec<_>>()
        };
        let offered = restart(RestartOffer::Offered);
        assert_eq!(offered[3..], ["Restart to update", "Quit Chatkord"]);
        assert_eq!(restart(RestartOffer::DeferredUntilVoiceEnds).len(), 4);
        assert_eq!(restart(RestartOffer::NotApplicable).len(), 4);
    }
}
