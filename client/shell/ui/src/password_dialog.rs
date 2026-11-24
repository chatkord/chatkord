//! The dialog that changes the password: the one in force, then the new one typed twice, over the account page.

use crate::door::{length_part, match_part, named};
use crate::field_said;
use crate::labels::phrase;
use crate::{Button, Renderer, SecretInput, Theme};
use design::{ButtonVariant, Content, Space, TypeStyle};
use iced_core::{Element, Widget};
use iced_widget::Column;
use screens::{
    AccountMessage, ChangingPassword, Field, MatchNote, PageMessage, PasswordProblem, Place, PlaceMessage,
    WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The open server's password dialog, while it is open, and what a press outside it sends.
pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    words: &'a Words,
) -> Option<(Part<'a, R>, Message)> {
    let place = window.open()?;
    let form = place.account()?.password()?;
    let id = place.id();
    let at = move |said: AccountMessage| Message::At(id, PlaceMessage::Page(PageMessage::Account(said)));
    let fields = Column::with_children([
        field(form, Field::Current, Phrase::CurrentPasswordLabel, current_note(place, form, words), false, &at, words),
        field(form, Field::New, Phrase::NewPasswordLabel, new_note(form, words), true, &at, words),
        field(
            form,
            Field::Again,
            Phrase::NewPasswordAgainLabel,
            match_part(form.match_note(), words),
            false,
            &at,
            words,
        ),
    ])
    .spacing(Space::M.px());
    let line = crate::coloured(
        words.phrase(Phrase::ChangePasswordLine),
        TypeStyle::Body,
        crate::TextClass::Content(Content::Secondary),
    );
    let mut body: Vec<Part<'a, R>> = vec![line._boxed(), fields._boxed()];
    body.extend(form.problem().and_then(|problem| about_the_server(place, problem, words)));
    let save =
        Button::new(phrase(Phrase::SavePasswordAction, words)).variant(ButtonVariant::Primary).loading(form.asking());
    let save = if form.ready() { save.on_press(at(AccountMessage::Save)) } else { save };
    let cancel = Button::new(phrase(Phrase::Cancel, words))
        .variant(ButtonVariant::Secondary)
        .on_press(at(AccountMessage::Cancel));
    let body = Column::with_children(body).spacing(design::DIALOG_GAP);
    let panel = crate::dialog_panel(
        words.label(Phrase::ChangePasswordTitle),
        body._boxed(),
        vec![cancel.view::<R>(), save.view::<R>()],
        at(AccountMessage::Cancel),
        words,
    );
    let panel = crate::on_escape(panel, at(AccountMessage::Cancel), form.ready().then_some(at(AccountMessage::Save)));
    Some((panel._boxed(), at(AccountMessage::Outside)))
}

/// What is said under the current password: why the server turned it away.
fn current_note<'a, R: Renderer + 'a>(place: &Place, form: &ChangingPassword, words: &'a Words) -> Option<Part<'a, R>> {
    let problem = form.problem().filter(|problem| problem.field() == Some(Field::Current))?;
    Some(field_said(&problem.said(place.server().shown()), words))
}

/// What is said under the new password: it is the one in force, or how long it is.
fn new_note<'a, R: Renderer + 'a>(form: &ChangingPassword, words: &'a Words) -> Option<Part<'a, R>> {
    if form.same_as_now() {
        return Some(field_said(&screens::same_as_now(), words));
    }
    length_part(form.length_note(), words)
}

/// A refusal that is about the server and not a field.
fn about_the_server<'a, R: Renderer + 'a>(
    place: &Place,
    problem: PasswordProblem,
    words: &'a Words,
) -> Option<Part<'a, R>> {
    problem.field().is_none().then(|| field_said(&problem.said(place.server().shown()), words))
}

fn field<'a, R: Renderer + 'a>(
    form: &'a ChangingPassword,
    field: Field,
    name: Phrase,
    note: Option<Part<'a, R>>,
    chosen: bool,
    at: &(impl Fn(AccountMessage) -> Message + Copy + 'a),
    words: &'a Words,
) -> Part<'a, R> {
    let invalid = match field {
        Field::Current => matches!(form.problem(), Some(PasswordProblem::Wrong | PasswordProblem::TooManyTries)),
        Field::New => form.same_as_now(),
        Field::Again => form.match_note() == Some(MatchNote::Differs),
    };
    let (typed, toggle) = (*at, *at);
    let mut input = SecretInput::new(words.label(name), form.field(field))
        .revealed(form.revealed(field))
        .invalid(invalid)
        .on_input(move |secret| typed(AccountMessage::Typed(field, secret)))
        .on_submit(toggle(AccountMessage::Save));
    if chosen {
        input = input.placeholder(words.say(&Sentence::AtLeast));
    }
    named(name, note, crate::password_field::<Message, R>(input, at(AccountMessage::Reveal(field)), words), words)
}
