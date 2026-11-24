//! What the tests of the screens, this crate's and the shell's, build their windows from: two servers, a clock moved by
//! hand, sign-ins, a call and a channel, each made the one way here.
#![expect(clippy::unwrap_used, reason = "a test's fixture: a step that does not hold is the test failing")]

use crate::call::room::{End, Event as RoomEvent, Lane, Person, Track};
use crate::door::{self, Heard, Server};
use crate::place::{self, Channel, Moment, PlaceId};
use crate::sign_in::{Report, Standing};
use crate::window::{Ask, Message, Pick, Window};
use net::Jitter;
use proto::chatkord_v1::CloseReason;
use proto::rules::{Grant, Grants, Place as RolePlace};
use std::time::{Duration, Instant};
use terms::{Ended, LinkError, MemberId, Secret, ServerUrl};

/// The username every fixture signs in with.
pub const ADA: &str = "Ada";

/// Where an administrator stands in the fixtures' roles' order: as a new server places its `admin` role.
pub const ADMIN_PLACE: RolePlace = RolePlace::at(u32::MAX / 2);

/// The text channel every fixture's server lists.
pub const GENERAL: &str = "general";

/// A server at `host`, named `name`, with no colour or picture.
pub fn server(host: &str, name: &str) -> Server {
    Server {
        url: ServerUrl::parse(&format!("wss://{host}/v1")).unwrap(),
        name: Some(name.into()),
        colour: None,
        picture: None,
    }
}

/// Night Owls, the first server.
pub fn owls() -> Server {
    server("chat.example.invalid", "Night Owls")
}

/// Book club, the second server.
pub fn books() -> Server {
    server("books.example.invalid", "Book club")
}

/// The machine a picture of the debug report states: Fedora on Wayland, an AMD adapter on Vulkan.
pub fn machine() -> crate::Machine {
    use report::{Fact, Product, Section, word};
    let mut system = Section::new("System");
    system.push("system", &Product::fact_of("Fedora Linux 44 (KDE Plasma)")).unwrap();
    system.push("version", &Product::fact_of("6.17.4-200.fc44.x86_64")).unwrap();
    system.push("session", &Fact::Word(word!("Wayland"))).unwrap();
    let mut graphics = Section::new("Graphics");
    graphics.push("adapter", &Product::fact_of("AMD Radeon RX 9060 XT")).unwrap();
    graphics.push("driver", &Product::fact_of("radv")).unwrap();
    graphics.push("driver info", &Product::fact_of("Mesa 26.0.1")).unwrap();
    graphics.push("backend", &Fact::Word(word!("Vulkan"))).unwrap();
    crate::Machine { system, graphics }
}

/// A server's debug report as `chatkord-server` writes one, which the pictures of its listing show.
pub const SERVER_REPORT: &str = include_str!("server_report_sample.txt");

/// The wall clock every fixture's moment reads, in seconds since the epoch.
pub const WALL: i64 = 1_763_035_200;

/// A clock the test moves by hand, and no jitter.
#[derive(Debug)]
pub struct Clock(pub Instant);

impl Clock {
    /// The moment it shows, with the middle jitter and a fixed nonce.
    pub fn at(&self) -> Moment {
        Moment { now: self.0, jitter: Jitter::from_bits(1 << 63), nonce: terms::Nonce::new([7; 16]), wall: WALL }
    }

    /// Moves it on by `by`.
    pub fn pass(&mut self, by: Duration) {
        self.0 += by;
    }
}

/// What the door reports while signing in, as its place hears it.
pub fn flow(report: Report) -> place::Message {
    place::Message::Session(place::SessionMessage::Door(door::Message::Reported(Heard::Flow(report))))
}

/// The member the server knows as `id`.
pub fn member(id: &str) -> MemberId {
    MemberId::parse(id).unwrap()
}

/// What the server says of `user_id` as it lets it in: no name, colour or words, owning nothing.
pub fn standing_as(user_id: &str) -> Standing {
    Standing {
        user_id: member(user_id),
        username: user_id.to_string(),
        shown: None,
        place: RolePlace::BOTTOM,
        must_change: false,
        grants: Grants::default(),
        colour: None,
    }
}

/// A sign-in accepted as `ada`, holding `grants`.
pub fn accepted(grants: Grants) -> Report {
    Report::Accepted { token: None, named: None, standing: Standing { grants, ..standing_as("ada") } }
}

/// A sign-in accepted as `user_id`, holding nothing.
pub fn accepted_as(user_id: &str) -> Report {
    Report::Accepted { token: None, named: None, standing: standing_as(user_id) }
}

/// The keyring's token, found.
pub fn token() -> Report {
    Report::TokenLoaded(Some(Secret::new("t".into())))
}

/// The channel the server knows as `id`.
pub fn channel_id(id: &str) -> terms::ChannelId {
    terms::ChannelId::parse(id).unwrap()
}

/// The timeline the server knows as `id`.
pub fn timeline_id(id: &str) -> terms::TimelineId {
    terms::TimelineId::parse(id).unwrap()
}

/// The invite the server knows as `id`.
pub fn invite_id(id: &str) -> terms::InviteId {
    terms::InviteId::parse(id).unwrap()
}

/// The role the server knows as `id`.
pub fn role_id(id: &str) -> terms::RoleId {
    terms::RoleId::parse(id).unwrap()
}

/// A role as the server lists it, standing at `at` with no colour: the owner role where `at` is the owner's place.
pub fn role_entry(id: &str, name: &str, at: RolePlace, grants: &[Grant]) -> proto::chatkord_v1::RoleEntry {
    proto::chatkord_v1::RoleEntry {
        role_id: id.into(),
        name: name.into(),
        is_owner: at.is_owner(),
        is_default: false,
        position: at.position(),
        grants: grants.iter().map(|grant| grant.word().to_string()).collect(),
        colour: None,
    }
}

/// [`role_entry`] as the default role, the one every member holds.
pub fn default_role_entry(id: &str, name: &str, at: RolePlace, grants: &[Grant]) -> proto::chatkord_v1::RoleEntry {
    proto::chatkord_v1::RoleEntry { is_default: true, ..role_entry(id, name, at, grants) }
}

/// The owner role of [`server_roles`].
pub const OWNER_ROLE: &str = "11";
/// The admin role of [`server_roles`].
pub const ADMIN_ROLE: &str = "12";
/// The default role of [`server_roles`].
pub const MEMBER_ROLE: &str = "13";

/// The roles a new server lists: the owner's, Admin holding every word at [`ADMIN_PLACE`], and Member, the default.
pub fn server_roles() -> Vec<proto::chatkord_v1::RoleEntry> {
    vec![
        role_entry(OWNER_ROLE, "Owner", RolePlace::OWNER, &Grant::ALL),
        role_entry(ADMIN_ROLE, "Admin", ADMIN_PLACE, &Grant::ALL),
        default_role_entry(MEMBER_ROLE, "Member", RolePlace::BOTTOM, &[]),
    ]
}

/// The place `id` reads [`server_roles`], as it does on signing in.
pub fn roles_listed(window: &mut Window, clock: &Clock, id: PlaceId) {
    tell(window, clock, id, place::Message::Server(place::ServerMessage::RolesListed(Ok(server_roles()))));
}

/// #general's timeline.
pub fn general_timeline() -> terms::TimelineId {
    terms::TimelineId::parse("t-general").unwrap()
}

/// The composer of `#general`'s timeline.
pub fn general_compose() -> crate::place::Compose {
    crate::place::Compose::Timeline(general_timeline())
}

/// `listed` with every channel in one category, Chat, in the order the channels are held, text before voice.
pub fn in_chat(mut listed: place::ChannelList) -> place::ChannelList {
    let chat = terms::CategoryId::parse("chat").unwrap();
    listed.categories.push(place::Category { id: chat.clone(), name: "Chat".into(), position: 0 });
    let text = listed.text.iter().map(|channel| channel.id.clone());
    let ids: Vec<_> = text.chain(listed.voice.iter().map(|channel| channel.id.clone())).collect();
    for (position, id) in (0..).zip(ids) {
        listed.placed.insert(id, place::Placement { category: chat.clone(), position });
    }
    listed
}

/// The server's channels: #general, and the voice channel Lounge with no one in it, both in the category Chat.
pub fn channels() -> place::ChannelList {
    in_chat(bare_channels())
}

fn bare_channels() -> place::ChannelList {
    place::ChannelList {
        text: vec![place::TextChannel {
            id: crate::harness::channel_id(GENERAL),
            name: "general".into(),
            timeline: general_timeline(),
            revision: 0,
        }],
        voice: vec![Channel {
            id: crate::harness::channel_id("lounge"),
            name: "Lounge".into(),
            people: Vec::new(),
            timeline: terms::TimelineId::parse("t-lounge").unwrap(),
        }],
        categories: Vec::new(),
        placed: Default::default(),
        version: 0,
        standings: Default::default(),
        private: Default::default(),
    }
}

/// [`channels`], this account holding `grants` in each, as a server with no overwrite there lists them.
pub fn channels_holding(grants: Grants) -> place::ChannelList {
    let mut listed = channels();
    let ids = listed.text.iter().map(|channel| channel.id.clone()).chain(listed.voice.iter().map(|c| c.id.clone()));
    listed.standings =
        ids.map(|id| (id, place::ChannelStanding { grants, ..place::ChannelStanding::default() })).collect();
    listed
}

/// The id of the server added `at`th.
pub fn id(window: &Window, at: usize) -> PlaceId {
    window.places()[at].id()
}

/// `message` for the server `id`, at the clock's moment.
pub fn tell(window: &mut Window, clock: &Clock, id: PlaceId, message: place::Message) -> Vec<Ask> {
    window.update(Message::At(id, message), clock.at())
}

/// What the call on `place` is told.
pub fn call_says(window: &mut Window, clock: &Clock, place: PlaceId, message: place::CallMessage) -> Vec<Ask> {
    tell(window, clock, place, place::Message::Call(message))
}

/// The connection ended by the server, for `reason`.
pub fn ended(reason: CloseReason) -> place::Message {
    place::Message::Session(place::SessionMessage::Disconnected(Ended { reason: Some(reason), ..Ended::default() }))
}

/// The call closed by the server because this account joined from elsewhere.
pub fn moved() -> place::Message {
    place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(End::Closed(Some(CloseReason::Moved)))))
}

/// The server refusing with `code`.
pub fn refused(code: proto::chatkord_v1::ErrorCode) -> LinkError {
    LinkError::Refused(code)
}

/// Two remembered accounts, both signed in on their tokens, with their channels listed.
pub fn two_signed_in(clock: &Clock) -> Window {
    let rows = vec![door::Opening::Account(owls(), ADA.into()), door::Opening::Account(books(), ADA.into())];
    let (mut window, _) = Window::launch(rows, None, clock.0);
    for at in 0..2 {
        let place = id(&window, at);
        tell(&mut window, clock, place, flow(token()));
        tell(&mut window, clock, place, flow(accepted(Grants::default())));
        tell(&mut window, clock, place, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
    }
    window
}

/// One account at Night Owls, signed in holding `grants`, its channels listed.
pub fn holding(clock: &Clock, grants: Grants) -> (Window, PlaceId) {
    placed_holding(clock, RolePlace::BOTTOM, grants)
}

/// [`holding`], at `place` in the roles' order from the sign-in on.
pub fn placed_holding(clock: &Clock, place: RolePlace, grants: Grants) -> (Window, PlaceId) {
    let (mut window, _) = Window::launch(vec![door::Opening::Account(owls(), ADA.into())], None, clock.0);
    let owls_id = id(&window, 0);
    tell(&mut window, clock, owls_id, flow(token()));
    let standing = Standing { place, grants, ..standing_as("ada") };
    tell(&mut window, clock, owls_id, flow(Report::Accepted { token: None, named: None, standing }));
    let listed = channels_holding(grants);
    tell(&mut window, clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(listed))));
    (window, owls_id)
}

/// [`holding`] the word to invite.
pub fn inviter(clock: &Clock) -> (Window, PlaceId) {
    holding(clock, [Grant::InviteMember].into_iter().collect())
}

/// Night Owls's admin at a server that greeted offering its name and colour, with Overview open.
pub fn overview_open(clock: &Clock) -> (Window, PlaceId) {
    let mut window = two_signed_in(clock);
    let owls = id(&window, 0);
    let identity = terms::ServerIdentity { name: Some("Night Owls".into()), colour: None, picture: None };
    tell(
        &mut window,
        clock,
        owls,
        place::Message::Server(place::ServerMessage::Greeted { identity, limits: terms::FileLimits::default() }),
    );
    tell(
        &mut window,
        clock,
        owls,
        place::Message::Session(place::SessionMessage::Standing {
            place: ADMIN_PLACE,
            grants: [Grant::ManageServer, Grant::ManageMembers].into_iter().collect(),
        }),
    );
    window.update(Message::Open(owls), clock.at());
    window.update(Message::Pick(owls, Pick::Settings), clock.at());
    (window, owls)
}

/// [`overview_open`] with the server's About page open.
pub fn about_open(clock: &Clock) -> (Window, PlaceId) {
    let (mut window, owls) = overview_open(clock);
    tell(&mut window, clock, owls, place::Message::Page(place::PageMessage::Settings(crate::Page::About)));
    (window, owls)
}

/// Something done in the name field of the account page of the server `id`.
pub fn account_name(window: &mut Window, clock: &Clock, id: PlaceId, message: crate::name_field::Message) -> Vec<Ask> {
    tell(window, clock, id, place::Message::Page(place::PageMessage::Name(message)))
}

/// Joins Lounge on `place` and hears the room's answer, with Ada alone in it and both of her lanes up.
pub fn in_call(window: &mut Window, clock: &Clock, place: PlaceId) {
    let todo = tell(
        window,
        clock,
        place,
        place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
    );
    assert!(matches!(&todo[..], [Ask::At(at, place::Ask::Join { .. })] if *at == place), "{todo:?}");
    let joined = RoomEvent::Joined {
        members: vec![Person::new(member("ada"), ADA)],
        streams: vec![(member("ada"), Track::Microphone)],
    };
    tell(window, clock, place, place::Message::Call(place::CallMessage::Room(joined)));
    let up = RoomEvent::Lanes { send: Lane::Up, receive: Lane::Up };
    tell(window, clock, place, place::Message::Call(place::CallMessage::Room(up)));
}

/// Signed in as `ada` at the first server, with its channels listed and a call on Lounge joined.
pub fn ada_in_call(clock: &Clock) -> (Window, PlaceId) {
    let (mut window, _) = Window::launch(vec![door::Opening::Account(owls(), ADA.into())], None, clock.0);
    let place = id(&window, 0);
    tell(&mut window, clock, place, flow(token()));
    tell(&mut window, clock, place, flow(accepted_as("ada")));
    tell(&mut window, clock, place, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
    in_call(&mut window, clock, place);
    (window, place)
}

/// Ada, at `at` in the roles' order and holding `grants`, in a call on Lounge with Bo (a member), Cy (an admin) and Di
/// (the owner), who the server places as said.
pub fn moderator_in_call(clock: &Clock, at: RolePlace, grants: Grants) -> (Window, PlaceId) {
    let (mut window, _) = Window::launch(vec![door::Opening::Account(owls(), ADA.into())], None, clock.0);
    let place = id(&window, 0);
    tell(&mut window, clock, place, flow(token()));
    let standing = Standing { place: at, grants, ..standing_as("ada") };
    tell(&mut window, clock, place, flow(Report::Accepted { token: None, named: None, standing }));
    let listed = channels_holding(grants);
    tell(&mut window, clock, place, place::Message::Lobby(place::LobbyMessage::Channels(Ok(listed))));
    in_call(&mut window, clock, place);
    for (user_id, name) in [("bo", "Bo"), ("cy", "Cy"), ("di", "Di")] {
        tell(
            &mut window,
            clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::MemberJoined {
                user_id: member(user_id),
                name: name.into(),
                colour: None,
            })),
        );
    }
    let person = |user_id: &str, name: &str, at: RolePlace| place::Occupant {
        user_id: member(user_id),
        colour: None,
        name: name.into(),
        state: choices::State::default(),
        sharing: false,
        camera: false,
        held: false,
        place: at,
    };
    let people = vec![
        person("ada", ADA, at),
        person("bo", "Bo", proto::rules::Place::BOTTOM),
        person("cy", "Cy", ADMIN_PLACE),
        person("di", "Di", proto::rules::Place::OWNER),
    ];
    tell(
        &mut window,
        clock,
        place,
        place::Message::Lobby(place::LobbyMessage::Occupancy {
            channel_id: crate::harness::channel_id("lounge"),
            people,
        }),
    );
    (window, place)
}

/// Ada in a call with Bo, Cy and Di, holding `grants`.
pub fn in_call_offering_a_camera(clock: &Clock, grants: Grants) -> (Window, PlaceId) {
    moderator_in_call(clock, RolePlace::BOTTOM, grants)
}

/// Ada in a call with Bo, Cy and Di, allowed to share a screen on a device that encodes H.264, and Bo sharing one.
pub fn sharer_in_call(clock: &Clock) -> (Window, PlaceId) {
    let grants = Grants::from_words(&["SHARE_SCREEN"]);
    let (mut window, place) = moderator_in_call(clock, RolePlace::BOTTOM, grants);
    window.update(
        Message::Encoders(vec![choices::EncoderOption {
            codec: choices::Codec::H264,
            on: choices::EncodesOn::GraphicsCard,
        }]),
        clock.at(),
    );
    tell(
        &mut window,
        clock,
        place,
        place::Message::Call(place::CallMessage::Room(RoomEvent::Started {
            user_id: member("bo"),
            track: Track::ScreenVideo,
        })),
    );
    (window, place)
}

/// Bo's screen playing in a call, `place` the call's.
pub fn watching_bo(clock: &Clock) -> (Window, PlaceId) {
    let (mut window, place) = sharer_in_call(clock);
    let card = crate::Watched::Card(picture_of("bo"));
    tell(&mut window, clock, place, place::Message::Call(place::CallMessage::Watched(card)));
    (window, place)
}

/// The open server's call.
pub fn the_call(window: &Window) -> &place::Call {
    window.open().and_then(place::Place::call).unwrap()
}

/// The grid's cards in words: a person by name, a screen by whose it is.
pub fn cards(window: &Window) -> Vec<String> {
    the_call(window)
        .cards()
        .iter()
        .map(|card| match card {
            crate::Card::Person(tile) => tile.name.clone(),
            crate::Card::OwnScreen(_) => "own screen".into(),
            crate::Card::Screen { sharer, .. } => format!("{}'s screen", sharer.name),
        })
        .collect()
}

/// A playing picture of `who`'s screen, as the call reports it once a second.
pub fn picture_of(who: &str) -> crate::ShareCard {
    crate::ShareCard {
        who: member(who),
        width: 1920,
        height: 1080,
        frame_rate: Some(30),
        dropped: 0,
        codec: "H.264".into(),
        decoder: crate::Route::Hardware { via: "VA-API".into() },
        refused: 0,
        freezes: (0, std::time::Duration::ZERO),
        state: crate::ShareState::Live,
    }
}

/// A laptop's and a headset's microphones and speakers, the system's default first.
pub fn sound() -> crate::Sound {
    let listed =
        |asked: Option<&str>, label: &str| crate::Listed { asked: asked.map(str::to_string), label: label.into() };
    crate::Sound {
        microphones: vec![listed(None, "Laptop"), listed(Some("Headset"), "Headset")],
        speakers: vec![listed(None, "Laptop"), listed(Some("Headset"), "Headset")],
        microphone: None,
        speaker: None,
    }
}

/// Night Owls offering messages, with this account allowed to send, its #general open and its newest page read.
pub fn chatting(clock: &Clock) -> (Window, PlaceId) {
    let (mut window, _) = Window::launch(vec![door::Opening::Account(owls(), ADA.into())], None, clock.0);
    let owls = id(&window, 0);
    tell(&mut window, clock, owls, flow(token()));
    let ada = standing_as(ADA);
    tell(&mut window, clock, owls, flow(Report::Accepted { token: None, named: None, standing: ada }));
    // The channels arriving with none open open the first text one, #general.
    let todo = standing(&mut window, clock, owls, &[Grant::ChannelRead, Grant::SendMessage]);
    assert!(
        matches!(&todo[..], [Ask::At(_, ask)] if sends(ask, proto::chatkord_v1::History { timeline_id: general_timeline().as_str().into(), anchor: None, limit: 0 })),
        "{todo:?}"
    );
    let page = chat::Page { messages: vec![said(1, 5, ADA, "hello")], older: false, newer: false, revision: 5 };
    tell(&mut window, clock, owls, first_page(page));
    (window, owls)
}

/// The server telling Night Owls's account it holds `words`, and owns nothing; the list it then reads holds them in
/// each channel. What the list's arrival asks.
pub fn standing(window: &mut Window, clock: &Clock, owls: PlaceId, words: &[Grant]) -> Vec<Ask> {
    let grants = words.iter().copied().collect();
    tell(
        window,
        clock,
        owls,
        place::Message::Session(place::SessionMessage::Standing { place: proto::rules::Place::BOTTOM, grants }),
    );
    tell(window, clock, owls, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels_holding(grants)))))
}

/// Something for the open text channel.
pub fn discussion(message: place::DiscussionMessage) -> place::Message {
    place::Message::Discussion(message)
}

/// #general's newest page, read.
pub fn first_page(page: chat::Page) -> place::Message {
    discussion(place::DiscussionMessage::Page { timeline: general_timeline(), end: None, page: Ok(page) })
}

/// `message` told by the server in #general.
pub fn told(message: chat::Message) -> place::Message {
    discussion(place::DiscussionMessage::Told { timeline: general_timeline(), message: Box::new(message) })
}

/// The id of the message at `place` in its channel.
pub fn message_id(place: u64) -> chat::MessageId {
    chat::MessageId::parse(&format!("m{place}")).unwrap()
}

/// A message `author` wrote, `body`, at `place` in its channel and at `revision`.
pub fn said(place: u64, revision: u64, author: &str, body: &str) -> chat::Message {
    chat::Message {
        id: message_id(place),
        author: chat::Author::Member(member(author)),
        place,
        revision,
        body: Some(chat::Body::plain(body)),
        created_at: 1_763_035_200,
        edited_at: None,
        attachments: Vec::new(),
        reply: None,
        mention_author: false,
        reactions: Vec::new(),
        mine: Vec::new(),
        pinned_at: None,
        thread: None,
        preview: None,
    }
}

/// The server greeting with every switch on.
pub fn every_switch_on(window: &mut Window, clock: &Clock, id: PlaceId) {
    switched_off(window, clock, id, &[]);
}

/// The server greeting with the switches `off` set off and the rest on.
pub fn switched_off(window: &mut Window, clock: &Clock, id: PlaceId, off: &[proto::rules::Switch]) {
    let served = place::Served::read(off);
    tell(window, clock, id, place::Message::Server(place::ServerMessage::Serves(served)));
}

/// The people the listing a sign-in reads answers with: Ada, as the account signed in at `id`, grace and Bo signed in,
/// mira not.
pub fn people_listed(window: &mut Window, clock: &Clock, id: PlaceId) {
    people_listed_as(window, clock, id, proto::status::Shown::Offline, &[]);
}

/// The people [`people_listed`] lists, mira `mira`, and each member `held` names by id holding the roles it names by
/// id.
pub fn people_listed_as(
    window: &mut Window,
    clock: &Clock,
    id: PlaceId,
    mira: proto::status::Shown,
    held: &[(&str, &[&str])],
) {
    let me = window.places().iter().find(|place| place.id() == id).and_then(place::Place::me).cloned().unwrap();
    let named = |user_id: &str, name: &str| {
        let roles = held.iter().filter(|(holder, _)| *holder == user_id).flat_map(|(_, roles)| roles.iter());
        place::Named {
            name: name.into(),
            colour: None,
            roles: roles.map(|role| role_id(role)).collect(),
            picture: None,
        }
    };
    let online = proto::status::Shown::Online;
    let others = [("grace", "grace", online), ("bo", "Bo", online), ("mira", "mira", mira)]
        .map(|(user_id, name, state)| (member(user_id), named(user_id, name), state));
    let entries = std::iter::once((me.clone(), named(me.as_str(), ADA), online)).chain(others).collect();
    let page = place::PeoplePage { entries, next: None, presence: 1 };
    tell(window, clock, id, place::Message::People(place::PeopleMessage::Page(Ok(page))));
}

/// Bo, a member the directory names.
pub fn bo_arrives(window: &mut Window, clock: &Clock, id: PlaceId) {
    let person = place::Named { name: "Bo".into(), colour: None, roles: Vec::new(), picture: None };
    let arrived = place::PeopleMessage::Arrived { user_id: member("bo"), person };
    tell(window, clock, id, place::Message::People(arrived));
}

/// The conversation `id` between Ada and `with`, with its timeline `t-{id}`; `with` none for one whose other member
/// is gone.
pub fn conversation_wire(id: &str, with: Option<&str>, sendable: bool) -> proto::chatkord_v1::Conversation {
    let mut user_ids = vec!["Ada".to_owned()];
    user_ids.extend(with.map(str::to_owned));
    proto::chatkord_v1::Conversation {
        conversation_id: id.into(),
        timeline_id: format!("t-{id}"),
        user_ids,
        revision: 1,
        standing: None,
        last: None,
        sendable,
    }
}

/// A conversation told to Ada's sockets.
pub fn conversation_told(window: &mut Window, clock: &Clock, id: PlaceId, told: proto::chatkord_v1::Conversation) {
    let message = place::Message::Talk(place::TalkMessage::Opened(Box::new(told)));
    tell(window, clock, id, message);
}

/// The build About Chatkord states in the design, whatever version runs.
pub const BUILD: crate::window::Build =
    crate::window::Build { version: "0.9.0", protocol: proto::envelope::PROTO, licence: "GPL-3.0-or-later" };

/// The notices About Chatkord shows in the design: projects the package builds, each with where it came from, then
/// the Rust packages under a licence, the fonts and the art.
pub fn notices() -> notices::Notices {
    notices::Notices::parse(NOTICES.as_bytes()).unwrap()
}

const NOTICES: &str = r#"{"built": {"version": "0.9.0", "from": {"tag": "v0.9.0"}}, "groups": [
    {"name": "FFmpeg", "what": "Decodes and encodes video", "licence": "GPL-2.0-or-later",
        "source": "Built from commit 37515cfd with our patches. The recipe is in this package.",
        "text": "GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991\n\nCopyright (C) 1989, 1991 Free Software Foundation, Inc. 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA\n\nEveryone is permitted to copy and distribute verbatim copies of this license document, but changing it is not allowed.\n\nPreamble\n\nThe licenses for most software are designed to take away your freedom to share and change it. By contrast, the GNU General Public License is intended to guarantee your freedom to share and change free software…"},
    {"name": "x264", "what": "Encodes H.264 when no graphics card can", "licence": "GPL-2.0-or-later",
        "source": "Built from commit b35605ac.", "text": "The licence’s full text."},
    {"name": "WebRTC", "what": "Calls: sound, pictures and the network", "licence": "BSD-3-Clause and others",
        "source": "LiveKit’s build webrtc-a545de4 of webrtc-sdk m150.", "text": "The licence’s full text."},
    {"name": "dav1d and SVT-AV1", "what": "AV1 pictures", "licence": "BSD-2-Clause, BSD-3-Clause-Clear",
        "source": "SVT-AV1 comes with the Alliance for Open Media’s patent licence.", "text": "The licence’s full text."},
    {"name": "MIT", "what": "248 Rust packages: iced, tokio, serde and more", "licence": "MIT",
        "text": "The licence’s full text."},
    {"name": "Apache-2.0", "what": "197 Rust packages: wgpu, rustls, prost and more", "licence": "Apache-2.0",
        "text": "The licence’s full text."},
    {"name": "Manrope and IBM Plex Mono", "what": "The fonts", "licence": "OFL-1.1", "text": "The licence’s full text."},
    {"name": "Twemoji and Lucide", "what": "Emoji and icons",
        "licence": "CC-BY-4.0, ISC", "text": "The licence’s full text."}
]}"#;

/// A toast of `telling` for the first place, shown from `at`, as a front end's tests draw one.
pub fn toast(telling: crate::Telling, at: Instant) -> crate::Toast {
    crate::Toast::new(PlaceId(0), telling, at)
}

/// The kind of op a place asks of its server, as tests name it.
pub use proto::chatkord_v1::wire_message::Kind as Op;

/// Whether `ask` sends an op of `kind` to the server, whatever it holds.
pub fn is(ask: &place::Ask, kind: Op) -> bool {
    ask.op().is_some_and(|op| Op::from(op) == kind)
}

/// Whether `ask` sends exactly `op` to the server.
pub fn sends(ask: &place::Ask, op: impl Into<proto::chatkord_v1::wire_message::Body>) -> bool {
    ask.op() == Some(&op.into())
}

/// The ops a place's asks hold, for a test to match on.
pub use proto::chatkord_v1::wire_message::Body;

/// The schema's ops, for a test to build the op it expects.
pub use proto::chatkord_v1 as wire;
