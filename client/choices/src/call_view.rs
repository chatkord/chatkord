//! How a call is drawn on this device: the people with no picture left out while some card has one, whether the call
//! bar always shows, and one's own share left out. Picked in the call's Voice options and kept with how the window
//! looks.

/// Which people's cards a call shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum People {
    /// Everyone's.
    #[default]
    All,
    /// Only those with a picture, while some card has one; a call with none shows everyone.
    Pictured,
}

/// Whether this account's own share has a card here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum OwnShare {
    /// It has.
    #[default]
    Shown,
    /// It has not; it still goes out to the others.
    Hidden,
}

/// When the call bar shows over the stage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum Controls {
    /// Always, in room the stage keeps for it.
    #[default]
    Always,
    /// While the pointer is over the call, a control of the bar has the keyboard's focus or a menu from it is open;
    /// the stage takes the room it kept.
    WhileHovered,
}

/// How a call is drawn on this device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallView {
    /// Which people's cards it shows.
    pub people: People,
    /// When the call bar shows.
    pub controls: Controls,
    /// Whether one's own share has a card.
    pub own_share: OwnShare,
}

/// A switch of Voice options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPick {
    /// Hide people with no picture, or show them again.
    People,
    /// Show the call bar always, or only while it is wanted.
    Controls,
    /// Hide one's own share, or show it again.
    OwnShare,
}

impl CallView {
    /// Flips the switch `pick` names and leaves the others.
    pub fn pick(&mut self, pick: ViewPick) {
        match pick {
            ViewPick::People => {
                self.people = match self.people {
                    People::All => People::Pictured,
                    People::Pictured => People::All,
                };
            }
            ViewPick::Controls => {
                self.controls = match self.controls {
                    Controls::Always => Controls::WhileHovered,
                    Controls::WhileHovered => Controls::Always,
                };
            }
            ViewPick::OwnShare => {
                self.own_share = match self.own_share {
                    OwnShare::Shown => OwnShare::Hidden,
                    OwnShare::Hidden => OwnShare::Shown,
                };
            }
        }
    }
}
