//! How each component looks in each state, as roles. A front end resolves a role through the palette and never
//! picks a colour, so the contrast contract covers every state of every component drawn from here.

use crate::listed;
use crate::role::{AccentRole, Border, Content, DestructiveRole, Role, Status, StatusRole, Surface};
use crate::scale::{BORDER_WIDTH, RING_WIDTH, Radius};

listed! {
    /// What a button is for.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum ButtonVariant {
        /// The one main action of a view, in the server's accent.
        Primary,
        /// An ordinary action, with an edge.
        Secondary,
        /// A destructive action: solid danger.
        Danger,
        /// An action with no weight of its own: a toolbar's.
        Ghost,
        /// A lesser action beside a stronger one, in secondary text until a pointer is on it: a picture's Remove.
        Quiet,
        /// An action inside a line of text, underlined in the accent's text colour.
        Link,
        /// A glyph on the scrim over a picture: a card's bar of controls, light in both schemes.
        OnScrim,
        /// An action with an edge on a picture's black ground, light in both schemes: a card's Stop sharing.
        OnPicture,
    }
}

listed! {
    /// What a pointer, the keyboard's focus or the work in hand is doing to a control: a control the focus is on
    /// is drawn as under the pointer.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Interaction {
        /// Neither under the pointer nor pressed.
        Idle,
        /// Under the pointer, or under the keyboard's focus.
        Hover,
        /// Pressed, by a pointer or a key.
        Pressed,
        /// Cannot be used now.
        Disabled,
        /// The action is under way: it looks as it did, takes no input, and says so in its label.
        Loading,
    }
}

/// The roles a button is painted with. `None` is nothing painted there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonLook {
    /// The ground behind the label, if it has one.
    pub fill: Option<Role>,
    /// The label and its icon.
    pub text: Role,
    /// The edge round it, if it has one.
    pub border: Option<Role>,
}

/// Every button's corners: a pill.
pub const BUTTON_RADIUS: Radius = Radius::Full;

/// The corners of a card or a panel on the window.
pub const CARD_RADIUS: Radius = Radius::Xl;
/// The corners of a person's tile in a call.
pub const TILE_RADIUS: Radius = Radius::Xl;
/// A menu's corners.
pub const MENU_RADIUS: Radius = Radius::L;
/// A dialog's corners.
pub const DIALOG_RADIUS: Radius = Radius::Xxl;

const fn surface(surface: Surface) -> Option<Role> {
    Some(Role::Surface(surface))
}

/// A destructive button: solid danger, as the design draws removing, leaving and stopping.
const fn danger(fill: DestructiveRole) -> ButtonLook {
    ButtonLook { fill: Some(Role::Destructive(fill)), text: Role::Destructive(DestructiveRole::OnFill), border: None }
}

/// The look of a button variant in an interaction. A hovered and a pressed control differ from the
/// idle one and from each other, so the change is seen without relying on a colour's hue.
pub const fn button(variant: ButtonVariant, interaction: Interaction) -> ButtonLook {
    let primary = Role::Content(Content::Primary);
    let disabled = Role::Content(Content::Disabled);
    match (variant, interaction) {
        (ButtonVariant::Primary, Interaction::Disabled)
        | (ButtonVariant::Secondary, Interaction::Disabled)
        | (ButtonVariant::Danger, Interaction::Disabled) => {
            ButtonLook { fill: Some(Role::DisabledFill), text: disabled, border: None }
        }
        (ButtonVariant::Ghost | ButtonVariant::Quiet | ButtonVariant::Link, Interaction::Disabled) => {
            ButtonLook { fill: None, text: disabled, border: None }
        }
        (ButtonVariant::Primary, Interaction::Idle | Interaction::Loading) => ButtonLook {
            fill: Some(Role::Accent(AccentRole::Fill)),
            text: Role::Accent(AccentRole::OnFill),
            border: None,
        },
        (ButtonVariant::Primary, Interaction::Hover) => ButtonLook {
            fill: Some(Role::Accent(AccentRole::FillHover)),
            text: Role::Accent(AccentRole::OnFill),
            border: None,
        },
        (ButtonVariant::Primary, Interaction::Pressed) => ButtonLook {
            fill: Some(Role::Accent(AccentRole::FillPressed)),
            text: Role::Accent(AccentRole::OnFill),
            border: None,
        },
        (ButtonVariant::Secondary, Interaction::Idle | Interaction::Loading) => {
            ButtonLook { fill: None, text: primary, border: Some(Role::Border(Border::Strong)) }
        }
        (ButtonVariant::Secondary, Interaction::Hover) => {
            ButtonLook { fill: surface(Surface::Hover), text: primary, border: Some(Role::Border(Border::Strong)) }
        }
        (ButtonVariant::Secondary, Interaction::Pressed) => {
            ButtonLook { fill: surface(Surface::Pressed), text: primary, border: Some(Role::Border(Border::Strong)) }
        }
        (ButtonVariant::Danger, Interaction::Idle | Interaction::Loading) => danger(DestructiveRole::Fill),
        (ButtonVariant::Danger, Interaction::Hover) => danger(DestructiveRole::FillHover),
        (ButtonVariant::Danger, Interaction::Pressed) => danger(DestructiveRole::FillPressed),
        (ButtonVariant::Ghost, Interaction::Idle | Interaction::Loading) => {
            ButtonLook { fill: None, text: primary, border: None }
        }
        (ButtonVariant::Quiet, Interaction::Idle | Interaction::Loading) => {
            ButtonLook { fill: None, text: Role::Content(Content::Secondary), border: None }
        }
        (ButtonVariant::Ghost | ButtonVariant::Quiet, Interaction::Hover) => {
            ButtonLook { fill: surface(Surface::Hover), text: primary, border: None }
        }
        (ButtonVariant::Ghost | ButtonVariant::Quiet, Interaction::Pressed) => {
            ButtonLook { fill: surface(Surface::Pressed), text: primary, border: None }
        }
        (ButtonVariant::OnScrim, Interaction::Disabled) => {
            ButtonLook { fill: None, text: Role::OnScrimDisabled, border: None }
        }
        (ButtonVariant::OnScrim, Interaction::Idle | Interaction::Loading) => {
            ButtonLook { fill: None, text: Role::OnScrim, border: None }
        }
        (ButtonVariant::OnScrim, Interaction::Hover) => {
            ButtonLook { fill: Some(Role::ScrimHover), text: Role::OnScrim, border: None }
        }
        (ButtonVariant::OnScrim, Interaction::Pressed) => {
            ButtonLook { fill: Some(Role::ScrimPressed), text: Role::OnScrim, border: None }
        }
        (ButtonVariant::OnPicture, Interaction::Disabled) => {
            ButtonLook { fill: None, text: Role::OnScrimDisabled, border: Some(Role::PictureEdge) }
        }
        (ButtonVariant::OnPicture, Interaction::Idle | Interaction::Loading) => {
            ButtonLook { fill: None, text: Role::OnScrim, border: Some(Role::PictureEdge) }
        }
        (ButtonVariant::OnPicture, Interaction::Hover) => {
            ButtonLook { fill: Some(Role::ScrimHover), text: Role::OnScrim, border: Some(Role::PictureEdge) }
        }
        (ButtonVariant::OnPicture, Interaction::Pressed) => {
            ButtonLook { fill: Some(Role::ScrimPressed), text: Role::OnScrim, border: Some(Role::PictureEdge) }
        }
        (ButtonVariant::Link, Interaction::Idle | Interaction::Loading) => {
            ButtonLook { fill: None, text: Role::Accent(AccentRole::Text), border: None }
        }
        // A link is underlined and fills nothing; the pointer and the keyboard's focus answer in its colour.
        (ButtonVariant::Link, Interaction::Hover) => ButtonLook { fill: None, text: primary, border: None },
        (ButtonVariant::Link, Interaction::Pressed) => {
            ButtonLook { fill: None, text: Role::Content(Content::Secondary), border: None }
        }
    }
}

listed! {
    /// What a text field is doing.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum InputState {
        /// Neither under the pointer nor focused.
        Idle,
        /// Under the pointer.
        Hover,
        /// Where the keyboard's typing goes.
        Focused,
        /// Cannot be typed in now.
        Disabled,
    }
}

/// The roles a text field is painted with, and how thick its edge is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputLook {
    /// The ground the text is typed on.
    pub fill: Role,
    /// What is typed.
    pub text: Role,
    /// The hint shown while it is empty.
    pub placeholder: Role,
    /// The edge round it.
    pub border: Role,
    /// How thick that edge is, in logical pixels.
    pub border_width: f32,
}

/// Every text field's corners.
pub const INPUT_RADIUS: Radius = Radius::M;

/// A search field's corners: a pill, as peer apps draw a search apart from a form's fields.
pub const SEARCH_RADIUS: Radius = Radius::Full;

/// The roles the text an input method is composing is drawn with, over the focused field it goes into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreeditLook {
    /// The ground behind the composed text.
    pub fill: Role,
    /// The composed text.
    pub text: Role,
    /// How thick its underline is, in logical pixels.
    pub underline_width: f32,
}

/// The focused field's own fill and text, underlined as every system marks composing text; the clause being
/// converted is drawn in the selection colour.
pub const fn preedit() -> PreeditLook {
    let field = input(InputState::Focused, false);
    PreeditLook { fill: field.fill, text: field.text, underline_width: RING_WIDTH }
}

/// The look of a text field. Focus is a thicker edge in the ring's colour and a fault an edge in danger's, beside
/// a sentence, so neither is told by hue alone. A disabled field is only disabled, valid or not.
pub const fn input(state: InputState, invalid: bool) -> InputLook {
    let inset = Role::Surface(Surface::Inset);
    let danger = Role::Status(Status::Danger, StatusRole::Text);
    match (state, invalid) {
        (InputState::Disabled, _) => InputLook {
            fill: Role::Surface(Surface::Base),
            text: Role::Content(Content::Disabled),
            placeholder: Role::Content(Content::Disabled),
            border: Role::Border(Border::Subtle),
            border_width: BORDER_WIDTH,
        },
        (InputState::Idle, false) => InputLook {
            fill: inset,
            text: Role::Content(Content::Primary),
            placeholder: Role::Content(Content::Secondary),
            border: Role::Border(Border::Strong),
            border_width: BORDER_WIDTH,
        },
        (InputState::Hover, false) => InputLook {
            fill: inset,
            text: Role::Content(Content::Primary),
            placeholder: Role::Content(Content::Secondary),
            border: Role::Content(Content::Secondary),
            border_width: BORDER_WIDTH,
        },
        (InputState::Focused, false) => InputLook {
            fill: inset,
            text: Role::Content(Content::Primary),
            placeholder: Role::Content(Content::Secondary),
            border: Role::Accent(AccentRole::Ring),
            border_width: RING_WIDTH,
        },
        (InputState::Idle | InputState::Hover, true) => InputLook {
            fill: inset,
            text: Role::Content(Content::Primary),
            placeholder: Role::Content(Content::Secondary),
            border: danger,
            border_width: BORDER_WIDTH,
        },
        (InputState::Focused, true) => InputLook {
            fill: inset,
            text: Role::Content(Content::Primary),
            placeholder: Role::Content(Content::Secondary),
            border: danger,
            border_width: RING_WIDTH,
        },
    }
}

/// The roles a list row is painted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowLook {
    /// The ground behind the row, if it has one.
    pub fill: Option<Role>,
    /// Its words and icons.
    pub text: Role,
    /// The row is set in the strong weight: a selected row is told by weight as well as by its fill.
    pub strong: bool,
}

/// Every row's corners.
pub const ROW_RADIUS: Radius = Radius::M;

listed! {
    /// A pill among pills: one choice of a few, or one page of a few.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum PillKind {
        /// On a track: the chosen one raised in the inset surface with the accent's text.
        Choice,
        /// In a page list: the current one on the accent's tint.
        Page,
        /// On a track of what to do to something, the choice that takes it away: chosen, on the danger tint in the
        /// danger text.
        Denied,
        /// On that track, the choice that leaves it as it is: chosen, raised in the inset surface in ordinary text.
        Kept,
        /// On that track, the choice that gives it: chosen, on the success tint in the success text.
        Allowed,
    }
}

/// The look of a pill, chosen (or current) or not, in an interaction. One not chosen is quiet until the pointer is
/// on it; a chosen one is told by weight as well as by its fill.
pub const fn pill(kind: PillKind, chosen: bool, interaction: Interaction) -> RowLook {
    let quiet = Role::Content(Content::Secondary);
    let primary = Role::Content(Content::Primary);
    match (kind, chosen, interaction) {
        (_, _, Interaction::Disabled) => RowLook { fill: None, text: Role::Content(Content::Disabled), strong: chosen },
        (PillKind::Choice, true, Interaction::Idle | Interaction::Loading) => {
            RowLook { fill: surface(Surface::Inset), text: Role::Accent(AccentRole::Text), strong: true }
        }
        (PillKind::Page, true, Interaction::Idle | Interaction::Loading) => {
            RowLook { fill: Some(Role::Accent(AccentRole::Tint)), text: primary, strong: true }
        }
        (PillKind::Denied, true, Interaction::Idle | Interaction::Loading) => RowLook {
            fill: Some(Role::Status(Status::Danger, StatusRole::Tint)),
            text: Role::Status(Status::Danger, StatusRole::Text),
            strong: true,
        },
        (PillKind::Kept, true, Interaction::Idle | Interaction::Loading) => {
            RowLook { fill: surface(Surface::Inset), text: primary, strong: true }
        }
        (PillKind::Allowed, true, Interaction::Idle | Interaction::Loading) => RowLook {
            fill: Some(Role::Status(Status::Success, StatusRole::Tint)),
            text: Role::Status(Status::Success, StatusRole::Text),
            strong: true,
        },
        // Under the pointer or the keyboard's focus a chosen one answers too; a choice keeps its accent text.
        (PillKind::Choice, true, Interaction::Hover) => {
            RowLook { fill: surface(Surface::Hover), text: Role::Accent(AccentRole::Text), strong: true }
        }
        (PillKind::Page | PillKind::Kept, true, Interaction::Hover) => {
            RowLook { fill: surface(Surface::Hover), text: primary, strong: true }
        }
        (PillKind::Denied, true, Interaction::Hover) => RowLook {
            fill: surface(Surface::Hover),
            text: Role::Status(Status::Danger, StatusRole::Text),
            strong: true,
        },
        (PillKind::Allowed, true, Interaction::Hover) => RowLook {
            fill: surface(Surface::Hover),
            text: Role::Status(Status::Success, StatusRole::Text),
            strong: true,
        },
        (_, true, Interaction::Pressed) => RowLook { fill: surface(Surface::Pressed), text: primary, strong: true },
        (_, false, Interaction::Idle | Interaction::Loading) => RowLook { fill: None, text: quiet, strong: false },
        (_, false, Interaction::Hover) => RowLook { fill: surface(Surface::Hover), text: primary, strong: false },
        (_, false, Interaction::Pressed) => RowLook { fill: surface(Surface::Pressed), text: primary, strong: false },
    }
}

/// The roles a switch is painted with: the track it runs along and the knob on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchLook {
    /// The track the knob runs along.
    pub track: Role,
    /// The knob.
    pub knob: Role,
    /// How much of both shows, out of 255: all of it, or [`DISABLED_ALPHA`] while it cannot be taken.
    pub alpha: u8,
}

/// The share of a control that shows while it cannot be taken, out of 255: the canvas's 45 %, so a switch held on
/// still reads as on.
pub const DISABLED_ALPHA: u8 = 115;

/// A switch, on or off: on is the accent's fill with its own text colour for the knob, off the strong edge colour with
/// the window's surface for the knob, so the knob is found on the track in both and the two are told apart by colour as
/// well as by where the knob is.
pub const fn switch(on: bool, interaction: Interaction) -> SwitchLook {
    let hovered = matches!(interaction, Interaction::Hover | Interaction::Pressed);
    let alpha = if matches!(interaction, Interaction::Disabled) { DISABLED_ALPHA } else { u8::MAX };
    let (track, knob) = match (on, hovered) {
        (true, false) => (Role::Accent(AccentRole::Fill), Role::Accent(AccentRole::OnFill)),
        (true, true) => (Role::Accent(AccentRole::FillHover), Role::Accent(AccentRole::OnFill)),
        (false, false) => (Role::Border(Border::Strong), Role::Surface(Surface::Base)),
        // A step stronger under the pointer or the keyboard's focus.
        (false, true) => (Role::Content(Content::Secondary), Role::Surface(Surface::Base)),
    };
    SwitchLook { track, knob, alpha }
}

/// The roles a slider is painted with: its rail up to the handle, the rest of it, and the handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderLook {
    /// The rail from the start up to the handle: how much is set.
    pub filled: Role,
    /// The rail past the handle.
    pub rest: Role,
    /// The handle.
    pub handle: Role,
}

/// A slider: the accent up to its handle and on it, the strong edge colour past it, a step stronger under the pointer,
/// the keyboard's focus or a drag, as a switch is.
pub const fn slider(interaction: Interaction) -> SliderLook {
    let fill = match interaction {
        Interaction::Hover | Interaction::Pressed => AccentRole::FillHover,
        Interaction::Disabled => {
            let disabled = Role::Content(Content::Disabled);
            return SliderLook { filled: disabled, rest: Role::Border(Border::Strong), handle: disabled };
        }
        Interaction::Idle | Interaction::Loading => AccentRole::Fill,
    };
    SliderLook { filled: Role::Accent(fill), rest: Role::Border(Border::Strong), handle: Role::Accent(fill) }
}

/// The roles a card that is chosen by a press is painted with (a source in our own picker).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardLook {
    /// The card's ground.
    pub fill: Role,
    /// Its edge.
    pub edge: Role,
    /// How thick that edge is, in logical pixels.
    pub edge_width: f32,
}

/// A card chosen by a press: on the raised surface with the subtle edge, the pointer's fills and the strong edge under
/// it; selected, on the selected fill (a selected row's) in the accent's ring.
pub const fn source_card(selected: bool, interaction: Interaction) -> CardLook {
    let fill = match (selected, interaction) {
        (_, Interaction::Pressed) | (true, Interaction::Hover) => Surface::Pressed,
        (true, _) | (false, Interaction::Hover) => Surface::Hover,
        (false, Interaction::Idle | Interaction::Disabled | Interaction::Loading) => Surface::Raised,
    };
    let (edge, edge_width) = match (selected, interaction) {
        (true, _) => (Role::Accent(AccentRole::Ring), RING_WIDTH),
        (false, Interaction::Hover | Interaction::Pressed) => (Role::Border(Border::Strong), BORDER_WIDTH),
        (false, Interaction::Idle | Interaction::Disabled | Interaction::Loading) => {
            (Role::Border(Border::Subtle), BORDER_WIDTH)
        }
    };
    CardLook { fill: Role::Surface(fill), edge, edge_width }
}

/// The look of a row, selected or not, in an interaction.
/// A selected row is filled with the hover colour, so under the pointer it takes the pressed one.
pub const fn row(selected: bool, interaction: Interaction) -> RowLook {
    let primary = Role::Content(Content::Primary);
    match (selected, interaction) {
        (_, Interaction::Disabled) => RowLook { fill: None, text: Role::Content(Content::Disabled), strong: false },
        (false, Interaction::Idle | Interaction::Loading) => RowLook { fill: None, text: primary, strong: false },
        (true, Interaction::Idle | Interaction::Loading) => {
            RowLook { fill: surface(Surface::Hover), text: primary, strong: true }
        }
        (false, Interaction::Hover) => RowLook { fill: surface(Surface::Hover), text: primary, strong: false },
        (true, Interaction::Hover) | (_, Interaction::Pressed) => {
            RowLook { fill: surface(Surface::Pressed), text: primary, strong: selected }
        }
    }
}

/// A row whose action removes or leaves something (a menu's Leave): its name in the danger colour and the strong
/// weight, filled like any row under the pointer.
pub const fn danger_row(interaction: Interaction) -> RowLook {
    match interaction {
        Interaction::Disabled => row(false, interaction),
        Interaction::Idle | Interaction::Loading | Interaction::Hover | Interaction::Pressed => {
            RowLook { text: Role::Status(Status::Danger, StatusRole::Text), strong: true, ..row(false, interaction) }
        }
    }
}

/// The choice of a menu that is taken: on the accent's tint, its weight the others', under the pointer as any row.
pub const fn chosen_row(interaction: Interaction) -> RowLook {
    match interaction {
        Interaction::Idle | Interaction::Loading => {
            RowLook { fill: Some(Role::Accent(AccentRole::Tint)), text: Role::Content(Content::Primary), strong: false }
        }
        Interaction::Hover | Interaction::Pressed | Interaction::Disabled => row(false, interaction),
    }
}

/// A row of the side pane: not selected, its name is quiet until the pointer is on it.
pub const fn quiet_row(selected: bool, interaction: Interaction) -> RowLook {
    match (selected, interaction) {
        (false, Interaction::Idle | Interaction::Loading) => {
            RowLook { fill: None, text: Role::Content(Content::Secondary), strong: false }
        }
        _ => row(selected, interaction),
    }
}

listed! {
    /// What a person in a call, or the call itself, is doing: each is a word on its badge, so none is told by colour alone.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Presence {
        /// In the call, not speaking.
        Online,
        /// Speaking now.
        Speaking,
        /// Their microphone is off.
        Muted,
        /// They hear nothing, and their microphone is off.
        Deafened,
        /// An administrator muted them for everyone.
        MutedByAdmin,
        /// The call is being joined.
        Connecting,
        /// The call could not be joined, or was lost.
        Failed,
        /// Sharing a screen.
        Live,
        /// The call was lost and is being joined again.
        Reconnecting,
    }
}

/// The roles a badge is painted with: a status colour behind its word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadgeLook {
    /// The badge's ground.
    pub fill: Role,
    /// Its word.
    pub text: Role,
}

/// The look of a badge saying `presence`, in the colour of its kind of news.
pub const fn badge(presence: Presence) -> BadgeLook {
    let status = match presence {
        Presence::Online | Presence::Speaking => Status::Success,
        Presence::Connecting | Presence::Muted => Status::Info,
        Presence::Reconnecting | Presence::Deafened => Status::Warning,
        Presence::Failed | Presence::Live | Presence::MutedByAdmin => Status::Danger,
    };
    BadgeLook { fill: Role::Status(status, StatusRole::Fill), text: Role::Status(status, StatusRole::OnFill) }
}

listed! {
    /// A person's status as the dot beside their picture marks it: each by its shape as well as its colour.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum StatusMark {
        /// Signed in and about: a whole dot.
        Online,
        /// Away: a crescent, its top left cut.
        Away,
        /// Not to be disturbed: a bar across it.
        DoNotDisturb,
        /// Signed in on no device: a whole dot of the strong edge.
        Offline,
        /// The account's own invisible choice: a ring with nothing in it.
        Hollow,
    }
}

/// What is cut from a status dot, in the colour of the ground it stands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusCut {
    /// Nothing.
    Whole,
    /// A disc at its top left, [`crate::AWAY_NOTCH_SHARE`] of the dot wide.
    Notch,
    /// A bar across its middle, [`crate::DO_NOT_DISTURB_BAR_SHARE`] of the dot.
    Bar,
    /// All inside its ring, [`RING_WIDTH`] wide.
    Hollow,
}

/// The roles a status dot is painted with: its colour and what is cut from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusLook {
    /// The dot, or its ring where it is hollow.
    pub fill: Role,
    /// What is cut from it.
    pub cut: StatusCut,
}

/// The look of the dot that marks `mark`.
pub const fn status_mark(mark: StatusMark) -> StatusLook {
    let (status, cut) = match mark {
        StatusMark::Online => (Some(Status::Success), StatusCut::Whole),
        StatusMark::Away => (Some(Status::Warning), StatusCut::Notch),
        StatusMark::DoNotDisturb => (Some(Status::Danger), StatusCut::Bar),
        StatusMark::Offline => (None, StatusCut::Whole),
        StatusMark::Hollow => (None, StatusCut::Hollow),
    };
    let fill = match status {
        Some(status) => Role::Status(status, StatusRole::Fill),
        None => Role::Border(Border::Strong),
    };
    StatusLook { fill, cut }
}

/// The roles a picture of someone with no colour known is painted with: an app, a former member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvatarLook {
    /// The disc.
    pub fill: Role,
    /// The initials on it.
    pub text: Role,
}

/// The look of a picture with no colour known. Its disc is told apart from every surface it can sit on: a card
/// on the inset surface, the chrome, a row under the pointer (the hover fill does not show on the light chrome).
pub const fn plain_avatar() -> AvatarLook {
    AvatarLook { fill: Role::Surface(Surface::Pressed), text: Role::Content(Content::Primary) }
}

/// What a round control in a call stands for. The icon says how things are now, the colour what kind of control it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallControl {
    /// A control as it is: a microphone that is on.
    Rest,
    /// A control whose last use failed: as at rest, with a badge in the danger colour ([`fault_badge`]) until it is
    /// pressed or works again.
    Failed,
    /// Muted or deafened: a red tint.
    Off,
    /// Sharing: a coral tint.
    Live,
    /// Leave in the call bar: solid red.
    Leave,
    /// Leave in the side pane: a red tint, as it is quieter there.
    LeaveQuiet,
    /// The connection's bars on the call panel: no fill of their own, the icon in the status of the verdict (quiet
    /// before there is one), and a fill while the popover they open is open.
    Signal {
        /// The verdict on the connection; none before there is one.
        status: Option<Status>,
        /// Whether the popover the bars open is open.
        open: bool,
    },
}

impl CallControl {
    /// Every control, each signal with each verdict it can show (none, success, warning, danger), closed and open.
    pub const ALL: [CallControl; 14] = [
        CallControl::Rest,
        CallControl::Failed,
        CallControl::Off,
        CallControl::Live,
        CallControl::Leave,
        CallControl::LeaveQuiet,
        CallControl::Signal { status: None, open: false },
        CallControl::Signal { status: Some(Status::Success), open: false },
        CallControl::Signal { status: Some(Status::Warning), open: false },
        CallControl::Signal { status: Some(Status::Danger), open: false },
        CallControl::Signal { status: None, open: true },
        CallControl::Signal { status: Some(Status::Success), open: true },
        CallControl::Signal { status: Some(Status::Warning), open: true },
        CallControl::Signal { status: Some(Status::Danger), open: true },
    ];
}

/// The roles a round call control is painted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallLook {
    /// The control's ground.
    pub fill: Role,
    /// Its icon.
    pub icon: Role,
}

/// The look of a call control in an interaction: under the pointer or pressed, the neutral surface above its own and
/// the same icon colour, which the contract keeps readable on both; the solid Leave deepens instead.
pub const fn call_button(control: CallControl, interaction: Interaction) -> CallLook {
    if matches!(interaction, Interaction::Disabled | Interaction::Loading) {
        return CallLook { fill: Role::Surface(Surface::Hover), icon: Role::Content(Content::Disabled) };
    }
    let touched = matches!(interaction, Interaction::Hover | Interaction::Pressed);
    match (control, touched) {
        (CallControl::Rest | CallControl::Failed, false) => {
            CallLook { fill: Role::Surface(Surface::Hover), icon: Role::Content(Content::Primary) }
        }
        (CallControl::Rest | CallControl::Failed, true) => {
            CallLook { fill: Role::Surface(Surface::Pressed), icon: Role::Content(Content::Primary) }
        }
        (CallControl::Off | CallControl::LeaveQuiet, false) => CallLook {
            fill: Role::Status(Status::Danger, StatusRole::Tint),
            icon: Role::Status(Status::Danger, StatusRole::Text),
        },
        (CallControl::Off | CallControl::LeaveQuiet, true) => {
            CallLook { fill: Role::Surface(Surface::Pressed), icon: Role::Status(Status::Danger, StatusRole::Text) }
        }
        (CallControl::Live, false) => {
            CallLook { fill: Role::Accent(AccentRole::Tint), icon: Role::Accent(AccentRole::Text) }
        }
        (CallControl::Live, true) => {
            CallLook { fill: Role::Surface(Surface::Pressed), icon: Role::Accent(AccentRole::Text) }
        }
        (CallControl::Leave, false) => CallLook {
            fill: Role::Destructive(DestructiveRole::Fill),
            icon: Role::Destructive(DestructiveRole::OnFill),
        },
        (CallControl::Leave, true) => CallLook {
            fill: Role::Destructive(DestructiveRole::FillHover),
            icon: Role::Destructive(DestructiveRole::OnFill),
        },
        (CallControl::Signal { status, open }, touched) => CallLook {
            fill: Role::Surface(match (open, touched) {
                (false, false) => Surface::Base,
                (false, true) | (true, false) => Surface::Hover,
                (true, true) => Surface::Pressed,
            }),
            icon: match status {
                Some(status) => Role::Status(status, StatusRole::Text),
                None => Role::Content(Content::Secondary),
            },
        },
    }
}

/// The badge on a control that failed: the danger colour behind its mark.
pub const fn fault_badge() -> CallLook {
    CallLook {
        fill: Role::Status(Status::Danger, StatusRole::Fill),
        icon: Role::Status(Status::Danger, StatusRole::OnFill),
    }
}

/// The edge round a notice of a kind.
pub const fn notice_edge(status: Status) -> Role {
    Role::Status(status, StatusRole::Fill)
}

/// Every rail item's corners, and a server's picture's: a rounded square, where a person is a circle.
pub const RAIL_RADIUS: Radius = Radius::L;
