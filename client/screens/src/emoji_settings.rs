//! Server settings › Emoji, as Discord's page: Upload emoji picks a picture and sends it, then the new emoji is named
//! in place before it is added; a card's pencil renames its emoji in place and its bin asks first. A name out of shape
//! is said under its field and never sent; what the server refuses is said once on the page.

use crate::picture_row::Picked;
use chat::EmojiId;
use proto::files::{FileId, FileKind, FileRefusal};
use proto::rules::{EMOJI_UPLOAD_BYTES, EmojiName, MAX_EMOJI, emoji_name};
use terms::{FileError, LinkError};
use words::Sentence;

/// The page's own state; the emoji are the place's list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmojiSettings {
    doing: Doing,
    naming: Option<Naming>,
    refused: Option<Sentence>,
}

/// What Upload emoji is waiting on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Doing {
    /// Nothing: it can be pressed.
    #[default]
    Nothing,
    /// The system's picker is open.
    Picking,
    /// The picture is going up.
    Uploading,
}

/// Which emoji a name is typed for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// One the server holds, renamed.
    Kept(EmojiId),
    /// A new one, its picture sent as this file and not yet added.
    New(FileId),
}

/// A name being typed in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Naming {
    named: Named,
    draft: String,
    misshapen: bool,
    asking: bool,
}

impl Naming {
    /// Which emoji it names.
    pub fn named(&self) -> &Named {
        &self.named
    }

    /// The name as typed, without colons.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Saved out of shape: the rule is said under the field until the name changes.
    pub fn misshapen(&self) -> bool {
        self.misshapen
    }

    /// Sent, and the server's answer awaited.
    pub fn asking(&self) -> bool {
        self.asking
    }
}

/// What a person does on the page, and what the host and the server answer.
#[derive(Clone, Debug)]
pub enum Message {
    /// Upload emoji pressed.
    Upload,
    /// The system's picker closed.
    Picked(Picked),
    /// The picture went up as this file, or why not.
    Uploaded(Result<FileId, FileError>),
    /// A card's pencil: its emoji renamed in place.
    Rename(EmojiId),
    /// The name in the field, as typed.
    Draft(String),
    /// Enter or Save in the field.
    Save,
    /// Escape or Cancel in the field.
    Cancel,
    /// A card's bin: the window asks first.
    Delete(EmojiId),
    /// What the page says, put away.
    Dismiss,
    /// The server's answer to adding the new emoji.
    Added(Result<(), LinkError>),
    /// The server's answer to a rename.
    Renamed(Result<(), LinkError>),
    /// The server's answer to deleting one.
    Deleted(Result<(), LinkError>),
}

/// What the page asks of its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// Open the system's picker for a picture.
    Pick,
    /// Send these bytes as an emoji's picture.
    Upload(Vec<u8>),
    /// Fetch what the server made of this picture, to paint it.
    Fetch(FileId),
    /// Add the emoji named so with the picture sent.
    Add {
        /// Its name.
        name: EmojiName,
        /// Its picture.
        file: FileId,
    },
    /// Rename an emoji.
    Rename {
        /// The emoji.
        emoji: EmojiId,
        /// Its new name.
        name: EmojiName,
    },
}

impl EmojiSettings {
    /// What Upload emoji is waiting on; while it waits, a press does nothing.
    pub fn doing(&self) -> Doing {
        self.doing
    }

    /// The name being typed in place, if one is.
    pub fn naming(&self) -> Option<&Naming> {
        self.naming.as_ref()
    }

    /// What the server or the picker last refused, until it is put away or the next ask goes through.
    pub fn refused(&self) -> Option<&Sentence> {
        self.refused.as_ref()
    }

    /// What `message` changes, with `held` the emoji the server holds now and `name_of` their names by id.
    pub(crate) fn update(
        &mut self,
        message: Message,
        held: usize,
        name_of: impl Fn(&EmojiId) -> Option<EmojiName>,
    ) -> Vec<Ask> {
        match message {
            Message::Upload if self.doing == Doing::Nothing && held < MAX_EMOJI => {
                self.doing = Doing::Picking;
                self.refused = None;
                vec![Ask::Pick]
            }
            Message::Upload => Vec::new(),
            Message::Picked(picked) if self.doing == Doing::Picking => {
                self.doing = Doing::Nothing;
                match picked {
                    Picked::Nothing => Vec::new(),
                    Picked::Unreadable => self.refuse_file(&FileError::Unreadable { kind: FileKind::Emoji }),
                    Picked::File(bytes) if u64::try_from(bytes.len()).map_or(true, |len| len > EMOJI_UPLOAD_BYTES) => {
                        self.refuse_file(&FileError::Refused { kind: FileKind::Emoji, refusal: FileRefusal::TooLarge })
                    }
                    Picked::File(bytes) => {
                        self.doing = Doing::Uploading;
                        vec![Ask::Upload(bytes)]
                    }
                }
            }
            Message::Uploaded(answer) if self.doing == Doing::Uploading => {
                self.doing = Doing::Nothing;
                match answer {
                    Ok(file) => {
                        self.naming = Some(Naming::new(Named::New(file), String::new()));
                        vec![Ask::Fetch(file)]
                    }
                    Err(failed) => self.refuse_file(&failed),
                }
            }
            // An answer to nothing asked.
            Message::Picked(_) | Message::Uploaded(_) => Vec::new(),
            Message::Rename(emoji) => {
                if let Some(name) = name_of(&emoji) {
                    self.naming = Some(Naming::new(Named::Kept(emoji), name.as_str().to_string()));
                }
                Vec::new()
            }
            Message::Draft(draft) => {
                if let Some(naming) = self.naming.as_mut().filter(|naming| !naming.asking) {
                    naming.draft = draft;
                    naming.misshapen = false;
                }
                Vec::new()
            }
            Message::Save => self.save(&name_of),
            Message::Cancel => {
                self.naming = None;
                Vec::new()
            }
            // The window's: it asks first.
            Message::Delete(_) => Vec::new(),
            Message::Dismiss => {
                self.refused = None;
                Vec::new()
            }
            Message::Added(answer) | Message::Renamed(answer) => {
                match answer {
                    Ok(()) => {
                        self.naming = None;
                        self.refused = None;
                    }
                    Err(failed) => {
                        if let Some(naming) = self.naming.as_mut() {
                            naming.asking = false;
                        }
                        self.refused = Some(words::failure::said(&failed));
                    }
                }
                Vec::new()
            }
            Message::Deleted(answer) => {
                self.refused = answer.err().map(|failed| words::failure::said(&failed));
                Vec::new()
            }
        }
    }

    /// The name typed, sent when it is in shape and new.
    fn save(&mut self, name_of: &impl Fn(&EmojiId) -> Option<EmojiName>) -> Vec<Ask> {
        let Some(naming) = self.naming.as_mut().filter(|naming| !naming.asking) else { return Vec::new() };
        let Ok(name) = emoji_name(&naming.draft) else {
            naming.misshapen = true;
            return Vec::new();
        };
        let ask = match &naming.named {
            Named::Kept(emoji) if name_of(emoji).as_ref() == Some(&name) => {
                self.naming = None;
                return Vec::new();
            }
            Named::Kept(emoji) => Ask::Rename { emoji: emoji.clone(), name },
            Named::New(file) => Ask::Add { name, file: *file },
        };
        naming.asking = true;
        self.refused = None;
        vec![ask]
    }

    fn refuse_file(&mut self, failed: &FileError) -> Vec<Ask> {
        self.refused = Some(words::failure::file(failed));
        Vec::new()
    }
}

impl Naming {
    fn new(named: Named, draft: String) -> Naming {
        Naming { named, draft, misshapen: false, asking: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWL: &str = "0190f5a0-0000-7000-8000-000000000001";
    const PICTURE: &str = "0190f5a000007000800000000000aaaa";

    fn owl() -> EmojiId {
        EmojiId::parse(OWL).unwrap()
    }

    fn picture() -> FileId {
        FileId::parse(PICTURE).unwrap()
    }

    pub(crate) fn named(name: &str) -> impl Fn(&EmojiId) -> Option<EmojiName> {
        let name = emoji_name(name).unwrap();
        move |emoji| (*emoji == owl()).then(|| name.clone())
    }

    #[test]
    fn an_upload_is_named_in_place_then_added_and_a_misshapen_name_is_never_sent() {
        let mut page = EmojiSettings::default();
        let none = named("owlwave");
        assert_eq!(page.update(Message::Upload, 12, &none), [Ask::Pick]);
        assert_eq!(page.update(Message::Picked(Picked::File(vec![1, 2])), 12, &none), [Ask::Upload(vec![1, 2])]);
        assert_eq!(page.update(Message::Uploaded(Ok(picture())), 12, &none), [Ask::Fetch(picture())]);
        page.update(Message::Draft("owl wave!".into()), 12, &none);
        assert!(page.update(Message::Save, 12, &none).is_empty());
        assert!(page.naming().unwrap().misshapen());
        page.update(Message::Draft("party_owl".into()), 12, &none);
        let asked = page.update(Message::Save, 12, &none);
        assert_eq!(asked, [Ask::Add { name: emoji_name("party_owl").unwrap(), file: picture() }]);
        page.update(Message::Added(Err(LinkError::Refused(proto::chatkord_v1::ErrorCode::AlreadyExists))), 12, &none);
        assert!(page.naming().is_some_and(|naming| !naming.asking()), "a refused name stays to be changed");
        assert!(page.refused().is_some());
        page.update(Message::Save, 12, &none);
        page.update(Message::Added(Ok(())), 13, &none);
        assert_eq!((page.naming(), page.refused()), (None, None));
    }

    #[test]
    fn a_full_server_offers_no_upload_and_a_picture_too_large_is_refused_before_it_is_sent() {
        let mut page = EmojiSettings::default();
        let none = named("owlwave");
        assert!(page.update(Message::Upload, MAX_EMOJI, &none).is_empty());
        page.update(Message::Upload, 0, &none);
        let large = vec![0; usize::try_from(EMOJI_UPLOAD_BYTES).unwrap() + 1];
        assert!(page.update(Message::Picked(Picked::File(large)), 0, &none).is_empty());
        assert_eq!(page.refused(), Some(&Sentence::EmojiTooLarge));
        assert_eq!(page.doing(), Doing::Nothing);
    }

    #[test]
    fn a_rename_sends_only_a_new_name() {
        let mut page = EmojiSettings::default();
        let held = named("owlwave");
        page.update(Message::Rename(owl()), 1, &held);
        assert_eq!(page.naming().map(Naming::draft), Some("owlwave"));
        assert!(page.update(Message::Save, 1, &held).is_empty());
        assert!(page.naming().is_none(), "the same name closes the field");
        page.update(Message::Rename(owl()), 1, &held);
        page.update(Message::Draft("owl_wave".into()), 1, &held);
        assert_eq!(
            page.update(Message::Save, 1, &held),
            [Ask::Rename { emoji: owl(), name: emoji_name("owl_wave").unwrap() }]
        );
    }
}
