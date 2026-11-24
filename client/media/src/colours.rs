//! What the samples of an 8-bit YUV picture mean: the matrix that turns luma and the colour differences into RGB, and
//! the range they span. A stream is tagged with what its pictures carry and a picture is painted by what it carries,
//! so the colours a camera or a screen has are the colours a viewer sees.

use crate::av::{ColourRange, ColourSpace, Frame};

/// The weights of red and blue in luma, which fix how a picture's colour differences become RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Matrix {
    /// ITU-R BT.601: a camera's picture (a JPEG's by JFIF, a raw one's as UVC sends it).
    Bt601,
    /// ITU-R BT.709: what a screen's convert writes.
    Bt709,
}

impl Matrix {
    /// The weights of red and blue in luma (`Kr`, `Kb`); green's is what is left of one.
    pub const fn weights(self) -> (f32, f32) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
        }
    }

    /// The tag a stream of this matrix carries. BT.470 BG and SMPTE 170M weigh the same as BT.601; the first is the
    /// one this client writes.
    pub(crate) const fn tag(self) -> ColourSpace {
        match self {
            Self::Bt601 => ColourSpace::Bt470bg,
            Self::Bt709 => ColourSpace::Bt709,
        }
    }

    /// The matrix a tag names: BT.601's three spellings and FCC's, which weighs nearly the same, are BT.601; any other
    /// is BT.709, as is none, the tag every stream carried before streams said which.
    fn of_tag(tag: ColourSpace) -> Self {
        match tag {
            ColourSpace::Bt470bg | ColourSpace::Smpte170m | ColourSpace::Fcc => Self::Bt601,
            _ => Self::Bt709,
        }
    }
}

/// How far an 8-bit sample's values reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Range {
    /// Studio swing: luma from 16 to 235, colour differences from 16 to 240.
    Limited,
    /// Every value: 0 to 255 for luma and for colour differences.
    Full,
}

impl Range {
    /// The luma sample of black, and how many values reach from it to white.
    pub const fn luma(self) -> (u8, u8) {
        match self {
            Self::Limited => (16, 219),
            Self::Full => (0, 255),
        }
    }

    /// How many values a colour difference spans around its middle, 128.
    pub const fn chroma(self) -> u8 {
        match self {
            Self::Limited => 224,
            Self::Full => 255,
        }
    }

    /// The tag a stream of this range carries.
    pub(crate) const fn tag(self) -> ColourRange {
        match self {
            Self::Limited => ColourRange::Limited,
            Self::Full => ColourRange::Full,
        }
    }

    /// The range a tag names: full only where it says so, else studio swing, as a tag that names none always was.
    fn of_tag(tag: ColourRange) -> Self {
        match tag {
            ColourRange::Full => Self::Full,
            _ => Self::Limited,
        }
    }
}

/// The matrix and range a picture's samples are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colours {
    /// What turns its luma and colour differences into RGB.
    pub matrix: Matrix,
    /// How far its samples reach.
    pub range: Range,
}

impl Colours {
    /// BT.709 in limited range: what a screen's convert writes, and what a stream that names neither is read as.
    pub const HD: Self = Self { matrix: Matrix::Bt709, range: Range::Limited };

    /// Every pair a picture can carry, at the place [`Colours::index`] names.
    pub const ALL: [Self; 4] = [
        Self { matrix: Matrix::Bt601, range: Range::Limited },
        Self { matrix: Matrix::Bt601, range: Range::Full },
        Self { matrix: Matrix::Bt709, range: Range::Limited },
        Self { matrix: Matrix::Bt709, range: Range::Full },
    ];

    /// Where these colours stand in [`Colours::ALL`]; a new matrix or range stops the build here.
    pub const fn index(self) -> usize {
        let matrix = match self.matrix {
            Matrix::Bt601 => 0,
            Matrix::Bt709 => 2,
        };
        let range = match self.range {
            Range::Limited => 0,
            Range::Full => 1,
        };
        matrix + range
    }

    /// What the tags of a decoded frame say.
    pub(crate) fn tagged(space: ColourSpace, range: ColourRange) -> Self {
        Self { matrix: Matrix::of_tag(space), range: Range::of_tag(range) }
    }

    /// What `frame` says its samples are.
    pub(crate) fn of(frame: &Frame) -> Self {
        Self::tagged(frame.colour_space(), frame.colour_range())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ColourRange as R;
    use ColourSpace as S;

    #[test]
    fn a_matrix_tag_is_read_as_the_matrix_it_weighs() {
        let read = |space| Colours::tagged(space, R::Limited).matrix;
        for space in [S::Bt470bg, S::Smpte170m, S::Fcc] {
            assert_eq!(read(space), Matrix::Bt601, "{space:?}");
        }
        assert_eq!(read(S::Bt709), Matrix::Bt709);
        for named_none_or_other in [S::Unspecified, S::Reserved, S::Bt2020Ncl] {
            assert_eq!(read(named_none_or_other), Matrix::Bt709, "{named_none_or_other:?}");
        }
    }

    #[test]
    fn full_is_only_what_a_range_tag_says_is_full() {
        let read = |range| Colours::tagged(S::Bt709, range).range;
        assert_eq!(read(R::Full), Range::Full);
        assert_eq!(read(R::Limited), Range::Limited);
        assert_eq!(read(R::Unspecified), Range::Limited);
    }

    #[test]
    fn every_pair_stands_at_its_own_index() {
        for (at, colours) in Colours::ALL.into_iter().enumerate() {
            assert_eq!(colours.index(), at);
        }
    }

    #[test]
    fn what_a_stream_is_tagged_with_reads_back_as_itself() {
        for matrix in [Matrix::Bt601, Matrix::Bt709] {
            for range in [Range::Limited, Range::Full] {
                let colours = Colours { matrix, range };
                assert_eq!(Colours::tagged(matrix.tag(), range.tag()), colours);
            }
        }
    }
}
