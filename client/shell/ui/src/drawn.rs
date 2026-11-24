//! What a widget last drew of the pointer and the press, kept in its tree so it outlives the interface being built
//! again: an event that changes it asks for a frame, and one that changes nothing asks none.

use iced_core::{Event, Shell, window};

/// The look a widget drew at the last frame, none before its first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Drawn<L>(Option<L>);

impl<L> Default for Drawn<L> {
    fn default() -> Self {
        Self(None)
    }
}

impl<L: Copy + PartialEq> Drawn<L> {
    /// After `event` the widget would draw `now`: a frame's own redraw is what it draws, and any other event that
    /// leaves it unlike the last frame asks for the next.
    pub(crate) fn follow<Message>(&mut self, event: &Event, now: L, shell: &mut Shell<'_, Message>) {
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            self.0 = Some(now);
        } else if self.0.is_some_and(|drawn| drawn != now) {
            shell.request_redraw();
        }
    }
}

/// What a widget asks for once it drew a frame with the pointer at `drawn` and is then built again, as a window's
/// loop builds it each turn, and given `events` with the pointer at `now`.
#[cfg(test)]
pub(crate) fn asked_after<'a, Message: 'a>(
    element: impl Fn() -> crate::testing::Element<'a, Message>,
    size: iced_core::Size,
    drawn: iced_core::Point,
    now: iced_core::Point,
    events: &[Event],
) -> window::RedrawRequest {
    asked_after_held(element, size, &[], drawn, now, events)
}

/// As [`asked_after`], once the widget was first given each of `held`'s events with the pointer at its point, before
/// the frame: a press held and dragged.
#[cfg(test)]
pub(crate) fn asked_after_held<'a, Message: 'a>(
    element: impl Fn() -> crate::testing::Element<'a, Message>,
    size: iced_core::Size,
    held: &[(iced_core::Point, Event)],
    drawn: iced_core::Point,
    now: iced_core::Point,
    events: &[Event],
) -> window::RedrawRequest {
    use iced_core::{mouse, shell, time};
    use iced_runtime::user_interface::{Cache, State, UserInterface};
    crate::testing::hermetic_fonts();
    let mut renderer = crate::testing::Renderer::new(crate::fonts::settings());
    let mut messages = shell::Bus::new();
    let mut interface = UserInterface::build(element(), size, Cache::default(), &mut renderer);
    let redraw = Event::Window(window::Event::RedrawRequested(time::Instant::now()));
    let before = held.iter().map(|(at, event)| (*at, event)).chain(std::iter::once((drawn, &redraw)));
    for (at, event) in before {
        let _ = interface.update(
            &window::Headless,
            &shell::Waker::noop(),
            std::slice::from_ref(event),
            mouse::Cursor::Available(at),
            &mut renderer,
            &mut messages,
        );
    }
    let cache = interface.into_cache();
    let mut interface = UserInterface::build(element(), size, cache, &mut renderer);
    let (state, _) = interface.update(
        &window::Headless,
        &shell::Waker::noop(),
        events,
        mouse::Cursor::Available(now),
        &mut renderer,
        &mut messages,
    );
    match state {
        State::Updated { redraw_request, .. } => redraw_request,
        State::Outdated => window::RedrawRequest::NextFrame,
    }
}
