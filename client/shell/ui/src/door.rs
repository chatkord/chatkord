//! Getting in: the welcome window's steps, the same steps in the add dialog, and the panels a server's sign-in shows
//! in the app window. It draws a `screens::Door` and decides nothing; every word is a phrase or a sentence.

use crate::clipboard::clipboard_reader;
use crate::labels::label;
use crate::{
    AvatarKind, Button, CalloutParts, FocusScope, Icon, Renderer, SecretInput, Surfaced, TextClass, TextInput, Theme,
};
use crate::{avatar, callout, coloured, field_said, text};
use design::{
    AccentRole, AvatarSize, BRAND_PANE_WIDTH, ButtonVariant, Content, Elevation, FORM_WIDTH, Radius, Rgb, Role,
    SemanticRole, Semantics, Space, Status, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container, scrollable, stack};
use proto::pin::KeyPin;
use screens::{
    Callout, Door, Heard, Invite, LengthNote, MatchNote, Message, NameNote, Problem, Shown, Step, Window, Work,
    route_warning,
};
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

fn button<'a>(phrase: Phrase, variant: ButtonVariant, on: Option<Message>, words: &'a Words) -> Button<'a, Message> {
    let made = Button::new(words.label(phrase)).variant(variant);
    match on {
        Some(message) => made.on_press(message),
        None => made,
    }
}

/// The step's main button: pressed only when it does something.
fn main_button<'a, R: Renderer + 'a>(door: &Door, phrase: Phrase, words: &'a Words) -> Part<'a, R> {
    button(phrase, ButtonVariant::Primary, door.ready().then_some(Message::Submit), words).view::<R>()
}

fn back<'a, R: Renderer + 'a>(words: &'a Words) -> Part<'a, R> {
    button(Phrase::Back, ButtonVariant::Secondary, Some(Message::Back), words).view::<R>()
}

/// A row of buttons, the first at the start and the rest at the end.
fn actions<'a, R: Renderer + 'a>(first: Option<Part<'a, R>>, rest: Vec<Part<'a, R>>) -> Part<'a, R> {
    let mut cells: Vec<Part<'a, R>> = first.into_iter().collect();
    cells.push(Gap::new().width(Length::Fill)._boxed());
    cells.extend(rest);
    let row = Row::with_children(cells).spacing(Space::S.px()).align_y(Alignment::Center);
    container(row).padding(Padding { top: Space::Xs.px(), ..Padding::ZERO })._boxed()
}

/// A plain line of secondary text in `style`.
fn quiet<'a, M: 'a, R: Renderer + 'a>(sentence: String, style: TypeStyle) -> Element<'a, M, Theme, R> {
    coloured(sentence, style, TextClass::Content(Content::Secondary))._boxed()
}

/// A server's key in the monospaced face, in four rows on the panel's colour, `width` wide.
fn fingerprint<'a, M: 'a, R: Renderer + 'a>(pin: &KeyPin, width: Length) -> Element<'a, M, Theme, R> {
    let rows = pin.rows().map(|row| text(row, TypeStyle::Mono)._boxed());
    container(Column::with_children(rows))
        .padding(Padding::from([Space::S.px(), Space::S.px() + Space::Xxs.px()]))
        .width(width)
        .class(Surfaced::Fill {
            fill: Role::Surface(Surface::Inset),
            text: Role::Content(Content::Primary),
            radius: Radius::S,
        })
        ._boxed()
}

/// A field's name above it, and under it the line that says where it stands: help, or a refusal of what was typed.
pub(crate) fn named<'a, M: 'a, R: Renderer + 'a>(
    name: Phrase,
    note: Option<Element<'a, M, Theme, R>>,
    input: Element<'a, M, Theme, R>,
    words: &'a Words,
) -> Element<'a, M, Theme, R> {
    let head = coloured(words.phrase(name), TypeStyle::Label, TextClass::Content(Content::Primary));
    Column::with_children([head._boxed(), input].into_iter().chain(note)).spacing(Space::Xs.px())._boxed()
}

/// [`named`] with `aside` at the right of the name's line: a word about the field as a whole, such as that it is
/// optional or cannot be changed, which the note under it never repeats.
pub(crate) fn named_aside<'a, M: 'a, R: Renderer + 'a>(
    name: Phrase,
    aside: Phrase,
    note: Option<Element<'a, M, Theme, R>>,
    input: Element<'a, M, Theme, R>,
    words: &'a Words,
) -> Element<'a, M, Theme, R> {
    let head = Row::with_children([
        coloured(words.phrase(name), TypeStyle::Label, TextClass::Content(Content::Primary))
            .width(Length::Fill)
            ._boxed(),
        coloured(words.phrase(aside), TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .align_y(Alignment::Center);
    Column::with_children([head._boxed(), input].into_iter().chain(note)).spacing(Space::Xs.px())._boxed()
}

fn password<'a, R: Renderer + 'a>(
    door: &'a Door,
    name: Phrase,
    again: bool,
    note: Option<Part<'a, R>>,
    invalid: bool,
    chosen: bool,
    words: &'a Words,
) -> Part<'a, R> {
    let (value, shown, toggle) = if again {
        (door.again(), door.revealed_again(), Message::RevealAgain)
    } else {
        (door.password(), door.revealed(), Message::Reveal)
    };
    let mut input = SecretInput::new(words.label(name), value)
        .revealed(shown)
        .invalid(invalid)
        .on_input(move |typed| if again { Message::Again(typed) } else { Message::Password(typed) })
        .on_submit(Message::Submit);
    if chosen && !again {
        input = input.placeholder(words.say(&Sentence::AtLeast));
    }
    named(name, note, crate::password_field::<Message, R>(input, toggle, words), words)
}

fn length_note<'a, R: Renderer + 'a>(door: &Door, words: &'a Words) -> Option<Part<'a, R>> {
    length_part(door.length_note(), words)
}

/// The note under a new password's field.
pub(crate) fn length_part<'a, M: 'a, R: Renderer + 'a>(
    note: Option<LengthNote>,
    words: &'a Words,
) -> Option<Element<'a, M, Theme, R>> {
    note.map(|note| field_said(&note.said(), words))
}

fn match_note<'a, R: Renderer + 'a>(door: &Door, words: &'a Words) -> Option<Part<'a, R>> {
    match_part(door.match_note(), words)
}

/// The note under the second field.
pub(crate) fn match_part<'a, M: 'a, R: Renderer + 'a>(
    note: Option<MatchNote>,
    words: &'a Words,
) -> Option<Element<'a, M, Theme, R>> {
    note.map(|note| field_said(&note.said(), words))
}

/// The server's name for a sentence: its own, else its address.
fn shown(door: &Door) -> String {
    door.server().map(|server| server.shown().to_string()).unwrap_or_default()
}

/// A problem under the field it is about; one about the server is a box of its own.
fn problem_line<'a, R: Renderer + 'a>(door: &Door, words: &'a Words) -> Option<Part<'a, R>> {
    Some(match door.problem()?.shown() {
        Shown::Field(said) => field_said(&said, words),
        Shown::Callout(boxed) => {
            let parts = CalloutParts { act: Some(Message::OpenListed), away: None, under: None };
            callout::<Message, R>(&boxed, parts, words)
        }
    })
}

fn heading<'a, M: 'a, R: Renderer + 'a>(said: String) -> Element<'a, M, Theme, R> {
    text(said, TypeStyle::Heading)._boxed()
}

fn add<'a, R: Renderer + 'a>(door: &'a Door, in_dialog: bool, words: &'a Words) -> Vec<Part<'a, R>> {
    // In the body face, not the monospaced one: iced loses a field's font for a value set from outside (a paste).
    let field = TextInput::new(words.label(Phrase::WhereLabel), door.text())
        .trailing_room(design::INLINE_ACTION_WIDTH)
        .placeholder(words.phrase(Phrase::WherePlaceholder))
        .invalid(door.problem().is_some_and(|problem| matches!(problem.shown(), Shown::Field(_))))
        .on_input(Message::Where)
        .on_submit(Message::Submit);
    let paste = button(Phrase::Paste, ButtonVariant::Secondary, Some(Message::Paste), words)
        .compact()
        .width(Length::Fixed(design::INLINE_ACTION_WIDTH - Space::Xs.px()))
        .view::<R>();
    let inside = container(paste).align_right(Length::Fill).center_y(Length::Fill).padding([0.0, Space::Xs.px()]);
    let mut where_: Vec<Part<'a, R>> = vec![
        coloured(words.phrase(Phrase::WhereLabel), TypeStyle::Label, TextClass::Content(Content::Primary))._boxed(),
        crate::over_field::over_field(crate::focus::take_focus(field.view::<R>())._boxed(), inside._boxed()),
    ];
    where_.extend(problem_line(door, words));
    if door.pasting() {
        where_.push(clipboard_reader(|text| Message::Reported(Heard::Pasted(text)))._boxed());
    }
    let continuing = button(Phrase::Continue, ButtonVariant::Primary, door.ready().then_some(Message::Submit), words)
        .loading(door.step() == Step::Finding)
        .view::<R>();
    let mut buttons = Vec::new();
    if in_dialog {
        buttons.push(button(Phrase::Cancel, ButtonVariant::Secondary, Some(Message::Cancel), words).view::<R>());
    }
    buttons.push(continuing);
    vec![
        Column::with_children([
            heading(words.phrase(Phrase::AddTitle).into()),
            quiet(words.phrase(Phrase::AddBody).into(), TypeStyle::Body),
        ])
        .spacing(Space::Xs.px())
        ._boxed(),
        Column::with_children(where_).spacing(Space::Xs.px())._boxed(),
        actions(None, buttons),
    ]
}

/// The server as a card: its picture, its name and its address, and the way back to pick another.
fn server_card<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Part<'a, R> {
    let Some(server) = door.server() else { return Gap::new()._boxed() };
    let mut about: Vec<Part<'a, R>> = vec![text(server.shown().to_string(), TypeStyle::Title)._boxed()];
    // A server with no name of its own is shown by its address already.
    if server.name.is_some() {
        about.push(
            coloured(server.host().to_string(), TypeStyle::Mono, TextClass::Content(Content::Secondary))._boxed(),
        );
    }
    if let Some(word) = route_warning(&server.url) {
        about.push(coloured(words.phrase(word), TypeStyle::Small, TextClass::Status(Status::Warning))._boxed());
    }
    card(server.shown(), server.colour(), about, words)
}

/// A server's card: its picture in `colour` with `name`'s letter, what is said of it, and "Not this one? Change".
fn card<'a, R: Renderer + 'a>(name: &str, colour: Rgb, about: Vec<Part<'a, R>>, words: &'a Words) -> Part<'a, R> {
    let change = button(Phrase::NotThisOne, ButtonVariant::Link, Some(Message::Back), words).view::<R>();
    let row = Row::with_children([
        avatar(label(name), AvatarKind::Server(colour), AvatarSize::L),
        Column::with_children(about).width(Length::Fill)._boxed(),
        change,
    ])
    .spacing(Space::M.px())
    .align_y(Alignment::Center);
    container(row)
        .padding(Padding::from([Space::S.px() + Space::Xxs.px(), Space::M.px()]))
        .width(Length::Fill)
        .class(Surfaced::Panel(Elevation::Raised, Radius::M))
        ._boxed()
}

fn preview<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Vec<Part<'a, R>> {
    let mut lines: Vec<Part<'a, R>> =
        vec![heading(words.phrase(Phrase::PreviewTitle).into()), server_card(door, words)];
    let name = shown(door);
    match door.invite() {
        Some(Invite::Works { owner: true, .. }) => {
            let owner = Callout::new(Sentence::YoullOwn(name)).titled(Sentence::Phrase(Phrase::OwnerInvite));
            lines.push(callout::<Message, R>(&owner, CalloutParts::default(), words));
        }
        Some(Invite::Works { until, owner: false }) => lines.push(
            Row::with_children([
                coloured(words.phrase(Phrase::InviteWorks), TypeStyle::BodyStrong, TextClass::Status(Status::Success))
                    ._boxed(),
                quiet(words.say(&Sentence::Until(until)), TypeStyle::Body),
            ])
            .spacing(Space::Xs.px())
            ._boxed(),
        ),
        Some(Invite::Refused) => {
            let refused = Callout::new(Sentence::Text(Text::InviteRefused)).of(Status::Danger);
            lines.push(callout::<Message, R>(&refused, CalloutParts::default(), words));
        }
        None => lines.push(quiet(words.phrase(Phrase::SignInThere).into(), TypeStyle::Body)),
    }
    let next = if door.invite() == Some(Invite::Refused) {
        button(Phrase::HaveAccount, ButtonVariant::Secondary, Some(Message::ToSignIn), words).view::<R>()
    } else {
        main_button(door, Phrase::Continue, words)
    };
    lines.push(actions(Some(back(words)), vec![next]));
    lines
}

/// "Is this the right server?" of a server no authority vouches for: by its address, as it has not greeted, its key
/// to trust or not, and Trust only as a press of its own.
fn trust<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Vec<Part<'a, R>> {
    let Some(asked) = door.unvouched() else { return Vec::new() };
    let about = vec![text(asked.host.clone(), TypeStyle::Title)._boxed()];
    let warning = Callout::new(Sentence::Text(Text::SelfSignedBody))
        .titled(Sentence::Phrase(Phrase::SelfSigned))
        .of(Status::Warning);
    let warning = callout::<Message, R>(
        &warning,
        CalloutParts {
            under: Some(named(Phrase::Fingerprint, None, fingerprint(&asked.pin, Length::Shrink), words)),
            ..CalloutParts::default()
        },
        words,
    );
    let mut lines: Vec<Part<'a, R>> = vec![
        heading(words.phrase(Phrase::PreviewTitle).into()),
        card(&asked.host, asked.colour(), about, words),
        warning,
    ];
    if !door.invited() {
        lines.push(quiet(words.phrase(Phrase::SignInThere).into(), TypeStyle::Body));
    }
    let trusting = button(Phrase::TrustServer, ButtonVariant::Primary, Some(Message::Trust), words).view::<R>();
    lines.push(actions(Some(back(words)), vec![trusting]));
    lines
}

fn name_line<'a, R: Renderer + 'a>(door: &Door, words: &'a Words) -> Part<'a, R> {
    let note = field_said(&door.name_note().said(&shown(door), door.username()), words);
    match door.name_note() {
        NameNote::Taken => Column::with_children([
            note,
            button(Phrase::SignInInstead, ButtonVariant::Link, Some(Message::ToSignIn), words).view::<R>(),
        ])
        .spacing(Space::Xxs.px())
        ._boxed(),
        NameNote::Hint | NameNote::Spaces(_) | NameNote::Characters | NameNote::TooLong => note,
    }
}

fn username<'a, R: Renderer + 'a>(door: &'a Door, invalid: bool, words: &'a Words) -> Part<'a, R> {
    let field = TextInput::new(words.label(Phrase::UsernameLabel), door.username())
        .invalid(invalid)
        .on_input(Message::Username)
        .on_submit(Message::Submit);
    named(Phrase::UsernameLabel, None, crate::focus::take_focus(field.view::<R>())._boxed(), words)
}

fn footer<'a, R: Renderer + 'a>(lead: String, phrase: Phrase, message: Message, words: &'a Words) -> Part<'a, R> {
    Row::with_children([
        quiet(lead, TypeStyle::Small),
        button(phrase, ButtonVariant::Link, Some(message), words).view::<R>(),
    ])
    // The word space between the sentence and its link.
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center)
    ._boxed()
}

fn create<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Vec<Part<'a, R>> {
    let name_wrong = !matches!(door.name_note(), NameNote::Hint);
    let mut lines: Vec<Part<'a, R>> = vec![
        heading(words.say(&Sentence::CreateTitle(shown(door)))),
        Column::with_children([username(door, name_wrong, words), name_line(door, words)])
            .spacing(Space::Xs.px())
            ._boxed(),
        password(door, Phrase::PasswordLabel, false, length_note(door, words), false, true, words),
        password(
            door,
            Phrase::PasswordAgainLabel,
            true,
            match_note(door, words),
            door.match_note() == Some(MatchNote::Differs),
            true,
            words,
        ),
    ];
    lines.extend(problem_line(door, words));
    lines.push(actions(Some(back(words)), vec![main_button(door, Phrase::CreateAction, words)]));
    lines.push(footer(words.say(&Sentence::AlreadyHave(shown(door))), Phrase::SignInAction, Message::ToSignIn, words));
    lines
}

fn sign_in<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Vec<Part<'a, R>> {
    let wrong = matches!(door.problem(), Some(Problem::Wrong { .. }));
    let mut lines: Vec<Part<'a, R>> = vec![heading(words.say(&Sentence::SignInTitle(shown(door))))];
    if let Some(why) = door.why() {
        lines.push(callout::<Message, R>(&Callout::new(why.clone()), CalloutParts::default(), words));
    }
    lines.push(username(door, false, words));
    let mut secret: Vec<Part<'a, R>> = vec![password(door, Phrase::PasswordLabel, false, None, wrong, false, words)];
    secret.extend(problem_line(door, words));
    secret.push(button(Phrase::ForgotPassword, ButtonVariant::Link, Some(Message::Forgot), words).view::<R>());
    lines.push(Column::with_children(secret).spacing(Space::Xs.px())._boxed());
    if door.forgot_open() {
        let forgot = Callout::new(Sentence::Forgot(shown(door))).dismissable();
        let parts = CalloutParts { act: None, away: Some(Message::Forgot), under: None };
        lines.push(callout::<Message, R>(&forgot, parts, words));
    }
    lines.push(actions(Some(back(words)), vec![main_button(door, Phrase::SignInAction, words)]));
    lines.push(if door.can_create() {
        footer(words.phrase(Phrase::NewHere).into(), Phrase::CreateAnAccount, Message::ToCreate, words)
    } else {
        footer(words.phrase(Phrase::NeedInvite).into(), Phrase::PasteOne, Message::StartOver, words)
    });
    lines
}

/// The server's picture and what is under way, with Cancel when it can be stopped.
fn working<'a, R: Renderer + 'a>(door: &'a Door, doing: String, cancel: bool, words: &'a Words) -> Vec<Part<'a, R>> {
    let name = shown(door);
    let mut lines: Vec<Part<'a, R>> = vec![
        container(avatar(label(&name), AvatarKind::Server(door.server_colour()), AvatarSize::L))
            .padding(Space::Xxs.px() + 1.0)
            .class(Surfaced::Speaking(Radius::Xl))
            ._boxed(),
        text(doing, TypeStyle::Title)._boxed(),
    ];
    if cancel {
        lines.push(button(Phrase::Cancel, ButtonVariant::Secondary, Some(Message::Cancel), words).view::<R>());
    }
    lines
}

fn new_password<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Vec<Part<'a, R>> {
    let mut lines: Vec<Part<'a, R>> = vec![
        Column::with_children([
            heading(words.phrase(Phrase::NewPasswordTitle).into()),
            quiet(words.say(&Sentence::NewPasswordBody(shown(door))), TypeStyle::Body),
        ])
        .spacing(Space::Xs.px())
        ._boxed(),
        crate::focus::take_focus(password(
            door,
            Phrase::NewPasswordLabel,
            false,
            length_note(door, words),
            false,
            true,
            words,
        ))
        ._boxed(),
        password(
            door,
            Phrase::NewPasswordAgainLabel,
            true,
            match_note(door, words),
            door.match_note() == Some(MatchNote::Differs),
            true,
            words,
        ),
    ];
    lines.extend(problem_line(door, words));
    lines.push(
        button(Phrase::SaveAndContinue, ButtonVariant::Primary, door.ready().then_some(Message::Submit), words)
            .width(Length::Fill)
            .view::<R>(),
    );
    lines
}

/// A server's own sign-in, inside the app window: its title, why it is asked, the form, and Sign in. No way back and
/// no other server: the rail is the way to those.
fn signed_out<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Vec<Part<'a, R>> {
    let wrong = matches!(door.problem(), Some(Problem::Wrong { .. }));
    let mut lines: Vec<Part<'a, R>> = vec![heading(words.say(&Sentence::SignInTitle(shown(door))))];
    if let Some(why) = door.why() {
        lines.push(callout::<Message, R>(&Callout::new(why.clone()), CalloutParts::default(), words));
    }
    let other = (!door.username().is_empty())
        .then(|| button(Phrase::NotYou, ButtonVariant::Link, Some(Message::OtherAccount), words).view::<R>());
    let field = TextInput::new(words.label(Phrase::UsernameLabel), door.username())
        .on_input(Message::Username)
        .on_submit(Message::Submit);
    lines.push(named(Phrase::UsernameLabel, other, crate::focus::take_focus(field.view::<R>())._boxed(), words));
    let mut secret: Vec<Part<'a, R>> = vec![password(door, Phrase::PasswordLabel, false, None, wrong, false, words)];
    if wrong {
        secret.extend(problem_line(door, words));
    }
    secret.push(button(Phrase::ForgotPassword, ButtonVariant::Link, Some(Message::Forgot), words).view::<R>());
    lines.push(Column::with_children(secret).spacing(Space::Xs.px())._boxed());
    if door.forgot_open() {
        let forgot = Callout::new(Sentence::Forgot(shown(door))).dismissable();
        let parts = CalloutParts { act: None, away: Some(Message::Forgot), under: None };
        lines.push(callout::<Message, R>(&forgot, parts, words));
    }
    if !wrong {
        lines.extend(problem_line(door, words));
    }
    let working = door.step() == Step::Working;
    let signing = if working {
        button(Phrase::SigningInNow, ButtonVariant::Primary, None, words).loading(true)
    } else {
        button(Phrase::SignInAction, ButtonVariant::Primary, door.ready().then_some(Message::Submit), words)
    };
    lines.push(signing.width(Length::Fill).view::<R>());
    lines
}

/// The panel a server's sign-in shows in the app window, its messages made the window's by `to`: signed out, a
/// temporary password to replace, or, `deleted`, only the reason; `way_out` is Remove, or Sign out instead.
pub fn panel<'a, M: Clone + 'static, R: Renderer + 'a>(
    door: &'a Door,
    deleted: bool,
    to: impl Fn(Message) -> M + Copy + 'a,
    way_out: Option<M>,
    words: &'a Words,
) -> Element<'a, M, Theme, R> {
    let renewing = matches!(door.step(), Step::NewPassword) || door.work() == Some(Work::SavingPassword);
    let lines = if deleted {
        let mut lines = vec![heading(words.say(&Sentence::NoLongerOn(shown(door))))];
        lines.extend(
            door.why().map(|why| callout::<Message, R>(&Callout::new(why.clone()), CalloutParts::default(), words)),
        );
        lines
    } else if renewing {
        new_password(door, words)
    } else {
        signed_out(door, words)
    };
    let mut lines: Vec<Element<'a, M, Theme, R>> = lines.into_iter().map(|line| line._map(to)._boxed()).collect();
    if let Some(out) = way_out {
        lines.push(if deleted {
            Button::new(label(&words.say(&Sentence::RemoveFromHere(shown(door)))))
                .variant(ButtonVariant::Danger)
                .width(Length::Fill)
                .on_press(out)
                .view::<R>()
        } else {
            Button::new(words.label(Phrase::SignOutInstead)).variant(ButtonVariant::Link).on_press(out).view::<R>()
        });
    }
    crate::on_escape(form_panel(lines), to(Message::Cancel), door.ready().then_some(to(Message::Submit)))._boxed()
}

/// The panel of a server that presented another key than the one this device trusts for it: both keys as facts, and
/// two equal ways on, `remove` and `trust`; nothing is sent to it meanwhile.
pub fn key_changed<'a, M: Clone + 'a, R: Renderer + 'a>(
    door: &'a Door,
    remove: M,
    trust: M,
    words: &'a Words,
) -> Element<'a, M, Theme, R> {
    let Some(Problem::KeyChanged { trusted, presented }) = door.problem() else { return Gap::new()._boxed() };
    let warning = Callout::new(Sentence::Phrase(Phrase::KeyChangedWhy))
        .titled(Sentence::Phrase(Phrase::KeyChanged))
        .of(Status::Warning);
    let warning = callout::<M, R>(
        &warning,
        CalloutParts {
            under: Some(quiet(words.text(Text::KeyChangedBody).into(), TypeStyle::Body)),
            ..CalloutParts::default()
        },
        words,
    );
    let keys = Row::with_children([
        container(named(Phrase::PreviousFingerprint, None, fingerprint(trusted, Length::Fill), words))
            .width(Length::FillPortion(1))
            ._boxed(),
        container(named(Phrase::NewFingerprint, None, fingerprint(presented, Length::Fill), words))
            .width(Length::FillPortion(1))
            ._boxed(),
    ])
    .spacing(Space::M.px());
    let way = |phrase: Phrase, on: M| {
        Button::new(words.label(phrase)).variant(ButtonVariant::Secondary).width(Length::Fill).on_press(on).view::<R>()
    };
    let ways = Column::with_children([way(Phrase::RemoveHere, remove), way(Phrase::TrustNewCertificate, trust)])
        .spacing(Space::S.px());
    let lines = vec![
        heading(words.say(&Sentence::IdentityChanged(shown(door)))),
        warning,
        keys._boxed(),
        container(ways).padding(Padding { top: Space::Xs.px(), ..Padding::ZERO })._boxed(),
    ];
    form_panel(lines)
}

/// A server's panel in the app window: its lines in a column the panel's width, centred, scrolling when the window is
/// shorter than they are.
fn form_panel<'a, M: 'a, R: Renderer + 'a>(lines: Vec<Element<'a, M, Theme, R>>) -> Element<'a, M, Theme, R> {
    let column = Column::with_children(lines)
        .spacing(Space::M.px() + Space::Xxs.px())
        .width(Length::Fill.max(design::PANEL_WIDTH));
    // No Tab region of its own: inside the window's, the rail and the pane are reached from it.
    let body = container(column).center_x(Length::Fill).padding(Space::Xl.px());
    let scrolled = scrollable(body)
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .height(Length::Shrink);
    container(scrolled).center_y(Length::Fill).width(Length::Fill)._boxed()
}

/// The welcome window's side: the mark, the name, and what the product is.
fn brand<'a, R: Renderer + 'a>(words: &'a Words) -> Part<'a, R> {
    let mark = Row::with_children([
        crate::icon(Icon::Logo, design::IconSize::L, crate::IconClass::Role(Role::Accent(AccentRole::Text)))._boxed(),
        text(words.phrase(Phrase::Wordmark), TypeStyle::Wordmark)._boxed(),
    ])
    .spacing(Space::S.px() + Space::Xxs.px())
    .align_y(Alignment::Center);
    let column = Column::with_children([
        mark._boxed(),
        Gap::new().height(Length::Fill)._boxed(),
        quiet(words.phrase(Phrase::Tagline).into(), TypeStyle::Small),
    ]);
    container(column)
        .padding(Padding::from([Space::Xxl.px(), Space::L.px() + Space::M.px()]))
        .width(Length::Fixed(BRAND_PANE_WIDTH))
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Raised))
        ._boxed()
}

/// A hairline between the side and the form.
fn rule<'a, R: Renderer + 'a>() -> Part<'a, R> {
    container(Gap::new())
        .width(Length::Fixed(design::BORDER_WIDTH))
        .height(Length::Fill)
        .class(Surfaced::Fill {
            fill: Role::Border(design::Border::Subtle),
            text: Role::Content(Content::Primary),
            radius: Radius::None,
        })
        ._boxed()
}

/// The form: a column `width` wide, centred, scrolling when the window is shorter than it.
fn form<'a, R: Renderer + 'a>(lines: Vec<Part<'a, R>>, centred: bool, width: f32) -> Part<'a, R> {
    let mut column = Column::with_children(lines).spacing(Space::L.px()).width(Length::Fixed(width));
    if centred {
        column = column.align_x(Alignment::Center);
    }
    let body = container(FocusScope::new(column)).center_x(Length::Fill).padding(Space::L.px());
    // As tall as the form up to the window, so a short form sits in the middle and a tall one scrolls.
    let scrolled = scrollable(body)
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .height(Length::Shrink);
    container(scrolled).center_y(Length::Fill).width(Length::Fill)._boxed()
}

/// The lines of the step a door is on, whether the welcome window draws its side beside them, and whether they are
/// centred (work under way).
fn step<'a, R: Renderer + 'a>(door: &'a Door, in_dialog: bool, words: &'a Words) -> (Vec<Part<'a, R>>, bool, bool) {
    let name = shown(door);
    match door.step() {
        Step::Add | Step::Finding => (add(door, in_dialog, words), true, false),
        Step::Trust => (trust(door, words), true, false),
        Step::Preview => (preview(door, words), true, false),
        Step::Create => (create(door, words), true, false),
        Step::SignIn => (sign_in(door, words), true, false),
        Step::Working => {
            let doing = match door.work() {
                Some(Work::Creating) => words.say(&Sentence::Creating(name)),
                Some(Work::SavingPassword) => words.phrase(Phrase::SavingPassword).into(),
                Some(Work::SigningIn) | None => words.say(&Sentence::SigningIn(name)),
            };
            (working(door, doing, true, words), true, true)
        }
        Step::NewPassword => (new_password(door, words), false, false),
        Step::Opening | Step::Done => {
            (working(door, words.say(&Sentence::Connecting(name)), false, words), false, true)
        }
        Step::Retrying => {
            let mut lines = working(door, words.say(&Sentence::NoAnswer(name)), false, words);
            if let Some(notice) = door.notice() {
                lines.push(quiet(words.phrase(notice).into(), TypeStyle::Small));
            }
            lines.push(button(Phrase::TryAgain, ButtonVariant::Primary, Some(Message::Retry), words).view::<R>());
            (lines, false, true)
        }
    }
}

/// What the foot of the welcome window says of the client's debug report, which a person who cannot get further can
/// still make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foot<'a> {
    /// Offers the report.
    Ask,
    /// The report was saved under this name.
    Downloaded(&'a str),
    /// The report could not be made.
    Refused,
}

/// How wide the Language field is at the welcome window's foot.
const LANGUAGE_WIDTH: f32 = 240.0;

/// The foot: the Language field where the window offers it, over the line about the report, both centred.
fn foot<'a, R: Renderer + 'a>(report: Foot<'a>, language: Option<&Window>, words: &'a Words) -> Part<'a, R> {
    let line = report_line(report, words);
    let Some(window) = language else { return line };
    let field = container(crate::language_field(window, words, Message::Language))
        .width(Length::Fixed(LANGUAGE_WIDTH))
        ._boxed();
    Column::with_children([container(field).center_x(Length::Fill)._boxed(), line]).spacing(Space::M.px())._boxed()
}

/// The foot's line: the offer with its link, or what became of the report.
fn report_line<'a, R: Renderer + 'a>(foot: Foot<'a>, words: &'a Words) -> Part<'a, R> {
    let line = match foot {
        Foot::Ask => Row::with_children([
            quiet(words.phrase(Phrase::ReportAsk).into(), TypeStyle::Small),
            button(Phrase::ReportAskLink, ButtonVariant::Link, Some(Message::DebugReport), words).view::<R>(),
        ])
        .spacing(Space::Xs.px())
        .align_y(Alignment::Center)
        ._boxed(),
        Foot::Downloaded(file) => {
            let said = words.say(&Sentence::ReportDownloaded(file.to_string()));
            let shown = quiet(said.clone(), TypeStyle::Small);
            crate::Labelled::new(shown, Semantics { role: SemanticRole::Status, label: label(&said) })._boxed()
        }
        Foot::Refused => {
            let said = words.phrase(Phrase::ReportNotMade).into_owned();
            let shown = coloured(said.clone(), TypeStyle::Small, TextClass::Status(Status::Danger));
            crate::Labelled::new(shown, Semantics { role: SemanticRole::Alert, label: label(&said) })._boxed()
        }
    };
    container(line).center_x(Length::Fill)._boxed()
}

/// The door in a window `width` wide, with `report` at its foot, and over it the Language field of `language`'s
/// window where the door is the first run's: beside the form, or under it where the window is the least.
pub fn view<'a, R: Renderer + 'a>(
    door: &'a Door,
    width: f32,
    report: Foot<'a>,
    language: Option<&Window>,
    words: &'a Words,
) -> Part<'a, R> {
    let (mut lines, welcome, centred) = step(door, false, words);
    let side = welcome && screens::frame::door_side_by_side(width);
    let column = if side { FORM_WIDTH } else { FORM_WIDTH.min(width - 2.0 * Space::L.px()).max(0.0) };
    if !side {
        lines.push(foot(report, language, words));
    }
    // Esc goes back one step, or stops what is under way; the door ignores it where neither applies.
    let mut content =
        crate::on_escape(form(lines, centred, column), Message::Back, door.ready().then_some(Message::Submit))._boxed();
    if side {
        let low = container(foot(report, language, words))
            .align_y(iced_core::alignment::Vertical::Bottom)
            .padding(Padding { bottom: Space::L.px() + Space::Xs.px(), ..Padding::ZERO })
            .width(Length::Fill)
            .height(Length::Fill);
        content = stack([content, low._boxed()]).width(Length::Fill).height(Length::Fill)._boxed();
    }
    let window = if side { Row::with_children([brand(words), rule(), content])._boxed() } else { content };
    container(window).width(Length::Fill).height(Length::Fill).class(Surfaced::On(Surface::Base))._boxed()
}

/// The steps of adding another server, for the add dialog: as on the welcome window, with Cancel on the first.
pub fn in_dialog<'a, R: Renderer + 'a>(door: &'a Door, words: &'a Words) -> Part<'a, R> {
    let (lines, _, centred) = step(door, true, words);
    let mut column = Column::with_children(lines).spacing(Space::L.px()).width(Length::Fill);
    if centred {
        column = column.align_x(Alignment::Center);
    }
    crate::on_escape(column, Message::Back, door.ready().then_some(Message::Submit))._boxed()
}
