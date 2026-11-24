//! The language the window is said in: the one chosen in Settings › Appearance, else the first of the system's
//! languages this build carries. What the window keeps to say (a toast, a notice) is a value said through its words,
//! so a change of language reaches it as it reaches every view.

use super::*;
use choices::LanguageTag;

/// What a person does to the Language field, wherever it is drawn: Settings › Appearance, or the first-run door,
/// whose messages are not the window's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageSaid {
    /// The field pressed: its menu opens, or closes while it is open.
    Toggle,
    /// A language picked, or none to match the system's.
    Choose(Option<Language>),
    /// Escape, or a press outside the open menu.
    Close,
}

impl From<LanguageSaid> for Message {
    fn from(said: LanguageSaid) -> Message {
        match said {
            LanguageSaid::Toggle => Message::LanguageMenu,
            LanguageSaid::Choose(chosen) => Message::ChooseLanguage(chosen),
            LanguageSaid::Close => Message::CloseMenu,
        }
    }
}

/// What everything here is said in, and what the system asks for.
#[derive(Debug)]
pub(super) struct Speech {
    /// What everything here is said in: the language chosen, else the one the system's languages ask for.
    pub(super) words: Words,
    /// The languages the system names, most wanted first; none until the host says.
    pub(super) system: Vec<LanguageIdentifier>,
}

impl Default for Speech {
    fn default() -> Speech {
        Speech { words: Words::english(), system: Vec::new() }
    }
}

impl Window {
    /// The language this window says everything in.
    pub fn words(&self) -> &Words {
        &self.speech.words
    }

    /// The language the system's languages ask for: the one Match system says the window in.
    pub fn system_language(&self) -> Language {
        Language::negotiate(&self.speech.system)
    }

    /// The language chosen for the window; none while it matches the system's.
    pub fn chosen_language(&self) -> Option<Language> {
        self.voice.language.and_then(Language::from_tag)
    }

    /// The system's languages as the host read them.
    pub(super) fn system_languages(&mut self, system: Vec<LanguageIdentifier>) -> Vec<Ask> {
        self.speech.system = system;
        self.speak(self.chosen_language());
        Vec::new()
    }

    /// Whether the Language field is on show: on Settings › Appearance, or on the first-run door, which is the
    /// window while no server is added.
    fn language_field_shown(&self) -> bool {
        self.hardware.device == Some(DevicePage::Appearance) || self.welcome_door().is_some()
    }

    /// Whether the Language field's menu is open.
    pub fn language_menu_open(&self) -> bool {
        self.desk.menu == Some(menu::Opened::Language)
    }

    /// The Language field pressed, on the page or the door that holds it: its menu opens, or closes while it is open.
    pub(super) fn language_menu(&mut self) -> Vec<Ask> {
        self.desk.menu = match self.desk.menu.take() {
            Some(menu::Opened::Language) => None,
            _ if self.language_field_shown() => Some(menu::Opened::Language),
            other => other,
        };
        Vec::new()
    }

    /// The Language field's menu goes with the page or the door that holds it.
    pub(super) fn settle_language_menu(&mut self) {
        if self.desk.menu == Some(menu::Opened::Language) && !self.language_field_shown() {
            self.desk.menu = None;
        }
    }

    /// A language picked, or none to match the system's: said at once and kept, and the field's menu closed.
    pub(super) fn choose_language(&mut self, chosen: Option<Language>) -> Vec<Ask> {
        if self.language_menu_open() {
            self.desk.menu = None;
        }
        self.voice.language = chosen.map(LanguageTag::from);
        self.speak(chosen);
        vec![Ask::Keep(Keep::Language(chosen))]
    }

    /// The words in `chosen`, else the system's language; read again only when that language changes. A language just
    /// picked is said as given, not as the catalog its tag names.
    pub(super) fn speak(&mut self, chosen: Option<Language>) {
        let language = chosen.unwrap_or_else(|| self.system_language());
        if self.speech.words.language() != language {
            self.speech.words = Words::new(language);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, id, tell, two_signed_in};
    use proto::rules::{Grant, Grants};

    /// A French catalog the tests bring, holding what they read, apart from the one this build carries.
    fn french() -> Language {
        Language::given(
            "fr",
            "Français",
            "language-name = Français\n\
             tray-open = Ouvrir Chatkord\n\
             mute-mic = Couper le micro\n\
             deafen-self = Couper le son\n\
             tray-quit = Quitter Chatkord\n\
             raised-on = Vous avez un rôle plus élevé sur { $server }\n",
        )
    }

    fn system(tags: &[&str]) -> Message {
        Message::SystemLanguages(tags.iter().filter_map(|tag| tag.parse().ok()).collect())
    }

    #[test]
    fn a_language_picked_is_said_at_once_kept_and_beats_the_systems() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        window.update(system(&["en-GB"]), clock.at());
        assert_eq!(window.words().language(), Language::ENGLISH);
        let french = french();
        let todo = window.update(Message::ChooseLanguage(Some(french)), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Language(Some(kept)))] if *kept == french), "{todo:?}");
        assert_eq!((window.words().language(), window.chosen_language()), (french, Some(french)));
        window.update(system(&["ja-JP", "en-US"]), clock.at());
        assert_eq!(window.words().language(), french, "the system's languages change nothing chosen");
        let todo = window.update(Message::ChooseLanguage(None), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Language(None))]), "{todo:?}");
        assert_eq!(window.words().language(), Language::ENGLISH, "the system's again");
    }

    #[test]
    fn match_system_says_the_first_of_the_systems_languages_carried_else_english() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        assert_eq!((window.system_language(), window.chosen_language()), (Language::ENGLISH, None), "none told");
        window.update(system(&["ja-JP", "en-AU"]), clock.at());
        assert_eq!(window.system_language(), Language::ENGLISH, "English is the second asked, by its region");
        window.update(system(&["ja-JP"]), clock.at());
        assert_eq!((window.system_language(), window.words().language()), (Language::ENGLISH, Language::ENGLISH));
    }

    #[test]
    fn a_language_kept_from_the_last_run_is_said_from_the_start() {
        let french = french();
        let (window, _) = Window::launch(Vec::new(), None, Instant::now());
        let window =
            window.remembering(Remembered { language: Some(french.into()), ..Remembered::default() }, BTreeMap::new());
        assert_eq!(window.words().phrase(Phrase::TrayOpen), "Ouvrir Chatkord");
    }

    #[test]
    fn the_language_menu_opens_on_appearance_alone_and_closes_as_any_menu_does_or_by_a_pick() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::LanguageMenu, clock.at());
        assert!(!window.language_menu_open(), "not while Appearance is not shown");
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        window.update(Message::LanguageMenu, clock.at());
        assert!(window.language_menu_open());
        window.update(Message::LanguageMenu, clock.at());
        assert!(!window.language_menu_open(), "pressed again");
        window.update(Message::LanguageMenu, clock.at());
        window.update(Message::CloseMenu, clock.at());
        assert!(!window.language_menu_open(), "Escape, or a press outside");
        window.update(Message::LanguageMenu, clock.at());
        window.update(Message::ChooseLanguage(Some(Language::ENGLISH)), clock.at());
        assert!(!window.language_menu_open(), "a language picked");
        window.update(Message::LanguageMenu, clock.at());
        let owls = id(&window, 0);
        window.update(Message::Menu(owls, MenuFrom::Rail), clock.at());
        assert!(!window.language_menu_open() && window.menu().is_some(), "one menu at a time");
        window.update(Message::CloseMenu, clock.at());
        window.update(Message::LanguageMenu, clock.at());
        window.update(Message::OpenDevice(DevicePage::VoiceAndVideo), clock.at());
        assert!(!window.language_menu_open(), "it goes with its page");
    }

    #[test]
    fn the_first_run_door_offers_the_language_menu_and_the_add_dialog_and_a_sign_in_do_not() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        assert!(window.welcome_door().is_some(), "no server is added");
        window.update(Message::LanguageMenu, clock.at());
        assert!(window.language_menu_open(), "opens on the first-run door");
        window.update(Message::CloseMenu, clock.at());
        assert!(!window.language_menu_open(), "Escape, or a press outside");
        window.update(Message::LanguageMenu, clock.at());
        let french = french();
        let todo = window.update(Message::ChooseLanguage(Some(french)), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Language(Some(kept)))] if *kept == french), "{todo:?}");
        assert_eq!((window.words().language(), window.chosen_language()), (french, Some(french)), "said at once");
        assert!(!window.language_menu_open(), "a language picked");

        let mut window = two_signed_in(&clock);
        window.update(Message::Add, clock.at());
        assert!(window.add_dialog().is_some() && window.welcome_door().is_none());
        window.update(Message::LanguageMenu, clock.at());
        assert!(!window.language_menu_open(), "the Add a server dialog has no language field");
    }

    #[test]
    fn a_toast_shown_and_the_tray_menu_are_said_again_in_the_language_picked() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let books_id = id(&window, 1);
        let admin: Grants = [Grant::ManageMembers].into_iter().collect();
        let raised = place::SessionMessage::Standing { place: crate::harness::ADMIN_PLACE, grants: admin };
        tell(&mut window, &clock, books_id, place::Message::Session(raised));
        let said = |window: &Window| window.toast().map(|toast| window.words().said(toast.said()));
        let english = said(&window);
        assert_eq!(english.as_deref(), Some("You have a higher role on Book club now"));
        let tray = |window: &Window| {
            let menu = window.tray_menu();
            std::iter::once(menu.title).chain(menu.items.into_iter().map(|item| item.label)).collect::<Vec<_>>()
        };
        assert_eq!(tray(&window), [channel::THIS.product, "Open Chatkord", "Mute", "Deafen", "Quit Chatkord"]);
        window.update(Message::ChooseLanguage(Some(french())), clock.at());
        assert_eq!(said(&window).as_deref(), Some("Vous avez un rôle plus élevé sur Book club"), "the same toast");
        assert_eq!(
            tray(&window),
            [channel::THIS.product, "Ouvrir Chatkord", "Couper le micro", "Couper le son", "Quitter Chatkord"]
        );
    }
}
