//! The server-wide search as the window draws it: the bar over the pane and the content, the magnifier a folded
//! window's headers hold, and the layer the search opens. What it finds, and what a press sends, is `screens`'.

use crate::discussion::{Handles, author_avatar, author_name, author_picture, files_alone, unmarked};
use crate::labels::{label, phrase};
use crate::pins_panel::Pin;
use crate::{FoundFile, Renderer, SearchBody, Theme};
use design::MessageSize;
use iced_core::{Element, Size};
use screens::chat;
use screens::{Place, PlaceMessage, SearchMessage, SearchStage, Window, WindowMessage as Message};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

fn said(place: &Place, message: SearchMessage) -> Message {
    Message::At(place.id(), PlaceMessage::Search(message))
}

/// Whether `place` offers the search a bar for: it is signed in to a server that serves it.
pub(crate) fn offered(place: &Place) -> bool {
    place.search().is_some()
}

fn name(place: &Place, words: &Words) -> design::Label {
    label(&words.say(&Sentence::SearchMessages(place.server().shown().to_string())))
}

/// The top bar across the pane and the content of `place`'s server: the search's field in its middle where the server
/// offers the search, `end` at its end.
pub(crate) fn bar<'a, R: Renderer + 'a>(place: &Place, end: Vec<Part<'a, R>>, words: &Words) -> Part<'a, R> {
    let field = place.search().map(|search| {
        let hint = words.phrase(Phrase::SearchShortcut).into_owned();
        crate::search_pill(name(place, words), hint, search.open, said(place, SearchMessage::Open))
    });
    crate::top_bar(field, end)
}

/// The magnifier of a header whose bar is folded away; none where the search is not offered.
pub(crate) fn magnifier<'a, R: Renderer + 'a>(place: &Place, words: &Words) -> Option<Part<'a, R>> {
    let search = place.search()?;
    Some(crate::search_magnifier(name(place, words), search.open, said(place, SearchMessage::Open)))
}

/// The layer over a window of `size` while `place`'s search is open; `handles` gives the authors' pictures.
pub(crate) fn layer<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    size: Size,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Option<(Part<'a, R>, Message)> {
    let search = place.search().filter(|search| search.open)?;
    let close = said(place, SearchMessage::Close);
    let field = crate::search_field(
        name(place, words),
        &words.phrase(Phrase::SearchPlaceholder),
        phrase(Phrase::ClearSearch, words),
        &search.typed,
        move |typed| said(place, SearchMessage::Type(typed)),
        said(place, SearchMessage::Clear),
    );
    let today = window.today();
    let placed = |found: &chat::Found| match place.found_in(found) {
        screens::FoundIn::Channel(channel) => Some(format!("#{channel}")),
        screens::FoundIn::Thread(channel) => Some(words.say(&Sentence::SearchInThread(channel))),
        screens::FoundIn::Person(person) => {
            Some(person.unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned()))
        }
        screens::FoundIn::Unknown => None,
    };
    let body = match &search.stage {
        SearchStage::Blank => SearchBody::Start,
        SearchStage::Waiting { .. } | SearchStage::Asking(_) => SearchBody::Waiting,
        SearchStage::Failed { .. } => SearchBody::Failed(said(place, SearchMessage::Retry)),
        SearchStage::Found { found, .. } if found.items().is_empty() => {
            SearchBody::Nothing(search.searched().to_owned())
        }
        SearchStage::Found { .. } => {
            let messages = search
                .messages()
                .filter_map(|found| {
                    let message = &found.message;
                    let body = message.body.as_ref()?;
                    let pin = Pin {
                        name: author_name(place, &message.author, words),
                        avatar: author_avatar(place, &message.author),
                        picture: author_picture(place, &message.author, handles),
                        at: chat::local(message.created_at, window.zone())
                            .map(|at| words.say(&Sentence::PinnedAt { at, today }))
                            .unwrap_or_default(),
                        text: unmarked(body, false),
                        body: if body.text().is_empty() {
                            files_alone(words)
                        } else {
                            crate::RichBody::new(body, MessageSize::DEFAULT, words)
                                .channels(move |channel| place.opens_channel(channel))
                                .view()
                        },
                        jump: said(place, SearchMessage::OpenFound(message.id.clone())),
                        unpin: None,
                    };
                    Some((placed(found), pin))
                })
                .collect();
            let files = place.found_files().map(|found| {
                found
                    .into_iter()
                    .map(|file| {
                        let message = &file.found.message;
                        let note = chat::local(message.created_at, window.zone()).map(|at| {
                            words.say(&Sentence::FoundFile {
                                size: file.attached.size,
                                place: placed(file.found).unwrap_or_default(),
                                at,
                                today,
                            })
                        });
                        FoundFile {
                            name: file.attached.name.as_str().to_owned(),
                            note: note.unwrap_or_default(),
                            picture: file.picture(),
                            jump: said(place, SearchMessage::OpenFound(message.id.clone())),
                        }
                    })
                    .collect()
            });
            SearchBody::Found { messages, files, more: search.more().then(|| said(place, SearchMessage::More)) }
        }
    };
    Some((crate::search_layer(name(place, words), field, body, close.clone(), size, words), close))
}
