//! The notice of a new version of Chatkord: a card at the foot of the side pane, above the account, never a dialog, so
//! it never interrupts a call; what it says and whether restarting is offered are `screens`' to say.

use crate::typography::text;
use crate::{Button, Icon, IconClass, Labelled, Renderer, Surfaced, Theme, icon};
use design::{
    AccentRole, ButtonVariant, Content, IconSize, Radius, Role, SemanticRole, Semantics, Space, Status, StatusRole,
    TypeStyle,
};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, container, progress_bar};
use screens::{RestartOffer, UpdateNotice};
use words::{Phrase, Words};

/// The icon's colour: the accent's, but a failed download says so in the warning colour.
fn mark(notice: UpdateNotice) -> Role {
    match notice {
        UpdateNotice::Failed => Role::Status(Status::Warning, StatusRole::Text),
        UpdateNotice::Downloading(_) | UpdateNotice::Ready(_) => Role::Accent(AccentRole::Text),
    }
}

/// The notice `screens` gives, its sentence beside the icon, a bar while the version downloads and, when a restart is
/// offered, a button sending `restart`.
pub fn update_notice<'a, Message: Clone + 'a, R: Renderer + 'a>(
    notice: UpdateNotice,
    restart: Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let said = words.label(notice.phrase());
    let progress = match notice {
        UpdateNotice::Downloading(progress) => Some(
            progress_bar(0.0..=100.0, f32::from(progress.get())).length(Length::Fill).girth(Space::Xs.px())._boxed(),
        ),
        UpdateNotice::Ready(_) | UpdateNotice::Failed => None,
    };
    let action = (notice == UpdateNotice::Ready(RestartOffer::Offered)).then(|| {
        let label = words.label(Phrase::UpdateRestart);
        Button::new(label).variant(ButtonVariant::Primary).compact().on_press(restart).view::<R>()
    });
    let head = Row::with_children(
        [
            icon(Icon::Download, IconSize::M, IconClass::Role(mark(notice)))._boxed(),
            text(said.as_str().to_string(), TypeStyle::Small).width(Length::Fill)._boxed(),
        ]
        .into_iter()
        .chain(action),
    )
    .spacing(Space::S.px() + Space::Xxs.px())
    .align_y(Alignment::Center);
    let column = Column::with_children([head._boxed()].into_iter().chain(progress)).spacing(Space::S.px());
    let card = container(column).padding(Space::S.px() + Space::Xxs.px()).width(Length::Fill).class(Surfaced::Fill {
        fill: Role::Accent(AccentRole::Tint),
        text: Role::Content(Content::Primary),
        radius: Radius::L,
    });
    Labelled::new(card, Semantics { role: SemanticRole::Status, label: said })._boxed()
}
