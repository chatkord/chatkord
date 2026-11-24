//! Server settings › Overview › Hosting: where people reach the server and the files it takes. Each
//! field is saved on Enter and says under itself what is wrong with it; what is typed is the model's.

use crate::labels::phrase;
use crate::server_settings::{no_answer, page_message};
use crate::{Button, Renderer, TextClass, TextInput, Theme, coloured};
use design::{ButtonVariant, Content, FIELD_UNIT_ROOM, FORM_WIDTH, GRID_FIELD_WIDTH, Space, TypeStyle};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::hosting::{AddressLine, Field, Fields, HostingForm, Message as Said};
use screens::{FieldSaid, Loaded, OverviewMessage, PageMessage, Place, WindowMessage as Message};
use words::{Phrase, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The section's parts, each a part of the page: its heading, then the address and the numbers; or that the
/// server did not answer, with Try again.
pub(crate) fn view<'a, R: Renderer + 'a>(
    place: &'a Place,
    form: &'a HostingForm,
    words: &'a Words,
) -> Vec<Part<'a, R>> {
    let heading = crate::about_page::heading(Phrase::Hosting, words);
    let say = move |message: Said| page_message(place, PageMessage::Overview(OverviewMessage::Hosting(message)));
    let body: Vec<Part<'a, R>> = match form.facts() {
        Loaded::Loading => Vec::new(),
        Loaded::Failed(_) => vec![
            Column::with_children([
                no_answer(place, words),
                Button::new(phrase(Phrase::TryAgain, words))
                    .variant(ButtonVariant::Secondary)
                    .compact()
                    .on_press(say(Said::Reload))
                    .view::<R>(),
            ])
            .spacing(Space::S.px())
            ._boxed(),
        ],
        Loaded::Ready(fields) => {
            let address = container(field_of(fields, Field::Address, &say, words)).width(Length::Fill.max(FORM_WIDTH));
            let line = coloured(
                words.phrase(Phrase::HostingFilesLine),
                TypeStyle::Small,
                TextClass::Content(Content::Secondary),
            );
            let shown = Field::NUMBERS.into_iter().map(|field| {
                container(field_of(fields, field, &say, words)).width(Length::Fixed(GRID_FIELD_WIDTH))._boxed()
            });
            let grid = Row::with_children(shown).spacing(Space::L.px()).wrap().vertical_spacing(Space::M.px());
            let numbers = Column::with_children([line._boxed(), grid._boxed()]).spacing(Space::S.px())._boxed();
            let parts = std::iter::once(numbers);
            std::iter::once(address._boxed()).chain(parts).collect()
        }
    };
    std::iter::once(heading).chain(body).collect()
}

/// One field: its name, the box with its unit laid over its end, and what is said under it.
fn field_of<'a, R: Renderer + 'a>(
    fields: &'a Fields,
    field: Field,
    say: &(impl Fn(Said) -> Message + Clone + 'a),
    words: &'a Words,
) -> Part<'a, R> {
    let typed = fields.typed(field);
    let note = fields.said(field);
    let invalid = note.as_ref().is_some_and(FieldSaid::refuses);
    // An optional number left empty is no limit: it says so in place of a unit.
    let unlimited = fields.unlimited(field);
    let (name, unit) = match field {
        Field::Address => (Phrase::PublicAddress, None),
        Field::Number(number) => (Phrase::hosting_label(number), (!unlimited).then_some(Phrase::UnitMb)),
    };
    let (typing, saving) = (say.clone(), say.clone());
    let input = TextInput::new(phrase(name, words), typed)
        .placeholder(if unlimited { words.phrase(Phrase::NoLimit).into_owned() } else { String::new() })
        .invalid(invalid)
        .on_input(move |text| typing(Said::Typed(field, text)))
        .on_submit(saving(Said::Save(field)));
    let input: Part<'a, R> = match unit {
        Some(unit) => {
            let unit = coloured(words.phrase(unit), TypeStyle::Small, TextClass::Content(Content::Secondary));
            let over = container(unit)
                .align_right(Length::Fill)
                .center_y(Length::Fill)
                .padding(Padding { right: Space::M.px(), ..Padding::ZERO });
            crate::over_field::over_field(input.trailing_room(FIELD_UNIT_ROOM).view::<R>(), over._boxed())
        }
        None if field == Field::Address => input.style(TypeStyle::Mono).view::<R>(),
        None => input.view::<R>(),
    };
    let below: Option<Part<'a, R>> = (field == Field::Address).then(|| {
        let line = fields.address_line();
        match line {
            AddressLine::Empty => crate::warning(words.said(&line.said())),
            AddressLine::Explained => {
                coloured(words.said(&line.said()), TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed()
            }
        }
    });
    let note = note.map(|said| crate::field_said(&said, words));
    // An optional number is said so beside its name; the address, which a server may also leave unset, warns instead.
    let named = match field {
        Field::Number(_) if field.optional() => crate::door::named_aside(name, Phrase::Optional, note, input, words),
        Field::Number(_) | Field::Address => crate::door::named(name, note, input, words),
    };
    Column::with_children(std::iter::once(named).chain(below)).spacing(Space::Xs.px())._boxed()
}
