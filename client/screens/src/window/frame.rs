//! How the window is laid out in the room it has: the pane, its drawer and splitter, and settings folded.

use super::*;

/// Which of folded settings shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Folded {
    /// The list of pages.
    List,
    /// The page open.
    Page,
}

/// How the window is laid out in the room it has.
#[derive(Debug)]
pub(super) struct Layout {
    /// The room the window last gave its frame; none until the host first says it.
    pub(super) room: Option<Room>,
    /// Whether the pane's drawer is over the content while the frame is folded.
    pub(super) drawer: Drawer,
    /// Whether the members column is over the content where it cannot dock.
    pub(super) members_over: MembersOver,
    /// Whether a call's chat column is over the call where it cannot dock.
    pub(super) call_chat_over: MembersOver,
    /// While settings are folded, whether the list of pages shows or the page open.
    pub(super) settings: Folded,
    /// Where the floating card is, for as long as the window runs.
    pub(super) float_at: FloatAt,
    /// Whether the floating card was closed since the call's view was last shown.
    pub(super) away: Away,
    /// Whether the window shows on the display: not minimised, covered whole or in the tray.
    pub(super) shown: bool,
    /// The watched screens the call was last told the window shows nowhere.
    pub(super) told_unseen: Vec<(MemberId, Video)>,
    /// Files are dragged over the window.
    pub(super) dragging: bool,
}

impl Default for Layout {
    fn default() -> Layout {
        Layout {
            room: None,
            drawer: Drawer::Shut,
            members_over: MembersOver::Shut,
            call_chat_over: MembersOver::Shut,
            settings: Folded::Page,
            float_at: FloatAt::HOME,
            away: Away::Shown,
            shown: true,
            told_unseen: Vec::new(),
            dragging: false,
        }
    }
}

impl Window {
    /// The pane's channels list pressed closed: the drawer is put away.
    pub(super) fn on_close_channels(&mut self) -> Vec<Ask> {
        self.layout.drawer = Drawer::Shut;
        Vec::new()
    }

    /// Back to the list of settings pages, while they are folded.
    pub(super) fn on_settings_list(&mut self) -> Vec<Ask> {
        self.layout.settings = Folded::List;
        Vec::new()
    }

    /// The room the window gives its frame now, told before each turn. Grown out of a folded shape, the drawer is put
    /// away, and grown to where a column docks, its place over the content, so neither opens again by itself when the
    /// window narrows.
    pub fn set_room(&mut self, room: Room) {
        self.layout.room = Some(room);
        if Frame::shape(room.width) == Shape::Docked {
            self.layout.drawer = Drawer::Shut;
        }
        if self.members_dock(room) {
            self.layout.members_over = MembersOver::Shut;
        }
        if self.call_chat_docks(room) {
            self.layout.call_chat_over = MembersOver::Shut;
        }
    }

    /// The frame in `room`: the room the view is drawn in, which the host also told [`Window::set_room`].
    pub fn frame(&self, room: Room) -> Frame {
        let pane = self.open().filter(|place| place.shows_pane()).map(|_| self.voice.look.pane);
        let frame =
            Frame::of(room, pane, self.layout.drawer, self.members_ask()).with_call_chat(room, self.call_chat_ask());
        match self.bar(room) {
            Some(_) => frame.under_bar(),
            None => frame,
        }
    }

    /// The server whose top bar runs across the pane and the content in `room`: the open one, while its channels, a
    /// discussion, a list or its call shows beside the docked pane. Folded, the bar goes with the pane and each view's
    /// header holds its controls; a page in place of the pane and the content has none.
    pub fn bar(&self, room: Room) -> Option<&Place> {
        if self.device().is_some() || Frame::shape(room.width) != Shape::Docked {
            return None;
        }
        self.open().filter(|place| {
            matches!(
                place.panel(),
                Panel::Channels | Panel::Discussion | Panel::Threads | Panel::Conversations | Panel::Call
            )
        })
    }

    /// What a call's chat column asks of the frame: beside the call the open server shows, never over a screen shown
    /// whole.
    fn call_chat_ask(&self) -> Option<CallChatAsk> {
        let place = self.open().filter(|place| place.panel() == Panel::Call && self.full_screen().is_none())?;
        let cards = place.call().map_or(0, |call| call.cards().len());
        Some(CallChatAsk { kept: self.voice.look.call_chat, over: self.layout.call_chat_over, cards })
    }

    /// Whether a call's chat column, opened, docks in `room`.
    fn call_chat_docks(&self, room: Room) -> bool {
        let opened = self.call_chat_ask().map(|ask| CallChatAsk {
            kept: choices::CallChat { shown: choices::CallChatShown::Open, ..ask.kept },
            over: MembersOver::Shut,
            ..ask
        });
        self.frame(room).with_call_chat(room, opened).call_chat_docks()
    }

    /// The call header's Chat button: open or closed, kept, beside the call where the column docks and over it where
    /// it cannot. Open, it stands in the members column's place, which returns when it closes.
    pub(super) fn toggle_call_chat(&mut self) -> Vec<Ask> {
        if !self.layout.room.is_some_and(|room| self.call_chat_docks(room)) {
            self.layout.call_chat_over = self.layout.call_chat_over.toggled();
            self.voice.look.call_chat.shown = match self.layout.call_chat_over {
                MembersOver::Open => choices::CallChatShown::Open,
                MembersOver::Shut => choices::CallChatShown::Closed,
            };
            return vec![Ask::Keep(Keep::Look(self.voice.look))];
        }
        self.voice.look.call_chat.shown = self.voice.look.call_chat.shown.toggled();
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }

    /// The call's chat put away, wherever it shows, and kept closed.
    pub(super) fn close_call_chat(&mut self) -> Vec<Ask> {
        self.layout.call_chat_over = MembersOver::Shut;
        match self.voice.look.call_chat.shown {
            choices::CallChatShown::Open => {
                self.voice.look.call_chat.shown = choices::CallChatShown::Closed;
                vec![Ask::Keep(Keep::Look(self.voice.look))]
            }
            choices::CallChatShown::Closed => Vec::new(),
        }
    }

    /// A call's chat column's splitter moved, by a drag or a key: kept once a drag is let go.
    pub(super) fn call_chat_width(&mut self, pick: PanePick) -> Vec<Ask> {
        let now = self.voice.look.call_chat.width.px();
        let width = match pick {
            PanePick::To(px) => {
                self.voice.look.call_chat.width = choices::CallChatWidth::new(px);
                return Vec::new();
            }
            PanePick::Let => self.voice.look.call_chat.width,
            PanePick::Narrower => choices::CallChatWidth::new(now - PANE_STEP),
            PanePick::Wider => choices::CallChatWidth::new(now + PANE_STEP),
            PanePick::Reset => choices::CallChatWidth::START,
        };
        self.voice.look.call_chat.width = width;
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }

    /// What the members column asks of the frame: shown beside everything a signed-in server's content area shows, the
    /// call and the empty panel included, never over a screen shown whole.
    fn members_ask(&self) -> Option<MembersAsk> {
        let place = self.open().filter(|place| place.signed_in())?;
        let call = match place.panel() {
            Panel::Call if self.full_screen().is_none() => Some(place.call().map_or(0, |call| call.cards().len())),
            Panel::Channels | Panel::Discussion | Panel::Threads | Panel::Conversations => None,
            Panel::Call
            | Panel::Connecting
            | Panel::Lost
            | Panel::NotAnswering { .. }
            | Panel::SignedOut
            | Panel::Deleted
            | Panel::KeyChanged
            | Panel::NewPassword
            | Panel::Settings
            | Panel::Account => return None,
        };
        Some(MembersAsk { kept: self.voice.look.members, over: self.layout.members_over, call })
    }

    /// The members column's toggle: open or closed, kept, beside the content where it docks and over it where it
    /// cannot.
    pub(super) fn toggle_members(&mut self) -> Vec<Ask> {
        // One column at a time beside a call: asked for while the chat shows, the members take its place.
        if self.layout.room.is_some_and(|room| self.frame(room).call_chat.is_some()) {
            let mut asks = self.close_call_chat();
            if self.layout.room.is_some_and(|room| self.frame(room).members.is_none()) {
                asks.extend(self.flip_members());
            }
            return asks;
        }
        self.flip_members()
    }

    /// Whether the members column, opened, docks in `room`.
    fn members_dock(&self, room: Room) -> bool {
        let docked =
            self.members_ask().map(|ask| MembersAsk { kept: Members { shown: MembersShown::Open, ..ask.kept }, ..ask });
        let pane = self.open().filter(|place| place.shows_pane()).map(|_| self.voice.look.pane);
        Frame::of(room, pane, self.layout.drawer, docked).members_dock()
    }

    /// The members column opened where it is closed and closed where it is open.
    fn flip_members(&mut self) -> Vec<Ask> {
        if !self.layout.room.is_some_and(|room| self.members_dock(room)) {
            self.layout.members_over = self.layout.members_over.toggled();
            self.voice.look.members.shown = match self.layout.members_over {
                MembersOver::Open => MembersShown::Open,
                MembersOver::Shut => MembersShown::Closed,
            };
            return vec![Ask::Keep(Keep::Look(self.voice.look))];
        }
        self.voice.look.members.shown = self.voice.look.members.shown.toggled();
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }

    /// The members column over the content put away, and kept closed.
    pub(super) fn close_members(&mut self) -> Vec<Ask> {
        if self.layout.members_over == MembersOver::Shut {
            return Vec::new();
        }
        self.layout.members_over = MembersOver::Shut;
        self.voice.look.members.shown = MembersShown::Closed;
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }

    /// The members column's splitter moved, by a drag or a key: kept once a drag is let go.
    pub(super) fn members_width(&mut self, pick: PanePick) -> Vec<Ask> {
        let now = self.voice.look.members.width.px();
        let width = match pick {
            PanePick::To(px) => {
                self.voice.look.members.width = MembersWidth::new(px);
                return Vec::new();
            }
            PanePick::Let => self.voice.look.members.width,
            PanePick::Narrower => MembersWidth::new(now - PANE_STEP),
            PanePick::Wider => MembersWidth::new(now + PANE_STEP),
            PanePick::Reset => MembersWidth::START,
        };
        self.voice.look.members.width = width;
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }

    /// What settings show in `frame`: the list beside the page where both fit, else the list or the page, as the person
    /// last went.
    pub fn settings_shown(&self, frame: &Frame) -> SettingsShown {
        match (frame.settings_room() >= SETTINGS_BESIDE_FROM, self.layout.settings) {
            (true, _) => SettingsShown::Beside,
            (false, Folded::List) => SettingsShown::List,
            (false, Folded::Page) => SettingsShown::Page,
        }
    }

    /// What the open roles page shows in `frame`: the list beside the chosen role where both fit the page, else the
    /// list or the role, as the person last went; none while no roles page is open.
    pub fn roles_shown(&self, frame: &Frame) -> Option<RolesShown> {
        let roles = self.open()?.roles()?;
        if frame.settings_page(self.settings_shown(frame)) >= ROLES_BESIDE_FROM {
            return Some(RolesShown::Beside);
        }
        Some(match roles.step() {
            crate::roles::Step::List => RolesShown::List,
            crate::roles::Step::Role => RolesShown::Role,
        })
    }

    /// What the open channels page shows in `frame`, as [`Window::roles_shown`] decides for roles; none while no
    /// channels page is open.
    pub fn channels_shown(&self, frame: &Frame) -> Option<crate::channel_settings::Shows> {
        use crate::channel_settings::{Shows, Step};
        let channels = self.open()?.channel_settings()?;
        // The same list's width beside the same form as the roles page's.
        if frame.settings_page(self.settings_shown(frame)) >= ROLES_BESIDE_FROM {
            return Some(Shows::Beside);
        }
        Some(match channels.step() {
            Step::List => Shows::List,
            Step::Channel => Shows::Channel,
        })
    }

    /// Whether the pane is in a drawer in the room last told.
    pub(super) fn folded(&self) -> bool {
        self.layout.room.is_some_and(|room| Frame::shape(room.width) != Shape::Docked)
    }

    /// What the content shows: the open server, its panel, the channel of its call, and this device's page over them.
    pub(super) fn showing(&self) -> (Option<PlaceId>, Option<Panel>, Option<ChannelId>, Option<DevicePage>) {
        let open = self.open();
        let call = open.and_then(Place::call).map(|call| call.channel.id.clone());
        (self.desk.open, open.map(Place::panel), call, self.hardware.device)
    }

    /// The pane's width set to `px`, held where the content keeps its least in the room last told.
    pub(super) fn pane_to(&mut self, px: f32) {
        let most = self.layout.room.map_or(design::PANE_MOST, |room| self.frame(room).pane_most);
        self.voice.look.pane = PaneWidth::new(px.min(most));
    }

    /// The drawer opened, or shut when it is open.
    pub(super) fn toggle_channels(&mut self) -> Vec<Ask> {
        self.layout.drawer = match self.layout.drawer {
            Drawer::Shut => Drawer::Open,
            Drawer::Open => Drawer::Shut,
        };
        Vec::new()
    }

    /// The splitter moved, by a drag or a key: kept once a drag is let go.
    pub(super) fn pane(&mut self, pick: PanePick) -> Vec<Ask> {
        let now = self.voice.look.pane.px();
        match pick {
            PanePick::To(px) => {
                self.pane_to(px);
                return Vec::new();
            }
            PanePick::Let => {}
            PanePick::Narrower => self.pane_to(now - PANE_STEP),
            PanePick::Wider => self.pane_to(now + PANE_STEP),
            PanePick::Reset => self.voice.look.pane = PaneWidth::LEAST,
        }
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, account_name, id, in_call, tell, two_signed_in};

    const NARROW: Room = Room { width: 480.0, height: 360.0 };

    const WIDE: Room = Room { width: 1280.0, height: 800.0 };

    #[test]
    fn a_narrow_channels_page_shows_the_list_then_a_channel_opened_from_it_and_back() {
        use crate::channel_settings::{Message as Channels, Shows};
        let clock = Clock(Instant::now());
        let words = proto::rules::Grants::from_words(&["MANAGE_CHANNEL"]);
        let (mut window, owls) = crate::harness::placed_holding(&clock, crate::harness::ADMIN_PLACE, words);
        let page = |message| place::Message::Page(place::PageMessage::Channels(message));
        let narrow = Frame::of(Room { width: 360.0, height: 800.0 }, None, Drawer::Shut, None);
        assert_eq!(window.channels_shown(&narrow), None, "no channels page open");
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::Settings(crate::Page::Channels)));
        let wide = window.frame(WIDE);
        assert_eq!(window.channels_shown(&wide), Some(Shows::Beside));
        assert_eq!(window.channels_shown(&narrow), Some(Shows::List));
        let general = crate::harness::channel_id(crate::harness::GENERAL);
        tell(&mut window, &clock, owls, page(Channels::Choose(general)));
        assert_eq!(window.channels_shown(&narrow), Some(Shows::Channel));
        assert_eq!(window.channels_shown(&wide), Some(Shows::Beside));
        tell(&mut window, &clock, owls, page(Channels::Back));
        assert_eq!(window.channels_shown(&narrow), Some(Shows::List));
    }

    fn drawer_open(window: &Window, room: Room) -> bool {
        window.frame(room).drawer_open()
    }

    #[test]
    fn a_server_picked_in_the_rail_while_the_pane_is_a_drawer_opens_its_channels_and_the_open_one_toggles_them() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.set_room(NARROW);
        let (owls, books) = (id(&window, 0), id(&window, 1));
        window.update(Message::Rail(books), clock.at());
        assert_eq!(window.open().map(Place::id), Some(books));
        assert!(drawer_open(&window, NARROW), "another server opens on its channels");
        window.update(Message::Rail(books), clock.at());
        assert!(!drawer_open(&window, NARROW), "the open server's picture puts them away");
        window.update(Message::Rail(books), clock.at());
        assert!(drawer_open(&window, NARROW), "and brings them back");
        window.update(Message::Rail(owls), clock.at());
        assert!(drawer_open(&window, NARROW), "a third server keeps them open, now its own");
        window.set_room(WIDE);
        window.update(Message::Rail(books), clock.at());
        assert_eq!(window.open().map(Place::id), Some(books));
        assert!(!drawer_open(&window, WIDE), "a docked pane has no drawer to open");
    }

    #[test]
    fn a_server_picked_in_the_rail_leaves_every_page_and_a_name_typed_there_is_saved() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.set_room(WIDE);
        let (owls, books) = (id(&window, 0), id(&window, 1));
        let page = |message| place::Message::Page(message);
        let opens =
            [Message::At(owls, page(place::PageMessage::OpenAccount)), Message::OpenDevice(DevicePage::Appearance)];
        for open in opens {
            window.update(Message::Open(owls), clock.at());
            window.update(open, clock.at());
            assert!(window.page_shown());
            window.update(Message::Rail(owls), clock.at());
            assert!(!window.page_shown(), "the open server's picture only leaves");
            assert_eq!(window.open().unwrap().panel(), Panel::Discussion);
        }
        window.update(Message::Pick(owls, Pick::Settings), clock.at());
        tell(&mut window, &clock, books, page(place::PageMessage::OpenAccount));
        window.update(Message::Rail(books), clock.at());
        assert_eq!(window.open().map(Place::id), Some(books));
        assert_eq!(window.open().unwrap().panel(), Panel::Discussion, "another server opens on its first channel");
        assert_eq!(window.place(owls).unwrap().panel(), Panel::Discussion, "and the one left keeps no page open");
        tell(&mut window, &clock, books, page(place::PageMessage::OpenAccount));
        account_name(&mut window, &clock, books, crate::name_field::Message::Typed("Countess".into()));
        let todo = window.update(Message::Rail(owls), clock.at());
        assert!(
            todo.iter()
                .any(|done| matches!(done, Ask::At(at, ask) if *at == books && matches!(ask.op(), Some(crate::harness::Body::SetDisplayName(set)) if set.display_name.is_some()))),
            "{todo:?}"
        );
    }

    #[test]
    fn a_choice_that_changes_the_content_and_growing_out_of_the_fold_put_the_drawer_away() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        window.set_room(NARROW);
        window.update(Message::ToggleChannels, clock.at());
        assert!(drawer_open(&window, NARROW));
        in_call(&mut window, &clock, owls);
        assert!(!drawer_open(&window, NARROW), "joining a channel shows the call");
        window.update(Message::ToggleChannels, clock.at());
        window.update(Message::CloseChannels, clock.at());
        assert!(!drawer_open(&window, NARROW), "the scrim or Escape");
        window.update(Message::ToggleChannels, clock.at());
        window.set_room(Room { width: 669.0, ..NARROW });
        assert!(drawer_open(&window, Room { width: 669.0, ..NARROW }), "still folded");
        window.set_room(Room { width: 670.0, ..NARROW });
        window.set_room(NARROW);
        assert!(!drawer_open(&window, NARROW), "docked once, it does not come back by itself");
    }

    #[test]
    fn the_splitter_keeps_a_drag_once_let_go_and_never_takes_the_contents_least() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let room = Room { width: 800.0, height: 600.0 };
        window.set_room(room);
        assert!(window.update(Message::Pane(PanePick::To(300.0)), clock.at()).is_empty(), "nothing kept mid-drag");
        assert_eq!(window.look().pane.whole(), 300);
        assert_eq!(window.frame(room).pane, Some(300.0));
        window.update(Message::Pane(PanePick::To(5000.0)), clock.at());
        let most = 800.0 - design::RAIL_WIDTH - 2.0 * design::BORDER_WIDTH - design::CONTENT_LEAST;
        assert_eq!(window.look().pane.px(), most, "the content keeps its least");
        let kept = window.update(Message::Pane(PanePick::Let), clock.at());
        assert!(matches!(&kept[..], [Ask::Keep(Keep::Look(look))] if look.pane.px() == most), "{kept:?}");
        window.update(Message::Pane(PanePick::Narrower), clock.at());
        assert_eq!(window.look().pane.px(), most - PANE_STEP);
        window.update(Message::Pane(PanePick::Wider), clock.at());
        window.update(Message::Pane(PanePick::Wider), clock.at());
        assert_eq!(window.look().pane.px(), most, "a key at the end holds");
        let reset = window.update(Message::Pane(PanePick::Reset), clock.at());
        assert_eq!(window.look().pane, PaneWidth::LEAST);
        assert!(matches!(&reset[..], [Ask::Keep(Keep::Look(_))]), "{reset:?}");
    }

    #[test]
    fn the_members_toggle_is_kept_docked_and_over_the_content_alike() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.set_room(WIDE);
        assert_eq!(window.frame(WIDE).members, Some(crate::frame::MembersColumn::Docked(design::MEMBERS_WIDTH)));
        let kept = window.update(Message::ToggleMembers, clock.at());
        assert!(
            matches!(&kept[..], [Ask::Keep(Keep::Look(look))] if look.members.shown == MembersShown::Closed),
            "{kept:?}"
        );
        assert_eq!(window.frame(WIDE).members, None);
        window.update(Message::ToggleMembers, clock.at());
        let narrow = Room { width: 800.0, height: 600.0 };
        window.set_room(narrow);
        assert_eq!(window.frame(narrow).members, None, "no room beside the content");
        let over = window.update(Message::ToggleMembers, clock.at());
        assert!(
            matches!(&over[..], [Ask::Keep(Keep::Look(look))] if look.members.shown == MembersShown::Open),
            "over the content, the same choice: {over:?}"
        );
        assert_eq!(window.frame(narrow).members, Some(crate::frame::MembersColumn::Over(design::MEMBERS_WIDTH)));
        window.update(Message::CloseMembers, clock.at());
        assert_eq!(window.frame(narrow).members, None);
        assert_eq!(window.look().members.shown, MembersShown::Closed, "put away, kept closed");
        assert!(window.update(Message::CloseMembers, clock.at()).is_empty(), "shut already, nothing to keep");
        window.set_room(WIDE);
        assert!(window.update(Message::MembersWidth(PanePick::To(500.0)), clock.at()).is_empty(), "kept once let go");
        assert_eq!(window.look().members.width.px(), design::MEMBERS_MOST);
        let kept = window.update(Message::MembersWidth(PanePick::Let), clock.at());
        assert!(matches!(&kept[..], [Ask::Keep(Keep::Look(_))]), "{kept:?}");
        window.update(Message::MembersWidth(PanePick::Narrower), clock.at());
        assert_eq!(window.look().members.width.px(), design::MEMBERS_MOST - PANE_STEP);
        window.update(Message::MembersWidth(PanePick::Reset), clock.at());
        assert_eq!(window.look().members.width, MembersWidth::START);
        let owls = id(&window, 0);
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.frame(WIDE).members, None, "a page over the channels shows none");
    }

    #[test]
    fn the_members_column_closed_in_a_narrow_window_stays_closed_once_it_widens() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let narrow = Room { width: 800.0, height: 600.0 };
        window.set_room(narrow);
        window.update(Message::ToggleMembers, clock.at());
        let shut = window.update(Message::ToggleMembers, clock.at());
        assert!(
            matches!(&shut[..], [Ask::Keep(Keep::Look(look))] if look.members.shown == MembersShown::Closed),
            "{shut:?}"
        );
        window.set_room(WIDE);
        assert_eq!(window.frame(WIDE).members, None, "closed by the toggle");
        window.update(Message::ToggleMembers, clock.at());
        window.set_room(narrow);
        window.update(Message::ToggleMembers, clock.at());
        let closed = window.update(Message::CloseMembers, clock.at());
        assert!(matches!(&closed[..], [Ask::Keep(_)]), "{closed:?}");
        window.set_room(WIDE);
        assert_eq!(window.frame(WIDE).members, None, "closed by a press beside it");
        window.update(Message::ToggleMembers, clock.at());
        assert!(window.frame(WIDE).members_dock(), "open again");
        window.set_room(narrow);
        assert_eq!(window.frame(narrow).members, None, "narrowed, it goes, and does not come back over the content");
    }

    #[test]
    fn the_call_chat_closed_over_the_call_stays_closed_once_the_window_widens() {
        let clock = Clock(Instant::now());
        let (mut window, _) = crate::harness::ada_in_call(&clock);
        let narrow = Room { width: 800.0, height: 600.0 };
        window.set_room(narrow);
        window.update(Message::ToggleCallChat, clock.at());
        assert!(
            window
                .frame(narrow)
                .call_chat
                .is_some_and(|column| !matches!(column, crate::frame::MembersColumn::Docked(_)))
        );
        let closed = window.update(Message::CloseCallChat, clock.at());
        assert!(
            matches!(&closed[..], [Ask::Keep(Keep::Look(look))] if look.call_chat.shown == choices::CallChatShown::Closed),
            "{closed:?}"
        );
        let wide = Room { width: 1440.0, height: 900.0 };
        window.set_room(wide);
        assert_eq!(window.frame(wide).call_chat, None, "closed over the call, closed beside it");
    }

    #[test]
    fn the_members_column_stands_beside_every_view_of_a_signed_in_server() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::chatting(&clock);
        crate::harness::every_switch_on(&mut window, &clock, owls);
        window.set_room(WIDE);
        let docked = Some(crate::frame::MembersColumn::Docked(design::MEMBERS_WIDTH));
        assert_eq!(window.open().map(Place::panel), Some(Panel::Discussion));
        assert_eq!(window.frame(WIDE).members, docked, "a text channel");
        for roll in [place::Roll::Threads, place::Roll::Conversations] {
            tell(&mut window, &clock, owls, place::Message::Talk(place::TalkMessage::Show(roll)));
            assert_eq!(window.frame(WIDE).members, docked, "the list {roll:?}");
        }
        in_call(&mut window, &clock, owls);
        assert_eq!(window.open().map(Place::panel), Some(Panel::Call));
        assert_eq!(window.frame(WIDE).members, docked, "the call");
        let mut window = two_signed_in(&clock);
        window.set_room(WIDE);
        assert_eq!(window.open().map(Place::panel), Some(Panel::Discussion), "the first text channel opens");
        assert_eq!(window.frame(WIDE).members, docked, "the first text channel");
    }

    #[test]
    fn the_top_bar_runs_over_a_docked_pane_and_content_and_goes_folded_and_under_a_page() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::chatting(&clock);
        let bar = design::SERVER_BAR_HEIGHT + design::BORDER_WIDTH;
        assert_eq!(window.bar(WIDE).map(Place::id), Some(owls), "over a text channel");
        assert_eq!(window.frame(WIDE).top, bar);
        assert_eq!((window.bar(NARROW).map(Place::id), window.frame(NARROW).top), (None, 0.0), "folded: none");
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!((window.bar(WIDE).map(Place::id), window.frame(WIDE).top), (None, 0.0), "under a page: none");
        window.update(Message::Rail(owls), clock.at());
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        assert_eq!((window.bar(WIDE).map(Place::id), window.frame(WIDE).top), (None, 0.0), "a device's page: none");
    }

    #[test]
    fn folded_settings_show_the_page_picked_and_back_shows_the_list() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        let narrow = Frame::of(NARROW, Some(PaneWidth::LEAST), Drawer::Shut, None);
        let wide = Frame::of(WIDE, Some(PaneWidth::LEAST), Drawer::Shut, None);
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.settings_shown(&narrow), SettingsShown::Page, "opened from anywhere: the page");
        assert_eq!(window.settings_shown(&wide), SettingsShown::Beside);
        window.update(Message::SettingsList, clock.at());
        assert_eq!(window.settings_shown(&narrow), SettingsShown::List);
        assert_eq!(window.settings_shown(&wide), SettingsShown::Beside, "wide, the list is always beside");
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.settings_shown(&narrow), SettingsShown::Page, "a page picked in the list");
        window.update(Message::SettingsList, clock.at());
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.settings_shown(&narrow), SettingsShown::Page, "the page already open, picked again");
    }
}
