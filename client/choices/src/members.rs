//! The members column beside a server's content, as this device keeps it: open or closed by its toggle, and as wide as
//! its splitter left it.

use design::{MEMBERS_LEAST, MEMBERS_MOST, MEMBERS_WIDTH};

/// Whether the members column shows: beside the content where it docks; where it cannot, it goes, and its toggle opens
/// it over the content, which keeps the same choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum MembersShown {
    /// Shown beside the content, as it starts.
    #[default]
    Open,
    /// Put away by its toggle.
    Closed,
}

impl MembersShown {
    /// The other one: what the toggle makes of it.
    #[must_use]
    pub const fn toggled(self) -> MembersShown {
        match self {
            MembersShown::Open => MembersShown::Closed,
            MembersShown::Closed => MembersShown::Open,
        }
    }
}

/// The members column's width as this device keeps it: whole pixels, never outside [`MEMBERS_LEAST`] to
/// [`MEMBERS_MOST`].
pub type MembersWidth = ColumnWidth<MembersSpan>;

/// The members column's range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MembersSpan;

impl Span for MembersSpan {
    const START: f32 = MEMBERS_WIDTH;
    const LEAST: f32 = MEMBERS_LEAST;
    const MOST: f32 = MEMBERS_MOST;
}

/// A column's range of widths: where it starts and how far its splitter drags it.
pub trait Span: Copy + std::fmt::Debug + Eq + std::hash::Hash {
    /// As it starts, and after a reset.
    const START: f32;
    /// The narrowest.
    const LEAST: f32;
    /// The widest.
    const MOST: f32;
}

/// A column's width as this device keeps it: whole pixels, never outside its span.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColumnWidth<S: Span> {
    px: u16,
    span: std::marker::PhantomData<S>,
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "the tokens are whole and small")]
impl<S: Span> ColumnWidth<S> {
    /// As it starts, and after a reset.
    pub const START: ColumnWidth<S> = ColumnWidth { px: S::START as u16, span: std::marker::PhantomData };

    /// `px` wide, rounded to a whole pixel and held within its range; a value that is not a number is its start.
    pub fn new(px: f32) -> ColumnWidth<S> {
        if px.is_nan() {
            return ColumnWidth::START;
        }
        ColumnWidth { px: px.round().clamp(S::LEAST, S::MOST) as u16, span: std::marker::PhantomData }
    }

    /// In logical pixels.
    pub fn px(self) -> f32 {
        f32::from(self.px)
    }

    /// In whole pixels, as it is kept.
    pub fn whole(self) -> u16 {
        self.px
    }
}

impl<S: Span> Default for ColumnWidth<S> {
    fn default() -> ColumnWidth<S> {
        ColumnWidth::START
    }
}

/// The members column as this device keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Members {
    /// Open or closed where it docks.
    pub shown: MembersShown,
    /// Its width, as its splitter left it.
    pub width: MembersWidth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_members_width_is_whole_within_its_range_and_starts_open() {
        assert_eq!(MembersWidth::new(100.0).px(), MEMBERS_LEAST);
        assert_eq!(MembersWidth::new(9000.0).px(), MEMBERS_MOST);
        assert_eq!(MembersWidth::new(f32::NAN), MembersWidth::START);
        assert_eq!(MembersWidth::new(300.4).whole(), 300);
        assert_eq!(Members::default(), Members { shown: MembersShown::Open, width: MembersWidth::new(240.0) });
        assert_eq!(MembersShown::Open.toggled().toggled(), MembersShown::Open);
    }
}
