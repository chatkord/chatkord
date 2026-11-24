//! The client's components on iced: they map `design`'s looks and `screens`' decisions onto widgets and decide
//! neither, and depend on none of the engine, so their tests build without the media stack.
#![forbid(unsafe_code)]
#![recursion_limit = "256"]

mod about_page;
pub mod access;
mod access_card;
mod account_page;
mod add_menu;
pub mod app;
pub mod app_icon;
mod appearance_page;
mod at_most;
mod attachments;
mod audio_controls;
mod avatar;
mod badge;
mod bar_controls;
mod beside;
mod body_text;
mod button;
mod call;
mod call_button;
mod callout;
mod card_bar;
mod category_select;
mod centre_panel;
mod channel_list;
mod channels_page;
mod choice;
mod chooser;
mod clipboard;
mod color;
mod colour_picker;
mod composer;
mod context;
mod copy_field;
mod custom_choice;
mod device_menu;
mod dialog;
mod dialogs;
mod discussion;
pub mod door;
mod drawn;
mod drop_target;
mod edge_scroll;
mod emoji_page;
mod emoji_picker;
mod end_card;
mod escape;
mod field_message;
mod floating;
mod focus;
mod fold;
pub mod fonts;
mod full_screen;
pub mod host;
mod hosting_form;
mod hover_bar;
mod icons;
mod identity;
mod input;
mod keep_at;
mod labelled;
mod labels;
mod language_field;
mod layer;
mod level_bar;
mod line_graph;
mod link_preview;
mod list_row;
mod members_pane;
mod menu;
mod notifications_page;
mod over_field;
mod panel;
mod panel_parts;
mod password_dialog;
mod people;
mod person_popover;
pub mod picture;
mod picture_row;
mod pins_panel;
mod pop_out;
mod preedit;
mod radio;
mod rail;
mod rail_mark;
mod reactions;
mod reorder;
mod reply_bar;
mod report_dialog;
mod reveal;
mod rich_body;
mod role_badge;
mod rows;
mod screen_card;
mod search_bar;
mod search_field;
mod search_layer;
mod search_view;
mod seat;
mod secret;
mod select;
mod server_settings;
mod settings;
mod settings_page;
mod share_card;
mod share_panel;
mod share_quality;
mod slider;
mod source_picker;
mod spinner;
mod splitter;
mod startup_page;
mod state_line;
mod stats_popover;
mod status_dot;
mod status_mark;
mod switch;
mod switch_panel;
mod table;
mod talk_lists;
mod text_box;
mod theme;
mod thread_chip;
mod tile;
mod toast;
mod tooltip;
mod typography;
mod update_notice;
mod viewer;
mod voice_settings;
mod volume_row;
pub mod watch;
mod webhooks_page;
mod whom;
mod wrap;
mod zoom_keys;
mod zoomed;

#[cfg(feature = "testing")]
pub mod testing;

pub use attachments::{FileCard, Pending, PendingState, Picture, attach_button, file_card, pending_strip, pictures};
pub use audio_controls::{AudioWords, SoundRow, audio_controls};
pub use avatar::{AvatarKind, Held, avatar, avatar_pictured};
pub use badge::badge;
pub(crate) use beside::beside;
pub use button::{Button, ButtonClass, RowTone, pointer_press};
pub use call_button::{CallButton, CallSize, bar_divider, stage_bar};
pub(crate) use callout::callouts_above;
pub use callout::{CalloutParts, callout};
pub use card_bar::{Tool, card_bar, stage_toggle};
pub(crate) use centre_panel::{Face, centre_panel};
pub use centre_panel::{centre, centre_small};
pub use choice::choice;
pub(crate) use choice::choice_track;
pub use chooser::{ChooserGroup, ChooserKeys, ChooserLeading, ChooserRow, chooser, chooser_keys};
pub use clipboard::{ClipboardReader, ClipboardWriter, clipboard_reader, clipboard_writer};
pub(crate) use colour_picker::colour_picker;
pub use composer::{Composer, EditorClass};
pub use copy_field::copy_field;
pub(crate) use device_menu::device_menu_over;
pub use device_menu::{DeviceSection, device_menu};
pub(crate) use dialog::{alert_panel, dialog_frame};
pub use dialog::{dialog, dialog_panel};
pub use drop_target::drop_over;
pub use emoji_page::{EmojiCard, Renaming, emoji_action, emoji_page};
pub use emoji_picker::{EmojiPicker, EmojiSends, PickedEmoji, PickerSection, ServerEmoji, emoji_picker};
pub use end_card::end_card;
pub(crate) use escape::on_escape;
pub use field_message::field_said;
pub(crate) use field_message::{field_message, refusal, warning};
pub(crate) use focus::Focus;
pub use focus::FocusScope;
pub(crate) use fold::fold;
pub use hover_bar::{HoverTools, hover_bar};
pub use icons::{Icon, IconClass, icon};
pub use input::{InputClass, TextInput};
pub(crate) use labelled::Labelled;
pub(crate) use language_field::language_field;
pub use level_bar::{level_bar, level_bar_marked};
pub use line_graph::{GraphWords, line_graph};
pub use link_preview::{CardPicture, LinkCard, PreviewBar, link_card, preview_bar};
pub use list_row::ListRow;
pub use members_pane::{MemberLine, MemberLines, MemberRow, members_pane};
pub use menu::{Leading, MenuChoice, MenuEntry, drawer, menu_at, menu_at_pointer, menu_items};
pub use panel::panel;
pub use person_popover::{
    Heard, HeldRole, PersonWords, Presence, StatusChoice, StatusChoices, app_popover, person_popover,
};
pub use pins_panel::{Pin, pins_panel};
pub use preedit::{Composing, Preedit, PreeditColors};
pub use rail::{RailItem, add_tile, gear_tile};
pub use reactions::{ReactionChip, ReactionEmoji, reactions};
pub use reorder::reorder;
pub use reply_bar::{Quoted, QuotedReply, Replying, quoted_reply, reply_bar};
pub use reveal::{reveal, reveal_when};
pub use rich_body::{BodyPress, RichBody};
pub use role_badge::role_badge;
pub use screen_card::{Paused, ScreenCard, screen_card};
pub use search_bar::{search_magnifier, search_pill, top_bar};
pub use search_field::search_field;
pub use search_layer::{FoundFile, SearchBody, search_layer};
pub use seat::seat_row;
pub use secret::SecretInput;
pub(crate) use secret::password_field;
pub use select::Select;
pub use settings::{Heading, SettingsGroup, SettingsPage, SettingsWays, settings_frame};
pub use settings_page::{field_label, settings_body, settings_row, settings_section};
pub use share_card::share_card;
pub use share_panel::{info_panel, share_panel};
pub use share_quality::{Bitrate, quality_rows};
pub use slider::{slider, slider_unoffered};
pub use source_picker::{Sends as PickerSends, source_picker};
pub use spinner::spinner;
pub use state_line::{STATE_LINE_ROOM, state_line};
pub use stats_popover::{stats_popover, stats_popover_over};
pub use status_mark::{person_with_status, status_mark_dot};
pub use switch::{switch, switch_noted, switch_offered};
pub use switch_panel::{SwitchRow, switch_panel};
pub use table::table;
pub(crate) use table::{ROW_PADDING, flush_card, headed_card, list_card};
pub use theme::{ProgressClass, ScrollClass, Surfaced, TextClass, Theme};
pub use thread_chip::thread_chip;
pub use tile::{voice_grid, voice_tile};
pub use toast::{ToastSends, toast};
pub use tooltip::tooltip;
pub use typography::text;
pub(crate) use typography::{coloured, line};
pub use update_notice::update_notice;
pub use viewer::{Viewed, viewer_over};
pub use volume_row::{VolumeWords, volume_row};

/// What a component needs of a renderer: text in our fonts, vector icons, decoded pictures, and paths for a graph. The wgpu window and the
/// software renderer the tests draw with both are one, so a component is written once.
pub trait Renderer:
    iced_core::text::Renderer
    + iced_core::svg::Renderer
    + iced_core::image::Renderer<Handle = iced_core::image::Handle>
    + iced_graphics::geometry::Renderer
{
}

impl<
    T: iced_core::text::Renderer
        + iced_core::svg::Renderer
        + iced_core::image::Renderer<Handle = iced_core::image::Handle>
        + iced_graphics::geometry::Renderer,
> Renderer for T
{
}
mod count_badge;
pub use count_badge::{count_badge, unread_dot};
