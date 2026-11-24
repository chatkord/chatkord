//! A voice channel as it is shown: what each person's tile says of them, and how many tiles of what size fit the
//! space there is.

use crate::people::{presence_phrase, swatch_of_text};
use crate::place::Received;
use design::{PersonColour, Presence};
use terms::MemberId;
use words::{Phrase, Words};

/// How a person's connection to the call is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Connection {
    /// In the call: the tile says what else is true of them.
    #[default]
    Connected,
    /// Joining: their connection is not made yet.
    Connecting,
    /// Their connection failed.
    Failed,
    /// Their connection dropped and is being made again.
    Reconnecting,
}

/// A person's camera, as their tile has it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraSeen {
    /// No camera.
    #[default]
    Off,
    /// Sent, but not shown here: the side pane's list outside the call, or a camera this device cannot show.
    Unshown,
    /// Its picture fills the tile.
    Shown,
    /// Its picture fills the tile, held at the last picture: the person paused it.
    Paused,
}

impl CameraSeen {
    /// Whether a picture fills the tile, moving or held.
    pub fn pictured(self) -> bool {
        matches!(self, CameraSeen::Shown | CameraSeen::Paused)
    }
}

/// One person in the call, as the grid shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    /// Who the server knows them as: none for a tile made of a name alone.
    pub user_id: Option<MemberId>,
    /// The name shown on the tile: their display name, else their id.
    pub name: String,
    /// Their colour: the one they picked, else their id's swatch.
    pub colour: PersonColour,
    /// This is the account that looks.
    pub own: bool,
    /// How their connection to the call is; anything but connected is the only thing the tile says.
    pub connection: Connection,
    /// Their microphone was heard just now; a loud level holds it a moment.
    pub speaking: bool,
    /// No sound comes from them: muted, deafened, held or sending no microphone.
    pub muted: bool,
    /// They hear nothing of the call.
    pub deafened: bool,
    /// An administrator muted them for everyone (which shows as muted too).
    pub held: bool,
    /// They are sharing their screen: the tile says it is live, and the screen is a card of its own.
    pub sharing: bool,
    /// Their camera: off, sent but not shown here, or filling the tile.
    pub camera: CameraSeen,
    /// This account is in the channel on another device, so the row offers the seat here.
    pub elsewhere: bool,
}

impl Tile {
    /// Someone with nothing to say of them yet, coloured as an account named `name` would be.
    pub fn new(name: impl Into<String>) -> Tile {
        let name = name.into();
        Tile {
            user_id: None,
            colour: PersonColour::Swatch(swatch_of_text(&name)),
            own: false,
            name,
            connection: Connection::Connected,
            speaking: false,
            muted: false,
            deafened: false,
            held: false,
            sharing: false,
            camera: CameraSeen::Off,
            elsewhere: false,
        }
    }

    /// Whether this is the account that looks.
    pub fn own(&self) -> bool {
        self.own
    }

    /// What the tile says of the person, most important first: a connection not whole comes first, since nothing
    /// else is true until it is. Speaking is a word as well as a ring, never told by colour alone.
    pub fn badges(&self) -> Vec<Presence> {
        let mut said = Vec::new();
        match self.connection {
            Connection::Connecting => return vec![Presence::Connecting],
            Connection::Failed => return vec![Presence::Failed],
            Connection::Reconnecting => return vec![Presence::Reconnecting],
            Connection::Connected => {}
        }
        if self.speaking && !self.muted && !self.deafened {
            said.push(Presence::Speaking);
        }
        if self.held {
            said.push(Presence::MutedByAdmin);
        } else if self.muted {
            said.push(Presence::Muted);
        }
        if self.deafened {
            said.push(Presence::Deafened);
        }
        if self.sharing {
            said.push(Presence::Live);
        }
        if said.is_empty() {
            said.push(Presence::Online);
        }
        said
    }

    /// Whether the ring shows: only for someone heard, which is a connected, unmuted, undeafened speaker.
    pub fn ringed(&self) -> bool {
        self.badges().contains(&Presence::Speaking)
    }

    /// The name and what it says of them, one line for a screen reader: "Ada, Speaking". Online, which everyone in a
    /// call is, is left unsaid.
    pub fn said(&self, words: &Words) -> String {
        let mut line = self.name.clone();
        if self.own() {
            line.push_str(", ");
            line.push_str(&words.phrase(Phrase::YouAre));
        }
        for badge in self.badges().into_iter().filter(|badge| *badge != Presence::Online) {
            line.push_str(", ");
            line.push_str(&words.phrase(presence_phrase(badge)));
        }
        line
    }
}

/// One card of a call's grid: a person, or a shared screen right after its sharer, this account's own first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card<'a> {
    /// A person's tile.
    Person(&'a Tile),
    /// This account's own screen, going out, showing what is sent once it can.
    OwnScreen(Preview),
    /// Someone else's screen, playing here from its start.
    Screen {
        /// The sharer's tile, whose name and colour the card shows.
        sharer: &'a Tile,
        /// The sharer's user id, which pins the card.
        user_id: &'a MemberId,
        /// What has arrived of the screen so far.
        received: &'a Received,
    },
}

/// What this account's own screen card shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preview {
    /// The screen is picked and its first picture is awaited.
    #[default]
    Waiting,
    /// What is sent.
    Shown,
    /// This device cannot show it; the card says so.
    Unshown,
}

/// Which card fills the stage: a person or someone's screen by their id, or this account's own screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pinned {
    /// A person's tile, by their user id.
    Person(MemberId),
    /// This account's own screen.
    OwnScreen,
    /// Someone's screen, by the sharer's user id.
    Screen(MemberId),
}

impl Card<'_> {
    /// Whether it shows a picture: a screen does, and a person while their camera shows.
    pub fn pictured(&self) -> bool {
        match self {
            Card::Person(tile) => tile.camera.pictured(),
            Card::OwnScreen(_) | Card::Screen { .. } => true,
        }
    }

    /// What pins this card.
    pub fn pin(&self) -> Option<Pinned> {
        match self {
            Card::Person(tile) => tile.user_id.clone().map(Pinned::Person),
            Card::OwnScreen(_) => Some(Pinned::OwnScreen),
            Card::Screen { user_id, .. } => Some(Pinned::Screen((*user_id).clone())),
        }
    }
}

/// Whether a call's stage is tall enough to keep a pinned card its least height with the strip under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The strip fits under the pinned card.
    Tall,
    /// It does not: the strip is put away unless asked for while short.
    Short,
}

impl Stage {
    /// The least room tall a stage keeps a pinned card [`design::STAGE_LEAST_HEIGHT`] tall in over the strip, under
    /// the top bar or a folded call's header: the stage's top inset, the bar's room, a strip card and its gap.
    pub const STRIP_FROM: f32 = design::Space::L.px()
        + design::STAGE_BAR_ROOM
        + design::STRIP_TILE.1
        + design::Space::M.px()
        + design::STAGE_LEAST_HEIGHT;

    /// The stage of a call whose room under the top bar or its folded header is `height` tall.
    pub fn of(height: f32) -> Stage {
        if height >= Stage::STRIP_FROM { Stage::Tall } else { Stage::Short }
    }
}

/// How the tiles are laid out in a space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Tiles across: as many as give each the largest screen-shaped picture.
    pub columns: usize,
    /// Rows the tiles fill, the last perhaps not full.
    pub rows: usize,
    /// Each tile's width and height.
    pub tile: (f32, f32),
    /// Whether every tile is at least as large as `least` asks. When it is not the space is too small for the people
    /// in it and the front end scrolls rather than shrinking a person to nothing.
    pub fits: bool,
}

/// A tile no shorter than this much of its least width: the shape of a screen.
const LEAST_SHAPE: f32 = 16.0 / 9.0;

/// The fewest columns `count` people stand in: one fills the space, up to four are two across, more are three.
const fn fewest_columns(count: usize) -> usize {
    match count {
        0 | 1 => 1,
        2..=4 => 2,
        _ => 3,
    }
}

/// The columns and rows of `count` tiles in a `width` by `height` space with `gap` between, stretched to fill it. From
/// the fewest columns for the count up to as many as the width holds at `least` wide, the count whose tiles hold the
/// largest screen-shaped picture at their least height or more is taken, so a large space holds a crowd in tiles of a
/// screen's shape rather than in strips. When no count keeps that height the tiles are that tall, as many across as fit, and `fits` is false
/// so the front end scrolls.
pub fn grid(count: usize, width: f32, height: f32, gap: f32, least: f32) -> Option<Grid> {
    if count == 0 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let least_height = least / LEAST_SHAPE;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a float cast saturates, and at least one column is kept"
    )]
    let room = (((width + gap) / (least + gap)).floor() as usize).clamp(1, count);
    let laid = |columns: usize| {
        let rows = count.div_ceil(columns);
        let wide = (width - gap * (columns - 1) as f32) / columns as f32;
        let high = (height - gap * (rows - 1) as f32) / rows as f32;
        Grid { columns, rows, tile: (wide, high), fits: wide >= least }
    };
    let picture = |grid: &Grid| {
        let across = grid.tile.0.min(grid.tile.1 * LEAST_SHAPE);
        across * across / LEAST_SHAPE
    };
    let best = (fewest_columns(count).min(room)..=room)
        .map(laid)
        .filter(|grid| grid.tile.1 >= least_height)
        .max_by(|one, other| picture(one).total_cmp(&picture(other)));
    Some(best.unwrap_or_else(|| {
        let crowded = laid(room);
        Grid { tile: (crowded.tile.0.max(least.min(width)), least_height), fits: false, ..crowded }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::call::sound::Keep;
    use crate::harness::{Clock, cards, member, sharer_in_call, tell, the_call};
    use crate::window::Window;
    use crate::{Ask, WindowMessage, place};
    use std::time::Instant;

    fn badges(edit: impl FnOnce(&mut Tile)) -> Vec<Presence> {
        let mut tile = Tile::new("Ada");
        edit(&mut tile);
        tile.badges()
    }

    #[test]
    fn a_person_with_nothing_to_say_is_online_and_a_connection_not_whole_says_only_that() {
        assert_eq!(badges(|_| {}), [Presence::Online]);
        for (connection, said) in [
            (Connection::Connecting, Presence::Connecting),
            (Connection::Failed, Presence::Failed),
            (Connection::Reconnecting, Presence::Reconnecting),
        ] {
            assert_eq!(
                badges(|tile| {
                    tile.connection = connection;
                    tile.speaking = true;
                    tile.muted = true;
                    tile.sharing = true;
                }),
                [said],
                "{connection:?}: nothing else is true of them until it is whole"
            );
        }
    }

    #[test]
    fn what_they_are_doing_is_said_in_words_and_speaking_needs_to_be_heard() {
        assert_eq!(badges(|tile| tile.speaking = true), [Presence::Speaking]);
        assert_eq!(badges(|tile| tile.muted = true), [Presence::Muted]);
        assert_eq!(badges(|tile| tile.deafened = true), [Presence::Deafened]);
        assert_eq!(badges(|tile| tile.sharing = true), [Presence::Live]);
        assert_eq!(
            badges(|tile| (tile.muted, tile.deafened, tile.sharing) = (true, true, true)),
            [Presence::Muted, Presence::Deafened, Presence::Live]
        );
        assert_eq!(
            badges(|tile| (tile.speaking, tile.muted) = (true, true)),
            [Presence::Muted],
            "a muted person is not heard"
        );
        assert_eq!(badges(|tile| (tile.speaking, tile.deafened) = (true, true)), [Presence::Deafened]);
        assert_eq!(badges(|tile| (tile.speaking, tile.sharing) = (true, true)), [Presence::Speaking, Presence::Live]);
    }

    #[test]
    fn the_ring_shows_for_someone_heard_and_for_no_one_else() {
        let mut tile = Tile::new("Ada");
        assert!(!tile.ringed());
        tile.speaking = true;
        assert!(tile.ringed());
        tile.muted = true;
        assert!(!tile.ringed());
        tile.muted = false;
        tile.connection = Connection::Reconnecting;
        assert!(!tile.ringed());
    }

    #[test]
    fn a_screen_reader_hears_the_name_and_what_it_says() {
        let mut tile = Tile::new("Ada");
        tile.speaking = true;
        tile.sharing = true;
        assert_eq!(tile.said(&Words::english()), "Ada, Speaking, Live");
        tile.own = true;
        assert_eq!(tile.said(&Words::english()), "Ada, you, Speaking, Live", "this account is told to itself");
        (tile.speaking, tile.sharing) = (false, false);
        assert_eq!(tile.badges(), [Presence::Online], "the tile shows a quiet person online");
        assert_eq!(tile.said(&Words::english()), "Ada, you", "which everyone in a call is, so it goes unsaid");
    }

    fn used(g: &Grid, gap: f32) -> (f32, f32) {
        (
            g.columns as f32 * g.tile.0 + (g.columns - 1) as f32 * gap,
            g.rows as f32 * g.tile.1 + (g.rows - 1) as f32 * gap,
        )
    }

    #[test]
    fn no_people_is_no_grid_and_one_fills_the_space() {
        assert_eq!(grid(0, 800.0, 600.0, 8.0, 160.0), None);
        let one = grid(1, 800.0, 600.0, 8.0, 160.0).unwrap();
        assert_eq!((one.columns, one.rows, one.fits), (1, 1, true));
        assert_eq!(one.tile, (800.0, 600.0), "the tile is the space, whatever its shape");
    }

    #[test]
    fn in_a_landscape_stage_people_stand_one_then_two_then_two_by_two_then_three_across() {
        let shape = |count| {
            let g = grid(count, 1568.0, 647.0, 12.0, 160.0).unwrap();
            (g.columns, g.rows)
        };
        let expected = [(1, 1), (2, 1), (2, 2), (2, 2), (3, 2), (3, 2), (4, 2), (4, 2), (4, 3), (4, 3)];
        let tall = |count| {
            let g = grid(count, 1200.0, 800.0, 12.0, 160.0).unwrap();
            (g.columns, g.rows)
        };
        assert_eq!(tall(2), (2, 1), "two stand side by side even where stacking them would draw larger");
        for (count, shape_of) in (1..=10).zip(expected) {
            assert_eq!(shape(count), shape_of, "{count} people");
        }
    }

    /// Whatever the count and the space: the tiles stretch to fill what is there when they fit, and are never
    /// narrower than the least unless the space is.
    #[test]
    fn the_tiles_fill_the_space_when_they_fit_and_scroll_when_they_do_not() {
        let gap = 12.0;
        for (width, height) in
            [(480.0, 360.0), (800.0, 600.0), (1280.0, 720.0), (1920.0, 1080.0), (360.0, 900.0), (1500.0, 300.0)]
        {
            for count in 1..=40 {
                let g = grid(count, width, height, gap, 160.0).unwrap();
                assert!(g.columns * g.rows >= count && (g.rows - 1) * g.columns < count, "{count}: {g:?}");
                let (across, down) = used(&g, gap);
                assert!(g.tile.0 >= 160.0f32.min(width) - 0.01, "{count} in {width}x{height}: {g:?}");
                assert!(across <= width.max(160.0) + 0.01, "{count} in {width}x{height} runs over: {g:?}");
                if g.fits {
                    assert!(down <= height + 0.01, "{count} in {width}x{height} runs under: {g:?}");
                } else {
                    assert!(down > height - 0.01 || g.tile.0 < 160.0 + 0.01, "{count} in {width}x{height}: {g:?}");
                }
            }
        }
    }

    #[test]
    fn a_crowd_takes_the_space_s_shape_and_no_other_count_of_columns_gives_a_larger_picture() {
        // A room's most, 22, on a 1080p monitor's stage: three columns made strips 600 by 105.
        let crowd = grid(22, 1888.0, 927.0, 12.0, 160.0).unwrap();
        assert!(crowd.fits, "{crowd:?}");
        assert_eq!((crowd.columns, crowd.rows), (5, 5));
        let picture = |g: &Grid| {
            let across = g.tile.0.min(g.tile.1 * LEAST_SHAPE);
            across * across / LEAST_SHAPE
        };
        for (width, height) in [(387.0, 175.0), (800.0, 600.0), (1500.0, 300.0), (2218.0, 1287.0), (3700.0, 2000.0)] {
            for count in 1..=50 {
                let chosen = grid(count, width, height, 12.0, 160.0).unwrap();
                if !chosen.fits {
                    continue;
                }
                for columns in fewest_columns(count)..=count {
                    let rows = count.div_ceil(columns);
                    let wide = (width - 12.0 * (columns - 1) as f32) / columns as f32;
                    let high = (height - 12.0 * (rows - 1) as f32) / rows as f32;
                    let other = Grid { columns, rows, tile: (wide, high), fits: true };
                    if wide >= 160.0 && high >= 160.0 / LEAST_SHAPE {
                        assert!(
                            picture(&chosen) >= picture(&other),
                            "{count} in {width}x{height}: {chosen:?} {other:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_stage_is_short_where_a_pinned_card_over_the_strip_would_be_under_its_least() {
        assert_eq!(Stage::STRIP_FROM, 16.0 + 88.0 + 76.0 + 12.0 + 160.0);
        assert_eq!(Stage::of(Stage::STRIP_FROM), Stage::Tall);
        assert_eq!(Stage::of(Stage::STRIP_FROM - 1.0), Stage::Short);
        let header = design::PANE_HEADER_HEIGHT + design::BORDER_WIDTH;
        assert_eq!(Stage::of(design::WINDOW_LEAST.1 - header), Stage::Short);
    }

    #[test]
    fn a_crowd_in_a_small_space_keeps_each_person_readable_and_runs_on_in_rows() {
        let crowd = grid(50, 480.0, 360.0, 8.0, 120.0).unwrap();
        assert!(!crowd.fits);
        assert_eq!(crowd.columns, 3);
        assert_eq!(crowd.rows, 17);
        assert!(crowd.tile.0 >= 120.0 && crowd.tile.1 >= 120.0 / LEAST_SHAPE - 0.01, "{crowd:?}");
        let narrow = grid(5, 100.0, 400.0, 8.0, 120.0).unwrap();
        assert_eq!((narrow.columns, narrow.rows), (1, 5), "even a space narrower than the least holds one column");
    }

    fn view(window: &mut Window, clock: &Clock, pick: choices::ViewPick) -> Vec<Ask> {
        window.update(WindowMessage::Look(choices::LookPick::View(pick)), clock.at())
    }

    #[test]
    fn hiding_people_with_no_picture_leaves_the_shares_and_only_while_there_is_one() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let todo = view(&mut window, &clock, choices::ViewPick::People);
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::Look(look))] if look.call.people == choices::People::Pictured),
            "kept: {todo:?}"
        );
        let bo = the_call(&window).tiles().iter().find(|t| t.user_id == Some(member("bo"))).unwrap().name.clone();
        assert_eq!(cards(&window), [format!("{bo}'s screen")], "only the share");
        let stops = RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(stops)));
        assert_eq!(cards(&window).len(), the_call(&window).tiles().len(), "with no picture left, everyone");
        view(&mut window, &clock, choices::ViewPick::People);
        assert_eq!(window.look().call, choices::CallView::default(), "switched back");
    }

    #[test]
    fn a_call_bar_shown_only_while_wanted_is_kept_and_held_while_a_menu_of_its_own_is_open() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let todo = view(&mut window, &clock, choices::ViewPick::Controls);
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::Look(look))] if look.call.controls == choices::Controls::WhileHovered),
            "kept: {todo:?}"
        );
        assert!(!the_call(&window).bar_held(), "nothing of the bar's is open");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert!(the_call(&window).bar_held(), "the share panel hangs from the bar");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CloseOver));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ViewMenu));
        assert!(!the_call(&window).bar_held(), "Voice options hang from the header");
        view(&mut window, &clock, choices::ViewPick::Controls);
        assert_eq!(window.look().call, choices::CallView::default(), "switched back");
    }

    #[test]
    fn hiding_one_s_own_share_takes_its_card_pin_and_popover_and_a_hidden_person_s_pin() {
        use crate::Pinned;
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::Starting);
        let route = crate::Route::Hardware { via: "VA-API".into() };
        call(
            &mut window,
            place::CallMessage::Shared(crate::Shared::Started { width: 1920, height: 1080, encoder: route.clone() }),
        );
        let looked = crate::Outgoing {
            codec: "H.264".into(),
            encoder: route,
            width: 1920,
            height: 1080,
            scaled_from: None,
            frame_rate: Some(60),
            bits_per_second: 8_000_000,
            frames: 120,
            key_frames_made: 0,
            key_frames_asked: 0,
            resent_bytes: 0,
            restarts: 0,
            limited: false,
            reduced: None,
        };
        call(&mut window, place::CallMessage::Shared(crate::Shared::Looked(looked)));
        call(&mut window, place::CallMessage::Pin(Pinned::OwnScreen));
        call(&mut window, place::CallMessage::ToggleOwnInfo);
        assert_eq!(the_call(&window).open(), Some(place::Over::OwnPicture));
        view(&mut window, &clock, choices::ViewPick::OwnShare);
        assert!(!cards(&window).contains(&"own screen".to_string()), "{:?}", cards(&window));
        assert_eq!(the_call(&window).pinned(), None, "its pin goes with it");
        assert_eq!(the_call(&window).open(), None, "and its popover");
        call(&mut window, place::CallMessage::ToggleOwnInfo);
        assert_eq!(the_call(&window).open(), None, "nothing to open it on");
        assert!(the_call(&window).outgoing().is_some(), "it still goes out");

        call(&mut window, place::CallMessage::Pin(Pinned::Person(member("ada"))));
        view(&mut window, &clock, choices::ViewPick::People);
        assert_eq!(the_call(&window).pinned(), None, "a person hidden takes their pin");
    }

    #[test]
    fn a_shared_screen_is_a_card_after_its_sharer_and_this_account_s_own_come_first() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let names: Vec<String> = the_call(&window).tiles().iter().map(|tile| tile.name.clone()).collect();
        let (me, others) = names.split_first().unwrap();
        assert!(the_call(&window).tiles()[0].own());
        let bo = the_call(&window).tiles().iter().find(|t| t.user_id == Some(member("bo"))).unwrap().name.clone();
        let expected = |own_screen: bool| {
            let mut cards = vec![me.clone()];
            if own_screen {
                cards.push("own screen".into());
            }
            for name in others {
                cards.push(name.clone());
                if *name == bo {
                    cards.push(format!("{bo}'s screen"));
                }
            }
            cards
        };
        assert_eq!(cards(&window), expected(false), "bo's screen is beside bo");

        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(cards(&window), expected(true), "this account's screen is second, after its own tile");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(cards(&window), expected(false), "a share that stopped takes its card away");
    }

    #[test]
    fn any_card_is_pinned_one_at_a_time_a_screen_from_its_start() {
        use crate::Pinned;
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let pinned = |window: &Window| the_call(window).pinned().cloned();
        let pin = |window: &mut Window, pinned: Pinned| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Pin(pinned)))
        };

        assert!(pin(&mut window, Pinned::Person(member("cy"))).is_empty(), "pinning is this window's own");
        assert_eq!(pinned(&window), Some(Pinned::Person(member("cy"))));
        pin(&mut window, Pinned::Person(member("ada")));
        assert_eq!(pinned(&window), Some(Pinned::Person(member("ada"))), "one at a time, this account's own tile too");

        pin(&mut window, Pinned::Screen(member("cy")));
        pin(&mut window, Pinned::Person(member("zed")));
        pin(&mut window, Pinned::OwnScreen);
        assert_eq!(
            pinned(&window),
            Some(Pinned::Person(member("ada"))),
            "nor a screen not shared, nor someone not here, nor a share not going out"
        );

        pin(&mut window, Pinned::Screen(member("bo")));
        assert_eq!(pinned(&window), Some(Pinned::Screen(member("bo"))), "before its first picture too");
        assert_eq!(pinned(&window), Some(Pinned::Screen(member("bo"))));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Unpin));
        assert_eq!(pinned(&window), None);

        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        pin(&mut window, Pinned::OwnScreen);
        assert_eq!(pinned(&window), Some(Pinned::OwnScreen), "this account's screen while it goes out");
    }

    #[test]
    fn a_pin_goes_with_its_card() {
        use crate::Pinned;
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let pinned = |window: &Window| the_call(window).pinned().cloned();
        let pin = |window: &mut Window, pinned: Pinned| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Pin(pinned)))
        };
        let room = |window: &mut Window, event: RoomEvent| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Room(event)))
        };

        pin(&mut window, Pinned::Person(member("cy")));
        room(&mut window, RoomEvent::MemberLeft { user_id: member("cy") });
        assert_eq!(pinned(&window), None, "a person who left");

        pin(&mut window, Pinned::Screen(member("bo")));
        room(&mut window, RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo });
        assert_eq!(pinned(&window), None, "a share that ended");

        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        pin(&mut window, Pinned::OwnScreen);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(pinned(&window), None, "this account's share that it stopped");
    }

    #[test]
    fn the_strip_hides_and_shows_and_stays_so_for_the_call_on_each_height_apart() {
        use crate::{Pinned, Stage, Strip};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let strip = |window: &Window, stage| the_call(window).strip(stage);
        let call = |window: &mut Window, message: place::CallMessage| {
            tell(window, &clock, place, place::Message::Call(message))
        };

        assert_eq!(strip(&window, Stage::Tall), Strip::Shown, "a call starts with its strip");
        assert_eq!(strip(&window, Stage::Short), Strip::Hidden, "except on a stage too short for it");
        call(&mut window, place::CallMessage::ToggleStrip(Stage::Tall));
        assert_eq!(strip(&window, Stage::Tall), Strip::Shown, "there is no strip to hide with nothing pinned");

        call(&mut window, place::CallMessage::Pin(Pinned::Person(member("cy"))));
        assert!(call(&mut window, place::CallMessage::ToggleStrip(Stage::Tall)).is_empty(), "this window's own");
        assert_eq!(strip(&window, Stage::Tall), Strip::Hidden);
        call(&mut window, place::CallMessage::Unpin);
        call(&mut window, place::CallMessage::Pin(Pinned::Screen(member("bo"))));
        assert_eq!(strip(&window, Stage::Tall), Strip::Hidden, "kept as the next card is pinned");
        call(&mut window, place::CallMessage::ToggleStrip(Stage::Tall));
        assert_eq!(strip(&window, Stage::Tall), Strip::Shown);
        call(&mut window, place::CallMessage::ToggleStrip(Stage::Short));
        assert_eq!(
            (strip(&window, Stage::Short), strip(&window, Stage::Tall)),
            (Strip::Shown, Strip::Shown),
            "shown while short, and the tall choice untouched"
        );
    }
}
