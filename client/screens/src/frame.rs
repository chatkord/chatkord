//! The window's frame at its size: the rail, the server pane, the content and the members column, folded by room. The
//! pane docks while the rail, a rule, the pane at its least, a rule and the content's least fit; the rail stays while
//! the rail, a rule and the content's least fit; below that the rail and the pane open together as one drawer over the
//! content. The members column docks beside a docked pane's content while the content keeps its least beside it (in a
//! call, while the grid keeps its least card), else its toggle opens it over the content for a moment.

use choices::{CallChat, CallChatShown, Controls, Members, MembersShown, PaneWidth};
use design::{
    BORDER_WIDTH, BRAND_PANE_WIDTH, CONTENT_LEAST, FORM_WIDTH, HIT_TARGET, MESSAGE_SIDE, MESSAGE_SIDE_NARROW,
    MESSAGE_SIDE_NARROW_BELOW, PANE_HEADER_HEIGHT, PANE_LEAST, PANE_MOST, RAIL_WIDTH, ROLE_LIST_WIDTH,
    SERVER_BAR_HEIGHT, SETTINGS_NAV_WIDTH, STAGE_BAR_ROOM, Space, THREAD_CHANNEL_LEAST, THREAD_WIDTH, TILE_LEAST,
};

/// The room the window gives its frame, in logical pixels after the zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    /// Across.
    pub width: f32,
    /// Down.
    pub height: f32,
}

/// What a splitter did: the one between the pane and the content, or the one on the members column's edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanePick {
    /// Dragged to this width; kept only once let go.
    To(f32),
    /// Let go: the width it was dragged to is kept.
    Let,
    /// An arrow key that makes the column narrower: toward the rail for the pane, toward the window's edge for the
    /// members.
    Narrower,
    /// An arrow key that makes it wider.
    Wider,
    /// A double press: back to its width at the start.
    Reset,
}

/// Whether the members column is over the content where it cannot dock: opened by its toggle, for a moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MembersOver {
    /// Put away.
    #[default]
    Shut,
    /// Over the content.
    Open,
}

impl MembersOver {
    /// The other one: what a toggle makes of it.
    #[must_use]
    pub const fn toggled(self) -> MembersOver {
        match self {
            MembersOver::Shut => MembersOver::Open,
            MembersOver::Open => MembersOver::Shut,
        }
    }
}

/// What the members column asks of the frame where the open place shows one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MembersAsk {
    /// Open or closed where it docks, and its width, as this device keeps them.
    pub kept: Members,
    /// Whether it is over the content where it cannot dock.
    pub over: MembersOver,
    /// The cards in the grid of the call the content shows; none outside a call.
    pub call: Option<usize>,
}

/// What a call's chat column asks of the frame where a call shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallChatAsk {
    /// Open or closed, and its width, as this device keeps them.
    pub kept: CallChat,
    /// Whether it is over the call where it cannot dock.
    pub over: MembersOver,
    /// The cards in the call's grid.
    pub cards: usize,
}

/// The members column, or a call's chat column, as the frame draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MembersColumn {
    /// Beside the content, this wide, the content narrowed by it and a rule.
    Docked(f32),
    /// Over the content at the window's edge, this wide.
    Over(f32),
}

/// What the frame shows beside the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The rail and the pane beside the content.
    Docked,
    /// The rail beside the content; the pane opens as a drawer at the rail's edge.
    PaneDrawer,
    /// The content alone; the rail and the pane open together as one drawer.
    Drawer,
}

/// Whether a folded frame's drawer is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Drawer {
    /// Put away.
    #[default]
    Shut,
    /// Over the content.
    Open,
}

/// The frame at one room: its shape, whether its drawer shows, and how wide the pane and the content are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// What shows beside the content.
    pub shape: Shape,
    /// Open only while the frame is folded.
    pub drawer: Drawer,
    /// The pane's width as drawn: the kept width while it fits, less while docked in a narrow window, and in a drawer
    /// never so wide that less than a target of the content shows beside it; none where the open place shows none.
    pub pane: Option<f32>,
    /// The content's width.
    pub content: f32,
    /// The widest the splitter may drag the pane now, so the content keeps its least.
    pub pane_most: f32,
    /// The members column, where it shows.
    pub members: Option<MembersColumn>,
    /// A call's chat column, where it shows; the members column shows beside a call only while this does not.
    pub call_chat: Option<MembersColumn>,
    /// The room the top bar and its rule take over the pane and the content: none where the bar does not show.
    pub top: f32,
}

/// The room the rail and its rule take.
const RAIL: f32 = RAIL_WIDTH + BORDER_WIDTH;

/// The narrowest room the pane docks in.
pub const DOCKS_FROM: f32 = RAIL + PANE_LEAST + BORDER_WIDTH + CONTENT_LEAST;

/// The narrowest room the rail stays beside the content in.
pub const RAIL_FROM: f32 = RAIL + CONTENT_LEAST;

/// The narrowest room settings keep their list of pages beside the open page in.
pub const SETTINGS_BESIDE_FROM: f32 = SETTINGS_NAV_WIDTH + BORDER_WIDTH + CONTENT_LEAST;

/// What settings show, decided by their room and, folded, by where the person is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsShown {
    /// The list of pages beside the open page.
    Beside,
    /// The list alone, the whole room wide.
    List,
    /// The open page alone, with Back to the list.
    Page,
}

impl SettingsShown {
    /// The room on either side of a settings page: less where the page is shown alone.
    pub const fn gutter(self) -> f32 {
        match self {
            SettingsShown::Page => Space::L.px(),
            SettingsShown::Beside | SettingsShown::List => Space::Xxl.px() + Space::Xs.px(),
        }
    }
}

/// The narrowest settings page that keeps the roles' list beside the chosen role's editor.
pub const ROLES_BESIDE_FROM: f32 = ROLE_LIST_WIDTH + Space::Xl.px() + FORM_WIDTH;

/// What Server settings › Roles shows, decided by the page's room and, folded, by where the person is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolesShown {
    /// The list of roles beside the chosen role.
    Beside,
    /// The list alone.
    List,
    /// The chosen role alone, with Back to the list in place of the page's heading.
    Role,
}

/// The narrowest window the first run's door keeps its side beside the form in.
pub const SIDE_BY_SIDE_FROM: f32 = BRAND_PANE_WIDTH + BORDER_WIDTH + FORM_WIDTH + 2.0 * Space::L.px();

/// Whether the door in a window `width` wide keeps its side beside the form; narrower, the side is folded away.
pub fn door_side_by_side(width: f32) -> bool {
    width >= SIDE_BY_SIDE_FROM
}

/// The room either side of a text channel's messages and composer in `pane` drawn `width` wide.
pub fn message_side(pane: crate::Pane, width: f32) -> f32 {
    match pane {
        crate::Pane::Thread => Space::L.px(),
        crate::Pane::Content | crate::Pane::CallChat if width < MESSAGE_SIDE_NARROW_BELOW => MESSAGE_SIDE_NARROW,
        crate::Pane::Content | crate::Pane::CallChat => MESSAGE_SIDE,
    }
}

/// The room the stage keeps under it for the call bar; a bar shown only while wanted floats over the stage instead.
pub fn bar_room(controls: Controls) -> f32 {
    match controls {
        Controls::Always => STAGE_BAR_ROOM,
        Controls::WhileHovered => Space::L.px(),
    }
}

/// How the thread open beside a channel is laid out in the room the content has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadColumn {
    /// The thread's width: [`THREAD_WIDTH`], or the whole room where it covers the channel.
    pub width: f32,
    /// What the channel keeps beside it.
    pub channel: f32,
    /// The members column shows too: it goes while the channel would keep less than [`THREAD_CHANNEL_LEAST`].
    pub members: bool,
    /// The room is too narrow for both: the thread covers the channel, its back arrow closes it.
    pub covers: bool,
}

/// How far an arrow key moves the splitter.
pub const PANE_STEP: f32 = Space::S.px();

/// Whether a call of `cards` keeps its least card in content `width` wide in a room `height` tall: its stage, the
/// content less its insets, under the top bar (folded, the call's header, as tall) and over its call bar.
fn call_fits(cards: usize, width: f32, height: f32) -> bool {
    let stage_width = width - 2.0 * Space::L.px();
    let stage_height = height - PANE_HEADER_HEIGHT - BORDER_WIDTH - Space::L.px() - STAGE_BAR_ROOM;
    crate::call::voice::grid(cards, stage_width, stage_height, Space::M.px(), TILE_LEAST).is_some_and(|grid| grid.fits)
}

impl Frame {
    /// The frame in `room` for a pane kept `pane` wide, none where the open place shows none, with its drawer as
    /// `drawer` says while folded, and the members column `members` asks, none where the open place shows none.
    pub fn of(room: Room, pane: Option<PaneWidth>, drawer: Drawer, members: Option<MembersAsk>) -> Frame {
        let shape = Frame::shape(room.width);
        let docked_room = room.width - RAIL - BORDER_WIDTH - CONTENT_LEAST;
        let pane_most = docked_room.clamp(PANE_LEAST, PANE_MOST);
        let frame = match shape {
            Shape::Docked => {
                let pane = pane.map(|pane| pane.px().min(pane_most));
                let content = room.width - RAIL - pane.map_or(0.0, |pane| pane + BORDER_WIDTH);
                Frame {
                    shape,
                    drawer: Drawer::Shut,
                    pane,
                    content,
                    pane_most,
                    members: None,
                    call_chat: None,
                    top: 0.0,
                }
            }
            Shape::PaneDrawer | Shape::Drawer => {
                let content = if shape == Shape::Drawer { room.width } else { room.width - RAIL };
                let pane = pane.map(|pane| pane.px().min(room.width - RAIL - HIT_TARGET).max(PANE_LEAST));
                Frame { shape, drawer, pane, content, pane_most, members: None, call_chat: None, top: 0.0 }
            }
        };
        match members {
            Some(ask) => frame.with_members(room, ask),
            None => frame,
        }
    }

    /// This frame with the members column `ask` asks: docked, the content narrowed by it, while the content keeps its
    /// least beside it (in a call, while the grid keeps its least card, unless the call holds it even without the
    /// column); else over the content while its toggle opened it there.
    fn with_members(self, room: Room, ask: MembersAsk) -> Frame {
        let open = ask.kept.shown == MembersShown::Open;
        let (members, content) = self.column(room, ask.kept.width.px(), open, ask.over, ask.call);
        Frame { members, content, ..self }
    }

    /// This frame with a call's chat column, as `ask` asks: by the members column's rule, the call's grid for the
    /// content, and in its place, so the members column goes while it shows.
    pub fn with_call_chat(self, room: Room, ask: Option<CallChatAsk>) -> Frame {
        let Some(ask) = ask else { return self };
        let open = ask.kept.shown == CallChatShown::Open;
        let (content, members) = match self.members {
            Some(MembersColumn::Docked(width)) => (self.content + width + BORDER_WIDTH, None),
            Some(MembersColumn::Over(_)) | None => (self.content, self.members),
        };
        let unmembered = Frame { content, members, ..self };
        let (call_chat, content) = unmembered.column(room, ask.kept.width.px(), open, ask.over, Some(ask.cards));
        match call_chat {
            Some(_) => Frame { call_chat, content, members: None, ..unmembered },
            None => self,
        }
    }

    /// A column `width` wide at the content's edge: docked, the content narrowed by it and a rule, while the content
    /// keeps its least beside it (with `call`'s cards, while the grid keeps its least card, unless the call holds it
    /// even without the column) and it is kept `open`; else over the content while its toggle opened it there. The
    /// column, and the content's width beside it.
    fn column(
        &self,
        room: Room,
        width: f32,
        open: bool,
        over: MembersOver,
        call: Option<usize>,
    ) -> (Option<MembersColumn>, f32) {
        let beside = self.content - width - BORDER_WIDTH;
        let fits = |content: f32| call.is_none_or(|cards| call_fits(cards, content, room.height));
        let docks = self.shape == Shape::Docked && beside >= CONTENT_LEAST && (fits(beside) || !fits(self.content));
        let column = match (docks, open, over) {
            (true, true, _) => Some(MembersColumn::Docked(width)),
            (true, false, _) | (false, _, MembersOver::Shut) => None,
            (false, _, MembersOver::Open) => Some(MembersColumn::Over(width.min(self.content))),
        };
        let content = match column {
            Some(MembersColumn::Docked(_)) => beside,
            Some(MembersColumn::Over(_)) | None => self.content,
        };
        (column, content)
    }

    /// This frame under the top bar, which runs across the pane and the content.
    pub fn under_bar(self) -> Frame {
        Frame { top: SERVER_BAR_HEIGHT + BORDER_WIDTH, ..self }
    }

    /// Whether a call's chat column docks.
    pub fn call_chat_docks(&self) -> bool {
        matches!(self.call_chat, Some(MembersColumn::Docked(_)))
    }

    /// Whether the members column docks where the open place shows one.
    pub fn members_dock(&self) -> bool {
        matches!(self.members, Some(MembersColumn::Docked(_)))
    }

    /// The shape a frame takes in a room `width` wide.
    pub fn shape(width: f32) -> Shape {
        if width >= DOCKS_FROM {
            Shape::Docked
        } else if width >= RAIL_FROM {
            Shape::PaneDrawer
        } else {
            Shape::Drawer
        }
    }

    /// The room settings take: the pane's and the content's while docked, else the content's.
    pub fn settings_room(&self) -> f32 {
        match self.shape {
            Shape::Docked => self.pane.map_or(0.0, |pane| pane + BORDER_WIDTH) + self.content,
            Shape::PaneDrawer | Shape::Drawer => self.content,
        }
    }

    /// The room a settings page's content has, between its gutters, as `shown` lays settings out.
    pub fn settings_page(&self, shown: SettingsShown) -> f32 {
        let list = match shown {
            SettingsShown::Beside => SETTINGS_NAV_WIDTH + BORDER_WIDTH,
            SettingsShown::List | SettingsShown::Page => 0.0,
        };
        self.settings_room() - list - 2.0 * shown.gutter()
    }

    /// Whether a settings page shown as `shown` keeps a search beside the choice of what it lists, as the members
    /// page does, else under it the page's whole width: beside where the page is a form's width.
    pub fn search_beside(&self, shown: SettingsShown) -> bool {
        self.settings_page(shown) >= FORM_WIDTH
    }

    /// Whether the pane is in a drawer.
    pub fn folded(&self) -> bool {
        self.shape != Shape::Docked
    }

    /// Whether the drawer is over the content now.
    pub fn drawer_open(&self) -> bool {
        self.folded() && self.drawer == Drawer::Open
    }

    /// The room the rail and its rule take at the window's left: none where the rail is in the drawer.
    pub fn rail_room(&self) -> f32 {
        if self.shape == Shape::Drawer { 0.0 } else { RAIL }
    }

    /// The room a docked pane and its rule take beside the rail: none while the pane is in a drawer.
    pub fn pane_room(&self) -> f32 {
        self.pane.filter(|_| !self.folded()).map_or(0.0, |pane| pane + BORDER_WIDTH)
    }

    /// The room a docked members column and its rule take at the window's right edge.
    pub fn members_room(&self) -> f32 {
        match self.members {
            Some(MembersColumn::Docked(width)) => width + BORDER_WIDTH,
            Some(MembersColumn::Over(_)) | None => 0.0,
        }
    }

    /// The room at the window's top that the bar takes, or folded, the header each view has in its place: where a
    /// call's stage and the members column over the content start.
    pub fn top_room(&self) -> f32 {
        if self.folded() { PANE_HEADER_HEIGHT + BORDER_WIDTH } else { self.top }
    }

    /// The thread's column beside a channel in this frame's content, where a thread is open in a discussion: beside
    /// the channel while it keeps its least, using the members column's room too where that is what it takes, else
    /// over the channel.
    pub fn thread_column(&self) -> ThreadColumn {
        let beside = |room: f32| room - THREAD_WIDTH - BORDER_WIDTH;
        if beside(self.content) >= THREAD_CHANNEL_LEAST {
            return ThreadColumn {
                width: THREAD_WIDTH,
                channel: beside(self.content),
                members: self.members.is_some(),
                covers: false,
            };
        }
        let whole = self.content + self.members_room();
        if beside(whole) >= THREAD_CHANNEL_LEAST {
            return ThreadColumn { width: THREAD_WIDTH, channel: beside(whole), members: false, covers: false };
        }
        ThreadColumn { width: whole, channel: 0.0, members: false, covers: true }
    }

    /// Whether the members column shows beside the content: where a thread is open `thread`, as it lays them out.
    pub fn members_shown(&self, thread: Option<ThreadColumn>) -> bool {
        thread.map_or(self.members.is_some(), |thread| thread.members)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use design::WINDOW_LEAST;

    fn room(width: f32) -> Room {
        Room { width, height: 600.0 }
    }

    #[test]
    fn the_pane_docks_and_the_rail_stays_exactly_where_their_rooms_add_up() {
        assert_eq!(DOCKS_FROM, 60.0 + 1.0 + 248.0 + 1.0 + 360.0);
        assert_eq!(RAIL_FROM, 60.0 + 1.0 + 360.0);
        for pane in [PaneWidth::LEAST, PaneWidth::new(320.0), PaneWidth::MOST] {
            let at = |width| Frame::of(room(width), Some(pane), Drawer::Shut, None).shape;
            assert_eq!(at(DOCKS_FROM), Shape::Docked, "{pane:?}");
            assert_eq!(at(DOCKS_FROM - 1.0), Shape::PaneDrawer, "{pane:?}");
            assert_eq!(at(RAIL_FROM), Shape::PaneDrawer, "{pane:?}");
            assert_eq!(at(RAIL_FROM - 1.0), Shape::Drawer, "{pane:?}");
        }
    }

    #[test]
    fn a_docked_pane_gives_up_width_before_the_content_loses_its_least() {
        for width in [DOCKS_FROM, 700.0, 800.0, 1280.0, 2560.0] {
            for pane in [PaneWidth::LEAST, PaneWidth::new(320.0), PaneWidth::MOST] {
                let frame = Frame::of(room(width), Some(pane), Drawer::Shut, None);
                assert!(frame.content >= CONTENT_LEAST, "{width} {pane:?}: {frame:?}");
                assert!(
                    frame.pane.is_some_and(|drawn| drawn >= PANE_LEAST && drawn <= pane.px()),
                    "{width} {pane:?}: {frame:?}"
                );
                assert_eq!(
                    RAIL + frame.pane.unwrap_or_default() + BORDER_WIDTH + frame.content,
                    width,
                    "the row fills the room"
                );
            }
        }
        let wide = Frame::of(room(1280.0), Some(PaneWidth::MOST), Drawer::Shut, None);
        assert_eq!(wide.pane, Some(PANE_MOST), "with room, the kept width");
    }

    #[test]
    fn a_drawer_leaves_a_target_of_the_content_beside_it_even_in_the_least_window() {
        assert!(WINDOW_LEAST.0 >= RAIL + PANE_LEAST + HIT_TARGET, "the least window holds the widest drawer's least");
        for width in [WINDOW_LEAST.0, 360.0, RAIL_FROM - 1.0, RAIL_FROM, DOCKS_FROM - 1.0] {
            for pane in [PaneWidth::LEAST, PaneWidth::MOST] {
                let frame = Frame::of(room(width), Some(pane), Drawer::Open, None);
                assert!(frame.drawer_open());
                assert!(width - RAIL - frame.pane.unwrap_or_default() >= HIT_TARGET, "{width} {pane:?}: {frame:?}");
                assert!(frame.pane.is_some_and(|drawn| drawn >= PANE_LEAST), "{width} {pane:?}: {frame:?}");
            }
        }
    }

    #[test]
    fn a_drawer_is_open_only_while_the_frame_is_folded() {
        assert!(!Frame::of(room(1280.0), Some(PaneWidth::LEAST), Drawer::Open, None).drawer_open());
        assert!(Frame::of(room(500.0), Some(PaneWidth::LEAST), Drawer::Open, None).drawer_open());
        assert!(!Frame::of(room(500.0), Some(PaneWidth::LEAST), Drawer::Shut, None).drawer_open());
    }

    #[test]
    fn settings_keep_their_list_beside_the_page_while_it_and_the_contents_least_fit() {
        assert_eq!(SETTINGS_BESIDE_FROM, 220.0 + 1.0 + 360.0);
        for width in [WINDOW_LEAST.0, RAIL_FROM - 1.0, RAIL_FROM, 641.0, 642.0, DOCKS_FROM - 1.0, DOCKS_FROM, 1280.0] {
            let frame = Frame::of(room(width), Some(PaneWidth::MOST), Drawer::Shut, None);
            let rail = if frame.shape == Shape::Drawer { 0.0 } else { RAIL };
            assert_eq!(frame.settings_room(), width - rail, "{width}: the room beside the rail");
        }
        assert!(
            Frame::of(room(DOCKS_FROM), Some(PaneWidth::LEAST), Drawer::Shut, None).settings_room()
                >= SETTINGS_BESIDE_FROM
        );
        assert!(
            Frame::of(room(RAIL + SETTINGS_BESIDE_FROM), Some(PaneWidth::LEAST), Drawer::Shut, None).settings_room()
                >= SETTINGS_BESIDE_FROM
        );
        assert!(
            Frame::of(room(RAIL + SETTINGS_BESIDE_FROM - 1.0), Some(PaneWidth::LEAST), Drawer::Shut, None)
                .settings_room()
                < SETTINGS_BESIDE_FROM
        );
    }

    #[test]
    fn a_settings_page_s_room_is_the_settings_less_the_list_beside_it_and_its_gutters() {
        let frame = Frame::of(room(1280.0), Some(PaneWidth::LEAST), Drawer::Shut, None);
        assert_eq!(frame.settings_page(SettingsShown::Beside), 1280.0 - RAIL - 220.0 - 1.0 - 2.0 * 36.0);
        assert!(frame.settings_page(SettingsShown::Beside) >= ROLES_BESIDE_FROM, "the canvas's window");
        let narrow = Frame::of(room(WINDOW_LEAST.0), None, Drawer::Shut, None);
        assert_eq!(narrow.settings_page(SettingsShown::Page), WINDOW_LEAST.0 - 2.0 * 16.0);
        assert!(narrow.settings_page(SettingsShown::Page) < ROLES_BESIDE_FROM);
        assert!(frame.search_beside(SettingsShown::Beside) && !narrow.search_beside(SettingsShown::Page));
    }

    fn members(shown: MembersShown, over: MembersOver, call: Option<usize>) -> Option<MembersAsk> {
        Some(MembersAsk { kept: Members { shown, width: choices::MembersWidth::START }, over, call })
    }

    #[test]
    fn the_members_column_docks_exactly_where_the_content_keeps_its_least_beside_it_else_goes_over_it() {
        let width = design::MEMBERS_WIDTH;
        let from = DOCKS_FROM + width + BORDER_WIDTH;
        let at = |room_width: f32, over| {
            Frame::of(room(room_width), Some(PaneWidth::LEAST), Drawer::Shut, members(MembersShown::Open, over, None))
        };
        let docked = at(from, MembersOver::Shut);
        assert_eq!(docked.members, Some(MembersColumn::Docked(width)));
        assert_eq!(docked.content, CONTENT_LEAST, "the content keeps exactly its least");
        assert_eq!(at(from - 1.0, MembersOver::Shut).members, None, "a pixel less: it drops out");
        assert_eq!(at(from - 1.0, MembersOver::Open).members, Some(MembersColumn::Over(width)), "over, when asked");
        assert_eq!(at(from - 1.0, MembersOver::Open).content, from - 1.0 - RAIL - PANE_LEAST - BORDER_WIDTH);
        let closed = members(MembersShown::Closed, MembersOver::Shut, None);
        assert_eq!(Frame::of(room(1280.0), Some(PaneWidth::LEAST), Drawer::Shut, closed).members, None);
        let folded = Frame::of(
            room(600.0),
            Some(PaneWidth::LEAST),
            Drawer::Shut,
            members(MembersShown::Open, MembersOver::Shut, None),
        );
        assert_eq!(folded.members, None, "never docked beside a pane in a drawer");
    }

    #[test]
    fn in_a_call_the_members_column_drops_out_where_the_grid_would_lose_its_least_card() {
        let room = Room { width: 1000.0, height: 400.0 };
        let ask = |cards| members(MembersShown::Open, MembersOver::Shut, Some(cards));
        let none = Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, None);
        let beside = none.content - design::MEMBERS_WIDTH - BORDER_WIDTH;
        assert!(call_fits(1, beside, room.height), "one card fits beside the column");
        assert!(Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, ask(1)).members_dock());
        let crowd = (1..40)
            .find(|cards| call_fits(*cards, none.content, room.height) && !call_fits(*cards, beside, room.height));
        let crowd = crowd.expect("a call that fits only without the column");
        assert!(!Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, ask(crowd)).members_dock(), "{crowd} cards");
        let crammed =
            (1..200).find(|cards| !call_fits(*cards, none.content, room.height)).expect("a call that never fits");
        assert!(
            Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, ask(crammed)).members_dock(),
            "a call that scrolls either way keeps the column"
        );
    }

    #[test]
    fn a_calls_chat_docks_while_the_grid_keeps_its_least_card_beside_it_else_goes_over_it_in_the_members_place() {
        let room = Room { width: 1400.0, height: 500.0 };
        let kept = CallChat { shown: CallChatShown::Open, ..CallChat::default() };
        let ask = |cards, over| Some(CallChatAsk { kept, over, cards });
        let call = |cards| members(MembersShown::Open, MembersOver::Shut, Some(cards));
        let plain = Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, None);
        let beside = plain.content - design::CALL_CHAT_LEAST - BORDER_WIDTH;
        let docked = Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, call(1))
            .with_call_chat(room, ask(1, MembersOver::Shut));
        assert_eq!(docked.call_chat, Some(MembersColumn::Docked(design::CALL_CHAT_LEAST)));
        assert_eq!(docked.content, beside, "the call is narrowed by the column alone");
        assert_eq!(docked.members, None, "the members column gives way to it");
        let crowd = (1..60)
            .find(|cards| call_fits(*cards, plain.content, room.height) && !call_fits(*cards, beside, room.height))
            .expect("a call that fits only without the column");
        let crowded = Frame::of(room, Some(PaneWidth::LEAST), Drawer::Shut, None);
        assert_eq!(crowded.with_call_chat(room, ask(crowd, MembersOver::Shut)).call_chat, None, "{crowd} cards");
        assert_eq!(
            crowded.with_call_chat(room, ask(crowd, MembersOver::Open)).call_chat,
            Some(MembersColumn::Over(design::CALL_CHAT_LEAST)),
            "over the call once its button opened it"
        );
        let narrow = Room { width: 670.0, height: 600.0 };
        let folded = Frame::of(narrow, Some(PaneWidth::LEAST), Drawer::Shut, None);
        let over = folded.with_call_chat(narrow, ask(1, MembersOver::Open)).call_chat;
        assert_eq!(over, Some(MembersColumn::Over(design::CALL_CHAT_LEAST)), "a folded frame never docks it");
        let closed = CallChatAsk { kept: CallChat::default(), over: MembersOver::Shut, cards: 1 };
        assert_eq!(plain.with_call_chat(room, Some(closed)).call_chat, None);
    }

    fn thread_frame(content: f32, members: Option<MembersColumn>) -> Frame {
        Frame { content, members, ..Frame::of(room(1280.0), Some(PaneWidth::LEAST), Drawer::Shut, None) }
    }

    #[test]
    fn a_thread_sits_beside_the_channel_exactly_where_the_channel_keeps_its_least_else_covers_it() {
        let from = THREAD_WIDTH + BORDER_WIDTH + THREAD_CHANNEL_LEAST;
        let docked = Some(MembersColumn::Docked(design::MEMBERS_WIDTH));
        for members in [None, docked] {
            let at = |content| thread_frame(content, members).thread_column();
            let beside = at(from);
            assert!(!beside.covers && beside.channel == THREAD_CHANNEL_LEAST, "at: {beside:?}");
            assert_eq!(beside.members, members.is_some(), "the members column stays while it fits");
        }
        let whole = |content| thread_frame(content, docked).thread_column();
        let gives_up = whole(from - 1.0);
        assert!(!gives_up.covers && !gives_up.members, "just below: the members column goes: {gives_up:?}");
        assert_eq!(gives_up.channel, from - 1.0 + design::MEMBERS_WIDTH + BORDER_WIDTH - THREAD_WIDTH - BORDER_WIDTH);
        let covers = thread_frame(from - 1.0, None).thread_column();
        assert!(covers.covers && covers.channel == 0.0 && covers.width == from - 1.0, "no room for both: {covers:?}");
        let over = thread_frame(from - 1.0, Some(MembersColumn::Over(design::MEMBERS_WIDTH))).thread_column();
        assert!(over.covers, "an over column gives the channel no room back");
        assert!(thread_frame(from + 1.0, docked).thread_column().members, "above: both stay");
    }

    #[test]
    fn the_members_column_shows_unless_a_thread_took_its_place() {
        let plain = thread_frame(1000.0, Some(MembersColumn::Docked(design::MEMBERS_WIDTH)));
        assert!(plain.members_shown(None));
        assert!(plain.members_shown(Some(plain.thread_column())));
        let tight = thread_frame(THREAD_WIDTH + BORDER_WIDTH + THREAD_CHANNEL_LEAST - 1.0, plain.members);
        assert!(!tight.members_shown(Some(tight.thread_column())));
        assert!(tight.members_shown(None));
    }

    #[test]
    fn the_top_room_is_the_bar_docked_and_the_pane_header_folded() {
        let docked = Frame::of(room(DOCKS_FROM), Some(PaneWidth::LEAST), Drawer::Shut, None).under_bar();
        assert_eq!(docked.top_room(), SERVER_BAR_HEIGHT + BORDER_WIDTH);
        let folded = Frame::of(room(DOCKS_FROM - 1.0), Some(PaneWidth::LEAST), Drawer::Shut, None).under_bar();
        assert_eq!(folded.top_room(), PANE_HEADER_HEIGHT + BORDER_WIDTH);
    }

    #[test]
    fn the_rail_the_pane_and_the_members_take_their_room_only_where_docked() {
        let at = |width| Frame::of(room(width), Some(PaneWidth::LEAST), Drawer::Shut, None);
        assert_eq!(at(DOCKS_FROM).rail_room(), RAIL);
        assert_eq!(at(DOCKS_FROM).pane_room(), PANE_LEAST + BORDER_WIDTH);
        assert_eq!(at(DOCKS_FROM - 1.0).rail_room(), RAIL);
        assert_eq!(at(DOCKS_FROM - 1.0).pane_room(), 0.0);
        assert_eq!(at(RAIL_FROM).pane_room(), 0.0);
        assert_eq!(at(RAIL_FROM - 1.0).rail_room(), 0.0);
        let docked = thread_frame(1000.0, Some(MembersColumn::Docked(design::MEMBERS_WIDTH)));
        assert_eq!(docked.members_room(), design::MEMBERS_WIDTH + BORDER_WIDTH);
        assert_eq!(thread_frame(1000.0, Some(MembersColumn::Over(design::MEMBERS_WIDTH))).members_room(), 0.0);
    }

    #[test]
    fn messages_keep_a_narrower_side_below_the_narrow_width_but_a_thread_never_does() {
        use crate::Pane;
        for pane in [Pane::Content, Pane::CallChat] {
            assert_eq!(message_side(pane, MESSAGE_SIDE_NARROW_BELOW - 1.0), MESSAGE_SIDE_NARROW);
            assert_eq!(message_side(pane, MESSAGE_SIDE_NARROW_BELOW), MESSAGE_SIDE);
            assert_eq!(message_side(pane, MESSAGE_SIDE_NARROW_BELOW + 1.0), MESSAGE_SIDE);
        }
        assert_eq!(message_side(Pane::Thread, 100.0), Space::L.px());
    }

    #[test]
    fn the_call_bar_keeps_its_room_only_while_always_shown() {
        assert_eq!(bar_room(Controls::Always), STAGE_BAR_ROOM);
        assert_eq!(bar_room(Controls::WhileHovered), Space::L.px());
    }

    #[test]
    fn the_door_keeps_its_side_exactly_where_the_side_and_the_form_fit_beside_each_other() {
        assert!(!door_side_by_side(SIDE_BY_SIDE_FROM - 1.0));
        assert!(door_side_by_side(SIDE_BY_SIDE_FROM));
        assert!(door_side_by_side(SIDE_BY_SIDE_FROM + 1.0));
    }

    #[test]
    fn the_splitter_never_drags_the_content_under_its_least() {
        let frame = Frame::of(room(800.0), Some(PaneWidth::LEAST), Drawer::Shut, None);
        assert_eq!(frame.pane_most, 800.0 - RAIL - BORDER_WIDTH - CONTENT_LEAST);
        assert_eq!(Frame::of(room(2000.0), Some(PaneWidth::LEAST), Drawer::Shut, None).pane_most, PANE_MOST);
    }
}
