//! Server settings › Overview, an admin's: the server's picture, set once it is sent; its name,
//! saved on Enter or when the page is left; its colour, saved when the picker closes; the switches of what the server
//! offers; and the facts of how it is hosted. Each follows a change made elsewhere unless it is being changed here.

use crate::colour::{self, Picker};
use crate::door::{CHATKORD, Server};
use crate::hosting::{self, HostingForm};
use crate::name_field::{self, NameField};
use crate::picture_row::{self, PictureRow};
use crate::place::Served;
use proto::chatkord_v1::ServerHosting;
use proto::files::FileId;
use proto::rules::{Colour, Switch};
use terms::LinkError;

/// A switch of what the server offers, as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toggle {
    switch: Switch,
    on: bool,
    asking: Option<bool>,
    refused: Option<LinkError>,
}

impl Toggle {
    fn over(switch: Switch, served: &Served) -> Toggle {
        Toggle { switch, on: served.on(switch), asking: None, refused: None }
    }

    /// Which switch it is.
    pub fn switch(&self) -> Switch {
        self.switch
    }

    /// Whether it is on: while a change is asked, as asked.
    pub fn on(&self) -> bool {
        self.asking.unwrap_or(self.on)
    }

    /// Whether a change is asked and not yet answered, which holds it.
    pub fn asking(&self) -> bool {
        self.asking.is_some()
    }

    /// Why the last change was not taken, for the view to word; gone once it is changed again.
    pub fn refused(&self) -> Option<&LinkError> {
        self.refused.as_ref()
    }
}

/// The page, open.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    pub(crate) picture: PictureRow,
    pub(crate) name: NameField,
    pub(crate) colour: Picker,
    /// The switches, in [`Switch::ALL`]'s order.
    pub(crate) switches: Vec<Toggle>,
    /// The hosting facts.
    pub(crate) hosting: HostingForm,
}

/// What a person does on the page.
#[derive(Clone, Debug)]
pub enum Message {
    /// Something done in the picture's row.
    Picture(picture_row::Message),
    /// Something done in the name's field.
    Name(name_field::Message),
    /// Something done in the colour's picker.
    Colour(colour::Message),
    /// A switch turned on (true) or off.
    Switch(Switch, bool),
    /// Something done in the hosting form.
    Hosting(hosting::Message),
}

/// What the page asks: the system's picker, a picture sent; and of the server a picture, none to take it off; a name,
/// none to leave it nameless; a colour, none for Chatkord's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    PickPicture,
    UploadPicture(Vec<u8>),
    Picture(Option<FileId>),
    Name(Option<String>),
    Colour(Option<Colour>),
    Switch(Switch, bool),
    Hosting(hosting::Ask),
}

impl Overview {
    /// Opened on what `server` says of itself now and `served` of what it offers.
    pub(crate) fn new(server: &Server, served: &Served) -> Overview {
        Overview {
            picture: PictureRow::new(server.picture),
            name: NameField::new(server.name.clone().unwrap_or_default()),
            colour: Picker::new(server.colour, CHATKORD),
            switches: Switch::ALL.into_iter().map(|switch| Toggle::over(switch, served)).collect(),
            hosting: HostingForm::default(),
        }
    }

    /// The switches, in the order they show.
    pub fn switches(&self) -> &[Toggle] {
        &self.switches
    }

    /// The hosting form.
    pub fn hosting(&self) -> &HostingForm {
        &self.hosting
    }

    /// The row of the server's picture.
    pub fn picture(&self) -> &PictureRow {
        &self.picture
    }

    /// The field of the server's name, empty while it has none.
    pub fn name(&self) -> &NameField {
        &self.name
    }

    /// The picker of the server's colour, on Chatkord's while it has none.
    pub fn colour(&self) -> &Picker {
        &self.colour
    }

    pub(crate) fn update(&mut self, message: Message) -> Option<Ask> {
        match message {
            Message::Picture(message) => self.picture.update(message).map(|asked| match asked {
                picture_row::Ask::Pick => Ask::PickPicture,
                picture_row::Ask::Upload(bytes) => Ask::UploadPicture(bytes),
                picture_row::Ask::Hold(picture) => Ask::Picture(picture),
            }),
            Message::Name(message) => self.name.update(message).map(Ask::Name),
            Message::Colour(message) => self.colour.update(message).map(Ask::Colour),
            Message::Switch(switch, on) => {
                let toggle = self.switches.iter_mut().find(|toggle| toggle.switch == switch)?;
                if toggle.asking.is_some() || toggle.on == on {
                    return None;
                }
                toggle.asking = Some(on);
                toggle.refused = None;
                Some(Ask::Switch(switch, on))
            }
            Message::Hosting(message) => self.hosting.update(message).map(Ask::Hosting),
        }
    }

    /// The server's answer to switching `switch` to `on`.
    pub(crate) fn switched(&mut self, switch: Switch, on: bool, answer: &Result<(), LinkError>) {
        if let Some(toggle) = self.switches.iter_mut().find(|toggle| toggle.switch == switch) {
            toggle.asking = None;
            match answer {
                Ok(()) => toggle.on = on,
                Err(failed) => toggle.refused = Some(failed.clone()),
            }
        }
    }

    /// The server said what it offers now, here or on another device: a switch nobody is turning follows it.
    pub(crate) fn offered(&mut self, served: &Served) {
        for toggle in self.switches.iter_mut().filter(|toggle| toggle.asking.is_none()) {
            toggle.on = served.on(toggle.switch);
        }
    }

    /// The server's answer to asking how it is hosted, and which switches are off.
    pub(crate) fn listed(&mut self, answer: &Result<ServerHosting, LinkError>) {
        self.hosting.loaded(answer);
        if let Ok(told) = answer {
            let off = Switch::off(&told.switched_off);
            for toggle in self.switches.iter_mut().filter(|toggle| toggle.asking.is_none()) {
                toggle.on = !off.contains(&toggle.switch);
            }
        }
    }

    /// What leaving the page saves: a name typed and not yet sent, and a colour the open picker holds.
    pub(crate) fn left(&mut self) -> Vec<Ask> {
        let colour = if self.colour.is_open() { self.colour.update(colour::Message::Done) } else { None };
        let hosting = self.hosting.left().map(Ask::Hosting);
        [self.name.ask().map(Ask::Name), colour.map(Ask::Colour), hosting].into_iter().flatten().collect()
    }

    /// The server said what it is now, here or on another device.
    pub(crate) fn follow(&mut self, server: &Server) {
        self.picture.follow(server.picture);
        self.name.follow(server.name.clone().unwrap_or_default());
        self.colour.follow(server.colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, overview_open, owls, tell};
    use crate::harness::{sends, wire};
    use crate::place::{self, Panel};
    use crate::settings::Page;
    use std::time::Instant;

    fn night_owls(colour: Option<Colour>) -> Server {
        Server { colour, ..owls() }
    }

    #[test]
    fn a_name_saves_on_enter_and_a_colour_when_the_picker_closes() {
        let mut page = Overview::new(&night_owls(None), &Served::read(&[]));
        assert_eq!(page.name().typed(), "Night Owls");
        page.update(Message::Name(name_field::Message::Typed("Owls".into())));
        assert_eq!(page.update(Message::Name(name_field::Message::Save)), Some(Ask::Name(Some("Owls".into()))));
        page.update(Message::Colour(colour::Message::Toggle));
        page.update(Message::Colour(colour::Message::Typed("#3d7fd6".into())));
        let blue = Colour::parse("#3D7FD6").ok();
        assert_eq!(page.update(Message::Colour(colour::Message::Done)), Some(Ask::Colour(blue)));
    }

    #[test]
    fn leaving_saves_what_is_typed_and_what_the_open_picker_holds() {
        let mut page = Overview::new(&night_owls(None), &Served::read(&[]));
        page.update(Message::Name(name_field::Message::Typed(String::new())));
        page.update(Message::Colour(colour::Message::Toggle));
        page.update(Message::Colour(colour::Message::Typed("#3d7fd6".into())));
        assert_eq!(page.left(), [Ask::Name(None), Ask::Colour(Colour::parse("#3D7FD6").ok())]);
        assert_eq!(page.left(), [], "asked once");
    }

    #[test]
    fn a_picture_sent_is_asked_of_the_server_and_remove_takes_it_off() {
        use crate::picture_row::{Message as Row, Picked};
        let owl = FileId::from_bytes([7; FileId::LEN]);
        let mut page = Overview::new(&night_owls(None), &Served::read(&[]));
        assert_eq!(page.update(Message::Picture(Row::Choose)), Some(Ask::PickPicture));
        assert_eq!(
            page.update(Message::Picture(Row::Picked(Picked::File(vec![1])))),
            Some(Ask::UploadPicture(vec![1]))
        );
        assert_eq!(page.update(Message::Picture(Row::Uploaded(Ok(owl)))), Some(Ask::Picture(Some(owl))));
        assert_eq!(page.update(Message::Picture(Row::Remove)), Some(Ask::Picture(None)));
    }

    #[test]
    fn a_change_from_elsewhere_moves_what_nobody_is_changing() {
        let mut page = Overview::new(&night_owls(None), &Served::read(&[]));
        let blue = Colour::parse("#3D7FD6").ok();
        page.follow(&Server { name: Some("Owls".into()), ..night_owls(blue) });
        assert_eq!((page.name().typed(), page.colour().picked()), ("Owls", Colour::parse("#3D7FD6").expect("blue")));
    }

    fn settled() -> Served {
        Served::read(&[Switch::Conversations, Switch::LinkPreviews, Switch::Search, Switch::Welcome])
    }

    #[test]
    fn a_switch_is_asked_once_shown_as_asked_and_kept_or_put_back_by_the_answer() {
        let mut page = Overview::new(&night_owls(None), &settled());
        let on = |page: &Overview, switch| page.switches().iter().find(|t| t.switch() == switch).map(Toggle::on);
        assert_eq!((on(&page, Switch::Threads), on(&page, Switch::Search)), (Some(true), Some(false)));
        assert_eq!(page.update(Message::Switch(Switch::Search, true)), Some(Ask::Switch(Switch::Search, true)));
        assert_eq!(on(&page, Switch::Search), Some(true), "shown as asked");
        assert_eq!(page.update(Message::Switch(Switch::Search, false)), None, "held while asked");
        let refused = LinkError::Refused(proto::chatkord_v1::ErrorCode::NotAuthorized);
        page.switched(Switch::Search, true, &Err(refused.clone()));
        assert_eq!(on(&page, Switch::Search), Some(false), "put back");
        assert_eq!(
            page.switches().iter().find(|t| t.switch() == Switch::Search).and_then(Toggle::refused),
            Some(&refused)
        );
        page.update(Message::Switch(Switch::Threads, false));
        page.switched(Switch::Threads, false, &Ok(()));
        assert_eq!(on(&page, Switch::Threads), Some(false));
    }

    #[test]
    fn what_the_server_offers_now_moves_a_switch_nobody_is_turning() {
        let mut page = Overview::new(&night_owls(None), &settled());
        page.update(Message::Switch(Switch::Threads, false));
        page.offered(&Served::read(&[Switch::Conversations, Switch::LinkPreviews, Switch::Threads, Switch::Welcome]));
        let on = |switch| page.switches().iter().find(|t| t.switch() == switch).map(Toggle::on);
        assert_eq!((on(Switch::Search), on(Switch::Threads)), (Some(true), Some(false)), "the asked one is held");
    }

    #[test]
    fn opening_the_page_asks_for_the_hosting_facts() {
        use proto::rules::Grant;
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, [Grant::ManageServer].into_iter().collect());
        let open = place::Message::Page(place::PageMessage::Settings(Page::Overview));
        let todo = tell(&mut window, &clock, owls, open);
        assert!(todo.iter().any(|done| matches!(done, crate::Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::GetServerHosting))), "{todo:?}");
    }

    #[test]
    fn the_release_the_server_greeted_with_is_the_places_to_show() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = overview_open(&clock);
        assert_eq!(window.place(owls).unwrap().build(), None);
        let build = terms::ServerBuild::read("0.9.0").unwrap();
        tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::Built(build.clone())));
        assert_eq!(window.place(owls).unwrap().build(), build.as_ref());
    }

    #[test]
    fn the_protocol_the_server_greeted_with_is_kept_and_none_before_it_has() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = overview_open(&clock);
        assert_eq!(window.place(owls).unwrap().protocol(), None);
        let spoke = proto::chatkord_v1::ProtoVersion { major: 1, minor: 0 };
        tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::Spoke(spoke)));
        assert_eq!(window.place(owls).unwrap().protocol(), Some(spoke));
    }

    #[test]
    fn about_is_the_last_page_and_holds_the_report_overview_no_longer_does() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = overview_open(&clock);
        assert_eq!(window.place(owls).unwrap().pages().last(), Some(&Page::About));
        assert!(window.place(owls).unwrap().server_report().is_none(), "Overview holds no report");
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::Settings(Page::About)));
        let place = window.place(owls).unwrap();
        assert_eq!(place.settings().map(crate::ServerSettings::page), Some(Page::About));
        assert!(place.server_report().is_some());
    }

    #[test]
    fn overview_is_an_admins_first_page() {
        let clock = Clock(Instant::now());
        let (window, owls) = overview_open(&clock);
        let place = window.place(owls).unwrap();
        assert_eq!(place.pages(), [Page::Overview, Page::Members, Page::About]);
        assert!(matches!(place.panel(), Panel::Settings));
    }

    #[test]
    fn a_name_saves_on_enter_and_leaving_overview_saves_what_the_open_picker_holds() {
        use crate::colour::Message as Picking;
        use crate::name_field::Message as Naming;
        use crate::overview::Message as On;
        let clock = Clock(Instant::now());
        let (mut window, owls) = overview_open(&clock);
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Page(place::PageMessage::Overview(On::Name(Naming::Typed("Owls".into())))),
        );
        let todo =
            tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::Overview(On::Name(Naming::Save))));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)] if sends(ask, wire::SetServerName { name: Some("Owls".into()) })),
            "{todo:?}"
        );
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Page(place::PageMessage::Overview(On::Colour(Picking::Toggle))),
        );
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Page(place::PageMessage::Overview(On::Colour(Picking::Typed("#3d7fd6".into())))),
        );
        let todo = tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::CloseSettings));
        let blue = proto::rules::Colour::parse("#3D7FD6").ok();
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)] if sends(ask, wire::SetServerColour { colour: blue.map(|blue| blue.to_string()) })),
            "{todo:?}"
        );
    }

    #[test]
    fn a_refused_server_colour_is_said_and_the_picker_returns_to_the_saved_one() {
        use crate::colour::Message as Picking;
        use crate::overview::Message as On;
        let clock = Clock(Instant::now());
        let (mut window, owls) = overview_open(&clock);
        let on = |picking| place::Message::Page(place::PageMessage::Overview(On::Colour(picking)));
        tell(&mut window, &clock, owls, on(Picking::Toggle));
        tell(&mut window, &clock, owls, on(Picking::Typed("#3d7fd6".into())));
        let todo = tell(&mut window, &clock, owls, on(Picking::Done));
        assert_eq!(todo.len(), 1, "{todo:?}");
        let refused = Err(LinkError::Refused(proto::chatkord_v1::ErrorCode::NotAuthorized));
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::ServerColourSet(refused)));
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("That colour wasn’t saved, so the old one is back.")
        );
        let page = window.place(owls).unwrap().settings().and_then(crate::settings::ServerSettings::overview).unwrap();
        assert!(page.colour().is_fallback() && !page.colour().is_open(), "back on the one it had: none");
    }

    #[test]
    fn a_change_the_server_tells_is_shown_kept_and_followed_by_an_open_overview() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = overview_open(&clock);
        crate::harness::every_switch_on(&mut window, &clock, owls);
        let identity = terms::ServerIdentity {
            name: Some("Owls".into()),
            colour: proto::rules::Colour::parse("#3D7FD6").ok(),
            picture: None,
        };
        let event = proto::chatkord_v1::Event {
            cursor: Vec::new(),
            kind: Some(proto::chatkord_v1::event::Kind::ServerChanged(proto::chatkord_v1::ServerChanged {
                name: Some("Owls".into()),
                colour: Some("#3d7fd6".into()),
                picture: None,
                ..proto::chatkord_v1::ServerChanged::default()
            })),
        };
        let heard = place::Message::heard(&event).expect("a change to the server");
        let todo = tell(&mut window, &clock, owls, heard);
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::RememberServer { identity: kept, .. })] if *kept == identity),
            "{todo:?}"
        );
        let place = window.place(owls).unwrap();
        assert_eq!(place.server().shown(), "Owls");
        assert_eq!(place.server().colour(), design::Rgb::hex(0x3D7FD6));
        let open = place.settings().and_then(crate::settings::ServerSettings::overview).unwrap();
        assert_eq!(open.name().typed(), "Owls", "the open page follows");
        let malformed = proto::chatkord_v1::Event {
            cursor: Vec::new(),
            kind: Some(proto::chatkord_v1::event::Kind::ServerChanged(proto::chatkord_v1::ServerChanged {
                name: None,
                colour: Some("plaid".into()),
                picture: None,
                ..proto::chatkord_v1::ServerChanged::default()
            })),
        };
        assert!(place::Message::heard(&malformed).is_none(), "a colour out of shape is passed over");
    }
}
