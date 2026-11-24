//! A context menu's ways in: a right press on what it is for, or the menu key or Shift+F10 while a control inside it
//! has the keyboard's focus, as every desktop system offers them.

use crate::Theme;
use crate::focus::focused_index;
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::widget::{Tree, tree};
use iced_core::{Element, Event, Layout, Rectangle, Shell, Widget, mouse, renderer};

/// `content`, whose context menu `open` asks for.
pub(crate) fn on_menu<'a, Message: Clone + 'a, R: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    open: Message,
) -> Element<'a, Message, Theme, R> {
    let pressed = iced_widget::mouse_area(content.into()).on_right_press(open.clone());
    OnMenu { content: pressed._boxed(), open }._boxed()
}

struct OnMenu<'a, Message, R> {
    content: Element<'a, Message, Theme, R>,
    open: Message,
}

/// Whether a key press asks for a context menu: the menu key, or Shift+F10.
fn asks_menu(key: &Key, modifiers: keyboard::Modifiers) -> bool {
    match key {
        Key::Named(Named::ContextMenu) => true,
        Key::Named(Named::F10) => modifiers.shift(),
        _ => false,
    }
}

impl<Message, R> iced_core::widget::Meta for OnMenu<'_, Message, R> {}

impl<Message: Clone, R: renderer::Renderer> Widget<Message, Theme, R> for OnMenu<'_, Message, R> {
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

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
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return };
        if shell.is_event_captured() || !asks_menu(key, *modifiers) {
            return;
        }
        let (content, child) = (&mut self.content, &mut tree.children[0]);
        if focused_index(|operation| content.operate(child, layout, viewport, renderer, operation)).is_some() {
            shell.publish(self.open.clone());
            shell.capture_event();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Renderer;
    use iced_core::keyboard::Modifiers;
    use iced_core::{Point, Size};
    use iced_test::Simulator;

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Pressed,
        Menu,
    }

    fn row() -> Simulator<'static, Said, Theme, Renderer> {
        crate::testing::hermetic_fonts();
        let button = crate::Button::new(design::Label::new("general").unwrap()).on_press(Said::Pressed);
        let page = iced_widget::Column::with_children([on_menu(button.view::<Renderer>(), Said::Menu)]);
        Simulator::with_size(iced_core::Settings::default(), Size::new(300.0, 100.0), page)
    }

    fn key(sim: &mut Simulator<'static, Said, Theme, Renderer>, key: Named, modifiers: Modifiers) {
        sim.simulate([Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })]);
    }

    #[test]
    fn a_right_press_or_the_menu_key_on_the_focused_row_opens_its_menu() {
        let mut sim = row();
        key(&mut sim, Named::ContextMenu, Modifiers::empty());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [], "nothing has the focus");
        sim.point_at(Point::new(20.0, 10.0));
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Menu], "a right press");
        sim.simulate(crate::testing::click());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Pressed], "a press is the row's own, and focuses it");
        key(&mut sim, Named::ContextMenu, Modifiers::empty());
        key(&mut sim, Named::F10, Modifiers::SHIFT);
        key(&mut sim, Named::F10, Modifiers::empty());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Menu, Said::Menu], "the menu key and Shift+F10, not F10");
    }
}
