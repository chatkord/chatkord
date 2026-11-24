//! The window's words in one language: its Fluent catalog, read once, among the catalogs this build carries. A model
//! or a view says a [`Phrase`] or a [`crate::Sentence`] through the `Words` it was given; nothing holds a sentence of
//! its own.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use std::borrow::Cow;
use std::fmt;
use unic_langid::LanguageIdentifier;

include!(concat!(env!("OUT_DIR"), "/words.rs"));

/// A catalog as the build carries it.
struct Catalog {
    /// Its language tag, canonical, which names its file.
    tag: &'static str,
    /// Its language in its own name.
    name: &'static str,
    source: &'static str,
}

impl Catalog {
    /// Its tag as an identifier; the build refused a file whose name is not one, so it is always there.
    fn id(&self) -> Option<LanguageIdentifier> {
        self.tag.parse().ok()
    }
}

/// A language this build carries a catalog for, the only kind a window can be said in.
#[derive(Clone, Copy)]
pub struct Language(&'static Catalog);

impl Language {
    /// English, the catalog every id is generated from and the one said where nothing else is asked for.
    pub const ENGLISH: Language = Language(&ENGLISH);

    /// Every language this build carries, in the order of their own names.
    pub fn all() -> impl Iterator<Item = Language> {
        CATALOGS.iter().map(Language)
    }

    /// The carried language `id` names exactly; none for one this build has no catalog for.
    pub fn of(id: &LanguageIdentifier) -> Option<Language> {
        Language::all().find(|language| language.0.id().as_ref() == Some(id))
    }

    /// The carried language that best answers `wanted`, a person's languages most wanted first: the first that one
    /// of them names, its region aside where need be, else English.
    pub fn negotiate(wanted: &[LanguageIdentifier]) -> Language {
        negotiated(wanted, Language::all())
    }

    /// Its language tag, canonical: what a choice of it is kept as.
    pub fn tag(self) -> &'static str {
        self.0.tag
    }

    /// Its own name for itself, which a list of languages shows it by.
    pub fn name(self) -> &'static str {
        self.0.name
    }
}

/// The language of `among` that best answers `wanted`, else English.
fn negotiated(wanted: &[LanguageIdentifier], among: impl Iterator<Item = Language>) -> Language {
    let offered: Vec<(LanguageIdentifier, Language)> =
        among.filter_map(|language| language.0.id().map(|id| (id, language))).collect();
    let ids: Vec<LanguageIdentifier> = offered.iter().map(|(id, _)| id.clone()).collect();
    let best = negotiate_languages(wanted, &ids, None, NegotiationStrategy::Lookup).into_iter().next();
    best.and_then(|best| offered.iter().find(|(id, _)| id == best)).map_or(Language::ENGLISH, |(_, language)| *language)
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.0.tag == other.0.tag
    }
}

impl Eq for Language {}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Language").field(&self.0.tag).finish()
    }
}

#[cfg(any(test, feature = "english"))]
impl Language {
    /// A catalog a test brings, of a language this build may not carry: what the window says once it changes to it.
    /// It lives as long as the test's process.
    pub fn given(tag: &'static str, name: &'static str, source: &'static str) -> Language {
        Language(Box::leak(Box::new(Catalog { tag, name, source })))
    }
}

/// One language's catalog.
pub struct Words {
    bundle: FluentBundle<FluentResource>,
    language: Language,
}

impl Words {
    /// The catalog of `language`.
    pub fn new(language: Language) -> Self {
        // The build parsed every carried catalog and refused it whole on any fault, so the resource is complete.
        let resource = FluentResource::try_new(language.0.source.to_string()).unwrap_or_else(|(resource, _)| resource);
        let mut bundle = FluentBundle::new_concurrent(language.0.id().into_iter().collect());
        // A name or a number set into a sentence keeps its own direction in a right-to-left language.
        bundle.set_use_isolating(true);
        // The only refusal is an id defined twice, which the build refused.
        let _ = bundle.add_resource(resource);
        Words { bundle, language }
    }

    /// The English catalog, which every id is generated from.
    pub fn english() -> Self {
        Words::new(Language::ENGLISH)
    }

    /// The language these words are in.
    pub fn language(&self) -> Language {
        self.language
    }

    /// What `phrase` says.
    pub fn phrase(&self, phrase: Phrase) -> Cow<'_, str> {
        self.format(phrase.id(), None)
    }

    /// What `text` says.
    pub fn text(&self, text: Text) -> Cow<'_, str> {
        self.format(text.id(), None)
    }

    /// What `phrase` says as a control's label. The build refuses a carried catalog whose phrase is not plain text,
    /// or is empty or longer than a label holds, so this cannot fail.
    #[expect(clippy::expect_used, reason = "every phrase of every carried catalog is checked as a label by the build")]
    pub fn label(&self, phrase: Phrase) -> design::Label {
        design::Label::new(&self.phrase(phrase)).expect("the build holds every phrase to the bound of a label")
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> Cow<'_, str> {
        let Some(pattern) = self.bundle.get_message(id).and_then(|message| message.value()) else {
            // Every id is generated from the English catalog, so only a translation can lack one.
            return Cow::Owned(id.to_string());
        };
        let mut errors = Vec::new();
        let said = self.bundle.format_pattern(pattern, args, &mut errors);
        // The marks change nothing in a line with no right-to-left letter, and would cost it its plain shaping.
        if said.chars().any(right_to_left) || !said.chars().any(isolating) {
            said
        } else {
            Cow::Owned(without_isolation(&said))
        }
    }
}

#[cfg(any(test, feature = "english"))]
impl Phrase {
    /// What a test reads: the English it says.
    pub fn in_english(self) -> String {
        without_isolation(&Words::english().phrase(self))
    }
}

#[cfg(any(test, feature = "english"))]
impl Text {
    /// What a test reads: the English it says.
    pub fn in_english(self) -> String {
        without_isolation(&Words::english().text(self))
    }
}

#[cfg(any(test, feature = "english"))]
impl crate::Sentence {
    /// What a test reads: the English it says.
    pub fn in_english(&self) -> String {
        without_isolation(&Words::english().say(self))
    }
}

impl Words {
    /// `sentence` said without the marks that isolate its placeables: for a terminal, which has no shaping that
    /// hides them, and for a test that compares the words.
    pub fn said(&self, sentence: &crate::Sentence) -> String {
        without_isolation(&self.say(sentence))
    }
}

/// `said` without the marks that isolate a placeable's direction (U+2066 to U+2069).
pub fn without_isolation(said: &str) -> String {
    said.chars().filter(|mark| !isolating(*mark)).collect()
}

/// Whether `mark` isolates a placeable's direction.
fn isolating(mark: char) -> bool {
    ('\u{2066}'..='\u{2069}').contains(&mark)
}

/// Whether `letter` is written right to left, as Hebrew and Arabic are.
fn right_to_left(letter: char) -> bool {
    matches!(unicode_bidi::bidi_class(letter), unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL)
}

impl fmt::Debug for Words {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Words").field("language", &self.language).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    /// Room for the longest notice the design draws (an invite that stopped working), and no paragraph.
    const MOST_CHARS: usize = 120;

    fn english() -> Vec<catalog::Message> {
        catalog::read(ENGLISH.source).unwrap_or_default()
    }

    #[test]
    fn every_phrase_is_short_plain_text() {
        let words = Words::english();
        let phrases: Vec<_> = english().into_iter().filter(|message| message.variables.is_empty()).collect();
        assert!(phrases.len() > 300, "{}", phrases.len());
        for message in phrases {
            let said = words.format(&message.id, None);
            assert!(
                !said.trim().is_empty() && said.chars().count() <= MOST_CHARS && !said.contains('\n'),
                "{}",
                message.id
            );
        }
    }

    /// What is told to a person as a sentence ends like one; a label or a caption does not.
    #[test]
    fn a_message_to_a_person_is_a_sentence() {
        let words = Words::english();
        for phrase in [
            Phrase::SavedSignInExpired,
            Phrase::SavedSignInHeld,
            Phrase::KeyringUnavailable,
            Phrase::AddBody,
            Phrase::SignInThere,
            Phrase::NameHint,
            Phrase::NameCharacters,
            Phrase::QuietForNow,
            Phrase::TooManyTries,
            Phrase::Untrusted,
            Phrase::OldEncryption,
            Phrase::SelfSigned,
            Phrase::KeyChanged,
            Phrase::KeyChangedWhy,
            Phrase::PaneNotConnected,
            Phrase::OtherVersion,
            Phrase::ServerTooOld,
            Phrase::UpdateToJoin,
            Phrase::NotALink,
            Phrase::FellBehind,
            Phrase::UpdateDownloading,
            Phrase::UpdateReady,
            Phrase::UpdateFailed,
            Phrase::UpdateWaitsForCall,
            Phrase::UpdateUnchecked,
            Phrase::UpdateUpToDate,
            Phrase::UpdateNotChecked,
            Phrase::UpdatesInPackage,
            Phrase::AutoUpdateHint,
            Phrase::PickVoiceChannel,
            Phrase::DoNotDisturbOn,
            Phrase::DoNotDisturbOffNote,
            Phrase::PaneNotAnswering,
            Phrase::PaneSignedOut,
            Phrase::PickAnother,
            Phrase::SignOutBody,
            Phrase::OwnerLeaveBody,
            Phrase::PasswordDoesntMatch,
            Phrase::SendThisLink,
            Phrase::NoInvites,
            Phrase::DeleteInviteBody,
            Phrase::CantDeleteInvite,
            Phrase::PlainTextOnly,
            Phrase::DeleteVoiceBody,
            Phrase::CantManageChannels,
            Phrase::ChannelGone,
            Phrase::SameAsNow,
            Phrase::PasswordNotAccepted,
            Phrase::NameNotAccepted,
            Phrase::ColourHexHint,
            Phrase::NameNotAllowed,
            Phrase::SetUpBody,
            Phrase::ZoomKeys,
            Phrase::OnThisDevice,
            Phrase::DeletedBody,
            Phrase::PasswordChanged,
            Phrase::NothingAnswers,
            Phrase::NoSuchHost,
            Phrase::NoNetwork,
            Phrase::NotAChatkordServer,
            Phrase::NoAnswerInTime,
            Phrase::ServerStoppedAnswering,
            Phrase::ConnectionEnded,
            Phrase::VoiceChannelClosed,
            Phrase::ClosedSignedOut,
            Phrase::ClosedSessionExpired,
            Phrase::ClosedKicked,
            Phrase::ClosedUnreachable,
            Phrase::ClosedClientShutdown,
            Phrase::ClosedServerStopping,
            Phrase::ClosedMoved,
            Phrase::ClosedNotAllowed,
            Phrase::ClosedChannelDeleted,
            Phrase::ClosedTakenOut,
            Phrase::ClosedOverRate,
            Phrase::ClosedPasswordChanged,
            Phrase::ClosedPasswordReset,
            Phrase::ClosedAccountDeleted,
            Phrase::ClosedBanned,
            Phrase::RefusedNoVoiceAddress,
            Phrase::RefusedNoVoiceAddressManage,
            Phrase::CallFaulted,
            Phrase::StartTooSlow,
            Phrase::PictureUnreadable,
            Phrase::FileSignedOut,
            Phrase::CantChangePicture,
            Phrase::PictureGone,
            Phrase::NotAPicture,
            Phrase::ServerFull,
            Phrase::TooManyRequests,
            Phrase::ServerCouldntDoIt,
            Phrase::FileNotTaken,
            Phrase::ScreenUnreachable,
            Phrase::ScreenNeedsCopy,
            Phrase::ScreenSizeUnshareable,
            Phrase::CaptureStopped,
            Phrase::CaptureNeedsPipeWire,
            Phrase::ShareEnded,
            Phrase::ScreenSilent,
            Phrase::EncoderRefusedSize,
            Phrase::VideoHardwareClosed,
            Phrase::ScreenResized,
            Phrase::EncodeStopped,
            Phrase::ShareSoundNeedsPipeWire,
            Phrase::ShareSoundNeedsNewerWindows,
            Phrase::ShareSoundStream,
            Phrase::ShareSoundStopped,
            Phrase::CameraDenied,
            Phrase::CameraBusy,
            Phrase::CameraGone,
            Phrase::CameraFormat,
            Phrase::CameraUnavailable,
            Phrase::VoiceOffer,
            Phrase::VoiceEngine,
            Phrase::EngineStalled,
            Phrase::SoundStopped,
            Phrase::AlreadySending,
            Phrase::SoundUnavailable,
            Phrase::DeviceMissing,
            Phrase::DeviceRefused,
            Phrase::PickerNoPortal,
            Phrase::PickerNeedsNewerWindows,
            Phrase::PickerUnavailable,
            Phrase::NothingChosen,
            Phrase::PickerFailed,
            Phrase::CannotDecode,
            Phrase::ScreenNotCarried,
            Phrase::ShareSoundNotCarried,
            Phrase::ShareNowOnProcessor,
            Phrase::ShareNowOnAnotherEncoder,
        ] {
            assert!(words.phrase(phrase).ends_with('.'), "{phrase:?}");
        }
        for text in [
            Text::TokenLeft,
            Text::InviteRefused,
            Text::SelfSignedBody,
            Text::KeyChangedBody,
            Text::HeldOwnMuted,
            Text::HeldOwnOn,
            Text::RemoveBody,
            Text::VideoHardwareNeedsCopy,
        ] {
            assert!(words.text(text).ends_with('.'), "{text:?}");
        }
    }

    #[test]
    fn every_catalog_has_every_id_and_its_values() {
        let reference = english();
        assert!(!reference.is_empty());
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("catalogs");
        let mut read = 0;
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "ftl") {
                let source = std::fs::read_to_string(&path).unwrap_or_default();
                let other = catalog::read(&source);
                assert_eq!(other.as_ref().map(|other| catalog::differences(&reference, other)), Ok(vec![]), "{path:?}");
                read += 1;
            }
        }
        assert!(read >= 1, "no catalog in {folder:?}");
        assert_eq!(Language::all().count(), read, "the build carries every catalog in {folder:?}");
    }

    /// Each carried catalog is named by its canonical tag, says its own name, and is what its words are in.
    #[test]
    fn every_carried_language_names_itself_and_is_found_by_its_tag() {
        for language in Language::all() {
            let id: LanguageIdentifier = language.tag().parse().unwrap();
            assert_eq!(id.to_string(), language.tag());
            assert_eq!(Language::of(&id), Some(language));
            let words = Words::new(language);
            assert_eq!(words.language(), language);
            assert_eq!(words.phrase(Phrase::LanguageName), language.name());
        }
        assert_eq!((Language::ENGLISH.tag(), Language::ENGLISH.name()), ("en", "English"));
        assert_eq!(Language::of(&"en-US".parse().unwrap()), None, "only a carried tag, exactly");
    }

    #[test]
    fn a_persons_languages_find_the_first_carried_one_else_english() {
        let french = Language::given("fr", "Français", "language-name = Français\n");
        let brazilian = Language::given("pt-BR", "Português (Brasil)", "language-name = Português (Brasil)\n");
        let among = [Language::ENGLISH, french, brazilian];
        let asked = |tags: &[&str]| {
            let ids: Vec<LanguageIdentifier> = tags.iter().map(|tag| tag.parse().unwrap()).collect();
            negotiated(&ids, among.into_iter())
        };
        assert_eq!(asked(&["fr"]), french, "exactly");
        assert_eq!(asked(&["fr-CA", "en-US"]), french, "the first asked, its region aside");
        assert_eq!(asked(&["de-DE", "en-GB", "fr"]), Language::ENGLISH, "the first carried, in the person's order");
        assert_eq!(asked(&["pt"]), brazilian, "a language with only a regional catalog");
        assert_eq!(asked(&["de", "ja"]), Language::ENGLISH, "nothing carried");
        assert_eq!(asked(&[]), Language::ENGLISH, "nothing asked");
        assert_eq!(Language::negotiate(&["en-AU".parse().unwrap()]), Language::ENGLISH);
    }

    #[test]
    fn words_in_another_language_say_its_catalog_and_its_plurals() {
        let french = Language::given(
            "fr",
            "Français",
            "language-name = Français\n\
             people = { $count ->\n    [one] { $count } personne\n   *[other] { $count } personnes\n}\n",
        );
        let words = Words::new(french);
        assert_eq!(words.phrase(Phrase::LanguageName), "Français");
        assert_eq!(without_isolation(&words.people(1)), "1 personne");
        assert_eq!(without_isolation(&words.people(2)), "2 personnes");
        assert_eq!(words.phrase(Phrase::Close), Phrase::Close.id(), "a missing id says itself, never English");
    }

    #[test]
    fn a_translation_that_lacks_or_adds_an_id_or_a_value_is_found() {
        let reference = catalog::read("a = A\nb = B { $n }\nc = C { $x } { $y }\n").unwrap_or_default();
        let other = catalog::read("b = B\nc = C { $x } { $y } { $z }\nd = D\n").unwrap_or_default();
        let names = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
        assert_eq!(
            catalog::differences(&reference, &other),
            vec![
                catalog::Mismatch::Missing { id: "a".into() },
                catalog::Mismatch::Variables { id: "b".into(), wanted: names(&["n"]), found: names(&[]) },
                catalog::Mismatch::Variables {
                    id: "c".into(),
                    wanted: names(&["x", "y"]),
                    found: names(&["x", "y", "z"])
                },
                catalog::Mismatch::Extra { id: "d".into() },
            ]
        );
    }

    #[test]
    fn a_message_named_by_another_shares_its_values() {
        let read = catalog::read("day = { $day }\nuntil = until { day }\n").unwrap_or_default();
        assert_eq!(read.iter().map(|message| message.variables.len()).collect::<Vec<_>>(), [1, 1]);
        assert_eq!(
            catalog::read("a = { missing }\n"),
            Err(catalog::Fault::UnknownMessage { id: "a".into(), named: "missing".into() })
        );
        assert_eq!(catalog::read("a = A\na = B\n"), Err(catalog::Fault::Twice { id: "a".into() }));
    }

    #[test]
    fn a_phrase_that_is_not_a_label_is_refused_at_build() {
        let longest = "α".repeat(design::MAX_LABEL_CHARS);
        let english = catalog::read(&format!("a = A\n# {} the note.\nb = B\n", catalog::TEXT_MARK)).unwrap();
        let checked = |source: &str| catalog::labels(&english, &catalog::read(source).unwrap());
        assert_eq!(checked(&format!("a = {longest}\nb = B\n")), Ok(()));
        assert_eq!(
            checked(&format!("a = {longest}α\nb = B\n")),
            Err(catalog::Fault::NotLabel { id: "a".into(), why: design::LabelError::TooLong })
        );
        assert_eq!(checked("a = {\"\"}\nb = B\n"), Err(catalog::Fault::LabelNotPlain { id: "a".into() }));
        assert_eq!(checked(&format!("a = A\nb = {longest}α\n")), Ok(()), "text is not a label");
    }

    /// The build reads a label's length from its source as the bundle would say it, so `Words::label` holds.
    #[test]
    fn every_label_of_every_carried_catalog_is_said_as_the_build_measured_it() {
        let english = english();
        for language in Language::all() {
            let words = Words::new(language);
            for message in catalog::read(language.0.source).unwrap() {
                if english.iter().any(|ours| ours.id == message.id && ours.used() == catalog::Use::Label) {
                    let said = words.format(&message.id, None);
                    assert_eq!(message.plain.as_deref(), Some(&*said), "{} {}", language.tag(), message.id);
                }
            }
        }
    }

    #[test]
    fn a_message_is_written_as_its_catalog_writes_it_on_one_line() {
        let read = catalog::read("# Said once.\n-brand = Chatkord\nabout = About { -brand }\n  and more\n");
        let text = read.unwrap_or_default().into_iter().map(|message| message.text).collect::<Vec<_>>();
        assert_eq!(text, ["About { -brand } and more"]);
    }

    #[test]
    fn a_placeable_is_isolated_only_in_a_line_with_a_right_to_left_letter() {
        let words = Words::english();
        assert_eq!(words.say(&crate::Sentence::ScreenOf("Ada".into())), "Ada’s stream");
        assert_eq!(words.say(&crate::Sentence::ScreenOf("עדה".into())), "\u{2068}עדה\u{2069}’s stream");
    }

    #[test]
    fn a_plural_and_a_month_are_chosen_by_the_number() {
        let words = Words::english();
        assert_eq!(without_isolation(&words.people(1)), "1 person");
        assert_eq!(without_isolation(&words.people(25)), "25 people");
        let day = words.day_month(DayMonthValues { day: 29.into(), month: 2.into() });
        assert_eq!(without_isolation(&day), "29 February");
    }
}
