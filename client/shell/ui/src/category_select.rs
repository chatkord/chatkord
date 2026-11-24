//! A channel's category, picked from a field that opens a menu of them in their order.

use crate::labels::{label, phrase};
use crate::{MenuChoice, Renderer, Select, Theme};
use iced_core::Element;
use screens::Category;
use terms::CategoryId;
use words::{Phrase, Words};

/// The field named Category over `categories`, showing `chosen` and its menu while `open`; `toggle` opens or closes
/// it and `pick` makes a pick a message.
pub(crate) fn category_select<'a, Message: Clone + 'a, R: Renderer + 'a>(
    categories: &[Category],
    chosen: &CategoryId,
    open: bool,
    toggle: Message,
    pick: impl Fn(CategoryId) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let choices: Vec<MenuChoice<Message>> = categories
        .iter()
        .map(|category| MenuChoice {
            leading: None,
            label: label(&category.name),
            note: None,
            chosen: *chosen == category.id,
            message: pick(category.id.clone()),
        })
        .collect();
    let shown = choices.iter().find(|choice| choice.chosen).map_or_else(|| label(""), |choice| choice.label.clone());
    let field = Select {
        leading: None,
        name: phrase(Phrase::CategoryLabel, words),
        shown,
        choices,
        open,
        toggle: toggle.clone(),
        close: toggle,
    }
    .view::<R>();
    crate::door::named(Phrase::CategoryLabel, None, field, words)
}
