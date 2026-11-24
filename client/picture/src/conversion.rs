//! What the shader needs to turn a picture's samples into RGB, worked out from the matrix and range they are in, so the
//! shader holds no constant of either.

use media::Colours;

/// The size in bytes of the shader's `Conversion`.
pub(crate) const BYTES: u64 = 48;

/// An 8-bit sample's top value, which a texture's unit range is divided by.
const TOP: f32 = 255.0;
/// The middle of an 8-bit colour difference, which has none to the left of black.
const MIDDLE: f32 = 128.0;

/// The shader's `Conversion`: three vectors of four floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Conversion {
    /// The luma sample of black; the factor that takes luma above it to 0 to 1; the factor that takes a colour
    /// difference around its middle to -0.5 to 0.5; that middle.
    levels: [f32; 4],
    /// What a unit of the red difference adds to red, green and blue.
    from_red: [f32; 4],
    /// What a unit of the blue difference adds to red, green and blue.
    from_blue: [f32; 4],
}

impl Conversion {
    /// The conversion of samples in `colours`: for the matrix's weights `Kr` and `Kb` (and green's `Kg`, what is left of
    /// one), red is luma plus `2 (1 - Kr)` of the red difference, blue is luma plus `2 (1 - Kb)` of the blue one, and
    /// green is luma less the share of both that restores `Kg`.
    pub(crate) fn of(Colours { matrix, range }: Colours) -> Self {
        let (kr, kb) = matrix.weights();
        let kg = 1.0 - kr - kb;
        let (black, span) = range.luma();
        Self {
            levels: [f32::from(black) / TOP, TOP / f32::from(span), TOP / f32::from(range.chroma()), MIDDLE / TOP],
            from_red: [2.0 * (1.0 - kr), -2.0 * kr * (1.0 - kr) / kg, 0.0, 0.0],
            from_blue: [0.0, -2.0 * kb * (1.0 - kb) / kg, 2.0 * (1.0 - kb), 0.0],
        }
    }

    /// The bytes of the shader's uniform.
    pub(crate) fn bytes(&self) -> [u8; 48] {
        let mut bytes = [0; 48];
        let values = self.levels.iter().chain(&self.from_red).chain(&self.from_blue);
        for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            *chunk = value.to_le_bytes();
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::{Matrix, Range};

    /// Red, green and blue of the samples `y`, `cb` and `cr` as the shader works them out.
    fn painted(conversion: Conversion, [y, cb, cr]: [u8; 3]) -> [f32; 3] {
        let unit = |sample: u8| f32::from(sample) / TOP;
        let [black, luma, chroma, middle] = conversion.levels;
        let (y, cb, cr) = ((unit(y) - black) * luma, (unit(cb) - middle) * chroma, (unit(cr) - middle) * chroma);
        std::array::from_fn(|at| (y + cr * conversion.from_red[at] + cb * conversion.from_blue[at]).clamp(0.0, 1.0))
    }

    /// Whether `got` is `want` (0 to 255 a channel) within two of 255.
    fn is(got: [f32; 3], want: [u8; 3]) -> bool {
        got.iter().zip(want).all(|(got, want)| (got * TOP - f32::from(want)).abs() <= 2.0)
    }

    const fn colours(matrix: Matrix, range: Range) -> Colours {
        Colours { matrix, range }
    }

    #[test]
    fn a_red_is_red_in_the_matrix_and_range_it_was_made_in() {
        let red = [255, 0, 0];
        // The standards' own samples of pure red, not the shader's output.
        let bt601_limited = painted(Conversion::of(colours(Matrix::Bt601, Range::Limited)), [81, 90, 240]);
        let bt709_limited = painted(Conversion::of(colours(Matrix::Bt709, Range::Limited)), [63, 102, 240]);
        let bt601_full = painted(Conversion::of(colours(Matrix::Bt601, Range::Full)), [76, 85, 255]);
        assert!(is(bt601_limited, red), "{bt601_limited:?}");
        assert!(is(bt709_limited, red), "{bt709_limited:?}");
        assert!(is(bt601_full, red), "{bt601_full:?}");
    }

    #[test]
    fn a_cameras_red_read_as_bt709_leans_to_orange_which_is_the_bug_this_ends() {
        let lean = painted(Conversion::of(colours(Matrix::Bt709, Range::Limited)), [81, 90, 240]);
        assert!(lean[1] * TOP > 10.0, "green rises from 0: {lean:?}");
        assert!(!is(lean, [255, 0, 0]), "{lean:?}");
    }

    #[test]
    fn grey_and_the_limits_of_a_range_are_the_same_in_every_matrix() {
        for matrix in [Matrix::Bt601, Matrix::Bt709] {
            let limited = Conversion::of(colours(matrix, Range::Limited));
            let full = Conversion::of(colours(matrix, Range::Full));
            assert!(is(painted(limited, [16, 128, 128]), [0, 0, 0]), "{matrix:?} limited black");
            assert!(is(painted(limited, [235, 128, 128]), [255, 255, 255]), "{matrix:?} limited white");
            assert!(is(painted(limited, [126, 128, 128]), [128, 128, 128]), "{matrix:?} limited grey");
            assert!(is(painted(full, [0, 128, 128]), [0, 0, 0]), "{matrix:?} full black");
            assert!(is(painted(full, [255, 128, 128]), [255, 255, 255]), "{matrix:?} full white");
        }
    }

    #[test]
    fn the_uniform_is_the_size_the_shader_declares() {
        let bytes = Conversion::of(Colours::HD).bytes();
        assert_eq!(u64::try_from(bytes.len()), Ok(BYTES));
        assert_eq!(&bytes[..4], (16.0f32 / TOP).to_le_bytes(), "the first float is the luma of black");
    }
}
