//! The emoji picker, as Discord's: a search and the skin tone of its hands, a row of tabs (the emoji picked lately, the
//! server's own, Unicode's groups), and one grid of every section from the tab chosen onward, or what the search
//! finds while something is typed.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::panel_parts::{hairline, section_heading};
use crate::reactions::{ReactionEmoji, emoji_face, emoji_in_slot};
use crate::typography::coloured;
use crate::{AvatarKind, Icon, IconClass, Labelled, Renderer, Surfaced, TextClass, Theme, avatar, icon, tooltip};
use design::{
    AvatarSize, Content, EMOJI_CELL, EMOJI_CELL_GLYPH, EMOJI_CELL_PICTURE, EMOJI_GRID_COLUMNS, EMOJI_PICKER_FIELD,
    EMOJI_PICKER_GAP, EMOJI_PICKER_HEIGHT, EMOJI_PICKER_PAD, EMOJI_PICKER_WIDTH, EMOJI_SERVER_TILE, EMOJI_TONE_GLYPH,
    Elevation, HIT_TARGET, IconSize, Label, Radius, Rgb, Role, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget, image};
use iced_widget::tooltip::Position;
use iced_widget::{Column, Row, Space as Gap, button, container, scrollable};
use screens::chat::EmojiId;
use screens::chat::emoji::{self, Emoji, Group, SkinTone, TONES};
use words::{Phrase, Words};

/// A section of the picker's grid: the model's.
pub use screens::PickerSection;

/// An emoji picked, or picked lately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickedEmoji {
    /// A unicode emoji, in the tone it was picked in.
    Unicode(&'static Emoji),
    /// A server's own, by its id.
    Custom(EmojiId),
}

/// One of the server's own emoji as the picker shows it.
#[derive(Clone, Debug)]
pub struct ServerEmoji {
    /// Its id at the server.
    pub id: EmojiId,
    /// Its name, without colons.
    pub name: String,
    /// Its picture, once fetched; `:name:` is drawn until then.
    pub picture: Option<image::Handle>,
}

/// What the picker shows: what it lists borrowed for as long as it is built (`'p`), its search for as long as it is
/// drawn (`'a`).
#[derive(Clone, Copy, Debug)]
pub struct EmojiPicker<'p, 'a> {
    /// The server's name: its tab's label and its section's heading.
    pub server: &'p Label,
    /// The server's colour, behind its initial on its tab.
    pub server_colour: Rgb,
    /// The server's own emoji, in its order.
    pub custom: &'p [ServerEmoji],
    /// The emoji picked lately, latest first.
    pub recent: &'p [PickedEmoji],
    /// What the search holds.
    pub query: &'a str,
    /// The skin tone hands are shown and picked in.
    pub tone: SkinTone,
    /// The section the grid starts at.
    pub shown: PickerSection,
}

/// What the picker sends.
pub struct EmojiSends<'a, Message> {
    /// The search's text changed.
    pub query: Box<dyn Fn(String) -> Message + 'a>,
    /// The search was cleared.
    pub clear: Message,
    /// The skin tone button was pressed: the tone after the one shown.
    pub tone: Box<dyn Fn(SkinTone) -> Message + 'a>,
    /// A tab was pressed: the grid starts at its section.
    pub section: Box<dyn Fn(PickerSection) -> Message + 'a>,
    /// An emoji was picked.
    pub pick: Box<dyn Fn(PickedEmoji) -> Message + 'a>,
}

/// The groups the tabs name, as the canvas has them; the rest are reached by scrolling on from Objects.
const TABBED: [Group; 6] = [
    Group::SmileysAndEmotion,
    Group::PeopleAndBody,
    Group::AnimalsAndNature,
    Group::FoodAndDrink,
    Group::Activities,
    Group::Objects,
];

/// The emoji that shows the skin tone on its button: a raised hand.
const TONE_SAMPLE: &str = "\u{270B}";

/// The picker of `picker`, its presses sent through `sends`, said in `words`.
pub fn emoji_picker<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picker: EmojiPicker<'_, 'a>,
    sends: EmojiSends<'a, Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let EmojiSends { query, clear, tone, section, pick } = sends;
    let search = crate::search_field(
        crate::labels::phrase(Phrase::FindEmoji, words),
        words.phrase(Phrase::FindEmoji).as_ref(),
        crate::labels::phrase(Phrase::ClearSearch, words),
        picker.query,
        query,
        clear,
    );
    let top =
        Row::with_children([container(search).width(Length::Fill)._boxed(), tone_button(picker.tone, &*tone, words)])
            .spacing(Space::Xs.px() + Space::Xxs.px())
            .align_y(Alignment::Center);
    let grid = if picker.query.is_empty() { sections(&picker, &*pick, words) } else { found(&picker, &*pick, words) };
    let column = Column::with_children([
        top._boxed(),
        tabs(&picker, &*section, words),
        hairline(),
        scrollable(grid)
            .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
            .height(Length::Fill)
            ._boxed(),
    ])
    .spacing(EMOJI_PICKER_GAP);
    let panel = container(column)
        .padding(EMOJI_PICKER_PAD)
        .width(Length::Fixed(EMOJI_PICKER_WIDTH))
        .height(Length::Fixed(EMOJI_PICKER_HEIGHT))
        .class(Surfaced::Panel(Elevation::Overlay, Radius::Xl));
    Labelled::new(
        panel,
        Semantics { role: SemanticRole::Dialog, label: crate::labels::phrase(Phrase::AddReaction, words) },
    )
    ._boxed()
}

fn tone_button<'a, Message: Clone + 'a, R: Renderer + 'a>(
    tone: SkinTone,
    send: &(dyn Fn(SkinTone) -> Message + 'a),
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let at = TONES.iter().position(|each| *each == tone).unwrap_or(0);
    let next = TONES.get((at + 1) % TONES.len()).copied().unwrap_or(SkinTone::Default);
    let shown = emoji::known(TONE_SAMPLE).map_or(TONE_SAMPLE, |hand| emoji::toned(hand, tone).as_str());
    let face = emoji_face(&ReactionEmoji::Unicode(shown.to_string()), EMOJI_TONE_GLYPH, EMOJI_PICKER_FIELD);
    let message = send(next);
    let inner = button::Button::new(container(face).center(Length::Fixed(EMOJI_PICKER_FIELD)))
        .class(ButtonClass::Field)
        .padding(0)
        .on_press(message.clone());
    let label = crate::labels::phrase(Phrase::SkinTone, words);
    let tip = crate::labels::phrase(tone_name(tone), words);
    tooltip(actionable(inner, Semantics { role: SemanticRole::Button, label }, Some(message)), &tip, Position::Bottom)
}

const fn tone_name(tone: SkinTone) -> Phrase {
    match tone {
        SkinTone::Light => Phrase::ToneLight,
        SkinTone::MediumLight => Phrase::ToneMediumLight,
        SkinTone::Medium => Phrase::ToneMedium,
        SkinTone::MediumDark => Phrase::ToneMediumDark,
        SkinTone::Dark => Phrase::ToneDark,
        // `emojis` adds two-person tones the picker never offers.
        _ => Phrase::ToneDefault,
    }
}

const fn group_name(group: Group) -> Phrase {
    match group {
        Group::SmileysAndEmotion => Phrase::EmojiGroupSmileys,
        Group::PeopleAndBody => Phrase::EmojiGroupPeople,
        Group::AnimalsAndNature => Phrase::EmojiGroupNature,
        Group::FoodAndDrink => Phrase::EmojiGroupFood,
        Group::TravelAndPlaces => Phrase::EmojiGroupTravel,
        Group::Activities => Phrase::EmojiGroupActivities,
        Group::Objects => Phrase::EmojiGroupObjects,
        Group::Symbols => Phrase::EmojiGroupSymbols,
        Group::Flags => Phrase::EmojiGroupFlags,
    }
}

const fn group_icon(group: Group) -> Icon {
    match group {
        Group::SmileysAndEmotion => Icon::Smile,
        Group::PeopleAndBody => Icon::Hand,
        Group::AnimalsAndNature => Icon::Leaf,
        Group::FoodAndDrink => Icon::Apple,
        Group::TravelAndPlaces => Icon::Plane,
        Group::Activities => Icon::Dice,
        Group::Objects => Icon::Lightbulb,
        Group::Symbols => Icon::Heart,
        Group::Flags => Icon::Flag,
    }
}

/// The sections that hold anything, in the grid's order.
fn offered(picker: &EmojiPicker<'_, '_>) -> Vec<PickerSection> {
    let mut all = Vec::new();
    if !picker.recent.is_empty() {
        all.push(PickerSection::Recent);
    }
    if !picker.custom.is_empty() {
        all.push(PickerSection::Server);
    }
    all.extend(emoji::groups().map(PickerSection::Group));
    all
}

fn tabs<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picker: &EmojiPicker<'_, '_>,
    send: &(dyn Fn(PickerSection) -> Message + 'a),
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let tabbed = offered(picker).into_iter().filter(|section| match section {
        PickerSection::Recent | PickerSection::Server => true,
        PickerSection::Group(group) => TABBED.contains(group),
    });
    let cells: Vec<Element<'a, Message, Theme, R>> = tabbed
        .map(|section| {
            let chosen = section == picker.shown;
            let tone = if chosen { Content::Primary } else { Content::Secondary };
            let (face, label): (Element<'a, Message, Theme, R>, Label) = match section {
                PickerSection::Recent => (
                    icon(Icon::Clock, IconSize::Bar, IconClass::Role(Role::Content(tone)))._boxed(),
                    crate::labels::phrase(Phrase::EmojiRecent, words),
                ),
                PickerSection::Server => (
                    container(avatar(picker.server.clone(), AvatarKind::Server(picker.server_colour), AvatarSize::Xs))
                        .center(Length::Fixed(EMOJI_SERVER_TILE))
                        ._boxed(),
                    picker.server.clone(),
                ),
                PickerSection::Group(group) => (
                    icon(group_icon(group), IconSize::Bar, IconClass::Role(Role::Content(tone)))._boxed(),
                    crate::labels::phrase(group_name(group), words),
                ),
            };
            let message = send(section);
            let inner = button::Button::new(container(face).center(Length::Fixed(HIT_TARGET)))
                .class(ButtonClass::Row { selected: chosen, tone: RowTone::Plain })
                .padding(0)
                .on_press(message.clone());
            let semantics = Semantics { role: SemanticRole::Radio { chosen }, label: label.clone() };
            tooltip(actionable(inner, semantics, Some(message)), &label, Position::Bottom)
        })
        .collect();
    let row = Row::with_children(cells).spacing(Space::Xxs.px());
    Labelled::new(
        row,
        Semantics { role: SemanticRole::RadioGroup, label: crate::labels::phrase(Phrase::EmojiGroups, words) },
    )
    ._boxed()
}

/// Every section from the one shown onward, each under its heading.
fn sections<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picker: &EmojiPicker<'_, '_>,
    pick: &(dyn Fn(PickedEmoji) -> Message + 'a),
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let all = offered(picker);
    let from = all.iter().position(|section| *section == picker.shown).unwrap_or(0);
    let mut lines: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    for section in all.into_iter().skip(from) {
        let (heading, cells): (Label, Vec<Element<'a, Message, Theme, R>>) = match section {
            PickerSection::Recent => (
                crate::labels::phrase(Phrase::EmojiRecent, words),
                picker.recent.iter().filter_map(|picked| picked_cell(picker, picked, pick)).collect(),
            ),
            PickerSection::Server => {
                (picker.server.clone(), picker.custom.iter().map(|own| custom_cell(own, pick)).collect())
            }
            PickerSection::Group(group) => (
                crate::labels::phrase(group_name(group), words),
                emoji::in_group(group).filter_map(|each| drawn_cell(emoji::toned(each, picker.tone), pick)).collect(),
            ),
        };
        lines.push(container(section_heading(&heading)).padding(Padding::from([0.0, Space::Xxs.px()]))._boxed());
        lines.push(grid(cells));
    }
    Column::with_children(lines)
        .spacing(EMOJI_PICKER_GAP)
        .padding(Padding { right: Space::S.px(), ..Padding::ZERO })
        ._boxed()
}

/// What the search finds: the server's own whose name holds it, then Unicode's.
fn found<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picker: &EmojiPicker<'_, '_>,
    pick: &(dyn Fn(PickedEmoji) -> Message + 'a),
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let wanted = picker.query.to_lowercase();
    let mut cells: Vec<Element<'a, Message, Theme, R>> = picker
        .custom
        .iter()
        .filter(|own| own.name.to_lowercase().contains(&wanted))
        .map(|own| custom_cell(own, pick))
        .collect();
    cells.extend(emoji::search(picker.query).filter_map(|each| drawn_cell(emoji::toned(each, picker.tone), pick)));
    if cells.is_empty() {
        return container(coloured(
            words.phrase(Phrase::NoEmojiMatch).into_owned(),
            TypeStyle::Small,
            TextClass::Content(Content::Secondary),
        ))
        .padding(Space::Xs.px())
        ._boxed();
    }
    container(grid(cells)).padding(Padding { right: Space::S.px(), ..Padding::ZERO })._boxed()
}

/// `cells` in rows of the grid's columns, the last row's room kept so every column lines up.
fn grid<'a, Message: 'a, R: Renderer + 'a>(
    cells: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        let mut row: Vec<Element<'a, Message, Theme, R>> = cells.by_ref().take(EMOJI_GRID_COLUMNS).collect();
        while row.len() < EMOJI_GRID_COLUMNS {
            row.push(Gap::new().width(Length::Fill)._boxed());
        }
        rows.push(Row::with_children(row).spacing(Space::Xxs.px())._boxed());
    }
    Column::with_children(rows).spacing(Space::Xxs.px())._boxed()
}

fn picked_cell<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picker: &EmojiPicker<'_, '_>,
    picked: &PickedEmoji,
    pick: &(dyn Fn(PickedEmoji) -> Message + 'a),
) -> Option<Element<'a, Message, Theme, R>> {
    match picked {
        PickedEmoji::Unicode(each) => Some(unicode_cell(each, pick)),
        // One deleted since it was picked is no longer offered.
        PickedEmoji::Custom(id) => picker.custom.iter().find(|own| own.id == *id).map(|own| custom_cell(own, pick)),
    }
}

fn unicode_cell<'a, Message: Clone + 'a, R: Renderer + 'a>(
    each: &'static Emoji,
    pick: &(dyn Fn(PickedEmoji) -> Message + 'a),
) -> Element<'a, Message, Theme, R> {
    cell(&ReactionEmoji::Unicode(each.as_str().to_string()), each.name(), pick(PickedEmoji::Unicode(each)))
}

/// A cell for `each`, offered only where the bundled emoji font draws it, so the grid never shows a box.
fn drawn_cell<'a, Message: Clone + 'a, R: Renderer + 'a>(
    each: &'static Emoji,
    pick: &(dyn Fn(PickedEmoji) -> Message + 'a),
) -> Option<Element<'a, Message, Theme, R>> {
    crate::fonts::emoji_draws(each.as_str()).then(|| unicode_cell(each, pick))
}

fn custom_cell<'a, Message: Clone + 'a, R: Renderer + 'a>(
    own: &ServerEmoji,
    pick: &(dyn Fn(PickedEmoji) -> Message + 'a),
) -> Element<'a, Message, Theme, R> {
    let shown = ReactionEmoji::Custom { name: own.name.clone(), picture: own.picture.clone() };
    cell(&shown, &format!(":{}:", own.name), pick(PickedEmoji::Custom(own.id.clone())))
}

fn cell<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &ReactionEmoji,
    name: &str,
    message: Message,
) -> Element<'a, Message, Theme, R> {
    let face = emoji_in_slot(shown, EMOJI_CELL_GLYPH, EMOJI_CELL_PICTURE);
    let inner = button::Button::new(container(face).center_x(Length::Fill).center_y(Length::Fixed(EMOJI_CELL)))
        .class(ButtonClass::Row { selected: false, tone: RowTone::Plain })
        .padding(0)
        .width(Length::Fill)
        .on_press(message.clone());
    let label = crate::labels::label(name);
    tooltip(
        actionable(inner, Semantics { role: SemanticRole::Button, label: label.clone() }, Some(message)),
        &label,
        Position::Top,
    )
}
