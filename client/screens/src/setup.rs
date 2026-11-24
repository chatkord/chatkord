//! The first visit of whoever owns a server, at a server that offers it: a name, a picture and a
//! colour people will recognise, saved together, or skipped. A picture is sent when it is chosen and set only by Save.

use crate::colour::{self, Picker};
use crate::door::{CHATKORD, Server};
use crate::picture_row::{self, Doing, PictureRow};
use crate::place;
use crate::place::ops;
use crate::window::{Confirming, DialogMessage, Next};
use proto::files::FileId;
use proto::rules::{Colour, MAX_NAME_CHARS, text_shape};

/// The dialog's fields.
#[derive(Clone, Debug, PartialEq)]
pub struct SetUp {
    typed: String,
    /// The name, picture and colour as they were when the dialog opened, so Save sends only a change.
    named: Option<String>,
    pictured: Option<FileId>,
    coloured: Option<Colour>,
    picture: PictureRow,
    colour: Picker,
}

/// What Save sends: each field that changed, `None` for one that did not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Saved {
    pub name: Option<String>,
    pub picture: Option<Option<FileId>>,
    pub colour: Option<Option<Colour>>,
}

/// What a person does in it.
#[derive(Clone, Debug)]
pub enum Message {
    /// The name field's whole text after an edit.
    Name(String),
    /// Something done in the picture's row.
    Picture(picture_row::Message),
    /// Something done in the colour's picker.
    Colour(colour::Message),
}

impl SetUp {
    pub(crate) fn new(server: &Server) -> SetUp {
        SetUp {
            typed: server.name.clone().unwrap_or_default(),
            named: server.name.clone(),
            pictured: server.picture,
            coloured: server.colour,
            picture: PictureRow::new(server.picture),
            colour: Picker::new(server.colour, CHATKORD),
        }
    }

    /// What the name field holds, untrimmed; the server's name when the dialog opened.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// The row of the server's picture; what it holds is set only by Save.
    pub fn picture(&self) -> &PictureRow {
        &self.picture
    }

    /// The picker of the server's colour, on Chatkord's while it has none.
    pub fn colour(&self) -> &Picker {
        &self.colour
    }

    /// Whether Save may be pressed: a name is there, of the shape a name others read has.
    pub fn ready(&self) -> bool {
        text_shape(&self.typed, MAX_NAME_CHARS).is_ok()
    }

    /// Whether a press outside would lose something: a name or picture changed, a picture on its way, or a colour
    /// being picked.
    pub(crate) fn kept(&self) -> bool {
        self.typed.trim() != self.named.as_deref().unwrap_or_default()
            || self.picture.held() != self.pictured
            || self.picture.doing() != Doing::Nothing
            || self.colour.is_open()
    }

    /// What the picture row asks of the host: its picker, or a picture sent.
    pub(crate) fn update(&mut self, message: Message) -> Option<picture_row::Ask> {
        match message {
            Message::Name(typed) => self.typed = typed,
            Message::Picture(message) => {
                return self.picture.update(message).filter(|asked| !matches!(asked, picture_row::Ask::Hold(_)));
            }
            Message::Colour(message) => drop(self.colour.update(message)),
        }
        None
    }

    /// Save: the name, the picture and the colour, each when it differs from the server's own; a picker still open is
    /// closed first. Not while a picture is on its way.
    pub(crate) fn saved(&mut self) -> Option<Saved> {
        let name = text_shape(&self.typed, MAX_NAME_CHARS).ok()?.to_string();
        if self.picture.doing() != Doing::Nothing {
            return None;
        }
        self.colour.update(colour::Message::Done);
        let picked = (!self.colour.is_fallback()).then(|| self.colour.picked());
        let colour = (picked != self.coloured).then_some(picked);
        let picture = (self.picture.held() != self.pictured).then_some(self.picture.held());
        let name = (Some(name.as_str()) != self.named.as_deref()).then_some(name);
        Some(Saved { name, picture, colour })
    }
}

/// The set-up dialog: a picture sent when it is chosen, and the rest saved together.
impl Confirming for SetUp {
    fn holds_outside(&self) -> bool {
        self.kept()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::SetUp(message) => {
                self.update(message).map_or(Next::Stays, |asked| Next::Asks(picture_asked(asked)))
            }
            DialogMessage::Confirm => {
                let Some(saved) = self.saved() else { return Next::Stays };
                let name = saved.name.map(|name| ops::set_server_name(Some(name)));
                let picture = saved.picture.map(ops::set_server_picture);
                let colour = saved.colour.map(ops::set_server_colour);
                Next::Closing(name.into_iter().chain(picture).chain(colour).collect())
            }
            DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::CopyTemporary
            | DialogMessage::Named(_)
            | DialogMessage::Making(_) => Next::Stays,
        }
    }
}

/// What the picture row asks, as the host carries it out.
fn picture_asked(asked: picture_row::Ask) -> place::Ask {
    match asked {
        picture_row::Ask::Pick => place::Ask::PickPicture,
        picture_row::Ask::Upload(bytes) => {
            place::Ask::UploadPicture { bytes, kind: proto::files::FileKind::ServerPicture }
        }
        picture_row::Ask::Hold(picture) => ops::set_server_picture(picture),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::{self, Heard, Server};
    use crate::harness::{ADA, Clock, owls, standing_as, tell};
    use crate::harness::{sends, wire};
    use crate::setup::Message as SetUpMessage;
    use crate::sign_in::Standing;
    use crate::window::{Asking, Dialog, DialogMessage, Window};
    use crate::{Ask, WindowMessage, place};
    use proto::rules::Grant;
    use std::time::Instant;
    use terms::Secret;
    use words::Sentence;

    fn unnamed() -> Server {
        Server { name: None, ..owls() }
    }

    #[test]
    fn save_needs_a_name_and_sends_what_changed() {
        let mut first = SetUp::new(&unnamed());
        assert!(!first.ready());
        assert_eq!(first.saved(), None);
        first.update(Message::Name(" Night Owls ".into()));
        assert!(first.ready() && first.kept());
        assert_eq!(
            first.saved(),
            Some(Saved { name: Some("Night Owls".into()), ..Saved::default() }),
            "no colour picked, none sent"
        );
        first.update(Message::Colour(colour::Message::Toggle));
        first.update(Message::Colour(colour::Message::Typed("#3d7fd6".into())));
        let blue = Colour::parse("#3D7FD6").ok();
        assert_eq!(
            first.saved(),
            Some(Saved { name: Some("Night Owls".into()), colour: Some(blue), ..Saved::default() }),
            "the open picker's colour too"
        );
    }

    #[test]
    fn a_colour_kept_by_closing_the_picker_is_saved_with_the_name() {
        let mut first = SetUp::new(&unnamed());
        first.update(Message::Name("Night Owls".into()));
        first.update(Message::Colour(colour::Message::Toggle));
        first.update(Message::Colour(colour::Message::Typed("#2fa36b".into())));
        first.update(Message::Colour(colour::Message::Done));
        let green = Colour::parse("#2FA36B").ok();
        assert_eq!(
            first.saved(),
            Some(Saved { name: Some("Night Owls".into()), colour: Some(green), ..Saved::default() }),
            "closed before Save, still sent"
        );
        first.update(Message::Colour(colour::Message::Toggle));
        first.update(Message::Colour(colour::Message::Reset));
        first.update(Message::Colour(colour::Message::Done));
        assert_eq!(
            first.saved(),
            Some(Saved { name: Some("Night Owls".into()), ..Saved::default() }),
            "back on the server's own: nothing to send"
        );
    }

    #[test]
    fn a_picture_sent_is_set_only_by_save_and_not_while_it_goes_up() {
        use crate::picture_row::{Ask, Message as Row, Picked};
        let owl = FileId::from_bytes([7; FileId::LEN]);
        let mut first = SetUp::new(&unnamed());
        first.update(Message::Name("Night Owls".into()));
        assert_eq!(first.update(Message::Picture(Row::Choose)), Some(Ask::Pick));
        assert_eq!(first.update(Message::Picture(Row::Picked(Picked::File(vec![1])))), Some(Ask::Upload(vec![1])));
        assert!(first.kept());
        assert_eq!(first.saved(), None, "Save waits for the picture");
        assert_eq!(first.update(Message::Picture(Row::Uploaded(Ok(owl)))), None, "held, not set");
        let saved = first.saved().expect("ready");
        assert_eq!(saved.picture, Some(Some(owl)));
    }

    #[test]
    fn a_name_out_of_shape_cannot_be_saved() {
        let mut first = SetUp::new(&unnamed());
        first.update(Message::Name("n".repeat(MAX_NAME_CHARS + 1)));
        assert!(!first.ready());
        first.update(Message::Name("a\u{202E}b".into()));
        assert!(!first.ready());
    }

    /// An owner registers at a server that offers `server-identity`, through the invite that makes them its owner.
    fn owner_arrives(clock: &Clock) -> Window {
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        let adding = window.welcome_door().unwrap().id();
        let code = "Ow9kP2".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        tell(
            &mut window,
            clock,
            adding,
            door(door::Message::Where(format!("https://chat.example.invalid/join#{code}"))),
        );
        tell(&mut window, clock, adding, door(door::Message::Submit));
        let server = Server { ..owls() };
        let found = door::Found { server, invite: Some(door::Invite::Works { until: 0, owner: true }) };
        tell(&mut window, clock, adding, door(door::Message::Reported(Heard::Found(Ok(found)))));
        tell(&mut window, clock, adding, door(door::Message::Submit));
        tell(&mut window, clock, adding, door(door::Message::Username(ADA.into())));
        let password = || Secret::new("evening games with friends".into());
        tell(&mut window, clock, adding, door(door::Message::Password(password())));
        tell(&mut window, clock, adding, door(door::Message::Again(password())));
        tell(&mut window, clock, adding, door(door::Message::Submit));
        let standing = Standing {
            place: proto::rules::Place::OWNER,
            grants: Grant::ALL.into_iter().collect(),
            ..standing_as("ada")
        };
        let admitted = door::Admitted { token: None, named: None, standing };
        tell(&mut window, clock, adding, door(door::Message::Reported(Heard::Registered(Ok(admitted)))));
        window
    }

    /// Save sends what changed, and the note that they own the server is there after.
    #[test]
    fn an_owner_s_first_arrival_asks_for_a_name_and_a_colour_and_save_sends_them() {
        use crate::colour::Message as Picking;
        let clock = Clock(Instant::now());
        let mut window = owner_arrives(&clock);
        assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::SetUp(_))), "asked for a name and a colour");
        assert!(window.note(window.open().unwrap()).is_none(), "the note waits for the dialog");
        let colour = |picking| WindowMessage::Dialog(DialogMessage::SetUp(SetUpMessage::Colour(picking)));
        window.update(colour(Picking::Toggle), clock.at());
        window.update(colour(Picking::Typed("#2fa36b".into())), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::SetUp(SetUpMessage::Name("Owls".into()))), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let green = proto::rules::Colour::parse("#2FA36B").ok();
        assert!(
            matches!(&todo[..], [
                Ask::At(_, name),
                Ask::At(_, colour),
            ] if sends(name, wire::SetServerName { name: Some("Owls".into()) })
                && sends(colour, wire::SetServerColour { colour: green.map(|green| green.to_string()) })),
            "{todo:?}"
        );
        assert!(window.dialog().is_none());
        let owls = window.open().unwrap().id();
        let said = |window: &Window| window.note(window.place(owls).unwrap()).map(|note| note.said);
        assert!(matches!(said(&window), Some(Sentence::YouOwn(_))), "the note that they own it");
        let event = proto::chatkord_v1::Event {
            cursor: Vec::new(),
            kind: Some(proto::chatkord_v1::event::Kind::ServerChanged(proto::chatkord_v1::ServerChanged {
                name: Some("Owls".into()),
                colour: Some("#2FA36B".into()),
                picture: None,
                ..proto::chatkord_v1::ServerChanged::default()
            })),
        };
        tell(&mut window, &clock, owls, place::Message::heard(&event).expect("a change to the server"));
        assert_eq!(said(&window), Some(Sentence::YouOwn("Owls".into())), "the note names the server as it is now");
    }

    #[test]
    fn skipping_the_first_dialog_sends_nothing_and_shows_the_owner_s_note() {
        let clock = Clock(Instant::now());
        let mut window = owner_arrives(&clock);
        window.update(WindowMessage::Dialog(DialogMessage::SetUp(SetUpMessage::Name("Owls".into()))), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        assert!(todo.is_empty(), "{todo:?}");
        assert!(window.dialog().is_none());
        assert!(window.note(window.open().unwrap()).is_some());
    }
}
