//! A row of presets with Custom at its end: an invite's number of people, a share's bitrate. While Custom is taken, the
//! slider and the field of the one value stand under it, the field kept as typed, with what the value makes or why the
//! text typed makes none.

use crate::{Renderer, TextClass, TextInput, Theme, choice, coloured};
use design::{CUSTOM_FIELD_WIDTH, Content, Label, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, container};
use std::ops::RangeInclusive;
use std::rc::Rc;

/// Which of the row is taken.
pub(crate) enum Taken<'a, Message> {
    /// The preset at this place in the row.
    Preset(usize),
    /// Custom, with its slider and field.
    Custom(CustomField<'a, Message>),
}

/// The custom value's slider and field, and the line under them.
pub(crate) struct CustomField<'a, Message> {
    /// What a screen reader calls the slider and the field.
    pub name: Label,
    /// The slider's steps, and the step it stands at.
    pub steps: RangeInclusive<u16>,
    /// The step the slider stands at.
    pub at: u16,
    /// What moving the slider to a step sends.
    pub slid: Rc<dyn Fn(u16) -> Message + 'a>,
    /// The field's text, as typed.
    pub typed: &'a str,
    /// What an edit of the field sends, with its whole text.
    pub on_input: Box<dyn Fn(String) -> Message + 'a>,
    /// The unit written after the field, where the row's value has one.
    pub unit: Option<String>,
    /// What Enter in the field sends, where it does something.
    pub on_submit: Option<Message>,
    /// The line under them; none while there is nothing to say.
    pub note: Option<CustomNote>,
}

/// The line under a custom value.
pub(crate) enum CustomNote {
    /// What the value makes, quiet.
    Said(String),
    /// Why the text typed makes none: the field's edge and the line in the danger colour.
    Refused(String),
}

/// The row named `name` of `presets` and `custom`, the label and the press of Custom, `taken` as it says, filling its
/// width where `fill`; under it the custom field while Custom is taken.
pub(crate) fn custom_choice<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    presets: Vec<(Label, Message)>,
    custom: (Label, Message),
    taken: Taken<'a, Message>,
    fill: bool,
) -> Element<'a, Message, Theme, R> {
    let chosen = match &taken {
        Taken::Preset(at) => *at,
        Taken::Custom(_) => presets.len(),
    };
    let mut options = presets;
    options.push(custom);
    let track = choice(name, options, chosen, fill);
    let Taken::Custom(field) = taken else { return track };
    let slid = Rc::clone(&field.slid);
    let rail = crate::slider(field.name.clone(), field.steps, field.at, move |at| (field.slid)(at), move |at| slid(at));
    let refused = matches!(field.note, Some(CustomNote::Refused(_)));
    let mut input = TextInput::new(field.name, field.typed).number().invalid(refused).on_input(field.on_input);
    if let Some(submit) = field.on_submit {
        input = input.on_submit(submit);
    }
    let input = container(input.view::<R>()).width(Length::Fixed(CUSTOM_FIELD_WIDTH));
    let mut controls = vec![container(rail).width(Length::Fill)._boxed(), input._boxed()];
    controls.extend(
        field.unit.map(|unit| coloured(unit, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed()),
    );
    let controls = Row::with_children(controls).spacing(Space::M.px()).align_y(Alignment::Center);
    let mut lines = vec![track, controls._boxed()];
    lines.extend(field.note.map(|note| match note {
        CustomNote::Refused(said) => crate::refusal(said),
        CustomNote::Said(said) => coloured(said, TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed(),
    }));
    Column::with_children(lines).spacing(Space::S.px())._boxed()
}
