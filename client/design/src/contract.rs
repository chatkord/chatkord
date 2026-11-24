//! The contrast every pair of roles keeps, in every scheme and under every accent. It is the one
//! owner of which colour sits on which: the palette derives the accent, status and speaking
//! colours by searching for ones that satisfy it, and the tests check it whole.

use crate::color::{Blend, Ratio, Rgb};
use crate::listed;
use crate::palette::Palette;
use crate::role::{AccentRole, Border, Content, DestructiveRole, Role, Status, StatusRole, Surface};

/// What a colour is read against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    /// An opaque role.
    Role(Role),
    /// The palette's scrim over any picture at all: checked over pure black and pure white, mixed
    /// both ways a renderer may mix it.
    Scrim,
}

/// One requirement: `role` against `ground` at least `min`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    /// The colour that must be found.
    pub role: Role,
    /// What it is read on.
    pub ground: Ground,
    /// The least ratio between them.
    pub min: Ratio,
}

/// A requirement a palette does not meet, and the worst ratio found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Violation {
    /// The requirement.
    pub pair: Pair,
    /// The worst ratio it has in that palette, below its least.
    pub found: Ratio,
}

listed! {
    /// How strict the contract is: the standard one, or "More contrast", which a person picks per device.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum Contrast {
        #[default]
        /// What WCAG 2.2's AA asks.
        Standard,
        /// What its AAA asks of text, and more of the rest.
        More,
    }
}

impl Contrast {
    /// The least ratio of text against what it sits on.
    pub const fn text(self) -> Ratio {
        match self {
            Contrast::Standard => Ratio::TEXT,
            Contrast::More => Ratio::TEXT_MORE,
        }
    }

    /// The least ratio of what must be found (an edge, a fill, a ring, an icon) against what it sits on.
    pub const fn non_text(self) -> Ratio {
        match self {
            Contrast::Standard => Ratio::NON_TEXT,
            Contrast::More => Ratio::NON_TEXT_MORE,
        }
    }

    /// The least ratio of what is disabled, so it is still seen.
    pub const fn disabled(self) -> Ratio {
        match self {
            Contrast::Standard => Ratio::DISABLED,
            Contrast::More => Ratio::DISABLED_MORE,
        }
    }
}

/// Every requirement of the standard contract.
pub fn contract() -> Vec<Pair> {
    contract_at(Contrast::Standard)
}

/// Every requirement at `contrast`. Standard: text 4.5:1, what must be found (control edges, fills, rings, icons)
/// 3:1, disabled 2:1, each against every surface it can sit on; More: 7, 4.5 and 3.
pub(crate) fn contract_at(contrast: Contrast) -> Vec<Pair> {
    let (text, non_text) = (contrast.text(), contrast.non_text());
    let mut pairs = Vec::new();
    let mut on_every_surface = |role: Role, min: Ratio| {
        pairs.extend(Surface::ALL.map(|surface| Pair { role, ground: Ground::Role(Role::Surface(surface)), min }));
    };
    on_every_surface(Role::Content(Content::Primary), text);
    on_every_surface(Role::Content(Content::Secondary), text);
    on_every_surface(Role::Content(Content::Disabled), contrast.disabled());
    on_every_surface(Role::Border(Border::Strong), non_text);
    for fill in [AccentRole::Fill, AccentRole::FillHover, AccentRole::FillPressed] {
        on_every_surface(Role::Accent(fill), non_text);
    }
    on_every_surface(Role::Accent(AccentRole::Text), text);
    on_every_surface(Role::Accent(AccentRole::Ring), non_text);
    for status in Status::ALL {
        on_every_surface(Role::Status(status, StatusRole::Fill), non_text);
        on_every_surface(Role::Status(status, StatusRole::Text), text);
    }
    on_every_surface(Role::Speaking, non_text);
    for fill in [DestructiveRole::Fill, DestructiveRole::FillHover, DestructiveRole::FillPressed] {
        on_every_surface(Role::Destructive(fill), non_text);
    }

    for fill in [AccentRole::Fill, AccentRole::FillHover, AccentRole::FillPressed] {
        pairs.push(Pair {
            role: Role::Accent(AccentRole::OnFill),
            ground: Ground::Role(Role::Accent(fill)),
            min: text,
        });
    }
    for fill in [DestructiveRole::Fill, DestructiveRole::FillHover, DestructiveRole::FillPressed] {
        pairs.push(Pair {
            role: Role::Destructive(DestructiveRole::OnFill),
            ground: Ground::Role(Role::Destructive(fill)),
            min: text,
        });
    }
    for status in Status::ALL {
        pairs.push(Pair {
            role: Role::Status(status, StatusRole::OnFill),
            ground: Ground::Role(Role::Status(status, StatusRole::Fill)),
            min: text,
        });
    }
    // What a tinted box holds: its own status's text, ordinary text and, on the accent's, a link.
    let tinted =
        [Role::Accent(AccentRole::Tint)].into_iter().chain(Status::ALL.map(|s| Role::Status(s, StatusRole::Tint)));
    for tint in tinted {
        for role in [Role::Content(Content::Primary), Role::Content(Content::Secondary)] {
            pairs.push(Pair { role, ground: Ground::Role(tint), min: text });
        }
    }
    pairs.push(Pair {
        role: Role::Accent(AccentRole::Text),
        ground: Ground::Role(Role::Accent(AccentRole::Tint)),
        min: text,
    });
    for status in Status::ALL {
        pairs.push(Pair {
            role: Role::Status(status, StatusRole::Text),
            ground: Ground::Role(Role::Status(status, StatusRole::Tint)),
            min: text,
        });
    }
    pairs.push(Pair { role: Role::OnScrim, ground: Ground::Scrim, min: text });
    pairs.push(Pair { role: Role::OnScrimDanger, ground: Ground::Scrim, min: non_text });
    pairs.push(Pair { role: Role::OnScrimDisabled, ground: Ground::Scrim, min: contrast.disabled() });
    for fill in [Role::ScrimHover, Role::ScrimPressed] {
        pairs.push(Pair { role: Role::OnScrim, ground: Ground::Role(fill), min: text });
    }
    pairs.push(Pair {
        role: Role::Content(Content::Disabled),
        ground: Ground::Role(Role::DisabledFill),
        min: contrast.disabled(),
    });
    let picture = Ground::Role(Role::PictureGround);
    pairs.push(Pair { role: Role::OnScrim, ground: picture, min: text });
    pairs.push(Pair { role: Role::OnScrimDanger, ground: picture, min: non_text });
    pairs.push(Pair { role: Role::OnScrimDisabled, ground: picture, min: contrast.disabled() });
    pairs.push(Pair { role: Role::PictureEdge, ground: picture, min: non_text });
    pairs
}

/// Every requirement `palette` does not meet.
pub fn violations(palette: &Palette) -> Vec<Violation> {
    contract_at(palette.contrast())
        .into_iter()
        .filter_map(|pair| {
            let found = pair.measure(palette);
            (!found.meets(pair.min)).then_some(Violation { pair, found })
        })
        .collect()
}

impl Pair {
    /// Whether this requirement is about `role`, on either side.
    pub fn mentions(&self, role: Role) -> bool {
        self.role == role || self.ground == Ground::Role(role)
    }

    /// The worst ratio of this pair in `palette`.
    pub fn measure(&self, palette: &Palette) -> Ratio {
        let color = palette.color(self.role);
        match self.ground {
            Ground::Role(ground) => Ratio::between(color, palette.color(ground)),
            Ground::Scrim => [Rgb::BLACK, Rgb::WHITE]
                .into_iter()
                .flat_map(|picture| Blend::ALL.map(|blend| palette.scrim().over(picture, blend)))
                .map(|mixed| Ratio::between(color, mixed))
                .fold(Ratio::between(Rgb::BLACK, Rgb::WHITE), |worst, ratio| if ratio < worst { ratio } else { worst }),
        }
    }

    /// Whether `palette` meets this requirement.
    pub fn holds(&self, palette: &Palette) -> bool {
        self.measure(palette).meets(self.min)
    }
}
