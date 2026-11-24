//! What a call holds, and how this place keeps hold of it as the room, the tiles and the controls change.

use super::*;

/// Where a call's ending stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ending {
    /// Its loop runs and nobody asked to leave.
    Live,
    /// The person asked to leave: the call goes when its loop returns.
    Leaving,
    /// The host's loop for it returned: nothing runs for it any more.
    Over,
}

/// Whether a call reads the loudness of who it hears, as the window last told it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Levels {
    /// Read, as every call starts.
    Read,
    /// Not read: the window does not show.
    Unread,
}

/// A call on a voice channel of this server: its room, tiles, shares, notices and what is open over it.
#[derive(Debug)]
pub struct Call {
    /// The voice channel the call is on, renamed with it.
    pub channel: Channel,
    pub(crate) room: Room,
    pub(super) tiles: Vec<Tile>,
    pub(super) me: Option<MemberId>,
    /// Whether the host's loop runs, and whether the person asked to leave.
    pub(super) ending: Ending,
    /// The microphones and speakers there are and which the call uses, once the call has said.
    pub(super) sound: Option<Sound>,
    pub(super) open: Option<Opened>,
    /// How the connection was at the last look, once the call has said.
    pub(super) health: Option<Health>,
    /// The round trips of the looks so far, for the connection's graph.
    pub(super) round_trips: RoundTrips,
    /// The looks so far, for the figures of the call once it is over.
    pub(super) looks: crate::call::health::CallLooks,
    /// What failed of the microphone, camera and share, until it is pressed or works again.
    pub(crate) faults: Faults,
    pub(super) sharing: Option<Sharing>,
    /// What each screen shared here shows, by its sharer's id, once the call has said.
    pub(super) screens: BTreeMap<MemberId, Received>,
    /// Why a screen's sound is not heard here while its picture plays, by its sharer's id: a track's end ends only
    /// that track.
    pub(super) unheard: BTreeMap<MemberId, Failure>,
    /// Why a camera is not shown on this device, by its sender's id: its card stays a person's tile.
    pub(super) unshown_cameras: BTreeMap<MemberId, Failure>,
    /// This account's own camera, as the call last told.
    pub(super) own_camera: OwnCamera,
    /// The pictures of others the person paused, before or while watching them, by their senders' ids and kinds.
    pub(super) paused: std::collections::BTreeSet<(MemberId, Video)>,
    /// Whether anyone takes this account's own screen and camera, as the server last told.
    pub(super) own_watch: BTreeMap<Video, OwnWatch>,
    /// Which of this account's own share and camera the connection popover shows while both are sent, kept for the
    /// call.
    pub(super) own_shown: Video,
    /// The card that fills the stage, while one of `cards()` still is it.
    pub(super) pinned: Option<Pinned>,
    /// Whether the strip shows under a pinned card on a tall stage and on a short one, each kept for the call as cards
    /// are pinned and unpinned and the window is resized.
    pub(super) strip: (Strip, Strip),
    /// The watched screen that fills the whole display, while it plays.
    pub(super) full: Option<Full>,
    /// The zoom on the screen shown large, pinned or over the whole display, while it is on.
    pub(super) zoom: Option<Zooming>,
    /// Whose picture is in a window of its own, and which, while it plays: a screen or a camera.
    pub(super) out: Option<(MemberId, Video)>,
    /// How the next share goes out: the defaults when the panel opens, then what is picked there.
    pub(super) quality: ShareQuality,
    /// The Bitrate row as the share panel has it: a preset, or the custom rate as typed; `quality` holds its ceiling
    /// while it is one.
    pub(super) rate: PresetOrCustom<Ceiling>,
    /// What the grid leaves out, as the window keeps it.
    pub(super) view: CallView,
    /// Whether the call reads its loudness, as it was last told.
    pub(super) levels: Levels,
}

impl Call {
    pub(crate) fn new(channel: Channel, me: Option<MemberId>, view: CallView) -> Call {
        Call {
            channel,
            room: Room::new(me.clone()),
            tiles: Vec::new(),
            me,
            ending: Ending::Live,
            sound: None,
            open: None,
            health: None,
            round_trips: RoundTrips::default(),
            looks: crate::call::health::CallLooks::default(),
            faults: Faults::default(),
            sharing: None,
            screens: BTreeMap::new(),
            unheard: BTreeMap::new(),
            unshown_cameras: BTreeMap::new(),
            own_camera: OwnCamera::Off,
            paused: std::collections::BTreeSet::new(),
            own_watch: BTreeMap::new(),
            own_shown: Video::Screen,
            pinned: None,
            strip: (Strip::Shown, Strip::Hidden),
            full: None,
            zoom: None,
            out: None,
            quality: ShareQuality::default(),
            rate: PresetOrCustom::default(),
            view,
            levels: Levels::Read,
        }
    }

    /// Whether the room's snapshot has arrived: until it has, the call is being joined.
    pub fn joined(&self) -> bool {
        self.room.joined()
    }

    /// Whether nobody else is in the room.
    pub fn alone(&self) -> bool {
        self.tiles.len() <= 1
    }

    /// The people in the room, as the grid draws them.
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// How this device draws the call, as Voice options set it.
    pub fn view(&self) -> CallView {
        self.view
    }

    /// The tiles as the room is now: a camera this device cannot show leaves its person's tile with no picture.
    pub(crate) fn refresh_tiles(&mut self, now: Instant) {
        self.tiles = self.room.tiles(now);
        for tile in &mut self.tiles {
            let unshown = tile.user_id.as_ref().is_some_and(|user_id| self.unshown_cameras.contains_key(user_id));
            if unshown && tile.camera == crate::CameraSeen::Shown {
                tile.camera = crate::CameraSeen::Unshown;
            }
        }
        // A camera that went off is no longer paused: the next of theirs is a new one.
        let tiles = &self.tiles;
        self.paused.retain(|(user_id, video)| {
            *video == Video::Screen
                || tiles.iter().any(|tile| tile.user_id.as_ref() == Some(user_id) && tile.camera.pictured())
        });
        for tile in &mut self.tiles {
            let held = tile.user_id.as_ref().is_some_and(|id| self.paused.contains(&(id.clone(), Video::Camera)));
            if held && tile.camera == crate::CameraSeen::Shown {
                tile.camera = crate::CameraSeen::Paused;
            }
        }
    }

    /// Keeps what the call shows in step with what it holds, after anything that changes it: the own tile is live while
    /// its screen goes out, which the server never tells, a share stopped shows nothing, and a pin outlives no card.
    pub(crate) fn settle(&mut self) {
        let sending = self.sharing.is_some();
        if let Some(own) = self.tiles.iter_mut().find(|tile| tile.own()) {
            own.sharing = sending;
        }
        let gone: Vec<MemberId> = self.screens.keys().filter(|user_id| !self.shares(user_id)).cloned().collect();
        for user_id in gone {
            self.drop_screen(&user_id);
        }
        let stopped: Vec<MemberId> = self
            .paused
            .iter()
            .filter(|(user_id, video)| *video == Video::Screen && !self.shares(user_id))
            .map(|(user_id, _)| user_id.clone())
            .collect();
        for user_id in stopped {
            self.paused.remove(&(user_id, Video::Screen));
        }
        if let Some(pinned) = &self.pinned
            && !self.cards().iter().any(|card| card.pin().as_ref() == Some(pinned))
        {
            self.pinned = None;
        }
        if let Some(Opened::ShareSound(user_id)) = &self.open
            && !self.shares_sound(user_id)
        {
            self.open = None;
        }
        if matches!(self.open, Some(Opened::OwnPicture))
            && !self.cards().iter().any(|card| matches!(card, Card::OwnScreen(_)))
        {
            self.open = None;
        }
        if matches!(self.open, Some(Opened::OwnCamera)) && !self.own_camera_pictured() {
            self.open = None;
        }
        if let Some(full) = &self.full
            && !(self.live() && matches!(self.screens.get(&full.user_id), Some(Received::Playing(_))))
        {
            self.full = None;
        }
        if let Some((user_id, video)) = &self.out {
            let playing = match video {
                Video::Screen => matches!(self.screens.get(user_id), Some(Received::Playing(_))),
                Video::Camera => self.camera_pictured(user_id),
            };
            if !(self.live() && playing) {
                self.out = None;
            }
        }
        if self.large().is_none_or(|user_id| !matches!(self.screens.get(user_id), Some(Received::Playing(_)))) {
            self.zoom = None;
        }
    }

    /// Whether the host's loop is still running for this call. Another call is not begun until it is not: the loop and the
    /// connection it holds are the old call's till it returns.
    pub fn running(&self) -> bool {
        self.ending != Ending::Over
    }

    /// Whether the person asked to leave: the call goes when its loop returns.
    pub fn leaving(&self) -> bool {
        self.ending == Ending::Leaving
    }

    /// Still holding a voice place: joined or joining, and not ended.
    pub fn live(&self) -> bool {
        self.ended().is_none()
    }
}

impl Place {
    /// The call open here, if one is.
    pub fn call(&self) -> Option<&Call> {
        match &self.status {
            Status::Signed(signed) => signed.call.as_ref(),
            Status::Door(_) | Status::Reconnecting(_) => None,
        }
    }

    /// A call here whose loop returned is put away: another was joined, so what it said is over.
    pub(crate) fn put_away_ended(&mut self) -> Vec<Ask> {
        match &mut self.status {
            Status::Signed(signed) if signed.call.as_ref().is_some_and(|call| !call.running()) => {
                signed.end_call(&mut self.held, self.zone.clone())
            }
            Status::Signed(_) | Status::Door(_) | Status::Reconnecting(_) => Vec::new(),
        }
    }

    /// Whether a call here holds a voice place, which the rail marks on this server's picture.
    pub fn calling(&self) -> bool {
        self.live_call().is_some()
    }

    /// Whether the live call here is to be told that the window now shows or not (`shown`), which it is then taken
    /// to be.
    pub(crate) fn levels_news(&mut self, shown: bool) -> bool {
        let Status::Signed(signed) = &mut self.status else { return false };
        let Some(call) = signed.call.as_mut().filter(|call| call.live()) else { return false };
        let now = if shown { Levels::Read } else { Levels::Unread };
        std::mem::replace(&mut call.levels, now) != now
    }

    /// Whether a call is running here: its loop and the connection it holds are not given back yet.
    pub(crate) fn in_call(&self) -> bool {
        self.call().is_some_and(Call::running)
    }

    /// The channel of the call held here, if one is.
    pub(crate) fn live_call(&self) -> Option<&Channel> {
        self.call().filter(|call| call.live()).map(|call| &call.channel)
    }

    /// The call this account is in and the content area shows: joined, and not over. Its controls are offered only
    /// while it is, not while it is being joined or has ended.
    pub fn own_call(&self) -> Option<&Call> {
        self.call().filter(|call| self.panel() == Panel::Call && call.joined() && call.live())
    }

    /// What a call's grid leaves out, as the window keeps it: for the call here now and the next.
    pub(crate) fn set_call_view(&mut self, view: CallView) {
        self.call_view = view;
        if let Status::Signed(signed) = &mut self.status
            && let Some(call) = signed.call.as_mut()
        {
            call.view = view;
            call.settle();
        }
    }

    pub(super) fn room_said(&mut self, event: RoomEvent, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            // A level changes only who is heard, which only the tiles show.
            let drawn = matches!(event, RoomEvent::Levels(_)).then(|| call.tiles.clone());
            *told = call.room.on(event, at.now).map(Told::Chime);
            call.refresh_tiles(at.now);
            if drawn.is_some_and(|drawn| drawn == call.tiles) {
                *told = Some(Told::Unseen);
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::call::room::Event as RoomEvent;
    use crate::harness::{Clock, id, in_call, member, tell, two_signed_in};
    use crate::place;
    use crate::window::Window;
    use std::time::Instant;

    #[test]
    fn a_level_moves_what_is_drawn_only_when_it_changes_who_is_heard() {
        let mut clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        in_call(&mut window, &clock, owls_id);
        let level = |window: &mut Window, clock: &Clock, loud: u8| {
            let drawn = window.revision();
            let heard = RoomEvent::Levels(vec![(member("ada"), loud)]);
            tell(window, clock, owls_id, place::Message::Call(place::CallMessage::Room(heard)));
            window.revision() != drawn
        };
        assert!(!level(&mut window, &clock, 0), "silence that stays silence draws nothing");
        assert!(level(&mut window, &clock, 90), "Ada is heard");
        clock.pass(std::time::Duration::from_millis(100));
        assert!(!level(&mut window, &clock, 90), "and still is");
        clock.pass(crate::call::room::HOLD + std::time::Duration::from_millis(1));
        assert!(level(&mut window, &clock, 0), "her ring ends at the level after the hold");
        assert!(!level(&mut window, &clock, 0));
        let drawn = window.revision();
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::ToggleMute));
        assert_ne!(window.revision(), drawn, "any other message may change what is drawn");
    }
}
