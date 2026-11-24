//! How a window is asked to draw: whole, with a new layout, or only to show a new picture in the layout it has; when
//! what is queued for its interface is turned; and what a turn of it asks for.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced_wgpu::graphics::Viewport;
use iced_winit::core::time::Instant;
use iced_winit::core::window::RedrawRequest;
use iced_winit::core::{Event, Size, mouse, touch};
use iced_winit::runtime::user_interface::{self, Change};
use iced_winit::winit::window::Window;

/// What a frame was drawn at: the viewport's pixels and its scale, the zoom included.
pub(crate) type Look = (Size<u32>, f32);

/// The look `viewport` draws at.
pub(crate) fn look(viewport: &Viewport) -> Look {
    (viewport.physical_size(), viewport.scale_factor())
}

/// How much of a window one redraw does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Redraw {
    /// The interface is laid out, updated and drawn again.
    Whole,
    /// The layout it has is presented again, and the pictures in it are taken anew.
    Picture,
}

impl Redraw {
    /// What a redraw does: whole when something asked for a new layout since the last, when no whole draw is held (a
    /// new pane, a renderer made again) or when the viewport is not the one it was drawn at; else only the pictures.
    pub(crate) fn of(stale: bool, drawn: Option<Look>, now: Look) -> Redraw {
        if stale || drawn != Some(now) { Redraw::Whole } else { Redraw::Picture }
    }
}

/// Nanoseconds a millihertz lasts.
const NANOS_PER_MILLIHERTZ: u64 = 1_000_000_000_000;

/// The frame interval taken where the monitor does not say its rate: a 60 Hz panel's.
pub(crate) const FRAME_FALLBACK: Duration = Duration::from_nanos(NANOS_PER_MILLIHERTZ / 60_000);

/// One frame of a monitor refreshing at `millihertz`, or [`FRAME_FALLBACK`] where its rate is unknown.
pub(crate) fn frame_interval(millihertz: Option<u32>) -> Duration {
    millihertz
        .filter(|rate| *rate > 0)
        .map_or(FRAME_FALLBACK, |rate| Duration::from_nanos(NANOS_PER_MILLIHERTZ / u64::from(rate)))
}

/// Whether `event` may wait with the others queued for the next frame interval: a pointer's, a wheel's or a finger's
/// move, which comes hundreds of times a second and shows only on a frame. A press, a key, an input method's text, a
/// focus or the window's own event is turned at once.
pub(crate) fn may_wait(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::WheelScrolled { .. })
            | Event::Touch(touch::Event::FingerMoved { .. })
    )
}

/// When what a window queued for its interface is turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Due {
    /// Nothing is queued.
    Idle,
    /// Now.
    Now,
    /// At this time, one frame interval after the last turn.
    At(Instant),
}

/// When `events`, queued since the window's last turn at `last` (none before the first), are turned at `now`, with
/// `asked` true when a screen reader asked something: moves alone wait out one `interval` since the last turn, so a
/// pointer costs a turn a frame however often it reports; anything else is turned at once.
pub(crate) fn due(events: &[Event], asked: bool, last: Option<Instant>, interval: Duration, now: Instant) -> Due {
    if events.is_empty() && !asked {
        return Due::Idle;
    }
    let next = last.map(|last| last + interval).filter(|next| *next > now);
    match next {
        Some(next) if !asked && events.iter().all(may_wait) => Due::At(next),
        Some(_) | None => Due::Now,
    }
}

/// What iced said of a window's interface after one update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Said {
    /// Its widgets must be built again.
    Outdated,
    /// It holds: what of it was made again, and when it asks to be drawn.
    Updated {
        /// Its layout or its overlays made again, or nothing.
        change: Change,
        /// When a widget asks for a frame: the next, at a time, or none.
        redraw: RedrawRequest,
    },
}

impl Said {
    /// An update that changed nothing and asks for nothing.
    pub(crate) const NOTHING: Said = Said::Updated { change: Change::None, redraw: RedrawRequest::Wait };

    /// What `state` says.
    pub(crate) fn of(state: &user_interface::State) -> Said {
        match state {
            user_interface::State::Outdated => Said::Outdated,
            user_interface::State::Updated { change, redraw_request, .. } => {
                Said::Updated { change: *change, redraw: *redraw_request }
            }
        }
    }

    /// This update and a later one of the same interface, as one: the most made again, the soonest asked.
    pub(crate) fn and(self, later: Said) -> Said {
        let (Said::Updated { change, redraw }, Said::Updated { change: then, redraw: asked }) = (self, later) else {
            return Said::Outdated;
        };
        let change = match (change, then) {
            (Change::Layout, _) | (_, Change::Layout) => Change::Layout,
            (Change::Overlay, _) | (_, Change::Overlay) => Change::Overlay,
            (Change::None, Change::None) => Change::None,
        };
        Said::Updated { change, redraw: redraw.min(asked) }
    }

    /// What is left of this update once a frame drawn after it shows it: a rebuild, or a time a widget asked for.
    pub(crate) fn answered(self) -> Said {
        match self {
            Said::Outdated => Said::Outdated,
            Said::Updated { redraw: RedrawRequest::At(at), .. } => {
                Said::Updated { change: Change::None, redraw: RedrawRequest::At(at) }
            }
            Said::Updated { redraw: RedrawRequest::NextFrame | RedrawRequest::Wait, .. } => Said::NOTHING,
        }
    }
}

/// What one turn of a window's interface came to: what iced said of it, and whether what its widgets published
/// changed the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// What iced said of the turn's updates, as one.
    pub(crate) said: Said,
    /// The scene has to be drawn again.
    pub(crate) changed: bool,
}

impl Outcome {
    /// The frame the turn asks for: a whole one (`NextFrame`) when the scene changed, the widgets must be built
    /// again, their layout or overlays were made again, or a widget asked for it; one at the time a widget asked
    /// for; else none (`Wait`).
    pub(crate) fn ask(self) -> RedrawRequest {
        match self.said {
            _ if self.changed => RedrawRequest::NextFrame,
            Said::Updated { change: Change::None, redraw } => redraw,
            Said::Outdated | Said::Updated { change: Change::Layout | Change::Overlay, .. } => RedrawRequest::NextFrame,
        }
    }
}

/// A window another thread may ask to be redrawn, while there is one, and since when such an ask went unanswered.
#[derive(Default)]
pub(crate) struct Asking {
    window: Mutex<Option<Arc<Window>>>,
    since: Mutex<Option<Instant>>,
    /// A whole redraw was asked since one was last given.
    stale: AtomicBool,
}

impl Asking {
    /// From now on asks `window`, with no ask outstanding.
    pub(crate) fn open(&self, window: Arc<Window>) {
        if let Ok(mut slot) = self.window.lock() {
            *slot = Some(window);
        }
        self.answered();
    }

    /// From now on asks no window: it is gone.
    pub(crate) fn close(&self) {
        if let Ok(mut slot) = self.window.lock() {
            *slot = None;
        }
    }

    /// Asks for a redraw that lays the interface out again: something it shows changed.
    pub(crate) fn whole(&self) {
        self.stale.store(true, Ordering::Release);
        self.ask();
    }

    /// Asks for a redraw that shows a new picture where the last one was laid out.
    pub(crate) fn picture(&self) {
        self.ask();
    }

    /// Whether a whole redraw is asked for and not yet given, leaving it asked.
    pub(crate) fn owed(&self) -> bool {
        self.stale.load(Ordering::Acquire)
    }

    /// Whether a whole redraw was asked for since the last time this was taken.
    pub(crate) fn take_stale(&self) -> bool {
        self.stale.swap(false, Ordering::AcqRel)
    }

    /// Every ask is answered: the window was given a frame.
    pub(crate) fn answered(&self) {
        if let Ok(mut since) = self.since.lock() {
            *since = None;
        }
    }

    /// Since when an ask has gone unanswered, if one has.
    pub(crate) fn since(&self) -> Option<Instant> {
        self.since.lock().ok().and_then(|since| *since)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the one place a redraw is asked: whole or picture decides what the redraw does"
    )]
    fn ask(&self) {
        if let Ok(Some(window)) = self.window.lock().as_deref() {
            window.request_redraw();
            if let Ok(mut since) = self.since.lock() {
                since.get_or_insert_with(Instant::now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOK: Look = (Size::new(1280, 720), 1.5);

    #[test]
    fn a_picture_after_a_whole_draw_redraws_only_the_picture() {
        assert_eq!(Redraw::of(false, Some(LOOK), LOOK), Redraw::Picture);
    }

    #[test]
    fn any_whole_ask_since_the_last_draw_makes_the_next_whole() {
        let asking = Asking::default();
        assert!(!asking.take_stale());
        asking.picture();
        assert!(!asking.take_stale(), "a picture is no reason for a layout");
        asking.picture();
        asking.whole();
        asking.picture();
        let stale = asking.take_stale();
        assert_eq!(Redraw::of(stale, Some(LOOK), LOOK), Redraw::Whole);
        assert!(!asking.take_stale(), "taken once, it is answered");
    }

    #[test]
    fn a_new_pane_a_renderer_made_again_or_a_new_viewport_is_drawn_whole() {
        assert_eq!(Redraw::of(false, None, LOOK), Redraw::Whole);
        assert_eq!(Redraw::of(false, Some((Size::new(1281, 720), 1.5)), LOOK), Redraw::Whole);
        assert_eq!(Redraw::of(false, Some((Size::new(1280, 720), 2.0)), LOOK), Redraw::Whole);
    }

    const FRAME: Duration = FRAME_FALLBACK;

    fn moved(x: f32) -> Event {
        Event::Mouse(mouse::Event::CursorMoved { position: iced_winit::core::Point::new(x, 10.0) })
    }

    #[test]
    fn fifty_pointer_moves_inside_one_frame_interval_make_one_turn() {
        let start = Instant::now();
        let (mut last, mut queued, mut turns) = (None, Vec::new(), 0);
        for step in 0..50_u32 {
            let now = start + FRAME * step / 60;
            queued.push(moved(1.0));
            if due(&queued, false, last, FRAME, now) == Due::Now {
                turns += 1;
                last = Some(now);
                queued.clear();
            }
        }
        assert_eq!(turns, 1, "the first move turns, the rest wait for the interval");
        assert_eq!(due(&queued, false, last, FRAME, start + FRAME / 2), Due::At(start + FRAME));
    }

    #[test]
    fn a_move_after_the_interval_turns_at_once() {
        let start = Instant::now();
        assert_eq!(due(&[moved(1.0)], false, None, FRAME, start), Due::Now, "the first ever");
        assert_eq!(due(&[moved(1.0)], false, Some(start), FRAME, start + FRAME), Due::Now);
        assert_eq!(due(&[], false, Some(start), FRAME, start + FRAME), Due::Idle, "nothing queued");
    }

    #[test]
    fn a_key_or_a_screen_readers_ask_is_never_held() {
        let start = Instant::now();
        let key = Event::Keyboard(iced_winit::core::keyboard::Event::KeyPressed {
            key: iced_winit::core::keyboard::Key::Character("a".into()),
            modified_key: iced_winit::core::keyboard::Key::Character("a".into()),
            physical_key: iced_winit::core::keyboard::key::Physical::Unidentified(
                iced_winit::core::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced_winit::core::keyboard::Location::Standard,
            modifiers: iced_winit::core::keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        });
        let soon = start + FRAME / 10;
        assert_eq!(due(&[moved(1.0), key], false, Some(start), FRAME, soon), Due::Now);
        assert_eq!(due(&[], true, Some(start), FRAME, soon), Due::Now);
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert_eq!(due(&[press], false, Some(start), FRAME, soon), Due::Now);
    }

    #[test]
    fn a_frame_lasts_as_long_as_the_monitor_refreshes() {
        assert_eq!(frame_interval(Some(60_000)), Duration::from_nanos(16_666_666));
        assert_eq!(frame_interval(Some(144_000)), Duration::from_nanos(6_944_444));
        assert_eq!(frame_interval(None), FRAME_FALLBACK);
        assert_eq!(frame_interval(Some(0)), FRAME_FALLBACK);
    }

    const MOVED: Said = Said::NOTHING;

    #[test]
    fn a_pointer_move_that_changes_no_look_asks_nothing() {
        let turned = Outcome { said: MOVED.and(MOVED), changed: false };
        assert_eq!(turned.ask(), RedrawRequest::Wait);
    }

    #[test]
    fn a_published_change_asks_a_whole_frame() {
        assert_eq!(Outcome { said: MOVED, changed: true }.ask(), RedrawRequest::NextFrame);
    }

    #[test]
    fn a_widget_asking_for_the_next_frame_or_a_new_layout_asks_a_whole_one() {
        let next = Said::Updated { change: Change::None, redraw: RedrawRequest::NextFrame };
        assert_eq!(Outcome { said: MOVED.and(next), changed: false }.ask(), RedrawRequest::NextFrame);
        for change in [Change::Layout, Change::Overlay] {
            let said = Said::Updated { change, redraw: RedrawRequest::Wait };
            assert_eq!(Outcome { said, changed: false }.ask(), RedrawRequest::NextFrame, "{change:?}");
        }
        assert_eq!(Outcome { said: Said::Outdated, changed: false }.ask(), RedrawRequest::NextFrame);
    }

    #[test]
    fn a_widget_asking_at_a_time_asks_at_that_time() {
        let at = Instant::now();
        let said = Said::Updated { change: Change::None, redraw: RedrawRequest::At(at) };
        assert_eq!(Outcome { said, changed: false }.ask(), RedrawRequest::At(at));
        let later = Said::Updated { change: Change::None, redraw: RedrawRequest::At(at + Duration::from_millis(5)) };
        assert_eq!(later.and(said), said, "the sooner of two times");
    }

    #[test]
    fn a_frame_answers_what_its_events_asked_of_it_but_a_time_or_a_rebuild() {
        let next = Said::Updated { change: Change::Layout, redraw: RedrawRequest::NextFrame };
        assert_eq!(next.answered(), Said::NOTHING);
        let at = Instant::now();
        let timed = Said::Updated { change: Change::Overlay, redraw: RedrawRequest::At(at) };
        assert_eq!(timed.answered(), Said::Updated { change: Change::None, redraw: RedrawRequest::At(at) });
        assert_eq!(Said::Outdated.answered(), Said::Outdated);
        assert_eq!(MOVED.and(Said::Outdated), Said::Outdated, "a rebuild is never merged away");
    }
}
