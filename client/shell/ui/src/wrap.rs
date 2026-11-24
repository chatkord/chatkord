//! What every widget that wraps one other repeats, forwarded to the child: a wrapper names what it forwards and writes
//! the rest. It holds its child as `content`, under the impl's `Message` and `R`, its tree `tree.children[0]`.

macro_rules! forward {
    () => {};
    (size $($rest:tt)*) => {
        fn size(&self) -> iced_core::Size<iced_core::Length> {
            self.content.size()
        }
        $crate::wrap::forward!($($rest)*);
    };
    (diff $($rest:tt)*) => {
        fn diff(&mut self, tree: &mut iced_core::widget::Tree) {
            tree.diff_children(std::slice::from_mut(&mut self.content));
        }
        $crate::wrap::forward!($($rest)*);
    };
    (layout $($rest:tt)*) => {
        fn layout(&mut self, tree: &mut iced_core::widget::Tree, renderer: &R, limits: &iced_core::layout::Limits) {
            self.content.layout(&mut tree.children[0], renderer, limits);
            tree.size = tree.children[0].size;
        }
        $crate::wrap::forward!($($rest)*);
    };
    (operate $($rest:tt)*) => {
        fn operate(
            &mut self,
            tree: &mut iced_core::widget::Tree,
            layout: iced_core::Layout,
            viewport: &iced_core::Rectangle,
            renderer: &R,
            operation: &mut dyn iced_core::widget::Operation,
        ) {
            self.content.operate(&mut tree.children[0], layout, viewport, renderer, operation);
        }
        $crate::wrap::forward!($($rest)*);
    };
    (update $($rest:tt)*) => {
        fn update(
            &mut self,
            tree: &mut iced_core::widget::Tree,
            event: &iced_core::Event,
            layout: iced_core::Layout,
            cursor: iced_core::mouse::Cursor,
            renderer: &R,
            shell: &mut iced_core::Shell<'_, Message>,
            viewport: &iced_core::Rectangle,
        ) {
            self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        }
        $crate::wrap::forward!($($rest)*);
    };
    (mouse_interaction $($rest:tt)*) => {
        fn mouse_interaction(
            &self,
            tree: &iced_core::widget::Tree,
            layout: iced_core::Layout,
            cursor: iced_core::mouse::Cursor,
            viewport: &iced_core::Rectangle,
            renderer: &R,
        ) -> iced_core::mouse::Interaction {
            self.content.mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }
        $crate::wrap::forward!($($rest)*);
    };
    (draw $($rest:tt)*) => {
        fn draw(
            &self,
            tree: &iced_core::widget::Tree,
            renderer: &mut R,
            theme: &$crate::Theme,
            style: &iced_core::renderer::Style,
            layout: iced_core::Layout,
            cursor: iced_core::mouse::Cursor,
            viewport: &iced_core::Rectangle,
        ) {
            self.content.draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
        }
        $crate::wrap::forward!($($rest)*);
    };
    (overlay $($rest:tt)*) => {
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut iced_core::widget::Tree,
            layout: iced_core::Layout,
            renderer: &R,
            viewport: &iced_core::Rectangle,
            translation: iced_core::Vector,
            window: iced_core::Size,
        ) -> Vec<iced_core::overlay::Element<'b, Message, $crate::Theme, R>> {
            self.content.overlay(&mut tree.children[0], layout, renderer, viewport, translation, window)
        }
        $crate::wrap::forward!($($rest)*);
    };
}

pub(crate) use forward;

use iced_core::widget::Operation;
use iced_core::widget::operation::{Focusable, Outcome, Scrollable};
use iced_core::{Rectangle, Size, Vector, widget::Id};

/// What an operation is not shown of a wrapped widget: the text it holds, or that it can be focused.
/// It forwards everything else, to every depth, so a wrapper hides one thing and no more.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hide {
    /// Widgets that hold text (`text_input`, `text`) are not reported.
    pub text: bool,
    /// Focusable widgets are not reported, so the Tab key passes over them.
    pub focus: bool,
    /// An empty field reports no text: iced's field reports its placeholder as its text while it holds none.
    pub placeholder: bool,
}

/// A field's state that says it holds no text, and forwards everything else.
struct Blank<'a>(&'a mut dyn iced_core::widget::operation::TextInput);

impl iced_core::widget::operation::TextInput for Blank<'_> {
    fn text(&self) -> iced_core::text::Fragment<'_> {
        iced_core::text::Fragment::Borrowed("")
    }

    fn move_cursor_to_front(&mut self) {
        self.0.move_cursor_to_front();
    }

    fn move_cursor_to_end(&mut self) {
        self.0.move_cursor_to_end();
    }

    fn move_cursor_to(&mut self, location: iced_core::text::Position) {
        self.0.move_cursor_to(location);
    }

    fn select_all(&mut self) {
        self.0.select_all();
    }

    fn select_range(&mut self, start: iced_core::text::Position, end: iced_core::text::Position) {
        self.0.select_range(start, end);
    }
}

pub(crate) struct Filtered<'a> {
    pub inner: &'a mut dyn Operation,
    pub hide: Hide,
}

impl Operation for Filtered<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let hide = self.hide;
        self.inner.traverse(&mut |inner| operate(&mut Filtered { inner, hide }));
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle, viewport: &Rectangle) {
        self.inner.container(id, bounds, viewport);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Size,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        self.inner.scrollable(id, bounds, content, translation, state);
    }

    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if !self.hide.focus {
            self.inner.focusable(id, bounds, state);
        }
    }

    fn text_input(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn iced_core::widget::operation::TextInput,
    ) {
        match (self.hide.text, self.hide.placeholder) {
            (true, _) => {}
            (false, true) => self.inner.text_input(id, bounds, &mut Blank(state)),
            (false, false) => self.inner.text_input(id, bounds, state),
        }
    }

    fn text(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn iced_core::widget::operation::Text) {
        if !self.hide.text {
            self.inner.text(id, bounds, state);
        }
    }

    fn custom(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        self.inner.custom(id, bounds, state);
    }

    fn finish(&self) -> Outcome<()> {
        self.inner.finish()
    }
}
