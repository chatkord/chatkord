//! The pictures a place has the host hold: those a view drew that the host did not hold, asked once a run each, and
//! what it does once the host lets one go.

use super::*;
use std::collections::HashSet;

impl Place {
    /// A view drew `files` and the host holds none of them: each this place names (a member's picture) or fetched
    /// before (an emoji, a thumbnail, a picture sent) and has not asked for since is asked now, once until it is let go
    /// again. The server's own is asked by [`Place::to_paint`]; a message's files first by the discussion that shows
    /// them.
    pub(super) fn missing(&mut self, files: Vec<FileId>) -> Vec<Ask> {
        let pictured: HashSet<FileId> = self.directory.people().filter_map(|(_, person)| person.picture).collect();
        let mut todo = Vec::new();
        for file in files {
            let ask = if pictured.contains(&file) {
                Ask::LookUpPicture { picture: file }
            } else if let Some(kind) = self.kinds.get(&file) {
                Ask::FetchFile { file, kind: *kind }
            } else {
                continue;
            };
            if let std::collections::hash_map::Entry::Vacant(free) = self.files.entry(file) {
                free.insert(Fetched::Asked);
                todo.push(ask);
            }
        }
        todo
    }

    /// The host let go of `file`, which no view drew: the next view that draws it asks for it again.
    pub(super) fn let_go(&mut self, file: FileId) -> Vec<Ask> {
        if self.files.get(&file) == Some(&Fetched::Ready) {
            self.files.remove(&file);
        }
        if self.painted == Some(file) {
            self.painted = None;
        }
        self.to_paint().into_iter().collect()
    }

    /// The host let go of every picture this place had it hold, as it does at a sign-out: each is asked again once
    /// drawn, the server's own now.
    pub(super) fn forget_pictures(&mut self) -> Option<Ask> {
        self.files.clear();
        self.painted = None;
        self.to_paint()
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, member, tell};
    use crate::place::{self, Named, PeopleMessage, PeoplePage};
    use proto::files::FileId;
    use proto::status::Shown;
    use std::time::Instant;

    const OWL: FileId = FileId::from_bytes([7; FileId::LEN]);
    const STRANGER: FileId = FileId::from_bytes([9; FileId::LEN]);
    const BO: &str = "bo";

    fn bo_pictured() -> (crate::Window, place::PlaceId, Clock) {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, proto::rules::Grants::default());
        let bo = Named { name: "Bo".into(), colour: None, roles: Vec::new(), picture: Some(OWL) };
        let page = PeoplePage { entries: vec![(member(BO), bo, Shown::Online)], next: None, presence: 1 };
        tell(&mut window, &clock, owls, place::Message::People(PeopleMessage::Page(Ok(page))));
        (window, owls, clock)
    }

    fn drawn(files: Vec<FileId>) -> place::Message {
        place::Message::Server(place::ServerMessage::Missing(files))
    }

    fn looked_up(todo: &[crate::Ask]) -> Vec<FileId> {
        todo.iter()
            .filter_map(|ask| match ask {
                crate::Ask::At(_, place::Ask::LookUpPicture { picture }) => Some(*picture),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_picture_drawn_but_not_held_is_asked_once_and_one_nobody_here_named_never() {
        let (mut window, owls, clock) = bo_pictured();
        assert_eq!(looked_up(&tell(&mut window, &clock, owls, drawn(vec![OWL, STRANGER]))), [OWL]);
        assert!(tell(&mut window, &clock, owls, drawn(vec![OWL, STRANGER])).is_empty(), "asked once a run");
    }

    #[test]
    fn a_picture_the_host_let_go_is_asked_again_once_drawn_again() {
        let (mut window, owls, clock) = bo_pictured();
        tell(&mut window, &clock, owls, drawn(vec![OWL]));
        tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::PictureReady(OWL)));
        let todo = tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::LetGo(OWL)));
        assert!(todo.is_empty(), "nothing until a view draws it: {todo:?}");
        assert_eq!(looked_up(&tell(&mut window, &clock, owls, drawn(vec![OWL]))), [OWL]);
    }
}
