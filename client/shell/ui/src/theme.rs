//! The theme: a `design` palette for one scheme and the window's accent, and how each widget the components draw
//! looks in it. A picked colour (a server's, a person's) is drawn as picked, with the letter that reads on it.

use crate::button::{ButtonClass, RowTone};
use crate::color::{color, translucent};
use design::{
    Accent, BORDER_WIDTH, BUTTON_RADIUS, Border as DesignBorder, Content, Contrast, Elevation, InputState, Interaction,
    MotionPreference, Palette, PersonColour, RING_WIDTH, ROW_RADIUS, Radius, Rgb, Role, SPEAKING_GLOW, Scheme, Status,
    StatusRole, Surface, TILE_RADIUS, readable_on,
};
use iced_core::theme::{Base, Mode, Style};
use iced_core::{Background, Border};
use iced_widget::{button, container, progress_bar, scrollable, text};

/// The palette of one scheme, accent and contrast, and the motion the system asks for.
#[derive(Clone, Debug)]
pub struct Theme {
    palette: Palette,
    /// Unique to the scheme, accent and contrast, because iced compares it to notice a theme change.
    name: String,
    motion: MotionPreference,
}

/// The end of a theme's name when the system asks for less motion, so iced sees the change.
const STILL: &str = "-still";

impl Theme {
    /// The theme of `scheme` and `accent` at standard contrast, with full motion.
    pub fn new(scheme: Scheme, accent: Accent) -> Theme {
        Theme::with_contrast(scheme, accent, Contrast::Standard)
    }

    /// The theme of `scheme` and `accent` at `contrast`, with full motion; [`Theme::with_motion`] sets that.
    pub fn with_contrast(scheme: Scheme, accent: Accent, contrast: Contrast) -> Theme {
        let seed = accent.seed();
        let name =
            format!("chatkord-{scheme:?}-{:02x}{:02x}{:02x}-{contrast:?}", seed.r, seed.g, seed.b).to_lowercase();
        Theme { palette: Palette::with_contrast(scheme, accent, contrast), name, motion: MotionPreference::Full }
    }

    /// The same theme for a system that asks for `motion`: what moves by itself holds still when it asks for less.
    pub fn with_motion(mut self, motion: MotionPreference) -> Theme {
        let moving = self.name.trim_end_matches(STILL).len();
        self.name.truncate(moving);
        if motion == MotionPreference::Reduced {
            self.name.push_str(STILL);
        }
        self.motion = motion;
        self
    }

    /// The motion the system asks for, which what moves by itself reads before it moves.
    pub fn motion(&self) -> MotionPreference {
        self.motion
    }

    /// Every role's colour in this scheme, accent and contrast.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The painted colour of a role.
    pub fn color(&self, role: Role) -> iced_core::Color {
        color(self.palette.color(role))
    }

    /// `role`'s colour showing `alpha` of 255.
    fn veiled(&self, role: Role, alpha: u8) -> iced_core::Color {
        crate::color::translucent(design::Rgba::new(self.palette.color(role), alpha))
    }

    /// The shadow `elevation` casts in this scheme; none for what lies flat.
    fn cast(&self, elevation: Elevation) -> iced_core::Shadow {
        elevation.shadow(self.palette.scheme()).map_or_else(iced_core::Shadow::default, |shadow| iced_core::Shadow {
            color: iced_core::Color::BLACK.scale_alpha(f32::from(shadow.alpha) / 255.0),
            offset: iced_core::Vector::new(0.0, shadow.offset_y),
            blur_radius: shadow.blur,
        })
    }
}

impl Base for Theme {
    fn default(preference: Mode) -> Self {
        let scheme = match preference {
            Mode::Light => Scheme::Light,
            Mode::Dark | Mode::None => Scheme::Dark,
        };
        Theme::new(scheme, Accent::DEFAULT)
    }

    fn mode(&self) -> Mode {
        match self.palette.scheme() {
            Scheme::Light => Mode::Light,
            Scheme::Dark => Mode::Dark,
        }
    }

    fn base(&self) -> Style {
        Style {
            background_color: self.color(Role::Surface(Surface::Base)),
            text_color: self.color(Role::Content(Content::Primary)),
        }
    }

    fn seed(&self) -> Option<iced_core::theme::palette::Seed> {
        None
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// What text is coloured as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextClass {
    /// The colour of whatever holds it: a button's, a container's. The default, so text inside a
    /// component is painted by that component's look and never overridden.
    Inherit,
    /// A content role's colour, whatever holds the text.
    Content(Content),
    /// A link, accented text on a surface.
    Accent,
    /// A status said in text on a surface.
    Status(Status),
}

impl text::Catalog for Theme {
    type Class<'a> = TextClass;

    fn default<'a>() -> Self::Class<'a> {
        TextClass::Inherit
    }

    fn style(&self, class: &Self::Class<'_>) -> text::Style {
        let role = match class {
            TextClass::Inherit => None,
            TextClass::Content(content) => Some(Role::Content(*content)),
            TextClass::Accent => Some(Role::Accent(design::AccentRole::Text)),
            TextClass::Status(status) => Some(Role::Status(*status, StatusRole::Text)),
        };
        text::Style { color: role.map(|role| self.color(role)), selection: Some(translucent(self.palette.selection())) }
    }

    fn selection(&self) -> iced_core::Color {
        translucent(self.palette.selection())
    }
}

/// A container that is a surface: its background, and the text on it, from the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surfaced {
    /// Nothing painted.
    Clear,
    /// A surface's colour, with the primary content colour for the text on it.
    On(Surface),
    /// A panel at a height, with its corners: its surface, a hairline edge, and the shadow the height casts.
    Panel(Elevation, Radius),
    /// A surface with a hairline edge and its corners, casting nothing: a choice's track, a table, a field shown and
    /// not typed into.
    Framed(Surface, Radius),
    /// A switch's knob, as its look says.
    Knob(design::SwitchLook),
    /// Filled with a role, with the text on it, and rounded: a badge, a picture of a person.
    Fill {
        /// The role filled in.
        fill: Role,
        /// The role of the text on the fill.
        text: Role,
        /// Its corners.
        radius: Radius,
    },
    /// A check box: filled with a role inside a `CHECK_BOX_EDGE` edge of another, one box, so its corners stay
    /// concentric and it never reads as a radio's circle.
    CheckBox {
        /// The role filled in.
        fill: Role,
        /// The edge's role.
        edge: Role,
    },
    /// A callout: the surface colour in a hairline edge, in its status's colour when it has one.
    Callout(Option<Status>),
    /// The translucent layer under a label on a picture, with the text that reads on it over any picture.
    Scrim,
    /// The same layer over a whole screen's card, at its corners: a picture that is not live now.
    Veil,
    /// Nothing filled, a glowing ring round it: someone who is speaking.
    Speaking(Radius),
    /// Filled with a server's colour as picked, with the letter that reads on it: a server's picture. `dim` draws it
    /// faint, for a server not signed in.
    Server {
        /// The server's colour, as picked.
        colour: Rgb,
        /// Its corners.
        radius: Radius,
        /// Faint, for a server not signed in.
        dim: bool,
    },
    /// The accent's ring round a server's picture, a gap of the chrome inside it: a server connecting.
    ServerRing(Radius),
    /// A picked colour as a field shows it: filled with it, with the strong edge a field has.
    Swatch(Rgb),
    /// A colour's dot: filled with the colour picked, or with the strong edge's where none was.
    Dot(Option<Rgb>),
    /// The bar at the open server's edge of the rail, rounded on its free side.
    RailBar,
    /// A toast: the text colour as the ground, the window's colour as the text.
    Toast,
    /// A person's picture in a call: their colour behind their initials.
    Person {
        /// Their colour, behind their initials.
        colour: PersonColour,
        /// Its corners.
        radius: Radius,
        /// Drawn faint: someone offline, in the members column.
        faint: bool,
    },
    /// A person's tile in a call: their colour thinly over the window, a ring round the inside while they are heard.
    Tile {
        /// Their colour, laid thinly over the window.
        colour: PersonColour,
        /// Ringed while they are heard.
        ringed: bool,
    },
    /// A card with a picture in it, a shared screen or a camera: [`Role::PictureGround`] around a picture shown whole,
    /// rounded as a person's tile is, its text and icons on it in [`Role::OnScrim`].
    Screen,
    /// What raises the floating card over the window: the overlay's shadow, rounded as a screen's card is.
    Floating,
    /// What a picture over the whole display is letterboxed on: [`Role::PictureGround`], as a card's.
    Backdrop,
    /// A ring in the speaking colour round a small picture, with no glow: someone heard, listed in the side pane.
    Ring(Radius),
    /// The call bar: a pill floating over the call.
    StageBar,
    /// A name on a tile: the scrim, as a pill.
    ScrimPill,
    /// What is being edited in place among its like (a server's emoji renamed): a surface and its corners, edged in the
    /// accent.
    Editing(Surface, Radius),
}

impl container::Catalog for Theme {
    type Class<'a> = Surfaced;

    fn default<'a>() -> Self::Class<'a> {
        Surfaced::Clear
    }

    fn style(&self, class: &Self::Class<'_>) -> container::Style {
        match class {
            Surfaced::Clear => container::Style::default(),
            Surfaced::On(surface) => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(self.color(Role::Surface(*surface)).into()),
                ..container::Style::default()
            },
            Surfaced::Panel(elevation, radius) => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(self.color(Role::Surface(elevation.surface())).into()),
                border: Border {
                    color: self.color(Role::Border(DesignBorder::Subtle)),
                    width: BORDER_WIDTH,
                    radius: radius.px().into(),
                },
                shadow: self.cast(*elevation),
                ..container::Style::default()
            },
            Surfaced::Framed(surface, radius) => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(self.color(Role::Surface(*surface)).into()),
                border: Border {
                    color: self.color(Role::Border(DesignBorder::Subtle)),
                    width: BORDER_WIDTH,
                    radius: radius.px().into(),
                },
                ..container::Style::default()
            },
            Surfaced::Knob(look) => container::Style {
                background: Some(self.veiled(look.knob, look.alpha).into()),
                border: Border { radius: Radius::Full.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Fill { fill, text, radius } => container::Style {
                text_color: Some(self.color(*text)),
                background: Some(self.color(*fill).into()),
                border: Border { radius: radius.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::CheckBox { fill, edge } => container::Style {
                background: Some(self.color(*fill).into()),
                border: Border {
                    color: self.color(*edge),
                    width: design::CHECK_BOX_EDGE,
                    radius: design::CHECK_BOX_RADIUS.px().into(),
                },
                ..container::Style::default()
            },
            Surfaced::Scrim => container::Style {
                text_color: Some(self.color(Role::OnScrim)),
                background: Some(translucent(self.palette.scrim()).into()),
                border: Border { radius: Radius::S.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Speaking(radius) => container::Style {
                border: Border {
                    color: self.color(Role::Speaking),
                    width: SPEAKING_GLOW.width,
                    radius: radius.px().into(),
                },
                shadow: iced_core::Shadow {
                    color: self.color(Role::Speaking).scale_alpha(f32::from(SPEAKING_GLOW.alpha) / 255.0),
                    offset: iced_core::Vector::ZERO,
                    blur_radius: SPEAKING_GLOW.blur,
                },
                ..container::Style::default()
            },
            Surfaced::Server { colour, radius, dim } => {
                let faint = |colour: iced_core::Color| if *dim { colour.scale_alpha(design::RAIL_DIM) } else { colour };
                container::Style {
                    text_color: Some(faint(color(readable_on(*colour)))),
                    background: Some(faint(color(*colour)).into()),
                    border: Border { radius: radius.px().into(), ..Border::default() },
                    ..container::Style::default()
                }
            }
            Surfaced::Dot(colour) => container::Style {
                background: Some(colour.map_or_else(|| self.color(Role::Border(DesignBorder::Strong)), color).into()),
                border: Border { radius: Radius::Full.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Swatch(colour) => container::Style {
                background: Some(color(*colour).into()),
                border: Border {
                    color: self.color(Role::Border(DesignBorder::Strong)),
                    width: BORDER_WIDTH,
                    radius: design::INPUT_RADIUS.px().into(),
                },
                ..container::Style::default()
            },
            Surfaced::ServerRing(radius) => container::Style {
                border: Border {
                    color: self.color(Role::Accent(design::AccentRole::Ring)),
                    width: RING_WIDTH,
                    radius: radius.px().into(),
                },
                ..container::Style::default()
            },
            Surfaced::RailBar => {
                let round = design::RAIL_BAR.0 - 1.0;
                container::Style {
                    background: Some(self.color(Role::Content(Content::Primary)).into()),
                    border: Border {
                        radius: iced_core::border::Radius {
                            top_left: 0.0,
                            top_right: round,
                            bottom_right: round,
                            bottom_left: 0.0,
                        },
                        ..Border::default()
                    },
                    ..container::Style::default()
                }
            }
            Surfaced::Person { colour, radius, faint } => {
                let look = self.palette.person(*colour);
                let faint =
                    |colour: iced_core::Color| if *faint { colour.scale_alpha(design::OFFLINE_DIM) } else { colour };
                container::Style {
                    text_color: Some(faint(color(look.on_fill))),
                    background: Some(faint(color(look.fill)).into()),
                    border: Border { radius: radius.px().into(), ..Border::default() },
                    ..container::Style::default()
                }
            }
            Surfaced::Tile { colour, ringed } => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(color(self.palette.person(*colour).ground).into()),
                border: Border {
                    color: self.color(Role::Speaking),
                    width: if *ringed { RING_WIDTH } else { 0.0 },
                    radius: TILE_RADIUS.px().into(),
                },
                ..container::Style::default()
            },
            Surfaced::Veil => container::Style {
                text_color: Some(self.color(Role::OnScrim)),
                background: Some(translucent(self.palette.scrim()).into()),
                border: Border { radius: TILE_RADIUS.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Backdrop => container::Style {
                text_color: Some(self.color(Role::OnScrim)),
                background: Some(self.color(Role::PictureGround).into()),
                ..container::Style::default()
            },
            Surfaced::Screen => container::Style {
                text_color: Some(self.color(Role::OnScrim)),
                background: Some(self.color(Role::PictureGround).into()),
                border: Border { radius: TILE_RADIUS.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Ring(radius) => container::Style {
                border: Border { color: self.color(Role::Speaking), width: RING_WIDTH, radius: radius.px().into() },
                ..container::Style::default()
            },
            Surfaced::StageBar => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(self.color(Role::Surface(Surface::Inset)).into()),
                border: Border {
                    color: self.color(Role::Border(DesignBorder::Subtle)),
                    width: BORDER_WIDTH,
                    radius: Radius::Full.px().into(),
                },
                shadow: self.cast(Elevation::Overlay),
                ..container::Style::default()
            },
            Surfaced::Floating => container::Style {
                border: Border { radius: TILE_RADIUS.px().into(), ..Border::default() },
                shadow: self.cast(Elevation::Overlay),
                ..container::Style::default()
            },
            Surfaced::ScrimPill => container::Style {
                text_color: Some(self.color(Role::OnScrim)),
                background: Some(translucent(self.palette.scrim()).into()),
                border: Border { radius: Radius::Full.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Editing(surface, radius) => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(self.color(Role::Surface(*surface)).into()),
                border: Border {
                    color: self.color(Role::Accent(design::AccentRole::Fill)),
                    width: BORDER_WIDTH,
                    radius: radius.px().into(),
                },
                ..container::Style::default()
            },
            Surfaced::Toast => container::Style {
                text_color: Some(self.color(Role::Surface(Surface::Base))),
                background: Some(self.color(Role::Content(Content::Primary)).into()),
                border: Border { radius: Radius::M.px().into(), ..Border::default() },
                ..container::Style::default()
            },
            Surfaced::Callout(status) => container::Style {
                text_color: Some(self.color(Role::Content(Content::Primary))),
                background: Some(self.color(Role::Surface(Surface::Raised)).into()),
                border: Border {
                    color: self.color(status.map_or(Role::Border(design::Border::Subtle), design::notice_edge)),
                    width: BORDER_WIDTH,
                    radius: Radius::M.px().into(),
                },
                ..container::Style::default()
            },
        }
    }
}

impl button::Catalog for Theme {
    type Class<'a> = ButtonClass;

    fn default<'a>() -> Self::Class<'a> {
        ButtonClass::default()
    }

    fn style(&self, class: &Self::Class<'_>, status: button::Status) -> button::Style {
        let loading = matches!(class, ButtonClass::Button { loading: true, .. });
        let interaction = match status {
            button::Status::Active => Interaction::Idle,
            button::Status::Hovered => Interaction::Hover,
            button::Status::Pressed => Interaction::Pressed,
            // A button with no message is disabled to iced; one whose work is under way only looks so.
            button::Status::Disabled if loading => Interaction::Loading,
            button::Status::Disabled => Interaction::Disabled,
        };
        let none = iced_core::Color::TRANSPARENT;
        match class {
            ButtonClass::Button { variant, .. } => {
                let look = design::button(*variant, interaction);
                button::Style {
                    background: look.fill.map(|role| Background::Color(self.color(role))),
                    text_color: self.color(look.text),
                    border: Border {
                        color: look.border.map_or(none, |role| self.color(role)),
                        width: if look.border.is_some() { BORDER_WIDTH } else { 0.0 },
                        radius: BUTTON_RADIUS.px().into(),
                    },
                    ..button::Style::default()
                }
            }
            ButtonClass::RailFace { current } => {
                let look = design::quiet_row(*current, interaction);
                button::Style {
                    background: look.fill.map(|role| Background::Color(self.color(role))),
                    text_color: self.color(look.text),
                    border: Border { radius: design::RAIL_RADIUS.px().into(), ..Border::default() },
                    ..button::Style::default()
                }
            }
            ButtonClass::Rail | ButtonClass::Swatch | ButtonClass::SearchPill => {
                button::Style { text_color: self.color(Role::Content(Content::Primary)), ..button::Style::default() }
            }
            ButtonClass::Card { selected } => {
                let look = design::source_card(*selected, interaction);
                button::Style {
                    background: Some(Background::Color(self.color(look.fill))),
                    text_color: self.color(Role::Content(Content::Primary)),
                    border: Border {
                        color: self.color(look.edge),
                        width: look.edge_width,
                        radius: design::SOURCE_CARD_RADIUS.px().into(),
                    },
                    ..button::Style::default()
                }
            }
            ButtonClass::Field => {
                // A press shows as a pointer over it; the focus is the ring drawn round it, as round any control.
                let state = match interaction {
                    Interaction::Idle => InputState::Idle,
                    Interaction::Hover | Interaction::Pressed => InputState::Hover,
                    Interaction::Disabled | Interaction::Loading => InputState::Disabled,
                };
                let look = design::input(state, false);
                button::Style {
                    background: Some(Background::Color(self.color(look.fill))),
                    text_color: self.color(look.text),
                    border: Border {
                        color: self.color(look.border),
                        width: look.border_width,
                        radius: design::INPUT_RADIUS.px().into(),
                    },
                    ..button::Style::default()
                }
            }
            ButtonClass::Chip { mine } => {
                let look = design::chip(*mine, interaction);
                button::Style {
                    background: Some(Background::Color(self.color(look.fill))),
                    text_color: self.color(look.text),
                    border: Border {
                        color: self.color(look.edge),
                        width: BORDER_WIDTH,
                        radius: Radius::Full.px().into(),
                    },
                    ..button::Style::default()
                }
            }
            ButtonClass::Switch { on } => button::Style {
                background: Some(Background::Color({
                    let look = design::switch(*on, interaction);
                    self.veiled(look.track, look.alpha)
                })),
                border: Border { radius: Radius::Full.px().into(), ..Border::default() },
                ..button::Style::default()
            },
            ButtonClass::Call(control) => {
                let look = design::call_button(*control, interaction);
                button::Style {
                    background: Some(Background::Color(self.color(look.fill))),
                    text_color: self.color(look.icon),
                    border: Border { radius: BUTTON_RADIUS.px().into(), ..Border::default() },
                    ..button::Style::default()
                }
            }
            ButtonClass::Pill { kind, chosen } => {
                let look = design::pill(*kind, *chosen, interaction);
                button::Style {
                    background: look.fill.map(|role| Background::Color(self.color(role))),
                    text_color: self.color(look.text),
                    border: Border { radius: BUTTON_RADIUS.px().into(), ..Border::default() },
                    ..button::Style::default()
                }
            }
            ButtonClass::Row { selected, tone } => {
                let look = match tone {
                    RowTone::Plain => design::row(*selected, interaction),
                    RowTone::Quiet => design::quiet_row(*selected, interaction),
                    RowTone::Danger => design::danger_row(interaction),
                    RowTone::Chosen => design::chosen_row(interaction),
                };
                button::Style {
                    background: look.fill.map(|role| Background::Color(self.color(role))),
                    text_color: self.color(look.text),
                    border: Border { radius: ROW_RADIUS.px().into(), ..Border::default() },
                    ..button::Style::default()
                }
            }
        }
    }
}

/// How full a progress bar is shown: the bar on an inset track, in the accent, or in the success colour for how loud a
/// microphone is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProgressClass {
    /// In the accent: how far something has come.
    #[default]
    Accent,
    /// In the success colour: how loud a microphone is.
    Level,
}

impl progress_bar::Catalog for Theme {
    type Class<'a> = ProgressClass;

    fn default<'a>() -> Self::Class<'a> {
        ProgressClass::default()
    }

    fn style(&self, class: &Self::Class<'_>) -> progress_bar::Style {
        let bar = match class {
            ProgressClass::Accent => Role::Accent(design::AccentRole::Fill),
            ProgressClass::Level => Role::Status(design::Status::Success, design::StatusRole::Fill),
        };
        progress_bar::Style {
            background: Background::Color(self.color(Role::Surface(Surface::Inset))),
            bar: Background::Color(self.color(bar)),
            border: Border { radius: Radius::Full.px().into(), ..Border::default() },
        }
    }
}

/// The one scroll bar: a slim pill on a clear rail of its own beside what scrolls, never over it, as the canvas draws
/// it; `ScrollClass` colours it. Every scrollable takes it.
pub(crate) fn scroll_bar() -> iced_widget::scrollable::Scrollbar {
    iced_widget::scrollable::Scrollbar::new()
        .width(design::SCROLL_RAIL)
        .scroller_width(design::SCROLLER)
        .spacing(design::Space::Xs.px())
}

/// A scrollable's class: the one look, a scroller in the strong edge colour on a clear rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollClass;

impl scrollable::Catalog for Theme {
    type Class<'a> = ScrollClass;

    fn default<'a>() -> Self::Class<'a> {
        ScrollClass
    }

    fn style(&self, _class: &Self::Class<'_>, status: scrollable::Status) -> scrollable::Style {
        let held = matches!(status, scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. });
        let scroller = scrollable::Scroller {
            background: Background::Color(self.color(if held {
                Role::Content(Content::Secondary)
            } else {
                Role::Border(DesignBorder::Strong)
            })),
            border: Border { radius: Radius::Full.px().into(), ..Border::default() },
        };
        let rail = scrollable::Rail { background: None, border: Border::default(), scroller };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(self.color(Role::Surface(Surface::Overlay))),
                border: Border { radius: Radius::Full.px().into(), ..Border::default() },
                shadow: iced_core::Shadow::default(),
                icon: self.color(Role::Content(Content::Primary)),
            },
        }
    }
}
