//! One turn of a window's interface: building it, giving it events, drawing it and handing a screen reader the tree.

use iced_wgpu::Renderer;
use iced_winit::core::theme::Base;
use iced_winit::core::time::Instant;
use iced_winit::core::{self, Element, Event, Size, mouse, renderer, shell, window};
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;

use crate::asks::Said;

/// What one turn of a window's interface is for.
#[derive(Clone, Copy, Debug)]
pub(super) enum Pass {
    /// What happened since the last turn, given at this time between frames: nothing is drawn.
    Between(Instant),
    /// A frame at this time: what happened since the last turn, then the frame's own redraw, then the drawing.
    Frame(Instant),
}

impl Pass {
    /// When the turn is taken.
    pub(super) fn at(self) -> Instant {
        match self {
            Pass::Between(at) | Pass::Frame(at) => at,
        }
    }
}

/// What the last frame showed the widgets: when it was drawn, and where the pointer was.
#[derive(Clone, Copy, Debug)]
pub(super) struct Shown {
    pub(super) at: Instant,
    pub(super) cursor: mouse::Cursor,
}

/// What the updates of one turn came to.
pub(super) struct Interacted<M> {
    /// What the last frame's redraw, given again before the events between frames, said; nothing for a frame.
    pub(super) primed: Said,
    /// The update with the events.
    pub(super) events: user_interface::State,
    /// For a frame, when it was drawn and the update with its own redraw, after the events.
    pub(super) frame: Option<(Instant, user_interface::State)>,
    /// What the widgets published, in order.
    pub(super) published: Vec<M>,
    /// Of each event, whether a widget took it.
    pub(super) statuses: Vec<core::event::Status>,
}

/// Builds the interface for `element`, gives it `events` and, for a frame, the frame's redraw, draws it then, and
/// says what each update said and what the widgets published.
pub(super) fn interact<M, T: Base + 'static>(
    element: Element<'_, M, T, Renderer>,
    theme: &T,
    turn: &mut Turn<'_>,
    events: &[Event],
    pass: Pass,
) -> Interacted<M> {
    let mut interface = UserInterface::build(element, turn.size, std::mem::take(turn.cache), turn.renderer);
    if let Some(at) = turn.focus_at.take() {
        interface.operate(turn.renderer, &mut ui::access::FocusAt::new(at));
    }
    let waker = shell::Waker::noop();
    let primed = match (pass, turn.shown) {
        // Built afresh, a stock widget forgets what it last drew: the last frame's redraw is given again (what it
        // publishes was published then), so it asks for a frame only when the events change its look.
        (Pass::Between(_), Some(shown)) => {
            let redraw = [redraw_at(shown.at)];
            let mut dropped = shell::Bus::new();
            let (state, _) = interface.update(turn.window, &waker, &redraw, shown.cursor, turn.renderer, &mut dropped);
            Said::of(&state)
        }
        (Pass::Between(_), None) | (Pass::Frame(_), _) => Said::NOTHING,
    };
    let mut published = shell::Bus::new();
    let (after_events, statuses) =
        interface.update(turn.window, &waker, events, turn.cursor, turn.renderer, &mut published);
    let frame = match pass {
        Pass::Between(_) => None,
        Pass::Frame(at) => {
            let redraw = [redraw_at(at)];
            let (state, _) = interface.update(turn.window, &waker, &redraw, turn.cursor, turn.renderer, &mut published);
            let style = renderer::Style { text_color: theme.base().text_color };
            interface.draw(turn.renderer, theme, &style, turn.cursor);
            // A screen reader is handed what the frame shows; while none reads the window, nothing is built.
            let (size, scale, title, renderer) = (turn.size, turn.scale, turn.title, &*turn.renderer);
            turn.access.update(|| {
                let title = design::Label::new(title).ok();
                ui::access::tree_with_controls(size, scale, title.as_ref(), |operation| {
                    interface.operate(renderer, operation);
                })
            });
            Some((at, state))
        }
    };
    *turn.cache = interface.into_cache();
    Interacted { primed, events: after_events, frame, published: published.into_iter().collect(), statuses }
}

/// The window's redraw at `at`, as a widget hears it.
fn redraw_at(at: Instant) -> Event {
    Event::Window(window::Event::RedrawRequested(at))
}

/// What one turn of the loop needs of the window, borrowed field by field so the view may keep borrowing the scene.
pub(super) struct Turn<'r> {
    pub(super) renderer: &'r mut Renderer,
    pub(super) cache: &'r mut user_interface::Cache,
    pub(super) window: &'r winit::window::Window,
    pub(super) size: Size,
    /// Physical pixels to one of `size`'s.
    pub(super) scale: f32,
    pub(super) cursor: mouse::Cursor,
    pub(super) access: &'r mut crate::access::Access,
    /// What the window is named, for a screen reader.
    pub(super) title: &'r str,
    /// The focusable a screen reader asked to focus, taken before the events are given.
    pub(super) focus_at: Option<usize>,
    /// What the last frame showed the widgets, none before the first.
    pub(super) shown: Option<Shown>,
}

/// What one turn of a pane's interface came to.
pub(super) struct Turned<M> {
    /// What its widgets published.
    pub(super) published: Vec<M>,
    /// What iced said of the turn's updates, as one.
    pub(super) said: Said,
    /// A press of the main button that no widget took.
    pub(super) pressed_through: bool,
}
