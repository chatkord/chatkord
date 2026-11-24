//! The window: every server this device was added to, each signed in and kept by itself, the one open, the welcome
//! steps, a server's menu and its dialogs, and the toast. One call at a time across them: joining on another server
//! leaves the call first. Time and randomness come in with each update.

use crate::call::floating::{Away, FloatAt, FloatMessage, Floating};
use crate::call::sound::Keep;
use crate::call::sources::Chooser;
use crate::call::voice_settings::{ProbeAsk, ProbeHeard, Testing, VoiceProbe};
use crate::channels::{ChannelAction, Deleting, DeletingCategory, Making, MakingMessage, Named, Naming};
use crate::chime::{Chime, MESSAGE_CHIME_GAP};
use crate::client_report::{Counted, Download, Finished, Machine, ReportMessage};
use crate::door::{self, Door, Opening, Problem, Server, Step};
use crate::frame::{
    CallChatAsk, Drawer, Frame, MembersAsk, MembersOver, PANE_STEP, PanePick, ROLES_BESIDE_FROM, RolesShown, Room,
    SETTINGS_BESIDE_FROM, SettingsShown, Shape,
};
use crate::invite::{DeletingInvite, InviteMessage, Inviting};
use crate::loaded::Loaded;
use crate::members::{self, BanAsk, Banning, Kicking, MemberAction, Resetting, Transferring};
use crate::notice::{Notice, Subject};
use crate::place::{self, FullScreen, Lobby, Moment, Panel, Place, PlaceId, Told};
use crate::proof::Proof;
use crate::roles::{Creating, DeletingRole};
use crate::settings::Page;
use crate::setup::{Message as SetUpMessage, SetUp};
use crate::startup::{self, TrayMenu, TrayPick};
use crate::tell::{Action, Callout, Telling, Toast};
use crate::update::{UpdateAsk, UpdateMessage, Updates};
use choices::EventSounds;
use choices::MessageSounds;
use choices::PaneWidth;
use choices::State;
use choices::Volumes;
use choices::{EncoderOption, ShareQuality};
use choices::{Look, LookPick};
use choices::{Members, MembersShown, MembersWidth};
use choices::{NoiseGate, VoiceProcessing};
use choices::{Picked, Remembered};
use choices::{Startup, Tray};
use proto::rules::{Grant, Kind};
use proto::video::Video;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use terms::{ChannelId, InviteId, LinkError, MemberId, MessageId, Secret, ServerUrl};
use words::Sentence;
use words::{Language, LanguageIdentifier, Phrase, Words};

mod about_chatkord;
mod alerts;
mod app_about;
mod awake;
mod clipboard;
mod clock;
mod debug_report;
mod device_page;
mod devices;
mod dialog;
mod drop;
mod frame;
mod language;
mod links;
mod menu;
mod message;
mod person;
mod places;
mod remembered;
mod seen;
mod tray;
mod updates;

pub use about_chatkord::{Build, Carried, Stated};
pub use alerts::{
    Badge, BadgeShown, IDLE_AFTER, IdleRun, IdleWatch, POPUP_BODY_MAX, POPUP_ID_MAX, POPUP_TITLE_MAX, PopupId,
};
pub use app_about::AppAbout;
pub use awake::Awake;
pub(crate) use clipboard::Copied;
use clipboard::Copying;
pub use clock::Revision;
pub use debug_report::ReportDialog;
pub use device_page::{DevicePage, Gear};
pub use dialog::{Asking, DeletingEmoji, DeletingMessage, Dialog, DialogMessage};
pub(crate) use dialog::{Confirming, Next};
pub use drop::DropTarget;
use frame::Folded;
pub use language::LanguageSaid;
pub use links::{Clicked, RoleShown};
pub use menu::{
    Entry, ListItem, ListMenu, ListMenuView, ListPick, ListSub, Menu, MenuFrom, Pick, SubItem, SubMenuView,
};
pub use message::{Ask, Message, NotCopied};
pub use notices::Origin;
pub use person::{AdminPart, PersonCard, PersonFrom, PersonOpen, RoleBadge};
pub use places::{Adding, ArrivalNote};
pub use seen::{FullView, PoppedOut};

/// Whether `message` is a call's levels, heard ten times a second, which say themselves whether they changed a tile.
fn levels(message: &Message) -> bool {
    matches!(message, Message::At(_, place::Message::Call(place::CallMessage::Room(crate::RoomEvent::Levels(_)))))
}

/// Everything the window shows and knows, told by [`Message`]s and answered by [`Ask`]s.
///
/// `places` and `voice` stay here: nearly every part reads them (the servers, and what this device remembers).
#[derive(Debug)]
pub struct Window {
    /// In the order they were added.
    places: Vec<Place>,
    /// What this device remembers for calls.
    voice: Remembered,
    desk: places::Desk,
    layout: frame::Layout,
    hardware: devices::DeviceSettings,
    report: debug_report::Reports,
    alerts: alerts::Alerts,
    speech: language::Speech,
    times: clock::Times,
    desktop: tray::Desktop,
}

impl Window {
    /// The window a launch opens on what a run before this one knew: every remembered account signs in at once, and
    /// the server open last time is open again. It speaks English until it is told a language.
    pub fn launch(rows: Vec<Opening>, open: Option<&ServerUrl>, now: Instant) -> (Window, Vec<Ask>) {
        let mut window = Window {
            places: Vec::new(),
            voice: Remembered::default(),
            desk: places::Desk::default(),
            layout: frame::Layout::default(),
            hardware: devices::DeviceSettings::default(),
            report: debug_report::Reports::default(),
            alerts: alerts::Alerts::default(),
            speech: language::Speech::default(),
            times: clock::Times::new(now),
            desktop: tray::Desktop::default(),
        };
        let mut todo = Vec::new();
        for row in rows {
            let (server, identity) = match row {
                Opening::Fresh => continue,
                Opening::Server(server) => (server, None),
                Opening::Account(server, identity) => (server, Some(identity)),
            };
            let id = window.issue();
            let (place, asked) = Place::remembered(id, server, identity);
            todo.extend(asked.into_iter().map(|asked| Ask::At(id, asked)));
            window.places.push(place);
        }
        window.desk.open = open
            .and_then(|url| window.places.iter().find(|place| place.server().url == *url))
            .or(window.places.first())
            .map(Place::id);
        if window.places.is_empty() {
            window.welcome();
        }
        (window, todo)
    }

    /// Every server added, in the order they were added.
    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// The server open in the pane, if one is.
    pub fn open(&self) -> Option<&Place> {
        self.desk.open.and_then(|id| self.place(id))
    }

    /// The server `id`, while the window holds it.
    pub(crate) fn place(&self, id: PlaceId) -> Option<&Place> {
        self.places.iter().find(|place| place.id() == id)
    }

    /// The toast, until its time is past: held while the pointer is on it.
    pub fn toast(&self) -> Option<&Toast> {
        self.desk.toast.as_ref().filter(|toast| toast.shows(self.times.now))
    }

    /// Takes `message` at `at`, the time and randomness it may need, and returns what the host is to carry out. A
    /// choice that changes what the content shows puts the drawer away.
    pub fn update(&mut self, message: Message, at: Moment) -> Vec<Ask> {
        let before = self.showing();
        let startup_before = self.startup_shown();
        let picked = match message {
            Message::Rail(id) => Some(id),
            _ => None,
        };
        // A page picked, from the list or from anywhere that opens settings, is the page shown.
        let page_picked = matches!(
            message,
            Message::OpenSettings
                | Message::OpenDevice(_)
                | Message::At(
                    _,
                    place::Message::Page(place::PageMessage::Settings(_) | place::PageMessage::OpenAccount)
                )
        );
        // Only a call's levels may leave what is drawn as it was, and their place says whether they did; every other
        // message is drawn, wherever it is routed.
        if !levels(&message) {
            self.times.revision.moved();
        }
        let mut todo = self.apply(message, at);
        self.float_back();
        todo.extend(self.tell_unseen());
        todo.extend(self.tell_levels());
        todo.extend(self.reschedule_update());
        if !startup_before {
            todo.extend(self.read_startup());
        }
        if page_picked {
            self.layout.settings = Folded::Page;
        }
        self.layout.drawer = match picked {
            Some(id) if self.folded() => match (before.0 == Some(id), self.layout.drawer) {
                (true, Drawer::Open) => Drawer::Shut,
                _ => Drawer::Open,
            },
            _ if self.showing() != before => Drawer::Shut,
            _ => self.layout.drawer,
        };
        todo
    }

    fn apply(&mut self, message: Message, at: Moment) -> Vec<Ask> {
        self.on_moment(at);
        if matches!(message, Message::Input | Message::Focused) {
            self.input_seen();
        }
        self.report.counted.see(&message);
        let mut todo = match message {
            Message::Rail(id) => return self.rail(id, at),
            Message::ToggleChannels => self.toggle_channels(),
            Message::CloseChannels => self.on_close_channels(),
            Message::OpenSettings => self.open_settings(at),
            Message::OpenDevice(page) => self.open_device(page),
            Message::CloseDevice => self.on_close_device(),
            Message::SettingsList => self.on_settings_list(),
            Message::Pane(pick) => self.pane(pick),
            Message::Open(id) => self.open_place(id),
            Message::PaneShare(id) => self.pane_share(id, at),
            Message::TokenLeft(server, identity) => self.token_left(server, identity),
            Message::Float(FloatMessage::To(x, y)) => self.float_to(x, y),
            Message::Float(FloatMessage::Reset) => self.on_float_reset(),
            Message::Float(FloatMessage::Close) => self.on_float_close(),
            Message::Float(FloatMessage::GoToCall) => return self.go_to_call(at),
            Message::JoinAgain(id) => self.join_again(id, at),
            Message::ToastAct => self.toast_act(at),
            Message::DismissToast => self.on_dismiss_toast(),
            Message::HoldToast(held) => self.on_hold_toast(held, at),
            Message::Tick => self.on_tick(at),
            Message::Menu(id, from) => self.open_menu(id, from),
            Message::RailScrolled(by) => self.on_rail_scrolled(by),
            Message::CloseMenu => self.on_close_menu(),
            Message::Pick(id, pick) => self.pick(id, pick, at),
            Message::Copied(done) => self.copied(done),
            Message::DeleteInvite(id, invite) => self.delete_invite(id, invite),
            Message::Member(id, action, user_id) => self.member(id, action, &user_id),
            Message::CreateChannel(id) => self.create_channel(id, None),
            Message::CreateChannelIn(id, category) => self.create_channel(id, Some(category)),
            Message::CreateCategory(id) => self.create_category(id),
            Message::ListMenu(id, key) => self.open_list_menu(id, key),
            Message::ListPick(id, key, pick) => self.list_pick(id, key, pick, at),
            Message::Person(id, user_id, from) => self.open_person(id, user_id, from),
            Message::AppAbout(id, message) => self.open_app_about(id, message),
            Message::Moderate(act) => self.moderate(act),
            Message::Status(message) => self.on_status(message),
            Message::ToggleMembers => self.toggle_members(),
            Message::CloseMembers => self.close_members(),
            Message::MembersWidth(pick) => self.members_width(pick),
            Message::Channel(id, action, channel) => self.channel(id, action, &channel),
            Message::Category(id, action, category) => self.category(id, action, &category),
            Message::CreateRole(id) => self.create_role(id),
            Message::DeleteRole(id) => self.delete_role(id),
            Message::Dialog(said) => self.confirm(said, at),
            Message::Encoders(encoders) => self.listed_encoders(encoders),
            Message::Chooser(chooser) => self.chooser(chooser),
            Message::Look(pick) => self.pick_look(pick),
            Message::EmojiTone(tone) => self.emoji_tone(tone),
            Message::NextMute => self.on_next_mute(),
            Message::NextDeafen => self.on_next_deafen(),
            Message::CloseAsked => self.close_asked(),
            Message::ToggleAtSignIn => self.startup_switched(Startup::toggled_at_sign_in),
            Message::ToggleStartsIn => self.startup_switched(Startup::toggled_starts_in),
            Message::ToggleCloseTo => self.startup_switched(Startup::toggled_close_to),
            Message::TrayShown(tray) => self.on_tray_shown(tray),
            Message::StartupRead(startup) => self.on_startup_read(startup),
            Message::NoticesRead(read) => self.notices_read(read),
            Message::Machine(machine) => self.machine_read(machine),
            Message::Report(message) => self.report_asked(message),
            Message::ToggleNotice(group) => self.toggle_notice(group),
            Message::Focused => self.on_focused(),
            Message::FocusLost => self.on_focus_lost(),
            Message::Clicked(id, clicked) => self.clicked(id, clicked, at),
            Message::FilesHovered(hovered) => self.files_hovered(hovered),
            Message::FilesDropped(paths) => self.files_dropped(paths, at),
            Message::CloseRole => self.on_close_role(),
            Message::ToggleCallChat => self.toggle_call_chat(),
            Message::CloseCallChat => self.close_call_chat(),
            Message::CallChatWidth(pick) => self.call_chat_width(pick),
            Message::SessionEnding => self.on_session_ending(),
            Message::Ended => self.on_ended(),
            Message::Shown(shown) => self.on_shown(shown),
            Message::FromTray(pick) => self.tray_picked(pick, at),
            Message::ToggleEventSounds => self.toggle_event_sounds(),
            Message::ToggleMessageSounds => self.toggle_message_sounds(),
            Message::PickMicrophone(asked) => self.on_pick_microphone(asked),
            Message::PickSpeaker(asked) => self.on_pick_speaker(asked),
            Message::PlayTestSound => self.on_play_test_sound(),
            Message::SetProcessing(asked) => self.on_set_processing(asked),
            Message::TuneProcessing(asked) => self.on_tune_processing(asked),
            Message::GateFromRoom => self.gate_from_room(at),
            Message::TestMicrophone => self.test_microphone(),
            Message::PickCamera(picked) => self.on_pick_camera(picked),
            Message::PickCameraResolution(resolution) => self.on_pick_camera_resolution(resolution),
            Message::PickCameraRate(rate) => self.on_pick_camera_rate(rate),
            Message::ToggleMirror => self.on_toggle_mirror(),
            Message::TestCamera => self.test_camera(),
            Message::Probed(heard) => match self.on_probed(heard) {
                Some(todo) => todo,
                None => return Vec::new(),
            },
            Message::DismissProbeNotice(subject) => self.dismiss_probe_notice(subject),
            Message::Add => self.add(),
            Message::CloseAdd => self.close_add(),
            Message::AddOutside => self.add_outside(),
            Message::At(id, place::Message::Session(place::SessionMessage::Door(door::Message::OpenListed)))
                if self.add_dialog().is_some_and(|adding| adding.id == id) =>
            {
                match self.on_open_listed(at) {
                    Some(todo) => todo,
                    None => return Vec::new(),
                }
            }
            Message::Remove(id) => self.on_remove(id, at),
            // Escape or Cancel on the dialog's first step closes it; on a later one Escape goes back, as on the welcome
            // steps.
            Message::At(
                id,
                place::Message::Session(place::SessionMessage::Door(door::Message::Back | door::Message::Cancel)),
            ) if self.add_dialog().is_some_and(|adding| adding.id == id && adding.door.step() == Step::Add) => {
                self.close_add()
            }
            Message::At(id, place::Message::Session(place::SessionMessage::Door(message)))
                if self.desk.adding.as_ref().is_some_and(|adding| adding.id == id) =>
            {
                self.adding_door(message, at)
            }
            Message::At(id, place::Message::Server(message))
                if self.desk.adding.as_ref().is_some_and(|adding| adding.id == id) =>
            {
                self.on_adding_told(message)
            }
            Message::At(id, place::Message::Session(place::SessionMessage::LeaveAnswered(answer))) => {
                self.left(id, answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::Invited(answer))) => self.invited(id, answer, at),
            Message::At(id, place::Message::Page(place::PageMessage::Webhooks(crate::webhooks::Message::CopyUrl))) => {
                self.copy_webhook(id)
            }
            Message::At(id, place::Message::Page(place::PageMessage::InviteDeleted(answer))) => {
                self.invite_deleted(id, &answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::RoleSet { name, answer })) => {
                self.role_set(id, name, &answer)
            }
            Message::At(id, place::Message::Page(place::PageMessage::PasswordReset(answer))) => {
                self.reset(id, answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::Transferred(answer))) => {
                self.transferred(id, &answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::Kicked(answer))) => self.kicked(id, &answer, at),
            Message::At(id, place::Message::Page(place::PageMessage::BanSet(answer))) => self.ban_set(id, &answer, at),
            Message::At(id, place::Message::Page(place::PageMessage::RoleCreated(answer))) => {
                self.role_created(id, answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::RoleChanged { name, answer })) => {
                self.role_changed(id, name, &answer)
            }
            Message::At(id, place::Message::Page(place::PageMessage::RoleDeleted(answer))) => {
                self.role_deleted(id, &answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::ChannelCreated(answer))) => {
                self.channel_made(id, answer, at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::ChannelRenamed(answer))) => {
                self.named(id, answer.map(|()| None), at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::CategoryCreated(answer))) => {
                self.named(id, answer.map(Some), at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::CategoryRenamed(answer))) => {
                self.named(id, answer.map(|()| None), at)
            }
            Message::At(id, place::Message::Page(place::PageMessage::CategoryDeleted(answer))) => {
                self.category_deleted(id, &answer, at)
            }
            Message::At(id, place::Message::Server(place::ServerMessage::Moderated { user_id, act, answer })) => {
                self.moderated(id, &user_id, act, &answer)
            }
            Message::At(id, place::Message::Talk(place::TalkMessage::StatusSet(answer))) => {
                self.status_set(id, &answer)
            }
            Message::At(id, place::Message::Page(place::PageMessage::ChannelDeleted(answer))) => {
                self.channel_deleted(id, &answer, at)
            }
            Message::At(
                id,
                place::Message::Page(place::PageMessage::Emoji(crate::EmojiPageMessage::Delete(emoji))),
            ) => self.delete_emoji(id, &emoji),
            Message::At(id, place::Message::Lobby(place::LobbyMessage::Rejoin)) => self.rejoin(id, at),
            Message::At(id, place::Message::Lobby(place::LobbyMessage::Join(channel))) => self.join(id, channel, at),
            Message::At(id, place::Message::Talk(place::TalkMessage::Message(user_id))) => {
                self.on_conversation(id, user_id, at)
            }
            Message::At(id, place::Message::Discussion(place::DiscussionMessage::Open(channel))) => {
                self.open_discussion(id, &channel)
            }
            Message::At(id, place::Message::Discussion(place::DiscussionMessage::Delete(message))) => {
                self.delete_message(id, place::Pane::Content, &message, at)
            }
            Message::At(id, place::Message::CallChat(place::DiscussionMessage::Delete(message))) => {
                self.delete_message(id, place::Pane::CallChat, &message, at)
            }
            Message::At(id, place::Message::Thread(place::DiscussionMessage::Delete(message))) => {
                self.delete_message(id, place::Pane::Thread, &message, at)
            }
            Message::At(id, place::Message::Discussion(place::DiscussionMessage::Copy(message))) => {
                self.copy_message(id, &message, at)
            }
            Message::Zone(zone) => self.on_zone(zone),
            Message::TogglePopups => self.toggle_popups(),
            Message::PopupClicked(id) => self.popup_clicked(id, at),
            Message::EarlierRunEnded => self.earlier_run_ended(),
            Message::IdleChanged(run, idleness) => self.idleness_changed(run, idleness),
            Message::IdleSourceLost(run) => self.idle_source_lost(run),
            Message::Input => Vec::new(),
            Message::Today(today) => self.on_today(today),
            Message::Update(message) => self.updated(message),
            Message::SystemLanguages(system) => self.system_languages(system),
            Message::ChooseLanguage(chosen) => self.choose_language(chosen),
            Message::LanguageMenu => self.language_menu(),
            Message::At(id, place::Message::Page(place::PageMessage::Picture(message))) if self.asking_set_up(id) => {
                self.on_picture(message, at)
            }
            Message::At(id, message) => self.at(id, message, at),
        };
        self.keep_finished_calls();
        self.settle_device();
        self.settle_call_popover();
        self.settle_language_menu();
        todo.extend(self.follow_places(at));
        todo.extend(self.follow_idleness());
        todo.extend(self.wake());
        todo.extend(self.probing());
        match self.camera_test_yielded() {
            // The test lets its camera go before the call opens it.
            Some(ended) => std::iter::once(ended).chain(todo).collect(),
            None => todo,
        }
    }

    /// The next time what the open server shows changes by itself: its countdown's number, or a full screen's
    /// controls going. Asked once per change of that time: the host keeps one wake, which a later ask replaces.
    fn wake(&mut self) -> Option<Ask> {
        let calm = self.full_screen().filter(|full| full.controls).map(|full| full.screen.calm_at);
        let (now, wall) = (self.times.now, self.times.wall);
        let places = self.places.iter().filter_map(|place| place.due(now, wall));
        let sources = self.countdown().into_iter().chain(calm).chain(places).chain(self.idle_due());
        let when = sources.filter(|when| *when > now).min()?;
        if self.times.waking == Some(when) {
            return None;
        }
        self.times.waking = Some(when);
        Some(Ask::WakeAt(when))
    }

    /// What each server's view makes due after any change: the read marks of what the open one shows in a focused
    /// window, and its call's chat column opened or put away as kept.
    fn follow_places(&mut self, at: Moment) -> Vec<Ask> {
        let (open, in_view) =
            (self.desk.open, self.desktop.focused && self.layout.shown && self.hardware.device.is_none());
        let wanted = self.voice.look.call_chat.shown == choices::CallChatShown::Open;
        let mut asked_of = Vec::new();
        for place in &mut self.places {
            let id = place.id();
            let mut asked = place.passed(at.now);
            asked.extend(place.follow_call_chat(wanted, &self.times.zone));
            let shown = in_view && open == Some(id);
            let marks = place.viewed(shown, at.now);
            asked_of.push((id, asked.into_iter().chain(marks).collect::<Vec<_>>()));
        }
        let mut todo = Vec::new();
        for (id, asked) in asked_of {
            todo.extend(asked.into_iter().filter_map(|asked| self.popup_asks(id, asked)));
        }
        todo
    }

    /// When the open server's countdown next changes its number.
    fn countdown(&self) -> Option<Instant> {
        let Panel::NotAnswering { due } = self.open()?.panel() else { return None };
        let left = due.checked_duration_since(self.times.now)?;
        let shown = whole_seconds(left);
        due.checked_sub(Duration::from_secs(shown.saturating_sub(1)))
    }
}

/// A wait as the countdown shows it: whole seconds, rounded up.
pub fn whole_seconds(left: Duration) -> u64 {
    left.as_secs() + u64::from(left.subsec_nanos() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::Errand;
    use crate::harness::{ADA, Clock, accepted, books, channels, flow, id, owls, tell, token, two_signed_in};
    use crate::place::RailState;
    use crate::sign_in::Report;
    use proto::chatkord_v1::StandingChanged;
    use proto::rules::{Grant, Grants};
    use terms::Ended;

    #[test]
    fn a_server_that_does_not_answer_counts_down_while_another_keeps_working() {
        let mut clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(
            vec![Opening::Account(owls(), ADA.into()), Opening::Account(books(), ADA.into())],
            None,
            clock.0,
        );
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        tell(&mut window, &clock, owls_id, flow(token()));
        let todo = tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        let [Ask::At(_, place::Ask::Wait { attempt: 1, delay }), Ask::WakeAt(when)] = &todo[..] else {
            panic!("{todo:?}")
        };
        assert_eq!((*delay, *when), (Duration::from_secs(1), clock.0 + Duration::from_secs(1)));
        assert!(matches!(window.places()[0].panel(), Panel::NotAnswering { .. }));
        assert_eq!(window.places()[0].rail(), RailState::NotAnswering);
        tell(&mut window, &clock, books_id, flow(token()));
        tell(&mut window, &clock, books_id, flow(accepted(Grants::default())));
        tell(&mut window, &clock, books_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        assert_eq!(window.places()[1].panel(), Panel::Discussion, "the other server is not held up");
        // The next try is unanswered too: a two-second wait, its number changing once a second.
        clock.pass(Duration::from_secs(1));
        assert!(window.update(Message::Tick, clock.at()).is_empty(), "the place's own timer starts the try");
        let second = clock.0 + Duration::from_secs(1);
        let started = tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Reconnect(1)));
        // The wake the quiet try asks for is the countdown's next number too, so the failure asks for none.
        assert!(started.iter().any(|asked| matches!(asked, Ask::WakeAt(when) if *when == second)), "{started:?}");
        let todo = tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Wait { attempt: 2, .. })]), "{todo:?}");
        clock.pass(Duration::from_secs(1));
        let todo = window.update(Message::Tick, clock.at());
        assert!(matches!(&todo[..], [Ask::WakeAt(when)] if *when == second + Duration::from_secs(1)), "{todo:?}");
    }

    #[test]
    fn try_now_tries_at_once_and_the_timer_of_the_old_try_is_dropped() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let owls_id = id(&window, 0);
        tell(&mut window, &clock, owls_id, flow(token()));
        tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::TryNow));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::TryToken(..))))]), "{todo:?}");
        assert!(matches!(window.places()[0].panel(), Panel::NotAnswering { .. }), "a try under way says nothing yet");
        assert!(
            tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Reconnect(1))).is_empty(),
            "the old timer"
        );
        tell(&mut window, &clock, owls_id, flow(accepted(Grants::default())));
        assert_eq!(window.places()[0].panel(), Panel::Channels);
    }

    fn failed_once(clock: &Clock) -> (Window, PlaceId) {
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let owls_id = id(&window, 0);
        tell(&mut window, clock, owls_id, flow(token()));
        tell(&mut window, clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        (window, owls_id)
    }

    #[test]
    fn a_try_that_fails_at_once_never_says_connecting() {
        let mut clock = Clock(Instant::now());
        let (mut window, owls_id) = failed_once(&clock);
        let held = |window: &Window| {
            assert!(
                matches!(window.places()[0].panel(), Panel::NotAnswering { .. }),
                "{:?}",
                window.places()[0].panel()
            );
            assert_eq!(window.places()[0].rail(), RailState::NotAnswering);
        };
        clock.pass(Duration::from_secs(1));
        window.update(Message::Tick, clock.at());
        tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Reconnect(1)));
        held(&window);
        clock.pass(Duration::from_millis(5));
        window.update(Message::Tick, clock.at());
        held(&window);
        tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        held(&window);
        window.update(Message::Tick, clock.at());
        held(&window);
    }

    #[test]
    fn a_try_that_runs_says_connecting_after_a_moment_and_holds_it() {
        let mut clock = Clock(Instant::now());
        let (mut window, owls_id) = failed_once(&clock);
        clock.pass(Duration::from_secs(1));
        tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Reconnect(1)));
        clock.pass(design::CONNECTING_AFTER);
        window.update(Message::Tick, clock.at());
        assert_eq!(window.places()[0].panel(), Panel::Connecting, "a try that has run a moment");
        clock.pass(Duration::from_millis(10));
        tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        assert_eq!(window.places()[0].panel(), Panel::Connecting, "held, not a flicker");
        clock.pass(design::CONNECTING_HELD);
        let todo = window.update(Message::Tick, clock.at());
        assert!(todo.iter().any(|done| matches!(done, Ask::At(_, place::Ask::Wait { attempt: 2, .. }))), "{todo:?}");
        assert!(matches!(window.places()[0].panel(), Panel::NotAnswering { .. }));
    }

    #[test]
    fn a_late_report_for_a_server_the_window_never_had_reaches_nothing() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        assert!(
            tell(
                &mut window,
                &clock,
                PlaceId(99),
                place::Message::Session(place::SessionMessage::Disconnected(Ended::default()))
            )
            .is_empty()
        );
        assert!(window.places().iter().all(|place| place.panel() == Panel::Discussion));
    }

    #[test]
    fn being_given_a_higher_role_is_toasted_once_and_the_toast_goes() {
        let mut clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        // As the server words it.
        let changed = StandingChanged {
            place: crate::harness::ADMIN_PLACE.wire(),
            grants: vec!["MANAGE_MEMBERS".into(), "INVITE_MEMBER".into()],
        };
        let admin = place::Message::standing(&changed);
        let todo = tell(&mut window, &clock, owls_id, admin.clone());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask1), Ask::WakeAt(_)] if crate::harness::is(ask1, crate::harness::Op::ChannelsList)),
            "{todo:?}"
        );
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("You have a higher role on Night Owls now")
        );
        assert!(window.places()[0].grants().holds(Grant::InviteMember));
        clock.pass(design::TOAST_PLAIN_FOR);
        window.update(Message::Tick, clock.at());
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None);
        tell(&mut window, &clock, owls_id, admin);
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "no news the second time");
        let lowered = StandingChanged { place: proto::rules::Place::BOTTOM.wire(), grants: Vec::new() };
        tell(&mut window, &clock, owls_id, place::Message::standing(&lowered));
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "a role taken is no news");
        window.update(Message::Open(owls_id), clock.at());
    }

    #[test]
    fn picking_a_server_puts_the_toast_away_and_is_remembered() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let books_id = id(&window, 1);
        let admin: Grants = [Grant::ManageMembers].into_iter().collect();
        tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: crate::harness::ADMIN_PLACE,
                grants: admin,
            }),
        );
        let todo = window.update(Message::Open(books_id), clock.at());
        assert!(matches!(&todo[..], [Ask::RememberOpen(url)] if *url == books().url), "{todo:?}");
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None);
        assert_eq!(window.open().unwrap().id(), books_id);
    }
}
