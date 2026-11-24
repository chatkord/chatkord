//! The window host's decisions, as values in and values out: no window, no clock, no system.

use design::{
    Appearance, MotionPreference, POP_OUT_GRIP, POP_OUT_LEAST, POP_OUT_MOST, Preference, Scheme, WINDOW_LEAST, Zoom,
};
use iced_core::time::Instant;
use iced_core::window::RedrawRequest;
use iced_core::{Point, Size, mouse};
use std::time::Duration;
use ui::host::{Acquired, Edge, Event, Grip, Host, Mend, Out, grip, min_window, pop_out_size};

fn host() -> Host {
    Host::new(Size::new(800, 600), 1.0, Appearance::default())
}

fn light() -> Appearance {
    Appearance { scheme: Some(Scheme::Light), motion: MotionPreference::Full }
}

/// A pointer that leaves the window is nowhere, so nothing under it stays hovered.
#[test]
fn a_pointer_that_leaves_the_window_is_nowhere_and_the_window_is_drawn_again() {
    let mut host = host();
    host.on(Event::CursorMoved { x: 100.0, y: 50.0 });
    assert_eq!(host.cursor(), mouse::Cursor::Available(Point::new(100.0, 50.0)));
    let out = host.on(Event::CursorLeft);
    assert_eq!(host.cursor(), mouse::Cursor::Unavailable);
    assert!(out.redraw, "what was hovered must be drawn unhovered");
}

/// A change of scale rebuilds the viewport, and every pointer position after it is in the new scale.
#[test]
fn a_change_of_scale_rebuilds_the_viewport_and_rescales_the_pointer() {
    let mut host = host();
    host.on(Event::CursorMoved { x: 100.0, y: 50.0 });
    let out = host.on(Event::ScaleFactorChanged { scale_factor: 2.0 });
    assert!(out.viewport && out.redraw);
    assert_eq!(host.scale_factor(), 2.0);
    host.on(Event::CursorMoved { x: 100.0, y: 50.0 });
    assert_eq!(host.cursor(), mouse::Cursor::Available(Point::new(50.0, 25.0)));
}

/// The zoom rides on the monitor's scale, and a move to a monitor of another scale keeps it.
#[test]
fn a_zoom_multiplies_the_monitors_scale_and_a_move_to_another_monitor_keeps_it() {
    let mut host = host();
    let zoom = Zoom::of_percent(150).unwrap();
    let out = host.on(Event::Zoom(zoom));
    assert!(out.viewport && out.redraw);
    assert_eq!(
        host.on(Event::Zoom(zoom)),
        Out { viewport: false, scheme: None, surface: Mend::Keep, redraw: false, full_screen: None },
        "no change"
    );
    let out = host.on(Event::ScaleFactorChanged { scale_factor: 1.6 });
    assert!(out.viewport);
    assert_eq!(host.zoom(), zoom, "the move keeps the zoom");
    assert!((host.scale().total() - 2.4).abs() < 1e-6, "{:?}", host.scale());
    host.on(Event::CursorMoved { x: 300.0, y: 120.0 });
    let mouse::Cursor::Available(at) = host.cursor() else { panic!("the pointer is in the window") };
    assert!((at.x - 125.0).abs() < 1e-3 && (at.y - 50.0).abs() < 1e-3, "{at:?}");
}

#[test]
fn the_least_window_grows_with_the_zoom() {
    let mut host = host();
    assert_eq!(host.least(), min_window());
    host.on(Event::Zoom(Zoom::of_percent(200).unwrap()));
    assert_eq!(host.least(), Size::new(WINDOW_LEAST.0 * 2.0, WINDOW_LEAST.1 * 2.0));
}

#[test]
fn a_resize_rebuilds_the_viewport_at_the_new_size() {
    let mut host = host();
    let out = host.on(Event::Resized { width: 1024, height: 768 });
    assert!(out.viewport);
    assert_eq!(host.size(), Size::new(1024, 768));
}

/// The scheme follows the system, the person's choice wins, and an unchanged scheme draws nothing.
#[test]
fn the_scheme_follows_the_system_and_the_persons_choice_wins() {
    let mut host = host();
    assert_eq!(host.scheme(), Scheme::Dark, "nothing said: dark");
    let out = host.on(Event::System(light()));
    assert_eq!((out.scheme, out.redraw), (Some(Scheme::Light), true));
    assert_eq!(
        host.on(Event::System(light())),
        Out { viewport: false, scheme: None, surface: Mend::Keep, redraw: false, full_screen: None },
        "no change, no redraw"
    );
    let out = host.on(Event::Preference(Preference::Dark));
    assert_eq!(out.scheme, Some(Scheme::Dark));
    host.on(Event::System(Appearance { scheme: Some(Scheme::Light), ..light() }));
    assert_eq!(host.scheme(), Scheme::Dark, "the person's choice outlasts the system's");
    assert_eq!(host.on(Event::Preference(Preference::System)).scheme, Some(Scheme::Light));
}

#[test]
fn reduced_motion_is_the_systems_to_say() {
    let mut host = host();
    assert_eq!(host.motion(), MotionPreference::Full);
    host.on(Event::System(Appearance { scheme: None, motion: MotionPreference::Reduced }));
    assert_eq!(host.motion(), MotionPreference::Reduced);
}

/// What iced asks for after an update is what the loop does: a redraw now, a wake at a time, or sleep.
#[test]
fn iceds_request_for_a_later_redraw_wakes_the_loop_then_and_not_before() {
    let mut host = host();
    let now = Instant::now();
    let later = now + Duration::from_millis(500);
    assert!(!host.on(Event::Updated { redraw: RedrawRequest::At(later) }).redraw);
    assert_eq!(host.due(), Some(later));
    assert!(!host.on(Event::Woke { now }).redraw);
    assert_eq!(host.due(), Some(later), "woken early: wait the rest");
    assert!(host.on(Event::Woke { now: later }).redraw);
    assert!(!host.on(Event::Woke { now: later }).redraw, "once");
    assert_eq!(host.due(), None);
}

/// The frame iced asked a later one from is itself drawn and presented before the loop sleeps, and so is whatever
/// else the window answers meanwhile: none of it forgets the wake, or a caret would stop blinking and a tooltip never
/// open in a window nothing else happens to.
#[test]
fn a_wake_asked_for_outlasts_the_frame_drawn_and_every_answer_that_says_nothing() {
    let mut host = host();
    let now = Instant::now();
    let later = now + Duration::from_millis(500);
    host.on(Event::Updated { redraw: RedrawRequest::At(later) });
    for said_nothing in [
        Event::Acquired { acquired: Acquired::Drawn, now },
        Event::Acquired { acquired: Acquired::Suboptimal, now },
        Event::CursorMoved { x: 4.0, y: 4.0 },
        Event::FullScreen(false),
        Event::Fullness(false),
        Event::Zoom(host.zoom()),
        Event::Preference(host.preference()),
    ] {
        let out = host.on(said_nothing);
        assert!(!out.redraw, "{said_nothing:?}");
        assert_eq!(host.due(), Some(later), "forgotten after {said_nothing:?}");
    }
    assert!(host.on(Event::Woke { now: later }).redraw, "and it is drawn when due");
}

#[test]
fn a_request_for_the_next_frame_draws_and_a_request_for_none_sleeps() {
    let mut host = host();
    let later = Instant::now() + Duration::from_secs(1);
    host.on(Event::Updated { redraw: RedrawRequest::At(later) });
    assert!(host.on(Event::Updated { redraw: RedrawRequest::NextFrame }).redraw);
    assert_eq!(host.due(), None, "the pending wake was replaced");
    assert!(!host.on(Event::Woke { now: later }).redraw);
    host.on(Event::Updated { redraw: RedrawRequest::At(later) });
    assert!(!host.on(Event::Updated { redraw: RedrawRequest::Wait }).redraw);
    assert_eq!(host.due(), None);
    assert!(!host.on(Event::Woke { now: later }).redraw, "nothing is due");
}

/// A surface lost or outdated is mended and asked for again after a wait that doubles up to a second: answered only
/// with another redraw, it would spin the window on a core.
#[test]
fn a_surface_that_was_not_given_is_mended_and_asked_again_later_and_later() {
    let mut host = host();
    let now = Instant::now();
    let lost = |host: &mut Host| host.on(Event::Acquired { acquired: Acquired::Lost, now });
    let first = lost(&mut host);
    assert_eq!((first.surface, first.redraw), (Mend::Recreate, true), "mended and asked again at once");
    let waits: Vec<Option<Instant>> = (0..6)
        .map(|_| {
            assert!(!lost(&mut host).redraw);
            host.due()
        })
        .collect();
    let after = |ms: u64| Some(now + Duration::from_millis(ms));
    assert_eq!(waits, [after(100), after(200), after(400), after(800), after(1000), after(1000)]);
    assert!(host.on(Event::Woke { now: now + Duration::from_millis(1000) }).redraw, "asked again");
    let drawn = host.on(Event::Acquired { acquired: Acquired::Drawn, now });
    assert_eq!((drawn.surface, drawn.redraw, host.due()), (Mend::Keep, false, None));
    assert!(host.on(Event::Acquired { acquired: Acquired::Outdated, now }).redraw, "a drawn frame resets it");
}

#[test]
fn each_answer_of_the_surface_has_its_own_mend() {
    let now = Instant::now();
    let mend = |acquired| host().on(Event::Acquired { acquired, now }).surface;
    assert_eq!(mend(Acquired::Drawn), Mend::Keep);
    assert_eq!(mend(Acquired::Suboptimal), Mend::Configure, "drawn, then fitted to the window");
    assert_eq!(mend(Acquired::Outdated), Mend::Configure);
    assert_eq!(mend(Acquired::Lost), Mend::Recreate);
    for acquired in [Acquired::Timeout, Acquired::Occluded, Acquired::Invalid] {
        assert_eq!(mend(acquired), Mend::Keep, "{acquired:?}");
    }
    let suboptimal = host().on(Event::Acquired { acquired: Acquired::Suboptimal, now });
    assert!(!suboptimal.redraw, "its frame was drawn: nothing more is asked");
}

#[test]
fn a_hidden_window_asks_for_no_frame_until_it_shows() {
    let mut host = host();
    let now = Instant::now();
    assert!(!host.on(Event::Occluded(true)).redraw);
    assert!(!host.on(Event::Updated { redraw: RedrawRequest::NextFrame }).redraw);
    assert!(!host.on(Event::Updated { redraw: RedrawRequest::At(now) }).redraw);
    assert_eq!(host.due(), None, "not even later");
    assert!(!host.on(Event::Acquired { acquired: Acquired::Timeout, now }).redraw);
    assert_eq!(host.due(), None);
    assert!(host.on(Event::Occluded(false)).redraw, "drawn as it shows");
    let none = host.on(Event::Resized { width: 0, height: 0 });
    assert_eq!((none.viewport, none.redraw), (false, false), "no area: no surface to fit, nothing to draw");
    let back = host.on(Event::Resized { width: 800, height: 600 });
    assert_eq!((back.viewport, back.redraw), (true, true));
}

#[test]
fn the_window_has_a_least_size_from_the_design_scale() {
    assert_eq!(min_window(), Size::new(WINDOW_LEAST.0, WINDOW_LEAST.1));
}

#[test]
fn the_input_method_is_told_only_what_changed() {
    use iced_core::Rectangle;
    use iced_core::input_method::Purpose;
    use ui::host::{InputMethodPlace, Tell};
    let caret = Rectangle::new(Point::new(10.0, 20.0), Size::new(1.0, 18.0));
    let further = Rectangle { x: 18.0, ..caret };
    let mut place = InputMethodPlace::default();
    assert_eq!(place.asked(true, None), Tell::default(), "nothing was ever allowed");
    assert_eq!(
        place.asked(true, Some((caret, Purpose::Normal))),
        Tell { allowed: Some(true), place: Some((caret, Purpose::Normal)) }
    );
    assert_eq!(
        place.asked(true, Some((caret, Purpose::Normal))),
        Tell::default(),
        "every frame asks again: nothing new"
    );
    assert_eq!(
        place.asked(true, Some((further, Purpose::Normal))),
        Tell { allowed: None, place: Some((further, Purpose::Normal)) }
    );
    assert_eq!(place.asked(true, None), Tell { allowed: Some(false), place: None });
    assert_eq!(
        place.asked(true, Some((further, Purpose::Normal))).allowed,
        Some(true),
        "allowed again on the next field"
    );
}

#[test]
fn an_update_that_drew_nothing_leaves_the_input_method_as_it_was() {
    use iced_core::Rectangle;
    use iced_core::input_method::Purpose;
    use ui::host::{InputMethodPlace, Tell};
    // A key press is an update that draws nothing, and iced's fields ask for an input method only as they are
    // drawn: switching it off there would end the composing at every key.
    let caret = Rectangle::new(Point::new(10.0, 20.0), Size::new(1.0, 18.0));
    let mut place = InputMethodPlace::default();
    assert_eq!(place.asked(true, Some((caret, Purpose::Normal))).allowed, Some(true));
    assert_eq!(place.asked(false, None), Tell::default(), "a key press");
    assert_eq!(place.asked(true, Some((caret, Purpose::Normal))), Tell::default(), "the next frame: nothing changed");
}

#[test]
fn the_window_goes_full_screen_when_asked_and_back_when_not() {
    use ui::host::FullScreen;
    let mut host = host();
    let told = |out: Out| out.full_screen;
    assert_eq!(told(host.on(Event::FullScreen(true))), Some(FullScreen::Enter));
    assert!(host.full_screen());
    assert_eq!(told(host.on(Event::FullScreen(true))), None, "asked once");
    assert_eq!(told(host.on(Event::Fullness(true))), None, "the system made it so");
    assert_eq!(told(host.on(Event::FullScreen(false))), Some(FullScreen::Leave));
    assert!(!host.full_screen());
    assert_eq!(told(host.on(Event::FullScreen(false))), None, "nothing to leave");
    assert_eq!(told(host.on(Event::Fullness(false))), None, "the system gave it back as asked");
}

#[test]
fn a_full_screen_the_system_ended_is_told_once_and_not_before_it_was_shown() {
    use ui::host::FullScreen;
    let mut host = host();
    host.on(Event::FullScreen(true));
    assert_eq!(host.on(Event::Fullness(false)).full_screen, None, "a resize before the system made it full");
    assert!(host.full_screen(), "still asked");
    host.on(Event::Fullness(true));
    assert_eq!(host.on(Event::Fullness(false)).full_screen, Some(FullScreen::Left), "the system ended it");
    assert!(!host.full_screen());
    assert_eq!(host.on(Event::Fullness(false)).full_screen, None, "told once");
    assert_eq!(host.on(Event::FullScreen(true)).full_screen, Some(FullScreen::Enter), "and it can be asked again");
}

#[test]
fn enter_after_the_system_ended_a_full_screen_asks_again_each_time() {
    use ui::host::FullScreen;
    let mut host = host();
    for _ in 0..2 {
        assert_eq!(host.on(Event::FullScreen(true)).full_screen, Some(FullScreen::Enter));
        host.on(Event::Fullness(true));
        assert_eq!(host.on(Event::Fullness(false)).full_screen, Some(FullScreen::Left), "given back by the system");
        assert!(!host.full_screen());
    }
}

/// A screen's own window opens at a quarter of the display's width, 16:9, never smaller or larger than its bounds.
#[test]
fn a_screens_own_window_opens_at_a_quarter_of_the_display_within_its_bounds() {
    assert_eq!(pop_out_size(1920.0), Size::new(480.0, 270.0));
    assert_eq!(pop_out_size(480.0), Size::new(POP_OUT_LEAST.0, POP_OUT_LEAST.1), "a narrow display: its least");
    assert_eq!(pop_out_size(7680.0), Size::new(POP_OUT_MOST.0, POP_OUT_MOST.1), "a very wide one: its most");
}

/// In a window with no frame, its edge resizes it and the rest of it moves it.
#[test]
fn a_frameless_window_is_resized_from_its_edge_and_moved_from_inside() {
    let size = Size::new(320.0, 180.0);
    let inner = POP_OUT_GRIP;
    let at = |x, y| grip(Point::new(x, y), size);
    assert_eq!(at(160.0, 90.0), Grip::Move);
    assert_eq!(at(inner, inner), Grip::Move, "the grip ends where the inside begins");
    assert_eq!(at(0.0, 0.0), Grip::Resize(Edge::NorthWest));
    assert_eq!(at(319.0, 0.0), Grip::Resize(Edge::NorthEast));
    assert_eq!(at(0.0, 179.0), Grip::Resize(Edge::SouthWest));
    assert_eq!(at(319.0, 179.0), Grip::Resize(Edge::SouthEast));
    assert_eq!(at(160.0, 0.0), Grip::Resize(Edge::North));
    assert_eq!(at(160.0, 179.0), Grip::Resize(Edge::South));
    assert_eq!(at(0.0, 90.0), Grip::Resize(Edge::West));
    assert_eq!(at(319.0, 90.0), Grip::Resize(Edge::East));
    assert_eq!(at(320.0 - inner - 0.5, 90.0), Grip::Move, "just inside the right grip");
    let narrow = |x, y| grip(Point::new(x, y), Size::new(inner, inner));
    assert_eq!(narrow(inner - 1.0, inner - 1.0), Grip::Resize(Edge::NorthWest), "the left and the top first");
}

/// A window that may be smaller than the main one keeps its own least, zoomed as the main one's is.
#[test]
fn a_window_with_a_least_of_its_own_keeps_it_and_zooms_it() {
    let least = Size::new(POP_OUT_LEAST.0, POP_OUT_LEAST.1);
    let mut host = host().with_least(least);
    assert_eq!(host.least(), least);
    host.on(Event::Zoom(Zoom::of_percent(200).unwrap()));
    assert_eq!(host.least(), least * 2.0);
}
