//! How much of a watched picture shows, and which part: from the whole picture up to five times it, always covering
//! the place the whole picture fits. Plain arithmetic, so it is decided here and checked without a window.

use crate::call::fit::Placed;

/// The whole picture, in tenths of its size.
const WHOLE_TENTHS: u8 = 10;
/// The most a picture is zoomed, in tenths of its size.
const MOST_TENTHS: u8 = 50;

/// A zoom on a picture: how large it is drawn, and which point of it sits at the centre of the place it fits whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    tenths: u8,
    /// A point of the picture, each coordinate a share of its side (0 to 1).
    centre: (f32, f32),
}

/// Where a zoomed picture is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    /// Left, top, width and height in the area's pixels, the area's own origin at (0, 0).
    pub viewport: [f32; 4],
    /// Left, top, width and height of the part of the picture shown there, each a share of the picture's side.
    pub window: [f32; 4],
}

impl Lens {
    /// The whole picture, as it is drawn unzoomed.
    pub const WHOLE: Self = Self { tenths: WHOLE_TENTHS, centre: (0.5, 0.5) };

    /// How large the picture is drawn, in percent of its whole size: 100 to 500.
    pub fn percent(self) -> u16 {
        u16::from(self.tenths) * 10
    }

    fn scale(self) -> f32 {
        f32::from(self.tenths) / f32::from(WHOLE_TENTHS)
    }

    /// Zoomed `notches` tenths in (positive) or out, keeping the point of the picture under `at` where it is. `at` is
    /// a share of the place the whole picture fits, and may lie in the bars beside it; one that is not a number leaves
    /// the lens as it was.
    pub fn wheel(self, notches: i8, at: (f32, f32)) -> Self {
        if !(at.0.is_finite() && at.1.is_finite()) {
            return self;
        }
        let step = notches.unsigned_abs();
        let tenths = if notches >= 0 {
            self.tenths.saturating_add(step).min(MOST_TENTHS)
        } else {
            self.tenths.saturating_sub(step).max(WHOLE_TENTHS)
        };
        let (was, now) = (self.scale(), f32::from(tenths) / f32::from(WHOLE_TENTHS));
        let kept = |centre: f32, at: f32| centre + (at - 0.5) / was - (at - 0.5) / now;
        Self { tenths, ..self }.centred((kept(self.centre.0, at.0), kept(self.centre.1, at.1)))
    }

    /// Moved with a drag of `by`, a share of the place the whole picture fits: the picture follows the pointer. A drag
    /// that is not a number leaves the lens as it was.
    pub fn pan(self, by: (f32, f32)) -> Self {
        if !(by.0.is_finite() && by.1.is_finite()) {
            return self;
        }
        let scale = self.scale();
        self.centred((self.centre.0 - by.0 / scale, self.centre.1 - by.1 / scale))
    }

    /// This lens centred on `centre`, moved back as far as needed for the picture to cover the place it fits whole.
    fn centred(self, centre: (f32, f32)) -> Self {
        let margin = 0.5 / self.scale();
        let kept = |at: f32| at.clamp(margin, 1.0 - margin);
        Self { centre: (kept(centre.0), kept(centre.1)), ..self }
    }

    /// Where the picture `placed` whole in an area `area` large is drawn through this lens, cut to the area: nothing
    /// when none of it would show.
    pub fn seen(self, placed: Placed, (area_width, area_height): (u32, u32)) -> Option<Seen> {
        let scale = self.scale();
        let (width, height) = (placed.width as f32 * scale, placed.height as f32 * scale);
        let left = placed.left as f32 + placed.width as f32 / 2.0 - self.centre.0 * width;
        let top = placed.top as f32 + placed.height as f32 / 2.0 - self.centre.1 * height;
        let (shown_left, shown_top) = (left.max(0.0), top.max(0.0));
        let shown_right = (left + width).min(area_width as f32);
        let shown_bottom = (top + height).min(area_height as f32);
        (shown_right > shown_left && shown_bottom > shown_top).then(|| Seen {
            viewport: [shown_left, shown_top, shown_right - shown_left, shown_bottom - shown_top],
            window: [
                (shown_left - left) / width,
                (shown_top - top) / height,
                (shown_right - shown_left) / width,
                (shown_bottom - shown_top) / height,
            ],
        })
    }
}

/// The least share of a camera's picture a card may show by covering it, as Telegram's group calls keep: three quarters.
pub const COVERED_AT_LEAST: f32 = 0.75;

impl Seen {
    /// A `picture` covering the whole of `area`, its shape kept and the middle of it shown: a camera on a card, cut to
    /// the card rather than barred. Nothing for a picture or an area with no size.
    pub fn covering(
        (picture_width, picture_height): (u32, u32),
        (area_width, area_height): (u32, u32),
    ) -> Option<Seen> {
        if picture_width == 0 || picture_height == 0 || area_width == 0 || area_height == 0 {
            return None;
        }
        let (picture, area) = (picture_width as f32 / picture_height as f32, area_width as f32 / area_height as f32);
        let (width, height) = if picture > area { (area / picture, 1.0) } else { (1.0, picture / area) };
        Some(Seen {
            viewport: [0.0, 0.0, area_width as f32, area_height as f32],
            window: [(1.0 - width) / 2.0, (1.0 - height) / 2.0, width, height],
        })
    }

    /// A camera on a card of `area`: covering the card when that cuts away at most a quarter of the picture; past
    /// that, the whole picture fitted in it on the card's own ground, so no face is cut off. Nothing for a picture or
    /// an area with no size.
    pub fn filling(picture: (u32, u32), area: (u32, u32)) -> Option<Seen> {
        let covering = Seen::covering(picture, area)?;
        let [_, _, width, height] = covering.window;
        if width * height >= COVERED_AT_LEAST {
            return Some(covering);
        }
        crate::fit(picture, area).and_then(|placed| Lens::WHOLE.seen(placed, area))
    }

    /// The same, drawn mirrored left to right: one's own camera, as a mirror shows a face.
    #[must_use]
    pub fn mirrored(self) -> Seen {
        let [left, top, width, height] = self.window;
        Seen { window: [left + width, top, -width, height], ..self }
    }
}

impl Placed {
    /// Where `point`, in the area's pixels, lies on this place, each coordinate a share of its side: outside 0 to 1 in
    /// the bars beside it.
    pub fn share_of(self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.left as f32) / self.width as f32, (y - self.top as f32) / self.height as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::fit::fit;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::harness::{Clock, member, tell, the_call, watching_bo};
    use crate::place;
    use crate::window::Window;
    use std::time::Instant;

    const AREA: (u32, u32) = (800, 800);

    fn placed() -> Placed {
        fit((1600, 900), AREA).expect("a picture fits an area")
    }

    fn zoomed(notches: i8) -> Lens {
        Lens::WHOLE.wheel(notches, (0.5, 0.5))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn a_camera_covers_its_card_up_to_a_quarter_cut_and_is_shown_whole_past_it() {
        let camera = (1280, 720);
        assert_eq!(
            Seen::filling(camera, (640, 400)),
            Seen::covering(camera, (640, 400)),
            "16:10 cuts a tenth: covered"
        );
        let tall = Seen::filling(camera, (400, 400)).unwrap();
        let [left, top, width, height] = tall.viewport;
        assert_eq!((left, width), (0.0, 400.0), "a square would cut 44 %: the picture whole, across the card");
        assert!((height - 225.0).abs() < 1.0 && (top - 87.5).abs() < 1.0, "and in its middle: {tall:?}");
        assert_eq!(tall.window, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(Seen::filling(camera, (0, 400)), None);
    }

    #[test]
    fn a_covering_picture_fills_its_area_and_shows_its_middle() {
        let wide = Seen::covering((1280, 720), (400, 400)).expect("a picture covers an area");
        assert_eq!(wide.viewport, [0.0, 0.0, 400.0, 400.0], "no bars");
        assert!(close(wide.window[0], 0.21875) && close(wide.window[2], 0.5625) && wide.window[3] == 1.0);
        let tall = Seen::covering((720, 1280), (1600, 900)).expect("a picture covers an area");
        assert!(tall.window[2] == 1.0 && close(tall.window[3], 0.316_406_25) && close(tall.window[1], 0.341_796_9));
        assert_eq!(Seen::covering((0, 720), AREA), None);
    }

    #[test]
    fn a_mirrored_picture_reads_the_same_part_right_to_left() {
        let seen = Seen::covering((1280, 720), (400, 400)).expect("covered");
        let mirrored = seen.mirrored();
        assert!(
            close(mirrored.window[0], seen.window[0] + seen.window[2]) && close(mirrored.window[2], -seen.window[2])
        );
        assert_eq!(
            (mirrored.viewport, mirrored.window[1], mirrored.window[3]),
            (seen.viewport, seen.window[1], seen.window[3])
        );
    }

    #[test]
    fn an_unzoomed_lens_shows_the_whole_picture_where_fit_puts_it() {
        let seen = Lens::WHOLE.seen(placed(), AREA).expect("the whole picture shows");
        assert_eq!(seen.viewport, [0.0, 175.0, 800.0, 450.0]);
        assert_eq!(seen.window, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(Lens::WHOLE.percent(), 100);
    }

    #[test]
    fn the_wheel_zooms_in_tenths_from_one_to_five_times() {
        assert_eq!(zoomed(1).percent(), 110);
        assert_eq!(zoomed(40).percent(), 500);
        assert_eq!(zoomed(127).percent(), 500, "never past five times");
        assert_eq!(zoomed(40).wheel(-1, (0.5, 0.5)).percent(), 490);
        assert_eq!(zoomed(3).wheel(-128, (0.5, 0.5)), Lens::WHOLE, "never under the whole picture");
    }

    #[test]
    fn the_point_under_the_pointer_stays_under_it_as_the_wheel_zooms() {
        let at = (0.8, 0.3);
        let before = zoomed(5);
        let after = before.wheel(7, at);
        let under = |lens: Lens| {
            let scale = lens.scale();
            (lens.centre.0 + (at.0 - 0.5) / scale, lens.centre.1 + (at.1 - 0.5) / scale)
        };
        let (was, now) = (under(before), under(after));
        assert!(close(was.0, now.0) && close(was.1, now.1), "{was:?} moved to {now:?}");
    }

    #[test]
    fn a_pan_never_shows_past_the_pictures_edge() {
        let place = placed();
        for by in [(10.0, 0.0), (-10.0, 0.0), (0.0, 10.0), (0.0, -10.0), (7.0, -7.0)] {
            for lens in [zoomed(1), zoomed(15), zoomed(40)] {
                let seen = lens.pan(by).seen(place, AREA).expect("a zoomed picture shows");
                let [left, top, width, height] = seen.viewport;
                assert!(left <= place.left as f32 + 1e-3 && top <= place.top as f32 + 1e-3, "{by:?}: {seen:?}");
                assert!(left + width >= (place.left + place.width) as f32 - 1e-3, "{by:?}: {seen:?}");
                assert!(top + height >= (place.top + place.height) as f32 - 1e-3, "{by:?}: {seen:?}");
                let [u, v, w, h] = seen.window;
                assert!(u >= 0.0 && v >= 0.0 && u + w <= 1.0 + 1e-6 && v + h <= 1.0 + 1e-6, "{by:?}: {seen:?}");
            }
        }
    }

    #[test]
    fn a_pointer_in_the_bars_zooms_toward_it_within_the_edges() {
        let lens = Lens::WHOLE.wheel(20, (0.5, -0.4));
        assert_eq!(lens.percent(), 300);
        assert!(close(lens.centre.1, 1.0 / 6.0), "the top edge, as far as it goes: {lens:?}");
        assert!(close(lens.centre.0, 0.5));
    }

    #[test]
    fn a_zoomed_picture_is_cut_to_its_area_and_its_window_matches_the_cut() {
        let seen = zoomed(10).seen(placed(), AREA).expect("a zoomed picture shows");
        // Twice as large, centred: 1600 × 900 over an 800 × 800 area, so the area's full height and width show its
        // middle half across and 800 of its 900 rows.
        assert_eq!(seen.viewport, [0.0, 0.0, 800.0, 800.0]);
        let [u, v, w, h] = seen.window;
        assert!(close(u, 0.25) && close(w, 0.5), "{seen:?}");
        assert!(close(v, 50.0 / 900.0) && close(h, 800.0 / 900.0), "{seen:?}");
    }

    #[test]
    fn a_point_is_told_as_a_share_of_the_place() {
        assert_eq!(placed().share_of((400.0, 175.0)), (0.5, 0.0));
        assert_eq!(placed().share_of((0.0, 0.0)), (0.0, -175.0 / 450.0));
    }

    #[test]
    fn a_pointer_that_is_not_a_number_leaves_the_lens_as_it_was() {
        let lens = zoomed(10).pan((0.1, 0.1));
        assert_eq!(lens.pan((f32::NAN, 0.0)), lens);
        assert_eq!(lens.wheel(-5, (f32::INFINITY, 0.5)), lens);
    }

    #[test]
    fn zoom_is_only_for_the_screen_shown_large_and_resets_when_that_changes() {
        use crate::Pinned;
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        let zoom = |window: &Window| the_call(window).zoom(&member("bo")).map(crate::Lens::percent);
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        assert_eq!(zoom(&window), None, "a screen in the grid is not shown large");

        call(&mut window, place::CallMessage::Pin(Pinned::Screen(member("bo"))));
        assert!(the_call(&window).zoomable(&member("bo")));
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        assert_eq!(zoom(&window), Some(100), "on, at the whole picture");
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        assert_eq!(zoom(&window), None, "off again");

        let wheel = place::CallMessage::ZoomWheel { notches: 5, at: (400.0, 300.0), area: (800.0, 600.0) };
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        call(&mut window, wheel.clone());
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        assert_eq!(zoom(&window), None, "entering full screen starts from the whole picture");
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        call(&mut window, wheel.clone());
        assert_eq!(window.full_screen().and_then(|full| full.zoom).map(crate::Lens::percent), Some(150));
        call(&mut window, place::CallMessage::LeaveFullScreen);
        assert_eq!(zoom(&window), None, "and leaving it");

        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        call(&mut window, place::CallMessage::Unpin);
        assert_eq!(zoom(&window), None, "unpinned");
        call(&mut window, place::CallMessage::Pin(Pinned::Screen(member("bo"))));
        assert_eq!(zoom(&window), None, "and pinned again, it starts off");
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo };
        call(&mut window, place::CallMessage::Room(stopped));
        assert_eq!(zoom(&window), None, "a share that ended takes its zoom");
    }

    #[test]
    fn the_wheel_zooms_toward_the_pointer_and_a_drag_pans_only_while_zoom_is_on() {
        use crate::Pinned;
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        let lens = |window: &Window| the_call(window).zoom(&member("bo"));
        call(&mut window, place::CallMessage::Pin(Pinned::Screen(member("bo"))));
        let area = (800.0, 450.0);
        call(&mut window, place::CallMessage::ZoomWheel { notches: 3, at: (0.0, 0.0), area });
        call(&mut window, place::CallMessage::Pan { by: (100.0, 0.0), area });
        assert_eq!(lens(&window), None, "nothing while zoom is off");

        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        call(&mut window, place::CallMessage::ZoomWheel { notches: 10, at: (0.0, 0.0), area });
        let corner = crate::Lens::WHOLE.wheel(10, (0.0, 0.0));
        assert_eq!(lens(&window), Some(corner), "the 1920 x 1080 picture fills 800 x 450: the top left corner");
        call(&mut window, place::CallMessage::Pan { by: (-200.0, -90.0), area });
        assert_eq!(lens(&window), Some(corner.pan((-0.25, -0.2))), "a drag moved as a share of the picture's place");
    }
}
