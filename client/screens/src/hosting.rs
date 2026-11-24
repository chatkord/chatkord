//! Server settings › Overview › Hosting, an admin's: where clients reach the server and the files it takes. Each
//! field is saved on Enter, the whole being sent with that one field changed; what is typed is read as
//! `proto::rules::Hosting` reads it, so a value out of its bounds is refused here in the same terms as at the server.

use crate::loaded::Loaded;
use crate::tell::FieldSaid;
use proto::chatkord_v1::{Hosting as Wire, ServerHosting};
use proto::rules::{Hosting, HostingField, HostingRefused};
use terms::LinkError;
use words::{Phrase, Sentence};

/// A field of the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// Where clients reach the server.
    Address,
    /// One of its numbers.
    Number(HostingField),
}

impl Field {
    /// The numbers, in the order the form shows them: the largest file, then each person's storage.
    pub const NUMBERS: [Field; HostingField::ALL.len()] =
        [Field::Number(HostingField::AttachmentMax), Field::Number(HostingField::AccountFilesMax)];

    /// Every field, in the order the form shows them: the address, then the numbers.
    pub const ALL: [Field; 1 + Self::NUMBERS.len()] = [Field::Address, Self::NUMBERS[0], Self::NUMBERS[1]];

    /// Whether leaving the field empty is a value: a number that may be unlimited is none when empty.
    pub const fn optional(self) -> bool {
        match self {
            Field::Address => true,
            Field::Number(field) => field.may_be_unlimited(),
        }
    }

    /// What `wire` holds of the field, as typed.
    fn shown(self, wire: &Wire) -> String {
        match self {
            Field::Address => wire.public_address.clone().unwrap_or_default(),
            Field::Number(field) => field.of(wire).map(|value| value.to_string()).unwrap_or_default(),
        }
    }

    /// `wire` with the field set to what `typed` says, or why it is not a value of the field.
    fn set(self, mut wire: Wire, typed: &str) -> Result<Wire, HostingRefused> {
        let typed = typed.trim();
        match self {
            Field::Address => wire.public_address = (!typed.is_empty()).then(|| typed.to_owned()),
            Field::Number(field) => {
                let value = if typed.is_empty() && self.optional() {
                    None
                } else {
                    Some(typed.parse::<u32>().map_err(|_| HostingRefused::OutOfRange(field))?)
                };
                field.set(&mut wire, value);
            }
        }
        Ok(wire)
    }
}

/// Why a field was not saved, for the view to word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// What was typed is not a value of the field, as `proto::rules` bounds it; a text that is no whole number is out
    /// of range.
    Typed(HostingRefused),
    /// The server would not take it, or did not answer.
    Server(LinkError),
}

/// What a field says beside its label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// What is typed is what the server holds.
    Unchanged,
    /// A change Enter would save.
    Unsaved,
    /// Sent; the server has not answered.
    Saving,
    /// The server took the last change, and nothing has been typed since.
    Saved,
    /// The last change was not taken.
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Save {
    Idle,
    Asking,
    Saved,
    Refused(Refusal),
}

/// A field, its text and where its last save stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    field: Field,
    typed: String,
    save: Save,
}

impl Entry {
    /// Which field it is.
    pub fn field(&self) -> Field {
        self.field
    }

    /// What is typed, untrimmed.
    pub fn typed(&self) -> &str {
        &self.typed
    }
}

/// What the line under the address says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressLine {
    /// What the address is for.
    Explained,
    /// That none is set: a warning.
    Empty,
}

impl AddressLine {
    /// What it says.
    pub const fn said(self) -> Sentence {
        match self {
            AddressLine::Explained => Sentence::Text(words::Text::PublicAddressLine),
            AddressLine::Empty => Sentence::Phrase(Phrase::PublicAddressEmpty),
        }
    }
}

/// What a person does in the form.
#[derive(Clone, Debug)]
pub enum Message {
    /// A field's whole text after an edit; ignored while a save is asked.
    Typed(Field, String),
    /// Enter in a field: it is saved, and with a number the other numbers typed and not yet sent.
    Save(Field),
    /// Ask for the facts again after they could not be read.
    Reload,
}

/// What the form asks of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// The hosting facts and the switches that are off.
    Get,
    /// Set the hosting facts whole.
    Set(Wire),
}

/// The fields, over the facts as the server last said them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fields {
    saved: Wire,
    entries: Vec<Entry>,
    asking: Option<Wire>,
}

impl Fields {
    fn over(saved: Wire) -> Fields {
        let entries =
            Field::ALL.map(|field| Entry { field, typed: field.shown(&saved), save: Save::Idle }).into_iter().collect();
        Fields { saved, entries, asking: None }
    }

    /// The fields, in the order they show.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn entry(&self, field: Field) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.field == field)
    }

    /// What `field` holds, as typed.
    pub fn typed(&self, field: Field) -> &str {
        self.entry(field).map_or("", Entry::typed)
    }

    /// What `field` says now: a refusal or the save's progress, else whether it differs from the server's.
    pub fn note(&self, field: Field) -> Note {
        let Some(entry) = self.entry(field) else { return Note::Unchanged };
        let changed = entry.typed.trim() != field.shown(&self.saved);
        match (&entry.save, changed) {
            (Save::Asking, _) => Note::Saving,
            (Save::Refused(refusal), _) => Note::Refused(refusal.clone()),
            (Save::Saved, false) => Note::Saved,
            (Save::Idle | Save::Saved, true) => Note::Unsaved,
            (Save::Idle, false) => Note::Unchanged,
        }
    }

    /// What `field` says under itself: that a change is unsaved, saving or saved, or why it was refused, in the
    /// numbers typed now.
    pub fn said(&self, field: Field) -> Option<FieldSaid> {
        let help = |phrase: Phrase| Some(FieldSaid::help(Sentence::Phrase(phrase)));
        match self.note(field) {
            Note::Unchanged => None,
            Note::Unsaved => help(Phrase::PressEnterToSave),
            Note::Saving => help(Phrase::Saving),
            Note::Saved => help(Phrase::Saved),
            Note::Refused(Refusal::Server(failed)) => Some(FieldSaid::refusal(words::failure::said(&failed))),
            Note::Refused(Refusal::Typed(refused)) => {
                let number = |of: Field| self.typed(of).trim().parse::<u32>().ok();
                let typed = number(field);
                // A number that is not one has no direction to miss in: it is not a number.
                if matches!(field, Field::Number(_)) && typed.is_none() {
                    return Some(FieldSaid::refusal(Sentence::Phrase(Phrase::WholeNumber)));
                }
                let within_now = match refused {
                    HostingRefused::Above { within, .. } => number(Field::Number(within)).unwrap_or_default(),
                    HostingRefused::PublicAddress | HostingRefused::OutOfRange(_) => 0,
                };
                Some(FieldSaid::refusal(Sentence::hosting_refused(refused, typed.unwrap_or_default(), within_now)))
            }
        }
    }

    /// Whether `field` is an optional number left empty, which is no limit: the field shows that, with no unit.
    pub fn unlimited(&self, field: Field) -> bool {
        matches!(field, Field::Number(_)) && field.optional() && self.typed(field).trim().is_empty()
    }

    /// What the line under the address says: a warning while none is typed.
    pub fn address_line(&self) -> AddressLine {
        if self.typed(Field::Address).trim().is_empty() { AddressLine::Empty } else { AddressLine::Explained }
    }

    fn typed_in(&mut self, field: Field, typed: String) {
        if self.asking.is_some() {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.field == field) {
            entry.typed = typed;
            entry.save = Save::Idle;
        }
        // A refusal of one number against another no longer stands once either changes.
        for entry in &mut self.entries {
            if let Save::Refused(Refusal::Typed(HostingRefused::Above { field: bound, within })) = entry.save
                && [Field::Number(bound), Field::Number(within)].contains(&field)
            {
                entry.save = Save::Idle;
            }
        }
    }

    /// Whether `field` holds text the server does not.
    fn changed(&self, field: Field) -> bool {
        self.entry(field).is_some_and(|entry| entry.typed.trim() != field.shown(&self.saved))
    }

    /// What Enter in `field` saves: the field, and with a number every other number typed and not yet sent, since one
    /// bounds another and both are checked as they show.
    fn saved_with(&self, field: Field) -> Vec<Field> {
        match field {
            Field::Address => vec![field],
            Field::Number(_) => {
                Field::NUMBERS.into_iter().filter(|other| *other == field || self.changed(*other)).collect()
            }
        }
    }

    /// Saves `fields`, each over the facts as the server holds them; nothing is asked while an ask awaits its answer
    /// or when a field's text is refused.
    fn save(&mut self, fields: &[Field]) -> Option<Ask> {
        if self.asking.is_some() {
            return None;
        }
        let mut wire = self.saved.clone();
        for &field in fields {
            let entry = self.entries.iter_mut().find(|entry| entry.field == field)?;
            wire = match field.set(wire, &entry.typed) {
                Ok(wire) => wire,
                Err(refused) => {
                    entry.save = Save::Refused(Refusal::Typed(refused));
                    return None;
                }
            };
        }
        let wire = match Hosting::parse(&wire) {
            Ok(hosting) => hosting.to_wire(),
            Err(refused) => {
                let at = match refused {
                    HostingRefused::PublicAddress => Field::Address,
                    HostingRefused::OutOfRange(field) | HostingRefused::Above { field, .. } => Field::Number(field),
                };
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.field == at) {
                    entry.save = Save::Refused(Refusal::Typed(refused));
                }
                return None;
            }
        };
        for entry in self.entries.iter_mut().filter(|entry| fields.contains(&entry.field)) {
            entry.save = Save::Asking;
        }
        self.asking = Some(wire.clone());
        Some(Ask::Set(wire))
    }

    /// Every field typed and not yet sent, saved together.
    fn left(&mut self) -> Option<Ask> {
        let changed: Vec<Field> = Field::ALL.into_iter().filter(|field| self.changed(*field)).collect();
        if changed.is_empty() { None } else { self.save(&changed) }
    }

    /// The facts the server holds now: they replace every field nobody has typed in.
    fn told(&mut self, saved: Wire) {
        for entry in &mut self.entries {
            if entry.typed.trim() == entry.field.shown(&self.saved) {
                entry.typed = entry.field.shown(&saved);
            }
        }
        self.saved = saved;
    }

    fn answered(&mut self, answer: &Result<(), LinkError>) {
        let Some(sent) = self.asking.take() else { return };
        for entry in self.entries.iter_mut().filter(|entry| entry.save == Save::Asking) {
            entry.save = match answer {
                Ok(()) => Save::Saved,
                Err(failed) => Save::Refused(Refusal::Server(failed.clone())),
            };
        }
        if answer.is_ok() {
            self.saved = sent;
        }
    }
}

/// The form: its fields, once the facts are read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostingForm {
    facts: Loaded<Fields>,
}

impl HostingForm {
    /// The fields once the facts are read; else that they are asked, or could not be read.
    pub fn facts(&self) -> &Loaded<Fields> {
        &self.facts
    }

    pub(crate) fn update(&mut self, message: Message) -> Option<Ask> {
        match message {
            Message::Typed(field, typed) => {
                if let Some(fields) = self.facts.ready_mut() {
                    fields.typed_in(field, typed);
                }
                None
            }
            Message::Save(field) => {
                let fields = self.facts.ready_mut()?;
                let saved = fields.saved_with(field);
                fields.save(&saved)
            }
            Message::Reload => {
                self.facts = Loaded::Loading;
                Some(Ask::Get)
            }
        }
    }

    /// What leaving the page saves: every field typed and not yet sent, together.
    pub(crate) fn left(&mut self) -> Option<Ask> {
        self.facts.ready_mut()?.left()
    }

    /// The server's answer to reading the facts; what it holds replaces every field nobody has typed in.
    pub(crate) fn loaded(&mut self, answer: &Result<ServerHosting, LinkError>) {
        let saved = match answer {
            Ok(told) => told.hosting.clone().unwrap_or_else(|| Hosting::DEFAULT.to_wire()),
            Err(failed) => {
                self.facts = Loaded::Failed(failed.clone());
                return;
            }
        };
        match self.facts.ready_mut() {
            Some(fields) => fields.told(saved),
            None => self.facts = Loaded::Ready(Fields::over(saved)),
        }
    }

    /// The server's answer to a save.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) {
        if let Some(fields) = self.facts.ready_mut() {
            fields.answered(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::chatkord_v1::ErrorCode;

    const LARGEST: Field = Field::Number(HostingField::AttachmentMax);
    const STORAGE: Field = Field::Number(HostingField::AccountFilesMax);

    fn ready() -> HostingForm {
        let mut form = HostingForm::default();
        form.loaded(&Ok(ServerHosting { hosting: Some(Hosting::DEFAULT.to_wire()), switched_off: Vec::new() }));
        form
    }

    /// The largest file a server holds before anyone sets one, as typed.
    fn held_largest() -> String {
        Hosting::DEFAULT.to_wire().attachment_max_mib.to_string()
    }

    fn fields(form: &HostingForm) -> &Fields {
        form.facts().ready().expect("the facts are read")
    }

    fn note(form: &HostingForm, field: Field) -> Note {
        fields(form).note(field)
    }

    fn typed(form: &mut HostingForm, field: Field, text: &str) {
        form.update(Message::Typed(field, text.into()));
    }

    #[test]
    fn nothing_is_typed_or_saved_before_the_facts_are_read() {
        let mut form = HostingForm::default();
        typed(&mut form, Field::Address, "example.invalid:8443");
        assert_eq!(form.update(Message::Save(Field::Address)), None);
        assert_eq!(form.left(), None);
        assert_eq!(form.facts(), &Loaded::Loading);
    }

    #[test]
    fn a_field_is_sent_whole_with_only_itself_changed_and_shows_saved_once_taken() {
        let mut form = ready();
        typed(&mut form, Field::Address, " example.invalid:8443 ");
        typed(&mut form, LARGEST, "1024");
        assert_eq!(note(&form, Field::Address), Note::Unsaved);
        let Some(Ask::Set(sent)) = form.update(Message::Save(Field::Address)) else { panic!("not asked") };
        assert_eq!(sent.public_address.as_deref(), Some("example.invalid:8443"));
        assert_eq!(sent.attachment_max_mib, HostingField::AttachmentMax.default_value(), "the other is not sent");
        assert_eq!(note(&form, Field::Address), Note::Saving);
        assert_eq!(form.update(Message::Save(LARGEST)), None, "one ask at a time");
        form.answered(&Ok(()));
        assert_eq!(note(&form, Field::Address), Note::Saved);
        assert_eq!(note(&form, LARGEST), Note::Unsaved);
    }

    #[test]
    fn a_value_out_of_its_bounds_is_refused_by_field_and_never_sent() {
        let mut form = ready();
        let ceiling = HostingField::AttachmentMax.ceiling() + 1;
        typed(&mut form, LARGEST, &ceiling.to_string());
        assert_eq!(form.update(Message::Save(LARGEST)), None);
        assert_eq!(
            note(&form, LARGEST),
            Note::Refused(Refusal::Typed(HostingRefused::OutOfRange(HostingField::AttachmentMax)))
        );
        for text in ["many", "", "0"] {
            typed(&mut form, LARGEST, text);
            assert_eq!(form.update(Message::Save(LARGEST)), None, "{text:?}");
        }
        typed(&mut form, STORAGE, "0");
        assert_eq!(form.update(Message::Save(STORAGE)), None);
        assert_eq!(
            note(&form, STORAGE),
            Note::Refused(Refusal::Typed(HostingRefused::OutOfRange(HostingField::AccountFilesMax)))
        );
        typed(&mut form, Field::Address, "https://example.invalid");
        assert_eq!(form.update(Message::Save(Field::Address)), None);
        assert_eq!(note(&form, Field::Address), Note::Refused(Refusal::Typed(HostingRefused::PublicAddress)));
    }

    #[test]
    fn a_largest_file_above_the_storage_it_is_within_is_refused_on_the_largest_file() {
        let mut form = ready();
        typed(&mut form, STORAGE, "40");
        assert_eq!(form.update(Message::Save(STORAGE)), None);
        assert_eq!(
            note(&form, LARGEST),
            Note::Refused(Refusal::Typed(HostingRefused::Above {
                field: HostingField::AttachmentMax,
                within: HostingField::AccountFilesMax
            }))
        );
        assert_eq!(fields(&form).said(LARGEST), Some(FieldSaid::refusal(Sentence::HostingWithin(40))));
    }

    #[test]
    fn a_refusal_between_the_numbers_goes_once_either_is_typed_again() {
        let mut form = ready();
        typed(&mut form, STORAGE, "40");
        form.update(Message::Save(STORAGE));
        assert!(matches!(note(&form, LARGEST), Note::Refused(_)));
        typed(&mut form, STORAGE, "1024");
        assert_eq!(note(&form, LARGEST), Note::Unchanged, "what it was refused against is gone");
        assert_eq!(fields(&form).said(LARGEST), None);
    }

    #[test]
    fn enter_in_one_number_saves_the_other_as_it_is_typed() {
        let mut form = ready();
        typed(&mut form, STORAGE, "1024");
        typed(&mut form, LARGEST, "500");
        let Some(Ask::Set(sent)) = form.update(Message::Save(LARGEST)) else { panic!("not asked") };
        assert_eq!((sent.attachment_max_mib, sent.account_files_max_mib), (500, Some(1024)));
        assert_eq!((note(&form, LARGEST), note(&form, STORAGE)), (Note::Saving, Note::Saving));
    }

    #[test]
    fn a_storage_left_empty_is_no_limit_and_is_sent_as_none() {
        let mut form = ready();
        assert!(!fields(&form).unlimited(STORAGE), "the default is a limit");
        assert!(!fields(&form).unlimited(LARGEST) && !fields(&form).unlimited(Field::Address));
        typed(&mut form, STORAGE, "4096");
        assert!(!fields(&form).unlimited(STORAGE));
        let Some(Ask::Set(sent)) = form.update(Message::Save(STORAGE)) else { panic!("not asked") };
        assert_eq!(sent.account_files_max_mib, Some(4096));
        form.answered(&Ok(()));
        typed(&mut form, STORAGE, "  ");
        assert!(fields(&form).unlimited(STORAGE), "empty: it shows No limit");
        let Some(Ask::Set(sent)) = form.update(Message::Save(STORAGE)) else { panic!("not asked") };
        assert_eq!(sent.account_files_max_mib, None, "empty is no limit");
    }

    #[test]
    fn a_refused_field_says_why_in_the_numbers_typed_now() {
        let mut form = ready();
        typed(&mut form, LARGEST, "many");
        form.update(Message::Save(LARGEST));
        assert_eq!(fields(&form).said(LARGEST), Some(FieldSaid::refusal(Sentence::Phrase(Phrase::WholeNumber))));
        let ceiling = HostingField::AttachmentMax.ceiling() + 1;
        typed(&mut form, LARGEST, &ceiling.to_string());
        form.update(Message::Save(LARGEST));
        assert_eq!(
            fields(&form).said(LARGEST),
            Some(FieldSaid::refusal(Sentence::HostingMost(HostingField::AttachmentMax)))
        );
        typed(&mut form, LARGEST, "0");
        form.update(Message::Save(LARGEST));
        assert_eq!(
            fields(&form).said(LARGEST),
            Some(FieldSaid::refusal(Sentence::HostingLeast(HostingField::AttachmentMax)))
        );
        typed(&mut form, LARGEST, &held_largest());
        assert_eq!(fields(&form).said(LARGEST), None, "back on what the server holds");
        typed(&mut form, LARGEST, "17");
        assert_eq!(fields(&form).said(LARGEST), Some(FieldSaid::help(Sentence::Phrase(Phrase::PressEnterToSave))));
    }

    #[test]
    fn the_address_line_warns_while_no_address_is_typed() {
        let mut form = ready();
        typed(&mut form, Field::Address, "  ");
        assert_eq!(fields(&form).address_line(), AddressLine::Empty);
        typed(&mut form, Field::Address, "example.invalid:8443");
        assert_eq!(fields(&form).address_line(), AddressLine::Explained);
    }

    #[test]
    fn every_number_the_server_holds_shows_once_after_the_address() {
        let mut shown: Vec<HostingField> = Field::NUMBERS
            .iter()
            .map(|field| match field {
                Field::Number(number) => *number,
                Field::Address => panic!("the address is no number"),
            })
            .collect();
        shown.sort_by_key(|field| field.name());
        let mut held = HostingField::ALL.to_vec();
        held.sort_by_key(|field| field.name());
        assert_eq!(shown, held);
        assert_eq!(Field::ALL[0], Field::Address);
        assert_eq!(Field::ALL[1..], Field::NUMBERS);
    }

    #[test]
    fn the_servers_refusal_is_kept_on_the_field_and_the_facts_stay_as_they_were() {
        let mut form = ready();
        typed(&mut form, LARGEST, "99");
        form.update(Message::Save(LARGEST));
        let refused = LinkError::Refused(ErrorCode::NotAuthorized);
        form.answered(&Err(refused.clone()));
        assert_eq!(note(&form, LARGEST), Note::Refused(Refusal::Server(refused)));
        typed(&mut form, LARGEST, &held_largest());
        assert_eq!(note(&form, LARGEST), Note::Unchanged, "back on what the server holds");
    }

    #[test]
    fn leaving_saves_everything_typed_together_once() {
        let mut form = ready();
        typed(&mut form, LARGEST, "99");
        typed(&mut form, STORAGE, "100");
        let Some(Ask::Set(sent)) = form.left() else { panic!("not asked") };
        assert_eq!((sent.attachment_max_mib, sent.account_files_max_mib), (99, Some(100)));
        assert_eq!(form.left(), None, "asked once");
    }

    #[test]
    fn facts_read_again_keep_what_is_typed_and_take_the_rest() {
        let mut form = ready();
        typed(&mut form, LARGEST, "99");
        let mut told = Hosting::DEFAULT.to_wire();
        told.account_files_max_mib = Some(77);
        form.loaded(&Ok(ServerHosting { hosting: Some(told), switched_off: Vec::new() }));
        assert_eq!(fields(&form).typed(LARGEST), "99");
        assert_eq!(fields(&form).typed(STORAGE), "77");
    }

    #[test]
    fn facts_that_could_not_be_read_are_asked_for_again_on_request() {
        let mut form = HostingForm::default();
        form.loaded(&Err(LinkError::Timeout));
        assert_eq!(form.facts(), &Loaded::Failed(LinkError::Timeout));
        assert_eq!(form.update(Message::Reload), Some(Ask::Get));
        assert_eq!(form.facts(), &Loaded::Loading);
    }
}
