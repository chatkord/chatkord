//! What a control is and what it is called, for a person who cannot see it: a screen reader reads
//! it, a tooltip shows it, and a control without one cannot be made.

use crate::Status;
use std::fmt;

/// The most characters a label holds. A label names a control; a sentence belongs elsewhere.
pub const MAX_LABEL_CHARS: usize = 120;

/// A control's name: not empty, not only spaces, at most `MAX_LABEL_CHARS` characters, refused whole
/// when it is longer, never cut.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Label(Box<str>);

/// Why a text cannot be a label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelError {
    /// It is empty, or only spaces.
    Empty,
    /// It is longer than a label may be.
    TooLong,
}

impl fmt::Display for LabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LabelError::Empty => f.write_str("a label is empty"),
            LabelError::TooLong => write!(f, "a label is longer than {MAX_LABEL_CHARS} characters"),
        }
    }
}

impl Label {
    /// `text` with the spaces at either end trimmed, if that is a label: not empty and within [`MAX_LABEL_CHARS`].
    /// The build of a catalog calls it on every phrase.
    pub fn check(text: &str) -> Result<&str, LabelError> {
        let text = text.trim();
        if text.is_empty() {
            Err(LabelError::Empty)
        } else if text.chars().count() > MAX_LABEL_CHARS {
            Err(LabelError::TooLong)
        } else {
            Ok(text)
        }
    }

    /// The label of `text` with the spaces at either end trimmed; refused if what is left is empty or too long.
    pub fn new(text: &str) -> Result<Label, LabelError> {
        Label::check(text).map(|text| Label(text.into()))
    }

    /// The label's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What kind of thing a control is, as an accessibility tree names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticRole {
    /// Does something when pressed.
    Button,
    /// Goes somewhere when pressed.
    Link,
    /// A field that takes text.
    TextInput,
    /// A text field whose value is never read out or exposed.
    SecretInput,
    /// A picture.
    Image,
    /// A line that tells what is happening, read out when it changes, once what is being read is done.
    Status,
    /// A failure, read out at once when it shows or changes, over whatever is being read.
    Alert,
    /// A window over the rest that holds the focus until it is closed.
    Dialog,
    /// A dialog that asks to confirm something that cannot be undone.
    AlertDialog,
    /// A list of actions that opens from a control.
    Menu,
    /// One action in a menu.
    MenuItem,
    /// One choice of a menu of which one is taken: a device in use, a language.
    MenuItemRadio {
        /// Whether it is the one taken.
        chosen: bool,
    },
    /// One choice of a menu of which any are taken, each ticked or not: a role a member holds.
    MenuItemCheckbox {
        /// Whether it is ticked.
        checked: bool,
    },
    /// Items read as one list.
    List,
    /// One item of a list.
    ListItem,
    /// A heading over what follows.
    Heading,
    /// A few choices of which one is taken: its options are [`SemanticRole::Radio`].
    RadioGroup,
    /// One option of a radio group.
    Radio {
        /// Whether it is the one taken.
        chosen: bool,
    },
    /// A control that is on or off, and says which.
    Switch {
        /// Whether it is on.
        on: bool,
    },
    /// A grid of rows under named columns.
    Table,
    /// A value picked along a range, by dragging or by the arrow keys.
    Slider {
        /// The value now.
        value: u16,
        /// The least it can be.
        least: u16,
        /// The most it can be.
        most: u16,
    },
    /// A button that opens a layer over the screen, and says whether it is open.
    Opens {
        /// What it opens.
        layer: Popup,
        /// Whether what it opens is showing.
        open: bool,
    },
}

/// What a control opens over the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Popup {
    /// A list of actions or choices.
    Menu,
    /// A panel of its own: a popover or a dialog.
    Dialog,
}

/// How a line that tells something is read out when it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Live {
    /// Once what is being read is done.
    Polite,
    /// At once, over what is being read.
    Assertive,
}

impl SemanticRole {
    /// The role of a line that tells something of `kind`: a failure is an [`SemanticRole::Alert`], read out at once;
    /// anything else a [`SemanticRole::Status`], which waits its turn.
    pub fn telling(kind: Status) -> SemanticRole {
        match kind {
            Status::Danger => SemanticRole::Alert,
            Status::Success | Status::Warning | Status::Info => SemanticRole::Status,
        }
    }

    /// How it is read out when it changes; none for what is read only when a person comes to it.
    pub fn live(self) -> Option<Live> {
        match self {
            SemanticRole::Status => Some(Live::Polite),
            SemanticRole::Alert => Some(Live::Assertive),
            SemanticRole::Button
            | SemanticRole::Link
            | SemanticRole::TextInput
            | SemanticRole::SecretInput
            | SemanticRole::Image
            | SemanticRole::Dialog
            | SemanticRole::AlertDialog
            | SemanticRole::Menu
            | SemanticRole::MenuItem
            | SemanticRole::MenuItemRadio { .. }
            | SemanticRole::MenuItemCheckbox { .. }
            | SemanticRole::List
            | SemanticRole::ListItem
            | SemanticRole::Heading
            | SemanticRole::RadioGroup
            | SemanticRole::Radio { .. }
            | SemanticRole::Switch { .. }
            | SemanticRole::Table
            | SemanticRole::Slider { .. }
            | SemanticRole::Opens { .. } => None,
        }
    }

    /// Whether a person operates it: one that cannot take the focus now is disabled. What only shows or holds others
    /// is never disabled.
    pub fn operated(self) -> bool {
        match self {
            SemanticRole::Button
            | SemanticRole::Link
            | SemanticRole::TextInput
            | SemanticRole::SecretInput
            | SemanticRole::MenuItem
            | SemanticRole::MenuItemRadio { .. }
            | SemanticRole::MenuItemCheckbox { .. }
            | SemanticRole::Radio { .. }
            | SemanticRole::Switch { .. }
            | SemanticRole::Slider { .. }
            | SemanticRole::Opens { .. } => true,
            SemanticRole::Image
            | SemanticRole::Status
            | SemanticRole::Alert
            | SemanticRole::Dialog
            | SemanticRole::AlertDialog
            | SemanticRole::Menu
            | SemanticRole::List
            | SemanticRole::ListItem
            | SemanticRole::Heading
            | SemanticRole::RadioGroup
            | SemanticRole::Table => false,
        }
    }
}

/// A control as an accessibility tree sees it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Semantics {
    /// What kind of thing it is.
    pub role: SemanticRole,
    /// What it is called.
    pub label: Label,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_is_trimmed_and_bounded_never_cut() {
        assert_eq!(Label::new("  Sign in ").map(|label| label.as_str().to_string()), Ok("Sign in".to_string()));
        assert_eq!(Label::new(" \t\n"), Err(LabelError::Empty));
        assert_eq!(Label::new(""), Err(LabelError::Empty));
        let greek = "Σύνδεση".repeat(17);
        assert_eq!(greek.chars().count(), 119);
        assert!(Label::new(&greek).is_ok(), "the bound counts characters, not bytes");
        assert_eq!(Label::new(&"α".repeat(121)), Err(LabelError::TooLong));
    }

    #[test]
    fn a_failure_interrupts_and_anything_else_told_waits_its_turn() {
        for kind in Status::ALL {
            let wanted = if kind == Status::Danger { Live::Assertive } else { Live::Polite };
            assert_eq!(SemanticRole::telling(kind).live(), Some(wanted), "{kind:?}");
        }
        assert_eq!(SemanticRole::Button.live(), None, "a control is read when a person comes to it");
    }
}
