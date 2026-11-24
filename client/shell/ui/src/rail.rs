//! An item of the server rail: the server's picture with its marks (the open one, connecting, needing a look, this
//! window's call), each also said in its label, the "+" that adds a server at the rail's end, and the gear at its foot
//! that opens the settings.

use crate::button::{ButtonClass, actionable};
use crate::rail_mark::RailMark;
use crate::{AvatarKind, Icon, IconClass, Renderer, Surfaced, Theme, avatar, icon, tooltip};
use design::{
    AvatarSize, Border, Content, IconSize, Interaction, Label, RAIL_CALL_MARK, RAIL_DOT, RAIL_WIDTH, RING_WIDTH,
    Radius, Rgb, Role, SemanticRole, Semantics, Status, StatusRole, Surface,
};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::svg::{Handle, Svg};
use iced_widget::tooltip::Position;
use iced_widget::{Space, button, container, mouse_area, stack};
use screens::{Badge, BadgeShown, RailState};
use std::sync::LazyLock;
use words::{Phrase, Words};

/// The ring's reach past the picture: a gap of the chrome, then the ring.
const RING_REACH: f32 = 2.0 * RING_WIDTH;
/// Where the bar sits below the top of the picture.
const BAR_TOP: f32 = design::Space::S.px();

/// Whether a server in `state` is drawn faint: not signed in.
pub(crate) fn faint(state: RailState) -> bool {
    matches!(state, RailState::SignedOut | RailState::NewPassword)
}

/// A server on the rail, pressed to open it and right-pressed for its menu.
pub struct RailItem<'a, Message> {
    name: Label,
    colour: Rgb,
    open: bool,
    state: RailState,
    in_call: bool,
    badge: Badge,
    on_press: Option<Message>,
    on_menu: Option<Message>,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a, Message: Clone + 'a> RailItem<'a, Message> {
    /// An item for the server named `name`, in its `colour`, online and not open. It is not a control until it has
    /// [`RailItem::on_press`].
    pub fn new(name: Label, colour: Rgb) -> Self {
        RailItem {
            name,
            colour,
            open: false,
            state: RailState::Online,
            in_call: false,
            badge: Badge::default(),
            on_press: None,
            on_menu: None,
            _lifetime: std::marker::PhantomData,
        }
    }

    /// The server shown now: the bar at the rail's edge beside it.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// How its server's connection stands, which its picture and ring show; online unless said.
    pub fn state(mut self, state: RailState) -> Self {
        self.state = state;
        self
    }

    /// Whether this window's call holds a voice place on this server.
    pub fn in_call(mut self, in_call: bool) -> Self {
        self.in_call = in_call;
        self
    }

    /// What is unread on its server: a count or a dot at the picture's corner, dimmed while the server is not
    /// connected and the count is its last.
    pub fn badge(mut self, badge: Badge) -> Self {
        self.badge = badge;
        self
    }

    /// What a press sends: open its server.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// What a right press on the item sends: its server's menu.
    pub fn on_menu(mut self, message: Message) -> Self {
        self.on_menu = Some(message);
        self
    }

    /// How tall an item is: the picture and the ring's reach above and below it.
    pub const HEIGHT: f32 = AvatarSize::Rail.px() + 2.0 * RING_REACH;
    /// How far below the item's top its picture starts.
    pub const PICTURE_TOP: f32 = RING_REACH;

    /// The item with the server's letter, in `words` for what a screen reader hears.
    pub fn view<R: Renderer + 'a>(self, words: &Words) -> Element<'a, Message, Theme, R> {
        self.view_painted(None, words)
    }

    /// The same, with the server's own picture in place of its letter when the host holds one.
    pub fn view_painted<R: Renderer + 'a>(
        self,
        painted: Option<Element<'a, Message, Theme, R>>,
        words: &Words,
    ) -> Element<'a, Message, Theme, R> {
        let faint = faint(self.state);
        let picture = if let Some(painted) = painted {
            painted
        } else if faint {
            crate::avatar::faint_server::<Message, R>(self.name.clone(), self.colour)
        } else {
            avatar::<Message, R>(self.name.clone(), AvatarKind::Server(self.colour), AvatarSize::Rail)
        };
        // The ring is round the picture; the button is the whole item, so its bar shows from the pointer it is given.
        let ringed = container(picture).padding(RING_REACH).class(if self.state == RailState::Connecting {
            Surfaced::ServerRing(Radius::Xl)
        } else {
            Surfaced::Clear
        });
        let inner = button::Button::new(marked(ringed._boxed(), self.open))
            .class(ButtonClass::Rail)
            .padding(Padding::ZERO)
            .on_press_maybe(self.on_press.clone());
        let said = match self.state {
            RailState::NotAnswering => Some(Phrase::RailNotAnswering),
            RailState::SignedOut => Some(Phrase::RailSignedOut),
            RailState::Connecting => Some(Phrase::RailConnecting),
            RailState::KeyChanged => Some(Phrase::RailIdentityChanged),
            RailState::Online | RailState::NewPassword => self.in_call.then_some(Phrase::RailInCall),
        };
        let label = said
            .and_then(|said| Label::new(&format!("{}, {}", self.name.as_str(), words.phrase(said))).ok())
            .unwrap_or_else(|| self.name.clone());
        let semantics = Semantics { role: SemanticRole::ListItem, label };
        let pressable = tooltip(actionable(inner, semantics, self.on_press), &self.name, Position::Right);
        let mut layers: Vec<Element<'a, Message, Theme, R>> = vec![pressable];
        let dot = match self.state {
            RailState::NotAnswering | RailState::KeyChanged => Some(Role::Status(Status::Warning, StatusRole::Fill)),
            RailState::SignedOut | RailState::NewPassword => Some(Role::Content(Content::Secondary)),
            RailState::Online | RailState::Connecting => None,
        };
        if let Some(fill) = dot {
            layers.push(container(mark(fill)).padding(dot_at())._boxed());
        } else if self.badge.shown != BadgeShown::Nothing {
            layers.push(badge_mark(self.badge));
        } else if self.in_call {
            layers.push(container(call_mark()).padding(call_mark_at())._boxed());
        }
        let item = container(stack(layers)).width(Length::Fixed(RAIL_WIDTH)).height(Length::Fixed(Self::HEIGHT));
        match self.on_menu {
            Some(menu) => mouse_area(item).on_right_press(menu)._boxed(),
            None => item._boxed(),
        }
    }
}

/// The "+"'s frame: a dashed rounded square at the rail picture's size (iced draws no dashed border).
static DASHED_TILE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(include_bytes!("../assets/shapes/dashed-tile.svg").as_slice()));

/// The rail's last item: a dashed square with a "+", which adds a server; `name` is what it is called. No server yet,
/// so no bar beside it.
pub fn add_tile<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    on_press: Message,
) -> Element<'a, Message, Theme, R> {
    let picture = AvatarSize::Rail.px();
    let frame = Svg::new(DASHED_TILE.clone())
        .width(Length::Fixed(picture))
        .height(Length::Fixed(picture))
        .class(IconClass::Role(Role::Border(Border::Strong)));
    let plus = container(icon(Icon::Plus, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed())
        .center(Length::Fixed(picture));
    face(stack([frame._boxed(), plus._boxed()])._boxed(), false, name, on_press)
}

/// The rail's foot: a gear, at the "+"'s size, that opens the settings; `name` is what it is called. Quiet as a pane's row
/// until the settings are `current`, then on a chosen row's fill; no server, so no bar beside it.
pub fn gear_tile<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    current: bool,
    on_press: Message,
) -> Element<'a, Message, Theme, R> {
    let look = design::quiet_row(current, Interaction::Idle);
    let glyph = icon(Icon::Settings, IconSize::M, IconClass::Role(look.text));
    face(container(glyph).center(Length::Fixed(AvatarSize::Rail.px()))._boxed(), current, name, on_press)
}

/// A rail item that is no server: `drawn` is its face, which fills under the pointer, centred in the item.
fn face<'a, Message: Clone + 'a, R: Renderer + 'a>(
    drawn: Element<'a, Message, Theme, R>,
    current: bool,
    name: &Label,
    on_press: Message,
) -> Element<'a, Message, Theme, R> {
    let pressable = button::Button::new(drawn)
        .class(ButtonClass::RailFace { current })
        .padding(Padding::ZERO)
        .on_press(on_press.clone());
    let semantics = Semantics { role: SemanticRole::Button, label: name.clone() };
    let face = container(actionable(pressable, semantics, Some(on_press))).padding(RING_REACH);
    tooltip(centred(face._boxed()), name, Position::Right)
}

/// `face` centred in an item of the rail.
fn centred<'a, Message: 'a, R: Renderer + 'a>(face: Element<'a, Message, Theme, R>) -> Element<'a, Message, Theme, R> {
    container(face)
        .center_x(Length::Fill)
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fixed(RailItem::<()>::HEIGHT))
        ._boxed()
}

/// `face` centred in an item of the rail, with the item's bar at its edge.
fn marked<'a, Message: 'a, R: Renderer + 'a>(
    face: Element<'a, Message, Theme, R>,
    open: bool,
) -> Element<'a, Message, Theme, R> {
    let face = container(face).center_x(Length::Fill)._boxed();
    let mark = RailMark { open, top: RING_REACH + BAR_TOP }._boxed();
    container(stack([face, mark]))
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fixed(RailItem::<()>::HEIGHT))
        ._boxed()
}

/// The dot, with a ring of the chrome round it that sets it off the picture.
fn mark<'a, Message: 'a, R: Renderer + 'a>(fill: Role) -> Element<'a, Message, Theme, R> {
    let dot = container(Space::new())
        .width(Length::Fixed(RAIL_DOT))
        .height(Length::Fixed(RAIL_DOT))
        .class(Surfaced::Fill { fill, text: fill, radius: Radius::Full });
    container(dot)
        .padding(RING_WIDTH)
        .class(Surfaced::Fill {
            fill: Role::Surface(Surface::Chrome),
            text: Role::Content(Content::Primary),
            radius: Radius::Full,
        })
        ._boxed()
}

/// Headphones on the inset surface, ringed with the chrome as the dot is.
fn call_mark<'a, Message: 'a, R: Renderer + 'a>() -> Element<'a, Message, Theme, R> {
    let glyph = icon(Icon::Headphones, IconSize::Mark, IconClass::Role(Role::Content(Content::Primary)));
    let disc = container(glyph).center(Length::Fixed(RAIL_CALL_MARK)).class(Surfaced::Fill {
        fill: Role::Surface(Surface::Inset),
        text: Role::Content(Content::Primary),
        radius: Radius::Full,
    });
    container(disc)
        .padding(RING_WIDTH)
        .class(Surfaced::Fill {
            fill: Role::Surface(Surface::Chrome),
            text: Role::Content(Content::Primary),
            radius: Radius::Full,
        })
        ._boxed()
}

/// The call mark overlaps the picture's lower right corner, 4 px past its right edge and 2 px past its foot.
fn call_mark_at() -> Padding {
    let picture = AvatarSize::Rail.px();
    let left = (RAIL_WIDTH - picture) / 2.0 + picture + design::Space::Xs.px() - RAIL_CALL_MARK - RING_WIDTH;
    let top = RING_REACH + picture + design::Space::Xxs.px() - RAIL_CALL_MARK - RING_WIDTH;
    Padding { top, left, ..Padding::ZERO }
}

/// The dot sits on the picture's lower right corner: flush with its right edge, its bottom 2 px above the picture's.
fn dot_at() -> Padding {
    let picture = AvatarSize::Rail.px();
    let left = (RAIL_WIDTH - picture) / 2.0 + picture - RAIL_DOT - RING_WIDTH;
    let top = RING_REACH + picture - RAIL_DOT - design::Space::Xxs.px() - RING_WIDTH;
    Padding { top, left, ..Padding::ZERO }
}

/// What is unread, ringed with the chrome as the dot is: a count on the accent, or the primary dot where what is unread
/// does not alert. A count that is the last held of a server not connected is on the inset surface, in the secondary
/// colour.
fn badge_mark<'a, Message: 'a, R: Renderer + 'a>(badge: Badge) -> Element<'a, Message, Theme, R> {
    let picture = AvatarSize::Rail.px();
    let ringed = |shown: Element<'a, Message, Theme, R>| {
        container(shown).padding(RING_WIDTH).class(Surfaced::Fill {
            fill: Role::Surface(Surface::Chrome),
            text: Role::Content(Content::Primary),
            radius: Radius::Full,
        })
    };
    let fill = if badge.stale { Role::Content(Content::Secondary) } else { Role::Content(Content::Primary) };
    let (mark, tall) = match badge.shown {
        BadgeShown::Count(count) => {
            (crate::count_badge::counted::<Message, R>(count, badge.stale), design::MENTION_BADGE)
        }
        BadgeShown::Dot | BadgeShown::Nothing => (mark_dot(fill), RAIL_DOT),
    };
    let top = RING_REACH + picture + design::Space::Xxs.px() - tall - RING_WIDTH;
    let right = (RAIL_WIDTH - picture) / 2.0 - design::Space::Xs.px() - RING_WIDTH;
    container(ringed(mark))
        .padding(Padding { top, right, ..Padding::ZERO })
        .width(Length::Fill)
        .align_right(Length::Fill)
        ._boxed()
}

/// The dot of `fill` alone.
fn mark_dot<'a, Message: 'a, R: Renderer + 'a>(fill: Role) -> Element<'a, Message, Theme, R> {
    container(Space::new())
        .width(Length::Fixed(RAIL_DOT))
        .height(Length::Fixed(RAIL_DOT))
        .class(Surfaced::Fill { fill, text: fill, radius: Radius::Full })
        ._boxed()
}
