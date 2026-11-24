//! Inviting people to a server: the terms a person picks, the link the server's answer makes (shown once: the server
//! keeps only a digest of its code), and the invites that still work, as the settings list them.

use crate::Asked;
use crate::place;
use crate::place::ops;
use crate::roles::RoleList;
use crate::settings::Page;
use crate::window::{Confirming, Copied, DialogMessage, Next};
use proto::chatkord_v1::{ErrorCode, InviteSummary};
use proto::rules::{Colour, Grant, MAX_INVITE_USES, MAX_NAME_CHARS, TextRefused, text_shape};
use terms::{ErrorKind, InviteId, Invited, LinkError, RoleId, Secret};
use words::{Sentence, Words};

/// How many people an invite admits: a whole number from one to what a server allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uses(u32);

/// Why a count of people was not taken: nothing typed, or not a whole number from one to [`Uses::MOST`] (a letter,
/// nought, one more than a server allows).
pub type UsesRefused = choices::TypedRefused;

impl Uses {
    /// One person.
    pub const ONE: Uses = Uses(1);
    /// Five people, the dialog's default.
    pub const FIVE: Uses = Uses(5);
    /// Twenty-five people.
    pub const TWENTY_FIVE: Uses = Uses(25);
    /// A hundred people.
    pub const HUNDRED: Uses = Uses(100);
    /// The counts the dialog offers as they are, in its order; any other is a custom one.
    pub const PRESETS: [Uses; 4] = [Uses::ONE, Uses::FIVE, Uses::TWENTY_FIVE, Uses::HUNDRED];
    /// The fewest an invite admits.
    pub const LEAST: Uses = Uses(1);
    /// The most an invite admits, as the server rules.
    pub const MOST: Uses = Uses(MAX_INVITE_USES);

    /// `count` people, if an invite may admit that many.
    pub fn new(count: u32) -> Result<Uses, UsesRefused> {
        if (Uses::LEAST.0..=Uses::MOST.0).contains(&count) { Ok(Uses(count)) } else { Err(UsesRefused::OutOfBounds) }
    }

    /// The count `typed` writes, spaces round it left out: digits alone.
    pub fn parse(typed: &str) -> Result<Uses, UsesRefused> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err(UsesRefused::Empty);
        }
        if !typed.bytes().all(|digit| digit.is_ascii_digit()) {
            return Err(UsesRefused::OutOfBounds);
        }
        typed.parse::<u32>().map_err(|_| UsesRefused::OutOfBounds).and_then(Uses::new)
    }

    /// How many people, as the server is asked for them.
    pub const fn count(self) -> u32 {
        self.0
    }
}

const _: () = assert!(Uses::HUNDRED.0 <= MAX_INVITE_USES, "a preset the server would refuse");

impl Default for Uses {
    fn default() -> Self {
        Uses::FIVE
    }
}

impl choices::Bounded for Uses {
    const LEAST: Uses = Uses::LEAST;
    const MOST: Uses = Uses::MOST;

    fn parse(typed: &str) -> Result<Uses, UsesRefused> {
        Uses::parse(typed)
    }

    fn written(self) -> String {
        self.0.to_string()
    }

    fn nearest(typed: &str) -> Option<Uses> {
        typed.trim().parse::<u32>().ok().map(|count| Uses(count.clamp(Uses::LEAST.0, Uses::MOST.0)))
    }

    fn position(self) -> u32 {
        self.0
    }

    fn at_position(position: u32) -> Uses {
        Uses(position.clamp(Uses::LEAST.0, Uses::MOST.0))
    }
}

impl choices::Preset for Uses {
    type Value = Uses;

    fn starts_custom(self) -> Uses {
        self
    }

    fn custom(value: Uses) -> Uses {
        value
    }
}

/// How many people an invite is to admit, as the dialog has it: one of [`Uses::PRESETS`], or a count typed or slid,
/// kept as typed so a number out of bounds stays on screen beside its refusal.
pub type UsesChoice = choices::PresetOrCustom<Uses>;

/// How long an invite works, as the dialog offers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Days {
    /// A day.
    One,
    /// A week, the dialog's default.
    #[default]
    Seven,
    /// Thirty days.
    Thirty,
}

impl Days {
    /// Every choice, in the order the dialog offers them.
    pub const ALL: [Days; 3] = [Days::One, Days::Seven, Days::Thirty];

    /// How many days, as the server is asked for them.
    pub const fn count(self) -> u32 {
        match self {
            Days::One => 1,
            Days::Seven => 7,
            Days::Thirty => 30,
        }
    }
}

/// Why an invite was not made, or its note cannot be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteProblem {
    /// The note is over its bound in characters.
    NoteTooLong,
    /// The note holds a line break or another control character.
    NoteCharacters,
    /// The server keeps as many working invites as it may.
    TooMany,
    /// The server refused: too many asks too soon.
    TooManyTries,
    /// The server does not let this account give the role chosen: it is not below them, or holds a word they lack.
    RoleNotGivable,
    /// The server could not be reached, or did not answer.
    NoAnswer,
}

/// What is done in the invite dialog, beside its main button and Cancel.
#[derive(Clone, Debug)]
pub enum InviteMessage {
    /// How many people, picked, typed or slid.
    Uses(UsesMessage),
    /// How long, picked.
    Days(Days),
    /// The Role field pressed: its menu opens, or closes while it is open.
    RoleMenu,
    /// The role the invite gives beside the default one, picked; none for the default role alone.
    Role(Option<RoleId>),
    /// The note as it is typed, whole.
    Note(String),
    /// Copy beside the link.
    CopyLink,
    /// "Manage invites": the settings' invites open.
    Manage,
}

/// What is done to the choice of how many people.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsesMessage {
    /// A preset picked.
    Preset(Uses),
    /// Custom picked: its field starts at the count now chosen.
    Custom,
    /// The custom field's whole text after an edit.
    Typed(String),
    /// The custom slider moved to this many people.
    Slid(u16),
}

/// The link an invite made, shown once and copied from here.
#[derive(Debug)]
pub struct Made {
    link: Secret,
    expires_at: i64,
    copied: bool,
}

impl Made {
    /// The join link, with the invite's code in it.
    pub fn link(&self) -> &Secret {
        &self.link
    }

    /// Whether the link went onto the clipboard.
    pub fn copied(&self) -> bool {
        self.copied
    }
}

/// What the server is asked to make an invite of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Terms {
    pub(crate) uses: Uses,
    pub(crate) days: Days,
    pub(crate) role: Option<RoleId>,
    pub(crate) note: Option<String>,
}

/// A choice of the Role field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleOption<'a> {
    /// The role it gives beside the default one; none for the default role alone.
    pub role: Option<RoleId>,
    /// Its name.
    pub name: &'a str,
    /// The colour it is shown in; none for none.
    pub colour: Option<Colour>,
    /// The one taken.
    pub chosen: bool,
}

/// The invite dialog: the terms, then the link.
#[derive(Debug, Default)]
pub struct Inviting {
    uses: UsesChoice,
    days: Days,
    role: Option<RoleId>,
    role_menu: bool,
    note: String,
    asked: Asked<InviteProblem>,
    made: Option<Made>,
}

impl Inviting {
    /// How many people the invite is to admit.
    pub fn uses(&self) -> &UsesChoice {
        &self.uses
    }

    /// Why the count of people is not one: none while it is, or while nothing is typed yet.
    pub fn uses_refused(&self) -> Option<UsesRefused> {
        self.uses.value().err().filter(|refused| *refused == UsesRefused::OutOfBounds)
    }

    /// How long the invite is to work.
    pub fn days(&self) -> Days {
        self.days
    }

    /// The role the invite is to give beside the default one; none for the default role alone.
    pub fn role(&self) -> Option<&RoleId> {
        self.role.as_ref()
    }

    /// The Role field's choices in `place`, the default role first, then each role its account may give; none where
    /// it may give no role, or the roles are not read yet.
    pub fn role_options<'a>(&self, place: &'a place::Place) -> Option<Vec<RoleOption<'a>>> {
        let (list, grants) = (place.role_list(), place.grants());
        let default = list.default_role().filter(|_| grants.holds(Grant::ManageRoles))?;
        let first = RoleOption { role: None, name: &default.name, colour: default.colour, chosen: self.role.is_none() };
        let given = list.givable(place.role_place(), grants).map(|role| RoleOption {
            role: Some(role.id.clone()),
            name: &role.name,
            colour: role.colour,
            chosen: self.role.as_ref() == Some(&role.id),
        });
        Some(std::iter::once(first).chain(given).collect())
    }

    /// Whether the Role field's menu is open.
    pub fn role_menu_open(&self) -> bool {
        self.role_menu
    }

    /// The note as it is typed, untrimmed; empty when there is none.
    pub fn note(&self) -> &str {
        &self.note
    }

    /// What is wrong, said under the fields: the note as it is typed, else the server's answer.
    pub fn problem(&self) -> Option<InviteProblem> {
        self.shaped_note().err().or(self.asked.refused())
    }

    /// Whether the server is being asked for the invite.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// The link the server's answer made; none until it answered with one.
    pub fn made(&self) -> Option<&Made> {
        self.made.as_ref()
    }

    /// Whether "Create invite link" does something now.
    pub fn ready(&self) -> bool {
        !self.asking() && self.made.is_none() && self.shaped_note().is_ok() && self.uses.value().is_ok()
    }

    /// Something typed or made, which a press outside the dialog must not throw away.
    pub(crate) fn kept(&self) -> bool {
        !self.note.is_empty() || self.asking() || self.made.is_some()
    }

    /// "5 people · Regulars · until 1 November · for Grace": what the link made admits, the role named in `roles`.
    pub fn summary(&self, words: &Words, roles: &RoleList) -> Option<String> {
        let made = self.made.as_ref()?;
        let note = self.shaped_note().ok().flatten();
        let uses = self.uses.value().ok()?.count();
        let role = roles.granted_name(self.role.as_ref(), words);
        Some(words.say(&Sentence::InviteTerms { uses, role, until: made.expires_at, note }))
    }

    /// The note to send: none when nothing is typed, else trimmed and in its bounds.
    pub(crate) fn shaped_note(&self) -> Result<Option<String>, InviteProblem> {
        match text_shape(&self.note, MAX_NAME_CHARS) {
            Ok(note) => Ok(Some(note.to_string())),
            Err(TextRefused::Empty) => Ok(None),
            Err(TextRefused::TooLong) => Err(InviteProblem::NoteTooLong),
            Err(TextRefused::ControlCharacter) => Err(InviteProblem::NoteCharacters),
        }
    }

    /// A choice or the note changed; nothing changes while the server is asked or once the link is made.
    pub(crate) fn choose(&mut self, message: &InviteMessage) {
        if self.asking() || self.made.is_some() {
            return;
        }
        match message {
            InviteMessage::Uses(uses) => self.chose_uses(uses),
            InviteMessage::Days(days) => self.days = *days,
            InviteMessage::RoleMenu => {
                self.role_menu = !self.role_menu;
                return;
            }
            InviteMessage::Role(role) => {
                self.role.clone_from(role);
                self.role_menu = false;
            }
            InviteMessage::Note(note) => self.note.clone_from(note),
            InviteMessage::CopyLink | InviteMessage::Manage => return,
        }
        self.asked.forget();
    }

    fn chose_uses(&mut self, message: &UsesMessage) {
        self.uses = match message {
            UsesMessage::Preset(uses) => UsesChoice::Preset(*uses),
            UsesMessage::Custom => self.uses.custom(),
            UsesMessage::Typed(typed) => UsesChoice::typed(typed.clone()),
            UsesMessage::Slid(at) => UsesChoice::slid(u32::from(*at)),
        };
    }

    /// The server is asked: the terms to send, or none when it cannot be now.
    pub(crate) fn ask(&mut self) -> Option<Terms> {
        if !self.ready() {
            return None;
        }
        let note = self.shaped_note().ok()?;
        let uses = self.uses.value().ok()?;
        self.asked.ask();
        Some(Terms { uses, days: self.days, role: self.role.clone(), note })
    }

    /// The server's answer: the link to the server greeted as `host`, or why there is none; the dialog stays.
    pub(crate) fn answered(&mut self, answer: Result<Invited, LinkError>, host: &str) -> Next {
        match answer {
            Ok(invited) => {
                self.asked.answered(None);
                let link = Secret::new(proto::envelope::join_link(host, invited.code.expose()));
                self.made = Some(Made { link, expires_at: invited.expires_at, copied: false });
            }
            Err(refused) => self.asked.answered(Some(refusal(&refused, self.role.is_some()))),
        }
        Next::Stays
    }

    pub(crate) fn copied(&mut self, done: bool) {
        if let Some(made) = self.made.as_mut() {
            made.copied = done;
        }
    }
}

/// Why the server refused an invite, `with_role` when it gives one.
fn refusal(refused: &LinkError, with_role: bool) -> InviteProblem {
    match refused.kind() {
        // Only the role can be what this account may not hand on, or what is gone.
        ErrorKind::Refused(ErrorCode::NotAuthorized | ErrorCode::NotFound) if with_role => {
            InviteProblem::RoleNotGivable
        }
        // The note was checked as the server checks it, so a refused shape is the cap on working invites.
        ErrorKind::Refused(ErrorCode::InvalidRequest) => InviteProblem::TooMany,
        ErrorKind::Refused(ErrorCode::RateLimited) => InviteProblem::TooManyTries,
        ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached => InviteProblem::NoAnswer,
    }
}

/// Why deleting an invite was refused, said in its confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteInviteRefused {
    /// It gives a role the asker does not hold every word of.
    NotYours,
    /// The server refused: too many asks too soon.
    TooManyTries,
    /// The server could not be reached, or did not answer.
    NoAnswer,
}

/// Deleting one invite: asked first.
#[derive(Debug)]
pub struct DeletingInvite {
    pub(crate) id: InviteId,
    asked: Asked<DeleteInviteRefused>,
}

impl DeletingInvite {
    pub(crate) fn new(id: InviteId) -> DeletingInvite {
        DeletingInvite { id, asked: Asked::Idle }
    }

    /// Whether the server is being asked to delete it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Why the last ask was refused; none while asking and before a refusal.
    pub fn refused(&self) -> Option<DeleteInviteRefused> {
        self.asked.refused()
    }
}

/// What the server’s answer to a delete means: gone (deleted now, or already), or why not.
fn invite_gone(answer: &Result<(), LinkError>) -> Result<(), DeleteInviteRefused> {
    match answer.as_ref().map_err(LinkError::kind) {
        Ok(()) | Err(ErrorKind::Refused(ErrorCode::NotFound)) => Ok(()),
        Err(ErrorKind::Refused(ErrorCode::NotAuthorized)) => Err(DeleteInviteRefused::NotYours),
        Err(ErrorKind::Refused(ErrorCode::RateLimited)) => Err(DeleteInviteRefused::TooManyTries),
        Err(ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached) => Err(DeleteInviteRefused::NoAnswer),
    }
}

/// A working invite as the settings list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedInvite {
    /// The invite's id on the server.
    pub id: InviteId,
    /// The note it was made with; none when it has none.
    pub note: Option<String>,
    /// How many people it has admitted.
    pub used: u32,
    /// How many people it admits in all.
    pub of: u32,
    /// When it stops working, in seconds since the epoch.
    pub ends: i64,
    /// The role it gives beside the default one; none for the default role alone.
    pub role: Option<RoleId>,
}

impl ListedInvite {
    /// The invite `summary` lists; none when it names no id, a server not keeping the protocol.
    pub(crate) fn read(summary: InviteSummary) -> Option<ListedInvite> {
        Some(ListedInvite {
            id: InviteId::parse(&summary.invite_id)?,
            note: summary.note,
            used: summary.uses,
            of: summary.max_uses,
            ends: summary.expires_at,
            role: summary.role_id.as_deref().and_then(RoleId::parse),
        })
    }
}

/// The invite dialog: the terms, asked once, then the link.
impl Confirming for Inviting {
    fn holds_outside(&self) -> bool {
        self.kept()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if self.made().is_some() => Next::Closes,
            DialogMessage::Confirm => match self.ask() {
                Some(Terms { uses, days, role, note }) => {
                    Next::Asks(ops::create_invite(uses.count(), days.count(), role.as_ref(), note))
                }
                None => Next::Stays,
            },
            DialogMessage::Invite(InviteMessage::CopyLink) => match self.made() {
                Some(made) => Next::Copies(made.link().clone(), Copied::Invite),
                None => Next::Stays,
            },
            DialogMessage::Invite(InviteMessage::Manage) => Next::Opens(Page::Invites),
            DialogMessage::Invite(chosen) => {
                self.choose(&chosen);
                Next::Stays
            }
            DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::CopyTemporary
            | DialogMessage::Named(_)
            | DialogMessage::SetUp(_)
            | DialogMessage::Making(_) => Next::Stays,
        }
    }
}

/// Deleting an invite is asked once at a time.
impl Confirming for DeletingInvite {
    fn holds_outside(&self) -> bool {
        self.asking()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if !self.asking() => {
                self.asked.ask();
                Next::Asks(ops::delete_invite(&self.id))
            }
            DialogMessage::Confirm
            | DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::CopyTemporary
            | DialogMessage::Named(_)
            | DialogMessage::SetUp(_)
            | DialogMessage::Making(_) => Next::Stays,
        }
    }
}

impl DeletingInvite {
    /// The server's answer: gone, the dialog closing and the invites listed again, or why not.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        match invite_gone(answer) {
            Ok(()) => {
                self.asked.answered(None);
                Next::Closing(vec![ops::list_invites()])
            }
            Err(refused) => {
                self.asked.answered(Some(refused));
                Next::Stays
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, inviter, tell};
    use crate::harness::{sends, wire};
    use crate::place::{self, Panel, PlaceId};
    use crate::window::{Asking, Dialog, DialogMessage, Pick, Window};
    use crate::{Ask, WindowMessage};
    use proto::chatkord_v1::ErrorCode;
    use proto::rules::{MAX_INVITE_DAYS, MAX_INVITE_USES};
    use std::time::Instant;
    use terms::{LinkError, Secret};
    use words::Words;

    #[test]
    fn every_offered_term_is_one_the_server_takes_and_the_defaults_are_five_people_for_a_week() {
        assert!(Uses::PRESETS.iter().all(|uses| Uses::new(uses.count()) == Ok(*uses)));
        assert!(Days::ALL.iter().all(|days| (1..=MAX_INVITE_DAYS).contains(&days.count())));
        let inviting = Inviting::default();
        assert_eq!((inviting.uses().value().map(Uses::count), inviting.days().count()), (Ok(5), 7));
    }

    #[test]
    fn a_note_is_trimmed_and_one_over_its_bound_is_refused_never_cut() {
        let mut inviting = Inviting::default();
        inviting.choose(&InviteMessage::Note("  for Bo ".into()));
        assert_eq!(inviting.shaped_note(), Ok(Some("for Bo".into())));
        inviting.choose(&InviteMessage::Note("n".repeat(MAX_NAME_CHARS + 1)));
        assert_eq!(inviting.problem(), Some(InviteProblem::NoteTooLong));
        assert!(!inviting.ready() && inviting.ask().is_none(), "nothing is asked");
        inviting.choose(&InviteMessage::Note("a\nb".into()));
        assert_eq!(inviting.problem(), Some(InviteProblem::NoteCharacters));
        inviting.choose(&InviteMessage::Note(String::new()));
        assert_eq!(inviting.ask(), Some(Terms { uses: Uses::FIVE, days: Days::Seven, role: None, note: None }));
    }

    fn inviting(window: &Window) -> &Inviting {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::Invite(inviting)) => inviting,
            other => panic!("not inviting: {other:?}"),
        }
    }

    fn invite(window: &mut Window, clock: &Clock, message: InviteMessage) -> Vec<Ask> {
        window.update(WindowMessage::Dialog(DialogMessage::Invite(message)), clock.at())
    }

    fn code() -> String {
        "c".repeat(proto::rules::TOKEN_LEN)
    }

    fn made(window: &mut Window, clock: &Clock, owls_id: PlaceId) {
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let invited = terms::Invited { code: Secret::new(code()), expires_at: 1_793_491_200 };
        tell(window, clock, owls_id, place::Message::Page(place::PageMessage::Invited(Ok(invited))));
    }

    #[test]
    fn an_invite_is_asked_for_with_the_terms_chosen_and_its_link_names_this_server() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        assert_eq!(
            (inviting(&window).uses(), inviting(&window).days()),
            (&UsesChoice::Preset(Uses::FIVE), Days::Seven)
        );
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Preset(Uses::ONE)));
        invite(&mut window, &clock, InviteMessage::Days(Days::Thirty));
        invite(&mut window, &clock, InviteMessage::Note(" for Bo ".into()));
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(at, ask)]
                if *at == owls_id
                    && sends(ask, wire::CreateInvite { uses: 1, days: 30, role_id: None, note: Some("for Bo".into()) })),
            "{todo:?}"
        );
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "asked once");
        let invited = terms::Invited { code: Secret::new(code()), expires_at: 1_793_491_200 };
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Ok(invited))));
        let made = inviting(&window).made().expect("a link");
        assert_eq!(made.link().expose(), format!("https://chat.example.invalid/join#{}", code()));
        assert_eq!(
            inviting(&window)
                .summary(&Words::english(), window.open().unwrap().role_list())
                .as_deref()
                .map(words::without_isolation)
                .as_deref(),
            Some("1 person · Member · until 1 November · for Bo")
        );
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_some(), "a press outside keeps a link made");
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(window.dialog().is_none(), "Done closes it");
    }

    #[test]
    fn a_press_outside_closes_an_untouched_invite_and_keeps_a_typed_note() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Preset(Uses::HUNDRED)));
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_none(), "a choice alone is not kept");
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        invite(&mut window, &clock, InviteMessage::Note("for Bo".into()));
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert_eq!(inviting(&window).note(), "for Bo");
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        assert!(window.dialog().is_none());
    }

    #[test]
    fn copying_an_invite_link_says_so_on_its_button_and_not_in_a_toast() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        made(&mut window, &clock, owls_id);
        invite(&mut window, &clock, InviteMessage::CopyLink);
        assert_eq!(window.copying().map(str::to_string), Some(format!("https://chat.example.invalid/join#{}", code())));
        window.update(WindowMessage::Copied(Ok(())), clock.at());
        assert!(inviting(&window).made().unwrap().copied());
        assert_eq!((window.copying(), window.toast().map(|toast| toast.said().in_english()).as_deref()), (None, None));
    }

    #[test]
    fn a_refused_invite_says_why_and_can_be_asked_again() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        let refused = |code| LinkError::Refused(code);
        for (answer, problem) in [
            (refused(ErrorCode::InvalidRequest), InviteProblem::TooMany),
            (refused(ErrorCode::RateLimited), InviteProblem::TooManyTries),
            (LinkError::Timeout, InviteProblem::NoAnswer),
        ] {
            assert!(!window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty());
            tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Err(answer))));
            assert_eq!((inviting(&window).problem(), inviting(&window).asking()), (Some(problem), false));
        }
        invite(&mut window, &clock, InviteMessage::Note("x".repeat(proto::rules::MAX_NAME_CHARS + 1)));
        assert_eq!(inviting(&window).problem(), Some(InviteProblem::NoteTooLong));
        assert!(
            window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(),
            "never cut, never sent"
        );
    }

    #[test]
    fn manage_opens_the_invites_and_a_delete_is_asked_first_then_listed_again() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        made(&mut window, &clock, owls_id);
        let todo = invite(&mut window, &clock, InviteMessage::Manage);
        assert!(window.dialog().is_none());
        assert_eq!(window.open().unwrap().panel(), Panel::Settings);
        assert!(
            matches!(&todo[..], [Ask::At(at, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListInvites) && (*at == owls_id)),
            "{todo:?}"
        );
        let summary = proto::chatkord_v1::InviteSummary {
            invite_id: "7".into(),
            uses: 2,
            max_uses: 5,
            expires_at: 1_793_491_200,
            note: Some("for Bo".into()),
            role_id: Some("14".into()),
        };
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invites(Ok(vec![summary]))));
        let listed = window.open().unwrap().settings().unwrap().invites().clone();
        assert!(
            matches!(listed, crate::Loaded::Ready(ref rows) if rows[0].id.as_str() == "7" && rows[0].used == 2
                && rows[0].role == Some(crate::harness::role_id("14"))),
            "the list says the role it gives: {listed:?}"
        );
        window.update(WindowMessage::DeleteInvite(owls_id, crate::harness::invite_id("7")), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if sends(ask, wire::RevokeInvite { invite_id: "7".into() })),
            "{todo:?}"
        );
        let refused = LinkError::Refused(ErrorCode::NotAuthorized);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::InviteDeleted(Err(refused))));
        assert!(
            matches!(window.dialog().map(Dialog::asking), Some(Asking::DeleteInvite(s)) if s.refused() == Some(DeleteInviteRefused::NotYours))
        );
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let todo = tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::InviteDeleted(Ok(()))));
        assert!(window.dialog().is_none());
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the list shows it gone");
        assert!(
            todo.iter().any(
                |done| matches!(done, Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ListInvites))
            ),
            "{todo:?}"
        );
        window.update(WindowMessage::DeleteInvite(owls_id, crate::harness::invite_id("8")), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let gone = LinkError::Refused(ErrorCode::NotFound);
        let todo =
            tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::InviteDeleted(Err(gone))));
        assert!(
            window.dialog().is_none() && window.toast().map(|toast| toast.said().in_english()).as_deref().is_none(),
            "one already gone leaves the list quietly"
        );
        assert!(
            todo.iter().any(
                |done| matches!(done, Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ListInvites))
            ),
            "{todo:?}"
        );
    }

    #[test]
    fn a_count_of_people_is_a_whole_number_from_one_to_what_the_server_takes() {
        assert_eq!(Uses::parse("1"), Ok(Uses::LEAST));
        assert_eq!(Uses::parse(&MAX_INVITE_USES.to_string()), Ok(Uses::MOST));
        assert_eq!(Uses::parse(" 12 ").map(Uses::count), Ok(12), "spaces round it are not part of it");
        assert_eq!(Uses::parse("0"), Err(UsesRefused::OutOfBounds));
        assert_eq!(Uses::parse(&(MAX_INVITE_USES + 1).to_string()), Err(UsesRefused::OutOfBounds));
        assert_eq!(Uses::parse("99999999999999999999"), Err(UsesRefused::OutOfBounds), "no wrap round");
        assert_eq!(Uses::parse(""), Err(UsesRefused::Empty));
        assert_eq!(Uses::parse("  "), Err(UsesRefused::Empty));
        for typed in ["five", "-3", "+5", "5.5", "1e1", "٣"] {
            assert_eq!(Uses::parse(typed), Err(UsesRefused::OutOfBounds), "{typed}");
        }
        assert_eq!(Uses::new(0), Err(UsesRefused::OutOfBounds));
    }

    #[test]
    fn custom_starts_at_the_count_chosen_and_the_slider_and_the_field_are_one_value() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Preset(Uses::TWENTY_FIVE)));
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Custom));
        assert_eq!(inviting(&window).uses(), &UsesChoice::Custom("25".into()));
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Slid(40)));
        assert_eq!(inviting(&window).uses().value().map(Uses::count), Ok(40));
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Typed("7".into())));
        assert_eq!(inviting(&window).uses().slid_to().count(), 7, "the slider follows the field");
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Custom));
        assert_eq!(inviting(&window).uses(), &UsesChoice::Custom("7".into()), "Custom again keeps what is typed");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if sends(ask, wire::CreateInvite { uses: 7, days: 7, role_id: None, note: None })),
            "{todo:?}"
        );
        let invited = terms::Invited { code: Secret::new(code()), expires_at: 1_793_491_200 };
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Ok(invited))));
        assert_eq!(
            inviting(&window)
                .summary(&Words::english(), window.open().unwrap().role_list())
                .as_deref()
                .map(words::without_isolation)
                .as_deref(),
            Some("7 people · Member · until 1 November"),
            "the summary says the custom count"
        );
    }

    #[test]
    fn a_custom_count_out_of_bounds_is_refused_in_a_sentence_and_nothing_is_asked_and_an_empty_one_only_waits() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Custom));
        for (typed, refused) in [
            ("1", None),
            (&MAX_INVITE_USES.to_string(), None),
            ("0", Some(UsesRefused::OutOfBounds)),
            (&(MAX_INVITE_USES + 1).to_string(), Some(UsesRefused::OutOfBounds)),
            ("", None),
            ("ten", Some(UsesRefused::OutOfBounds)),
        ] {
            invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Typed(typed.into())));
            let held = inviting(&window);
            assert_eq!(held.uses_refused(), refused, "{typed:?}");
            assert_eq!(held.ready(), held.uses().value().is_ok(), "{typed:?}");
            assert_eq!(
                typed,
                match held.uses() {
                    UsesChoice::Custom(kept) => kept.as_str(),
                    UsesChoice::Preset(_) => "a preset",
                },
                "what is typed stays on screen"
            );
        }
        invite(&mut window, &clock, InviteMessage::Uses(UsesMessage::Typed("0".into())));
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "never sent");
        assert_eq!(inviting(&window).uses().slid_to(), Uses::LEAST, "the slider rests at an end");
        assert_eq!(UsesChoice::Custom("500".into()).slid_to(), Uses::MOST);
    }

    /// Roles the invite dialog's asker may be offered: Admin high up, Helpers lower holding one word, Greeters
    /// holding a word only an administrator has, and the default role.
    fn with_roles(clock: &Clock, at: proto::rules::Place, grants: proto::rules::Grants) -> (Window, PlaceId) {
        use crate::harness::{ADMIN_PLACE, default_role_entry, placed_holding, role_entry};
        use proto::rules::{Grant, Place};
        let (mut window, owls_id) = placed_holding(clock, at, grants);
        let roles = vec![
            role_entry("11", "Owner", Place::OWNER, &Grant::ALL),
            role_entry("12", "Admin", ADMIN_PLACE, &Grant::ALL),
            role_entry("14", "Helpers", Place::at(ADMIN_PLACE.position() + 1), &[Grant::StartVoice]),
            role_entry("15", "Greeters", Place::at(ADMIN_PLACE.position() + 2), &[Grant::ManageMembers]),
            default_role_entry("13", "Member", Place::BOTTOM, &[]),
        ];
        tell(&mut window, clock, owls_id, place::Message::Server(place::ServerMessage::RolesListed(Ok(roles))));
        (window, owls_id)
    }

    fn offered(window: &Window, inviting: &Inviting) -> Option<Vec<(String, bool)>> {
        inviting
            .role_options(window.open().unwrap())
            .map(|options| options.into_iter().map(|option| (option.name.to_string(), option.chosen)).collect())
    }

    #[test]
    fn the_role_row_offers_the_default_role_then_each_role_the_asker_may_give_and_none_without_manage_roles() {
        use proto::rules::{Grant, Grants, Place};
        let clock = Clock(Instant::now());
        let names = |offered: Option<Vec<(String, bool)>>| {
            offered.map(|options| options.into_iter().map(|(name, _)| name).collect::<Vec<_>>())
        };
        let ask = |at: Place, grants: Grants| {
            let (mut window, owls_id) = with_roles(&clock, at, grants);
            window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
            let held = Inviting::default();
            (names(offered(&window, &held)), offered(&window, &held).map(|options| options[0].1))
        };
        let every: Grants = Grant::ALL.into_iter().collect();
        assert_eq!(
            ask(Place::OWNER, every).0,
            Some(vec!["Member".into(), "Admin".into(), "Helpers".into(), "Greeters".into()])
        );
        assert_eq!(ask(Place::OWNER, every).1, Some(true), "the default role is the one taken at first");
        let below = ask(crate::harness::ADMIN_PLACE, every);
        assert_eq!(
            below.0,
            Some(vec!["Member".into(), "Helpers".into(), "Greeters".into()]),
            "Admin is not below them"
        );
        let lacking: Grants = [Grant::InviteMember, Grant::ManageRoles, Grant::StartVoice].into_iter().collect();
        assert_eq!(
            ask(crate::harness::ADMIN_PLACE, lacking).0,
            Some(vec!["Member".into(), "Helpers".into()]),
            "a role holding a word they lack is not theirs to give"
        );
        let plain: Grants = [Grant::InviteMember].into_iter().collect();
        assert_eq!(ask(crate::harness::ADMIN_PLACE, plain).0, None, "someone who may invite but not give roles");
        assert_eq!(ask(Place::OWNER, plain).0, None, "nor the owner's place, without the word");
    }

    #[test]
    fn the_role_picked_is_sent_and_a_refusal_of_it_is_said_in_one_sentence() {
        use proto::rules::{Grant, Place};
        let clock = Clock(Instant::now());
        let every = Grant::ALL.into_iter().collect();
        let (mut window, owls_id) = with_roles(&clock, Place::OWNER, every);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        let helpers = crate::harness::role_id("14");
        invite(&mut window, &clock, InviteMessage::RoleMenu);
        assert!(inviting(&window).role_menu_open());
        invite(&mut window, &clock, InviteMessage::Role(Some(helpers.clone())));
        assert!(!inviting(&window).role_menu_open(), "a pick closes its menu");
        assert_eq!(offered(&window, inviting(&window)).map(|options| options[2].1), Some(true));
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::CreateInvite(made)) if made.role_id.as_deref() == Some(helpers.as_str()))),
            "{todo:?}"
        );
        for code in [ErrorCode::NotAuthorized, ErrorCode::NotFound] {
            let refused = LinkError::Refused(code);
            tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Err(refused))));
            assert_eq!(inviting(&window).problem(), Some(InviteProblem::RoleNotGivable), "{code:?}");
            assert!(!window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty());
        }
        let refused = LinkError::Refused(ErrorCode::NotAuthorized);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Err(refused))));
        invite(&mut window, &clock, InviteMessage::Role(None));
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if matches!(ask.op(), Some(crate::harness::Body::CreateInvite(made)) if made.role_id.is_none())),
            "{todo:?}"
        );
        let refused = LinkError::Refused(ErrorCode::NotAuthorized);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Err(refused))));
        assert_eq!(inviting(&window).problem(), Some(InviteProblem::NoAnswer), "no role, no refusal of one");
    }

    #[test]
    fn the_link_made_names_its_role_the_default_one_too() {
        use proto::rules::{Grant, Place};
        let clock = Clock(Instant::now());
        let every = Grant::ALL.into_iter().collect();
        let (mut window, owls_id) = with_roles(&clock, Place::OWNER, every);
        window.update(WindowMessage::Pick(owls_id, Pick::Invite), clock.at());
        invite(&mut window, &clock, InviteMessage::Role(Some(crate::harness::role_id("14"))));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let invited = terms::Invited { code: Secret::new(code()), expires_at: 1_793_491_200 };
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Invited(Ok(invited))));
        let said = |window: &Window| {
            inviting(window)
                .summary(&Words::english(), window.open().unwrap().role_list())
                .map(|said| words::without_isolation(&said))
        };
        assert_eq!(said(&window).as_deref(), Some("5 people · Helpers · until 1 November"));
        let (mut window, owls_id) = with_roles(&clock, Place::OWNER, every);
        made(&mut window, &clock, owls_id);
        assert_eq!(said(&window).as_deref(), Some("5 people · Member · until 1 November"));
    }
}
