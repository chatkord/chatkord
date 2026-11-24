//! The members column as it is built: the lines in and around its viewport, between two spaces standing for the rest,
//! so a server of thousands builds a screenful. Every line's height is a token's, so where each stands is arithmetic.

use super::*;
use design::{PRESENCE_PICTURE, Space, TypeStyle};
use proto::status::Shown;

/// The room above the column's first line.
pub const ROSTER_TOP: f32 = Space::Xs.px();
/// The room below its last line.
pub const ROSTER_BOTTOM: f32 = Space::S.px();
/// The room either side of its lines.
pub const ROSTER_SIDE: f32 = Space::S.px();
/// The room between two lines, and between a line and a space standing for others.
pub const ROSTER_SPACING: f32 = Space::Xxs.px();
/// The room above a group's heading's words.
pub const ROSTER_HEADING_ABOVE: f32 = Space::M.px() + Space::Xxs.px();
/// The room below them.
pub const ROSTER_HEADING_BELOW: f32 = Space::Xs.px();
/// A group's heading, as tall as its words in [`TypeStyle::CaptionStrong`] and the room round them.
pub const ROSTER_HEADING: f32 = ROSTER_HEADING_ABOVE + TypeStyle::CaptionStrong.line_height() + ROSTER_HEADING_BELOW;
/// A member's row: their picture with its status and a list row's room above and below it.
pub const ROSTER_ROW: f32 = PRESENCE_PICTURE.px() + 2.0 * (Space::S.px() - Space::Xxs.px());

/// The viewport's height until the column says it: tall, so the first screen is whole on any monitor.
const FIRST_VIEWPORT: f32 = 2400.0;

/// What a heading takes of the column, with the room under it.
const HEADING_PITCH: f32 = ROSTER_HEADING + ROSTER_SPACING;
/// What a member's row takes of the column, with the room under it.
const ROW_PITCH: f32 = ROSTER_ROW + ROSTER_SPACING;

/// The column's two groups, in the order it shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterGroup {
    /// Shown online, away or not to be disturbed.
    Online,
    /// Shown offline: signed in nowhere, or invisible.
    Offline,
}

/// A member as the members column lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RosterRow<'a> {
    /// Their id at the server.
    pub user_id: &'a MemberId,
    /// The name others see.
    pub name: &'a str,
    /// The colour they are shown in: the one they picked, else their id's.
    pub colour: design::PersonColour,
    /// How they are shown, as the server last told it.
    pub presence: Shown,
    /// The picture they chose, which the view asks the host for, their initials until it holds it; none for one who
    /// chose none.
    pub picture: Option<proto::files::FileId>,
}

/// One line of the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterLine<'a> {
    /// A group's heading, [`ROSTER_HEADING`] tall, with how many members the group holds.
    Heading(RosterGroup, usize),
    /// A member's row, [`ROSTER_ROW`] tall.
    Member(RosterRow<'a>),
}

/// The lines of the members column built for where it is scrolled: who is online, then who is not, each by name, a
/// heading over each group that has anyone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Roster<'a> {
    /// The height of the space standing for the lines above those built, [`ROSTER_SPACING`] from the first; none
    /// when none is above.
    pub before: Option<f32>,
    /// The lines built, in order.
    pub lines: Vec<RosterLine<'a>>,
    /// The height of the space standing for the lines below, [`ROSTER_SPACING`] from the last built; none when none is
    /// below.
    pub after: Option<f32>,
}

/// Where the members column is scrolled, as it last said.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RosterScroll {
    /// How far its top is scrolled past, in pixels.
    offset: f32,
    /// How tall it shows.
    viewport: f32,
}

impl Default for RosterScroll {
    fn default() -> RosterScroll {
        RosterScroll { offset: 0.0, viewport: FIRST_VIEWPORT }
    }
}

/// What the members column says of itself.
#[derive(Clone, Copy, Debug)]
pub enum RosterMessage {
    /// It was scrolled or resized.
    Scrolled {
        /// How far its top is scrolled past, in pixels.
        offset: f32,
        /// How tall it shows.
        viewport: f32,
    },
}

/// A line of the column, by what it shows.
#[derive(Clone, Copy)]
enum Line<'a> {
    Heading(RosterGroup, usize),
    Member(&'a MemberId),
}

/// The column's lines, as the sizes of its groups say them: a heading over each group with anyone, then its rows.
struct Lines<'a> {
    groups: [(RosterGroup, &'a [MemberId]); 2],
}

impl<'a> Lines<'a> {
    fn shown(&self) -> impl Iterator<Item = (RosterGroup, &'a [MemberId])> + '_ {
        self.groups.iter().copied().filter(|(_, members)| !members.is_empty())
    }

    fn count(&self) -> usize {
        self.shown().map(|(_, members)| 1 + members.len()).sum()
    }

    /// Where line `line` starts, from the top of the first; past the last, the height of them all and the room under.
    fn top(&self, line: usize) -> f32 {
        let (mut top, mut left) = (0.0, line);
        for (_, members) in self.shown() {
            if left == 0 {
                return top;
            }
            left -= 1;
            let rows = left.min(members.len());
            top += HEADING_PITCH + rows as f32 * ROW_PITCH;
            left -= rows;
        }
        top
    }

    /// The line at `y` from the top of the first, with the room under it; past the last, the count.
    fn at(&self, y: f32) -> usize {
        let (mut top, mut line) = (0.0, 0);
        for (_, members) in self.shown() {
            if y < top + HEADING_PITCH {
                return line;
            }
            top += HEADING_PITCH;
            line += 1;
            let rows = members.len() as f32 * ROW_PITCH;
            if y < top + rows {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a row of the group, past its top"
                )]
                let row = ((y - top) / ROW_PITCH) as usize;
                return line + row.min(members.len().saturating_sub(1));
            }
            top += rows;
            line += members.len();
        }
        line
    }

    /// Line `line`, past the last none.
    fn line(&self, line: usize) -> Option<Line<'a>> {
        let mut left = line;
        for (group, members) in self.shown() {
            if left == 0 {
                return Some(Line::Heading(group, members.len()));
            }
            left -= 1;
            if let Some(member) = members.get(left) {
                return Some(Line::Member(member));
            }
            left -= members.len();
        }
        None
    }

    /// The lines `first..end` to build for `scroll`, overscan included.
    fn window(&self, scroll: RosterScroll) -> (usize, usize) {
        let count = self.count();
        let top = scroll.offset - ROSTER_TOP;
        let first = self.at(top - OVERSCAN).min(count);
        let end = (self.at(top + scroll.viewport + OVERSCAN) + 1).clamp(first, count);
        (first, end)
    }
}

impl Place {
    /// The column's lines: every member known, those online first.
    fn lines(&self) -> Lines<'_> {
        let (online, offline) = self.directory.groups();
        Lines { groups: [(RosterGroup::Online, online), (RosterGroup::Offline, offline)] }
    }

    /// The members column as built where it is scrolled: every member known, shown online (online, away or not to be
    /// disturbed) then offline, each in the order the directory keeps them by name. The status is the server's fact:
    /// someone in a voice room shows in that room, and an invisible one stays offline here.
    pub fn roster(&self) -> Roster<'_> {
        let lines = self.lines();
        let (first, end) = lines.window(self.roster_scroll);
        let built = (first..end).filter_map(|line| match lines.line(line)? {
            Line::Heading(group, members) => Some(RosterLine::Heading(group, members)),
            Line::Member(user_id) => self.roster_row(user_id).map(RosterLine::Member),
        });
        let lines_built = built.collect();
        let total = lines.top(lines.count());
        Roster {
            before: (first > 0).then(|| lines.top(first) - ROSTER_SPACING),
            lines: lines_built,
            after: (end < lines.count()).then(|| total - lines.top(end) - ROSTER_SPACING),
        }
    }

    /// `user_id`'s row of the column, while the directory knows them.
    fn roster_row<'a>(&'a self, user_id: &'a MemberId) -> Option<RosterRow<'a>> {
        let named = self.directory.person(user_id)?;
        Some(RosterRow {
            user_id,
            name: &named.name,
            colour: colour_of(user_id, named.colour),
            presence: self.directory.presence(user_id),
            picture: self.picture_of(user_id),
        })
    }

    /// Where `user_id`'s row stands in the column's viewport: pixels from its top to the row's, scrolled; none for one
    /// it does not list.
    pub fn roster_top(&self, user_id: &MemberId) -> Option<f32> {
        let lines = self.lines();
        let mut before = 0;
        for (_, members) in lines.shown() {
            if let Some(at) = members.iter().position(|member| member == user_id) {
                return Some(ROSTER_TOP + lines.top(before + 1 + at) - self.roster_scroll.offset);
            }
            before += 1 + members.len();
        }
        None
    }

    /// What the members column says of itself.
    pub(super) fn on_roster(&mut self, message: RosterMessage) -> Vec<Ask> {
        match message {
            RosterMessage::Scrolled { offset, viewport } => {
                self.roster_scroll = RosterScroll { offset: offset.max(0.0), viewport: viewport.max(0.0) };
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, member, tell};
    use crate::place;
    use proto::files::FileId;
    use std::time::Instant;

    const MEMBERS: usize = 5_000;
    const ONLINE: usize = 2_000;

    fn picture(at: usize) -> FileId {
        let mut bytes = [0u8; FileId::LEN];
        bytes[..8].copy_from_slice(&(at as u64 + 1).to_le_bytes());
        FileId::from_bytes(bytes)
    }

    fn crowd() -> PeoplePage {
        let entries = (0..MEMBERS)
            .map(|at| {
                let id = format!("m{at:04}");
                let named = Named { name: id.clone(), colour: None, roles: Vec::new(), picture: Some(picture(at)) };
                (member(&id), named, if at < ONLINE { Shown::Online } else { Shown::Offline })
            })
            .collect();
        PeoplePage { entries, next: None, presence: 1 }
    }

    fn members(roster: &Roster<'_>) -> Vec<String> {
        roster
            .lines
            .iter()
            .filter_map(|line| match line {
                RosterLine::Member(row) => Some(row.name.to_owned()),
                RosterLine::Heading(..) => None,
            })
            .collect()
    }

    fn scrolled(offset: f32) -> place::Message {
        place::Message::Roster(RosterMessage::Scrolled { offset, viewport: 800.0 })
    }

    #[test]
    fn a_roster_of_thousands_builds_the_rows_in_view_and_spaces_as_tall_as_the_rest() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, proto::rules::Grants::default());
        tell(&mut window, &clock, owls, place::Message::People(PeopleMessage::Page(Ok(crowd()))));
        let whole = 2.0 * ROSTER_HEADING + MEMBERS as f32 * ROSTER_ROW + (MEMBERS + 1) as f32 * ROSTER_SPACING;
        for offset in [0.0, 30_000.0, 88_000.0, 1.0e9] {
            tell(&mut window, &clock, owls, scrolled(offset));
            let roster = window.open().unwrap().roster();
            let built: f32 = roster
                .lines
                .iter()
                .map(|line| match line {
                    RosterLine::Heading(..) => ROSTER_HEADING,
                    RosterLine::Member(_) => ROSTER_ROW,
                })
                .sum();
            let spaces = [roster.before, roster.after].into_iter().flatten();
            let children = roster.lines.len() + spaces.clone().count();
            let height = spaces.sum::<f32>() + built + (children - 1) as f32 * ROSTER_SPACING;
            assert!((height - whole).abs() < 0.5, "at {offset}: {height} of {whole}");
            let rows = (800.0 + 2.0 * OVERSCAN) / ROW_PITCH;
            assert!((members(&roster).len() as f32) < rows + 3.0, "at {offset}: {} rows built", roster.lines.len());
        }
    }

    #[test]
    fn the_rows_built_are_those_at_the_offset_under_their_heading() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, proto::rules::Grants::default());
        tell(&mut window, &clock, owls, place::Message::People(PeopleMessage::Page(Ok(crowd()))));
        let top = ROSTER_TOP + HEADING_PITCH + 1_000.0 * ROW_PITCH;
        tell(&mut window, &clock, owls, scrolled(top));
        let place = window.open().unwrap();
        let roster = place.roster();
        let names = members(&roster);
        assert!(names.contains(&"m1000".to_owned()) && names.contains(&"m1010".to_owned()), "{names:?}");
        assert!(!names.contains(&"m0900".to_owned()) && !names.contains(&"m1100".to_owned()), "{names:?}");
        assert_eq!(place.roster_top(&member("m1000")), Some(0.0), "the row at the offset is at the viewport's top");
        let offline = ROSTER_TOP + 2.0 * HEADING_PITCH + ONLINE as f32 * ROW_PITCH;
        tell(&mut window, &clock, owls, scrolled(offline - 200.0));
        let roster = window.open().unwrap().roster();
        assert!(
            roster.lines.contains(&RosterLine::Heading(RosterGroup::Offline, MEMBERS - ONLINE)),
            "{:?}",
            roster.lines
        );
        assert!(!roster.lines.iter().any(|line| matches!(line, RosterLine::Heading(RosterGroup::Online, _))));
    }
}
