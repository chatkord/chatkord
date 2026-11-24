//! The window's own keys, taken before anything inside sees them, so they work in a field too: the zoom's, as
//! Discord's and Teams' (Ctrl+= or + zooms in, Ctrl+- out, Ctrl+0 resets), and Ctrl+K, which opens the search.

use crate::Theme;
use choices::ZoomPick;
use iced_core::keyboard::key::{Code, Physical};
use iced_core::keyboard::{self, Key};
use iced_core::widget::{Meta, Tree};
use iced_core::{Event, Layout, Rectangle, Shell, Widget, mouse, renderer};

/// `content`, where a zoom key sends `zoom` with its step and Ctrl+K sends `search`, none where there is no search.
pub(crate) fn zoom_keys<W, Message>(
    content: W,
    zoom: fn(ZoomPick) -> Message,
    search: Option<Message>,
) -> ZoomKeys<W, Message> {
    ZoomKeys { content, zoom, search }
}

pub(crate) struct ZoomKeys<W, Message> {
    content: W,
    zoom: fn(ZoomPick) -> Message,
    search: Option<Message>,
}

impl<W, Message> Meta for ZoomKeys<W, Message> {}

/// The step a key press asks for: the command key alone held, and `=`, `+`, `-` or `0` as typed, or the key the
/// digit 0 or a keypad's +, - or 0 is on whatever it types, so a layout that types another letter there (AZERTY's `à`)
/// needs no Shift.
fn step(event: &Event) -> Option<ZoomPick> {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return None;
    };
    if !modifiers.command() || modifiers.alt() {
        return None;
    }
    let placed = match physical_key {
        Physical::Code(Code::Digit0 | Code::Numpad0) => Some(ZoomPick::Reset),
        Physical::Code(Code::NumpadAdd) => Some(ZoomPick::In),
        Physical::Code(Code::NumpadSubtract) => Some(ZoomPick::Out),
        Physical::Code(_) | Physical::Unidentified(_) => None,
    };
    let Key::Character(character) = key else { return placed };
    placed.or(match character.as_str() {
        "=" | "+" => Some(ZoomPick::In),
        "-" => Some(ZoomPick::Out),
        "0" => Some(ZoomPick::Reset),
        _ => None,
    })
}

/// Whether the event is Ctrl+K (the command key alone and K), K where a Latin layout has it on a layout that types
/// another script there.
fn is_search(event: &Event) -> bool {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return false;
    };
    modifiers.command()
        && !modifiers.alt()
        && !modifiers.shift()
        && key.to_latin(*physical_key).is_some_and(|latin| latin.eq_ignore_ascii_case(&'k'))
}

impl<Message, W, R> Widget<Message, Theme, R> for ZoomKeys<W, Message>
where
    Message: Clone,
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &R,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(step) = step(event) {
            shell.publish((self.zoom)(step));
            shell.capture_event();
            return;
        }
        if let Some(search) = self.search.as_ref().filter(|_| is_search(event)) {
            shell.publish(search.clone());
            shell.capture_event();
            return;
        }
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_core::keyboard::{Location, Modifiers};

    fn pressed(typed: &str, at: Code, modifiers: Modifiers) -> Event {
        let key = Key::Character(typed.into());
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Code(at),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn a_zoom_key_is_its_place_on_the_keyboard_so_azerty_needs_no_shift() {
        assert_eq!(step(&pressed("=", Code::Equal, Modifiers::COMMAND)), Some(ZoomPick::In));
        assert_eq!(step(&pressed("+", Code::NumpadAdd, Modifiers::COMMAND)), Some(ZoomPick::In));
        assert_eq!(step(&pressed("-", Code::Minus, Modifiers::COMMAND)), Some(ZoomPick::Out));
        assert_eq!(step(&pressed("0", Code::Digit0, Modifiers::COMMAND)), Some(ZoomPick::Reset));
        assert_eq!(step(&pressed("à", Code::Digit0, Modifiers::COMMAND)), Some(ZoomPick::Reset), "AZERTY's 0 key");
        assert_eq!(step(&pressed("-", Code::Digit6, Modifiers::COMMAND)), Some(ZoomPick::Out), "AZERTY's minus");
        assert_eq!(step(&pressed("0", Code::Numpad0, Modifiers::COMMAND)), Some(ZoomPick::Reset));
        assert_eq!(step(&pressed("à", Code::Digit0, Modifiers::empty())), None, "no command key");
        assert_eq!(step(&pressed("à", Code::Digit0, Modifiers::COMMAND | Modifiers::ALT)), None, "AltGr held");
        assert_eq!(step(&pressed("é", Code::Digit2, Modifiers::COMMAND)), None, "another key");
    }

    #[test]
    fn ctrl_k_is_the_search_on_a_latin_layout_and_on_one_that_types_another_script_there() {
        assert!(is_search(&pressed("k", Code::KeyK, Modifiers::COMMAND)));
        assert!(is_search(&pressed("κ", Code::KeyK, Modifiers::COMMAND)), "a Greek layout's K");
        assert!(!is_search(&pressed("κ", Code::KeyK, Modifiers::empty())), "no command key");
        assert!(!is_search(&pressed("k", Code::KeyK, Modifiers::COMMAND | Modifiers::SHIFT)), "Shift held");
        assert!(!is_search(&pressed("λ", Code::KeyL, Modifiers::COMMAND)), "another key");
    }
}
