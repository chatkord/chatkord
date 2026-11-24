//! What opens from the share button, and from a screen's card: the panel that offers to pick a screen, and the popover
//! that says what a screen's picture is.

use crate::layer::on_panel;
use crate::{Button, Icon, IconClass, Labelled, Renderer, TextClass, Theme, coloured, icon, text};
use design::{
    ABOUT_POPOVER_WIDTH, ButtonVariant, Content, IconSize, Label, MENU_RADIUS, Role, SHARE_PANEL_WIDTH, SemanticRole,
    Semantics, Space, Status, StatusRole, TypeStyle,
};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::{About, LineKind};
use words::Words;

/// The panel that offers the desktop's own picker: `title` for a screen reader and over the panel, the `rows` that say
/// how the share goes out, then `choose`, the one action.
pub fn share_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    rows: Vec<Element<'a, Message, Theme, R>>,
    choose: Label,
    on_choose: Message,
) -> Element<'a, Message, Theme, R> {
    let mut lines = vec![text(title.as_str().to_string(), TypeStyle::BodyStrong)._boxed()];
    lines.extend(rows);
    // The panel's one action, as wide as the panel, as the canvas draws it.
    lines.push(Button::new(choose).variant(ButtonVariant::Primary).width(Length::Fill).on_press(on_choose).view::<R>());
    let column = Column::with_children(lines)
        .spacing(Space::M.px())
        .width(Length::Fill.max(SHARE_PANEL_WIDTH - 2.0 * Space::M.px()));
    let panel = on_panel(column._boxed(), Space::M, MENU_RADIUS);
    Labelled::new(panel, Semantics { role: SemanticRole::Dialog, label: title })._boxed()
}

/// What a screen's picture is: whose, then a line each of what is true of it, a glyph before every one in one column;
/// `picked` under its sound line, where its sound is to be picked.
pub fn info_panel<'a, Message: 'a, R: Renderer + 'a>(
    about: &About,
    picked: Option<Element<'a, Message, Theme, R>>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let mut lines = vec![text(about.title.clone(), TypeStyle::BodyStrong)._boxed()];
    let mut picked = picked;
    for line in about.lines(words) {
        let after_sound = matches!(line.kind, LineKind::Sound | LineKind::NoSound);
        lines.push(line_of(line.kind, line.said));
        if after_sound {
            lines.extend(picked.take());
        }
    }
    lines.extend(picked);
    let column = Column::with_children(lines)
        .spacing(Space::S.px())
        .width(Length::Fill.max(ABOUT_POPOVER_WIDTH - 2.0 * Space::M.px()));
    let label = crate::labels::label(&about.title);
    Labelled::new(on_panel(column._boxed(), Space::M, MENU_RADIUS), Semantics { role: SemanticRole::Dialog, label })
        ._boxed()
}

/// One line of the popover: the glyph of its `kind` in the first column, or none for a detail, then `said` in the
/// kind's words.
pub(crate) fn line_of<'a, Message: 'a, R: Renderer + 'a>(
    kind: LineKind,
    said: String,
) -> Element<'a, Message, Theme, R> {
    let glyph = match kind {
        LineKind::Size => Some((Icon::Monitor, Role::Content(Content::Secondary))),
        LineKind::Hardware => Some((Icon::Cpu, Role::Status(Status::Success, StatusRole::Text))),
        LineKind::Software => Some((Icon::Code, Role::Status(Status::Warning, StatusRole::Text))),
        LineKind::Sound => Some((Icon::Volume, Role::Content(Content::Secondary))),
        LineKind::NoSound => Some((Icon::VolumeOff, Role::Content(Content::Secondary))),
        LineKind::Warning => Some((Icon::TriangleAlert, Role::Status(Status::Warning, StatusRole::Text))),
        LineKind::Info => Some((Icon::Info, Role::Content(Content::Secondary))),
        LineKind::Detail => None,
    };
    let (style, class) = match kind {
        LineKind::Size => (TypeStyle::Mono, TextClass::Content(Content::Primary)),
        LineKind::Warning => (TypeStyle::Small, TextClass::Status(Status::Warning)),
        LineKind::Hardware
        | LineKind::Software
        | LineKind::Sound
        | LineKind::NoSound
        | LineKind::Info
        | LineKind::Detail => (TypeStyle::Small, TextClass::Content(Content::Secondary)),
    };
    // The glyph sits level with the first line of words, which may wrap.
    let level = ((style.line_height() - IconSize::Line.px()) / 2.0).max(0.0);
    let first = match glyph {
        Some((glyph, tone)) => container(icon(glyph, IconSize::Line, IconClass::Role(tone)))
            .padding(Padding { top: level, ..Padding::ZERO })
            ._boxed(),
        None => iced_widget::Space::new().width(IconSize::Line.px())._boxed(),
    };
    Row::with_children([first, coloured(said, style, class).width(Length::Fill)._boxed()])
        .spacing(Space::S.px())
        ._boxed()
}

/// The applications playing sound, for the sharer to pick the one their window share sends: a heading, then a radio
/// row each, `picked` ticked; taking one sends what `pick` makes of it.
pub(crate) fn sound_apps<'a, Message: Clone + 'a, R: Renderer + 'a>(
    apps: &[screens::SoundApp],
    picked: Option<&str>,
    pick: impl Fn(screens::SoundApp) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let heading = coloured(
        words.phrase(words::Phrase::PickSoundApp).into_owned(),
        TypeStyle::Small,
        TextClass::Content(Content::Primary),
    );
    let rows = apps.iter().map(|app| {
        crate::ListRow::<Message, R>::new(crate::labels::label(&app.name))
            .radio(picked == Some(app.name.as_str()))
            .on_press(pick(app.clone()))
            .view()
    });
    Column::with_children(std::iter::once(heading._boxed()).chain(rows)).spacing(Space::Xxs.px())._boxed()
}
