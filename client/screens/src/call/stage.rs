//! What the stage shows of others' pictures: the cards, the one pinned, in a window of its own, over the whole display
//! or zoomed.

use super::*;

/// What a screen shows before the call has said anything of it.
static WAITING: Received = Received::Waiting;

/// What someone's shared screen shows here: every share plays in its card from its start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Received {
    /// Its first picture is awaited.
    #[default]
    Waiting,
    /// It plays, and how the picture is.
    Playing(ShareCard),
    /// This device cannot show it, and why.
    Unshown(Failure),
}

/// The watched screen that fills the whole display, and until when its controls and the pointer show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Full {
    pub(super) user_id: MemberId,
    calm_at: Instant,
}

/// The zoom on the screen shown large.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Zooming {
    lens: crate::Lens,
}

/// A watched screen that fills the whole display, while it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullScreen<'a> {
    /// Its sharer's id.
    pub user_id: &'a MemberId,
    /// Its sharer's name, as their card says it.
    pub name: &'a str,
    /// Its controls and the pointer show until then; a move of the pointer or a key puts it later.
    pub calm_at: Instant,
}

/// A share pinned on the stage that the floating card keeps showing while the call is out of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinnedShare<'a> {
    /// Someone else's screen, playing here.
    Someone(PinnedScreen<'a>),
    /// This account's own screen, going out, its preview shown.
    Own,
}

/// Someone else's screen pinned on the stage while it plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinnedScreen<'a> {
    /// Its sharer's id.
    pub user_id: &'a MemberId,
    /// Its sharer's name, as their card says it.
    pub name: &'a str,
    /// How its picture is.
    pub card: &'a ShareCard,
}

/// Someone else's screen or camera in a window of its own while it plays, drawn there alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Out<'a> {
    /// Whose it is.
    pub user_id: &'a MemberId,
    /// Their name, as their card says it.
    pub name: &'a str,
    /// Which of their pictures: a screen or their camera.
    pub video: Video,
    /// How a screen's picture is; none for a camera.
    pub card: Option<&'a ShareCard>,
    /// A screen's sound, so its window offers Volume; none for a camera and for a screen without sound.
    pub sound: Option<OutSound>,
}

/// The sound of a screen in a window of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutSound {
    /// Whether the popover of how loud it plays is open, in that window.
    pub open: bool,
}

impl Call {
    /// The grid's cards: each person, each followed by the screen they share; this account and its screen come first,
    /// as `tiles` does. What the view hides is left out: one's own share, then, while some card has a picture, every
    /// card without one.
    pub fn cards(&self) -> Vec<Card<'_>> {
        let mut cards = Vec::with_capacity(self.tiles.len() + 1);
        for tile in &self.tiles {
            cards.push(Card::Person(tile));
            if tile.own() {
                if let Some(sharing) = self.sharing.as_ref().filter(|_| self.view.own_share == OwnShare::Shown) {
                    cards.push(Card::OwnScreen(sharing.preview));
                }
            } else if let Some(user_id) = tile.user_id.as_ref().filter(|_| tile.sharing) {
                let received = self.screens.get(user_id).unwrap_or(&WAITING);
                cards.push(Card::Screen { sharer: tile, user_id, received });
            }
        }
        if self.view.people == People::Pictured && cards.iter().any(Card::pictured) {
            cards.retain(Card::pictured);
        }
        cards
    }

    /// The card that fills the stage, if one is pinned.
    pub fn pinned(&self) -> Option<&Pinned> {
        self.pinned.as_ref()
    }

    /// The screen shown large: the one over the whole display, else the one pinned on the stage unless it is in a
    /// window of its own.
    pub(super) fn large(&self) -> Option<&MemberId> {
        match (&self.full, &self.pinned) {
            (Some(full), _) => Some(&full.user_id),
            (None, Some(Pinned::Screen(user_id))) if !self.is_out(user_id, Video::Screen) => Some(user_id),
            (None, _) => None,
        }
    }

    /// Whether another person's camera fills their tile, moving or held.
    pub(super) fn camera_pictured(&self, user_id: &MemberId) -> bool {
        self.tiles.iter().any(|tile| tile.user_id.as_ref() == Some(user_id) && !tile.own() && tile.camera.pictured())
    }

    /// Whether this call has a picture to keep the display awake for: this account sends a screen or a camera, or
    /// plays someone else's that it did not hold.
    pub(crate) fn shows_pictures(&self) -> bool {
        let sends = *self.sending() != Sending::No || self.own_camera != OwnCamera::Off;
        let plays_screen = self.screens.iter().any(|(user_id, received)| {
            matches!(received, Received::Playing(_)) && !self.is_paused(user_id, Video::Screen)
        });
        let plays_camera = self.tiles.iter().any(|tile| !tile.own() && tile.camera == crate::CameraSeen::Shown);
        sends || plays_screen || plays_camera
    }

    /// Whether the person with this id has their `video` in a window of its own.
    pub fn is_out(&self, user_id: &MemberId, video: Video) -> bool {
        self.out.as_ref().is_some_and(|(out, shown)| out == user_id && *shown == video)
    }

    /// The screen or camera in a window of its own, while one is.
    pub fn out(&self) -> Option<Out<'_>> {
        let (user_id, video) = self.out.as_ref()?;
        let sharer = self.tiles.iter().find(|tile| tile.user_id.as_ref() == Some(user_id))?;
        let (card, sound) = match video {
            Video::Screen => {
                let Some(Received::Playing(card)) = self.screens.get(user_id) else { return None };
                let open = self.open() == Some(Over::ShareSound)
                    && matches!(&self.open, Some(Opened::ShareSound(open)) if open == user_id);
                (Some(card), self.shares_sound(user_id).then_some(OutSound { open }))
            }
            Video::Camera => {
                if !self.camera_pictured(user_id) {
                    return None;
                }
                (None, None)
            }
        };
        Some(Out { user_id, name: &sharer.name, video: *video, card, sound })
    }

    /// The zoom on the screen of the person with this id, while it is on: only the screen shown large has one.
    pub fn zoom(&self, user_id: &MemberId) -> Option<crate::Lens> {
        self.zoom.filter(|_| self.large() == Some(user_id)).map(|zooming| zooming.lens)
    }

    /// Whether the screen of the person with this id is shown large, and so may be zoomed.
    pub fn zoomable(&self, user_id: &MemberId) -> bool {
        self.large() == Some(user_id)
    }

    /// Where the screen shown large fits whole in an area `area` large, in the area's pixels.
    fn placed(&self, area: (f32, f32)) -> Option<crate::Placed> {
        let Some(Received::Playing(card)) = self.screens.get(self.large()?) else { return None };
        crate::fit((card.width, card.height), (crate::whole_pixels(area.0), crate::whole_pixels(area.1)))
    }

    /// The share pinned on the stage: someone else's screen while it plays here and not in a window of its own, or
    /// this account's own while its preview shows.
    pub fn pinned_share(&self) -> Option<PinnedShare<'_>> {
        match self.pinned.as_ref()? {
            Pinned::Screen(user_id) => {
                if self.is_out(user_id, Video::Screen) {
                    return None;
                }
                let Some(Received::Playing(card)) = self.screens.get(user_id) else { return None };
                let sharer = self.tiles.iter().find(|tile| tile.user_id.as_ref() == Some(user_id))?;
                Some(PinnedShare::Someone(PinnedScreen { user_id, name: &sharer.name, card }))
            }
            Pinned::OwnScreen => {
                let shown = self.cards().iter().any(|card| matches!(card, Card::OwnScreen(Preview::Shown)));
                shown.then_some(PinnedShare::Own)
            }
            Pinned::Person(_) => None,
        }
    }

    /// The watched screen that fills the whole display, while one does.
    pub fn full_screen(&self) -> Option<FullScreen<'_>> {
        let full = self.full.as_ref()?;
        let sharer = self.tiles.iter().find(|tile| tile.user_id.as_ref() == Some(&full.user_id))?;
        Some(FullScreen { user_id: &full.user_id, name: &sharer.name, calm_at: full.calm_at })
    }

    /// Whether `user_id` is someone else here sharing a screen.
    pub(super) fn shares(&self, user_id: &MemberId) -> bool {
        self.tiles.iter().any(|tile| tile.sharing && !tile.own() && tile.user_id.as_ref() == Some(user_id))
    }

    /// `user_id`'s screen shows nothing here any more, and what was open about it is closed.
    pub(super) fn drop_screen(&mut self, user_id: &MemberId) {
        self.screens.remove(user_id);
        self.unheard.remove(user_id);
        self.paused.remove(&(user_id.clone(), Video::Screen));
        if self.about() == Some(user_id) {
            self.open = None;
        }
    }
}

impl Place {
    /// Leaves a full screen, if this place's call shows one.
    pub(crate) fn leave_full_screen(&mut self) {
        if let Status::Signed(signed) = &mut self.status
            && let Some(call) = signed.call.as_mut()
        {
            call.full = None;
        }
    }

    pub(super) fn pin(&mut self, pinned: Pinned) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.live())
            && call.cards().iter().any(|card| card.pin().as_ref() == Some(&pinned))
        {
            call.pinned = Some(pinned);
            call.zoom = None;
        }
        Vec::new()
    }

    pub(super) fn unpin(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.pinned = None;
            call.zoom = None;
        }
        Vec::new()
    }

    pub(super) fn pop_out(&mut self, user_id: MemberId, video: Video) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut()
            && call.live()
            && match video {
                Video::Screen => matches!(call.screens.get(&user_id), Some(Received::Playing(_))),
                Video::Camera => call.camera_pictured(&user_id),
            }
        {
            if video == Video::Screen && call.full.as_ref().is_some_and(|full| full.user_id == user_id) {
                call.full = None;
            }
            call.out = Some((user_id, video));
        }
        Vec::new()
    }

    pub(super) fn pop_in(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.out = None;
        }
        Vec::new()
    }

    pub(super) fn full_screen_on(&mut self, user_id: MemberId, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            Some(call)
                if call.live()
                    && !call.is_out(&user_id, Video::Screen)
                    && matches!(call.screens.get(&user_id), Some(Received::Playing(_))) =>
            {
                call.full = Some(Full { user_id, calm_at: at.now + design::FULL_SCREEN_CALM });
                call.zoom = None;
                let watching = matches!(call.open.take(), Some(Opened::Sources(_)));
                if watching { vec![Ask::StopSources] } else { Vec::new() }
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn full_screen_off(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.full.is_some()) {
            call.full = None;
            call.zoom = None;
        }
        Vec::new()
    }

    pub(super) fn toggle_zoom(&mut self, user_id: &MemberId) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.zoomable(user_id)) {
            call.zoom = match call.zoom {
                Some(_) => None,
                None => Some(Zooming { lens: crate::Lens::WHOLE }),
            };
        }
        Vec::new()
    }

    pub(super) fn zoom_wheel(&mut self, notches: i8, at: (f32, f32), area: (f32, f32)) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut()
            && let Some(placed) = call.placed(area)
            && let Some(zooming) = call.zoom.as_mut()
        {
            zooming.lens = zooming.lens.wheel(notches, placed.share_of(at));
        }
        Vec::new()
    }

    pub(super) fn pan(&mut self, by: (f32, f32), area: (f32, f32)) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut()
            && let Some(placed) = call.placed(area)
            && let Some(zooming) = call.zoom.as_mut()
        {
            zooming.lens = zooming.lens.pan((by.0 / placed.width as f32, by.1 / placed.height as f32));
        }
        Vec::new()
    }

    pub(super) fn stirred(&mut self, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(full) = signed.call.as_mut().and_then(|call| call.full.as_mut()) {
            full.calm_at = at.now + design::FULL_SCREEN_CALM;
        }
        Vec::new()
    }

    pub(super) fn watched_reported(&mut self, reported: Watched, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            // What is said late of a screen no longer shared here goes again in `settle`.
            match reported {
                Watched::Card(card) => {
                    call.screens.insert(card.who.clone(), Received::Playing(card));
                }
                Watched::Failed { user_id, video: Video::Screen, failed } => {
                    call.screens.insert(user_id, Received::Unshown(failed));
                }
                Watched::Failed { user_id, video: Video::Camera, failed } => {
                    call.unshown_cameras.insert(user_id, failed);
                    call.refresh_tiles(at.now);
                }
                Watched::HandedOver { user_id, video: _, route } => {
                    if let Some(name) = call.room.shown_name(&user_id) {
                        let said = route.watched_handed_over(name);
                        *told = Some(Told::Toast(Telling::of(design::Status::Warning, said)));
                    }
                }
                Watched::Ended { user_id, video: Video::Screen } => call.drop_screen(&user_id),
                Watched::Paused { user_id, video } => {
                    call.paused.insert((user_id, video));
                    call.refresh_tiles(at.now);
                }
                Watched::Ended { user_id, video: Video::Camera } => {
                    call.paused.remove(&(user_id.clone(), Video::Camera));
                    call.unshown_cameras.remove(&user_id);
                    call.refresh_tiles(at.now);
                }

                Watched::Sound { user_id, refused: Some(failed) } => {
                    call.unheard.insert(user_id, failed);
                }
                Watched::Sound { user_id, refused: None } => {
                    call.unheard.remove(&user_id);
                }
            }
        }
        Vec::new()
    }
}
