//! Our own picker of what to share, where the system has none a share must go through (Windows; on Linux the desktop's
//! picker opens instead): the screens and applications the host lists, each with a still it refreshes while the picker
//! is open, one selected at a time, shared by Share.

use words::{Sentence, Words};

/// Which picker a share is chosen in, as the host says for its system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Chooser {
    /// The desktop's own (the portal on Linux).
    #[default]
    Desktop,
    /// Ours, over the window.
    Own,
}

/// A picture the host made and paints by this id: a source's still or an application's icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Painting(pub u64);

/// What a source is: a tab of the picker each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A whole screen, real or virtual.
    Screen,
    /// One application's window.
    Application,
}

/// The host's name for a source, while it exists; opaque here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceKey(pub String);

/// A screen or application as the host lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// What Share names it by to the host.
    pub key: SourceKey,
    /// Which tab it is listed on.
    pub kind: SourceKind,
    /// An application's window title; a screen is named by its place.
    pub title: Option<String>,
    /// A screen's size in pixels.
    pub size: Option<(u32, u32)>,
    /// An application's icon, where the system gives one.
    pub icon: Option<Painting>,
    /// A still of what it shows; none where none could be taken.
    pub still: Option<Painting>,
}

/// A tab of the picker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// The screens, which the picker opens on.
    #[default]
    Screens,
    /// The applications' windows.
    Applications,
}

impl Tab {
    fn kind(self) -> SourceKind {
        match self {
            Self::Screens => SourceKind::Screen,
            Self::Applications => SourceKind::Application,
        }
    }
}

/// A card of the open tab, as it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// The source it shows, which selecting it selects.
    pub key: SourceKey,
    /// "Screen 1", or the application's title.
    pub name: String,
    /// A screen's size, beside its name.
    pub note: Option<String>,
    /// The application's icon, where there is one.
    pub icon: Option<Painting>,
    /// The source's still, where there is one.
    pub still: Option<Painting>,
    /// It is the one Share would share.
    pub selected: bool,
}

/// The picker while it is open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    tab: Tab,
    listed: Vec<Source>,
    selected: Option<SourceKey>,
}

impl Sources {
    /// A picker as it would be: on `tab`, `listed`, with `selected` where it is listed. For a front end's tests and
    /// previews; the window makes its own from what the host lists.
    pub fn new(tab: Tab, listed: Vec<Source>, selected: Option<SourceKey>) -> Self {
        let mut sources = Self { tab, listed: Vec::new(), selected: None };
        sources.listed(listed);
        if let Some(key) = selected {
            sources.select(key);
        }
        sources
    }

    /// The tab that is open.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The open tab's cards, in the host's order: screens numbered from 1 in it.
    pub fn cards(&self, words: &Words) -> Vec<Card> {
        let kind = self.tab.kind();
        self.listed
            .iter()
            .filter(|source| source.kind == kind)
            .enumerate()
            .map(|(at, source)| Card {
                key: source.key.clone(),
                name: match (&source.title, source.kind) {
                    (Some(title), SourceKind::Application) => title.clone(),
                    _ => words.say(&Sentence::ScreenNumbered(at + 1)),
                },
                note: match source.kind {
                    SourceKind::Screen => source.size.map(|(w, h)| words.say(&Sentence::PixelSize(w, h))),
                    SourceKind::Application => None,
                },
                icon: source.icon,
                still: source.still,
                selected: self.selected.as_ref() == Some(&source.key),
            })
            .collect()
    }

    /// The source Share would share; none until one is selected.
    pub fn selected(&self) -> Option<&SourceKey> {
        self.selected.as_ref()
    }

    pub(crate) fn show(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// The host's newest list. A selected source still in it stays selected; one gone is let go of, so Share never
    /// names what is no longer there.
    pub(crate) fn listed(&mut self, listed: Vec<Source>) {
        if self.selected.as_ref().is_some_and(|key| !listed.iter().any(|source| &source.key == key)) {
            self.selected = None;
        }
        self.listed = listed;
    }

    /// Selects a listed source; a key not listed selects nothing.
    pub(crate) fn select(&mut self, key: SourceKey) {
        if self.listed.iter().any(|source| source.key == key) {
            self.selected = Some(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(key: &str, size: (u32, u32)) -> Source {
        Source {
            key: SourceKey(key.into()),
            kind: SourceKind::Screen,
            title: None,
            size: Some(size),
            icon: None,
            still: None,
        }
    }

    fn app(key: &str, title: &str) -> Source {
        Source {
            key: SourceKey(key.into()),
            kind: SourceKind::Application,
            title: Some(title.into()),
            size: None,
            icon: Some(Painting(7)),
            still: Some(Painting(8)),
        }
    }

    fn listed() -> Sources {
        let mut sources = Sources::default();
        sources.listed(vec![
            screen("m1", (3840, 2160)),
            app("w1", "Notes"),
            screen("m2", (1080, 1920)),
            app("w2", "Terminal"),
        ]);
        sources
    }

    #[test]
    fn screens_are_numbered_with_their_size_and_applications_named_by_their_title() {
        let mut sources = listed();
        let plain = words::without_isolation;
        let names: Vec<_> = sources
            .cards(&Words::english())
            .into_iter()
            .map(|card| (plain(&card.name), card.note.as_deref().map(plain)))
            .collect();
        assert_eq!(
            names,
            [("Screen 1".into(), Some("3840 × 2160".into())), ("Screen 2".into(), Some("1080 × 1920".into()))]
        );
        sources.show(Tab::Applications);
        let cards = sources.cards(&Words::english());
        assert_eq!(cards.iter().map(|card| card.name.as_str()).collect::<Vec<_>>(), ["Notes", "Terminal"]);
        assert!(cards.iter().all(|card| card.note.is_none() && card.icon.is_some() && card.still.is_some()));
    }

    #[test]
    fn nothing_is_selected_until_a_listed_card_is_and_one_at_a_time() {
        let mut sources = listed();
        assert_eq!(sources.selected(), None);
        sources.select(SourceKey("nowhere".into()));
        assert_eq!(sources.selected(), None, "a key not listed selects nothing");
        sources.select(SourceKey("m2".into()));
        sources.show(Tab::Applications);
        sources.select(SourceKey("w1".into()));
        assert_eq!(sources.selected(), Some(&SourceKey("w1".into())), "one at a time, across the tabs");
        assert_eq!(sources.cards(&Words::english()).iter().filter(|card| card.selected).count(), 1);
    }

    #[test]
    fn a_selected_source_that_goes_away_is_let_go_and_one_that_stays_is_kept() {
        let mut sources = listed();
        sources.select(SourceKey("w2".into()));
        sources.listed(vec![screen("m1", (3840, 2160)), app("w2", "Terminal (2)")]);
        assert_eq!(sources.selected(), Some(&SourceKey("w2".into())), "still there, renamed");
        sources.listed(vec![screen("m1", (3840, 2160))]);
        assert_eq!(sources.selected(), None, "closed since it was selected");
    }
}
