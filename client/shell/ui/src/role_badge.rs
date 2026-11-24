//! A role someone holds, as a badge: its colour's dot and its name on a bordered pill, the name in the neutral text
//! colour whatever the role's colour, so every role reads alike.

use crate::list_row::colour_dot;
use crate::typography::line;
use crate::{Labelled, Renderer, Surfaced, Theme};
use design::{BORDER_WIDTH, Radius, Rgb, SemanticRole, Semantics, Space, Surface, TypeStyle};
use iced_core::{Alignment, Element, Padding, Widget};
use iced_widget::{Row, container};

/// The badge of the role named `name`, with its `colour`'s dot; none for a role with no colour, whose badge has no
/// dot. Its name is cut with an ellipsis where the badge has no room for it.
pub fn role_badge<'a, Message: 'a, R: Renderer + 'a>(
    name: &str,
    colour: Option<Rgb>,
) -> Element<'a, Message, Theme, R> {
    let mut cells: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    cells.extend(colour.map(|colour| colour_dot(Some(colour))));
    cells.push(line(name.to_string(), TypeStyle::Caption)._boxed());
    let row = Row::with_children(cells).spacing(Space::Xs.px() + Space::Xxs.px()).align_y(Alignment::Center);
    // The border is inside the pill, round its padding.
    let pill = container(row)
        .padding(Padding::from([Space::Xxs.px() + BORDER_WIDTH, Space::S.px() + Space::Xxs.px() + BORDER_WIDTH]))
        .class(Surfaced::Framed(Surface::Overlay, Radius::Full));
    let label = crate::labels::label(name);
    Labelled::new(pill, Semantics { role: SemanticRole::ListItem, label })._boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Renderer;
    use iced_core::{Length, Size};
    use iced_widget::Column;

    #[test]
    fn badges_wrap_onto_more_lines_where_one_line_holds_them_not() {
        crate::testing::hermetic_fonts();
        let names = ["Moderators", "Game hosts", "Helpers"];
        let badges = names.iter().map(|name| role_badge::<(), Renderer>(name, None));
        let wrapped = Row::with_children(badges).spacing(Space::S.px()).wrap();
        let page = Column::with_children([container(wrapped).width(Length::Fixed(220.0))._boxed()]);
        let mut sim = iced_test::Simulator::with_size(iced_core::Settings::default(), Size::new(300.0, 200.0), page);
        let tree = crate::access::tree(Size::new(300.0, 200.0), 1.0, None, |operation| sim.operate(operation));
        let top = |name: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| node.label() == Some(name))
                .and_then(|(_, node)| node.bounds())
                .map(|bounds| bounds.y0)
                .unwrap_or_else(|| panic!("no {name}"))
        };
        assert_eq!(top("Moderators"), top("Game hosts"), "two fit on the first line");
        assert!(top("Helpers") > top("Moderators"), "the third goes on the next");
    }
}
