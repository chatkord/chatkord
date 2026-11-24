//! What every health log says the same way: how many looks a line covers, and how a picture is coded.

use choices::Codec;
use report::{Fact, word};
use screens::Route;

/// How many looks (one a second) a line covers.
pub(crate) const LOOKS_A_LINE: u32 = 10;

/// How a picture is coded, as two public facts: the codec, and whether the graphics hardware or the processor does it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Coded {
    pub(crate) codec: Fact,
    pub(crate) route: Fact,
}

impl Default for Coded {
    fn default() -> Self {
        Self { codec: Fact::Absent, route: Fact::Absent }
    }
}

impl Coded {
    /// `codec` coded by `route`.
    pub(crate) fn of(codec: Codec, route: &Route) -> Self {
        let codec = match codec {
            Codec::H264 => word!("H264"),
            Codec::Av1 => word!("AV1"),
        };
        let route = match route {
            Route::Hardware { .. } => word!("hardware"),
            Route::UntestedHardware { .. } => word!("hardware, untested"),
            Route::Software => word!("software"),
        };
        Self { codec: Fact::Word(codec), route: Fact::Word(route) }
    }
}
