//! The window, drawn from the shared components and a `screens::Window`: the welcome steps, or the rail, the open
//! server's pane and content and what is over them. It holds no state and decides nothing; every word is the model's.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::call::{bar_room, call_panel, call_toggles, call_view, floating_card, next_call_toggles, zoomable};
use crate::labels::{label, phrase};
use crate::picture::{Painted, Picture};
use crate::{
    AvatarKind, Button, CallSize, CalloutParts, FocusScope, Icon, IconClass, MenuEntry, RailItem, Renderer, Surfaced,
    TextClass, Theme, ToastSends, callout, centre, coloured, icon, state_line, text, toast,
};
use choices::LookPick;
use design::{
    ACCOUNT_STRIP_HEIGHT, AvatarSize, ButtonVariant, CALL_PANEL_HEIGHT, CONTROL_HEIGHT, Content, IconSize,
    PANE_HEADER_HEIGHT, Popup, RAIL_WIDTH, Radius, Role, SemanticRole, Semantics, Space, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Size, Vector, Widget};
use iced_widget::{Column, Row, Space as Gap, button, container, scrollable, stack};
use proto::video::Video;
use screens::frame::{Frame, MembersColumn, Room, Shape, ThreadColumn};
use screens::{
    ArrivalNote, DevicePage, DialogMessage, MenuFrom, Over, Panel, Pick, Place, PlaceId, PlaceMessage, StateLine,
    Window, WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

pub use crate::discussion::Handles;

pub(crate) type Part<'a, R> = Element<'a, Message, Theme, R>;

/// What Ctrl+K sends: the open server's search opens, where it has one.
fn searching(window: &Window) -> Option<Message> {
    let place = window.open().filter(|place| crate::search_view::offered(place))?;
    Some(Message::At(place.id(), PlaceMessage::Search(screens::SearchMessage::Open)))
}

/// The window at `size`, which the grid of a call is cut to, with a plain fill wherever the host would paint.
pub fn view<'a, R: Renderer + 'a>(window: &'a Window, size: Size) -> Part<'a, R> {
    view_with(window, size, &|_| None, &|painted| match painted {
        Painted::Picture { radius, .. } => container(Gap::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .class(Surfaced::Fill {
                fill: design::Role::Surface(Surface::Inset),
                text: design::Role::Content(design::Content::Primary),
                radius,
            })
            ._boxed(),
        Painted::Screen { .. }
        | Painted::OwnScreen(_)
        | Painted::Camera { .. }
        | Painted::OwnCamera(..)
        | Painted::Still(..) => container(Gap::new()).class(Surfaced::On(Surface::Inset))._boxed(),
    })
}

/// A watched screen's own window at `size`, while one is out, with a plain fill where the host would paint.
pub fn out_view<'a, R: Renderer + 'a>(window: &'a Window, size: Size) -> Option<Part<'a, R>> {
    out_view_with(window, size, &|_| container(Gap::new()).class(Surfaced::On(Surface::Inset))._boxed())
}

/// The same, with the host's own `picture` of it.
pub fn out_view_with<'a, R: Renderer + 'a>(
    window: &'a Window,
    size: Size,
    picture: &Picture<'a, R>,
) -> Option<Part<'a, R>> {
    let out = window.pop_out()?;
    let corners = design::Radius::None;
    let user_id = out.screen.user_id.clone();
    let shown = match out.screen.video {
        Video::Screen => picture(Painted::Screen { user_id, lens: screens::Lens::WHOLE, corners }),
        Video::Camera => picture(Painted::Camera { user_id }),
    };
    let words = window.words();
    // A camera's Volume is the person's: their popover opens in this window, not the main one.
    let person_volume = window.person().filter(|_| out.screen.video == Video::Camera).and_then(|card| {
        let volume = card.volume.filter(|_| card.open.from == screens::PersonFrom::Card)?;
        (card.open.user_id == *out.screen.user_id).then(|| {
            let said = crate::PersonWords {
                name: label(&card.name),
                volume: phrase(Phrase::Volume, words),
                as_admin: phrase(Phrase::AsAnAdmin, words),
                roles: phrase(Phrase::Roles, words),
            };
            crate::people::volume_popover(out.place.id(), out.screen.user_id, volume, said, words)
        })
    });
    let view = crate::pop_out::view(out, size, shown, person_volume, words);
    // The search opens in the main window, so Ctrl+K means nothing here.
    Some(crate::zoom_keys::zoom_keys(view, |step| Message::Look(LookPick::Zoom(step)), None)._boxed())
}

/// The same, with the host's `handles` of the files it holds decoded and its own `picture` where a screen plays.
pub fn view_with<'a, R: Renderer + 'a>(
    window: &'a Window,
    size: Size,
    handles: &'a Handles<'a>,
    picture: &Picture<'a, R>,
) -> Part<'a, R> {
    let words = window.words();
    let drawn = drawn(window, size, handles, picture, words);
    crate::zoom_keys::zoom_keys(drawn, |step| Message::Look(LookPick::Zoom(step)), searching(window))._boxed()
}

fn drawn<'a, R: Renderer + 'a>(
    window: &'a Window,
    size: Size,
    handles: &'a Handles<'a>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    // A watched screen over the whole display is all the window shows.
    if let Some(full) = window.full_screen() {
        let shown = zoomable(full.place, full.zoom, |lens| {
            picture(Painted::Screen { user_id: full.screen.user_id.clone(), lens, corners: design::Radius::None })
        });
        return crate::full_screen::view(full, shown, words);
    }
    if let Some(adding) = window.welcome_door() {
        let id = adding.id();
        let report = match (window.report_refused(), window.downloaded()) {
            (true, _) => crate::door::Foot::Refused,
            (false, Some(file)) => crate::door::Foot::Downloaded(file),
            (false, None) => crate::door::Foot::Ask,
        };
        let door = crate::door::view::<R>(adding.door(), size.width, report, Some(window), words)
            ._map(move |said| match said {
                screens::Message::DebugReport => Message::Report(screens::ReportMessage::Open),
                screens::Message::Language(said) => said.into(),
                said => Message::At(id, PlaceMessage::Session(screens::SessionMessage::Door(said))),
            })
            ._boxed();
        // The report's dialog opens over the first run's window as over any other.
        let (over, on_close) = over(window, size, handles, picture, words);
        return crate::dialog(door, over, on_close);
    }
    let room = Room { width: size.width, height: size.height };
    let shaped = window.frame(room);
    let mut columns: Vec<Part<'a, R>> = Vec::new();
    if shaped.shape != Shape::Drawer {
        columns.extend([rail(window, picture, words), rule()]);
    }
    let after_rail = columns.len();
    let mut splitter = None;
    let mut members_grip = None;
    let mut members_over = None;
    let mut chat_grip = None;
    let mut chat_over = None;
    let mut drawer = None;
    // The settings take the pane's place as well as the content's.
    let shown = window.settings_shown(&shaped);
    if let Some(page) = window.device() {
        columns.push(match page {
            DevicePage::Appearance => crate::appearance_page::view(window, shown, words),
            DevicePage::VoiceAndVideo => crate::voice_settings::view(window, shown, picture, words),
            DevicePage::Notifications => crate::notifications_page::view(window, shown, words),
            DevicePage::Startup => match window.startup() {
                Some(startup) => crate::startup_page::view(window, shown, startup, words),
                None => crate::notifications_page::view(window, shown, words),
            },
            DevicePage::AboutChatkord => crate::about_page::view(window, shown, words),
        });
    } else if let Some(place) = window.open() {
        if let Some(open) = place.settings() {
            columns.push(crate::server_settings::view(window, &shaped, place, open, picture, handles, words));
        } else if let Some(open) = place.account() {
            columns.push(crate::account_page::view(window, shown, place, open, picture, words));
        } else {
            if shaped.folded() {
                drawer = shaped.drawer_open().then(|| {
                    let mut held = vec![rail(window, picture, words)];
                    if let Some(width) = shaped.pane {
                        held.extend([rule(), pane(window, place, width, handles, words)]);
                    }
                    let held = Row::with_children(held);
                    container(held)
                        .height(Length::Fill)
                        .class(Surfaced::Panel(design::Elevation::Overlay, Radius::None))
                        ._boxed()
                });
            } else if let Some(width) = shaped.pane {
                columns.extend([pane(window, place, width, handles, words), rule()]);
                splitter = Some(pane_splitter(shaped, width, words));
            }
            // What the content and the columns beside it have of the window's height: all of it less the top bar.
            let height = size.height - shaped.top;
            let thread = thread_column(place, &shaped);
            let room = Size::new(thread.map_or(shaped.content, |thread| thread.channel), height);
            match (thread, place.thread()) {
                (Some(beside), Some(open)) if beside.covers => {
                    let room = Size::new(beside.width, height);
                    columns.push(
                        container(thread_pane(window, place, open, room, true, handles, words))
                            .width(Length::Fixed(beside.width))
                            ._boxed(),
                    );
                }
                (Some(beside), Some(open)) => {
                    columns.push(content(window, place, room, &shaped, handles, picture, words));
                    let room = Size::new(beside.width, height);
                    columns.extend([
                        rule(),
                        container(thread_pane(window, place, open, room, false, handles, words))
                            .width(Length::Fixed(beside.width))
                            ._boxed(),
                    ]);
                }
                (Some(_), None) | (None, _) => {
                    columns.push(content(window, place, room, &shaped, handles, picture, words));
                }
            }
            let chat = place.call_chat().zip(shaped.call_chat);
            match chat {
                Some((open, MembersColumn::Docked(width))) => {
                    let column = call_chat(window, place, open, Size::new(width, height), handles, words);
                    columns.extend([rule(), container(column).width(Length::Fixed(width))._boxed()]);
                    chat_grip = Some(call_chat_splitter(&shaped, size.width, width, words));
                }
                Some((open, MembersColumn::Over(width))) => {
                    let column = call_chat(window, place, open, Size::new(width, height), handles, words);
                    chat_over = Some(
                        container(column)
                            .width(Length::Fixed(width))
                            .height(Length::Fill)
                            .class(Surfaced::Panel(design::Elevation::Overlay, Radius::None))
                            ._boxed(),
                    );
                }
                None => {}
            }
            let members = shaped.members.filter(|_| thread.is_none_or(|thread| thread.members));
            match members {
                Some(MembersColumn::Docked(width)) => {
                    let column = crate::people::members_column(place, handles, words);
                    columns.extend([rule(), container(column).width(Length::Fixed(width))._boxed()]);
                    members_grip = Some(members_splitter(&shaped, size.width, width, words));
                }
                Some(MembersColumn::Over(width)) => {
                    let column = crate::people::members_column(place, handles, words);
                    members_over = Some(
                        container(column)
                            .width(Length::Fixed(width))
                            .height(Length::Fill)
                            .class(Surfaced::Panel(design::Elevation::Overlay, Radius::None))
                            ._boxed(),
                    );
                }
                None => {}
            }
        }
    }
    // One region for the Tab key: the rail, the pane and the content in turn, then the floating card over them.
    // The top bar runs across the server's pane and content, which a folded window does not have room for.
    let beside = columns.split_off(after_rail);
    let bar = window.bar(room).map(|place| {
        let end = crate::bar_controls::end::<R>(place, members_shown(place, &shaped), words);
        crate::search_view::bar::<R>(place, end, words)
    });
    match bar {
        Some(bar) => columns.push(
            Column::with_children([bar, Row::with_children(beside).height(Length::Fill)._boxed()])
                .width(Length::Fill)
                .height(Length::Fill)
                ._boxed(),
        ),
        None => columns.extend(beside),
    }
    let row = Row::with_children(columns).height(Length::Fill);
    let above: Vec<Part<'a, R>> = splitter
        .into_iter()
        .chain(members_grip)
        .chain(chat_grip)
        .chain(window.floating(room).map(|card| floating_card(card, picture, words)))
        .collect();
    let row: Part<'a, R> = if above.is_empty() {
        row._boxed()
    } else {
        stack(std::iter::once(row._boxed()).chain(above)).width(Length::Fill).height(Length::Fill)._boxed()
    };
    let frame =
        container(FocusScope::new(row)).width(Length::Fill).height(Length::Fill).class(Surfaced::On(Surface::Base));
    // Folded, the rail and the pane open as one drawer over the content: the rail where it was, or back at the edge.
    let frame = crate::drawer(frame._boxed(), drawer, Message::CloseChannels);
    // Where it cannot dock, the members column opens over the content at the window's other edge.
    let frame = crate::menu::end_drawer(frame, members_over, shaped.top_room(), Message::CloseMembers);
    // So does a call's chat, over the call.
    let frame = crate::menu::end_drawer(frame, chat_over, 0.0, Message::CloseCallChat);
    // A menu is over the whole window, so it takes the focus from any control and gives it back, placed where the
    // canvas has it: under the pane's header, or beside the server's picture in the rail.
    let menu = window.menu().and_then(|(menu, _)| {
        let place = window.places().iter().position(|place| place.id() == menu.place)?;
        let at = match menu.from {
            // Where the pointer pressed the pane, placed below.
            MenuFrom::Pane => return None,
            MenuFrom::Header => Vector::new(RAIL_WIDTH + Space::S.px(), shaped.top + PANE_HEADER_HEIGHT),
            MenuFrom::Rail => Vector::new(
                RAIL_WIDTH + Space::Xs.px(),
                RAIL_TOP + place as f32 * (RailItem::<Message>::HEIGHT + RAIL_GAP) + RailItem::<Message>::PICTURE_TOP
                    - window.rail_scrolled(),
            ),
        };
        Some((menu_of(window, &window.places()[place], words)?, at))
    });
    let (items, at) = match menu {
        Some((items, at)) => (Some(items), at),
        None => (None, Vector::ZERO),
    };
    let frame: Part<'a, R> = match window.toast() {
        Some(shown) => {
            let sends = ToastSends { act: Message::ToastAct, away: Message::DismissToast, held: Message::HoldToast };
            let floated = container(toast::<Message, R>(shown, &sends, words))
                .center_x(Length::Fill)
                .align_bottom(Length::Fill)
                .padding(toast_room(window, &shaped));
            stack([frame, floated._boxed()])._boxed()
        }
        None => frame._boxed(),
    };
    let framed = crate::menu_at(frame, items, at, Message::CloseMenu);
    // The pane's own menu, where its list was pressed; from the keyboard, under the pane's header.
    let made = window.menu().filter(|(menu, _)| menu.from == MenuFrom::Pane).and_then(|(menu, _)| {
        let place = window.places().iter().find(|place| place.id() == menu.place)?;
        menu_of(window, place, words)
    });
    let under_header = Vector::new(RAIL_WIDTH + Space::S.px(), shaped.top + PANE_HEADER_HEIGHT + Space::S.px());
    let framed = crate::menu_at_pointer(framed, made, under_header, Message::CloseMenu);
    // A channel's or a category's menu, or a person's popover, beside what opened it.
    let beside = list_menu(window, &shaped, words)
        .or_else(|| crate::people::person_over(window, &shaped, room, handles, words))
        .or_else(|| crate::people::app_over(window, &shaped, words));
    let framed = match beside {
        Some((layer, side, fallback)) => {
            crate::menu::menu_beside(framed, Some(layer), side, fallback, Message::CloseMenu)
        }
        None => crate::menu::menu_beside(
            framed,
            None,
            crate::layer::Side::Below,
            iced_core::Rectangle::default(),
            Message::CloseMenu,
        ),
    };
    let framed = viewer_over(window, framed, handles, words);
    let framed = connection_over(window, framed, size, words);
    let framed = picker_over(window, framed, size, picture, words);
    let (over, on_close) = over(window, size, handles, picture, words);
    let mut layers: Vec<Part<'a, R>> = vec![crate::dialog(framed, over, on_close)];
    if let Some(address) = window.copying() {
        layers.push(
            crate::clipboard_writer(address, |written| {
                Message::Copied(written.then_some(()).ok_or(screens::NotCopied))
            })
            ._boxed(),
        );
    }
    stack(layers)._boxed()
}

/// `screen` with the picture a channel of the open place has open over it: its original once held, its thumbnail
/// until then, and a sentence where the original could not be fetched.
fn viewer_over<'a, R: Renderer + 'a>(
    window: &'a Window,
    screen: Part<'a, R>,
    handles: &'a Handles<'a>,
    words: &'a Words,
) -> Part<'a, R> {
    let Some(place) = window.open() else { return screen };
    let open = [
        (screens::Pane::Content, place.discussion()),
        (screens::Pane::CallChat, place.call_chat()),
        (screens::Pane::Thread, place.thread()),
    ]
    .into_iter()
    .find_map(|(pane, open)| Some((pane, open?.viewed()?)));
    let Some((pane, (_, file))) = open else { return screen };
    let id = place.id();
    let said = move |message: screens::DiscussionMessage| {
        Message::At(
            id,
            match pane {
                screens::Pane::Content => PlaceMessage::Discussion(message),
                screens::Pane::CallChat => PlaceMessage::CallChat(message),
                screens::Pane::Thread => PlaceMessage::Thread(message),
            },
        )
    };
    let thumbnail = file.picture.as_ref().and_then(|picture| picture.thumbnail).and_then(handles);
    let (picture, under, failed) = match place.fetched(&file.file) {
        Some(screens::Fetched::Ready) => match handles(file.file) {
            Some(original) => (Some(original), thumbnail, false),
            None => (thumbnail, None, false),
        },
        Some(screens::Fetched::Failed) => (thumbnail, None, true),
        Some(screens::Fetched::Asked) | None => (thumbnail, None, false),
    };
    let note = match (failed, file.picture.as_ref()) {
        (true, _) => words.phrase(Phrase::PictureNotFetched).into_owned(),
        (false, Some(shape)) => {
            words.say(&Sentence::PictureNote { width: shape.width, height: shape.height, size: file.size })
        }
        (false, None) => words.say(&Sentence::FileNote { size: file.size, name: file.name.as_str().to_owned() }),
    };
    let viewed = crate::Viewed {
        picture,
        under,
        shape: file.picture.as_ref().map(|shape| (shape.width, shape.height)),
        name: file.name.as_str().to_owned(),
        note,
        save: said(screens::DiscussionMessage::Save { file: file.file, name: file.name.clone() }),
    };
    crate::viewer_over(screen, Some(viewed), said(screens::DiscussionMessage::CloseViewer), words)
}

/// Our own picker of what to share, over the whole window as a dialog, while the open place's call has it open.
fn picker_over<'a, R: Renderer + 'a>(
    window: &'a Window,
    screen: Part<'a, R>,
    size: Size,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let open = window.open().and_then(|place| Some((place.id(), place.call()?.sources()?)));
    let Some((id, sources)) = open else { return screen };
    let sends = crate::source_picker::Sends {
        tab: Box::new(move |tab| Message::At(id, PlaceMessage::Call(screens::CallMessage::SourcesTab(tab)))),
        select: Box::new(move |key| Message::At(id, PlaceMessage::Call(screens::CallMessage::SelectSource(key)))),
        share: Message::At(id, PlaceMessage::Call(screens::CallMessage::ShareSource)),
        close: Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)),
    };
    let picker = crate::source_picker::<Message, R>(sources, size, sends, picture, words);
    crate::dialog(screen, Some(picker), Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)))
}

/// The popover of how the call's connection is, over the window and rising from the call panel at the foot of the pane.
fn connection_over<'a, R: Renderer + 'a>(
    window: &'a Window,
    screen: Part<'a, R>,
    size: Size,
    words: &'a Words,
) -> Part<'a, R> {
    let shown = window.open().filter(|place| place.settings().is_none() && place.account().is_none());
    let open = shown.and_then(|place| {
        let call = place.call().filter(|call| call.open() == Some(Over::Connection))?;
        let id = place.id();
        Some((
            id,
            crate::stats_popover(
                phrase(Phrase::StatsTitle, words),
                call.health(),
                call.round_trips(),
                call.alone(),
                call.own_video(),
                move |video| Message::At(id, PlaceMessage::Call(screens::CallMessage::ShowOwn(video))),
                words,
            ),
        ))
    });
    // The panel sits above the account strip and its rule, a margin up from the pane's foot; the popover a margin
    // above it.
    let panel_top = size.height - ACCOUNT_STRIP_HEIGHT - design::BORDER_WIDTH - Space::S.px() - CALL_PANEL_HEIGHT;
    let at = (RAIL_WIDTH + design::BORDER_WIDTH + Space::S.px(), panel_top - Space::S.px());
    // Closed, the layer is still there so the window under it keeps its state; nothing sends its close.
    let (popover, close) = match open {
        Some((id, popover)) => (Some(popover), Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver))),
        None => (None, Message::CloseMenu),
    };
    crate::stats_popover_over(screen, popover, at, close)
}

/// What is open over the window, and what Escape and a press outside it send: the add dialog or a confirmation.
fn over<'a, R: Renderer + 'a>(
    window: &'a Window,
    size: Size,
    handles: &Handles<'_>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> (Option<Part<'a, R>>, Message) {
    if let Some(adding) = window.add_dialog() {
        let id = adding.id();
        let steps = crate::door::in_dialog::<R>(adding.door(), words)
            ._map(move |said| Message::At(id, PlaceMessage::Session(screens::SessionMessage::Door(said))));
        let frame = crate::dialog_frame(
            phrase(Phrase::AddTitle, words),
            SemanticRole::Dialog,
            steps._boxed(),
            Message::CloseAdd,
            words,
        );
        return (Some(frame), Message::AddOutside);
    }
    if let Some((open, close)) = crate::report_dialog::view(window, words) {
        return (Some(open), close);
    }
    if let Some((open, close)) = crate::password_dialog::view(window, words) {
        return (Some(open), close);
    }
    if let Some((open, close)) = crate::dialogs::notify(window, words) {
        return (Some(open), close);
    }
    if let Some(confirmation) =
        window.dialog().and_then(|dialog| crate::dialogs::view(window, dialog, picture, handles, words))
    {
        return (Some(confirmation), Message::Dialog(DialogMessage::Outside));
    }
    // The server-wide search, over the window, when nothing asks first.
    if let Some((layer, close)) =
        window.open().and_then(|place| crate::search_view::layer::<R>(window, place, size, handles, words))
    {
        return (Some(layer), close);
    }
    (None, Message::Dialog(DialogMessage::Outside))
}

/// The server's menu, open.
fn menu_of<'a, R: Renderer + 'a>(window: &'a Window, place: &'a Place, words: &'a Words) -> Option<Part<'a, R>> {
    let (menu, entries) = window.menu()?;
    let entries = entries
        .into_iter()
        .map(|entry| MenuEntry {
            label: label(&words.say(&entry.said)),
            message: Message::Pick(menu.place, entry.pick),
            danger: entry.danger,
            rule_before: entry.rule_before,
            icon: None,
        })
        .collect();
    Some(crate::menu_items(label(place.server().shown()), entries))
}

/// The channel's or category's menu open, below what opened it, else below the pane's header.
fn list_menu<'a, R: Renderer + 'a>(
    window: &'a Window,
    shaped: &Frame,
    words: &'a Words,
) -> Option<(Part<'a, R>, crate::layer::Side, iced_core::Rectangle)> {
    let open = window.list_menu()?;
    let (place, key) = (open.menu.place, open.menu.key.clone());
    let rail = shaped.rail_room();
    let width = shaped.pane.unwrap_or(design::PANE_LEAST);
    let row = window
        .places()
        .iter()
        .find(|one| one.id() == place)
        .and_then(|one| crate::channel_list::row_box(one, &key, rail, width))
        .map(|row| iced_core::Rectangle {
            x: rail + Space::Xl.px(),
            y: shaped.top + row.y,
            // The canvas hangs the menu over the row's foot by Xxs; the layer leaves Xs + Xxs under its opener.
            height: row.height - Space::S.px(),
            ..row
        });
    let fallback = row.unwrap_or(iced_core::Rectangle {
        x: rail + Space::S.px(),
        y: shaped.top + PANE_HEADER_HEIGHT,
        width: width - 2.0 * Space::S.px(),
        height: 0.0,
    });
    Some((crate::channel_list::list_menu_items(&open, words), crate::layer::Side::Below, fallback))
}

/// The space above the rail's first server, and between servers.
const RAIL_TOP: f32 = Space::S.px();

const RAIL_GAP: f32 = Space::Xxs.px();

/// The canvas's gear stands 6 above the rail's foot, less the ring's reach its item keeps below its picture.
const RAIL_FOOT: f32 = Space::Xxs.px();

/// A hairline between two panes.
fn rule<'a, R: Renderer + 'a>() -> Part<'a, R> {
    line(Length::Fixed(design::BORDER_WIDTH), Length::Fill)
}

fn line<'a, R: Renderer + 'a>(width: Length, height: Length) -> Part<'a, R> {
    container(Gap::new())
        .width(width)
        .height(height)
        .class(Surfaced::Fill {
            fill: Role::Border(design::Border::Subtle),
            text: Role::Content(Content::Primary),
            radius: Radius::None,
        })
        ._boxed()
}

fn rail<'a, R: Renderer + 'a>(window: &'a Window, picture: &Picture<'a, R>, words: &'a Words) -> Part<'a, R> {
    let open = window.open().map(Place::id);
    let mut items: Vec<Part<'a, R>> = window
        .places()
        .iter()
        .map(|place| {
            let faint = crate::rail::faint(place.rail());
            let painted = place.picture().map(|shown| {
                let (name, colour) = (place.server().shown(), place.server().colour());
                crate::picture_row::server_tile(name, colour, Some(shown), AvatarSize::Rail, faint, picture)
            });
            RailItem::new(label(place.server().shown()), place.server().colour())
                .open(Some(place.id()) == open)
                .state(place.rail())
                .in_call(place.calling())
                .badge(window.place_badge(place.id()))
                .on_press(Message::Rail(place.id()))
                .on_menu(Message::Menu(place.id(), MenuFrom::Rail))
                .view_painted::<R>(painted, words)
        })
        .collect();
    items.push(crate::add_tile(&phrase(Phrase::AddTitle, words), Message::Add));
    // The canvas's 10 between pictures, less the ring's reach each item keeps above and below its picture.
    let column = Column::with_children(items).spacing(RAIL_GAP).padding(Padding { top: RAIL_TOP, ..Padding::ZERO });
    let servers = scrollable(column)
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .on_scroll(|scroll| Message::RailScrolled(scroll.viewport.absolute_offset().y))
        .height(Length::Fill);
    // The settings are the window's, not a server's: their one way in stays at the rail's foot however far it scrolls.
    let gear = window.gear().map(|gear| {
        let tile = crate::gear_tile(&phrase(Phrase::SettingsLabel, words), gear.current, Message::OpenSettings);
        container(tile).padding(Padding { bottom: RAIL_FOOT, ..Padding::ZERO })._boxed()
    });
    let rail = Column::with_children(std::iter::once(servers._boxed()).chain(gear));
    container(rail).width(Length::Fixed(RAIL_WIDTH)).height(Length::Fill).class(Surfaced::On(Surface::Chrome))._boxed()
}

/// The pane's header: the server's name, which opens its menu under it, open while `open`.
fn header<'a, R: Renderer + 'a>(place: &'a Place, open: bool) -> Part<'a, R> {
    let press = Message::Menu(place.id(), MenuFrom::Header);
    let row = Row::with_children([
        text(place.server().shown(), TypeStyle::BodyStrong).width(Length::Fill)._boxed(),
        icon(Icon::ChevronDown, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let inner = button(container(row).center_y(Length::Fill))
        .class(ButtonClass::Row { selected: false, tone: RowTone::Plain })
        .padding(Padding { left: Space::L.px(), right: Space::M.px() + Space::Xxs.px(), ..Padding::ZERO })
        .width(Length::Fill)
        .height(Length::Fixed(PANE_HEADER_HEIGHT))
        .on_press(press.clone());
    let semantics =
        Semantics { role: SemanticRole::Opens { layer: Popup::Menu, open }, label: label(place.server().shown()) };
    actionable(inner, semantics, Some(press))
}

/// The open server's pane: its name, its voice channels or a line saying why there are none to show, and the account.
fn pane<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    width: f32,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let header = header(place, window.menu().is_some_and(|(menu, _)| menu.place == place.id()));
    let body = match (place.panel(), place.lobby()) {
        (
            Panel::Channels
            | Panel::Discussion
            | Panel::Threads
            | Panel::Conversations
            | Panel::Call
            | Panel::Settings
            | Panel::Account
            | Panel::Lost,
            Some(lobby),
        ) => crate::channel_list::channel_list(window, place, lobby, handles, words),
        (panel, _) => quiet(
            match panel {
                Panel::NotAnswering { .. } => Phrase::PaneNotAnswering,
                Panel::Connecting => Phrase::PaneConnecting,
                Panel::KeyChanged => Phrase::PaneNotConnected,
                Panel::SignedOut
                | Panel::Deleted
                | Panel::NewPassword
                | Panel::Channels
                | Panel::Discussion
                | Panel::Threads
                | Panel::Conversations
                | Panel::Call
                | Panel::Settings
                | Panel::Account
                | Panel::Lost => Phrase::PaneSignedOut,
            },
            words,
        ),
    };
    let column = Column::with_children([
        header._boxed(),
        line(Length::Fill, Length::Fixed(design::BORDER_WIDTH)),
        container(body).height(Length::Fill)._boxed(),
        call_panel(calling(window, place), place, words),
        update(window, words),
        line(Length::Fill, Length::Fixed(design::BORDER_WIDTH)),
        account(window, place, handles, words),
    ]);
    container(column).width(Length::Fixed(width)).height(Length::Fill).class(Surfaced::On(Surface::Raised))._boxed()
}

fn quiet<'a, R: Renderer + 'a>(said: Phrase, words: &'a Words) -> Part<'a, R> {
    container(coloured(words.phrase(said), TypeStyle::Small, TextClass::Content(Content::Secondary)))
        .padding(Padding::from([Space::Xl.px() - Space::Xs.px(), Space::L.px()]))
        ._boxed()
}

/// The server whose call the pane and the account bar show: the one holding a voice place, else `open`.
fn calling<'a>(window: &'a Window, open: &'a Place) -> &'a Place {
    window.places().iter().find(|place| place.calling()).unwrap_or(open)
}

/// The notice of a new version of Chatkord, above the account, while `screens` gives one.
fn update<'a, R: Renderer + 'a>(window: &'a Window, words: &'a Words) -> Part<'a, R> {
    let Some(notice) = window.update_notice() else { return Gap::new()._boxed() };
    let restart = Message::Update(screens::UpdateMessage::Restart);
    container(crate::update_notice(notice, restart, words))
        .padding(Padding { left: Space::S.px(), right: Space::S.px(), bottom: Space::S.px(), top: 0.0 })
        ._boxed()
}

/// The account at the open server: its name, its status with the dot on its picture (where a call is, the call panel
/// says), mute and deafen (the call's, or how the next one starts) and its page.
fn account<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let name = place.shown().unwrap_or_default();
    let line = window.account_line(place.id());
    let status = line.map(|line| words.phrase(line.phrase()).into_owned()).unwrap_or_default();
    let kind = place.own_colour().map_or(AvatarKind::Person, AvatarKind::Called);
    let held = place.me().and_then(|me| place.picture_of(me)).and_then(handles);
    let who = Row::with_children([
        crate::person_with_status(label(name), kind, held, line.and_then(screens::StatusWord::dot), Surface::Chrome),
        Column::with_children([
            crate::typography::line(name.to_string(), TypeStyle::Label)._boxed(),
            coloured(status, TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed(),
        ])
        ._boxed(),
    ])
    .spacing(Space::S.px() + Space::Xxs.px())
    .align_y(Alignment::Center)
    .padding(Padding { left: Space::Xs.px(), right: Space::Xs.px(), ..Padding::ZERO })
    .width(Length::Fill)
    .height(Length::Fixed(CONTROL_HEIGHT));
    // A control's height inside the strip, its hover reaching past the picture as the canvas draws it.
    let who = match place.me() {
        Some(me) => crate::button::opening(
            who._boxed(),
            Message::Person(place.id(), me.clone(), screens::PersonFrom::AccountBar),
            label(name),
        ),
        None => who._boxed(),
    };
    let mut cells: Vec<Part<'a, R>> = vec![who];
    let calling = calling(window, place);
    match calling.call().filter(|call| call.live()) {
        Some(call) => cells.extend(call_toggles(calling.id(), call, CallSize::Pane, words)),
        None => cells.extend(next_call_toggles(window.next_call(), words)),
    }
    // Apart as the canvas has them, so nothing of one control runs into the next.
    let row = Row::with_children(cells).spacing(Space::S.px() + Space::Xxs.px()).align_y(Alignment::Center);
    container(row)
        .padding(Padding { left: Space::M.px() - Space::Xs.px(), right: Space::S.px(), ..Padding::ZERO })
        .width(Length::Fill)
        .center_y(Length::Fixed(ACCOUNT_STRIP_HEIGHT))
        .class(Surfaced::On(Surface::Chrome))
        ._boxed()
}

fn content<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    size: Size,
    shaped: &Frame,
    handles: &'a Handles<'a>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let at = move |said: PlaceMessage| Message::At(id, said);
    match place.panel() {
        Panel::Connecting | Panel::NotAnswering { .. } => centred_panel(window, place, id, shaped.folded(), words),
        Panel::KeyChanged => match place.door() {
            Some(door) => {
                let trust = at(PlaceMessage::Session(screens::SessionMessage::TrustNewKey));
                crate::door::key_changed::<Message, R>(door, Message::Remove(id), trust, words)
            }
            None => Gap::new()._boxed(),
        },
        Panel::SignedOut | Panel::NewPassword | Panel::Deleted => match place.door() {
            Some(door) => {
                let way_out = match place.panel() {
                    Panel::Deleted => Some(Message::Remove(id)),
                    Panel::NewPassword => Some(at(PlaceMessage::Session(screens::SessionMessage::SignOutInstead))),
                    Panel::SignedOut
                    | Panel::KeyChanged
                    | Panel::Connecting
                    | Panel::Lost
                    | Panel::NotAnswering { .. }
                    | Panel::Channels
                    | Panel::Discussion
                    | Panel::Threads
                    | Panel::Conversations
                    | Panel::Call
                    | Panel::Settings
                    | Panel::Account => None,
                };
                crate::door::panel::<Message, R>(
                    door,
                    place.panel() == Panel::Deleted,
                    move |said| Message::At(id, PlaceMessage::Session(screens::SessionMessage::Door(said))),
                    way_out,
                    words,
                )
            }
            None => Gap::new()._boxed(),
        },
        Panel::Lost => {
            let line = StateLine::of(place).map(|shown| state_line::<Message, R>(&shown, words));
            page(Vec::new(), Some(centred_panel(window, place, id, shaped.folded(), words)), line)
        }
        Panel::Discussion => match place.discussion() {
            Some(open) => dropping(
                window,
                screens::Pane::Content,
                crate::discussion::view(
                    window,
                    place,
                    open,
                    screens::Pane::Content,
                    size,
                    crate::discussion::Around {
                        members: members_shown(place, shaped),
                        folded: shaped.folded(),
                        covers: false,
                    },
                    handles,
                    words,
                ),
                words,
            ),
            None => Gap::new()._boxed(),
        },
        Panel::Threads | Panel::Conversations => match place.roll() {
            Some(roll) => crate::talk_lists::view(
                window,
                place,
                roll,
                (shaped.folded(), members_shown(place, shaped)),
                handles,
                words,
            ),
            None => Gap::new()._boxed(),
        },
        Panel::Channels => {
            let note = window.note(place).map(|note| note_card(id, &note, words));
            let panel = page(Vec::new(), Some(centred_panel(window, place, id, shaped.folded(), words)), note);
            // Folded, the members and the search are reached from a header over the panel; otherwise the bar has them.
            if shaped.folded() {
                let header = crate::bar_controls::folded_header(Vec::new(), place, members_shown(place, shaped), words);
                Column::with_children([header, container(panel).height(Length::Fill)._boxed()])._boxed()
            } else {
                panel
            }
        }
        Panel::Call => match place.call() {
            Some(call) => call_view(window, place, call, size, shaped, handles, picture, words),
            None => Gap::new()._boxed(),
        },
        // Drawn in the pane's place too, by `view`.
        Panel::Settings | Panel::Account => Gap::new()._boxed(),
    }
}

/// `channel`, drawn in `pane`, under the drop's card while files dragged over the window would go to it.
fn dropping<'a, R: Renderer + 'a>(
    window: &'a Window,
    pane: screens::Pane,
    channel: Part<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let note = window.drop_target().filter(|target| target.pane == pane).map(|target| words.say(&target.note));
    crate::drop_target::drop_over(channel, note, words)
}

/// The content area: what it shows, then at its foot what holds or is noted, in the flow.
fn page<'a, R: Renderer + 'a>(
    top: Vec<Part<'a, R>>,
    body: Option<Part<'a, R>>,
    foot: Option<Part<'a, R>>,
) -> Part<'a, R> {
    let mut column: Vec<Part<'a, R>> = top;
    column.extend(body.map(|body| container(body).height(Length::Fill)._boxed()));
    let main = Column::with_children(column).spacing(Space::S.px()).height(Length::Fill);
    let main = container(main)
        .padding(Padding::from([Space::M.px(), Space::L.px() + Space::Xxs.px()]))
        .width(Length::Fill)
        .height(Length::Fill);
    match foot {
        Some(foot) => Column::with_children([main._boxed(), foot])._boxed(),
        None => main._boxed(),
    }
}

/// What the content shows while the server is being reached, or has no channel open: the panel `screens` gives.
fn centred_panel<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    id: PlaceId,
    folded: bool,
    words: &'a Words,
) -> Part<'a, R> {
    match screens::CentrePanel::of(place, window.now(), folded) {
        Some(shown) => centre::<Message, R>(
            &shown,
            |offer| match offer {
                screens::Offer::TryNow => Message::At(id, PlaceMessage::Session(screens::SessionMessage::TryNow)),
                screens::Offer::ShowChannels => Message::ToggleChannels,
                screens::Offer::TryAgain(roll) => Message::At(id, PlaceMessage::Talk(screens::TalkMessage::Show(roll))),
            },
            words,
        ),
        None => Gap::new()._boxed(),
    }
}

/// The grip on the rule between the pane and the content, over the row as wide as the rail, the pane and half the rule,
/// under the bar.
fn pane_splitter<'a, R: Renderer + 'a>(shaped: Frame, width: f32, words: &'a Words) -> Part<'a, R> {
    let at = RAIL_WIDTH + design::BORDER_WIDTH + width + (design::BORDER_WIDTH - design::SPLITTER_GRIP) / 2.0;
    let grip = crate::splitter::splitter(
        phrase(Phrase::PaneWidth, words),
        width,
        (design::PANE_LEAST, shaped.pane_most),
        crate::splitter::Grows::Right,
        Message::Pane,
    );
    container(grip).padding(Padding { left: at, top: shaped.top, ..Padding::ZERO }).height(Length::Fill)._boxed()
}

/// The members column's toggle that ends the top bar, or a folded view's header, `shown` while the column is.
pub(crate) fn members_toggle<'a, R: Renderer + 'a>(shown: bool, words: &Words) -> Part<'a, R> {
    let said = if shown { Phrase::HideMembers } else { Phrase::ShowMembers };
    Button::new(phrase(said, words))
        .icon_only(Icon::Users)
        .variant(ButtonVariant::Ghost)
        .compact()
        .on_press(Message::ToggleMembers)
        .view::<R>()
}

/// Whether the members column shows beside the content of `place` at `shaped`: it goes while a thread beside the
/// channel would leave the channel too little.
pub(crate) fn members_shown(place: &Place, shaped: &Frame) -> bool {
    shaped.members_shown(thread_column(place, shaped))
}

/// The thread's column in the content of `place` at `shaped`, none while no thread is open there.
fn thread_column(place: &Place, shaped: &Frame) -> Option<ThreadColumn> {
    place.thread()?;
    (place.panel() == Panel::Discussion).then(|| shaped.thread_column())
}

/// The thread `open` beside the channel in a column `size`, or over it where `covers`.
fn thread_pane<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    open: &'a screens::Discussion,
    size: Size,
    covers: bool,
    handles: &'a Handles<'a>,
    words: &'a Words,
) -> Part<'a, R> {
    let pane = screens::Pane::Thread;
    let around = crate::discussion::Around { covers, ..crate::discussion::Around::default() };
    dropping(window, pane, crate::discussion::view(window, place, open, pane, size, around, handles, words), words)
}

/// A call's channel's messages, in a column `size` beside the call or over it.
fn call_chat<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    open: &'a screens::Discussion,
    size: Size,
    handles: &'a Handles<'a>,
    words: &'a Words,
) -> Part<'a, R> {
    let pane = screens::Pane::CallChat;
    dropping(
        window,
        pane,
        crate::discussion::view(window, place, open, pane, size, crate::discussion::Around::default(), handles, words),
        words,
    )
}

/// The grip on the rule between the call and its chat column `width` wide, in a window `across` wide, under the bar.
fn call_chat_splitter<'a, R: Renderer + 'a>(shaped: &Frame, across: f32, width: f32, words: &'a Words) -> Part<'a, R> {
    let at = across - width - design::BORDER_WIDTH + (design::BORDER_WIDTH - design::SPLITTER_GRIP) / 2.0;
    let grip = crate::splitter::splitter(
        phrase(Phrase::CallChatWidth, words),
        width,
        (design::CALL_CHAT_LEAST, design::CALL_CHAT_MOST),
        crate::splitter::Grows::Left,
        Message::CallChatWidth,
    );
    container(grip)
        .padding(Padding { left: at.max(0.0), top: shaped.top, ..Padding::ZERO })
        .height(Length::Fill)
        ._boxed()
}

/// The grip on the rule between the content and the members column `width` wide, in a window `across` wide, under the
/// bar.
fn members_splitter<'a, R: Renderer + 'a>(shaped: &Frame, across: f32, width: f32, words: &'a Words) -> Part<'a, R> {
    let at = across - width - design::BORDER_WIDTH + (design::BORDER_WIDTH - design::SPLITTER_GRIP) / 2.0;
    let grip = crate::splitter::splitter(
        phrase(Phrase::MembersWidth, words),
        width,
        (design::MEMBERS_LEAST, design::MEMBERS_MOST),
        crate::splitter::Grows::Left,
        Message::MembersWidth,
    );
    container(grip)
        .padding(Padding { left: at.max(0.0), top: shaped.top, ..Padding::ZERO })
        .height(Length::Fill)
        ._boxed()
}

/// The note a first arrival opens with, as a callout at the foot of the channels: who others see, or that this account
/// owns the server, with Invite people for the owner and a ✕.
fn note_card<'a, R: Renderer + 'a>(id: screens::PlaceId, note: &ArrivalNote<'_>, words: &'a Words) -> Part<'a, R> {
    let parts = CalloutParts {
        act: Some(Message::Pick(id, Pick::Invite)),
        away: Some(Message::At(id, PlaceMessage::Lobby(screens::LobbyMessage::Dismiss))),
        under: None,
    };
    let shown = container(callout::<Message, R>(&note.callout(), parts, words))
        .padding(Padding::from([Space::M.px(), Space::L.px() + Space::Xxs.px()]));
    shown._boxed()
}

/// Where the toast sits: how far in from the window's left edge, past the rail and the pane, and from its right edge,
/// past a docked members column, and how far up from its foot, above the call bar while there is one.
fn toast_room(window: &Window, shaped: &Frame) -> Padding {
    let foot = Space::L.px() + Space::S.px();
    let Some(place) = window.open() else { return Padding { bottom: foot, ..Padding::ZERO } };
    let rail = shaped.rail_room();
    if window.page_shown() {
        return Padding { left: rail, bottom: foot, ..Padding::ZERO };
    }
    let (pane, right) = (shaped.pane_room(), shaped.members_room());
    // Just above the composer, or the call bar, and above the state line where one stands there.
    let line = if StateLine::of(place).is_some() { crate::STATE_LINE_ROOM } else { 0.0 };
    let bottom = match place.call().filter(|call| place.panel() == Panel::Call && call.live()) {
        Some(call) => bar_room(call) + Space::Xs.px() + line,
        None if place.panel() == Panel::Discussion => design::COMPOSER_ROOM + line,
        None => foot + line,
    };
    Padding { left: rail + pane, right, bottom, top: 0.0 }
}
