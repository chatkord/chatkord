//! The Language field: the one control that says the window in another language, drawn on Settings › Appearance and
//! on the first-run door alike, its menu opening where the window has the room.

use crate::labels::{label, phrase};
use crate::menu::Leading;
use crate::{Icon, MenuChoice, Renderer, Select, Theme};
use iced_core::Element;
use screens::{LanguageSaid, Window};
use words::{Language, Phrase, Sentence, Words};

/// A field showing the language the window is said in, Match system naming the one the system asks for, whose menu
/// lists Match system, then every language this build carries by its own name. What a person does to it is `send`.
pub(crate) fn language_field<'a, Message: Clone + 'a, R: Renderer + 'a>(
    window: &Window,
    words: &'a Words,
    send: fn(LanguageSaid) -> Message,
) -> Element<'a, Message, Theme, R> {
    let chosen = window.chosen_language();
    let system = window.system_language().name();
    let shown = match chosen {
        Some(language) => label(language.name()),
        None => label(&words.say(&Sentence::MatchingSystem(system.to_string()))),
    };
    let matching = MenuChoice {
        leading: None,
        label: phrase(Phrase::LanguageSystem, words),
        note: Some(label(system)),
        chosen: chosen.is_none(),
        message: send(LanguageSaid::Choose(None)),
    };
    let carried = Language::all().map(|language| MenuChoice {
        leading: None,
        label: label(language.name()),
        note: None,
        chosen: chosen == Some(language),
        message: send(LanguageSaid::Choose(Some(language))),
    });
    Select {
        name: phrase(Phrase::LanguageLabel, words),
        leading: Some(Leading::Icon(Icon::Globe)),
        shown,
        choices: std::iter::once(matching).chain(carried).collect(),
        open: window.language_menu_open(),
        toggle: send(LanguageSaid::Toggle),
        close: send(LanguageSaid::Close),
    }
    .view()
}
