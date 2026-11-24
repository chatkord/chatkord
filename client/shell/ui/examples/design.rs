#![expect(clippy::print_stdout, reason = "an example's output is what it prints")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a developer's tool: a failed step ends the run with its message"
)]
//! Our side of the check of a screen against the design canvas: each screen the table below names, drawn by the
//! software renderer with its words, controls and their boxes, and with `--flows`, where the canvas's clicks lead.

use design::{Accent, Scheme, SemanticRole, Semantics};
use iced_core::widget::operation::{Operation, TextInput, text::Text};
use iced_core::{Element, Rectangle, Size, widget::Id};
use iced_test::Simulator;
use proto::chatkord_v1::{CloseReason, ErrorCode, InviteSummary, MemberEntry};
use screens::SoundSaid;
use screens::colour::Message as Picking;
use screens::frame::{PanePick, Room};
use screens::{
    Admitted, Channel, ChannelAction, DialogMessage, End, Found, Heard, Invite, InviteMessage, Jitter, MemberAction,
    MenuFrom, Message, Moment, NameFieldMessage, Opening, OverviewMessage, Page, Person, Pick, PlaceId, PlaceMessage,
    Refusal, Report, RoomEvent, Server, SetUpMessage, Standing, Track, Window, WindowMessage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use terms::{Secret, ServerUrl};
use ui::testing::{self, Renderer};
use ui::{Theme, app, picture};
use words::Sentence;

/// The title bar the canvas draws for the OS; both sides are compared below it.
const TITLE_BAR: f32 = 32.0;

/// The canvas's window, title bar and all: `--size WxH`, 1280x800 if not given.
fn whole_size() -> Size {
    static SIZE: std::sync::OnceLock<Size> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| {
        let args: Vec<String> = std::env::args().collect();
        let given = args.iter().position(|arg| arg == "--size").and_then(|at| args.get(at + 1));
        let (width, height) = given.and_then(|size| size.split_once('x')).unwrap_or(("1280", "800"));
        Size::new(width.parse().expect("a width"), height.parse().expect("a height"))
    })
}

/// The main window below its title bar, as the canvas is cut.
fn window_size() -> Size {
    whole_size() - Size::new(0.0, TITLE_BAR)
}

/// Which window a state is drawn in: the main one, below the title bar the canvas draws for it, or a watched screen's
/// own, which has none.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drawn {
    Main,
    Out,
}

impl Drawn {
    /// What the window shows of the state.
    fn size(self) -> Size {
        match self {
            Drawn::Main => window_size(),
            Drawn::Out => whole_size(),
        }
    }

    /// The window's view of `window`; a screen's own window that closed leaves the main one to show.
    fn view(self, window: &Window) -> Element<'_, WindowMessage, Theme, Renderer> {
        match self {
            Drawn::Out if window.pop_out().is_some() => app::out_view::<Renderer>(window, self.size()).unwrap(),
            Drawn::Main | Drawn::Out => {
                app::view_with::<Renderer>(window, self.size(), &held, &|painted| plain(&painted))
            }
        }
    }
}

/// Every state, with the window it is drawn in.
fn every() -> impl Iterator<Item = (&'static Screen, Drawn)> {
    SCREENS.iter().map(|screen| (screen, Drawn::Main)).chain(OUT_SCREENS.iter().map(|screen| (screen, Drawn::Out)))
}

/// The states drawn in a watched screen's own window, at the whole `--size`: it has no title bar.
const OUT_SCREENS: &[Screen] = &[
    Screen { name: "voice.popout", make: || sharer(watching_out) },
    Screen { name: "voice.camera.popout", make: || with_camera(camera_out) },
];

/// A screen the canvas names, reached the way a person reaches it: by messages.
struct Screen {
    name: &'static str,
    make: fn() -> Window,
}

const SCREENS: &[Screen] = &[
    Screen { name: "welcome", make: welcome },
    Screen { name: "welcome.language", make: welcome_language },
    Screen { name: "welcome.typo", make: typo },
    Screen { name: "welcome.offline", make: offline },
    Screen { name: "welcome.invite", make: || previewed(link(BOOKS, INVITE), book_club(), works(false)) },
    Screen { name: "welcome.owner", make: || previewed(link(CHAT, OWNER), night_owls(), works(true)) },
    Screen { name: "welcome.expired", make: || previewed(link(BOOKS, EXPIRED), book_club(), Some(Invite::Refused)) },
    Screen { name: "welcome.address", make: || previewed(GARDEN.into(), plant_swap(), None) },
    Screen { name: "welcome.trust", make: trust },
    Screen { name: "welcome.create", make: || creating(ADA, PASSWORD, PASSWORD) },
    Screen { name: "welcome.create.problems", make: || creating("Ada Lovelace", "evening games", "evening game") },
    Screen { name: "welcome.create.taken", make: taken },
    Screen { name: "welcome.working", make: working },
    Screen { name: "welcome.signin.wrong", make: wrong },
    Screen { name: "app.nochat", make: no_chat },
    Screen { name: "app.drawer", make: || channels_shown(both(books_signed_in)) },
    Screen { name: "app.pane.wide", make: || pane_at_most(both(books_signed_in)) },
    Screen { name: "app.pane.handle", make: || both(books_signed_in) },
    Screen { name: "voice.drawer", make: || channels_shown(both(in_lounge)) },
    Screen { name: "app.joined", make: joined },
    Screen { name: "app.signedout", make: || books_ended(None) },
    Screen { name: "app.signedout.reset", make: || books_ended(Some(CloseReason::PasswordReset)) },
    Screen { name: "app.signedout.deleted", make: || books_ended(Some(CloseReason::AccountDeleted)) },
    Screen { name: "app.keychanged", make: key_changed },
    Screen { name: "app.forgot", make: forgot },
    Screen { name: "app.newpw", make: new_password },
    Screen { name: "app.down", make: down },
    Screen { name: "app.connecting", make: connecting },
    Screen { name: "app.lost", make: lost },
    Screen { name: "app.back", make: back },
    Screen { name: "chat", make: || in_general(|_, _| {}) },
    Screen { name: "chat.format", make: chat_format },
    Screen { name: "chat.empty", make: chat_empty },
    Screen { name: "chat.history", make: || in_general(scrolled_back) },
    Screen { name: "chat.menu", make: || in_general(|window, owls| menu_on(window, owls, 12)) },
    Screen { name: "chat.menu.other", make: || in_general(|window, owls| menu_on(window, owls, 10)) },
    Screen { name: "chat.edit", make: || in_general(|window, owls| editing(window, owls, 13)) },
    Screen { name: "chat.menu.admin", make: || in_general(|window, owls| admin_menu_on(window, owls, 10)) },
    Screen { name: "chat.menu.pin", make: || in_general(|window, owls| admin_menu_on(window, owls, 14)) },
    Screen { name: "chat.hover", make: || in_general(|_, _| {}) },
    Screen { name: "chat.reply", make: || in_general(replying) },
    Screen { name: "chat.reply.deleted", make: chat_reply_deleted },
    Screen {
        name: "chat.reactions",
        make: || in_general(|window, owls| reacting(window, owls, screens::PickerSection::Recent)),
    },
    Screen {
        name: "chat.reactions.custom",
        make: || in_general(|window, owls| reacting(window, owls, screens::PickerSection::Server)),
    },
    Screen { name: "chat.pins", make: || in_general(pins_open) },
    Screen { name: "app.search.open", make: || in_general(search_open) },
    Screen { name: "app.search", make: || in_general(search_found) },
    Screen { name: "app.search.files", make: || in_general(search_files) },
    Screen { name: "app.search.none", make: || in_general(search_empty) },
    Screen { name: "app.search.failed", make: || in_general(search_failed) },
    Screen { name: "app.search.waiting", make: || in_general(search_waiting) },
    Screen { name: "app.search.voice", make: || sharer(search_in_call) },
    Screen { name: "server.overview.features", make: || server_hosting(false) },
    Screen { name: "server.hosting", make: || server_hosting(false) },
    Screen { name: "server.hosting.refused", make: || server_hosting(true) },
    Screen { name: "server.webhooks", make: || server_webhooks(true) },
    Screen { name: "webhook.create", make: || webhooks_in(WebhookStage::Making) },
    Screen { name: "server.webhooks.made", make: || webhooks_in(WebhookStage::Made) },
    Screen { name: "confirm.webhook.regenerate", make: || webhooks_in(WebhookStage::Regenerating) },
    Screen { name: "confirm.webhook.delete", make: || webhooks_in(WebhookStage::Deleting) },
    Screen { name: "chat.app", make: || chat_app(false) },
    Screen { name: "chat.app.about", make: || chat_app(true) },
    Screen { name: "app.joined.quiet", make: joined_quiet },
    Screen { name: "voice.mini.own", make: || sharer(floating_own) },
    Screen { name: "server.webhooks.empty", make: || server_webhooks(false) },
    Screen { name: "voice.camera.out", make: || with_camera(camera_out) },
    Screen { name: "chat.preview", make: chat_preview },
    Screen { name: "chat.compose.preview", make: || chat_composing_preview(true) },
    Screen { name: "chat.compose.preview.loading", make: || chat_composing_preview(false) },
    Screen { name: "chat.format.spoiler", make: chat_format_spoiler },
    Screen { name: "chat.emoji", make: || in_general(|window, owls| drafting(window, owls, "what a night :par")) },
    Screen { name: "chat.channels", make: || in_general(|window, owls| drafting(window, owls, "the rules are in #")) },
    Screen { name: "chat.mentions", make: || in_general(mentioning_as_admin) },
    Screen { name: "chat.mentions.member", make: || in_general(mentioning) },
    Screen { name: "chat.files", make: || chat_files(uploading) },
    Screen { name: "chat.files.viewer", make: || chat_files(viewing_dice) },
    Screen { name: "chat.files.refused", make: || chat_files(uploads_refused) },
    Screen { name: "chat.files.drop", make: || chat_files(|window, _| files_over(window)) },
    Screen { name: "chat.typing", make: || in_general(|window, owls| typing(window, owls, &["grace"])) },
    Screen { name: "chat.typing.two", make: || in_general(|window, owls| typing(window, owls, &["grace", "bo"])) },
    Screen {
        name: "chat.typing.several",
        make: || in_general(|window, owls| typing(window, owls, &["grace", "bo", "mira"])),
    },
    Screen { name: "chat.unread", make: || in_general(unread_elsewhere) },
    Screen { name: "chat.narrow", make: || in_general(|_, _| {}) },
    Screen { name: "chat.deleted", make: chat_deleted },
    Screen {
        name: "chat.composer",
        make: || {
            in_general(|window, owls| {
                drafting(window, owls, "Saturday plan:\n20:00 in the Lounge\nbring the spare controller")
            })
        },
    },
    Screen {
        name: "chat.composer.limit",
        make: || in_general(|window, owls| drafting(window, owls, &"over the limit ".repeat(268))),
    },
    Screen { name: "chat.cannotsend", make: chat_cannot_send },
    Screen { name: "voice.grid", make: || both(in_lounge_speaking) },
    Screen { name: "voice.alone", make: alone },
    Screen { name: "voice.muted", make: || voice(|window, owls| at(window, owls, mute_state(true, false))) },
    Screen { name: "voice.deafened", make: || voice(|window, owls| at(window, owls, mute_state(true, true))) },
    Screen { name: "voice.moved", make: || voice(|window, owls| ended(CloseReason::Moved)(window, owls)) },
    Screen { name: "voice.reconnecting", make: || voice(mending) },
    Screen { name: "voice.reconnecting.others", make: || voice(others_mending) },
    Screen { name: "voice.connecting", make: || voice(own_connecting) },
    Screen { name: "voice.connecting.other", make: || voice(other_connecting) },
    Screen { name: "voice.failed", make: || voice(other_failed) },
    Screen { name: "voice.deleted", make: || voice(|window, owls| ended(CloseReason::ChannelDeleted)(window, owls)) },
    Screen { name: "voice.otherdevice", make: other_device },
    Screen { name: "voice.devices", make: || devices(Some(HEADSET_MIC), true, None) },
    Screen { name: "voice.stats", make: || connection(good(), false) },
    Screen { name: "voice.stats.fair", make: || connection(fair(), false) },
    Screen { name: "voice.stats.alone", make: || connection(good(), true) },
    Screen {
        name: "voice.share.menu",
        make: || sharer(|window, owls| at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleShare))),
    },
    Screen { name: "voice.share.custom", make: || sharer(|window, owls| share_custom(window, owls, "2.5")) },
    Screen { name: "voice.share.custom.refused", make: || sharer(|window, owls| share_custom(window, owls, "45")) },
    Screen { name: "voice.share.menu.nosound", make: || sharer(share_menu_silent) },
    Screen { name: "voice.share.menu.processor", make: || sharer(share_menu_processor) },
    Screen { name: "voice.share.menu.untested", make: || sharer(share_menu_untested) },
    Screen {
        name: "voice.share.pick",
        make: || sharer(|window, owls| picking(window, owls, screens::SourcesTab::Screens, None)),
    },
    Screen {
        name: "voice.share.pick.apps",
        make: || sharer(|window, owls| picking(window, owls, screens::SourcesTab::Applications, None)),
    },
    Screen {
        name: "voice.share.pick.chosen",
        make: || sharer(|window, owls| picking(window, owls, screens::SourcesTab::Applications, Some("w2"))),
    },
    Screen { name: "voice.share.unavailable", make: no_encoder },
    Screen { name: "voice.sharing", make: || sharer(sharing) },
    Screen { name: "voice.sharing.own", make: || sharer(sharing) },
    Screen { name: "voice.sharing.starting", make: || sharer(share_starting) },
    Screen { name: "voice.sharing.noshow", make: || sharer(sharing_unshown) },
    Screen { name: "voice.share.failed", make: || sharer(share_failed) },
    Screen { name: "voice.share.denied", make: || sharer(share_denied) },
    Screen { name: "voice.live", make: || sharer(watching_in_card) },
    Screen { name: "voice.live.two", make: || sharer(two_share) },
    Screen { name: "voice.live.unshown", make: || sharer(unshown) },
    Screen { name: "app.share.sound.not-carried", make: || sharer(sound_not_carried) },
    Screen { name: "voice.watch.card", make: || sharer(watching_in_card) },
    Screen { name: "voice.sharing.watch", make: || sharer(sharing_and_watching) },
    Screen { name: "voice.watch", make: || sharer(watching) },
    Screen { name: "voice.watch.strip", make: || sharer(watching_without_strip) },
    Screen { name: "voice.mini", make: || sharer(floating) },
    Screen { name: "voice.watch.out", make: || sharer(watching_out) },
    Screen { name: "voice.watch.zoom", make: || sharer(watching_zoomed) },
    Screen { name: "voice.fullscreen", make: || sharer(full_screen) },
    Screen { name: "voice.fullscreen.calm", make: || sharer(full_screen_calm) },
    Screen { name: "voice.card.hover", make: || sharer(|_, _| {}) },
    Screen { name: "voice.camera.off", make: || with_camera(|_, _| {}) },
    Screen { name: "voice.camera", make: || with_camera(cameras_on) },
    Screen { name: "voice.camera.unshown", make: || with_camera(camera_unshown) },
    Screen { name: "voice.camera.notallowed", make: || sharer(|_, _| {}) },
    Screen { name: "voice.camera.stopped", make: || with_camera(camera_stopped) },
    Screen { name: "voice.camera.denied", make: || with_camera(camera_denied) },
    Screen {
        name: "voice.pin.person",
        make: || sharer(|window, owls| pin(window, owls, screens::Pinned::Person(screens::harness::member("grace")))),
    },
    Screen { name: "voice.pin.own", make: || sharer(own_pinned) },
    Screen { name: "voice.watch.paused", make: || sharer(watching_paused) },
    Screen { name: "voice.camera.info", make: || with_camera(|window, owls| camera_about(window, owls, false)) },
    Screen { name: "voice.camera.info.held", make: || with_camera(|window, owls| camera_about(window, owls, true)) },
    Screen { name: "voice.camera.hover", make: || with_camera(camera_hover) },
    Screen { name: "voice.camera.paused", make: || with_camera(camera_paused) },
    Screen { name: "voice.watch.info", make: || sharer(watching_about) },
    Screen { name: "voice.watch.info.software", make: || sharer(watching_software_about) },
    Screen { name: "voice.watch.info.dropped", make: || sharer(watching_unpainted) },
    Screen { name: "voice.watch.behind", make: || sharer(watching_behind) },
    Screen { name: "voice.watch.info.behind", make: || sharer(watching_behind_about) },
    Screen { name: "voice.watch.info.froze", make: || sharer(watching_froze) },
    Screen { name: "voice.share.info", make: || sharer(|window, owls| own_about(window, owls, false)) },
    Screen {
        name: "voice.share.info.screen",
        make: || sharer(|window, owls| own_about_sound(window, owls, SoundSaid::Everything)),
    },
    Screen {
        name: "voice.share.info.nosound",
        make: || sharer(|window, owls| own_about_sound(window, owls, SoundSaid::NotFound)),
    },
    Screen { name: "app.share.sound.pick", make: || sharer(|window, owls| own_sound_picking(window, owls, None)) },
    Screen {
        name: "app.share.sound.picked",
        make: || sharer(|window, owls| own_sound_picking(window, owls, Some("Spotify"))),
    },
    Screen {
        name: "voice.share.info.own",
        make: || sharer(|window, owls| own_about_sound(window, owls, SoundSaid::Own)),
    },
    Screen { name: "voice.share.info.held", make: || sharer(|window, owls| own_about(window, owls, true)) },
    Screen { name: "voice.share.info.scaled", make: || sharer(own_about_scaled) },
    Screen { name: "voice.share.info.ceiling", make: || sharer(own_about_ceiling) },
    Screen { name: "voice.share.info.busy", make: || sharer(own_about_busy) },
    Screen { name: "voice.share.info.slow", make: || sharer(own_about_slow) },
    Screen { name: "voice.share.info.untested", make: || sharer(own_about_untested) },
    Screen { name: "voice.share.info.software", make: || sharer(own_about_software) },
    Screen { name: "voice.share.info.unwatched", make: || sharer(own_about_unwatched) },
    Screen { name: "voice.stats.share.unwatched", make: || sharer(unwatched_stats) },
    Screen { name: "voice.watch.reconnecting", make: || sharer(watch_reconnecting) },
    Screen {
        name: "voice.stats.share",
        make: || {
            sharer(|window, owls| {
                shared_stats(window, owls, with_share(good(), 8_100_000, 903_512, 1_200_000_000), false)
            })
        },
    },
    Screen {
        name: "voice.stats.poor",
        make: || {
            sharer(|window, owls| shared_stats(window, owls, with_share(poor(), 2_000_000, 247_016, 318_700_000), true))
        },
    },
    Screen { name: "voice.stats.unmeasured", make: || connection(screens::Health::default(), true) },
    Screen {
        name: "voice.stats.camera",
        make: || {
            with_camera(|window, owls| {
                camera_stats(window, owls, screens::Route::Hardware { via: "VA-API".into() }, false);
            })
        },
    },
    Screen {
        name: "voice.stats.camera.held",
        make: || {
            with_camera(|window, owls| {
                camera_stats(window, owls, screens::Route::Hardware { via: "VA-API".into() }, true);
            })
        },
    },
    Screen {
        name: "voice.stats.camera.software",
        make: || with_camera(|window, owls| camera_stats(window, owls, screens::Route::Software, false)),
    },
    Screen {
        name: "voice.stats.both",
        make: || {
            with_camera(|window, owls| {
                own_camera_looked(window, owls, screens::Route::Hardware { via: "VA-API".into() }, false);
                shared_stats(window, owls, with_share(good(), 8_100_000, 903_512, 1_200_000_000), false);
                at(window, owls, PlaceMessage::Call(screens::CallMessage::ShowOwn(proto::video::Video::Camera)));
            })
        },
    },
    Screen {
        name: "voice.mic.lost",
        make: || {
            let gone = Sentence::SoundGone { was: HEADSET_MIC.into(), now: Some(BUILT_IN.into()) };
            devices(None, false, Some(gone))
        },
    },
    Screen {
        name: "voice.mic.back",
        make: || devices(Some(HEADSET_MIC), false, Some(Sentence::SoundBack(HEADSET_MIC.into()))),
    },
    Screen { name: "app.madeadmin", make: raised },
    Screen { name: "voice.volume", make: || voice(volume) },
    Screen { name: "voice.volume.muted", make: || voice(volume_muted) },
    Screen { name: "voice.watch.volume", make: || sharer(watching_volume) },
    Screen { name: "voice.watch.nosound", make: || sharer(watching_no_sound) },
    Screen { name: "voice.audio", make: || sharer(audio_controls) },
    Screen { name: "voice.view", make: || sharer(view_menu) },
    Screen { name: "voice.view.hidden", make: || sharer(view_hidden) },
    Screen { name: "voice.view.controls", make: || sharer(view_controls) },
    Screen { name: "voice.audio.alone", make: audio_controls_alone },
    Screen { name: "voice.mod.menu", make: || voice(mod_menu) },
    Screen { name: "voice.mod.muted", make: || voice(mod_muted) },
    Screen { name: "voice.mod.muted.own", make: || voice(mod_muted_own) },
    Screen { name: "voice.mod.removed", make: || voice(mod_removed) },
    Screen { name: "settings.voice.taken", make: || voice(taken_under_a_page) },
    Screen { name: "app.call.ended.elsewhere", make: || voice(taken_elsewhere) },
    Screen { name: "app.call.elsewhere.mark", make: || voice(books_opened) },
    Screen { name: "menu.member", make: || menu(None) },
    Screen {
        name: "menu.owner",
        make: || {
            menu(Some(&Standing {
                place: proto::rules::Place::OWNER,
                grants: every_word(),
                ..screens::harness::standing_as("ada")
            }))
        },
    },
    Screen {
        name: "menu.admin",
        make: || {
            menu(Some(&Standing {
                place: screens::harness::ADMIN_PLACE,
                grants: every_word(),
                ..screens::harness::standing_as("ada")
            }))
        },
    },
    Screen { name: "add", make: || adding(None) },
    Screen { name: "add.already", make: || adding(Some(CHAT)) },
    Screen { name: "add.signin", make: add_sign_in },
    Screen { name: "confirm.signout", make: || confirm(Pick::SignOut, false) },
    Screen { name: "confirm.remove", make: || confirm(Pick::Remove, false) },
    Screen { name: "confirm.leave", make: || confirm(Pick::Leave, false) },
    Screen { name: "confirm.leave.owner", make: || confirm(Pick::Leave, true) },
    Screen { name: "confirm.quit", make: quit },
    Screen { name: "app.owner.note", make: owner_note },
    Screen { name: "app.owner.setup", make: || owner_first(false) },
    Screen { name: "app.owner.setup.colour", make: || owner_first(true) },
    Screen { name: "server.overview", make: || server_overview(false, None) },
    Screen { name: "server.overview.picker", make: || server_overview(true, None) },
    Screen {
        name: "server.overview.refused",
        make: || {
            server_overview(false, Some("Night Owls, the late-night gaming and film club for everyone who stays up"))
        },
    },
    Screen { name: "server.about", make: server_about },
    Screen { name: "server.about.report.busy", make: || server_report(ServerReportStage::Busy) },
    Screen { name: "server.about.report", make: || server_report(ServerReportStage::Shown) },
    Screen { name: "server.about.report.saved", make: || server_report(ServerReportStage::Saved) },
    Screen { name: "server.about.report.refused", make: || server_report(ServerReportStage::Refused) },
    Screen { name: "server.emoji", make: || server_emoji(server_emoji_list(), |_, _| {}) },
    Screen {
        name: "server.emoji.rename",
        make: || server_emoji(server_emoji_list(), |window, owls| renaming_owlwave(window, owls, "owl_wave")),
    },
    Screen {
        name: "server.emoji.refused",
        make: || {
            server_emoji(server_emoji_list(), |window, owls| {
                renaming_owlwave(window, owls, "owl wave!");
                on_emoji_page(window, owls, screens::EmojiPageMessage::Save);
            })
        },
    },
    Screen { name: "server.emoji.full", make: || server_emoji(server_emoji_full(), |_, _| {}) },
    Screen {
        name: "confirm.emoji.delete",
        make: || {
            server_emoji(server_emoji_list(), |window, owls| {
                on_emoji_page(window, owls, screens::EmojiPageMessage::Delete(emoji_id("owlwave")));
            })
        },
    },
    Screen {
        name: "confirm.message.delete",
        make: || {
            in_general(|window, owls| {
                made_admin(window, owls);
                at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Delete(message_id(10))));
            })
        },
    },
    Screen { name: "invite.options", make: invite_options },
    Screen { name: "invite.role", make: || inviting_as(true, |window| invite_said(window, InviteMessage::RoleMenu)) },
    Screen { name: "invite.norole", make: || inviting_as(false, |_| {}) },
    Screen {
        name: "invite.custom",
        make: || inviting_as(true, |window| invite_said(window, InviteMessage::Uses(screens::UsesMessage::Custom))),
    },
    Screen { name: "invite.custom.refused", make: || inviting_as(true, |window| invite_custom(window, "250")) },
    Screen {
        name: "invite.created",
        make: || invite_created(|window| invite_said(window, InviteMessage::Note("for Grace".into()))),
    },
    Screen {
        name: "invite.created.custom",
        make: || {
            invite_created(|window| {
                invite_custom(window, "12");
                invite_said(window, InviteMessage::Role(Some(screens::harness::role_id(REGULARS_ROLE))));
            })
        },
    },
    Screen { name: "server.invites", make: server_invites },
    Screen { name: "confirm.stop", make: confirm_delete },
    Screen { name: "server.members", make: || server_members(true) },
    Screen { name: "server.list", make: || settings_list(server_members(true)) },
    Screen { name: "server.members.admin", make: || server_members(false) },
    Screen { name: "server.members.refused", make: members_refused },
    Screen { name: "server.members.roles", make: || member_menu(screens::MemberMenu::Roles) },
    Screen { name: "server.members.more", make: || member_menu(screens::MemberMenu::More) },
    Screen { name: "server.members.search", make: members_searched },
    Screen { name: "server.members.banned", make: members_banned },
    Screen { name: "confirm.kick", make: || banning("bo", MemberAction::Kick, None) },
    Screen { name: "confirm.ban", make: || banning("bo", MemberAction::Ban, None) },
    Screen {
        name: "confirm.ban.wrong",
        make: || banning("bo", MemberAction::Ban, Some(Err(terms::LinkError::Refused(ErrorCode::Unauthenticated)))),
    },
    Screen { name: "confirm.unban", make: || banning("cy", MemberAction::Unban, None) },
    Screen { name: "server.roles", make: || server_roles(true) },
    Screen { name: "server.roles.open", make: role_opened },
    Screen { name: "server.roles.admin", make: || server_roles(false) },
    Screen { name: "server.roles.default", make: || role_chosen(MEMBER_ROLE) },
    Screen { name: "server.roles.owner", make: || role_chosen(OWNER_ROLE) },
    Screen { name: "server.roles.colour", make: role_colour },
    Screen { name: "server.roles.refused", make: roles_refused },
    Screen { name: "role.create", make: role_create },
    Screen { name: "confirm.role.delete", make: role_delete },
    Screen { name: "confirm.reset", make: || member_asked(MemberAction::ResetPassword) },
    Screen { name: "reset.done", make: reset_done },
    Screen { name: "confirm.transfer", make: || member_asked(MemberAction::MakeOwner) },
    Screen { name: "server.channels", make: server_channels },
    Screen { name: "server.channels.empty", make: server_channels_empty },
    Screen { name: "app.channels.empty", make: || owning_no_categories(|_, _| {}) },
    Screen {
        name: "app.channels.empty.member",
        make: || without_categories(proto::rules::Place::BOTTOM, member_words(), |_, _| {}),
    },
    Screen { name: "server.channels.open", make: channels_opened },
    Screen { name: "server.channels.voice", make: || channel_chosen("lounge", Vec::new()) },
    Screen { name: "server.channels.private", make: || channel_chosen("staff", staff_marks()) },
    Screen { name: "server.channels.category", make: category_chosen },
    Screen {
        name: "server.channels.add",
        make: || channel_said(screens::channel_settings::Message::Adding(staff_adding())),
    },
    Screen { name: "server.channels.advanced", make: || channel_said(screens::channel_settings::Message::Advanced) },
    Screen { name: "server.channels.admin", make: channels_admin },
    Screen { name: "server.channels.refused", make: channels_refused },
    Screen { name: "channel.create", make: || channel_create(|window, _| named(window, "film-club")) },
    Screen { name: "channel.create.voice", make: || channel_create(channel_create_voice) },
    Screen { name: "channel.create.private", make: || channel_create(channel_create_private) },
    Screen { name: "channel.create.readonly", make: || channel_create(channel_create_readonly) },
    Screen { name: "channel.create.category", make: || owning(channel_create_in) },
    Screen { name: "confirm.channel.delete.text", make: || owning(ideas_deleted) },
    Screen { name: "channel.create.typed", make: || channel_asked(None) },
    Screen { name: "channel.rename", make: || channel_asked(Some(ChannelAction::Rename)) },
    Screen { name: "confirm.channel.delete", make: || channel_asked(Some(ChannelAction::Delete)) },
    Screen { name: "settings.list", make: || settings_list(profile()) },
    Screen { name: "settings.account.picker", make: profile_picking },
    Screen { name: "settings.account", make: || account(&Form::Closed) },
    Screen { name: "settings.voice", make: voice_settings },
    Screen {
        name: "settings.voice.processing",
        make: || {
            let mut window = voice_settings();
            let asked = choices::VoiceProcessing {
                automatic_gain: choices::AutomaticGain::Off,
                gate: choices::NoiseGate::On,
                ..choices::VoiceProcessing::default()
            };
            window.update(WindowMessage::SetProcessing(asked), moment());
            window
        },
    },
    Screen {
        name: "settings.voice.test",
        make: || {
            let mut window = voice_settings();
            window.update(WindowMessage::TestMicrophone, moment());
            window
        },
    },
    Screen { name: "settings.voice.camera", make: || camera_settings(USB_CAMERA, None) },
    Screen { name: "settings.voice.camera.starting", make: || camera_settings(USB_CAMERA, Some(None)) },
    Screen { name: "settings.voice.camera.test", make: || camera_settings(USB_CAMERA, Some(Some(Ok(())))) },
    Screen {
        name: "settings.voice.camera.denied",
        make: || camera_settings(USB_CAMERA, Some(Some(Err(words::Failure::CameraDenied)))),
    },
    Screen {
        name: "settings.voice.camera.busy",
        make: || camera_settings(USB_CAMERA, Some(Some(Err(words::Failure::CameraBusy)))),
    },
    Screen {
        name: "settings.voice.camera.gone",
        make: || camera_settings(USB_CAMERA, Some(Some(Err(words::Failure::CameraGone)))),
    },
    Screen {
        name: "settings.voice.camera.away",
        make: || camera_settings(DESK_CAMERA, Some(Some(Err(words::Failure::CameraGone)))),
    },
    Screen { name: "settings.voice.camera.live", make: camera_live_settings },
    Screen { name: "settings.appearance", make: appearance },
    Screen { name: "settings.appearance.language", make: appearance_language },
    Screen { name: "settings.appearance.messages", make: appearance_messages },
    Screen { name: "settings.notifications", make: notifications },
    Screen { name: "settings.startup", make: startup },
    Screen { name: "settings.startup.held", make: startup_held },
    Screen { name: "settings.about", make: || about(None) },
    Screen { name: "settings.about.open", make: || about(Some(0)) },
    Screen { name: "settings.about.update.ready", make: about_update_ready },
    Screen { name: "settings.about.absent", make: about_absent },
    Screen { name: "settings.signedout", make: settings_signed_out },
    Screen { name: "welcome.report", make: || reported(welcome().built_as(screens::harness::BUILD), false) },
    Screen { name: "settings.about.report", make: || reported(about(None), false) },
    Screen { name: "settings.about.report.saved", make: || reported(about(None), true) },
    Screen { name: "app.update", make: || updating(Reached::Ready, false) },
    Screen { name: "app.update.downloading", make: || updating(Reached::Downloading, false) },
    Screen { name: "app.update.call", make: || updating(Reached::Ready, true) },
    Screen { name: "door.devices", make: door_devices },
    Screen { name: "door.report", make: door_report },
    Screen { name: "reconnecting.devices", make: reconnecting_devices },
    Screen { name: "settings.appearance.zoomed", make: || zoomed(3) },
    Screen { name: "settings.appearance.zoom.max", make: || zoomed(5) },
    Screen { name: "settings.password", make: || account(&Form::Empty) },
    Screen { name: "settings.password.ready", make: || account(&Form::Ready) },
    Screen { name: "settings.password.wrong", make: || account(&Form::Wrong) },
    Screen { name: "server.overview.picture", make: || overview_picture(Some(&PictureStage::Held)) },
    Screen { name: "server.overview.picture.busy", make: || overview_picture(Some(&PictureStage::Uploading)) },
    Screen { name: "server.overview.picture.refused", make: || overview_picture(None) },
    Screen { name: "app.owner.setup.picture", make: owner_first_picture },
    Screen { name: "app.picture", make: || pictured(voice(|_, _| {})) },
    Screen { name: "app", make: || both(in_lounge_reading) },
    Screen { name: "app.channels", make: || owning(|_, _| {}) },
    Screen { name: "app.channels.folded", make: || owning(folded) },
    Screen { name: "app.channels.hover", make: || owning(|_, _| {}) },
    Screen { name: "app.move.refused", make: || owning(move_refused) },
    Screen { name: "menu.channel", make: || owning(|window, owls| list_menu(window, owls, channel_key("ideas"))) },
    Screen { name: "menu.category", make: || owning(|window, owls| list_menu(window, owls, category_key("hangouts"))) },
    Screen { name: "menu.pane", make: || owning(pane_menu) },
    Screen {
        name: "menu.channel.notify",
        make: || owning(|window, owls| ideas_sub(window, owls, screens::ListSub::Notify)),
    },
    Screen {
        name: "menu.channel.mute",
        make: || owning(|window, owls| ideas_sub(window, owls, screens::ListSub::Mute)),
    },
    Screen { name: "notify.server", make: || notify_server(false) },
    Screen { name: "notify.server.mute", make: || notify_server(true) },
    Screen { name: "category.create", make: || owning(category_create) },
    Screen {
        name: "category.rename",
        make: || {
            owning(|window, owls| list_pick(window, owls, category_key("projects"), screens::ListPick::RenameCategory))
        },
    },
    Screen {
        name: "confirm.category.delete",
        make: || {
            owning(|window, owls| list_pick(window, owls, category_key("hangouts"), screens::ListPick::DeleteCategory))
        },
    },
    Screen { name: "confirm.category.delete.empty", make: || owning(archive_deleted) },
    Screen { name: "chat.private.empty", make: chat_private_empty },
    Screen { name: "chat.members", make: || in_general(|_, _| {}) },
    Screen { name: "chat.members.closed", make: || in_general(|window, _| toggle_members(window)) },
    Screen {
        name: "chat.members.person",
        make: || in_general(|window, owls| person(window, owls, "grace", screens::PersonFrom::Members)),
    },
    Screen { name: "chat.members.person.offline", make: || in_general(cy_opened) },
    Screen { name: "chat.members.person.member", make: || in_general(member_opened) },
    Screen {
        name: "chat.account.person",
        make: || in_general(|window, owls| own_status(window, owls, proto::status::Choice::Online, None)),
    },
    Screen {
        name: "chat.status.pick",
        make: || {
            in_general(|window, owls| {
                own_status(window, owls, proto::status::Choice::Online, None);
                let pick = screens::StatusMessage::Pick(proto::status::Choice::DoNotDisturb);
                window.update(WindowMessage::Status(pick), moment());
            })
        },
    },
    Screen {
        name: "chat.status.dnd",
        make: || {
            in_general(|window, owls| {
                status(window, owls, proto::status::Choice::DoNotDisturb, Some(tonight(32, 0)));
                shown(window, owls, &[("grace", proto::status::Shown::Away)]);
            })
        },
    },
    Screen {
        name: "chat.status.invisible",
        make: || in_general(|window, owls| own_status(window, owls, proto::status::Choice::Invisible, None)),
    },
    Screen { name: "chat.members.status", make: || in_general(members_status) },
    Screen {
        name: "chat.author.person",
        make: || in_general(|window, owls| person(window, owls, "grace", screens::PersonFrom::Author)),
    },
    Screen { name: "dm.list", make: || in_general(dm_list) },
    Screen { name: "dm.list.empty", make: || in_general(dm_list_empty) },
    Screen { name: "dm.open", make: || in_general(|window, owls| dm_open(window, owls, "grace")) },
    Screen { name: "dm.empty", make: || in_general(|window, owls| dm_open(window, owls, "mira")) },
    Screen { name: "dm.gone", make: || in_general(dm_gone) },
    Screen { name: "dm.channels", make: || in_general(dm_channels) },
    Screen { name: "dm.marks", make: || in_general(dm_marks) },
    Screen {
        name: "dm.person",
        make: || {
            in_general(|window, owls| {
                let presence = screens::PeopleMessage::Presence { generation: 5, shown: grace_online_bo_not() };
                at(window, owls, PlaceMessage::People(presence));
                person(window, owls, "grace", screens::PersonFrom::Members);
            })
        },
    },
    Screen { name: "thread.chip", make: || in_general(thread_chip) },
    Screen { name: "thread.open", make: || in_general(thread_open) },
    Screen { name: "thread.new", make: || in_general(thread_new) },
    Screen { name: "thread.narrow", make: || in_general(thread_open) },
    Screen { name: "thread.list", make: || in_general(thread_list) },
    Screen { name: "thread.list.empty", make: || in_general(thread_list_empty) },
    Screen { name: "settings.notifications.quiet", make: || notifications_quiet(true) },
    Screen { name: "app.badge", make: badges },
    Screen { name: "chat.members.handle", make: || in_general(|_, _| {}) },
    Screen { name: "chat.members.wide", make: || in_general(|window, _| members_widest(window)) },
    Screen { name: "voice.members", make: || voice(|_, _| {}) },
    Screen { name: "voice.members.narrow", make: || voice(members_shown) },
    Screen { name: "voice.chat", make: voice_chat },
    Screen { name: "voice.chat.narrow", make: voice_chat },
    Screen { name: "voice.mod.pane", make: mod_pane },
    Screen { name: "voice.mod.refused", make: || voice(mod_refused) },
    Screen { name: "app.picture.mark", make: || pictured(voice(books_opened)) },
];

/// Screens of the canvas not built yet, said instead of compared.
const NOT_BUILT: &[(&str, &str)] = &[
    ("voice.tooltip", "a tooltip shows after the pointer rests: the shell's host does not wait here"),
    ("app.voice.taken", "the take-out card"),
    ("app.camera", "Bo's camera outside a call: the list's camera mark is compared in voice.camera's side pane"),
    ("app.channels.drag", "a drag under way is the reorder list's own state: its reference image draws the drop line"),
    (
        "server.channels.drag",
        "a drag under way is the reorder list's own state: its reference image draws the drop line",
    ),
    ("server.roles.drag", "a drag under way is the reorder list's own state: its reference image draws the drop line"),
    ("chat.leave", "leaving with one's messages deleted: no item builds it yet"),
];

const CHAT: &str = "chat.example.invalid";
const BOOKS: &str = "books.example.invalid";
const GARDEN: &str = "garden.example.invalid";
const LAN: &str = "lan.example.invalid";
/// The canvas's fingerprints, as a pin is kept: the key a server presented first, and the one it presents later.
const FIRST_KEY: &str = "b41c9e0733af5d208c6ef1a904732b5ed9c26f18a07b4e9513f6c8d47a20e3b1";
const NEW_KEY: &str = "7e05d2a891c43b6f0a9d5e21c7f084b32f6c9b17e4d806a5c3e9718b5d0fa264";
const INVITE: &str = "Rt4wQ9";
const OWNER: &str = "Ow9kP2";
const EXPIRED: &str = "Ex2mL7";
const ADA: &str = "Ada";
const USB_CAMERA: &str = "USB camera";
const BUILT_IN_CAMERA: &str = "Built-in camera";
const DESK_CAMERA: &str = "Desk camera";
/// This account's own id, as the server names it in a room.
const ME: &str = "ada";
const PASSWORD: &str = "evening games with friends";
/// The canvas's invites end on 1 November.
const UNTIL: i64 = 1_793_491_200;
/// The day after the canvas's day, which it says as "Tomorrow".
const TOMORROW: i64 = UNTIL - 5 * 24 * 60 * 60;

/// A server as the canvas has it: named, coloured as it picked (none is Chatkord's), and serving its identity.
fn server(host: &str, name: &str, colour: Option<&str>) -> Server {
    let url = ServerUrl::parse(&format!("wss://{host}/v1")).unwrap();
    let colour = colour.map(|colour| proto::rules::Colour::parse(colour).unwrap());
    Server { url, name: Some(name.into()), colour, picture: None }
}

fn night_owls() -> Server {
    server(CHAT, "Night Owls", None)
}

fn book_club() -> Server {
    server(BOOKS, "Book club", Some("#E0457B"))
}

fn plant_swap() -> Server {
    server(GARDEN, "Plant swap", Some("#2FA36B"))
}

fn link(host: &str, start: &str) -> String {
    format!("https://{host}/join#{start}{}", "a".repeat(proto::rules::TOKEN_LEN - start.len()))
}

fn works(owner: bool) -> Option<Invite> {
    Some(Invite::Works { until: UNTIL, owner })
}

fn moment() -> Moment {
    Moment { now: Instant::now(), jitter: Jitter::from_bits(1 << 63), nonce: terms::Nonce::new([7; 16]), wall: TODAY }
}

fn at(window: &mut Window, id: PlaceId, message: PlaceMessage) {
    window.update(WindowMessage::At(id, message), moment());
}

/// A message to the welcome steps.
fn tell(window: &mut Window, message: Message) {
    let id = window.welcome_door().map(|adding| adding.id()).unwrap();
    at(window, id, PlaceMessage::Session(screens::SessionMessage::Door(message)));
}

fn heard(window: &mut Window, heard: Heard) {
    tell(window, Message::Reported(heard));
}

fn welcome() -> Window {
    Window::launch(Vec::new(), None, Instant::now()).0
}

/// The first run's door with the Language field's menu open.
fn welcome_language() -> Window {
    let mut window = welcome();
    window.update(WindowMessage::LanguageMenu, moment());
    window
}

fn offline() -> Window {
    let mut window = welcome();
    tell(&mut window, Message::Where("boks.example.invalid".into()));
    tell(&mut window, Message::Submit);
    heard(&mut window, Heard::Found(Err(terms::LinkError::NoNetwork)));
    window
}

fn typo() -> Window {
    let mut window = welcome();
    tell(&mut window, Message::Where("boks.example.invalid".into()));
    tell(&mut window, Message::Submit);
    heard(&mut window, Heard::Found(Err(terms::LinkError::NoSuchHost)));
    window
}

fn previewed(text: String, server: Server, invite: Option<Invite>) -> Window {
    let mut window = welcome();
    tell(&mut window, Message::Where(text));
    tell(&mut window, Message::Submit);
    heard(&mut window, Heard::Found(Ok(Found { server, invite })));
    window
}

fn key(kept: &str) -> proto::pin::KeyPin {
    proto::pin::KeyPin::parse(kept).unwrap()
}

/// An address whose server presents a certificate no authority signed.
fn trust() -> Window {
    let mut window = welcome();
    tell(&mut window, Message::Where(LAN.into()));
    tell(&mut window, Message::Submit);
    heard(&mut window, Heard::Found(Err(terms::LinkError::Unvouched(key(FIRST_KEY)))));
    window
}

fn creating(name: &str, password: &str, again: &str) -> Window {
    let mut window = previewed(link(BOOKS, INVITE), book_club(), works(false));
    tell(&mut window, Message::Submit);
    tell(&mut window, Message::Username(name.into()));
    tell(&mut window, Message::Password(Secret::new(password.into())));
    tell(&mut window, Message::Again(Secret::new(again.into())));
    window
}

fn taken() -> Window {
    let mut window = creating(ADA, PASSWORD, PASSWORD);
    tell(&mut window, Message::Submit);
    let taken = terms::LinkError::Refused(ErrorCode::AlreadyExists);
    heard(&mut window, Heard::Registered(Err(taken)));
    window
}

fn working() -> Window {
    let mut window = creating(ADA, PASSWORD, PASSWORD);
    tell(&mut window, Message::Submit);
    window
}

fn wrong() -> Window {
    let mut window = previewed(GARDEN.into(), plant_swap(), None);
    tell(&mut window, Message::Submit);
    tell(&mut window, Message::Username(ADA.into()));
    tell(&mut window, Message::Password(Secret::new("not it".into())));
    tell(&mut window, Message::Submit);
    let refused = Refusal { code: ErrorCode::Unauthenticated };
    heard(&mut window, Heard::Flow(Report::Refused(refused)));
    window
}

fn channel(id: &str, name: &str) -> Channel {
    Channel {
        id: screens::harness::channel_id(id),
        name: name.into(),
        people: Vec::new(),
        timeline: screens::harness::timeline_id(id),
    }
}

/// `listed` with a new server's two categories, Text Channels and Voice Channels, holding the channels of their kind.
fn in_first_categories(mut listed: screens::ChannelList) -> screens::ChannelList {
    let category = |id: &str, name: &str, position| screens::Category {
        id: terms::CategoryId::parse(id).expect("an id"),
        name: name.into(),
        position,
    };
    listed.categories = vec![category("text", "Text Channels", 0), category("voice", "Voice Channels", 1)];
    let text = listed.text.iter().map(|channel| (channel.id.clone(), "text"));
    let voice = listed.voice.iter().map(|channel| (channel.id.clone(), "voice"));
    for (position, (id, category)) in (0..).zip(text.chain(voice).collect::<Vec<_>>()) {
        let category = terms::CategoryId::parse(category).expect("an id");
        listed.placed.insert(id, screens::Placement { category, position });
    }
    listed
}

/// The canvas's voice channels of each server.
fn channels_of(server: &Server) -> screens::ChannelList {
    if server.url == book_club().url {
        in_first_categories(screens::ChannelList {
            voice: vec![channel("reading", "Reading room")],
            ..screens::ChannelList::default()
        })
    } else if server.url == plant_swap().url {
        let text = |id: &str| screens::TextChannel {
            id: screens::harness::channel_id(id),
            name: id.into(),
            timeline: screens::harness::general_timeline(),
            revision: 0,
        };
        in_first_categories(screens::ChannelList {
            text: vec![text("general"), text("swaps")],
            voice: vec![channel("shed", "Potting shed")],
            ..screens::ChannelList::default()
        })
    } else {
        let text = |id: &str| screens::TextChannel {
            id: screens::harness::channel_id(id),
            name: id.into(),
            timeline: screens::harness::general_timeline(),
            revision: 0,
        };
        let category = |id: &str, name: &str, position| screens::Category {
            id: terms::CategoryId::parse(id).expect("an id"),
            name: name.into(),
            position,
        };
        let placed = |category: &str, position| screens::Placement {
            category: terms::CategoryId::parse(category).expect("an id"),
            position,
        };
        let placed = [
            ("general", placed("hangouts", 0)),
            ("clips", placed("hangouts", 1)),
            ("ideas", placed("hangouts", 2)),
            ("lounge", placed("hangouts", 3)),
            ("corner", placed("hangouts", 4)),
            ("game-night", placed("projects", 0)),
        ]
        .into_iter()
        .map(|(id, placed)| (screens::harness::channel_id(id), placed))
        .collect();
        screens::ChannelList {
            text: vec![text("general"), text("clips"), text("ideas"), text("game-night")],
            voice: vec![channel("lounge", "Lounge"), channel("corner", "Quiet corner")],
            categories: vec![category("hangouts", "Hangouts", 0), category("projects", "Projects", 1)],
            placed,
            ..screens::ChannelList::default()
        }
    }
}

/// `list` with #staff, the canvas's one private channel, in Projects: the only place it joins the list.
fn with_staff(mut list: screens::ChannelList) -> screens::ChannelList {
    let id = screens::harness::channel_id("staff");
    list.text.push(screens::TextChannel {
        id: id.clone(),
        name: "staff".into(),
        timeline: screens::harness::general_timeline(),
        revision: 0,
    });
    let category = terms::CategoryId::parse("projects").expect("an id");
    list.placed.insert(id.clone(), screens::Placement { category, position: 1 });
    list.private.insert(id);
    list
}

/// Night Owls's list read again with #staff in it, for a state that shows a private channel.
fn staff_added(window: &mut Window, owls: PlaceId) {
    let listed = with_staff(channels_of(&night_owls()));
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(listed))));
}

/// The server telling Night Owls's account where it stands and that it holds `grants`, then the list it reads again
/// from there (the owner's holds #staff, whose overwrites bind everyone else) with the same words in each channel.
fn now_holding(window: &mut Window, owls: PlaceId, place: proto::rules::Place, grants: proto::rules::Grants) {
    at(window, owls, PlaceMessage::Session(screens::SessionMessage::Standing { place, grants }));
    let mut listed = channels_of(&night_owls());
    if place == proto::rules::Place::OWNER {
        listed = with_staff(listed);
        at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(listed.clone()))));
    }
    let ids = listed.text.into_iter().map(|channel| channel.id).chain(listed.voice.into_iter().map(|c| c.id));
    for channel_id in ids {
        let standing = screens::ChannelStanding { grants, read: 0, unread: 0, mentions: 0 };
        at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Standing { channel_id, standing }));
    }
}

fn flow(report: Report) -> PlaceMessage {
    PlaceMessage::Session(screens::SessionMessage::Door(Message::Reported(Heard::Flow(report))))
}

fn token() -> Report {
    Report::TokenLoaded(Some(Secret::new("t".into())))
}

/// Night Owls and Book club remembered with Ada's account, Night Owls signed in and open, on a Linux desktop, as the
/// canvas draws it (Startup and window is a page there); `books` does the rest.
fn both(books: fn(&mut Window)) -> Window {
    both_listing(books, channels_of(&night_owls()))
}

/// Night Owls with only its voice channels: no text channel opens, and the panel points at the voice ones.
fn no_chat() -> Window {
    let mut listed = channels_of(&night_owls());
    listed.text.clear();
    both_listing(books_signed_in, listed)
}

/// Both servers, Night Owls listing `listed`, after `books`.
fn both_listing(books: fn(&mut Window), listed: screens::ChannelList) -> Window {
    let rows = vec![Opening::Account(night_owls(), ADA.into()), Opening::Account(book_club(), ADA.into())];
    let (window, _) = Window::launch(rows, None, Instant::now());
    // The emoji this device picked lately, as the canvas's picker lists them.
    let owl = screens::chat::ReactionEmoji::Custom(emoji_id("owlwave"));
    let recent = ["👍", "🎲", "😂", "🎉", "❤️"].map(unicode).into_iter().chain([owl]);
    let linux = choices::Remembered {
        startup: Some(choices::Startup::default()),
        emoji: choices::EmojiPicks::new(recent, screens::chat::emoji::SkinTone::Default),
        ..choices::Remembered::default()
    };
    let mut window = window.remembering(linux, std::collections::BTreeMap::new());
    let owls = window.places()[0].id();
    at(&mut window, owls, flow(token()));
    let standing = screens::harness::standing_as(ME);
    at(&mut window, owls, flow(Report::Accepted { token: None, named: None, standing }));
    at(&mut window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(listed))));
    serves(&mut window, owls);
    emoji_listed(&mut window, owls, server_emoji_list());
    people(&mut window, owls, &[ADMIN_ROLE], false);
    books(&mut window);
    window
}

/// The canvas's roles, Game hosts and Helpers among them, and its people: Ada, grace holding `grace`, Bo a regular and
/// mira a moderator, all online; with `cy`, Cy too, offline, a helper.
fn people(window: &mut Window, owls: PlaceId, grace: &[&str], cy: bool) {
    at(window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles_more()))));
    // A page is taken while the members are being read, as they are at each sign-in.
    if window.places()[0].directory().person(&screens::harness::member(ME)).is_none() {
        let held: [(&str, &[&str]); 3] = [("grace", &[ADMIN_ROLE]), ("bo", &[REGULARS_ROLE]), ("mira", &[MODS_ROLE])];
        let clock = screens::harness::Clock(Instant::now());
        screens::harness::people_listed_as(window, &clock, owls, proto::status::Shown::Online, &held);
    }
    let roles = grace.iter().map(|role| screens::harness::role_id(role)).collect();
    let held = screens::PeopleMessage::Roles { user_id: screens::harness::member("grace"), roles };
    at(window, owls, PlaceMessage::People(held));
    if cy {
        let person = screens::Named {
            name: "Cy".into(),
            colour: None,
            roles: vec![screens::harness::role_id(HELPERS_ROLE)],
            picture: None,
        };
        let arrived = screens::PeopleMessage::Arrived { user_id: screens::harness::member("cy"), person };
        at(window, owls, PlaceMessage::People(arrived));
    }
}

/// The Channels button pressed: in a window too narrow to dock the pane, its drawer is open.
fn channels_shown(mut window: Window) -> Window {
    window.update(WindowMessage::ToggleChannels, moment());
    window
}

/// The pane dragged to its widest and let go.
fn pane_at_most(mut window: Window) -> Window {
    window.update(WindowMessage::Pane(PanePick::To(design::PANE_MOST)), moment());
    window.update(WindowMessage::Pane(PanePick::Let), moment());
    window
}

/// Back pressed on a settings page shown alone: in a window too narrow for the list beside it, the list shows.
fn settings_list(mut window: Window) -> Window {
    window.update(WindowMessage::SettingsList, moment());
    window
}

/// `screen` made, told the room the canvas's window gives it.
fn made(screen: &Screen) -> Window {
    let mut window = (screen.make)();
    told_the_room(&mut window);
    window
}

/// Today on the canvas, 12 November 2026, at its midnight in UTC.
const TODAY: i64 = 1_794_441_600;

fn said(place: u64, author: &str, at: i64, body: &str) -> screens::chat::Message {
    screens::chat::Message {
        id: screens::chat::MessageId::parse(&format!("m{place}")).expect("an id"),
        author: screens::chat::Author::Member(screens::harness::member(author)),
        place,
        revision: place,
        body: Some(screens::chat::Body::plain(body)),
        created_at: at,
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

fn tonight(hour: i64, minute: i64) -> i64 {
    TODAY + hour * 3_600 + minute * 60
}

/// A message whose body carries `marks`, each on the first place its text stands, in the text's order.
fn marked(
    place: u64,
    author: &str,
    at: i64,
    text: &str,
    marks: &[(&str, screens::chat::MarkKind)],
) -> screens::chat::Message {
    let marks = marks
        .iter()
        .map(|(needle, kind)| {
            let at = text.find(needle).expect("in the text");
            let start = text.char_indices().take_while(|(index, _)| *index < at).count();
            screens::chat::Mark { range: start..start + needle.chars().count(), kind: kind.clone() }
        })
        .collect();
    let mut message = said(place, author, at, text);
    message.body = Some(screens::chat::Body::new(text, marks).expect("marks in the text"));
    message
}

fn unicode(emoji: &str) -> screens::chat::ReactionEmoji {
    screens::chat::ReactionEmoji::Unicode(emoji.into())
}

fn custom(name: &str) -> screens::chat::MarkKind {
    screens::chat::MarkKind::CustomEmoji(emoji_id(name))
}

const RULES_LINK: &str = "https://games.example.invalid/rules";

/// The canvas's #general: Monday's and yesterday's ends, then tonight's runs, with a pin on each of two days, the
/// reactions on grace's question (this account's the thumb) and mira's reply to Bo.
fn general() -> Vec<screens::chat::Message> {
    use screens::chat::{MarkKind, Reaction, ReactionEmoji, Reply};
    let monday = |hour: i64, minute| tonight(hour - 72, minute);
    let yesterday = |hour: i64, minute| tonight(hour - 24, minute);
    let reaction = |emoji, count| Reaction { emoji, count };
    let mut night =
        said(1, "grace", monday(19, 30), "Game night is **Saturdays at 20:00**. Bring snacks, not spoilers.");
    night.pinned_at = Some(monday(19, 31));
    let mut spare = said(3, "mira", monday(19, 42), "I do, I’ll bring it Saturday");
    spare.reactions = vec![reaction(unicode("🙏"), 1)];
    let mut later = said(9, "grace", tonight(21, 2), "anyone up for a game later?");
    later.reactions = vec![
        reaction(unicode("👍"), 3),
        reaction(unicode("🎲"), 1),
        reaction(unicode("😂"), 2),
        reaction(ReactionEmoji::Custom(emoji_id("owlwave")), 2),
    ];
    later.mine = vec![unicode("👍")];
    let rules = format!("rules for the new one: {RULES_LINK}");
    let mut edited = marked(11, "bo", tonight(21, 6), &rules, &[(RULES_LINK, MarkKind::Link)]);
    edited.edited_at = Some(tonight(21, 8));
    edited.pinned_at = Some(tonight(21, 8));
    let mut eating = said(15, "mira", tonight(21, 13), "ten minutes is fine, still eating");
    eating.reply = Some(Reply { id: message_id(10), place: 10 });
    let game_night = MarkKind::Channel(screens::harness::channel_id("game-night"));
    let regulars = MarkKind::Role(screens::harness::role_id(REGULARS_ROLE));
    let ada = MarkKind::Mention(screens::harness::member(ME));
    vec![
        night,
        said(2, "bo", monday(19, 41), "who has the spare controller?"),
        spare,
        said(4, "mira", monday(19, 43), "it needs new batteries though"),
        said(5, "bo", monday(19, 58), "let’s try the co-op one next time"),
        said(6, "mira", yesterday(22, 40), "night all, same time tomorrow?"),
        marked(
            7,
            "mira",
            yesterday(22, 41),
            "I’ll bring the new map pack, more in #game-night",
            &[("#game-night", game_night)],
        ),
        marked(8, "grace", tonight(20, 58), "who’s around tonight? @Regulars", &[("@Regulars", regulars)]),
        later,
        said(10, "bo", tonight(21, 5), "yes! give me ten minutes"),
        edited,
        said(12, ME, tonight(21, 9), "I’m in. Lounge in *five*?"),
        marked(13, ME, tonight(21, 10), "bringing snacks :pizza:", &[(":pizza:", custom("pizza"))]),
        said(14, "mira", tonight(21, 12), "I can hear the game too"),
        eating,
        marked(16, "grace", tonight(21, 14), "@Ada can you open the Lounge?", &[("@Ada", ada)]),
    ]
}

/// What the canvas's default role holds, as a new server's default role does: its channels' words, voice and sharing.
fn member_words() -> proto::rules::Grants {
    use proto::rules::Grant::{
        AddReactions, AttachFiles, ChannelRead, SendMessage, ShareCamera, ShareScreen, StartVoice,
    };
    [ChannelRead, SendMessage, AddReactions, AttachFiles, StartVoice, ShareScreen, ShareCamera].into_iter().collect()
}

/// Night Owls open on `channel`, the day and the members as the canvas has them, `messages` read, then `then`.
fn in_channel(channel: &str, messages: Vec<screens::chat::Message>, then: impl FnOnce(&mut Window, PlaceId)) -> Window {
    opened(channel, messages, None, then)
}

/// The same, the channel opened read through `read` when one is given: the New line stands after it.
fn opened(
    channel: &str,
    messages: Vec<screens::chat::Message>,
    read: Option<u64>,
    then: impl FnOnce(&mut Window, PlaceId),
) -> Window {
    let mut window = both(books_signed_in);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    now_holding(&mut window, owls, proto::rules::Place::BOTTOM, member_words());
    if channel == "staff" {
        staff_added(&mut window, owls);
    }
    // grace, Bo and mira in Lounge, which this account is not in, as the canvas's chat has them.
    let lounge = screens::harness::channel_id("lounge");
    let seated = occupants_placed(&["grace", "Bo", "mira"]);
    at(&mut window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Occupancy { channel_id: lounge, people: seated }));
    let channel_id = screens::harness::channel_id(channel);
    if let Some(read) = read {
        let unread = messages.iter().filter(|message| message.place > read).count();
        let standing = screens::ChannelStanding {
            grants: member_words(),
            read,
            unread: u32::try_from(unread).expect("a count"),
            mentions: 0,
        };
        let told = screens::LobbyMessage::Standing { channel_id: channel_id.clone(), standing };
        at(&mut window, owls, PlaceMessage::Lobby(told));
    }
    at(&mut window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Open(channel_id)));
    let page = screens::chat::Page { messages, older: false, newer: false, revision: 100 };
    let first =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(&mut window, owls, PlaceMessage::Discussion(first));
    if read.is_some() {
        // Looked at in a focused window, the channel is read: its row in the list is plain again.
        window.update(WindowMessage::Focused, moment());
    }
    then(&mut window, owls);
    window
}

/// The canvas's chat.format: every markdown style and kind of mark, as tonight's runs in #general.
fn format() -> Vec<screens::chat::Message> {
    use screens::chat::MarkKind;
    let channel = MarkKind::Channel(screens::harness::channel_id("game-night"));
    let regulars = MarkKind::Role(screens::harness::role_id(REGULARS_ROLE));
    let everyone = "@everyone the game moves to #game-night this week, @Regulars you host";
    let linked = format!("@grace full rules: {RULES_LINK}");
    vec![
        said(
            1,
            "grace",
            tonight(20, 40),
            "**Saturday, 20:00 sharp.** *Bring snacks*, __no spoilers__, and it’s ~~19:30~~ 20:00 now. Roll with \
             `/roll 2d6`.",
        ),
        said(
            2,
            "bo",
            tonight(20, 44),
            "house rules, so nobody argues:\n```\ndoubles roll again\nthree doubles: skip a turn\n```",
        ),
        marked(
            3,
            "mira",
            tonight(20, 46),
            "> doubles roll again\nthat one always starts a fight :owlwave:",
            &[(":owlwave:", custom("owlwave"))],
        ),
        marked(
            4,
            "grace",
            tonight(20, 50),
            everyone,
            &[("@everyone", MarkKind::Everyone), ("#game-night", channel), ("@Regulars", regulars)],
        ),
        marked(
            5,
            "bo",
            tonight(20, 52),
            "@here the twist is ||the butler had the dice all along||",
            &[("@here", MarkKind::Here)],
        ),
        marked(
            6,
            "mira",
            tonight(20, 55),
            &linked,
            &[("@grace", MarkKind::Mention(screens::harness::member("grace"))), (RULES_LINK, MarkKind::Link)],
        ),
    ]
}

fn chat_format() -> Window {
    in_channel("general", format(), |_, _| {})
}

/// Bo's spoiler pressed: shown.
fn chat_format_spoiler() -> Window {
    in_channel("general", format(), |window, owls| {
        let twist = format().swap_remove(4);
        let spoiler = twist.body.as_ref().and_then(|body| body.runs().find_map(|run| run.style.spoiler));
        let shown = screens::DiscussionMessage::Reveal(twist.id, spoiler.expect("a spoiler"));
        at(window, owls, PlaceMessage::Discussion(shown));
    })
}

/// #general as the canvas opens it: read through Ada's own last message, so the New line stands over mira's next.
fn in_general(then: fn(&mut Window, PlaceId)) -> Window {
    opened("general", general(), Some(13), then)
}

/// A new channel: nothing in it yet.
fn chat_empty() -> Window {
    in_channel("ideas", Vec::new(), |_, _| {})
}

/// A deleted message in a run, and two former members, each their own.
fn chat_deleted() -> Window {
    let deleted = |mut message: screens::chat::Message| {
        message.body = None;
        message
    };
    let former = |place: u64, at: i64, body: &str, author: &str| screens::chat::Message {
        author: screens::chat::Author::Former(author.into()),
        ..said(place, "grace", at, body)
    };
    let messages = vec![
        said(1, "grace", tonight(21, 2), "anyone up for a game later?"),
        deleted(said(2, "grace", tonight(21, 2) + 30, "")),
        former(3, tonight(21, 4), "count me in", "f1"),
        former(4, tonight(21, 5), "same here, Lounge?", "f2"),
        deleted(former(5, tonight(21, 5) + 30, "", "f2")),
        deleted(said(6, "mira", tonight(21, 8), "")),
        said(7, "bo", tonight(21, 10), "Lounge it is"),
    ];
    in_channel("general", messages, |_, _| {})
}

/// A channel this account may read and not write in.
fn chat_cannot_send() -> Window {
    in_general(|window, owls| {
        let reader = [proto::rules::Grant::ChannelRead].into_iter().collect();
        now_holding(window, owls, proto::rules::Place::BOTTOM, reader);
    })
}

/// Scrolled up from the newest: older messages are being read, and the way back to the present is offered.
fn scrolled_back(window: &mut Window, owls: PlaceId) {
    let page = screens::chat::Page { messages: general(), older: true, newer: true, revision: 100 };
    let read =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(window, owls, PlaceMessage::Discussion(read));
    let scrolled = screens::DiscussionMessage::Scrolled { from_end: 900.0, viewport: 600.0 };
    at(window, owls, PlaceMessage::Discussion(scrolled));
}

fn message_id(place: u64) -> screens::chat::MessageId {
    screens::chat::MessageId::parse(&format!("m{place}")).expect("an id")
}

fn menu_on(window: &mut Window, owls: PlaceId, place: u64) {
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Menu(message_id(place))));
}

fn editing(window: &mut Window, owls: PlaceId, place: u64) {
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Edit(message_id(place))));
}

fn drafting(window: &mut Window, owls: PlaceId, text: &str) {
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Draft(text.into())));
}

/// Ada made an admin; opening the server puts away the toast that says so, as the canvas has none.
fn made_admin(window: &mut Window, owls: PlaceId) {
    now_holding(window, owls, screens::harness::ADMIN_PLACE, every_word());
    window.update(WindowMessage::Open(owls), moment());
}

/// Ada an admin, then the menu of the message at `place`.
fn admin_menu_on(window: &mut Window, owls: PlaceId, place: u64) {
    made_admin(window, owls);
    menu_on(window, owls, place);
}

/// Reply pressed on grace's question to Ada, and an answer typed.
fn replying(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Reply(message_id(16))));
    drafting(window, owls, "opening it now");
}

/// #general after Bo deleted the message mira answered: read alone, it is gone.
fn chat_reply_deleted() -> Window {
    let kept = general().into_iter().filter(|message| message.place != 10).collect();
    opened("general", kept, Some(13), |window, owls| {
        let gone = screens::chat::Page { messages: Vec::new(), older: false, newer: false, revision: 100 };
        let quoted = screens::DiscussionMessage::Quoted {
            timeline: screens::harness::general_timeline(),
            place: 10,
            page: Ok(gone),
        };
        at(window, owls, PlaceMessage::Discussion(quoted));
    })
}

/// React pressed on grace's question: the picker, its grid at `section`.
fn reacting(window: &mut Window, owls: PlaceId, section: screens::PickerSection) {
    use screens::{DiscussionMessage::Picker, PickFor, PickerMessage};
    at(window, owls, PlaceMessage::Discussion(Picker(PickerMessage::Open(PickFor::Message(message_id(9))))));
    if section != screens::PickerSection::Recent {
        at(window, owls, PlaceMessage::Discussion(Picker(PickerMessage::Section(section))));
    }
}

/// Ada an admin, the header's Pinned messages pressed and its page read: the newest pin first.
fn pins_open(window: &mut Window, owls: PlaceId) {
    made_admin(window, owls);
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::TogglePins));
    let mut pinned: Vec<screens::chat::Message> =
        general().into_iter().filter(|message| message.pinned_at.is_some()).collect();
    pinned.reverse();
    let page = screens::chat::Page { messages: pinned, older: false, newer: false, revision: 100 };
    let read = screens::DiscussionMessage::Pins { timeline: screens::harness::general_timeline(), page: Ok(page) };
    at(window, owls, PlaceMessage::Discussion(read));
}

/// "dice" typed in the header's search field, asked once typing paused: the conversation with grace is known, as one
/// match is written in it.
fn search_typed(window: &mut Window, owls: PlaceId, typed: &str) {
    serves(window, owls);
    talkers(window, owls);
    conversations_listed(window, owls, vec![conversation_of("c-grace", Some("grace"), 0, None)]);
    at(window, owls, PlaceMessage::Search(screens::SearchMessage::Open));
    at(window, owls, PlaceMessage::Search(screens::SearchMessage::Type(typed.into())));
    let paused = Moment { now: Instant::now() + screens::SEARCH_PAUSE * 2, ..moment() };
    window.update(WindowMessage::Tick, paused);
}

/// What the server answered to `typed`.
fn search_answered(
    window: &mut Window,
    owls: PlaceId,
    typed: &str,
    page: Result<screens::chat::Results, terms::LinkError>,
) {
    let query = proto::rules::SearchQuery::parse(typed).expect("a query");
    let answered = screens::SearchMessage::Answered { query, before: None, page };
    at(window, owls, PlaceMessage::Search(answered));
}

/// The canvas's matches for "dice": one in #general, one in a thread of it, one in the conversation with grace, and two
/// files' names; older ones follow.
fn search_found(window: &mut Window, owls: PlaceId) {
    found_dice(window, owls, true);
}

/// The same from a call, the day and the zone told as the chat has them, with no older matches.
fn search_in_call(window: &mut Window, owls: PlaceId) {
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    found_dice(window, owls, false);
}

/// "dice" found in three messages and two files' names, `older` matches following where told.
fn found_dice(window: &mut Window, owls: PlaceId, older: bool) {
    use proto::files::PictureType;
    use screens::chat::{Found, Home};
    search_typed(window, owls, "dice");
    let yesterday = |hour: i64, minute| tonight(hour - 24, minute);
    let general = screens::harness::general_timeline();
    let found = vec![
        Found {
            message: said(31, "mira", yesterday(22, 38), "who has the dice from **last week**?"),
            timeline: general.clone(),
            home: Home::Timeline,
            files: Vec::new(),
            in_body: true,
        },
        Found {
            message: said(32, "bo", tonight(21, 16), "bringing my own dice, the red ones"),
            timeline: screens::harness::timeline_id("t-thread"),
            home: Home::Thread {
                channel: screens::harness::channel_id("general"),
                root: screens::harness::message_id(9),
            },
            files: Vec::new(),
            in_body: true,
        },
        Found {
            message: said(
                33,
                "grace",
                tonight(18, 2) - 3 * 24 * 3_600,
                "don’t forget the dice on Saturday, *all five*",
            ),
            timeline: screens::harness::timeline_id("t-c-grace"),
            home: Home::Timeline,
            files: Vec::new(),
            in_body: true,
        },
        found_file(35, yesterday(22, 40), "dice.png", 2_400_000, proto::files::MediaType::Picture(PictureType::Png)),
        found_file(36, tonight(19, 30) - 3 * 24 * 3_600, "dice-odds.pdf", 182_000, proto::files::MediaType::Bytes),
    ];
    search_answered(window, owls, "dice", Ok(screens::chat::Results { found, older }));
}

/// A message in #general found only by the name of its one file.
fn found_file(place: u64, at: i64, name: &str, size: u64, media: proto::files::MediaType) -> screens::chat::Found {
    let mut message = said(place, "mira", at, "");
    let file = proto::files::FileId::from_bytes([u8::try_from(place).expect("a byte"); proto::files::FileId::LEN]);
    let name = proto::files::AttachmentName::parse(name).expect("a name");
    message.attachments = vec![screens::chat::Attachment { file, name, size, media, picture: None }];
    screens::chat::Found {
        message,
        timeline: screens::harness::general_timeline(),
        home: screens::chat::Home::Timeline,
        files: vec![file],
        in_body: false,
    }
}

/// The layer open with nothing typed.
fn search_open(window: &mut Window, owls: PlaceId) {
    serves(window, owls);
    at(window, owls, PlaceMessage::Search(screens::SearchMessage::Open));
}

/// "png" found only in files' names.
fn search_files(window: &mut Window, owls: PlaceId) {
    use screens::chat::{Found, Home};
    search_typed(window, owls, "png");
    let mut message = said(34, "mira", tonight(20, 1), "");
    let attachment = |file: u8, name: &str, size| screens::chat::Attachment {
        file: proto::files::FileId::from_bytes([file; proto::files::FileId::LEN]),
        name: proto::files::AttachmentName::parse(name).expect("a name"),
        size,
        media: proto::files::MediaType::Picture(proto::files::PictureType::Png),
        picture: None,
    };
    message.attachments = vec![attachment(1, "board.png", 860_000), attachment(2, "table.png", 3_100_000)];
    let files = message.attachments.iter().map(|attached| attached.file).collect();
    let found = vec![Found {
        message,
        timeline: screens::harness::general_timeline(),
        home: Home::Timeline,
        files,
        in_body: false,
    }];
    search_answered(window, owls, "png", Ok(screens::chat::Results { found, older: false }));
}

/// "xyzzy" found nowhere.
fn search_empty(window: &mut Window, owls: PlaceId) {
    search_typed(window, owls, "xyzzy");
    search_answered(window, owls, "xyzzy", Ok(screens::chat::Results::default()));
}

/// The search not answering.
fn search_failed(window: &mut Window, owls: PlaceId) {
    search_typed(window, owls, "dice");
    search_answered(window, owls, "dice", Err(terms::LinkError::Timeout));
}

/// The search asked and not answered yet.
fn search_waiting(window: &mut Window, owls: PlaceId) {
    search_typed(window, owls, "dice");
}

/// A message holding `url` as a link, with the preview its sender made of it.
fn with_preview(
    place: u64,
    author: &str,
    at: i64,
    lead: &str,
    url: &str,
    card: screens::chat::Card,
) -> screens::chat::Message {
    use screens::chat::{Body, Mark, MarkKind};
    let start = lead.chars().count();
    let mark = Mark { range: start..start + url.chars().count(), kind: MarkKind::Link };
    let mut message = said(place, author, at, "");
    message.body = Some(Body::new(format!("{lead}{url}"), vec![mark]).expect("a body"));
    message.preview = Some(card);
    message
}

/// A preview with a picture and one without, as the canvas's chat.preview has them.
fn chat_preview() -> Window {
    use screens::chat::{Card, CardPicture};
    let blog = "https://blog.example.invalid/game-night";
    let rules = "https://games.example.invalid/rules";
    let write_up = Card {
        url: blog.into(),
        host: "blog.example.invalid".into(),
        title: "Game night, week 12: the long campaign begins".into(),
        description: "Five players, one map and far too many snacks. How the first session went, what broke, and what \
                      we are changing before the next one."
            .into(),
        picture: Some(CardPicture {
            file: proto::files::FileId::from_bytes([5; proto::files::FileId::LEN]),
            width: 1600,
            height: 1200,
            thumbnail: proto::files::FileId::from_bytes([6; proto::files::FileId::LEN]),
        }),
    };
    let house_rules = Card {
        url: rules.into(),
        host: "games.example.invalid".into(),
        title: "House rules for the new game".into(),
        description: "Doubles roll again, three doubles skip a turn. Everything we agreed on, on one page.".into(),
        picture: None,
    };
    let messages = vec![
        with_preview(1, "grace", tonight(21, 15), "this week’s write-up is out: ", blog, write_up),
        with_preview(2, "bo", tonight(21, 18), "and the rules, for anyone new: ", rules, house_rules),
    ];
    in_channel("general", messages, |_, _| {})
}

/// A draft with a link in it, the page read (when `made`) or still being read.
fn chat_composing_preview(made: bool) -> Window {
    let url = "https://blog.example.invalid/game-night";
    in_channel("general", general(), move |window, owls| {
        serves(window, owls);
        drafting(window, owls, &format!("this week’s write-up: {url}"));
        let paused = Moment { now: Instant::now() + screens::PREVIEW_PAUSE * 2, ..moment() };
        window.update(WindowMessage::Tick, paused);
        if made {
            let card = screens::PreviewMade {
                url: url.into(),
                title: "Game night, week 12: the long campaign begins".into(),
                description: String::new(),
                picture: None,
            };
            let compose = screens::harness::general_compose();
            let told = screens::DiscussionMessage::Previewed { compose, link: url.into(), made: Some(card) };
            at(window, owls, PlaceMessage::Discussion(told));
        }
    })
}

/// An `@` typed by a member, with the roles the canvas's chat has.
fn mentioning(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles()))));
    drafting(window, owls, "who’s hosting? @");
}

/// The same by an admin, who holds Mention everyone.
fn mentioning_as_admin(window: &mut Window, owls: PlaceId) {
    made_admin(window, owls);
    mentioning(window, owls);
}

/// The server telling who is writing in #general.
fn typing(window: &mut Window, owls: PlaceId, who: &[&str]) {
    let users = who.iter().map(|user| screens::harness::member(user)).collect();
    let told = screens::DiscussionMessage::Typing { timeline: screens::harness::general_timeline(), users };
    at(window, owls, PlaceMessage::Discussion(told));
}

/// The account's notification settings read: only mentions, and each channel of `muted` muted until turned back on.
fn notify_read(window: &mut Window, owls: PlaceId, muted: &[&str]) {
    use proto::notify::{ChannelSettings, Level, Mute, ServerSettings};
    let quiet = ChannelSettings { level: None, mute: Some(Mute::Forever) };
    let read = screens::Notifying {
        server: ServerSettings { level: Level::Mentions, ..ServerSettings::default() },
        channels: muted.iter().map(|channel| (screens::harness::channel_id(channel), quiet)).collect(),
    };
    at(window, owls, PlaceMessage::Notify(screens::NotifyMessage::Read(Ok(read))));
}

/// Messages unread in other channels: one in #clips, two that mention Ada in #game-night, and a mention each in
/// #ideas, which is muted, and in Lounge's chat.
fn unread_elsewhere(window: &mut Window, owls: PlaceId) {
    notify_read(window, owls, &["ideas"]);
    for (channel, unread, mentions) in [("clips", 1, 0), ("game-night", 2, 2), ("ideas", 1, 1), ("lounge", 1, 1)] {
        let standing = screens::ChannelStanding { grants: member_words(), read: 0, unread, mentions };
        let told = screens::LobbyMessage::Standing { channel_id: screens::harness::channel_id(channel), standing };
        at(window, owls, PlaceMessage::Lobby(told));
    }
}

/// A picture of the canvas's as the host holds it: the file and its thumbnail, its shape, its bytes, and the colours
/// of its table and its floor.
struct Shot {
    name: &'static str,
    file: proto::files::FileId,
    thumbnail: proto::files::FileId,
    width: u32,
    height: u32,
    size: u64,
    table: [u8; 3],
    floor: [u8; 3],
}

const fn file_id(byte: u8) -> proto::files::FileId {
    proto::files::FileId::from_bytes([byte; proto::files::FileId::LEN])
}

const fn shot(name: &'static str, id: u8, shape: (u32, u32), size: u64, table: [u8; 3], floor: [u8; 3]) -> Shot {
    Shot { name, file: file_id(id), thumbnail: file_id(id + 1), width: shape.0, height: shape.1, size, table, floor }
}

const DICE: Shot = shot("dice.png", 0x10, (1600, 1200), 2_400_000, [0x2E, 0x5A, 0x43], [0x5B, 0x3A, 0x24]);
const TABLE: Shot = shot("table.png", 0x12, (1600, 1200), 3_100_000, [0x3B, 0x2F, 0x5C], [0x2A, 0x21, 0x42]);
const CARDS: Shot = shot("cards.png", 0x14, (1200, 1200), 1_800_000, [0x7A, 0x2E, 0x3B], [0x4A, 0x1C, 0x24]);
const SNACKS: Shot = shot("snacks.png", 0x16, (1200, 1200), 2_200_000, [0x1F, 0x4E, 0x6B], [0x6B, 0x4A, 0x2A]);
const BOARD: Shot = shot("board.png", 0x18, (1024, 768), 860_000, [0x5B, 0x3A, 0x24], [0x2E, 0x5A, 0x43]);
/// The rules grace sent, a file that is no picture.
const RULES: proto::files::FileId = file_id(0x1A);

impl Shot {
    fn attachment(&self) -> screens::chat::Attachment {
        use proto::files::{AttachmentName, MediaType, PictureType};
        screens::chat::Attachment {
            file: self.file,
            name: AttachmentName::parse(self.name).expect("a name"),
            size: self.size,
            media: MediaType::Picture(PictureType::Png),
            picture: Some(screens::chat::Picture {
                width: self.width,
                height: self.height,
                thumbnail: Some(self.thumbnail),
            }),
        }
    }

    /// The picture decoded, at a quarter of its size: the view scales it either way.
    fn handle(&self) -> iced_core::image::Handle {
        scene(self.width / 4, self.height / 4, self.table, self.floor)
    }
}

/// A picture of the canvas's kind: a table in one colour over a floor in another.
fn scene(width: u32, height: u32, table: [u8; 3], floor: [u8; 3]) -> iced_core::image::Handle {
    let horizon = height * 4 / 5;
    let rgba: Vec<u8> = (0..height)
        .flat_map(|y| {
            let [r, g, b] = if y < horizon { table } else { floor };
            (0..width).flat_map(move |_| [r, g, b, 255])
        })
        .collect();
    iced_core::image::Handle::from_rgba(width, height, rgba)
}

/// The files the host holds decoded: the canvas's pictures, each with its thumbnail, and Night Owls's emoji.
fn held(file: proto::files::FileId) -> Option<iced_core::image::Handle> {
    static HELD: std::sync::OnceLock<Vec<(proto::files::FileId, iced_core::image::Handle)>> =
        std::sync::OnceLock::new();
    let held = HELD.get_or_init(|| {
        let shots = [DICE, TABLE, CARDS, SNACKS, BOARD].into_iter().flat_map(|shot| {
            let handle = shot.handle();
            [(shot.file, handle.clone()), (shot.thumbnail, handle)]
        });
        let emoji = (0..).zip(EMOJI).map(|(at, (_, _, colour))| (emoji_file(at), scene(32, 32, colour, colour)));
        shots.chain(emoji).collect()
    });
    held.iter().find(|(id, _)| *id == file).map(|(_, handle)| handle.clone())
}

/// A plain fill wherever the host would paint a screen, a camera or a server's picture, as `app::view` draws one.
fn plain<'a>(painted: &picture::Painted) -> Element<'a, WindowMessage, Theme, Renderer> {
    use design::{Content, Role, Surface};
    use iced_core::{Length, Widget};
    let fill = iced_widget::container(iced_widget::Space::new());
    match painted {
        picture::Painted::Picture { radius, .. } => fill
            .width(Length::Fill)
            .height(Length::Fill)
            .class(ui::Surfaced::Fill {
                fill: Role::Surface(Surface::Inset),
                text: Role::Content(Content::Primary),
                radius: *radius,
            })
            ._boxed(),
        picture::Painted::Screen { .. }
        | picture::Painted::OwnScreen(_)
        | picture::Painted::Camera { .. }
        | picture::Painted::OwnCamera(..)
        | picture::Painted::Still(..) => fill.class(ui::Surfaced::On(Surface::Inset))._boxed(),
    }
}

/// The canvas's chat.files: a picture sent alone, three sent together, and a file to save.
fn files() -> Vec<screens::chat::Message> {
    use proto::files::{AttachmentName, MediaType};
    let mut alone = said(2, "bo", tonight(21, 18), "");
    alone.attachments = vec![DICE.attachment()];
    let mut photos = said(3, "mira", tonight(21, 19), "photos from Saturday");
    photos.attachments = vec![TABLE.attachment(), CARDS.attachment(), SNACKS.attachment()];
    let mut printable = said(4, "grace", tonight(21, 20), "and the rules, printable");
    printable.attachments = vec![screens::chat::Attachment {
        file: RULES,
        name: AttachmentName::parse("rules-v2.pdf").expect("a name"),
        size: 1_200_000,
        media: MediaType::Bytes,
        picture: None,
    }];
    vec![said(1, "grace", tonight(21, 15), "how did Saturday go?"), alone, photos, printable]
}

/// #general holding the canvas's files, the server greeting with the most it takes: 50 MB a file, 1 GB an account.
fn chat_files(then: fn(&mut Window, PlaceId)) -> Window {
    in_channel("general", files(), |window, owls| {
        let identity = terms::ServerIdentity { name: Some("Night Owls".into()), colour: None, picture: None };
        let limits = terms::FileLimits { attachment: Some(50 << 20), account: Some(1 << 30) };
        at(window, owls, PlaceMessage::Server(screens::ServerMessage::Greeted { identity, limits }));
        then(window, owls);
    })
}

/// `names` dropped on #general: each joins the composer's strip and starts to go; the uploads, in that order.
fn dropped(window: &mut Window, owls: PlaceId, names: &[&str]) -> Vec<screens::Upload> {
    let paths = names.iter().map(PathBuf::from).collect();
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Dropped(paths)));
    let composing = window.places()[0].composing(&screens::harness::general_compose()).expect("a composer");
    composing.files().iter().map(|file| file.upload).collect()
}

/// What the upload `upload` says of itself.
fn upload_said(
    window: &mut Window,
    owls: PlaceId,
    upload: screens::Upload,
    said: Result<screens::Uploaded, terms::FileError>,
) {
    let compose = screens::harness::general_compose();
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Upload { compose, upload, said }));
}

/// Two files waiting to send: the map pack a third gone, the board kept by the server.
fn uploading(window: &mut Window, owls: PlaceId) {
    let uploads = dropped(window, owls, &["map-pack.zip", BOARD.name]);
    upload_said(window, owls, uploads[0], Ok(screens::Uploaded::Sent { sent: 4_100_000, of: 12_000_000 }));
    upload_said(window, owls, uploads[1], Ok(screens::Uploaded::Sent { sent: BOARD.size, of: BOARD.size }));
    upload_said(window, owls, uploads[1], Ok(screens::Uploaded::Kept(BOARD.file)));
}

/// Two files refused: one over the most a file may be, one past what the account may keep.
fn uploads_refused(window: &mut Window, owls: PlaceId) {
    use proto::files::{FileKind, FileRefusal};
    let uploads = dropped(window, owls, &["season-recap.mp4", "all-photos.zip"]);
    for (upload, refusal) in uploads.into_iter().zip([FileRefusal::TooLarge, FileRefusal::AccountFull]) {
        let refused = terms::FileError::Refused { kind: FileKind::Attachment, refusal };
        upload_said(window, owls, upload, Err(refused));
    }
}

/// Bo's picture pressed, its original fetched by the host.
fn viewing_dice(window: &mut Window, owls: PlaceId) {
    let view = screens::DiscussionMessage::View { message: message_id(2), file: DICE.file };
    at(window, owls, PlaceMessage::Discussion(view));
    at(window, owls, PlaceMessage::Server(screens::ServerMessage::FileReady(DICE.file)));
}

/// Files dragged over the window.
fn files_over(window: &mut Window) {
    window.update(WindowMessage::FilesHovered(true), moment());
}

/// Night Owls's own emoji on the canvas: each one's name, who added it, and the colour its picture is here.
const EMOJI: [(&str, &str, [u8; 3]); 12] = [
    ("owlwave", "grace", [0x8A, 0x5A, 0x3C]),
    ("owl_sleepy", "grace", [0x6B, 0x5B, 0x95]),
    ("party_owl", "mira", [0x8A, 0x5A, 0x3C]),
    ("gold_die", "bo", [0xF2, 0xC1, 0x4E]),
    ("red_die", "bo", [0xE8, 0x55, 0x3B]),
    ("trophy", "grace", [0xF2, 0xC1, 0x4E]),
    ("tomato", "mira", [0xE8, 0x55, 0x3B]),
    ("owl_heart", "mira", [0xE0, 0x45, 0x7B]),
    ("night", "grace", [0xFF, 0xC8, 0x57]),
    ("gamepad", "bo", [0x8A, 0x44, 0xA8]),
    ("pizza", "bo", [0xFF, 0xC8, 0x57]),
    ("gg_star", "mira", [0xFF, 0xC8, 0x57]),
];

fn emoji_id(name: &str) -> screens::chat::EmojiId {
    screens::chat::EmojiId::parse(name).expect("an id")
}

/// The picture of the emoji at `at` of [`EMOJI`], as the host holds it.
const fn emoji_file(at: u8) -> proto::files::FileId {
    file_id(0x40 + at)
}

fn emoji_named(name: &str, by: &str, file: proto::files::FileId) -> screens::ServerEmoji {
    screens::ServerEmoji {
        id: emoji_id(name),
        name: proto::rules::emoji_name(name).expect("a name"),
        file,
        made_by: Some(screens::harness::member(by)),
    }
}

fn server_emoji_list() -> Vec<screens::ServerEmoji> {
    (0..).zip(EMOJI).map(|(at, (name, by, _))| emoji_named(name, by, emoji_file(at))).collect()
}

/// The canvas's twelve and as many more as a server holds, named to list after them, their pictures not held.
fn server_emoji_full() -> Vec<screens::ServerEmoji> {
    let more = (EMOJI.len()..proto::rules::MAX_EMOJI).map(|at| emoji_named(&format!("zz_{at:03}"), "grace", RULES));
    server_emoji_list().into_iter().chain(more).collect()
}

/// The server's emoji read, as at each sign-in.
fn emoji_listed(window: &mut Window, owls: PlaceId, emoji: Vec<screens::ServerEmoji>) {
    let page = screens::EmojiPage { emoji, next: None };
    at(window, owls, PlaceMessage::Emoji(screens::EmojiMessage::Page(Ok(page))));
}

fn on_emoji_page(window: &mut Window, owls: PlaceId, message: screens::EmojiPageMessage) {
    at(window, owls, PlaceMessage::Page(screens::PageMessage::Emoji(message)));
}

/// Server settings › Emoji as the server's owner in Lounge, the server holding `emoji`, then `then`.
fn server_emoji(emoji: Vec<screens::ServerEmoji>, then: fn(&mut Window, PlaceId)) -> Window {
    let mut window = confirm(Pick::Settings, true);
    let owls = window.places()[0].id();
    for added in emoji.into_iter().skip(EMOJI.len()) {
        at(&mut window, owls, PlaceMessage::Emoji(screens::EmojiMessage::Added(added)));
    }
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Emoji)));
    then(&mut window, owls);
    window
}

/// Rename pressed on :owlwave: and `typed` in its field.
fn renaming_owlwave(window: &mut Window, owls: PlaceId, typed: &str) {
    on_emoji_page(window, owls, screens::EmojiPageMessage::Rename(emoji_id("owlwave")));
    on_emoji_page(window, owls, screens::EmojiPageMessage::Draft(typed.into()));
}

/// The canvas's chat of Lounge, the voice channel.
fn lounge() -> Vec<screens::chat::Message> {
    use screens::chat::MarkKind;
    let lobby = "https://games.example.invalid/lobby";
    let ada = MarkKind::Mention(screens::harness::member(ME));
    vec![
        said(1, "grace", tonight(21, 20), "can everyone hear me?"),
        said(2, "bo", tonight(21, 21), "loud and clear"),
        marked(
            3,
            "mira",
            tonight(21, 21) + 30,
            "I’m deafened, reading here instead :owl_sleepy:",
            &[(":owl_sleepy:", custom("owl_sleepy"))],
        ),
        marked(4, "mira", tonight(21, 22), &format!("lobby: {lobby}"), &[(lobby, MarkKind::Link)]),
        marked(5, "grace", tonight(21, 24), "@Ada you’re up **next**", &[("@Ada", ada)]),
    ]
}

/// In Lounge's call holding what the canvas's default role does, the header's Chat pressed and its messages read:
/// beside the call where the window has the room, over it where not.
fn voice_chat() -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    let held = proto::rules::Grants::from_words(&[
        "CHANNEL_READ",
        "SEND_MESSAGE",
        "ADD_REACTIONS",
        "ATTACH_FILES",
        "SHARE_SCREEN",
        "SHARE_CAMERA",
        "START_VOICE",
    ]);
    now_holding(&mut window, owls, proto::rules::Place::BOTTOM, held);
    window.update(WindowMessage::Encoders(encoders()), moment());
    window.update(WindowMessage::Open(owls), moment());
    window.update(WindowMessage::ToggleCallChat, moment());
    let page = screens::chat::Page { messages: lounge(), older: false, newer: false, revision: 100 };
    let read = screens::DiscussionMessage::Page {
        timeline: screens::harness::timeline_id("lounge"),
        end: None,
        page: Ok(page),
    };
    at(&mut window, owls, PlaceMessage::Discussion(read));
    window
}

/// A right press where the channel list has no row.
fn pane_menu(window: &mut Window, owls: PlaceId) {
    window.update(WindowMessage::Menu(owls, MenuFrom::Pane), moment());
}

/// #ideas's menu with its submenu `sub` open, the account's notification settings read.
fn ideas_sub(window: &mut Window, owls: PlaceId, sub: screens::ListSub) {
    notify_read(window, owls, &[]);
    list_pick(window, owls, channel_key("ideas"), screens::ListPick::Open(sub));
}

/// Notification settings from Night Owls's menu, #general open during the call in Lounge as the canvas's window
/// is; with `muting`, its Mute row's durations open.
fn notify_server(muting: bool) -> Window {
    let mut window = both(in_lounge_reading);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    now_holding(&mut window, owls, proto::rules::Place::BOTTOM, member_words());
    let general = screens::DiscussionMessage::Open(screens::harness::channel_id("general"));
    at(&mut window, owls, PlaceMessage::Discussion(general));
    let messages = vec![
        said(1, "grace", tonight(21, 2), "anyone up for a game later?"),
        said(2, "bo", tonight(21, 5), "yes! give me ten minutes"),
        said(3, "mira", tonight(21, 12), "I can hear the game too"),
    ];
    let page = screens::chat::Page { messages, older: false, newer: false, revision: 100 };
    let first =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(&mut window, owls, PlaceMessage::Discussion(first));
    notify_read(&mut window, owls, &[]);
    window.update(WindowMessage::Pick(owls, Pick::Notify), moment());
    if muting {
        at(&mut window, owls, PlaceMessage::Notify(screens::NotifyMessage::MuteRow));
    }
    window
}

/// The channels page with the category Hangouts chosen.
fn category_chosen() -> Window {
    let mut window = server_channels();
    let owls = window.places()[0].id();
    let hangouts = terms::CategoryId::parse("hangouts").expect("an id");
    let choose = screens::channel_settings::Message::ChooseCategory(hangouts);
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Channels(choose)));
    window
}

/// Book club signed in too, as the canvas has it.
fn books_signed_in(window: &mut Window) {
    let id = books(window);
    at(window, id, flow(token()));
    at(window, id, flow(Report::Accepted { token: None, named: None, standing: screens::harness::standing_as("ada") }));
    at(window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(channels_of(&book_club())))));
}

fn books(window: &Window) -> PlaceId {
    window.places()[1].id()
}

/// Book club opened, after `then`.
fn on_books(then: fn(&mut Window)) -> Window {
    let mut window = both(then);
    let id = books(&window);
    window.update(WindowMessage::Open(id), moment());
    window
}

fn books_ended(reason: Option<CloseReason>) -> Window {
    let mut window = both(|_| {});
    let id = books(&window);
    at(&mut window, id, flow(token()));
    at(&mut window, id, flow(Report::Expired(reason)));
    window.update(WindowMessage::Open(id), moment());
    window
}

/// Book club, signing in again by itself, presents another key than the one this device trusts for it.
fn key_changed() -> Window {
    let mut window = both(|_| {});
    let id = books(&window);
    at(&mut window, id, flow(token()));
    let changed = terms::LinkError::KeyChanged { trusted: key(FIRST_KEY), presented: key(NEW_KEY) };
    at(&mut window, id, flow(Report::Unreached(changed)));
    window.update(WindowMessage::Open(id), moment());
    window
}

fn forgot() -> Window {
    let mut window = books_ended(None);
    let id = books(&window);
    at(&mut window, id, PlaceMessage::Session(screens::SessionMessage::Door(Message::Forgot)));
    window
}

fn down() -> Window {
    on_books(|window| {
        let id = books(window);
        at(window, id, flow(token()));
        at(window, id, flow(Report::Unreached(terms::LinkError::Timeout)));
    })
}

fn connecting() -> Window {
    on_books(|_| {})
}

fn new_password() -> Window {
    on_books(|window| {
        let id = books(window);
        at(window, id, flow(token()));
        at(window, id, flow(Report::Expired(None)));
        at(
            window,
            id,
            PlaceMessage::Session(screens::SessionMessage::Door(Message::Password(Secret::new(
                "river-copper-lamp-42".into(),
            )))),
        );
        at(window, id, PlaceMessage::Session(screens::SessionMessage::Door(Message::Submit)));
        let standing = Standing { must_change: true, ..screens::harness::standing_as("ada") };
        at(window, id, flow(Report::Accepted { token: None, named: None, standing }));
    })
}

/// Night Owls in a call in Lounge.
fn in_call(window: &mut Window) {
    let owls = window.places()[0].id();
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Join(screens::harness::channel_id("lounge"))));
    let mut members = vec![Person::new(screens::harness::member(ME), ADA)];
    members
        .extend(["grace", "Bo", "mira"].map(|name| Person::new(screens::harness::member(&name.to_lowercase()), name)));
    let streams = members.iter().map(|person| (person.user_id.clone(), Track::Microphone)).collect();
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Joined { members, streams })));
    at(window, owls, lanes(screens::Lane::Up, screens::Lane::Up));
    // grace is speaking, Bo has muted and mira has deafened, as the canvas has it.
    let state = |user: &str, mic_muted, deafened| {
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::State {
            user_id: screens::harness::member(user),
            state: choices::State { mic_muted, deafened },
        }))
    };
    at(window, owls, state("bo", true, false));
    at(window, owls, state("mira", true, true));
    at(window, owls, levels(false));
}

/// Who is heard in Lounge: grace, and this account only when `you` speak, which the canvas shows in `voice.grid` alone.
fn levels(you: bool) -> PlaceMessage {
    let mut heard = vec![(screens::harness::member("grace"), 90)];
    if you {
        heard.push((screens::harness::member(ME), 90));
    }
    PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Levels(heard)))
}

/// Night Owls in the call in Lounge with this account speaking too, as the canvas's `voice.grid` has it.
fn in_lounge_speaking(window: &mut Window) {
    in_lounge(window);
    let owls = window.places()[0].id();
    at(window, owls, levels(true));
}

/// How this account's two lanes of the call are.
fn lanes(send: screens::Lane, receive: screens::Lane) -> PlaceMessage {
    PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Lanes { send, receive }))
}

/// How this account's take of `user`'s voice stands.
fn taking(user: &str, take: screens::Take) -> PlaceMessage {
    PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Taking {
        user_id: screens::harness::member(user),
        track: Track::Microphone,
        take,
    }))
}

/// The call after `then` happened to it, Night Owls open.
fn voice(then: fn(&mut Window, PlaceId)) -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    then(&mut window, owls);
    window
}

/// Night Owls in Lounge with an account that may share, and `then` done: grace shares her screen, as the canvas has it.
fn sharer(then: fn(&mut Window, PlaceId)) -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    let grants = proto::rules::Grants::from_words(&["SHARE_SCREEN"]);
    now_holding(&mut window, owls, proto::rules::Place::BOTTOM, grants);
    window.update(WindowMessage::Encoders(encoders()), moment());
    window.update(WindowMessage::Open(owls), moment());
    then(&mut window, owls);
    window
}

/// Night Owls in Lounge with an account that may share and send its camera, and `then` done.
fn with_camera(then: fn(&mut Window, PlaceId)) -> Window {
    let mut window = sharer(|_, _| {});
    let owls = window.places()[0].id();
    let grants = proto::rules::Grants::from_words(&["SHARE_SCREEN", "SHARE_CAMERA"]);
    now_holding(&mut window, owls, proto::rules::Place::BOTTOM, grants);
    then(&mut window, owls);
    window
}

/// This account's own camera and Bo's on, as the canvas has them: each fills its card, one's own first.
fn cameras_on(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleCamera));
    let encoder = screens::Route::Hardware { via: "AMF".into() };
    let started = screens::CameraSent::Started { width: 1280, height: 720, encoder };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Camera(started)));
    for user_id in [ME, "bo"] {
        let started = RoomEvent::Started { user_id: screens::harness::member(user_id), track: Track::Camera };
        at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(started)));
    }
}

/// Both cameras on, and the popover about this account's own open, its picture held back for the link when `limited`.
fn camera_about(window: &mut Window, owls: PlaceId, limited: bool) {
    cameras_on(window, owls);
    let looked = screens::Outgoing { width: 1280, height: 720, frame_rate: Some(30), ..outgoing(limited) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Camera(screens::CameraSent::Looked(looked))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnCameraInfo));
}

/// Bo's camera on, which this device cannot show: his card stays his tile.
fn camera_unshown(window: &mut Window, owls: PlaceId) {
    let started = RoomEvent::Started { user_id: screens::harness::member("bo"), track: Track::Camera };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(started)));
    let failed = screens::Watched::Failed {
        user_id: screens::harness::member("bo"),
        video: proto::video::Video::Camera,
        failed: words::Failure::CannotDecode,
    };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(failed)));
}

/// This account's camera stopped by itself: the camera was unplugged.
fn camera_stopped(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleCamera));
    let failed = screens::CameraSent::Failed(words::Failure::CameraGone);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Camera(failed)));
}

/// The account's roles lose `SHARE_CAMERA` while its camera is on: the server stops only the camera.
fn camera_denied(window: &mut Window, owls: PlaceId) {
    cameras_on(window, owls);
    let grants = proto::rules::Grants::from_words(&["SHARE_SCREEN"]);
    now_holding(window, owls, proto::rules::Place::BOTTOM, grants);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Camera(screens::CameraSent::Denied)));
    let stopped = RoomEvent::Stopped { user_id: screens::harness::member(ME), track: Track::Camera };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(stopped)));
}

/// The canvas's device: H.264 and AV1 on its graphics card.
fn encoders() -> Vec<choices::EncoderOption> {
    [choices::Codec::H264, choices::Codec::Av1]
        .map(|codec| choices::EncoderOption { codec, on: choices::EncodesOn::GraphicsCard })
        .into()
}

fn someone_shares(window: &mut Window, owls: PlaceId) {
    for track in [Track::ScreenVideo, Track::ScreenAudio] {
        at(
            window,
            owls,
            PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Started {
                user_id: screens::harness::member("grace"),
                track,
            })),
        );
    }
}

/// This account's own screen going out, at the canvas's size.
fn sharing(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Starting));
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Started {
            width: 1920,
            height: 1080,
            encoder: screens::Route::Hardware { via: "VA-API".into() },
        })),
    );
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Started {
            user_id: screens::harness::member(ME),
            track: Track::ScreenVideo,
        })),
    );
    // The canvas shares Brave's window, whose sound goes along.
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Sound(SoundSaid::App(
            "Brave Web Browser".into(),
        )))),
    );
}

/// The share panel with both sound switches off.
fn share_menu_silent(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleShare));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Quality(choices::QualityPick::ScreenSound(false))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Quality(choices::QualityPick::WindowSound(false))));
}

/// A device whose graphics card makes no AV1, so its processor does (SVT-AV1), with AV1 chosen.
fn share_menu_processor(window: &mut Window, owls: PlaceId) {
    let mut encoders = encoders();
    encoders.retain(|encoder| encoder.codec != choices::Codec::Av1);
    encoders.push(choices::EncoderOption { codec: choices::Codec::Av1, on: choices::EncodesOn::Processor });
    window.update(WindowMessage::Encoders(encoders), moment());
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleShare));
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Quality(choices::QualityPick::Codec(choices::Codec::Av1))),
    );
}

/// A device whose graphics card's encoders nobody has tested with Chatkord (NVENC, Quick Sync), H.264 its default.
fn share_menu_untested(window: &mut Window, owls: PlaceId) {
    let encoders = [choices::Codec::H264, choices::Codec::Av1]
        .map(|codec| choices::EncoderOption { codec, on: choices::EncodesOn::UntestedCard })
        .into();
    window.update(WindowMessage::Encoders(encoders), moment());
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleShare));
}

/// Our own picker (Windows) open on `tab`, listing what the canvas lists, with `selected` selected.
fn picking(window: &mut Window, owls: PlaceId, tab: screens::SourcesTab, selected: Option<&str>) {
    use screens::{Painting, Source, SourceKey, SourceKind};
    window.update(WindowMessage::Chooser(screens::Chooser::Own), moment());
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleShare));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ChooseScreen));
    let screen = |key: &str, size, still| Source {
        key: SourceKey(key.into()),
        kind: SourceKind::Screen,
        title: None,
        size: Some(size),
        icon: None,
        still: Some(Painting(still)),
    };
    let app = |key: &str, title: &str, at: u64| Source {
        key: SourceKey(key.into()),
        kind: SourceKind::Application,
        title: Some(title.into()),
        size: None,
        icon: Some(Painting(100 + at)),
        still: Some(Painting(200 + at)),
    };
    let listed = vec![
        screen("m1", (3840, 2160), 1),
        screen("m2", (1080, 1920), 2),
        app("w1", "Plant swap wiki – Browser", 1),
        app("w2", "Recipes for Saturday – Notes", 2),
        app("w3", "poster.png – Photo editor", 3),
        app("w4", "Terminal", 4),
        app("w5", "Main menu – Game", 5),
    ];
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Sources(listed)));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::SourcesTab(tab)));
    if let Some(key) = selected {
        at(window, owls, PlaceMessage::Call(screens::CallMessage::SelectSource(SourceKey(key.into()))));
    }
}

/// One's own screen's popover, its sound as `said`.
fn own_about_sound(window: &mut Window, owls: PlaceId, said: SoundSaid) {
    own_about(window, owls, false);
    let unfound = said == SoundSaid::NotFound;
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Sound(said))));
    if unfound {
        at(window, owls, PlaceMessage::Call(screens::CallMessage::SoundApps(sound_apps())));
    }
}

/// The canvas's applications playing sound.
fn sound_apps() -> Vec<screens::SoundApp> {
    ["Firefox", "Spotify", "Steam"].map(|name| screens::SoundApp { name: name.into(), binary: None }).to_vec()
}

/// One's own screen pinned with its popover open on a window whose app was not found, the apps listed; `picked`
/// names the one picked.
fn own_sound_picking(window: &mut Window, owls: PlaceId, picked: Option<&str>) {
    own_pinned(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(outgoing(false)))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
    let unfound = screens::Shared::Sound(SoundSaid::NotFound);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(unfound)));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::SoundApps(sound_apps())));
    if let Some(name) = picked {
        let said = screens::Shared::Sound(SoundSaid::App(name.into()));
        at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(said)));
    }
}

/// The screen picked, its first picture awaited.
fn share_starting(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Starting));
}

/// The screen goes out, and this device cannot show it on its card.
fn sharing_unshown(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Unshown)));
}

fn share_failed(window: &mut Window, owls: PlaceId) {
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Failed(words::Failure::ScreenSilent))),
    );
}

/// The account's roles lose `SHARE_SCREEN` while it shares: the server stops only the share.
fn share_denied(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    let grants = proto::rules::Grants::from_words::<&str>(&[]);
    now_holding(window, owls, proto::rules::Place::BOTTOM, grants);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Denied)));
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Stopped {
            user_id: screens::harness::member(ME),
            track: Track::ScreenVideo,
        })),
    );
}

fn no_encoder() -> Window {
    let mut window = sharer(|_, _| {});
    window.update(WindowMessage::Encoders(Vec::new()), moment());
    window
}

/// grace's screen playing in its card in the grid, from its start.
fn watching_in_card(window: &mut Window, owls: PlaceId) {
    someone_shares(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(watched_card(
            screens::ShareState::Live,
        )))),
    );
}

/// grace's screen playing, its sound refused by a server whose budget is spent.
fn sound_not_carried(window: &mut Window, owls: PlaceId) {
    watching_in_card(window, owls);
    let refused = Some(words::Failure::ShareSoundNotCarried);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Sound {
            user_id: screens::harness::member("grace"),
            refused,
        })),
    );
}

/// grace's screen, which this device cannot decode.
fn unshown(window: &mut Window, owls: PlaceId) {
    someone_shares(window, owls);
    let failed = words::Failure::CannotDecode;
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Failed {
            user_id: screens::harness::member("grace"),
            video: proto::video::Video::Screen,
            failed,
        })),
    );
}

/// grace and Bo both sharing, both playing.
fn two_share(window: &mut Window, owls: PlaceId) {
    watching_in_card(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Started {
            user_id: screens::harness::member("bo"),
            track: Track::ScreenVideo,
        })),
    );
    let bo = screens::ShareCard { who: screens::harness::member("bo"), ..watched_card(screens::ShareState::Live) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(bo))));
}

/// This account's own screen going out while grace's plays: its card comes second, after its own tile.
fn sharing_and_watching(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    watching_in_card(window, owls);
}

fn pin(window: &mut Window, owls: PlaceId, card: screens::Pinned) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Pin(card)));
}

/// This account's own screen going out, pinned.
fn own_pinned(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    pin(window, owls, screens::Pinned::OwnScreen);
}

/// Where the pointer rests in a state whose card shows its controls, as the canvas's `hover` puts it.
fn pointer(state: &str) -> Option<iced_core::Point> {
    let at = |x, y| Some(iced_core::Point::new(x, y));
    match state {
        // Bo's tile, bottom left of four.
        "voice.card.hover" | "voice.camera.hover" | "voice.camera.paused" | "voice.camera.out" => at(500.0, 550.0),
        // grace's screen, the third card of five: top right.
        "voice.watch.card" | "voice.watch.nosound" | "voice.watch.info.software" => at(900.0, 250.0),
        // Our own screen, the second card: top middle.
        "voice.sharing.own"
        | "voice.share.info"
        | "voice.share.info.screen"
        | "voice.share.info.nosound"
        | "voice.share.info.held"
        | "voice.share.info.scaled"
        | "voice.share.info.ceiling"
        | "voice.share.info.busy"
        | "voice.share.info.slow"
        | "voice.share.info.untested"
        | "voice.share.info.software"
        | "voice.share.info.unwatched" => at(800.0, 250.0),
        // The pinned card on the stage.
        "voice.watch"
        | "voice.watch.out"
        | "voice.pin.person"
        | "voice.pin.own"
        | "app.share.sound.pick"
        | "app.share.sound.picked"
        | "voice.watch.reconnecting"
        | "voice.watch.paused"
        | "voice.watch.zoom" => at(800.0, 300.0),
        // The middle of a screen's own window.
        "voice.popout" => at(whole_size().width / 2.0, whole_size().height / 2.0),
        // The splitter, on the rule after the rail and the pane.
        "app.pane.handle" => at(design::RAIL_WIDTH + design::BORDER_WIDTH + design::PANE_LEAST + 0.5, 400.0),
        // The stage, away from the pinned card's bar: the strip's toggle shows.
        "voice.watch.strip" => at(800.0, 450.0),
        // The floating card, by the bottom right corner.
        "voice.mini" | "voice.mini.own" => at(1100.0, 650.0),
        // #ideas in the side pane, where its gear shows.
        "app.channels.hover" => at(180.0, 181.0),
        // Reply in the bar over Bo's edited message, the second of his run.
        "chat.hover" => at(956.0, 345.0),
        // The splitter on the members column's edge.
        "chat.members.handle" => at(whole_size().width - design::MEMBERS_WIDTH - design::BORDER_WIDTH + 0.5, 400.0),
        _ => None,
    }
}

/// grace's screen pinned: it fills the stage.
fn watching(window: &mut Window, owls: PlaceId) {
    watching_in_card(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Pin(screens::Pinned::Screen(screens::harness::member("grace")))),
    );
}

/// grace's screen pinned, then in a window of its own.
fn watching_out(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::PopOut(
            screens::harness::member("grace"),
            proto::video::Video::Screen,
        )),
    );
}

/// Bo's camera in a window of its own: its card says so.
fn camera_out(window: &mut Window, owls: PlaceId) {
    cameras_on(window, owls);
    let out = screens::CallMessage::PopOut(screens::harness::member("bo"), proto::video::Video::Camera);
    at(window, owls, PlaceMessage::Call(out));
}

/// grace's screen pinned with Appearance open over the call: it floats.
fn floating(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), moment());
}

/// grace's screen pinned with the strip put away: it takes the whole stage.
fn watching_without_strip(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleStrip(screens::Stage::Tall)));
}

/// grace's screen pinned and zoomed to 250 %, its centre where the canvas has it: 62 % across, 40 % down.
fn watching_zoomed(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleZoom(screens::harness::member("grace"))));
    // From the whole picture, a zoom to 2.5 toward a point 70 % across and a third down centres it there.
    let wheel = screens::CallMessage::ZoomWheel { notches: 15, at: (560.0, 150.0), area: (800.0, 450.0) };
    at(window, owls, PlaceMessage::Call(wheel));
}

/// grace's screen over the whole display, as its Full screen button leaves it.
fn full_screen(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::FullScreen(screens::harness::member("grace"))));
}

/// The same once the pointer rested: only the picture.
fn full_screen_calm(window: &mut Window, owls: PlaceId) {
    full_screen(window, owls);
    let later = screens::Moment { now: moment().now + design::FULL_SCREEN_CALM * 2, ..moment() };
    window.update(WindowMessage::Tick, later);
}

fn watching_about(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleInfo(screens::harness::member("grace"))));
}

/// grace's screen pinned and paused for this viewer alone: its last picture under the veil.
fn watching_paused(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    let paused = screens::Pinned::Screen(screens::harness::member("grace"));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::TogglePause(paused)));
}

/// grace's screen in its card with what it is open, decoded by the processor.
fn watching_software_about(window: &mut Window, owls: PlaceId) {
    watching_in_card(window, owls);
    let card = screens::ShareCard { decoder: screens::Route::Software, ..watched_card(screens::ShareState::Live) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleInfo(screens::harness::member("grace"))));
}

/// Bo's camera on and shown, his card under the pointer.
fn camera_hover(window: &mut Window, owls: PlaceId) {
    let started = RoomEvent::Started { user_id: screens::harness::member("bo"), track: Track::Camera };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(started)));
}

/// Bo's camera paused for this viewer alone: its last picture under the veil.
fn camera_paused(window: &mut Window, owls: PlaceId) {
    camera_hover(window, owls);
    let paused = screens::Pinned::Person(screens::harness::member("bo"));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::TogglePause(paused)));
}

/// grace's screen pinned with what it is open, after 37 pictures came faster than they were painted.
fn watching_unpainted(window: &mut Window, owls: PlaceId) {
    watching_about(window, owls);
    let card = screens::ShareCard { dropped: 37, ..watched_card(screens::ShareState::Live) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
}

/// grace's stream in its card, which this computer cannot keep up with.
fn watching_behind(window: &mut Window, owls: PlaceId) {
    someone_shares(window, owls);
    let card = watched_card(screens::ShareState::Behind);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
}

/// The same pinned with what it is open.
fn watching_behind_about(window: &mut Window, owls: PlaceId) {
    watching_about(window, owls);
    let card = watched_card(screens::ShareState::Behind);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
}

/// grace's screen pinned with what it is open, after it froze twice and three frames were not decoded.
fn watching_froze(window: &mut Window, owls: PlaceId) {
    watching_about(window, owls);
    let card = screens::ShareCard {
        refused: 3,
        freezes: (2, std::time::Duration::from_millis(900)),
        ..watched_card(screens::ShareState::Live)
    };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
}

/// The popover about this account's own screen, held back for the link when `limited`.
fn own_about(window: &mut Window, owls: PlaceId, limited: bool) {
    sharing(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(outgoing(limited)))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
}

/// This account's screen encoded by the processor, its popover open.
fn own_about_software(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    let software = screens::Outgoing { encoder: screens::Route::Software, ..outgoing(false) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(software))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
}

/// This account's screen while nobody watches it: nothing is sent, so the size has no frame rate.
fn unwatched() -> screens::Outgoing {
    screens::Outgoing { frame_rate: None, bits_per_second: 0, ..outgoing(false) }
}

/// The server's word that nobody takes this account's screen.
fn nobody_watches(window: &mut Window, owls: PlaceId) {
    let told =
        screens::CallMessage::OwnWatched { video: proto::video::Video::Screen, watched: screens::OwnWatch::Unwatched };
    at(window, owls, PlaceMessage::Call(told));
}

/// The popover about this account's screen while nobody watches it.
fn own_about_unwatched(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(unwatched()))));
    nobody_watches(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
}

/// The connection popover while this account shares and nobody watches: nothing goes out.
fn unwatched_stats(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(unwatched()))));
    nobody_watches(window, owls);
    looks(window, owls, with_share(good(), 48_000, 903_512, 1_200_000_000));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Stats));
}

/// This account's screen sent at 720p, as the canvas has it: 1920 × 1080 scaled down.
fn own_about_scaled(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    let scaled = screens::Outgoing { width: 1280, height: 720, scaled_from: Some((1920, 1080)), ..outgoing(false) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(scaled))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
}

/// This account's screen, `reduced` from what was chosen, as the canvas has it, its popover open.
fn own_about_reduced(window: &mut Window, owls: PlaceId, sent: screens::Outgoing, reduced: screens::Reduced) {
    sharing(window, owls);
    let looked = screens::Outgoing { reduced: Some(reduced), ..sent };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(looked))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
}

/// A 4K screen started at 15 a second to fit a ceiling of 4 Mbit/s.
fn own_about_ceiling(window: &mut Window, owls: PlaceId) {
    let sent = screens::Outgoing {
        width: 3840,
        height: 2160,
        frame_rate: Some(15),
        bits_per_second: 4_000_000,
        ..outgoing(false)
    };
    let cause = screens::Reduction::Ceiling { bits_per_second: 4_000_000 };
    own_about_reduced(window, owls, sent, screens::Reduced { from_size: None, from_frame_rate: 60, cause });
}

/// A busy screen sent at 30 a second though it was chosen at 60, under a rate of 8 Mbit/s.
fn own_about_busy(window: &mut Window, owls: PlaceId) {
    let sent = screens::Outgoing { frame_rate: Some(30), ..outgoing(false) };
    let cause = screens::Reduction::Busy { ceiling: Some(8_000_000) };
    own_about_reduced(window, owls, sent, screens::Reduced { from_size: None, from_frame_rate: 60, cause });
}

/// A screen sent at 30 a second though it was chosen at 60, because the encoder could not keep up.
fn own_about_slow(window: &mut Window, owls: PlaceId) {
    let sent = screens::Outgoing { frame_rate: Some(30), ..outgoing(false) };
    let cause = screens::Reduction::Encoder;
    own_about_reduced(window, owls, sent, screens::Reduced { from_size: None, from_frame_rate: 60, cause });
}

/// This account's screen sent through an encoder nobody has tested with Chatkord.
fn own_about_untested(window: &mut Window, owls: PlaceId) {
    sharing(window, owls);
    let untested =
        screens::Outgoing { encoder: screens::Route::UntestedHardware { via: "D3D11".into() }, ..outgoing(false) };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(untested))));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleOwnInfo));
}

fn watch_reconnecting(window: &mut Window, owls: PlaceId) {
    someone_shares(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Pin(screens::Pinned::Screen(screens::harness::member("grace")))),
    );
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(watched_card(
            screens::ShareState::Reconnecting,
        )))),
    );
}

fn watched_card(state: screens::ShareState) -> screens::ShareCard {
    screens::ShareCard {
        who: screens::harness::member("grace"),
        width: 1920,
        height: 1080,
        frame_rate: Some(60),
        dropped: 0,
        codec: "H.264".into(),
        decoder: screens::Route::Hardware { via: "VA-API".into() },
        refused: 0,
        freezes: (0, std::time::Duration::ZERO),
        state,
    }
}

/// The connection popover while this account shares, as the canvas has it, held back for the link when `limited`.
fn shared_stats(window: &mut Window, owls: PlaceId, health: screens::Health, limited: bool) {
    sharing(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Shared(screens::Shared::Looked(outgoing(limited)))));
    looks(window, owls, health);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Stats));
}

/// The connection popover while this account's camera alone is on, encoded by `encoder`, as the canvas has it.
fn camera_stats(window: &mut Window, owls: PlaceId, encoder: screens::Route, limited: bool) {
    own_camera_looked(window, owls, encoder, limited);
    looks(window, owls, good());
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Stats));
}

/// This account's camera on and looked at once, encoded by `encoder`, at the canvas's figures.
fn own_camera_looked(window: &mut Window, owls: PlaceId, encoder: screens::Route, limited: bool) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleCamera));
    let started = screens::CameraSent::Started { width: 1280, height: 720, encoder: encoder.clone() };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Camera(started)));
    let mine = RoomEvent::Started { user_id: screens::harness::member(ME), track: Track::Camera };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(mine)));
    let looked = screens::Outgoing {
        codec: "H.264".into(),
        encoder,
        width: 1280,
        height: 720,
        scaled_from: None,
        frame_rate: Some(30),
        bits_per_second: 1_500_000,
        frames: 54_210,
        key_frames_made: 1,
        key_frames_asked: 0,
        resent_bytes: 0,
        restarts: 0,
        limited,
        reduced: None,
    };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Camera(screens::CameraSent::Looked(looked))));
}

/// `health` with a share going out: Sending and the call's upload count it, as the canvas has them.
fn with_share(health: screens::Health, bits_per_second: u64, packets: u64, up_bytes: u64) -> screens::Health {
    screens::Health { sending: screens::Flow { packets, bits_per_second }, up_bytes, ..health }
}

/// This account's share as the canvas has it: held back, a quarter of the rate goes out.
fn outgoing(limited: bool) -> screens::Outgoing {
    screens::Outgoing {
        codec: "H.264".into(),
        encoder: screens::Route::Hardware { via: "VA-API".into() },
        width: 1920,
        height: 1080,
        scaled_from: None,
        frame_rate: Some(60),
        bits_per_second: if limited { 2_000_000 } else { 8_000_000 },
        frames: 212_604,
        key_frames_made: if limited { 9 } else { 2 },
        key_frames_asked: if limited { 8 } else { 1 },
        resent_bytes: if limited { 1_800_000 } else { 12_400 },
        restarts: if limited { 4 } else { 1 },
        limited,
        reduced: None,
    }
}

/// An administrator's menu on grace: what the canvas's person popover holds under "As an admin".
/// A member's popover on grace: her volume on this device, and nothing an administrator does.
fn volume(window: &mut Window, owls: PlaceId) {
    people(window, owls, &[ADMIN_ROLE, HOSTS_ROLE, HELPERS_ROLE], false);
    window.update(WindowMessage::Person(owls, screens::harness::member("grace"), screens::PersonFrom::Card), moment());
}

/// grace's voice muted for this device alone, her popover open.
fn volume_muted(window: &mut Window, owls: PlaceId) {
    volume(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::MuteForMe(screens::harness::member("grace"), choices::Audio::Voice)),
    );
}

/// grace's screen pinned, the popover of its sound open.
fn watching_volume(window: &mut Window, owls: PlaceId) {
    watching(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ShareSound(screens::harness::member("grace"))));
}

/// grace's screen shared without sound: its card has no volume.
fn watching_no_sound(window: &mut Window, owls: PlaceId) {
    watching_in_card(window, owls);
    let stopped = RoomEvent::Stopped { user_id: screens::harness::member("grace"), track: Track::ScreenAudio };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(stopped)));
}

/// Audio controls open while grace shares: Bo at 40 % and the sound of grace's screen muted here.
fn audio_controls(window: &mut Window, owls: PlaceId) {
    watching_in_card(window, owls);
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::KeepVolume(screens::harness::member("bo"), choices::Audio::Voice, 40)),
    );
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::MuteForMe(screens::harness::member("grace"), choices::Audio::Share)),
    );
    at(window, owls, PlaceMessage::Call(screens::CallMessage::AudioControls));
}

/// Voice options open while grace and this account share.
fn view_menu(window: &mut Window, owls: PlaceId) {
    sharing_and_watching(window, owls);
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ViewMenu));
}

/// Both of Voice options' hides on while grace and this account share: only grace's screen.
fn view_hidden(window: &mut Window, owls: PlaceId) {
    sharing_and_watching(window, owls);
    for pick in [choices::ViewPick::People, choices::ViewPick::OwnShare] {
        window.update(WindowMessage::Look(choices::LookPick::View(pick)), moment());
    }
}

/// Always show voice controls off while grace and this account share, the pointer off the call: no bar.
fn view_controls(window: &mut Window, owls: PlaceId) {
    sharing_and_watching(window, owls);
    window.update(WindowMessage::Look(choices::LookPick::View(choices::ViewPick::Controls)), moment());
}

/// Audio controls open with no one else in the call.
fn audio_controls_alone() -> Window {
    let mut window = alone();
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Call(screens::CallMessage::AudioControls));
    window
}

fn mod_menu(window: &mut Window, owls: PlaceId) {
    let seated: Vec<screens::Occupant> = ["grace", "bo", "mira"]
        .map(|user| screens::Occupant {
            user_id: screens::harness::member(user),
            colour: None,
            name: user.into(),
            state: choices::State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        })
        .into_iter()
        .chain([screens::Occupant {
            user_id: screens::harness::member(ME),
            colour: None,
            name: ADA.into(),
            state: choices::State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: screens::harness::ADMIN_PLACE,
        }])
        .collect();
    now_holding(window, owls, screens::harness::ADMIN_PLACE, every_word());
    people(window, owls, &[MODS_ROLE, HOSTS_ROLE, HELPERS_ROLE], false);
    // Becoming an administrator says so in a toast; opening the server puts it away.
    window.update(WindowMessage::Open(owls), moment());
    at(
        window,
        owls,
        PlaceMessage::Lobby(screens::LobbyMessage::Occupancy {
            channel_id: screens::harness::channel_id("lounge"),
            people: seated,
        }),
    );
    window.update(WindowMessage::Person(owls, screens::harness::member("grace"), screens::PersonFrom::Card), moment());
}

/// This account's send connection lost its route and is being made again.
fn mending(window: &mut Window, owls: PlaceId) {
    at(window, owls, lanes(screens::Lane::Mending, screens::Lane::Up));
}

/// The connection everyone is heard over lost its route and is being made again: each voice taken waits for it.
fn others_mending(window: &mut Window, owls: PlaceId) {
    for user in ["grace", "bo", "mira"] {
        at(window, owls, taking(user, screens::Take::Taken));
    }
    at(window, owls, lanes(screens::Lane::Up, screens::Lane::Mending));
}

/// Just joined: nothing was sent over this account's send connection yet.
fn own_connecting(window: &mut Window, owls: PlaceId) {
    at(window, owls, lanes(screens::Lane::Starting, screens::Lane::Up));
}

/// Bo's voice was asked for longer ago than a moment and is not taken yet; the room looks again at its next levels.
fn other_connecting(window: &mut Window, owls: PlaceId) {
    at(window, owls, taking("bo", screens::Take::Asked));
    let later = Moment { now: Instant::now() + design::CONNECTING_AFTER * 2, ..moment() };
    window.update(WindowMessage::At(owls, levels(false)), later);
}

/// Bo's voice was refused to this account.
fn other_failed(window: &mut Window, owls: PlaceId) {
    at(window, owls, taking("bo", screens::Take::Refused));
}

/// An administrator muted this account for everyone, its own mute on: the microphone on the danger ground and the
/// stage saying it stays muted once the hold is lifted.
fn mod_muted(window: &mut Window, owls: PlaceId) {
    held_by_admin(window, owls, true);
}

/// The same with its own mute off: the stage says the microphone is on once the hold is lifted.
fn mod_muted_own(window: &mut Window, owls: PlaceId) {
    held_by_admin(window, owls, false);
}

/// An administrator muted this account for everyone, its own mute `muted`.
fn held_by_admin(window: &mut Window, owls: PlaceId, muted: bool) {
    at(window, owls, mute_state(muted, false));
    at(
        window,
        owls,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Held {
            user_id: screens::harness::member(ME),
            held: true,
        })),
    );
}

/// The share panel open, its bitrate Custom and `typed` in its field.
fn share_custom(window: &mut Window, owls: PlaceId, typed: &str) {
    at(window, owls, PlaceMessage::Call(screens::CallMessage::ToggleShare));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Rate(screens::RateMessage::Custom)));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Rate(screens::RateMessage::Typed(typed.into()))));
}

/// An administrator took this account out of Lounge.
fn mod_removed(window: &mut Window, owls: PlaceId) {
    let end = screens::End::Closed(Some(proto::chatkord_v1::CloseReason::TakenOut));
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Ended(end))));
}

/// Taken out of Lounge while Appearance was open over the call: the page says so.
fn taken_under_a_page(window: &mut Window, owls: PlaceId) {
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), moment());
    mod_removed(window, owls);
}

/// Book club open while the call is in Lounge on Night Owls: its picture carries the call's mark.
fn books_opened(window: &mut Window, _: PlaceId) {
    let books = window.places()[1].id();
    window.update(WindowMessage::Open(books), moment());
}

/// Book club open while the call on Night Owls is taken out: the notice on Book club names Night Owls.
fn taken_elsewhere(window: &mut Window, owls: PlaceId) {
    books_opened(window, owls);
    mod_removed(window, owls);
}

/// Only this account in the call, in Lounge.
fn mute_state(mic_muted: bool, deafened: bool) -> PlaceMessage {
    PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::State {
        user_id: screens::harness::member(ME),
        state: choices::State { mic_muted, deafened },
    }))
}

/// Alone in Quiet corner, holding what a member holds.
fn alone() -> Window {
    let mut window = both(books_signed_in);
    let owls = window.places()[0].id();
    now_holding(&mut window, owls, proto::rules::Place::BOTTOM, member_words());
    at(&mut window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Join(screens::harness::channel_id("corner"))));
    let joined = RoomEvent::Joined {
        members: vec![Person::new(screens::harness::member(ME), ADA)],
        streams: vec![(screens::harness::member(ME), Track::Microphone)],
    };
    at(&mut window, owls, PlaceMessage::Call(screens::CallMessage::Room(joined)));
    at(&mut window, owls, lanes(screens::Lane::Up, screens::Lane::Up));
    window
}

/// This account listed in Lounge while another device holds the seat.
fn other_device() -> Window {
    let mut window = both(books_signed_in);
    let owls = window.places()[0].id();
    let people = vec![screens::Occupant {
        user_id: screens::harness::member(ME),
        colour: None,
        name: ADA.into(),
        state: choices::State::default(),
        sharing: false,
        camera: false,
        held: false,
        place: proto::rules::Place::BOTTOM,
    }];
    at(
        &mut window,
        owls,
        PlaceMessage::Lobby(screens::LobbyMessage::Occupancy {
            channel_id: screens::harness::channel_id("lounge"),
            people,
        }),
    );
    window
}

/// The call's devices as the canvas lists them, `microphone` in use or else the default; the menu open when `open`,
/// and a notice told when there is one.
fn devices(microphone: Option<&str>, open: bool, told: Option<Sentence>) -> Window {
    let listed =
        |asked: Option<&str>, label: &str| screens::Listed { asked: asked.map(str::to_string), label: label.into() };
    let sound = screens::Sound {
        microphones: vec![
            listed(None, BUILT_IN),
            listed(Some(HEADSET_MIC), HEADSET_MIC),
            listed(Some(BUILT_IN), BUILT_IN),
        ],
        speakers: vec![
            listed(None, BUILT_IN),
            listed(Some(HEADSET_SPEAKER), HEADSET_SPEAKER),
            listed(Some(BUILT_IN), BUILT_IN),
        ],
        microphone: microphone.map(str::to_string),
        speaker: Some(HEADSET_SPEAKER.into()),
    };
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Call(screens::CallMessage::Sound(sound)));
    if open {
        at(&mut window, owls, PlaceMessage::Call(screens::CallMessage::Devices));
    }
    if let Some(told) = told {
        at(&mut window, owls, PlaceMessage::Call(screens::CallMessage::Notice(screens::Subject::Microphone, told)));
    }
    window
}

/// The devices the canvas names: what the system has built in, and the headset.
const BUILT_IN: &str = "Built-in Audio Analog Stereo";
const HEADSET_MIC: &str = "USB Headset Mono";
const HEADSET_SPEAKER: &str = "USB Headset Analog Stereo";
/// A microphone the canvas's voice settings remember and the computer does not have now.
const DESK_MIC: &str = "USB Desk Microphone";

/// Appearance, on a system whose languages are the canvas's: English, which Match system names.
fn appearance() -> Window {
    let mut window = both(in_lounge);
    let english: Vec<words::LanguageIdentifier> = "en-US".parse().into_iter().collect();
    window.update(WindowMessage::SystemLanguages(english), moment());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), moment());
    window
}

/// Appearance with the Language field's menu open.
fn appearance_language() -> Window {
    let mut window = appearance();
    window.update(WindowMessage::LanguageMenu, moment());
    window
}

/// Appearance with messages at 18 px and 24 px between runs, as the canvas draws its preview larger.
fn appearance_messages() -> Window {
    let mut window = appearance();
    let size = design::MessageSize::of_px(18).expect("a step");
    let gap = design::RunGap::of_px(24).expect("a step");
    window.update(WindowMessage::Look(choices::LookPick::MessageSize(size)), moment());
    window.update(WindowMessage::Look(choices::LookPick::RunGap(gap)), moment());
    window
}

/// Book club signed out, its door open, and the rail's gear pressed there: this device's first page, Night Owls in its
/// call meanwhile.
fn door_devices() -> Window {
    let mut window = both(in_call);
    let id = books(&window);
    at(&mut window, id, flow(token()));
    at(&mut window, id, flow(Report::Expired(None)));
    window.update(WindowMessage::Open(id), moment());
    window.update(WindowMessage::OpenSettings, moment());
    window
}

/// Both servers remembered and neither signed in, Night Owls's session ended and Book club not answering, and the
/// rail's gear pressed: this device's first page alone.
fn settings_signed_out() -> Window {
    let rows = vec![Opening::Account(night_owls(), ADA.into()), Opening::Account(book_club(), ADA.into())];
    let window = Window::launch(rows, None, Instant::now()).0;
    let linux = choices::Remembered { startup: Some(choices::Startup::default()), ..choices::Remembered::default() };
    let mut window = window.remembering(linux, std::collections::BTreeMap::new());
    let (owls, club) = (window.places()[0].id(), books(&window));
    at(&mut window, owls, flow(token()));
    at(&mut window, owls, flow(Report::Expired(None)));
    at(&mut window, club, flow(token()));
    at(&mut window, club, flow(Report::Unreached(terms::LinkError::Timeout)));
    window.update(WindowMessage::Open(owls), moment());
    window.update(WindowMessage::OpenSettings, moment());
    window
}

/// `window` with the client's debug report asked for: its dialog, or, once `saved`, the page naming the file.
fn reported(mut window: Window, saved: bool) -> Window {
    use screens::ReportMessage;
    window.update(WindowMessage::Machine(screens::harness::machine()), moment());
    window.update(WindowMessage::Report(ReportMessage::Open), moment());
    window.update(WindowMessage::NoticesRead(Some(screens::harness::notices())), moment());
    window.update(WindowMessage::Report(ReportMessage::Read(screens::ReportLogs::default())), moment());
    if saved {
        window.update(WindowMessage::Report(ReportMessage::Download), moment());
        let file = screens::Saved::Written("chatkord-client-report-1794912120.txt".into());
        window.update(WindowMessage::Report(ReportMessage::Saved(file)), moment());
    }
    window
}

/// The sign-in door's gear, About Chatkord there, and the report's dialog over it.
fn door_report() -> Window {
    let mut window = door_devices();
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), moment());
    window.update(WindowMessage::NoticesRead(Some(screens::harness::notices())), moment());
    reported(window, false)
}

/// Voice and video open over Book club while it connects, the microphone test running.
fn reconnecting_devices() -> Window {
    let mut window = voice_settings_with(in_call);
    let id = books(&window);
    window.update(WindowMessage::Open(id), moment());
    window.update(WindowMessage::TestMicrophone, moment());
    window
}

fn notifications() -> Window {
    let mut window = both(in_lounge);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Notifications), moment());
    window
}

fn startup() -> Window {
    let mut window = both(in_lounge);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Startup), moment());
    window
}

/// The tray choice made, then the start at sign-in turned off: the second switch holds its value, disabled.
fn startup_held() -> Window {
    let mut window = startup();
    for flip in [WindowMessage::ToggleAtSignIn, WindowMessage::ToggleStartsIn, WindowMessage::ToggleAtSignIn] {
        window.update(flip, moment());
    }
    window
}

/// About Chatkord with the design's notices and version, whatever runs, found up to date; `open` is the group folded
/// open.
fn about(open: Option<usize>) -> Window {
    let mut window = both(in_lounge).built_as(screens::harness::BUILD);
    greeted_builds(&mut window);
    reach(&mut window, Reached::UpToDate);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), moment());
    window.update(WindowMessage::NoticesRead(Some(screens::harness::notices())), moment());
    if let Some(group) = open {
        window.update(WindowMessage::ToggleNotice(group), moment());
    }
    window
}

/// Each server greeted with its release: Night Owls 0.9.0, Book club 0.8.2.
fn greeted_builds(window: &mut Window) {
    let places: Vec<PlaceId> = window.places().iter().map(screens::Place::id).collect();
    for (place, build) in places.into_iter().zip(["0.9.0", "0.8.2"]) {
        let build = terms::ServerBuild::read(build).ok().flatten();
        at(window, place, PlaceMessage::Server(screens::ServerMessage::Built(build)));
        let spoke = proto::chatkord_v1::ProtoVersion { major: 1, minor: 0 };
        at(window, place, PlaceMessage::Server(screens::ServerMessage::Spoke(spoke)));
    }
}

/// About Chatkord outside a call, a newer version in place.
fn about_update_ready() -> Window {
    let mut window = both(books_signed_in).built_as(screens::harness::BUILD);
    greeted_builds(&mut window);
    reach(&mut window, Reached::Ready);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), moment());
    window.update(WindowMessage::NoticesRead(Some(screens::harness::notices())), moment());
    window
}

/// About Chatkord in a build with no package: no notices, and no update channel.
fn about_absent() -> Window {
    let mut window = both(in_lounge).built_as(screens::harness::BUILD);
    greeted_builds(&mut window);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), moment());
    window.update(WindowMessage::NoticesRead(None), moment());
    window
}

/// How far the canvas's update has got.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reached {
    UpToDate,
    Downloading,
    Ready,
}

/// `window` told its build offers updates, then checked by itself after its first wait, as far as `reached`.
fn reach(window: &mut Window, reached: Reached) {
    use screens::{UpdateFound, UpdateMessage};
    let start = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
    let checked = start + screens::FIRST_CHECK;
    let found = match reached {
        Reached::UpToDate => UpdateFound::Nothing,
        Reached::Downloading | Reached::Ready => UpdateFound::Newer("0.9.1".into()),
    };
    let mut told = vec![
        UpdateMessage::Offered(start),
        UpdateMessage::Clock(checked),
        UpdateMessage::Checked { at: checked, found },
    ];
    told.extend(match reached {
        Reached::UpToDate => None,
        Reached::Downloading => Some(UpdateMessage::Progress(screens::Percent::new(64))),
        Reached::Ready => Some(UpdateMessage::Placed),
    });
    for message in told {
        window.update(WindowMessage::Update(message), moment());
    }
}

/// The general channel open with the side pane's notice of an update as far as `reached`, in the call in Lounge when
/// `in_voice`.
fn updating(reached: Reached, in_voice: bool) -> Window {
    let mut window = if in_voice {
        in_general(|window, owls| {
            in_call(window);
            let open = screens::DiscussionMessage::Open(screens::harness::channel_id("general"));
            at(window, owls, PlaceMessage::Discussion(open));
            let page = screens::chat::Page { messages: general(), older: false, newer: false, revision: 100 };
            let timeline = screens::harness::general_timeline();
            let read = screens::DiscussionMessage::Page { timeline, end: None, page: Ok(page) };
            at(window, owls, PlaceMessage::Discussion(read));
        })
    } else {
        in_general(|_, _| {})
    };
    reach(&mut window, reached);
    window
}

/// The Appearance page after `steps` presses of +; the canvas draws its page unzoomed, so this is drawn so too.
fn zoomed(steps: usize) -> Window {
    let mut window = appearance();
    for _ in 0..steps {
        window.update(WindowMessage::Look(choices::LookPick::Zoom(choices::ZoomPick::In)), moment());
    }
    window
}

/// The sound devices as the canvas lists them: the default in use for the microphone, the headset for the speaker.
fn sound_listed() -> screens::ProbeHeard {
    let listed =
        |asked: Option<&str>, label: &str| screens::Listed { asked: asked.map(str::to_string), label: label.into() };
    let sound = screens::Sound {
        microphones: vec![
            listed(None, BUILT_IN),
            listed(Some(HEADSET_MIC), HEADSET_MIC),
            listed(Some(BUILT_IN), BUILT_IN),
        ],
        speakers: vec![
            listed(None, BUILT_IN),
            listed(Some(HEADSET_SPEAKER), HEADSET_SPEAKER),
            listed(Some(BUILT_IN), BUILT_IN),
        ],
        microphone: None,
        speaker: Some(HEADSET_SPEAKER.into()),
    };
    screens::ProbeHeard::Sound(sound)
}

/// Settings › Voice and video as the canvas draws it: the default in use for a microphone whose saved choice is
/// unplugged, the headset for the speaker, and the microphone's level at 58.
fn voice_settings() -> Window {
    voice_settings_with(in_lounge)
}

/// The same, with `then` done first.
fn voice_settings_with(then: fn(&mut Window)) -> Window {
    let remembered = choices::Remembered {
        startup: Some(choices::Startup::default()),
        microphone: Some(choices::Picked { id: DESK_MIC.into(), label: DESK_MIC.into() }),
        speaker: Some(choices::Picked { id: HEADSET_SPEAKER.into(), label: HEADSET_SPEAKER.into() }),
        next_call: choices::State::default(),
        share: choices::ShareQuality::default(),
        look: choices::Look::default(),
        event_sounds: choices::EventSounds::On,
        message_sounds: choices::MessageSounds::On,
        popups: choices::Popups::On,
        processing: choices::VoiceProcessing::default(),
        camera: choices::CameraSettings::default(),
        language: None,
        auto_update: choices::AutoUpdate::On,
        emoji: choices::EmojiPicks::default(),
    };
    let mut window = both(then).remembering(remembered, std::collections::BTreeMap::new());
    window.update(WindowMessage::Encoders(encoders()), moment());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::VoiceAndVideo), moment());
    window.update(WindowMessage::Probed(sound_listed()), moment());
    window.update(WindowMessage::Probed(screens::ProbeHeard::Loudness(-29)), moment());
    window
}

/// The cameras the canvas lists, in its order: the USB camera captures 60 a second at 720p and 480p.
fn cameras_listed() -> screens::ProbeHeard {
    let listed = |name: &str, sixty_at: Vec<choices::CameraResolution>| screens::ListedCamera {
        camera: screens::Listed { asked: Some(name.into()), label: name.into() },
        sixty_at,
    };
    let fast = vec![choices::CameraResolution::P720, choices::CameraResolution::P480];
    screens::ProbeHeard::Cameras(Ok(vec![listed(BUILT_IN_CAMERA, Vec::new()), listed(USB_CAMERA, fast)]))
}

/// Voice and video with `chosen` kept and the cameras listed, and the test as `test` says: none asked, asked and not
/// answered, or answered.
fn camera_settings(chosen: &str, test: Option<Option<Result<(), words::Failure>>>) -> Window {
    let mut window = voice_settings();
    let camera = choices::CameraSettings {
        device: Some(choices::Picked { id: chosen.into(), label: chosen.into() }),
        ..Default::default()
    };
    window.update(WindowMessage::PickCamera(camera.device), moment());
    window.update(WindowMessage::Probed(cameras_listed()), moment());
    if let Some(answer) = test {
        window.update(WindowMessage::TestCamera, moment());
        if let Some(answer) = answer {
            window.update(WindowMessage::Probed(screens::ProbeHeard::CameraTest(answer)), moment());
        }
    }
    window
}

/// Voice and video while the call sends this account's camera: the preview is the call's picture.
fn camera_live_settings() -> Window {
    let mut window = with_camera(cameras_on);
    let picked = choices::Picked { id: USB_CAMERA.into(), label: USB_CAMERA.into() };
    window.update(WindowMessage::PickCamera(Some(picked)), moment());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::VoiceAndVideo), moment());
    window.update(WindowMessage::Probed(sound_listed()), moment());
    window.update(WindowMessage::Probed(screens::ProbeHeard::Loudness(-29)), moment());
    window.update(WindowMessage::Probed(cameras_listed()), moment());
    window
}

/// The call with the connection popover open over its panel, `health` as the call last looked.
fn connection(health: screens::Health, alone_in_room: bool) -> Window {
    let mut window = if alone_in_room { alone() } else { both(in_lounge) };
    let owls = window.places()[0].id();
    looks(&mut window, owls, health);
    at(&mut window, owls, PlaceMessage::Call(screens::CallMessage::Stats));
    window
}

/// The call's looks at its connection up to `health`: the canvas's three minutes of round trips once there is a
/// verdict, each look as `health` but for its round trip, the last `health` itself (the canvas ends at its figure).
fn looks(window: &mut Window, owls: PlaceId, health: screens::Health) {
    let trips = health.quality.map(ui::testing::canvas_round_trips).unwrap_or_default();
    for trip in trips.iter().copied().take(trips.len().saturating_sub(1)) {
        let look = screens::Health { round_trip: Some(trip), ..health };
        at(window, owls, PlaceMessage::Call(screens::CallMessage::Health(look)));
    }
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Health(health)));
}

/// What the canvas's Good connection shows.
fn good() -> screens::Health {
    looked(screens::Quality::Good, 38, ((48_000, 12_408), (96_000, 24_117)), 0, (12_400_000, 48_100_000))
}

/// What the canvas's Poor connection shows.
fn poor() -> screens::Health {
    looked(screens::Quality::Poor, 268, ((31_000, 9_872), (52_000, 17_305)), 126, (9_700_000, 31_200_000))
}

/// What the canvas's Fair connection shows.
fn fair() -> screens::Health {
    looked(screens::Quality::Fair, 172, ((44_000, 11_902), (81_000, 22_540)), 9, (12_100_000, 46_700_000))
}

/// A look at a connection: its verdict and round trip, what was sent and heard as (rate, packets), what was lost, and
/// what the call carried up and down.
fn looked(
    quality: screens::Quality,
    millis: u64,
    ((sent, packets), (heard, received)): ((u64, u64), (u64, u64)),
    lost: u64,
    (up_bytes, down_bytes): (u64, u64),
) -> screens::Health {
    let flow = |bits_per_second, packets| screens::Flow { bits_per_second, packets };
    screens::Health {
        round_trip: Some(screens::RoundTrip {
            took: std::time::Duration::from_millis(millis),
            on: screens::MeasuredOn::Route,
        }),
        quality: Some(quality),
        sending: flow(sent, packets),
        receiving: flow(heard, received),
        lost,
        up_bytes,
        down_bytes,
    }
}

fn ended(reason: CloseReason) -> impl Fn(&mut Window, PlaceId) {
    move |window, owls| {
        at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Ended(End::Closed(Some(reason))))))
    }
}

fn lost() -> Window {
    let mut window = both(|window| {
        books_signed_in(window);
        in_call(window);
    });
    let owls = window.places()[0].id();
    at(
        &mut window,
        owls,
        PlaceMessage::Session(screens::SessionMessage::Disconnected(terms::Ended {
            silent: true,
            ..terms::Ended::default()
        })),
    );
    window
}

fn back() -> Window {
    let mut window = lost();
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Session(screens::SessionMessage::Reconnect(1)));
    at(&mut window, owls, flow(token()));
    at(
        &mut window,
        owls,
        flow(Report::Accepted { token: None, named: None, standing: screens::harness::standing_as("ada") }),
    );
    at(&mut window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(channels_of(&night_owls())))));
    window
}

/// Someone else gave this account a role above its highest: the toast says it is raised.
fn raised() -> Window {
    let mut window = both(books_signed_in);
    let owls = window.places()[0].id();
    let admin = proto::rules::Grant::ALL.into_iter().collect::<proto::rules::Grants>();
    now_holding(&mut window, owls, screens::harness::ADMIN_PLACE, admin);
    window
}

/// Night Owls open with both signed in and in a call in Lounge, as the canvas's window is.
fn in_lounge(window: &mut Window) {
    books_signed_in(window);
    let owls = window.places()[0].id();
    now_holding(window, owls, proto::rules::Place::BOTTOM, member_words());
    in_call(window);
}

/// In the call in Lounge and reading #general, the canvas's window when nothing else is open: tonight's three runs.
fn in_lounge_reading(window: &mut Window) {
    in_lounge(window);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    let general = screens::harness::channel_id("general");
    at(window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Open(general)));
    let messages = vec![
        said(1, "grace", tonight(21, 2), "anyone up for a game later?"),
        said(2, "bo", tonight(21, 5), "yes! give me ten minutes"),
        said(3, "mira", tonight(21, 12), "I can hear the game too"),
    ];
    let page = screens::chat::Page { messages, older: false, newer: false, revision: 100 };
    let first =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(window, owls, PlaceMessage::Discussion(first));
}

/// Night Owls's menu from the pane's header, with `standing` told first.
fn menu(standing: Option<&Standing>) -> Window {
    let mut window = both(in_lounge_reading);
    let owls = window.places()[0].id();
    if let Some(Standing { place, grants, .. }) = standing {
        now_holding(&mut window, owls, *place, *grants);
    }
    window.update(WindowMessage::Menu(owls, MenuFrom::Header), moment());
    window
}

/// A confirmation of Night Owls's menu, as its owner when `owner`.
/// The window's close asked during the call in Lounge.
fn quit() -> Window {
    let mut window = both(in_lounge_reading);
    window.update(WindowMessage::CloseAsked, moment());
    window
}

fn confirm(pick: Pick, owner: bool) -> Window {
    let mut window = both(in_lounge_reading);
    let owls = window.places()[0].id();
    if owner {
        now_holding(&mut window, owls, place_of(owner), every_word());
        // The toast that standing brings is put away, as the canvas has none.
        window.update(WindowMessage::Open(owls), moment());
    }
    window.update(WindowMessage::Pick(owls, pick), moment());
    window
}

/// What the owner role and the admin role hold.
fn every_word() -> proto::rules::Grants {
    proto::rules::Grant::ALL.into_iter().collect()
}

/// Night Owls's owner, just joined with an owner invite, who skipped naming it: the note that says so.
fn owner_note() -> Window {
    let mut window = owner_first(false);
    window.update(WindowMessage::Dialog(DialogMessage::Cancel), moment());
    window
}

/// Night Owls's owner, just joined with an owner invite: asked for a name and a colour, the picker open on green.
fn owner_first(colour: bool) -> Window {
    let mut window = previewed(link(CHAT, OWNER), night_owls(), works(true));
    tell(&mut window, Message::Submit);
    tell(&mut window, Message::Username(ADA.into()));
    tell(&mut window, Message::Password(Secret::new(PASSWORD.into())));
    tell(&mut window, Message::Again(Secret::new(PASSWORD.into())));
    tell(&mut window, Message::Submit);
    let standing =
        Standing { place: proto::rules::Place::OWNER, grants: every_word(), ..screens::harness::standing_as("ada") };
    let id = window.welcome_door().map(|adding| adding.id()).unwrap();
    heard(&mut window, Heard::Registered(Ok(Admitted { token: None, named: Some(ADA.into()), standing })));
    at(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(channels_of(&night_owls())))));
    if colour {
        for picking in [Picking::Toggle, Picking::Typed("#2FA36B".into())] {
            window.update(WindowMessage::Dialog(DialogMessage::SetUp(SetUpMessage::Colour(picking))), moment());
        }
    }
    window
}

/// Night Owls's owner in Lounge, inviting people.
fn invite_options() -> Window {
    inviting_as(true, |_| {})
}

/// Night Owls's server roles read, and the invite dialog open for its owner, or for an administrator holding every word
/// but the one that gives roles; `then` is what is done in it.
fn inviting_as(owner: bool, then: fn(&mut Window)) -> Window {
    let mut window = both(in_lounge_reading);
    let owls = window.places()[0].id();
    let held = if owner {
        every_word()
    } else {
        proto::rules::Grant::ALL.into_iter().filter(|word| *word != proto::rules::Grant::ManageRoles).collect()
    };
    now_holding(&mut window, owls, place_of(owner), held);
    at(&mut window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles_more()))));
    window.update(WindowMessage::Open(owls), moment());
    window.update(WindowMessage::Pick(owls, Pick::Invite), moment());
    then(&mut window);
    window
}

fn invite_said(window: &mut Window, said: InviteMessage) {
    window.update(WindowMessage::Dialog(DialogMessage::Invite(said)), moment());
}

/// Custom picked, `typed` in its field.
fn invite_custom(window: &mut Window, typed: &str) {
    invite_said(window, InviteMessage::Uses(screens::UsesMessage::Custom));
    invite_said(window, InviteMessage::Uses(screens::UsesMessage::Typed(typed.into())));
}

/// The owner's invite made after the terms `chosen`, its link shown.
fn invite_created(chosen: fn(&mut Window)) -> Window {
    let mut window = inviting_as(true, chosen);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Dialog(DialogMessage::Confirm), moment());
    let code = Secret::new(format!("Kx7fQ2{}", "a".repeat(proto::rules::TOKEN_LEN - 6)));
    at(
        &mut window,
        owls,
        PlaceMessage::Page(screens::PageMessage::Invited(Ok(terms::Invited { code, expires_at: UNTIL }))),
    );
    window
}

/// Server settings › Invites, as the canvas lists them.
fn server_invites() -> Window {
    let mut window = confirm(Pick::Settings, true);
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Invites)));
    at(&mut window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles_more()))));
    let row = |id: &str, uses, max_uses, expires_at, note: &str, role: Option<&str>| InviteSummary {
        invite_id: id.into(),
        uses,
        max_uses,
        expires_at,
        note: Some(note.into()),
        role_id: role.map(str::to_string),
    };
    let rows =
        vec![row("1", 2, 5, UNTIL, "for Grace", Some(REGULARS_ROLE)), row("2", 0, 1, TOMORROW, "Bo’s brother", None)];
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Invites(Ok(rows))));
    window
}

/// Server settings › Webhooks, with two webhooks listed or none.
fn server_webhooks(listed: bool) -> Window {
    webhooks_page(listed)
}

/// Where the Webhooks page is in its dialogs.
#[derive(Clone, Copy)]
enum WebhookStage {
    Making,
    Made,
    Regenerating,
    Deleting,
}

/// The Webhooks page with two webhooks, and the dialog `stage` names open over it.
fn webhooks_in(stage: WebhookStage) -> Window {
    use screens::webhooks::{Message as Said, WebhookId};
    let mut window = webhooks_page(true);
    let owls = window.places()[0].id();
    let mut page = |said: Said| at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Webhooks(said)));
    let first = || WebhookId::parse("h1").expect("an id");
    match stage {
        WebhookStage::Making => {
            page(Said::Make);
            page(Said::Name("Uptime".into()));
            page(Said::Channel(screens::harness::channel_id("staff")));
        }
        WebhookStage::Made => {
            page(Said::Make);
            page(Said::Name("Server alerts".into()));
            page(Said::Submit);
            let made = proto::chatkord_v1::Webhook {
                webhook_id: "h1".into(),
                channel_id: "staff".into(),
                name: "Server alerts".into(),
                made_by: Some("ada".into()),
                created_at: UNTIL,
                ..proto::chatkord_v1::Webhook::default()
            };
            let secret = Secret::new("example-secret".to_string());
            at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::WebhookMade(Ok((made, secret)))));
        }
        WebhookStage::Regenerating => page(Said::Regenerate(first())),
        WebhookStage::Deleting => page(Said::Delete(first())),
    }
    window
}

/// Server settings › Overview with the hosting facts answered; `refused` types a largest file past its ceiling and a
/// storage under its least.
fn server_hosting(refused: bool) -> Window {
    use screens::hosting::{Field, Message as Said};
    let mut window = server_overview(false, None);
    let owls = window.places()[0].id();
    let hosting = proto::chatkord_v1::Hosting {
        public_address: Some("chat.example.invalid".into()),
        ..proto::rules::Hosting::DEFAULT.to_wire()
    };
    let told = proto::chatkord_v1::ServerHosting { hosting: Some(hosting), switched_off: Vec::new() };
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::ServerHostingListed(Ok(told))));
    if refused {
        for (field, typed) in
            [(proto::rules::HostingField::AttachmentMax, "8192"), (proto::rules::HostingField::AccountFilesMax, "0")]
        {
            let field = Field::Number(field);
            for said in [Said::Typed(field, typed.into()), Said::Save(field)] {
                let message = screens::PageMessage::Overview(OverviewMessage::Hosting(said));
                at(&mut window, owls, PlaceMessage::Page(message));
            }
        }
    }
    window
}

/// A post by an app in #game-night, as the canvas's chat.app has it: its name and the APP badge; `about` presses its
/// name.
fn chat_app(about: bool) -> Window {
    let mut post = said(
        3,
        "ada",
        tonight(20, 45),
        "The projector is on and the living room is at 21 °C. Game night starts in 15 minutes.",
    );
    post.author =
        screens::chat::Author::App { name: "Home Assistant".into(), webhook_name: "Game night reminders".into() };
    let id = post.id.clone();
    let messages = vec![
        said(1, "grace", tonight(19, 58), "who’s bringing the projector tonight?"),
        said(2, "bo", tonight(20, 1), "me, it’s in the car"),
        post,
    ];
    in_channel("game-night", messages, move |window, owls| {
        if about {
            window.update(WindowMessage::AppAbout(owls, id), moment());
        }
    })
}

/// Book club joined, with its welcome note switched off: a joining member is not greeted.
fn joined_quiet() -> Window {
    let mut window = working();
    let admitted = Admitted { token: None, named: Some(ADA.into()), standing: screens::harness::standing_as("ada") };
    let id = window.welcome_door().map(|adding| adding.id()).unwrap();
    let offered = screens::Served::read(&[proto::rules::Switch::Welcome]);
    at(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Serves(offered)));
    heard(&mut window, Heard::Registered(Ok(admitted)));
    // Book club's channels as the canvas has them once joined: two text channels, and mira in Reading room.
    let text = |name: &str| screens::TextChannel {
        id: screens::harness::channel_id(name),
        name: name.into(),
        timeline: screens::harness::general_timeline(),
        revision: 0,
    };
    let listed = in_first_categories(screens::ChannelList {
        text: vec![text("general"), text("reading-list")],
        voice: vec![channel("reading", "Reading room")],
        ..screens::ChannelList::default()
    });
    at(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(listed))));
    let reading = screens::harness::channel_id("reading");
    let seated = occupants_placed(&["mira"]);
    at(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Occupancy { channel_id: reading, people: seated }));
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    let general = screens::DiscussionMessage::Open(screens::harness::channel_id("general"));
    at(&mut window, id, PlaceMessage::Discussion(general));
    let messages = vec![
        said(1, "mira", tonight(19, 40), "chapter four on Thursday?"),
        said(2, "grace", tonight(19, 44), "works for me"),
    ];
    let page = screens::chat::Page { messages, older: false, newer: false, revision: 100 };
    let first =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(&mut window, id, PlaceMessage::Discussion(first));
    window
}

/// This account's own screen pinned, then #general opened in place of the call: it floats.
fn floating_own(window: &mut Window, owls: PlaceId) {
    own_pinned(window, owls);
    let grants = member_words().union(proto::rules::Grants::of(proto::rules::Grant::ShareScreen));
    now_holding(window, owls, proto::rules::Place::BOTTOM, grants);
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    let open = screens::DiscussionMessage::Open(screens::harness::channel_id("general"));
    at(window, owls, PlaceMessage::Discussion(open));
    let page = screens::chat::Page { messages: general(), older: false, newer: false, revision: 100 };
    let first =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(window, owls, PlaceMessage::Discussion(first));
}

fn webhooks_page(listed: bool) -> Window {
    let mut window = confirm(Pick::Settings, true);
    let owls = window.places()[0].id();
    serves(&mut window, owls);
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Webhooks)));
    let hook = |id: &str, name: &str, channel: &str, by: &str, created_at| proto::chatkord_v1::Webhook {
        webhook_id: id.into(),
        channel_id: channel.into(),
        name: name.into(),
        mention_everyone: false,
        made_by: Some(by.into()),
        created_at,
    };
    // 2 November and 28 October, at noon.
    let day = 24 * 3_600;
    let rows = if listed {
        vec![
            hook("h1", "Server alerts", "staff", "grace", UNTIL + day + day / 2),
            hook("h2", "Game night reminders", "game-night", "ada", UNTIL - 4 * day + day / 2),
        ]
    } else {
        Vec::new()
    };
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::WebhooksListed(Ok(rows))));
    window
}

/// Server settings › Overview as the server's owner: the colour picker open, or a name typed.
fn server_overview(picking: bool, typed: Option<&str>) -> Window {
    let mut window = confirm(Pick::Settings, true);
    let owls = window.places()[0].id();
    greeted_builds(&mut window);
    serves(&mut window, owls);
    if let Some(typed) = typed {
        let typed = NameFieldMessage::Typed(typed.into());
        at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Overview(OverviewMessage::Name(typed))));
    }
    if picking {
        at(
            &mut window,
            owls,
            PlaceMessage::Page(screens::PageMessage::Overview(OverviewMessage::Colour(Picking::Toggle))),
        );
    }
    window
}

/// Server settings › About at a server that greeted with its release and protocol and offers its report.
fn server_about() -> Window {
    let mut window = server_overview(false, None);
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(screens::Page::About)));
    window
}

/// How far the server's debug report has got on About.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerReportStage {
    /// Download was pressed and the server has not answered.
    Busy,
    /// The server answered: the dialog lists the report.
    Shown,
    /// The file was saved: the dialog is gone and the card names it.
    Saved,
    /// The server refused for pace: the card says so as an alert.
    Refused,
}

/// Server settings › About with the server's debug report asked for, as far as `stage`.
fn server_report(stage: ServerReportStage) -> Window {
    use screens::{ReportMessage, ServerAboutMessage};
    let mut window = server_about();
    let owls = window.places()[0].id();
    let report = |window: &mut Window, message| {
        at(window, owls, PlaceMessage::Page(screens::PageMessage::About(ServerAboutMessage::Report(message))));
    };
    report(&mut window, ReportMessage::Open);
    let answer = match stage {
        ServerReportStage::Busy => return window,
        ServerReportStage::Refused => {
            Err(terms::ReportError::Reach(terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::RateLimited)))
        }
        ServerReportStage::Shown | ServerReportStage::Saved => {
            terms::ArrivedReport::read(screens::harness::SERVER_REPORT, screens::LISTED_LOG_LINES)
        }
    };
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::ServerReported(answer)));
    if stage == ServerReportStage::Saved {
        report(&mut window, ReportMessage::Download);
        let file = screens::Saved::Written("chatkord-server-report-1794912180.txt".into());
        report(&mut window, ReportMessage::Saved(file));
    }
    window
}

/// Who is in a voice room, by their names: the count a page shows is theirs.
fn occupants(names: &[&str]) -> Vec<screens::Occupant> {
    names
        .iter()
        .map(|name| screens::Occupant {
            user_id: screens::harness::member(&name.to_lowercase()),
            colour: None,
            name: (*name).into(),
            state: choices::State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        })
        .collect()
}

/// Server settings › Channels as the server's owner, with four people in the Lounge's call, #general chosen and its
/// overwrites read: none.
fn server_channels() -> Window {
    let mut window = confirm(Pick::Settings, true);
    let owls = window.places()[0].id();
    channel_people(&mut window, owls);
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Channels)));
    overwrites(&mut window, owls, "general", Vec::new());
    at(
        &mut window,
        owls,
        PlaceMessage::Lobby(screens::LobbyMessage::Occupancy {
            channel_id: screens::harness::channel_id("lounge"),
            people: occupants(&["Grace", "Bo", "Mira", "Cy"]),
        }),
    );
    window
}

/// Every category deleted: the server lists no category, so no channel.
fn no_categories(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(screens::ChannelList::default()))));
}

/// The Channels page of a server with no category.
fn server_channels_empty() -> Window {
    owning_no_categories(|window, owls| {
        window.update(WindowMessage::Pick(owls, Pick::Settings), moment());
        at(window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Channels)));
    })
}

/// Night Owls owned by Ada with no category, so no channel and no call in it; `then` does the rest.
fn owning_no_categories(then: fn(&mut Window, PlaceId)) -> Window {
    without_categories(proto::rules::Place::OWNER, every_word(), then)
}

/// Night Owls with no category, so no channel and no call in it, Ada holding `grants` at `place`; `then` does the rest.
fn without_categories(
    place: proto::rules::Place,
    grants: proto::rules::Grants,
    then: fn(&mut Window, PlaceId),
) -> Window {
    let mut window = both(books_signed_in);
    let owls = window.places()[0].id();
    now_holding(&mut window, owls, place, grants);
    window.update(WindowMessage::Open(owls), moment());
    no_categories(&mut window, owls);
    then(&mut window, owls);
    window
}

/// The canvas's roles and its people, as the channels page reads them: Ada, grace, Bo a regular and mira a moderator.
fn channel_people(window: &mut Window, owls: PlaceId) {
    at(window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles()))));
    let named = |name: &str, roles: &[&str]| screens::Named {
        name: name.into(),
        colour: None,
        roles: roles.iter().map(|role| screens::harness::role_id(role)).collect(),
        picture: None,
    };
    let entries = vec![
        (screens::harness::member(ME), named(ADA, &[]), proto::status::Shown::Online),
        (screens::harness::member("grace"), named("grace", &[]), proto::status::Shown::Online),
        (screens::harness::member("bo"), named("Bo", &[REGULARS_ROLE]), proto::status::Shown::Online),
        (screens::harness::member("mira"), named("mira", &[MODS_ROLE]), proto::status::Shown::Online),
    ];
    let page = screens::PeoplePage { entries, next: None, presence: 1 };
    at(window, owls, PlaceMessage::People(screens::PeopleMessage::Page(Ok(page))));
}

/// The server's answer listing the overwrites of the channel `id`.
fn overwrites(window: &mut Window, owls: PlaceId, id: &str, listed: Vec<proto::chatkord_v1::ChannelOverwrite>) {
    let told = screens::ServerMessage::Overwrites { channel: screens::harness::channel_id(id), listed: Ok(listed) };
    at(window, owls, PlaceMessage::Server(told));
}

/// #staff's overwrites on the canvas: hidden from Member, seen by Moderators and grace.
fn staff_marks() -> Vec<proto::chatkord_v1::ChannelOverwrite> {
    use proto::chatkord_v1::{ChannelOverwrite, OverwriteState, channel_overwrite::Whom};
    let read = proto::rules::Grant::ChannelRead.word().to_string();
    let mark =
        |whom, state: OverwriteState| ChannelOverwrite { whom: Some(whom), word: read.clone(), state: state.into() };
    vec![
        mark(Whom::RoleId(MEMBER_ROLE.into()), OverwriteState::Deny),
        mark(Whom::RoleId(MODS_ROLE.into()), OverwriteState::Allow),
        mark(Whom::UserId("grace".into()), OverwriteState::Allow),
    ]
}

fn staff_adding() -> screens::channel_settings::Adding {
    screens::channel_settings::Adding::Access(screens::Access::Private)
}

/// The channels page with the channel `id` chosen and its overwrites read as `marks`.
fn channel_chosen(id: &str, marks: Vec<proto::chatkord_v1::ChannelOverwrite>) -> Window {
    let mut window = server_channels();
    let owls = window.places()[0].id();
    let choose = screens::channel_settings::Message::Choose(screens::harness::channel_id(id));
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Channels(choose)));
    overwrites(&mut window, owls, id, marks);
    window
}

/// #staff chosen, then `message` on the page.
fn channel_said(message: screens::channel_settings::Message) -> Window {
    let mut window = channel_chosen("staff", staff_marks());
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Channels(message)));
    window
}

/// The channels page opened from #general's Edit channel, its overwrites read.
fn channels_opened() -> Window {
    owning(|window, owls| {
        channel_people(window, owls);
        list_pick(window, owls, channel_key("general"), screens::ListPick::EditChannel);
        overwrites(window, owls, "general", Vec::new());
    })
}

/// The channels page as Ada an admin who holds every word but Delete messages, Advanced permissions open.
fn channels_admin() -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    let held =
        proto::rules::Grant::ALL.into_iter().filter(|word| *word != proto::rules::Grant::ManageMessages).collect();
    now_holding(&mut window, owls, screens::harness::ADMIN_PLACE, held);
    window.update(WindowMessage::Open(owls), moment());
    channel_people(&mut window, owls);
    window.update(WindowMessage::Pick(owls, Pick::Settings), moment());
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Channels)));
    overwrites(&mut window, owls, "general", Vec::new());
    let advanced = screens::channel_settings::Message::Advanced;
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Channels(advanced)));
    window
}

/// #general made private, refused: the owner no longer holds Manage roles.
fn channels_refused() -> Window {
    let mut window = server_channels();
    let owls = window.places()[0].id();
    let switch = screens::channel_settings::Message::Switch(screens::Access::Private);
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Channels(switch)));
    let answer = Err(terms::LinkError::Refused(ErrorCode::NotAuthorized));
    let set = screens::PageMessage::OverwriteSet { channel: screens::harness::channel_id("general"), answer };
    at(&mut window, owls, PlaceMessage::Page(set));
    window
}

/// Create channel asked from the channels page, then `then`.
fn channel_create(then: fn(&mut Window, PlaceId)) -> Window {
    let mut window = server_channels();
    let owls = window.places()[0].id();
    window.update(WindowMessage::CreateChannel(owls), moment());
    then(&mut window, owls);
    window
}

fn named(window: &mut Window, name: &str) {
    window.update(WindowMessage::Dialog(DialogMessage::Named(name.into())), moment());
}

fn making(window: &mut Window, message: screens::MakingMessage) {
    window.update(WindowMessage::Dialog(DialogMessage::Making(message)), moment());
}

fn channel_create_voice(window: &mut Window, _: PlaceId) {
    making(window, screens::MakingMessage::Kind(proto::rules::Kind::Voice));
    named(window, "Movie night");
}

fn channel_create_private(window: &mut Window, _: PlaceId) {
    named(window, "planning");
    making(window, screens::MakingMessage::Category(terms::CategoryId::parse("projects").expect("an id")));
    making(window, screens::MakingMessage::Switch(screens::Access::Private));
    let mods = screens::Whom::Role(screens::harness::role_id(MODS_ROLE));
    making(window, screens::MakingMessage::Allow(screens::Access::Private, mods));
}

fn channel_create_readonly(window: &mut Window, _: PlaceId) {
    named(window, "news");
    making(window, screens::MakingMessage::Switch(screens::Access::ReadOnly));
    let mods = screens::Whom::Role(screens::harness::role_id(MODS_ROLE));
    making(window, screens::MakingMessage::Allow(screens::Access::ReadOnly, mods));
}

/// Create channel from Hangouts's "+" in the side pane.
fn channel_create_in(window: &mut Window, owls: PlaceId) {
    channel_people(window, owls);
    window.update(WindowMessage::CreateChannelIn(owls, terms::CategoryId::parse("hangouts").expect("an id")), moment());
}

/// Deleting #ideas asked from its menu in the side pane.
fn ideas_deleted(window: &mut Window, owls: PlaceId) {
    list_pick(window, owls, channel_key("ideas"), screens::ListPick::DeleteChannel);
}

/// Create channel with "Movie night" typed (`None`), a new name for the Quiet corner, or deleting the Lounge.
fn channel_asked(action: Option<ChannelAction>) -> Window {
    let mut window = server_channels();
    let owls = window.places()[0].id();
    let asked = match action {
        None => {
            window.update(WindowMessage::CreateChannel(owls), moment());
            WindowMessage::Dialog(DialogMessage::Named("Movie night".into()))
        }
        Some(ChannelAction::Rename) => {
            WindowMessage::Channel(owls, ChannelAction::Rename, screens::harness::channel_id("corner"))
        }
        Some(ChannelAction::Delete) => {
            WindowMessage::Channel(owls, ChannelAction::Delete, screens::harness::channel_id("lounge"))
        }
    };
    window.update(asked, moment());
    window
}

/// What the account page's password form shows.
enum Form {
    Closed,
    Empty,
    /// Filled in and ready to save.
    Ready,
    /// Ready, and the server said the current password is not the account's.
    Wrong,
}

/// Ada's account page at Night Owls.
fn profile() -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::OpenAccount));
    window
}

/// The account page with its colour's picker open on a blue picked there, as the canvas has it.
fn profile_picking() -> Window {
    let mut window = profile();
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::ProfileColour(screens::colour::Message::Toggle)));
    at(
        &mut window,
        owls,
        PlaceMessage::Page(screens::PageMessage::ProfileColour(screens::colour::Message::Typed("#5AA9FF".into()))),
    );
    window
}

/// Ada's account page at Night Owls, with the password form as `form` says.
fn account(form: &Form) -> Window {
    use screens::{AccountMessage, Field};
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::OpenAccount));
    if matches!(form, Form::Closed) {
        return window;
    }
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Account(AccountMessage::Change)));
    if matches!(form, Form::Empty) {
        return window;
    }
    let typed = |field, said: &str| {
        PlaceMessage::Page(screens::PageMessage::Account(AccountMessage::Typed(field, Secret::new(said.into()))))
    };
    at(&mut window, owls, typed(Field::Current, "hunter2 and more"));
    at(&mut window, owls, typed(Field::New, "a long enough new phrase"));
    at(&mut window, owls, typed(Field::Again, "a long enough new phrase"));
    if matches!(form, Form::Wrong) {
        at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Account(AccountMessage::Save)));
        let wrong = terms::LinkError::Refused(ErrorCode::Unauthenticated);
        at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::PasswordChanged(Err(wrong))));
    }
    window
}

fn confirm_delete() -> Window {
    let mut window = server_invites();
    let owls = window.places()[0].id();
    window.update(WindowMessage::DeleteInvite(owls, screens::harness::invite_id("1")), moment());
    window
}

const OWNER_ROLE: &str = "11";
const ADMIN_ROLE: &str = "12";
const MODS_ROLE: &str = "13";
const REGULARS_ROLE: &str = "14";
const MEMBER_ROLE: &str = "15";
const HOSTS_ROLE: &str = "16";
const HELPERS_ROLE: &str = "17";

/// The canvas's roles in their order: Owner, Admin, Moderators, Regulars, and Member, which everyone holds.
/// The canvas's roles with Game hosts and Helpers made, just above the default role.
fn roles_more() -> Vec<proto::chatkord_v1::RoleEntry> {
    let mut listed = roles();
    let default = listed.pop().expect("the default role");
    let below = default.position;
    let made = |id: &str, name: &str, at: u32, colour: Option<&str>| proto::chatkord_v1::RoleEntry {
        role_id: id.into(),
        name: name.into(),
        position: at,
        colour: colour.map(str::to_string),
        is_owner: false,
        is_default: false,
        ..default.clone()
    };
    listed.push(made(HOSTS_ROLE, "Game hosts", below - 2, Some("#F2C14E")));
    listed.push(made(HELPERS_ROLE, "Helpers", below - 1, None));
    listed.push(default);
    listed
}

fn roles() -> Vec<proto::chatkord_v1::RoleEntry> {
    use proto::rules::Grant::{
        ChannelRead, InviteMember, ManageMembers, ManageMessages, ModerateVoice, SendMessage, ShareCamera, ShareScreen,
        StartVoice,
    };
    let everyone = [ChannelRead, SendMessage, ShareCamera, ShareScreen, StartVoice];
    let words = |extra: &[proto::rules::Grant]| -> Vec<String> {
        everyone.iter().chain(extra).copied().collect::<proto::rules::Grants>().words()
    };
    let admin_at = screens::harness::ADMIN_PLACE.position();
    let role = |id: &str, name: &str, position: u32, colour: Option<&str>, grants: Vec<String>| {
        proto::chatkord_v1::RoleEntry {
            role_id: id.into(),
            name: name.into(),
            is_owner: id == OWNER_ROLE,
            is_default: id == MEMBER_ROLE,
            position,
            grants,
            colour: colour.map(str::to_string),
        }
    };
    vec![
        role(OWNER_ROLE, "Owner", 0, Some("#FF8A5B"), every_word().words()),
        role(ADMIN_ROLE, "Admin", admin_at, Some("#7CB7FF"), every_word().words()),
        role(
            MODS_ROLE,
            "Moderators",
            admin_at + 1,
            Some("#5FD39B"),
            words(&[ManageMessages, ModerateVoice, ManageMembers]),
        ),
        role(REGULARS_ROLE, "Regulars", admin_at + 2, Some("#E2A6FF"), words(&[InviteMember])),
        role(MEMBER_ROLE, "Member", proto::rules::Place::BOTTOM.position(), None, words(&[])),
    ]
}

/// A member holding `held` besides the default role, placed at the highest of them.
fn member(id: &str, name: &str, held: &[&str], banned: bool) -> MemberEntry {
    let listed = roles();
    let held: Vec<&proto::chatkord_v1::RoleEntry> =
        listed.iter().filter(|role| held.contains(&role.role_id.as_str()) || role.is_default).collect();
    let place = held.iter().map(|role| role.position).min().unwrap_or(proto::rules::Place::BOTTOM.position());
    let grants = held
        .iter()
        .fold(proto::rules::Grants::default(), |all, role| all.union(proto::rules::Grants::from_words(&role.grants)));
    MemberEntry {
        user_id: id.into(),
        handle: name.to_lowercase(),
        display_name: name.into(),
        role_ids: held.iter().map(|role| role.role_id.clone()).collect(),
        place: Some(place),
        grants: grants.words(),
        colour: None,
        banned,
    }
}

/// Server settings › Members, as the server's owner Ada with grace an admin, Bo a regular, mira a moderator and Cy
/// banned; or as Ada an admin while mira owns it.
fn server_members(owner: bool) -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    now_holding(&mut window, owls, place_of(owner), every_word());
    window.update(WindowMessage::Open(owls), moment());
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Members)));
    let rows = if owner {
        vec![
            member(ME, ADA, &[OWNER_ROLE], false),
            member("grace", "grace", &[ADMIN_ROLE], false),
            member("bo", "Bo", &[REGULARS_ROLE], false),
            member("mira", "mira", &[MODS_ROLE], false),
            member("cy", "Cy", &[], true),
        ]
    } else {
        vec![
            member(ME, ADA, &[ADMIN_ROLE], false),
            member("grace", "grace", &[ADMIN_ROLE], false),
            member("bo", "Bo", &[], false),
            member("mira", "mira", &[OWNER_ROLE], false),
        ]
    };
    at(&mut window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles()))));
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::MembersListed(Ok(rows))));
    window
}

fn members_said(window: &mut Window, message: screens::MembersMessage) {
    let owls = window.places()[0].id();
    at(window, owls, PlaceMessage::Page(screens::PageMessage::Members(message)));
}

/// A menu open on Bo's row, as the server's owner.
fn member_menu(menu: screens::MemberMenu) -> Window {
    let mut window = server_members(true);
    members_said(&mut window, screens::MembersMessage::Menu(screens::harness::member("bo"), menu));
    window
}

/// The members searched for "gr".
fn members_searched() -> Window {
    let mut window = server_members(true);
    members_said(&mut window, screens::MembersMessage::Search("gr".into()));
    window
}

/// The banned listed.
fn members_banned() -> Window {
    let mut window = server_members(true);
    members_said(&mut window, screens::MembersMessage::Tab(screens::MemberTab::Banned));
    window
}

/// Kick, Ban or Unban asked of `who`, a ban answered with `answer` once its password is typed.
fn banning(who: &str, action: MemberAction, answer: Option<Result<(), terms::LinkError>>) -> Window {
    let mut window = if action == MemberAction::Unban { members_banned() } else { server_members(true) };
    let owls = window.places()[0].id();
    window.update(WindowMessage::Member(owls, action, screens::harness::member(who)), moment());
    if let Some(answer) = answer {
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new(PASSWORD.into()))), moment());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), moment());
        at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::BanSet(answer)));
    }
    window
}

/// Server settings › Roles as the server's owner Ada, Moderators chosen; or as Ada an admin, Admin chosen.
fn server_roles(owner: bool) -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    now_holding(&mut window, owls, place_of(owner), every_word());
    window.update(WindowMessage::Open(owls), moment());
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Settings(Page::Roles)));
    at(&mut window, owls, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(roles()))));
    let chosen = if owner { MODS_ROLE } else { ADMIN_ROLE };
    roles_said(&mut window, screens::RolesMessage::Choose(screens::harness::role_id(chosen)));
    roles_said(&mut window, screens::RolesMessage::Back);
    window
}

/// Moderators opened from the list: alone in a narrow window.
fn role_opened() -> Window {
    let mut window = server_roles(true);
    roles_said(&mut window, screens::RolesMessage::Choose(screens::harness::role_id(MODS_ROLE)));
    window
}

fn roles_said(window: &mut Window, message: screens::RolesMessage) {
    let owls = window.places()[0].id();
    at(window, owls, PlaceMessage::Page(screens::PageMessage::Roles(message)));
}

/// The roles page as the owner with the role `chosen`.
fn role_chosen(chosen: &str) -> Window {
    let mut window = server_roles(true);
    roles_said(&mut window, screens::RolesMessage::Choose(screens::harness::role_id(chosen)));
    window
}

/// The roles page with Moderators's colour picker open.
fn role_colour() -> Window {
    let mut window = server_roles(true);
    roles_said(&mut window, screens::RolesMessage::Colour(Picking::Toggle));
    window
}

/// A change to Moderators refused: the page says so.
fn roles_refused() -> Window {
    let mut window = server_roles(true);
    let owls = window.places()[0].id();
    let refused = terms::LinkError::Refused(ErrorCode::NotAuthorized);
    let changed = screens::PageMessage::RoleChanged { name: "Moderators".into(), answer: Err(refused) };
    at(&mut window, owls, PlaceMessage::Page(changed));
    window
}

/// Create role pressed, "Film club" typed.
fn role_create() -> Window {
    let mut window = server_roles(true);
    let owls = window.places()[0].id();
    window.update(WindowMessage::CreateRole(owls), moment());
    window.update(WindowMessage::Dialog(DialogMessage::Named("Film club".into())), moment());
    window
}

/// Delete role pressed under Moderators.
fn role_delete() -> Window {
    let mut window = server_roles(true);
    let owls = window.places()[0].id();
    window.update(WindowMessage::DeleteRole(owls), moment());
    window
}

/// Giving Bo a role was refused: the page says so above its list.
fn members_refused() -> Window {
    let mut window = server_members(true);
    let owls = window.places()[0].id();
    let refused = terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::NotAuthorized);
    at(
        &mut window,
        owls,
        PlaceMessage::Page(screens::PageMessage::RoleSet { name: "Bo".into(), answer: Err(refused) }),
    );
    window
}

/// A button on grace's row that asks first, as the server's owner.
fn member_asked(action: MemberAction) -> Window {
    let mut window = server_members(true);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Member(owls, action, screens::harness::member("grace")), moment());
    window
}

fn reset_done() -> Window {
    let mut window = member_asked(MemberAction::ResetPassword);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Dialog(DialogMessage::Confirm), moment());
    at(
        &mut window,
        owls,
        PlaceMessage::Page(screens::PageMessage::PasswordReset(Ok(Secret::new("river-copper-lamp-42".into())))),
    );
    window
}

/// The add dialog from the rail's "+", with `typed` looked for.
fn adding(typed: Option<&str>) -> Window {
    let mut window = both(in_lounge_reading);
    window.update(WindowMessage::Add, moment());
    if let Some(typed) = typed {
        add(&mut window, Message::Where(typed.into()));
        add(&mut window, Message::Submit);
    }
    window
}

fn add(window: &mut Window, message: Message) {
    let id = window.add_dialog().map(|adding| adding.id()).unwrap();
    at(window, id, PlaceMessage::Session(screens::SessionMessage::Door(message)));
}

/// The add dialog at an address's sign-in step.
fn add_sign_in() -> Window {
    let mut window = adding(Some(GARDEN));
    add(&mut window, Message::Reported(Heard::Found(Ok(Found { server: plant_swap(), invite: None }))));
    add(&mut window, Message::Submit);
    window
}

/// The canvas adds Book club from the rail's "+"; here it is the first server, from the welcome steps.
fn joined() -> Window {
    let mut window = working();
    let admitted = Admitted { token: None, named: Some(ADA.into()), standing: screens::harness::standing_as("ada") };
    let id = window.welcome_door().map(|adding| adding.id()).unwrap();
    heard(&mut window, Heard::Registered(Ok(admitted)));
    at(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(channels_of(&book_club())))));
    window
}

#[derive(Clone, Debug)]
struct Seen {
    kind: &'static str,
    text: String,
    bounds: Rectangle,
}

/// Every text, field and control of an interface, in view order, where it shows.
#[derive(Default)]
struct Survey {
    texts: Vec<Seen>,
    controls: Vec<Seen>,
    /// How far what is being traversed is scrolled, all the scrollables around it together.
    scrolled: iced_core::Vector,
    /// A scrollable's offset, told before it traverses its content.
    scrolling: iced_core::Vector,
}

impl Operation for Survey {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let outside = self.scrolled;
        self.scrolled = outside + std::mem::take(&mut self.scrolling);
        operate(self);
        self.scrolled = outside;
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content: iced_core::Size,
        translation: iced_core::Vector,
        _state: &mut dyn iced_core::widget::operation::Scrollable,
    ) {
        self.scrolling = translation;
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Text) {
        let text = state.text().trim().to_string();
        if !text.is_empty() {
            self.texts.push(Seen { kind: "text", text, bounds: bounds - self.scrolled });
        }
    }

    fn text_input(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn TextInput) {
        let text = state.text().trim().to_string();
        if !text.is_empty() {
            self.texts.push(Seen { kind: "value", text, bounds: bounds - self.scrolled });
        }
    }

    fn custom(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        let bounds = bounds - self.scrolled;
        let Some(Semantics { role, label }) = state.downcast_ref::<Semantics>() else { return };
        let kind = match role {
            SemanticRole::TextInput | SemanticRole::SecretInput | SemanticRole::Slider { .. } => "field",
            SemanticRole::Link => "link",
            SemanticRole::Button
            | SemanticRole::ListItem
            | SemanticRole::MenuItem
            | SemanticRole::MenuItemRadio { .. }
            | SemanticRole::MenuItemCheckbox { .. }
            | SemanticRole::Radio { .. }
            | SemanticRole::Switch { .. }
            | SemanticRole::Opens { .. } => "button",
            SemanticRole::Image
            | SemanticRole::Status
            | SemanticRole::Alert
            | SemanticRole::Dialog
            | SemanticRole::AlertDialog
            | SemanticRole::Menu
            | SemanticRole::List
            | SemanticRole::RadioGroup
            | SemanticRole::Table
            | SemanticRole::Heading => return,
        };
        self.controls.push(Seen { kind, text: label.as_str().to_string(), bounds });
    }
}

/// Where the canvas has scrolled a state's page: the text it brings up and the height it stands at then.
fn scrolled_to(state: &str) -> Option<(&'static str, f32)> {
    match state {
        "server.overview.features" => Some(("Features", 9.0)),
        "server.hosting" | "server.hosting.refused" => Some(("Hosting", 30.0)),
        _ => None,
    }
}

/// What reaches a state's window before it is drawn and surveyed: the pointer where the canvas rests it, else the
/// wheel over the text the canvas scrolls to, as far as brings it to the canvas's height.
fn staged(name: &str, window: &Window, drawn: Drawn) -> testing::Scene {
    let rested = pointer(name).map_or_else(testing::Scene::default, testing::Scene::hovering);
    let Some((said, at)) = scrolled_to(name) else { return rested };
    let mut seen = Survey::default();
    simulator(window, drawn, &rested).operate(&mut seen);
    let Some(found) = seen.texts.into_iter().find(|seen| seen.text == said) else { return rested };
    let wheel = iced_core::mouse::Event::WheelScrolled {
        delta: iced_core::mouse::ScrollDelta::Pixels { x: 0.0, y: at - found.bounds.y },
    };
    testing::Scene {
        cursor: Some(found.bounds.center()),
        events: vec![iced_core::Event::Mouse(wheel)],
        ..testing::Scene::default()
    }
}

fn simulator<'a>(
    window: &'a Window,
    drawn: Drawn,
    scene: &testing::Scene,
) -> Simulator<'a, WindowMessage, Theme, Renderer> {
    testing::hermetic_fonts();
    let mut sim = Simulator::with_size(iced_core::Settings::default(), drawn.size(), drawn.view(window));
    if let Some(at) = scene.cursor {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at })]);
    }
    sim.simulate(scene.events.iter().cloned());
    sim.simulate([iced_core::Event::Window(
        iced_core::window::Event::RedrawRequested(iced_core::time::Instant::now()),
    )]);
    sim
}

fn survey(window: &Window, drawn: Drawn, scene: &testing::Scene) -> Survey {
    let mut survey = Survey::default();
    simulator(window, drawn, scene).operate(&mut survey);
    // Only what shows in the window, as the canvas lists it: a form scrolled past its end lists nothing below.
    let size = drawn.size();
    let inside = |seen: &Seen| {
        let b = seen.bounds;
        b.width > 0.0 && b.height > 0.0 && b.y + b.height > 0.0 && b.x < size.width && b.y < size.height
    };
    survey.texts.retain(inside);
    survey.controls.retain(inside);
    survey
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn entry(found: &Seen, name: &str) -> String {
    let b = found.bounds;
    format!(
        "{{\"kind\":\"{}\",\"{name}\":\"{}\",\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}",
        found.kind,
        escape(&found.text),
        b.x.round(),
        b.y.round(),
        b.width.round(),
        b.height.round()
    )
}

fn json(state: &str, scheme: &str, survey: &Survey) -> String {
    let texts: Vec<String> = survey.texts.iter().map(|found| entry(found, "text")).collect();
    let controls: Vec<String> = survey.controls.iter().map(|found| entry(found, "label")).collect();
    format!(
        "{{\"state\":\"{state}\",\"scheme\":\"{scheme}\",\"texts\":[\n{}\n],\"controls\":[\n{}\n]}}\n",
        texts.join(",\n"),
        controls.join(",\n")
    )
}

/// The `from` and `control` of each of the canvas's `{from, control, to}` edges, read without a JSON parser.
fn edges(file: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(file).unwrap();
    let field = |object: &str, key: &str| -> Option<String> {
        let start = object.find(&format!("\"{key}\": \""))? + key.len() + 5;
        let rest = object.get(start..)?;
        let mut value = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(value),
                '\\' => value.push(chars.next()?),
                c => value.push(c),
            }
        }
        None
    };
    text.split('}').filter_map(|object| Some((field(object, "from")?, field(object, "control")?))).collect()
}

/// What names a screen: its words, without what was typed into its fields.
fn words(survey: &Survey) -> BTreeSet<String> {
    survey.texts.iter().filter(|found| found.kind == "text").map(|found| found.text.clone()).collect()
}

fn similarity(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let both = a.intersection(b).count();
    let either = a.len() + b.len() - both;
    if either == 0 { 1.0 } else { both as f64 / either as f64 }
}

/// Where `control` leads from `screen`: the screen of the table whose words the result shares most.
fn follow(screen: &Screen, drawn: Drawn, control: &str, prints: &BTreeMap<&str, BTreeSet<String>>) -> String {
    let mut window = made(screen);
    let rested = staged(screen.name, &window, drawn);
    let before = survey(&window, drawn, &rested);
    let messages: Vec<WindowMessage> = {
        let mut sim = simulator(&window, drawn, &rested);
        if let Some(key) = control.strip_prefix("key ") {
            let named = match key {
                "Escape" => iced_core::keyboard::key::Named::Escape,
                _ => iced_core::keyboard::key::Named::Enter,
            };
            sim.simulate([testing::key_press(iced_core::keyboard::Key::Named(named), false)]);
        } else {
            let Some(found) = before.controls.iter().find(|found| found.text == control) else {
                return "missing".into();
            };
            // At the control's own centre: a point selector would pick the outermost box holding it.
            sim.point_at(found.bounds.center());
            let _ = sim.simulate(iced_test::simulator::click());
        }
        sim.drain().collect()
    };
    for message in messages {
        window.update(message, moment());
    }
    let after = words(&survey(&window, drawn, &testing::Scene::default()));
    let best = prints.iter().map(|(name, print)| (*name, similarity(&after, print))).max_by(|a, b| a.1.total_cmp(&b.1));
    match best {
        Some((name, score)) if score >= 0.8 => name.to_string(),
        // An unnamed screen by all its words, so the canvas's heading for it can be found among them.
        _ => format!("? {}", after.into_iter().collect::<Vec<_>>().join(" | ")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = args.first().map(PathBuf::from) else {
        panic!("usage: design OUT [--states a,b] [--size WxH] [--flows CANVAS_OUT/flows.json]");
    };
    let flag = |name: &str| args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1)).cloned();
    let chosen: Option<Vec<String>> = flag("--states").map(|list| list.split(',').map(str::to_string).collect());
    let wanted = |name: &str| chosen.as_ref().is_none_or(|list| list.iter().any(|each| each == name));
    std::fs::create_dir_all(&out).unwrap();
    for (scheme, word) in [(Scheme::Dark, "dark"), (Scheme::Light, "light")] {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (screen, drawn) in every().filter(|(screen, _)| wanted(screen.name)) {
            let window = made(screen);
            let scene = staged(screen.name, &window, drawn);
            let picture =
                testing::render_scene::<WindowMessage>(&theme, drawn.view(&window), drawn.size(), 1.0, &scene);
            testing::save(&out.join(format!("{}-{word}.png", screen.name)), &picture).unwrap();
            let found = survey(&window, drawn, &scene);
            std::fs::write(out.join(format!("{}-{word}.json", screen.name)), json(screen.name, word, &found)).unwrap();
            println!("{} {word}: {} texts, {} controls", screen.name, found.texts.len(), found.controls.len());
        }
    }
    let not_built: Vec<String> =
        NOT_BUILT.iter().map(|(state, phase)| format!("{{\"state\":\"{state}\",\"phase\":\"{phase}\"}}")).collect();
    std::fs::write(out.join("not-built.json"), format!("[{}]\n", not_built.join(",\n"))).unwrap();
    let Some(file) = flag("--flows") else { return };
    let prints: BTreeMap<&str, BTreeSet<String>> = every()
        .map(|(screen, drawn)| {
            let window = made(screen);
            (screen.name, words(&survey(&window, drawn, &staged(screen.name, &window, drawn))))
        })
        .collect();
    let mut lines = Vec::new();
    for (from, control) in edges(Path::new(&file)) {
        let Some((screen, drawn)) = every().find(|(screen, _)| screen.name == from) else { continue };
        let to = follow(screen, drawn, &control, &prints);
        lines.push(format!(
            "{{\"from\":\"{}\",\"control\":\"{}\",\"to\":\"{}\"}}",
            escape(&from),
            escape(&control),
            escape(&to)
        ));
    }
    std::fs::write(out.join("flows.json"), format!("[\n{}\n]\n", lines.join(",\n"))).unwrap();
}

/// The picture the host holds for Night Owls.
const OWL: proto::files::FileId = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);

/// How far Overview's picture has gone.
enum PictureStage {
    Held,
    Uploading,
}

/// `window` with Night Owls's picture said and held by the host.
fn pictured(mut window: Window) -> Window {
    let owls = window.places()[0].id();
    let identity = terms::ServerIdentity { name: Some("Night Owls".into()), colour: None, picture: Some(OWL) };
    let offers = screens::Offers { served: screens::Served::read(&[]), limits: window.places()[0].file_limits() };
    at(&mut window, owls, PlaceMessage::Server(screens::ServerMessage::ServerChanged { identity, offers }));
    at(&mut window, owls, PlaceMessage::Server(screens::ServerMessage::PictureReady(OWL)));
    window
}

/// Server settings › Overview: the picture held, a new one going up, or (`None`) no picture and one refused as too big.
fn overview_picture(stage: Option<&PictureStage>) -> Window {
    use screens::picture_row::{Message as Row, Picked};
    let window = confirm(Pick::Settings, true);
    let mut window = if stage.is_some() { pictured(window) } else { window };
    let owls = window.places()[0].id();
    greeted_builds(&mut window);
    serves(&mut window, owls);
    if matches!(stage, Some(PictureStage::Held)) {
        return window;
    }
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Picture(Row::Choose)));
    at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Picture(Row::Picked(Picked::File(vec![0; 16])))));
    if stage.is_none() {
        let refusal = proto::files::FileRefusal::TooLarge;
        let big = terms::FileError::Refused { kind: proto::files::FileKind::ServerPicture, refusal };
        at(&mut window, owls, PlaceMessage::Page(screens::PageMessage::Picture(Row::Uploaded(Err(big)))));
    }
    window
}

/// Night Owls's owner asked to set it up, with a picture chosen and sent, kept until Save.
fn owner_first_picture() -> Window {
    use screens::picture_row::{Message as Row, Picked};
    let mut window = owner_first(false);
    for row in [Row::Choose, Row::Picked(Picked::File(vec![0; 16])), Row::Uploaded(Ok(OWL))] {
        window.update(WindowMessage::Dialog(DialogMessage::SetUp(SetUpMessage::Picture(row))), moment());
    }
    window
}

/// Where Ada stands as the owner, else as an administrator.
/// Night Owls owned by Ada, in Lounge, and `then` done.
fn owning(then: fn(&mut Window, PlaceId)) -> Window {
    let mut window = both(in_lounge_reading);
    let owls = window.places()[0].id();
    now_holding(&mut window, owls, proto::rules::Place::OWNER, every_word());
    // Becoming the owner says so in a toast; opening the server puts it away.
    window.update(WindowMessage::Open(owls), moment());
    then(&mut window, owls);
    window
}

fn channel_key(id: &str) -> screens::ListKey {
    screens::ListKey::Channel(screens::harness::channel_id(id))
}

fn category_key(id: &str) -> screens::ListKey {
    screens::ListKey::Category(terms::CategoryId::parse(id).expect("an id"))
}

/// Projects folded with #game-night open: it stays under its header.
fn folded(window: &mut Window, owls: PlaceId) {
    let projects = terms::CategoryId::parse("projects").expect("an id");
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Fold(projects)));
    let open = screens::DiscussionMessage::Open(screens::harness::channel_id("game-night"));
    at(window, owls, PlaceMessage::Discussion(open));
}

/// The Quiet corner dragged into Projects, refused twice as the list changed meanwhile.
fn move_refused(window: &mut Window, owls: PlaceId) {
    let moved = screens::Moved { key: channel_key("corner"), before: Some(channel_key("game-night")) };
    let changed = || Err(terms::LinkError::Refused(ErrorCode::ListChanged));
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Move(moved)));
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Moved(changed())));
    // The owner's list, read again: #staff with the rest.
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(with_staff(channels_of(&night_owls()))))));
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Moved(changed())));
}

fn list_menu(window: &mut Window, owls: PlaceId, key: screens::ListKey) {
    window.update(WindowMessage::ListMenu(owls, key), moment());
}

/// `pick` chosen in the menu of the row `key`.
fn list_pick(window: &mut Window, owls: PlaceId, key: screens::ListKey, pick: screens::ListPick) {
    list_menu(window, owls, key.clone());
    window.update(WindowMessage::ListPick(owls, key, pick), moment());
}

/// Create category from the server's menu, "Events" typed.
fn category_create(window: &mut Window, owls: PlaceId) {
    window.update(WindowMessage::CreateCategory(owls), moment());
    window.update(WindowMessage::Dialog(DialogMessage::Named("Events".into())), moment());
}

/// An empty category, Archive, made; then deleting it asked.
fn archive_deleted(window: &mut Window, owls: PlaceId) {
    let mut listed = channels_of(&night_owls());
    listed.categories.push(screens::Category {
        id: terms::CategoryId::parse("archive").expect("an id"),
        name: "Archive".into(),
        position: 2,
    });
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(listed))));
    list_pick(window, owls, category_key("archive"), screens::ListPick::DeleteCategory);
}

/// #staff, private and holding no message yet, open by its owner outside a call.
fn chat_private_empty() -> Window {
    in_channel("staff", Vec::new(), |window, owls| {
        now_holding(window, owls, proto::rules::Place::OWNER, every_word());
        window.update(WindowMessage::Open(owls), moment());
    })
}

fn toggle_members(window: &mut Window) {
    told_the_room(window);
    window.update(WindowMessage::ToggleMembers, moment());
}

/// The members column shown, as the canvas has it open: docked where the window has room for it, else over the content.
fn members_shown(window: &mut Window, _owls: PlaceId) {
    let room = told_the_room(window);
    if window.frame(room).members.is_none() {
        window.update(WindowMessage::ToggleMembers, moment());
    }
}

/// `window` told the room the canvas's window gives it, which a toggle needs to know whether the column docks.
fn told_the_room(window: &mut Window) -> Room {
    let size = window_size();
    let room = Room { width: size.width, height: size.height };
    window.set_room(room);
    room
}

/// The members column dragged to its widest and let go.
fn members_widest(window: &mut Window) {
    window.update(WindowMessage::MembersWidth(PanePick::To(design::MEMBERS_MOST)), moment());
    window.update(WindowMessage::MembersWidth(PanePick::Let), moment());
}

/// `who`'s popover opened from `from`, grace holding Admin, Game hosts and Helpers.
fn person(window: &mut Window, owls: PlaceId, who: &str, from: screens::PersonFrom) {
    people(window, owls, &[ADMIN_ROLE, HOSTS_ROLE, HELPERS_ROLE], false);
    window.update(WindowMessage::Person(owls, screens::harness::member(who), from), moment());
}

/// Cy, offline and a helper, opened from the members column.
fn cy_opened(window: &mut Window, owls: PlaceId) {
    people(window, owls, &[HOSTS_ROLE, HELPERS_ROLE], true);
    window.update(WindowMessage::Person(owls, screens::harness::member("cy"), screens::PersonFrom::Members), moment());
}

/// Bo holding no role but the default one, opened from the members column.
fn member_opened(window: &mut Window, owls: PlaceId) {
    people(window, owls, &[], false);
    let held = screens::PeopleMessage::Roles { user_id: screens::harness::member("bo"), roles: Vec::new() };
    at(window, owls, PlaceMessage::People(held));
    window.update(WindowMessage::Person(owls, screens::harness::member("bo"), screens::PersonFrom::Members), moment());
}

/// Ada an administrator outside the call, grace a moderator in Lounge: her popover from her seat in the pane.
fn mod_pane() -> Window {
    let mut window = both(books_signed_in);
    let owls = window.places()[0].id();
    now_holding(&mut window, owls, screens::harness::ADMIN_PLACE, every_word());
    window.update(WindowMessage::Open(owls), moment());
    people(&mut window, owls, &[MODS_ROLE, HOSTS_ROLE, HELPERS_ROLE], false);
    let people = occupants_placed(&["grace", "Bo", "mira"]);
    let lounge = screens::harness::channel_id("lounge");
    at(&mut window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Occupancy { channel_id: lounge, people }));
    window.update(WindowMessage::Person(owls, screens::harness::member("grace"), screens::PersonFrom::Seat), moment());
    window
}

/// Taking grace out refused: she left the call first.
fn mod_refused(window: &mut Window, owls: PlaceId) {
    now_holding(window, owls, screens::harness::ADMIN_PLACE, every_word());
    window.update(WindowMessage::Open(owls), moment());
    let left = RoomEvent::MemberLeft { user_id: screens::harness::member("grace") };
    at(window, owls, PlaceMessage::Call(screens::CallMessage::Room(left)));
    let refused = Err(terms::LinkError::Refused(ErrorCode::NotFound));
    let told = screens::ServerMessage::Moderated {
        user_id: screens::harness::member("grace"),
        act: screens::Moderation::TakeOut,
        answer: refused,
    };
    at(window, owls, PlaceMessage::Server(told));
}

/// The people in a voice channel this account is not in, each at the bottom of the roles' order.
fn occupants_placed(names: &[&str]) -> Vec<screens::Occupant> {
    names
        .iter()
        .map(|name| screens::Occupant {
            user_id: screens::harness::member(&name.to_lowercase()),
            colour: None,
            name: (*name).into(),
            state: choices::State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        })
        .collect()
}

fn place_of(owner: bool) -> proto::rules::Place {
    if owner { proto::rules::Place::OWNER } else { screens::harness::ADMIN_PLACE }
}

/// The server greeting with every switch on.
fn serves(window: &mut Window, owls: PlaceId) {
    let served = screens::Served::read(&[]);
    at(window, owls, PlaceMessage::Server(screens::ServerMessage::Serves(served)));
}

fn talk(window: &mut Window, owls: PlaceId, message: screens::TalkMessage) {
    at(window, owls, PlaceMessage::Talk(message));
}

/// Ada, grace and mira shown online and Bo offline, as a presence change tells them.
fn grace_online_bo_not() -> Vec<(terms::MemberId, proto::status::Shown)> {
    use proto::status::Shown;
    let online = ["ada", "grace", "mira"].map(|id| (screens::harness::member(id), Shown::Online));
    online.into_iter().chain([(screens::harness::member("bo"), Shown::Offline)]).collect()
}

/// The people of the canvas's conversations, grace online and Bo not.
fn talkers(window: &mut Window, owls: PlaceId) {
    people(window, owls, &[ADMIN_ROLE, HOSTS_ROLE, HELPERS_ROLE], true);
    let presence = screens::PeopleMessage::Presence { generation: 5, shown: grace_online_bo_not() };
    at(window, owls, PlaceMessage::People(presence));
    let lounge = screens::harness::channel_id("lounge");
    let people = occupants_placed(&["grace", "mira"]);
    at(window, owls, PlaceMessage::Lobby(screens::LobbyMessage::Occupancy { channel_id: lounge, people }));
}

/// A conversation with `with` (none for one whose other member is gone), `unread` of it unread, and its newest message
/// as `last` says: who wrote it, its text and when.
fn conversation_of(
    id: &str,
    with: Option<&str>,
    unread: u32,
    last: Option<(&str, &str, i64)>,
) -> proto::chatkord_v1::Conversation {
    let timeline = format!("t-{id}");
    let mut user_ids = vec![ME.to_owned()];
    user_ids.extend(with.map(str::to_owned));
    proto::chatkord_v1::Conversation {
        conversation_id: id.into(),
        timeline_id: timeline.clone(),
        user_ids,
        revision: 1,
        standing: Some(proto::chatkord_v1::TimelineStanding {
            timeline_id: timeline,
            read: 1,
            unread,
            mentions: unread,
            followed: false,
        }),
        last: last.map(|(from, text, at)| proto::chatkord_v1::Preview {
            user_id: Some(from.to_owned()),
            text: text.into(),
            created_at: at,
        }),
        sendable: true,
    }
}

fn conversations_listed(window: &mut Window, owls: PlaceId, listed: Vec<proto::chatkord_v1::Conversation>) {
    let told = proto::chatkord_v1::Conversations { conversations: listed, more: false };
    talk(window, owls, screens::TalkMessage::Conversations(Ok(told)));
    talk(window, owls, screens::TalkMessage::Followed(Ok(proto::chatkord_v1::FollowedThreads::default())));
}

/// Conversations with grace (two unread) and with Bo (the last words Ada's), listed and shown.
fn dm_list(window: &mut Window, owls: PlaceId) {
    serves(window, owls);
    talkers(window, owls);
    let listed = vec![
        conversation_of("c-grace", Some("grace"), 2, Some(("grace", "are you still up for tonight?", tonight(21, 12)))),
        conversation_of("c-bo", Some("bo"), 0, Some((ME, "see you at the table", tonight(20, 15)))),
    ];
    conversations_listed(window, owls, listed);
    talk(window, owls, screens::TalkMessage::Show(screens::Roll::Conversations));
}

fn dm_list_empty(window: &mut Window, owls: PlaceId) {
    serves(window, owls);
    conversations_listed(window, owls, Vec::new());
    talk(window, owls, screens::TalkMessage::Show(screens::Roll::Conversations));
}

/// The conversation with `who` opened from the list, its messages read.
fn dm_open(window: &mut Window, owls: PlaceId, who: &str) {
    dm_open_with(window, owls, who, Vec::new());
}

/// grace's conversation with her last message marking #game-night, which Ada sees, and #staff, which she does not:
/// a link and its text alone.
fn dm_marks(window: &mut Window, owls: PlaceId) {
    let text = "rules are in #game-night, the seating plan in #staff";
    let marks = [
        ("#game-night", screens::chat::MarkKind::Channel(screens::harness::channel_id("game-night"))),
        ("#staff", screens::chat::MarkKind::Channel(screens::harness::channel_id("staff"))),
    ];
    dm_open_with(window, owls, "grace", vec![marked(5, "grace", tonight(21, 14), text, &marks)]);
}

/// grace's conversation, a `#` typed: the channels Ada sees.
fn dm_channels(window: &mut Window, owls: PlaceId) {
    status(window, owls, proto::status::Choice::Online, None);
    dm_open(window, owls, "grace");
    let typed = screens::DiscussionMessage::Draft("the rules are in #".into());
    at(window, owls, PlaceMessage::Discussion(typed));
}

/// The conversation with `who` opened from the list, its messages read, `more` after grace's.
fn dm_open_with(window: &mut Window, owls: PlaceId, who: &str, more: Vec<screens::chat::Message>) {
    serves(window, owls);
    talkers(window, owls);
    let id = format!("c-{who}");
    let timeline = screens::harness::timeline_id(&format!("t-{id}"));
    conversations_listed(window, owls, vec![conversation_of(&id, Some(who), 0, None)]);
    talk(window, owls, screens::TalkMessage::Show(screens::Roll::Conversations));
    talk(window, owls, screens::TalkMessage::OpenConversation(terms::ConversationId::parse(&id).unwrap()));
    let messages = if who == "grace" {
        vec![
            said(1, "grace", tonight(18, 20) - 24 * 3_600, "did you get the map I sent?"),
            said(2, ME, tonight(18, 24) - 24 * 3_600, "yes, printing it tonight"),
            said(3, "grace", tonight(21, 10), "sent you the map"),
            said(4, "grace", tonight(21, 10) + 30, "are you still up for tonight?"),
        ]
    } else {
        Vec::new()
    };
    let messages: Vec<_> = messages.into_iter().chain(more).collect();
    let revision = messages.iter().map(|message| message.revision).max().unwrap_or(4);
    let page = screens::chat::Page { messages, older: false, newer: false, revision };
    at(
        window,
        owls,
        PlaceMessage::Discussion(screens::DiscussionMessage::Page { timeline, end: None, page: Ok(page) }),
    );
}

/// Cy left the server: their conversation stays readable and takes no more messages.
fn dm_gone(window: &mut Window, owls: PlaceId) {
    serves(window, owls);
    talkers(window, owls);
    let timeline = screens::harness::timeline_id("t-c-cy");
    conversations_listed(window, owls, vec![conversation_of("c-cy", Some("cy"), 0, None)]);
    at(window, owls, PlaceMessage::People(screens::PeopleMessage::Left(screens::harness::member("cy"))));
    talk(window, owls, screens::TalkMessage::Show(screens::Roll::Conversations));
    talk(window, owls, screens::TalkMessage::OpenConversation(terms::ConversationId::parse("c-cy").unwrap()));
    let messages = vec![
        said(1, "cy", tonight(17, 2) - 3 * 24 * 3_600, "thanks for the invite"),
        said(2, ME, tonight(17, 5) - 3 * 24 * 3_600, "any time"),
    ];
    // Older messages are still to read, as the canvas has this conversation: its start is not shown yet.
    let page = screens::chat::Page { messages, older: true, newer: false, revision: 2 };
    at(
        window,
        owls,
        PlaceMessage::Discussion(screens::DiscussionMessage::Page { timeline, end: None, page: Ok(page) }),
    );
}

/// grace's message at 21:02 has a thread of three replies, the last at 21:14, which this account follows with two unread.
fn threaded(window: &mut Window, owls: PlaceId) {
    let mut root = general().into_iter().find(|message| message.place == 9).expect("grace's question");
    root.revision = 40;
    root.thread = Some(screens::chat::ThreadSummary {
        timeline: screens::harness::timeline_id("t-thread"),
        replies: 3,
        last_reply_at: Some(tonight(21, 14)),
    });
    at(window, owls, screens::harness::told(root));
    let followed = proto::chatkord_v1::FollowedThread {
        standing: Some(proto::chatkord_v1::TimelineStanding {
            timeline_id: "t-thread".into(),
            read: 1,
            unread: 2,
            mentions: 0,
            followed: true,
        }),
        channel_id: "general".into(),
        root_message_id: "m9".into(),
        root: Some(proto::chatkord_v1::Preview {
            user_id: Some("grace".into()),
            text: "anyone up for a game later?".into(),
            created_at: tonight(21, 2),
        }),
        last_reply_at: Some(tonight(21, 14)),
        replies: 3,
    };
    let told = proto::chatkord_v1::FollowedThreads { threads: vec![followed] };
    talk(window, owls, screens::TalkMessage::Followed(Ok(told)));
}

fn thread_chip(window: &mut Window, owls: PlaceId) {
    threaded(window, owls);
}

/// The thread opened from its chip, its replies read, and followed.
fn thread_open(window: &mut Window, owls: PlaceId) {
    threaded(window, owls);
    let read = screens::TalkMessage::Standing {
        timeline: screens::harness::timeline_id("t-thread"),
        standing: screens::TimelineStanding { read: 3, unread: 0, mentions: 0, followed: true },
    };
    talk(window, owls, read);
    talk(window, owls, screens::TalkMessage::OpenThread(message_id(9)));
    let replies = vec![
        said(1, "bo", tonight(21, 4), "I can host."),
        said(2, "mira", tonight(21, 9), "I’ll bring the map pack"),
        said(3, "bo", tonight(21, 14), "door’s open from eight"),
    ];
    let page = screens::chat::Page { messages: replies, older: false, newer: false, revision: 3 };
    let timeline = screens::harness::timeline_id("t-thread");
    at(window, owls, PlaceMessage::Thread(screens::DiscussionMessage::Page { timeline, end: None, page: Ok(page) }));
}

/// "Reply in thread" on grace's last message, which has none, beside the channel whose question has one: the pane
/// opens on it with its composer, nothing sent.
fn thread_new(window: &mut Window, owls: PlaceId) {
    threaded(window, owls);
    talk(window, owls, screens::TalkMessage::StartThread(message_id(16)));
}

fn thread_list(window: &mut Window, owls: PlaceId) {
    threaded(window, owls);
    talk(window, owls, screens::TalkMessage::Show(screens::Roll::Threads));
}

fn thread_list_empty(window: &mut Window, owls: PlaceId) {
    serves(window, owls);
    conversations_listed(window, owls, Vec::new());
    talk(window, owls, screens::TalkMessage::Show(screens::Roll::Threads));
}

/// Settings › Notifications on the voice settings' window; `quiet` has the account's status Do not disturb for an hour
/// from 21:30.
fn notifications_quiet(quiet: bool) -> Window {
    let mut window = both(in_lounge);
    let owls = window.places()[0].id();
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Notifications), moment());
    if quiet {
        status(&mut window, owls, proto::status::Choice::DoNotDisturb, Some(tonight(22, 30)));
    }
    window
}

/// The account's status `choice` until `until`.
fn status(window: &mut Window, owls: PlaceId, choice: proto::status::Choice, until: Option<i64>) {
    let chosen = proto::status::Chosen { choice, until };
    at(window, owls, PlaceMessage::Talk(screens::TalkMessage::StatusChosen(chosen)));
}

/// The account's own popover opened from its bar, its status `choice` until `until`.
fn own_status(window: &mut Window, owls: PlaceId, choice: proto::status::Choice, until: Option<i64>) {
    status(window, owls, choice, until);
    person(window, owls, ME, screens::PersonFrom::AccountBar);
}

/// grace away and Bo not to be disturbed, as everyone is shown them.
fn members_status(window: &mut Window, owls: PlaceId) {
    status(window, owls, proto::status::Choice::Online, None);
    shown(window, owls, &[("grace", proto::status::Shown::Away), ("bo", proto::status::Shown::DoNotDisturb)]);
}

/// The server telling each member's status as everyone is shown it.
fn shown(window: &mut Window, owls: PlaceId, statuses: &[(&str, proto::status::Shown)]) {
    let shown = statuses.iter().map(|(who, shown)| (screens::harness::member(who), *shown)).collect();
    let told = screens::PeopleMessage::Presence { generation: 6, shown };
    at(window, owls, PlaceMessage::People(told));
}

/// Three servers on the rail: three unread that mention Ada on the first, some that do not alert on the second, and the
/// third open and read.
fn badges() -> Window {
    let rows = vec![
        Opening::Account(night_owls(), ADA.into()),
        Opening::Account(book_club(), ADA.into()),
        Opening::Account(plant_swap(), ADA.into()),
    ];
    let (mut window, _) = Window::launch(rows, None, Instant::now());
    let ids: Vec<PlaceId> = window.places().iter().map(screens::Place::id).collect();
    for (id, server) in ids.iter().zip([night_owls(), book_club(), plant_swap()]) {
        at(&mut window, *id, flow(token()));
        let standing = screens::harness::standing_as(ME);
        at(&mut window, *id, flow(Report::Accepted { token: None, named: None, standing }));
        at(&mut window, *id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(channels_of(&server)))));
    }
    for (id, channel, unread, mentions) in [(ids[0], "game-night", 3, 3), (ids[1], "reading", 1, 0)] {
        let standing = screens::ChannelStanding { grants: member_words(), read: 0, unread, mentions };
        let told = screens::LobbyMessage::Standing { channel_id: screens::harness::channel_id(channel), standing };
        at(&mut window, id, PlaceMessage::Lobby(told));
    }
    for id in &ids {
        serves(&mut window, *id);
    }
    let plants = ids[2];
    window.update(WindowMessage::Open(plants), moment());
    window.update(WindowMessage::Zone(screens::chat::TimeZone::UTC), moment());
    window.update(WindowMessage::Today(screens::chat::Date::constant(2026, 11, 12)), moment());
    let named = |name: &str| screens::Named { name: name.into(), colour: None, roles: Vec::new(), picture: None };
    let (here, away) = (proto::status::Shown::Online, proto::status::Shown::Offline);
    let entries = [(ME, ADA, here), ("bo", "Bo", here), ("grace", "grace", here), ("mira", "mira", away)]
        .map(|(id, name, state)| (screens::harness::member(id), named(name), state))
        .into();
    let page = screens::PeoplePage { entries, next: None, presence: 1 };
    at(&mut window, plants, PlaceMessage::People(screens::PeopleMessage::Page(Ok(page))));
    let mut bo = occupants_placed(&["Bo"]);
    bo[0].state = choices::State { mic_muted: true, deafened: false };
    let shed = screens::harness::channel_id("shed");
    at(&mut window, plants, PlaceMessage::Lobby(screens::LobbyMessage::Occupancy { channel_id: shed, people: bo }));
    for channel in ["general", "swaps"] {
        let standing = screens::ChannelStanding { grants: member_words(), read: 1, unread: 0, mentions: 0 };
        let told = screens::LobbyMessage::Standing { channel_id: screens::harness::channel_id(channel), standing };
        at(&mut window, plants, PlaceMessage::Lobby(told));
    }
    let general = screens::harness::channel_id("general");
    at(&mut window, plants, PlaceMessage::Discussion(screens::DiscussionMessage::Open(general)));
    let messages = vec![said(1, "bo", tonight(10, 2), "anyone want tomato seedlings?")];
    let page = screens::chat::Page { messages, older: false, newer: false, revision: 1 };
    let first =
        screens::DiscussionMessage::Page { timeline: screens::harness::general_timeline(), end: None, page: Ok(page) };
    at(&mut window, plants, PlaceMessage::Discussion(first));
    window
}
