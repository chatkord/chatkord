//! The servers the window holds: added from the welcome steps or the add dialog, a message for one, one call at a time
//! across them, and a server put back or removed.

use super::*;

/// A server being added: the welcome steps, under the id its place will have.
#[derive(Debug)]
pub struct Adding {
    pub(super) id: PlaceId,
    pub(super) door: Door,
    /// What the server told its place before there was one: the greeting comes ahead of the sign-in's report, and the
    /// place it becomes takes it.
    pub(super) told: Vec<place::ServerMessage>,
}

impl Adding {
    /// The id the server's place will have once it is added.
    pub fn id(&self) -> PlaceId {
        self.id
    }

    /// The welcome steps, at the step they are on.
    pub fn door(&self) -> &Door {
        &self.door
    }
}

/// The note a first arrival opens with: what it says, and how the person arrived.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrivalNote<'a> {
    /// What it says, naming the server as it is now.
    pub said: Sentence,
    /// What it says under that: who others see, or for an owner whether anyone else is here yet.
    pub body: Sentence,
    /// Whether the person arrived as the server's owner or joined it.
    pub arrival: &'a door::Arrival,
}

impl ArrivalNote<'_> {
    /// The note as a callout: its sentence under its title, with a ✕, and for an owner Invite people.
    pub fn callout(&self) -> Callout {
        let callout = Callout::new(self.body.clone()).titled(self.said.clone()).dismissable();
        match self.arrival {
            door::Arrival::Owner => callout.offering(Action::InvitePeople),
            door::Arrival::Joined { .. } => callout,
        }
    }
}

/// The servers shown and what is open over them: the open one, the one being added, the menu, the dialog and the toast.
#[derive(Debug, Default)]
pub(super) struct Desk {
    /// The server open in the pane.
    pub(super) open: Option<PlaceId>,
    /// The server being added, by the welcome steps or the add dialog.
    pub(super) adding: Option<Adding>,
    /// The toast, held until its time is past.
    pub(super) toast: Option<Toast>,
    /// A channel to join once the call elsewhere is left.
    pub(super) pending: Option<(PlaceId, ChannelId)>,
    /// The last id a server was issued.
    pub(super) next: u64,
    /// How far the rail is scrolled, which is where a menu opened from it is drawn.
    pub(super) rail_scrolled: f32,
    /// The menu open over the window.
    pub(super) menu: Option<menu::Opened>,
    /// The confirmation open over the window.
    pub(super) dialog: Option<Dialog>,
    /// Text for the front end to put on the clipboard.
    pub(super) copying: Option<Copying>,
    /// A role's members, open from its mention.
    pub(super) role_open: Option<(PlaceId, terms::RoleId)>,
}

impl Window {
    /// The next id a server is issued.
    pub(super) fn issue(&mut self) -> PlaceId {
        self.desk.next += 1;
        PlaceId(self.desk.next)
    }

    /// Says `telling` of the server `place` in a toast: what went through where the person cannot see it (a copy,
    /// another device, another person's change) and what happened to them.
    pub(super) fn tell(&mut self, place: PlaceId, telling: Telling) -> Vec<Ask> {
        let toast = Toast::new(place, telling, self.times.now);
        let until = toast.until();
        self.desk.toast = Some(toast);
        vec![Ask::WakeAt(until)]
    }

    /// Says `said`, plain, in a toast.
    pub(super) fn done(&mut self, place: PlaceId, said: Sentence) -> Vec<Ask> {
        self.tell(place, Telling::plain(said))
    }

    /// The toast put away.
    pub(super) fn on_dismiss_toast(&mut self) -> Vec<Ask> {
        self.desk.toast = None;
        Vec::new()
    }

    /// The pointer is on the toast, or has left it: it is held, and wakes when its time is past.
    pub(super) fn on_hold_toast(&mut self, held: bool, at: Moment) -> Vec<Ask> {
        match self.desk.toast.as_mut() {
            Some(toast) => {
                toast.hold(held, at.now);
                vec![Ask::WakeAt(toast.until())]
            }
            None => Vec::new(),
        }
    }

    /// A server that is not signed in put away, once it says it cannot be used.
    pub(super) fn on_remove(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        match self.place(id) {
            Some(place) if matches!(place.panel(), Panel::Deleted | Panel::KeyChanged) => self.remove(id, at),
            Some(_) | None => Vec::new(),
        }
    }

    /// What the server being added told its place before there was one, kept for the place it becomes.
    pub(super) fn on_adding_told(&mut self, message: place::ServerMessage) -> Vec<Ask> {
        if let Some(adding) = self.desk.adding.as_mut() {
            adding.told.push(message);
        }
        Vec::new()
    }

    /// The server the add dialog found already added is opened, and the dialog closed. None while it found none, which
    /// leaves nothing to settle.
    pub(super) fn on_open_listed(&mut self, at: Moment) -> Option<Vec<Ask>> {
        let url = match self.desk.adding.as_ref().and_then(|adding| adding.door.problem()) {
            Some(Problem::Already { server }) => server.url.clone(),
            Some(_) | None => return None,
        };
        let id = self.places.iter().find(|place| place.server().url == url).map(Place::id)?;
        let mut todo = self.close_add();
        todo.extend(self.update(Message::Open(id), at));
        Some(todo)
    }

    /// A conversation opens in the content area: the popover and the drawer are done with.
    pub(super) fn on_conversation(&mut self, id: PlaceId, user_id: MemberId, at: Moment) -> Vec<Ask> {
        self.desk.menu = None;
        self.layout.drawer = Drawer::Shut;
        self.at(id, place::Message::Talk(place::TalkMessage::Message(user_id)), at)
    }

    pub(super) fn welcome(&mut self) {
        let id = self.issue();
        self.desk.adding = Some(Adding { id, door: Door::default(), told: Vec::new() });
    }

    /// The welcome steps, shown instead of everything else while no server is added.
    pub fn welcome_door(&self) -> Option<&Adding> {
        self.desk.adding.as_ref().filter(|_| self.places.is_empty())
    }

    /// The same steps in a dialog over the window, adding another server.
    pub fn add_dialog(&self) -> Option<&Adding> {
        self.desk.adding.as_ref().filter(|_| !self.places.is_empty())
    }

    /// The note `place` shows, naming the server as it is now: none while its owner is asked to name and colour it,
    /// which the note follows.
    pub fn note<'a>(&self, place: &'a Place) -> Option<ArrivalNote<'a>> {
        let asking = self
            .desk
            .dialog
            .as_ref()
            .is_some_and(|open| open.place == place.id() && matches!(open.asking, Asking::SetUp(_)));
        let arrival = place.note().filter(|_| !asking).filter(|arrival| {
            matches!(arrival, door::Arrival::Owner) || place.served().on(proto::rules::Switch::Welcome)
        })?;
        let server = place.server().shown().to_string();
        let said = match arrival {
            door::Arrival::Owner => Sentence::YouOwn(server),
            door::Arrival::Joined { .. } => Sentence::Welcome(server),
        };
        let body = match arrival {
            door::Arrival::Joined { name } => Sentence::SeenAs(name.clone()),
            door::Arrival::Owner if place.directory().people().count() > 1 => Sentence::Phrase(Phrase::InviteMore),
            door::Arrival::Owner => Sentence::Phrase(Phrase::QuietForNow),
        };
        Some(ArrivalNote { said, body, arrival })
    }

    /// What `id`'s change made news of: the connection back with a call to rejoin, or a call that ended where its own
    /// view is not on screen. What a person did, or sees happen, is not told again.
    fn news_of(&mut self, id: PlaceId, had_back: bool, had_ended: bool) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let on_screen = self.desk.open == Some(id) && self.hardware.device.is_none() && place.panel() == Panel::Call;
        let telling = match (place.back(), place.ended_telling(self.desk.open != Some(id))) {
            (Some(channel), _) if !had_back => Some(
                Telling::of(design::Status::Success, Sentence::Back(channel.name.clone()))
                    .offering(Action::Rejoin(channel.name.clone())),
            ),
            (_, Some(ended)) if !had_ended && !on_screen => Some(ended),
            _ => None,
        };
        match telling {
            Some(telling) => self.tell(id, telling),
            None => Vec::new(),
        }
    }

    /// The toast's action: what it offered is done for the server it came from, and the toast goes.
    pub(super) fn toast_act(&mut self, at: Moment) -> Vec<Ask> {
        let Some(toast) = self.desk.toast.take() else { return Vec::new() };
        let id = toast.place;
        match toast.telling.action {
            Some(Action::ShareAgain) => self.at(id, place::Message::Call(place::CallMessage::ChooseScreen), at),
            Some(Action::TurnOnCamera) => self.at(id, place::Message::Call(place::CallMessage::ToggleCamera), at),
            Some(Action::Again(_)) => self.join_again(id, at),
            Some(Action::Rejoin(_)) => self.rejoin(id, at),
            Some(Action::InvitePeople) => self.update(Message::Pick(id, Pick::Invite), at),
            Some(Action::OpenServer(_)) => Vec::new(),
            None => Vec::new(),
        }
    }

    /// A message for one server, and what its change means for the window.
    pub(super) fn at(&mut self, id: PlaceId, message: place::Message, at: Moment) -> Vec<Ask> {
        let Some(place) = self.places.iter_mut().find(|place| place.id() == id) else { return Vec::new() };
        // A call that ends leaves the next one as it was, as one state in and out of a call.
        let left = matches!(message, place::Message::Call(place::CallMessage::CallOver(_)))
            .then(|| place.call().map(place::Call::state))
            .flatten();
        let (had_back, had_ended) = (place.back().is_some(), place.ended_telling(false).is_some());
        let had_channels = matches!(place.lobby(), Some(Lobby::Ready(_)));
        let (mut asked, told) = place.update(message, at);
        // Channels that arrive with none open open the first text one; a server with only voice channels says so.
        if !had_channels && let Some(first) = place.first_text() {
            asked.extend(place.open_discussion(&first, self.times.zone.clone()));
        }
        if told != Some(Told::Unseen) {
            self.times.revision.moved();
        }
        let server = place.server().shown().to_string();
        // A share asked for with the panel's choice keeps it as the next share's start.
        let chosen = asked.iter().find_map(|asked| match asked {
            place::Ask::ChooseScreen(quality) | place::Ask::ShareSource { quality, .. } => Some(quality.clone()),
            _ => None,
        });
        let mut todo: Vec<Ask> = asked.into_iter().filter_map(|asked| self.popup_asks(id, asked)).collect();
        if let Some(quality) = chosen.filter(|quality| *quality != self.voice.share) {
            self.voice.share = quality;
            self.tell_places();
            todo.push(Ask::Keep(Keep::Share(self.voice.share.clone())));
        }
        match told {
            Some(Told::Chime(chime)) => todo.extend(self.chime_at(chime, at.now)),
            Some(Told::PasswordChanged) => todo.extend(self.done(id, Sentence::Phrase(Phrase::PasswordChanged))),
            Some(Told::Raised) => todo.extend(self.done(id, Sentence::RaisedOn(server))),
            Some(Told::Toast(telling)) => todo.extend(self.tell(id, telling)),
            Some(Told::Picked(emoji)) => {
                self.voice.emoji.picked(emoji);
                todo.push(Ask::Keep(Keep::Emoji(self.voice.emoji.clone())));
            }
            Some(Told::Unseen) | None => {}
        }
        todo.extend(self.news_of(id, had_back, had_ended));
        if let Some(state) = left.filter(|state| *state != self.voice.next_call) {
            self.voice.next_call = state;
            todo.push(Ask::NextCall(state));
        }
        todo.extend(self.join_pending(at));
        todo
    }

    /// One call at a time: a call live elsewhere, or on another channel here, is left first; the call's own channel
    /// shows it again.
    pub(super) fn join(&mut self, id: PlaceId, channel: ChannelId, at: Moment) -> Vec<Ask> {
        let live = self.places.iter().find(|place| place.in_call()).map(Place::id);
        let here = live == Some(id)
            && self.place(id).and_then(Place::call).is_some_and(|call| call.live() && call.channel.id == channel);
        match live {
            Some(_) if here => self.at(id, place::Message::Lobby(place::LobbyMessage::Join(channel)), at),
            Some(other) => {
                self.desk.pending = Some((id, channel));
                self.at(other, place::Message::Call(place::CallMessage::Leave), at)
            }
            None => self.joining(id, channel, at),
        }
    }

    /// Joins with no call running anywhere: a call that ended on another server is put away first, so its notice
    /// and its Join again go with it.
    fn joining(&mut self, id: PlaceId, channel: ChannelId, at: Moment) -> Vec<Ask> {
        let put_away: Vec<(PlaceId, place::Ask)> = self
            .places
            .iter_mut()
            .filter(|place| place.id() != id)
            .flat_map(|place| {
                let other = place.id();
                place.put_away_ended().into_iter().map(move |asked| (other, asked))
            })
            .collect();
        let mut todo: Vec<Ask> =
            put_away.into_iter().filter_map(|(other, asked)| self.popup_asks(other, asked)).collect();
        todo.extend(self.at(id, place::Message::Lobby(place::LobbyMessage::Join(channel)), at));
        todo
    }

    pub(super) fn join_again(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        let Some(channel) = self.place(id).and_then(Place::call).map(|call| call.channel.id.clone()) else {
            return Vec::new();
        };
        let mut todo = self.update(Message::Open(id), at);
        todo.extend(self.join(id, channel, at));
        todo
    }

    pub(super) fn join_pending(&mut self, at: Moment) -> Vec<Ask> {
        if self.places.iter().any(Place::in_call) {
            return Vec::new();
        }
        let Some((id, channel)) = self.desk.pending.take() else { return Vec::new() };
        self.joining(id, channel, at)
    }

    /// The welcome steps; signed in, they become the first server, or the listed one an invite was for.
    pub(super) fn adding_door(&mut self, message: door::Message, at: Moment) -> Vec<Ask> {
        let Some(adding) = self.desk.adding.as_mut() else { return Vec::new() };
        let asked = adding.door.update(message);
        let Some(id) = self.adding_under() else { return Vec::new() };
        let mut todo: Vec<Ask> = asked.into_iter().map(|asked| Ask::At(id, place::Ask::Door(asked))).collect();
        let Some(adding) = self.desk.adding.take_if(|adding| adding.door.signed_in()) else { return todo };
        let listed = self.places.iter_mut().find(|place| place.id() == id);
        let url = match listed {
            Some(place) => {
                place.signed_by(&adding.door);
                place.server().url.clone()
            }
            None => {
                let Some(place) = Place::added(id, &adding.door) else {
                    self.desk.adding = Some(adding);
                    return todo;
                };
                todo.extend(place.to_paint().map(|shown| Ask::At(id, shown)));
                let url = place.server().url.clone();
                self.places.push(place);
                url
            }
        };
        // A server's owner arriving the first time, at a server that can be named and coloured, asks for both.
        if matches!(adding.door.arrival(), Some(door::Arrival::Owner))
            && let Some(place) = self.place(id)
        {
            let asking = Asking::SetUp(SetUp::new(place.server()));
            self.desk.dialog = Some(Dialog { place: id, asking, call: None });
        }
        let told = adding.told;
        for message in told {
            todo.extend(self.at(id, place::Message::Server(message), at));
        }
        // What the server said it offers while the door was open decides which lists are read.
        let reads = self.places.iter_mut().find(|place| place.id() == id).map(Place::first_reads).unwrap_or_default();
        self.tell_places();
        self.desk.open = Some(id);
        todo.extend(reads.into_iter().map(|read| Ask::At(id, read)));
        todo.push(Ask::RememberOpen(url));
        todo
    }

    /// The id the add dialog's errands go out under, set before they do: the listed place its invite is for, so the
    /// account lands there, else one of its own; none without the dialog.
    fn adding_under(&mut self) -> Option<PlaceId> {
        let adding = self.desk.adding.as_ref()?;
        let bound = adding.door.bound().and_then(|url| self.places.iter().find(|place| place.server().url == *url));
        let id = match bound {
            Some(place) => place.id(),
            None if self.place(adding.id).is_some() => self.issue(),
            None => adding.id,
        };
        if let Some(adding) = self.desk.adding.as_mut() {
            adding.id = id;
        }
        Some(id)
    }

    /// Puts back, open and signed out, a server whose forget left its token, unless it was added again meanwhile.
    pub(super) fn token_left(&mut self, server: Server, identity: String) -> Vec<Ask> {
        if let Some(place) = self.places.iter_mut().find(|place| place.server().url == server.url) {
            place.token_stayed();
            return Vec::new();
        }
        let id = self.issue();
        let (place, shown) = Place::token_left(id, server, identity);
        self.places.push(place);
        self.desk.open = Some(id);
        self.desk.adding = None;
        shown.into_iter().map(|shown| Ask::At(id, shown)).collect()
    }

    /// Share pressed on the pane's call controls: the call's server and its page, unless shown, come first, so the
    /// panel it opens is one the person sees; a share under way is stopped where it is.
    pub(super) fn pane_share(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let Some(call) = place.call().filter(|call| call.live()) else { return Vec::new() };
        let page_shown = self.hardware.device.is_none() && self.desk.open == Some(id) && place.panel() == Panel::Call;
        let mut todo = Vec::new();
        if *call.sending() == place::Sending::No && !page_shown {
            let channel = call.channel.id.clone();
            todo.extend(self.update(Message::Open(id), at));
            todo.extend(self.at(id, place::Message::Lobby(place::LobbyMessage::Join(channel)), at));
        }
        todo.extend(self.at(id, place::Message::Call(place::CallMessage::ToggleShare), at));
        todo
    }

    /// What a call's page opened over the call is closed while the page is not shown, whichever way it went: another
    /// page, another server, the settings.
    pub(super) fn settle_call_popover(&mut self) {
        let shown = if self.hardware.device.is_none() { self.desk.open } else { None };
        for place in &mut self.places {
            place.settle_call_popover(shown == Some(place.id()));
        }
    }

    /// Takes a server off the window: the next one opens, and with none left the welcome steps do.
    pub(super) fn remove(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        let Some(index) = self.places.iter().position(|place| place.id() == id) else { return Vec::new() };
        let place = self.places.remove(index);
        let mut todo = vec![Ask::At(id, place.forget())];
        self.desk.pending = self.desk.pending.take().filter(|(to, _)| *to != id);
        self.desk.menu = self.desk.menu.take().filter(|open| match open {
            menu::Opened::Server(menu) => menu.place != id,
            menu::Opened::List(menu) => menu.place != id,
            menu::Opened::Person(open) => open.place != id,
            menu::Opened::App(open) => open.place != id,
            menu::Opened::Language => true,
        });
        self.desk.dialog = self.desk.dialog.take().filter(|dialog| dialog.place != id);
        self.desk.copying = self.desk.copying.take().filter(|copying| copying.place != id);
        if self.places.is_empty() {
            self.desk.open = None;
            self.desk.toast = None;
            self.desk.adding = None;
            self.welcome();
            return todo;
        }
        if self.desk.open == Some(id) {
            self.desk.open = self.places.first().map(Place::id);
            todo.extend(self.open().map(|open| Ask::RememberOpen(open.server().url.clone())));
        }
        todo.extend(self.join_pending(at));
        todo
    }

    pub(super) fn close_add(&mut self) -> Vec<Ask> {
        if self.places.is_empty() {
            return Vec::new();
        }
        let Some(adding) = self.desk.adding.take() else { return Vec::new() };
        match adding.door.step() {
            Step::Finding | Step::Working => vec![Ask::At(adding.id, place::Ask::Door(door::Ask::Cancel))],
            _ => Vec::new(),
        }
    }

    /// A server's picture pressed in the rail: the pages over the open server and this one close, and it opens.
    pub(super) fn rail(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        self.hardware.device = None;
        let left = [self.desk.open, Some(id)];
        let mut todo: Vec<Ask> = Vec::new();
        for place in self.places.iter_mut().filter(|place| left.contains(&Some(place.id()))) {
            let from = place.id();
            todo.extend(place.close_pages().into_iter().map(|asked| Ask::At(from, asked)));
        }
        todo.extend(self.apply(Message::Open(id), at));
        todo
    }

    /// `id` opened, if the window holds it, and remembered for the next run.
    pub(super) fn open_place(&mut self, id: PlaceId) -> Vec<Ask> {
        if self.place(id).is_none() {
            return Vec::new();
        }
        // A full screen is the open place's: one left behind never comes back with its server.
        for place in self.places.iter_mut().filter(|place| place.id() != id) {
            place.leave_full_screen();
        }
        self.desk.open = Some(id);
        self.desk.toast = None;
        self.place(id).map(|place| Ask::RememberOpen(place.server().url.clone())).into_iter().collect()
    }

    /// The add dialog opened, once a server is added and while it is not open already.
    pub(super) fn add(&mut self) -> Vec<Ask> {
        if self.desk.adding.is_some() || self.places.is_empty() {
            return Vec::new();
        }
        let known = self.places.iter().map(Place::listed).collect();
        let username = self.open().and_then(Place::identity).map(str::to_string);
        let id = self.issue();
        self.desk.adding = Some(Adding { id, door: Door::adding(known, username.as_deref()), told: Vec::new() });
        self.desk.menu = None;
        self.desk.toast = None;
        Vec::new()
    }

    /// A press outside the add dialog: it closes only before anything is typed.
    pub(super) fn add_outside(&mut self) -> Vec<Ask> {
        match &self.desk.adding {
            Some(adding) if adding.door.step() == Step::Add && adding.door.text().trim().is_empty() => self.close_add(),
            Some(_) | None => Vec::new(),
        }
    }

    /// Rejoin on the call `id` was dropped from.
    pub(super) fn rejoin(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        match self.place(id).and_then(Place::back) {
            Some(channel) => self.join(id, channel.id.clone(), at),
            None => Vec::new(),
        }
    }

    /// A text channel of `id` opened, in this device's time zone, with the menu and the drawer put away.
    pub(super) fn open_discussion(&mut self, id: PlaceId, channel: &ChannelId) -> Vec<Ask> {
        let zone = self.times.zone.clone();
        self.desk.menu = None;
        self.layout.drawer = Drawer::Shut;
        match self.places.iter_mut().find(|place| place.id() == id) {
            Some(place) => place.open_discussion(channel, zone).into_iter().map(|asked| Ask::At(id, asked)).collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{End, Event as RoomEvent};
    use crate::door::{Errand, Heard};
    use crate::harness::{
        ADA, Clock, accepted, books, channels, chatting, ended, id, in_call, overview_open, owls, server, standing_as,
        tell, two_signed_in,
    };
    use crate::harness::{sends, wire};
    use crate::place::RailState;
    use proto::chatkord_v1::CloseReason;
    use proto::rules::Grants;
    use terms::{Ended, Secret};

    #[test]
    fn a_first_run_opens_on_the_welcome_steps_and_its_first_server_gets_no_toast() {
        let clock = Clock(Instant::now());
        let (mut window, todo) = Window::launch(Vec::new(), None, clock.0);
        assert!(todo.is_empty());
        let adding = window.welcome_door().unwrap().id();
        let code = "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        tell(
            &mut window,
            &clock,
            adding,
            door(door::Message::Where(format!("https://books.example.invalid/join#{code}"))),
        );
        tell(&mut window, &clock, adding, door(door::Message::Submit));
        let found = door::Found { server: books(), invite: Some(door::Invite::Works { until: 0, owner: false }) };
        tell(&mut window, &clock, adding, door(door::Message::Reported(Heard::Found(Ok(found)))));
        tell(&mut window, &clock, adding, door(door::Message::Submit));
        tell(&mut window, &clock, adding, door(door::Message::Username(ADA.into())));
        tell(
            &mut window,
            &clock,
            adding,
            door(door::Message::Password(Secret::new("evening games with friends".into()))),
        );
        tell(&mut window, &clock, adding, door(door::Message::Again(Secret::new("evening games with friends".into()))));
        tell(&mut window, &clock, adding, door(door::Message::Submit));
        let admitted = door::Admitted { token: None, named: None, standing: standing_as("ada") };
        let todo = tell(&mut window, &clock, adding, door(door::Message::Reported(Heard::Registered(Ok(admitted)))));
        assert!(
            todo.iter().any(|done| matches!(done, Ask::At(at, ask1) if crate::harness::is(ask1, crate::harness::Op::ChannelsList) && (*at == adding))),
            "{todo:?}"
        );
        assert!(
            todo.iter().any(
                |done| matches!(done, Ask::At(at, ask) if *at == adding && sends(ask, wire::ListPeople { after: None }))
            ),
            "the people the window names authors by, as any other sign-in reads them: {todo:?}"
        );
        assert!(todo.iter().any(|done| matches!(done, Ask::RememberOpen(_))), "{todo:?}");
        assert!(window.welcome_door().is_none());
        let open = window.open().unwrap();
        assert_eq!((open.id(), open.server().shown()), (adding, "Book club"), "the place keeps the welcome's id");
        assert!(open.note().is_some(), "the welcome note");
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the first server is no news"
        );
    }

    #[test]
    fn what_the_server_greeted_with_before_the_form_signed_in_is_the_new_places_own() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        let adding = window.welcome_door().unwrap().id();
        let code = "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        let say = |window: &mut Window, message| tell(window, &clock, adding, message);
        say(&mut window, door(door::Message::Where(format!("https://books.example.invalid/join#{code}"))));
        say(&mut window, door(door::Message::Submit));
        let found = door::Found { server: books(), invite: Some(door::Invite::Works { until: 0, owner: false }) };
        say(&mut window, door(door::Message::Reported(Heard::Found(Ok(found)))));
        say(&mut window, door(door::Message::Submit));
        say(&mut window, door(door::Message::Username(ADA.into())));
        let password = || Secret::new("evening games with friends".into());
        say(&mut window, door(door::Message::Password(password())));
        say(&mut window, door(door::Message::Again(password())));
        say(&mut window, door(door::Message::Submit));
        // The errand tells the greeting first, and only then the sign-in's report.
        let build = terms::ServerBuild::read("0.9.0").unwrap();
        let served = place::Served::read(&[proto::rules::Switch::LinkPreviews]);
        say(&mut window, place::Message::Server(place::ServerMessage::Serves(served.clone())));
        say(&mut window, place::Message::Server(place::ServerMessage::Built(build.clone())));
        let admitted = door::Admitted { token: None, named: None, standing: standing_as("ada") };
        say(&mut window, door(door::Message::Reported(Heard::Registered(Ok(admitted)))));
        let place = window.place(adding).unwrap();
        assert_eq!(place.build(), build.as_ref(), "the build");
        assert_eq!(place.served(), &served, "what it serves");
    }

    fn joined_with(clock: &Clock, served: place::Served) -> (Window, PlaceId) {
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        let adding = window.welcome_door().unwrap().id();
        let code = "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        let say = |window: &mut Window, message| tell(window, clock, adding, message);
        say(&mut window, door(door::Message::Where(format!("https://books.example.invalid/join#{code}"))));
        say(&mut window, door(door::Message::Submit));
        let found = door::Found { server: books(), invite: Some(door::Invite::Works { until: 0, owner: false }) };
        say(&mut window, door(door::Message::Reported(Heard::Found(Ok(found)))));
        say(&mut window, door(door::Message::Submit));
        say(&mut window, door(door::Message::Username(ADA.into())));
        let password = || Secret::new("evening games with friends".into());
        say(&mut window, door(door::Message::Password(password())));
        say(&mut window, door(door::Message::Again(password())));
        say(&mut window, door(door::Message::Submit));
        say(&mut window, place::Message::Server(place::ServerMessage::Serves(served)));
        let admitted = door::Admitted { token: None, named: None, standing: standing_as("ada") };
        say(&mut window, door(door::Message::Reported(Heard::Registered(Ok(admitted)))));
        (window, adding)
    }

    #[test]
    fn a_joining_member_sees_no_welcome_when_switched_off() {
        let clock = Clock(Instant::now());
        let (window, adding) = joined_with(&clock, place::Served::read(&[]));
        assert!(window.note(window.place(adding).unwrap()).is_some(), "welcomed while the switch is on");
        let off = place::Served::read(&[proto::rules::Switch::Welcome]);
        let (window, adding) = joined_with(&clock, off);
        let place = window.place(adding).unwrap();
        assert!(place.note().is_some(), "the arrival is kept");
        assert!(window.note(place).is_none(), "and not shown while the welcome is switched off");
    }

    #[test]
    fn every_saved_account_signs_in_at_once_and_the_last_open_server_opens() {
        let clock = Clock(Instant::now());
        let rows = vec![
            Opening::Account(owls(), ADA.into()),
            Opening::Server(books()),
            Opening::Account(server("garden.example.invalid", "Plant swap"), ADA.into()),
        ];
        let (window, todo) = Window::launch(rows, Some(&books().url), clock.0);
        let loads: Vec<PlaceId> = todo
            .iter()
            .filter_map(|done| match done {
                Ask::At(at, place::Ask::Door(door::Ask::Go(Errand::LoadToken(_)))) => Some(*at),
                _ => None,
            })
            .collect();
        assert_eq!(loads, [id(&window, 0), id(&window, 2)], "both accounts at once, in order");
        assert_eq!(window.open().unwrap().server().shown(), "Book club");
        let rails: Vec<RailState> = window.places().iter().map(Place::rail).collect();
        assert_eq!(rails, [RailState::Connecting, RailState::SignedOut, RailState::Connecting]);
        assert_eq!(window.places()[1].panel(), Panel::SignedOut, "no account there: the sign-in panel");
    }

    #[test]
    fn a_server_with_no_account_here_shows_no_pane_and_its_content_takes_the_room() {
        let clock = Clock(Instant::now());
        let rows = vec![Opening::Account(owls(), ADA.into()), Opening::Server(books())];
        let (mut window, _) = Window::launch(rows, Some(&books().url), clock.0);
        let room = Room { width: 1280.0, height: 800.0 };
        window.set_room(room);
        assert_eq!(window.open().map(Place::panel), Some(Panel::SignedOut));
        let bare = window.frame(room);
        assert_eq!(bare.pane, None, "nothing in a pane works signed out");
        assert_eq!(bare.content, room.width - design::RAIL_WIDTH - design::BORDER_WIDTH, "the content takes its room");
        let owls_id = id(&window, 0);
        window.update(Message::Rail(owls_id), clock.at());
        assert!(window.frame(room).pane.is_some(), "a server with an account here keeps its pane, connecting or not");
    }

    #[test]
    fn joining_on_another_server_leaves_the_call_first() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        in_call(&mut window, &clock, owls_id);
        let todo = tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert!(
            matches!(&todo[..], [Ask::At(at, place::Ask::Leave), Ask::Chime(Chime::Left)] if *at == owls_id),
            "the call is left first: {todo:?}"
        );
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "a move the person made is not echoed"
        );
        assert!(window.places()[1].call().is_none(), "nothing joined until the call is left");
        let todo = tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        assert!(
            matches!(&todo[..], [Ask::At(_, ask1), Ask::At(a, ask2), Ask::At(b, place::Ask::Join { .. })] if crate::harness::is(ask1, crate::harness::Op::History) && crate::harness::is(ask2, crate::harness::Op::ChannelsList) && (*a == owls_id && *b == books_id)),
            "{todo:?}"
        );
    }

    #[test]
    fn a_call_that_ends_under_another_server_is_told_there_and_joined_from_there() {
        use crate::Action;
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        in_call(&mut window, &clock, owls_id);
        window.update(Message::Open(books_id), clock.at());
        assert!(window.places()[0].calling() && !window.places()[1].calling(), "the rail marks the call's server");
        assert!(window.toast().is_none(), "a live call says nothing elsewhere");
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        assert!(!window.places()[0].calling(), "an ended call holds no place");
        let toast = window.toast().expect("told on the open server");
        let server = window.places()[0].server().shown().to_string();
        assert_eq!(toast.place, owls_id, "it names the call's server, not the open one");
        assert_eq!(toast.said().in_english(), format!("An admin took you out of Lounge on {server}."));
        assert_eq!(toast.action(), Some(&Action::Again(place::WayBack::JoinAgain)));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        let todo = window.update(Message::ToastAct, clock.at());
        assert_eq!(window.open().map(Place::id), Some(owls_id), "the call's server opens");
        assert!(
            todo.iter().any(|todo| matches!(todo, Ask::At(at, place::Ask::Join { .. }) if *at == owls_id)),
            "and joins: {todo:?}"
        );
        assert!(window.toast().is_none(), "and the toast goes");
    }

    #[test]
    fn a_call_ended_elsewhere_and_put_away_by_a_join_returns_its_server_to_the_text_channel_last_open() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        let general = crate::harness::channel_id(crate::harness::GENERAL);
        tell(&mut window, &clock, owls_id, place::Message::Discussion(place::DiscussionMessage::Open(general.clone())));
        in_call(&mut window, &clock, owls_id);
        window.update(Message::Open(books_id), clock.at());
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        assert_eq!(window.places()[0].panel(), Panel::Call, "an ended call stays to say so");
        let todo = tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        let owls = &window.places()[0];
        assert!(owls.call().is_none(), "the call that ended is put away");
        assert_eq!(
            (owls.panel(), owls.discussion().map(|open| open.owner.clone())),
            (Panel::Discussion, Some(place::Owner::Channel(general))),
            "its server is back on the text channel it showed last"
        );
        assert!(
            todo.iter().any(|done| matches!(done, Ask::At(at, ask1) if crate::harness::is(ask1, crate::harness::Op::History) && (*at == owls_id))),
            "whose first page is asked for: {todo:?}"
        );
    }

    #[test]
    fn a_call_joined_again_over_one_that_ended_keeps_the_figures_of_the_one_that_ended() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        in_call(&mut window, &clock, owls_id);
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert_eq!(window.report.finished.iter().count(), 1, "the call that ended is reported");
    }

    #[test]
    fn a_toast_stays_while_the_pointer_is_on_it_and_goes_when_its_time_is_past() {
        let mut clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        in_call(&mut window, &clock, owls_id);
        window.update(Message::Open(books_id), clock.at());
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        assert!(window.toast().is_some());
        window.update(Message::HoldToast(true), clock.at());
        clock.pass(design::TOAST_NOTED_FOR * 2);
        window.update(Message::Tick, clock.at());
        assert!(window.toast().is_some(), "held: it stays past its time");
        window.update(Message::HoldToast(false), clock.at());
        clock.pass(design::TOAST_NOTED_FOR - Duration::from_secs(1));
        window.update(Message::Tick, clock.at());
        assert!(window.toast().is_some(), "left: its time starts again");
        clock.pass(Duration::from_secs(2));
        window.update(Message::Tick, clock.at());
        assert!(window.toast().is_none(), "and runs out");
    }

    #[test]
    fn removing_the_open_server_opens_the_next_and_removing_the_last_opens_the_welcome() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        window.update(Message::Pick(owls_id, Pick::Remove), clock.at());
        let todo = window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [
                Ask::At(at, place::Ask::EndAccount { identity: Some(_), sign_out: true, row: place::Row::Forgotten, .. }),
                Ask::RememberOpen(url),
            ] if *at == owls_id && *url == books().url),
            "{todo:?}"
        );
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the rail shows it gone");
        assert_eq!(window.open().unwrap().id(), books_id);
        assert!(
            tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))))
                .is_empty(),
            "gone"
        );
        window.update(Message::Pick(books_id, Pick::Remove), clock.at());
        let todo = window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::EndAccount { .. }), Ask::Idle(crate::window::IdleWatch::Stop)]),
            "the last signed-in server gone stops the idle watch: {todo:?}"
        );
        assert!(window.places().is_empty() && window.welcome_door().is_some(), "the welcome steps again");
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None);
    }

    #[test]
    fn a_server_removed_ends_its_session_whatever_it_shows_and_while_its_connection_is_made_again() {
        let clock = Clock(Instant::now());
        let removed = |window: &mut Window, place: PlaceId| {
            let shown = window.places().iter().find(|at| at.id() == place).map(Place::panel);
            window.update(Message::Pick(place, Pick::Remove), clock.at());
            let todo = window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
            let ends = matches!(
                todo.first(),
                Some(Ask::At(_, place::Ask::EndAccount { sign_out: true, row: place::Row::Forgotten, .. }))
            );
            assert!(ends, "removed from {shown:?}: {todo:?}");
            shown
        };

        let (mut window, owls) = chatting(&clock);
        assert_eq!(removed(&mut window, owls), Some(Panel::Discussion));

        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        in_call(&mut window, &clock, owls);
        assert_eq!(removed(&mut window, owls), Some(Panel::Call));

        let (mut window, owls) = overview_open(&clock);
        assert_eq!(removed(&mut window, owls), Some(Panel::Settings));

        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(removed(&mut window, owls), Some(Panel::Account));

        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        assert_eq!(removed(&mut window, owls), Some(Panel::Discussion), "under this device's page");

        // No connection is open, but the token it will sign in with again still opens a session at the server.
        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        let lost = Ended { silent: true, ..Ended::default() };
        tell(&mut window, &clock, owls, place::Message::Session(place::SessionMessage::Disconnected(lost)));
        assert_eq!(removed(&mut window, owls), Some(Panel::Lost));
    }

    #[test]
    fn a_token_left_by_a_sign_out_is_said_by_the_place_still_there() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        window.update(Message::Pick(owls_id, Pick::SignOut), clock.at());
        window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
        window.update(Message::TokenLeft(owls(), ADA.into()), clock.at());
        assert_eq!(window.places().len(), 2, "the same place");
        let place = window.places().iter().find(|place| place.id() == owls_id).unwrap();
        assert_eq!(place.door().and_then(Door::why), Some(&Sentence::Text(words::Text::TokenLeft)));
    }

    #[test]
    fn a_server_whose_token_the_keyring_kept_comes_back_signed_out_saying_so() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let books_id = id(&window, 1);
        for _ in 0..2 {
            window.update(Message::Pick(id(&window, 0), Pick::Remove), clock.at());
            window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
        }
        assert!(window.places().is_empty() && window.welcome_door().is_some());
        window.update(Message::TokenLeft(books(), ADA.into()), clock.at());
        let back = window.open().expect("put back and open");
        assert!(back.id() != books_id && back.server().url == books().url && window.welcome_door().is_none());
        assert_eq!(back.panel(), Panel::SignedOut);
        assert_eq!(back.door().and_then(Door::why), Some(&Sentence::Text(words::Text::TokenLeft)));
        window.update(Message::TokenLeft(books(), ADA.into()), clock.at());
        assert_eq!(window.places().len(), 1, "put back once");
    }

    #[test]
    fn a_join_waiting_on_a_server_that_is_removed_is_dropped() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        in_call(&mut window, &clock, owls_id);
        tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        window.update(Message::Pick(books_id, Pick::Remove), clock.at());
        window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
        let todo = tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        assert!(
            matches!(&todo[..], [Ask::At(_, ask1), Ask::At(_, ask2)] if crate::harness::is(ask1, crate::harness::Op::History) && crate::harness::is(ask2, crate::harness::Op::ChannelsList)),
            "no join on a server gone: {todo:?}"
        );
    }

    #[test]
    fn a_deleted_accounts_server_is_removed_from_its_panel_without_asking() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        assert!(window.update(Message::Remove(owls_id), clock.at()).is_empty(), "only a deleted account's panel");
        tell(&mut window, &clock, books_id, ended(CloseReason::AccountDeleted));
        let todo = window.update(Message::Remove(books_id), clock.at());
        assert!(matches!(&todo[0], Ask::At(_, place::Ask::EndAccount { sign_out: false, .. })), "{todo:?}");
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the rail shows it gone");
    }

    fn adding(window: &mut Window, clock: &Clock, message: door::Message) -> Vec<Ask> {
        let id = window.add_dialog().unwrap().id();
        tell(window, clock, id, place::Message::Session(place::SessionMessage::Door(message)))
    }

    #[test]
    fn the_add_dialog_finds_a_server_in_the_list_without_asking_it_and_opens_it() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::Open(id(&window, 1)), clock.at());
        window.update(Message::Add, clock.at());
        assert_eq!(window.add_dialog().unwrap().door().username(), ADA, "the username typed before");
        assert!(window.welcome_door().is_none());
        adding(&mut window, &clock, door::Message::Where("chat.example.invalid".into()));
        assert!(adding(&mut window, &clock, door::Message::Submit).is_empty(), "nothing is asked of it");
        let problem = window.add_dialog().unwrap().door().problem().cloned();
        assert_eq!(problem, Some(Problem::Already { server: owls() }));
        let code = "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        adding(&mut window, &clock, door::Message::Where(format!("https://books.example.invalid/join#{code}")));
        assert!(adding(&mut window, &clock, door::Message::Submit).is_empty(), "an invite to it is the same");
        let todo = adding(&mut window, &clock, door::Message::OpenListed);
        assert!(matches!(&todo[..], [Ask::RememberOpen(url)] if *url == books().url), "{todo:?}");
        assert!(window.add_dialog().is_none());
    }

    /// Night Owls signed in, and Book club listed with no account signed in at it on this device.
    fn books_signed_out(clock: &Clock) -> (Window, PlaceId) {
        let rows = vec![door::Opening::Account(owls(), ADA.into()), door::Opening::Server(books())];
        let (mut window, _) = Window::launch(rows, None, clock.0);
        let owls_id = id(&window, 0);
        tell(&mut window, clock, owls_id, crate::harness::flow(crate::harness::token()));
        tell(&mut window, clock, owls_id, crate::harness::flow(accepted(Grants::default())));
        window.update(Message::Open(owls_id), clock.at());
        let books_id = id(&window, 1);
        assert_eq!(window.place(books_id).unwrap().panel(), Panel::SignedOut);
        (window, books_id)
    }

    #[test]
    fn an_invite_to_a_listed_server_with_no_account_here_makes_one_in_its_own_place() {
        let clock = Clock(Instant::now());
        let (mut window, books_id) = books_signed_out(&clock);
        window.update(Message::Add, clock.at());
        adding(&mut window, &clock, door::Message::Where("books.example.invalid".into()));
        adding(&mut window, &clock, door::Message::Submit);
        let problem = window.add_dialog().unwrap().door().problem().cloned();
        assert_eq!(problem, Some(Problem::Already { server: books() }), "an address opens it, to sign in there");
        let code = "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        adding(&mut window, &clock, door::Message::Where(format!("https://books.example.invalid/join#{code}")));
        let todo = adding(&mut window, &clock, door::Message::Submit);
        assert!(
            matches!(&todo[..], [Ask::At(at, place::Ask::Door(door::Ask::Go(Errand::Find { code: Some(_), .. })))]
                if *at == books_id),
            "the invite is asked about, by the place it is for: {todo:?}"
        );
        assert_eq!(window.add_dialog().unwrap().id(), books_id);
        let found = door::Found { server: books(), invite: Some(door::Invite::Works { until: 0, owner: false }) };
        adding(&mut window, &clock, door::Message::Reported(Heard::Found(Ok(found))));
        assert_eq!(window.add_dialog().unwrap().door().step(), Step::Preview);
        adding(&mut window, &clock, door::Message::Submit);
        adding(&mut window, &clock, door::Message::Username(ADA.into()));
        let password = || Secret::new("evening games with friends".into());
        adding(&mut window, &clock, door::Message::Password(password()));
        adding(&mut window, &clock, door::Message::Again(password()));
        let todo = adding(&mut window, &clock, door::Message::Submit);
        assert!(
            matches!(&todo[..], [Ask::At(at, place::Ask::Door(door::Ask::Go(Errand::Register { .. })))]
                if *at == books_id),
            "{todo:?}"
        );
        let admitted = door::Admitted { token: None, named: None, standing: standing_as("ada") };
        let todo = adding(&mut window, &clock, door::Message::Reported(Heard::Registered(Ok(admitted))));
        assert!(todo.iter().any(|done| matches!(done, Ask::At(at, ask1) if crate::harness::is(ask1, crate::harness::Op::ChannelsList) && (*at == books_id))));
        assert!(window.add_dialog().is_none());
        assert_eq!(window.places().len(), 2, "no second Book club in the rail");
        let open = window.open().unwrap();
        assert_eq!((open.id(), open.identity()), (books_id, Some(ADA)));
        assert!(open.signed_in() && open.note().is_some(), "signed in, with the welcome note");
    }

    #[test]
    fn an_add_dialog_bound_to_a_listed_server_lets_go_of_it_for_another() {
        let clock = Clock(Instant::now());
        let (mut window, books_id) = books_signed_out(&clock);
        window.update(Message::Add, clock.at());
        let code = "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        adding(&mut window, &clock, door::Message::Where(format!("https://books.example.invalid/join#{code}")));
        adding(&mut window, &clock, door::Message::Submit);
        let todo = adding(&mut window, &clock, door::Message::Cancel);
        assert!(matches!(&todo[..], [Ask::At(at, place::Ask::Door(door::Ask::Cancel))] if *at == books_id), "{todo:?}");
        adding(&mut window, &clock, door::Message::Where("garden.example.invalid".into()));
        let todo = adding(&mut window, &clock, door::Message::Submit);
        let fresh = window.add_dialog().unwrap().id();
        assert!(window.place(fresh).is_none(), "a server of its own is looked for under an id of its own");
        assert!(
            matches!(&todo[..], [Ask::At(at, place::Ask::Door(door::Ask::Go(Errand::Find { .. })))] if *at == fresh)
        );
        let garden = server("garden.example.invalid", "Plant swap");
        adding(
            &mut window,
            &clock,
            door::Message::Reported(Heard::Found(Ok(door::Found { server: garden, invite: None }))),
        );
        adding(&mut window, &clock, door::Message::Submit);
        adding(&mut window, &clock, door::Message::Password(Secret::new("evening games with friends".into())));
        adding(&mut window, &clock, door::Message::Submit);
        adding(&mut window, &clock, door::Message::Reported(Heard::Flow(accepted(Grants::default()))));
        assert_eq!(window.places().len(), 3);
        assert_eq!(window.place(books_id).unwrap().panel(), Panel::SignedOut, "Book club is as it was");
    }

    #[test]
    fn the_add_dialog_closes_on_escape_at_its_first_step_and_keeps_what_was_typed_on_a_press_outside() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::Add, clock.at());
        window.update(Message::AddOutside, clock.at());
        assert!(window.add_dialog().is_none(), "nothing typed: a press outside closes it");
        window.update(Message::Add, clock.at());
        adding(&mut window, &clock, door::Message::Where("garden.example.invalid".into()));
        window.update(Message::AddOutside, clock.at());
        assert!(window.add_dialog().is_some(), "something typed is kept");
        let todo = adding(&mut window, &clock, door::Message::Submit);
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::Find { .. })))]), "{todo:?}");
        let todo = window.update(Message::CloseAdd, clock.at());
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Door(door::Ask::Cancel))]), "the look-up is stopped");
        window.update(Message::Add, clock.at());
        adding(&mut window, &clock, door::Message::Back);
        assert!(window.add_dialog().is_none(), "Escape on the first step");
        window.update(Message::Add, clock.at());
        adding(&mut window, &clock, door::Message::Cancel);
        assert!(window.add_dialog().is_none(), "its Cancel");
    }

    #[test]
    fn a_server_added_reads_the_lists_it_said_it_offers_before_its_sign_in() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::Add, clock.at());
        let id = window.add_dialog().unwrap().id();
        adding(&mut window, &clock, door::Message::Where("garden.example.invalid".into()));
        adding(&mut window, &clock, door::Message::Submit);
        let garden = server("garden.example.invalid", "Plant swap");
        adding(
            &mut window,
            &clock,
            door::Message::Reported(Heard::Found(Ok(door::Found { server: garden, invite: None }))),
        );
        let served = crate::Served::read(&[]);
        tell(&mut window, &clock, id, place::Message::Server(crate::ServerMessage::Serves(served)));
        adding(&mut window, &clock, door::Message::Submit);
        adding(&mut window, &clock, door::Message::Password(Secret::new("evening games with friends".into())));
        adding(&mut window, &clock, door::Message::Submit);
        let todo = adding(&mut window, &clock, door::Message::Reported(Heard::Flow(accepted(Grants::default()))));
        let conversations =
            todo.iter().any(|ask| matches!(ask, Ask::At(at, ask1) if crate::harness::is(ask1, crate::harness::Op::ListConversations) && (*at == id)));
        let followed = todo.iter().any(|ask| matches!(ask, Ask::At(at, ask1) if crate::harness::is(ask1, crate::harness::Op::ListFollowed) && (*at == id)));
        assert!(conversations && followed, "{todo:?}");
    }

    #[test]
    fn what_the_server_being_added_answers_is_drawn() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::Add, clock.at());
        adding(&mut window, &clock, door::Message::Where("garden.example.invalid".into()));
        adding(&mut window, &clock, door::Message::Submit);
        let drawn = window.revision();
        let garden = server("garden.example.invalid", "Plant swap");
        adding(
            &mut window,
            &clock,
            door::Message::Reported(Heard::Found(Ok(door::Found { server: garden, invite: None }))),
        );
        assert_eq!(window.add_dialog().unwrap().door().step(), Step::Preview);
        assert_ne!(window.revision(), drawn, "the answer is drawn without waiting for a press");
    }

    #[test]
    fn a_server_added_from_the_dialog_opens_with_no_toast() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        window.update(Message::Add, clock.at());
        adding(&mut window, &clock, door::Message::Where("garden.example.invalid".into()));
        adding(&mut window, &clock, door::Message::Submit);
        let garden = server("garden.example.invalid", "Plant swap");
        adding(
            &mut window,
            &clock,
            door::Message::Reported(Heard::Found(Ok(door::Found { server: garden, invite: None }))),
        );
        adding(&mut window, &clock, door::Message::Submit);
        adding(&mut window, &clock, door::Message::Back);
        assert_eq!(window.add_dialog().unwrap().door().step(), Step::Preview, "Escape on a later step goes back one");
        adding(&mut window, &clock, door::Message::Submit);
        adding(&mut window, &clock, door::Message::Password(Secret::new("evening games with friends".into())));
        let todo = adding(&mut window, &clock, door::Message::Submit);
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::SubmitPassword(..))))]),
            "{todo:?}"
        );
        adding(&mut window, &clock, door::Message::Reported(Heard::Flow(accepted(Grants::default()))));
        assert!(window.add_dialog().is_none());
        assert_eq!(window.places().len(), 3);
        assert_eq!(window.open().unwrap().server().shown(), "Plant swap");
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "it opens: that is the answer"
        );
    }
}
