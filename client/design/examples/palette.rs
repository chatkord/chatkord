//! Prints every role's colour for an accent in both schemes, so the design canvas can be kept to what
//! the code paints. `palette [#RRGGBB]`; with no accent, the default.
#![expect(clippy::print_stdout, reason = "an example's output is what it prints")]

use design::{Accent, Palette, Rgb, Role, Scheme};
use proto::rules::Colour;

fn main() -> Result<(), String> {
    let accent = match std::env::args().nth(1) {
        Some(written) => {
            let [r, g, b] = Colour::parse(&written).map_err(|_| "a colour is #RRGGBB".to_string())?.rgb();
            Accent::from(Rgb::new(r, g, b))
        }
        None => Accent::DEFAULT,
    };
    for scheme in Scheme::ALL {
        let palette = Palette::new(scheme, accent);
        println!("{scheme:?}");
        for role in Role::all() {
            let rgb = palette.color(role);
            println!("  {role:?}: #{:02X}{:02X}{:02X}", rgb.r, rgb.g, rgb.b);
        }
    }
    Ok(())
}
