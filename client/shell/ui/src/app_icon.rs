//! The app icon drawn as pixels: the mark's own paths on the tile `design::app_icon` describes, so the window's icon
//! and the files a desktop reads are one drawing.

use crate::Icon;
use design::Rgb;
use resvg::{tiny_skia, usvg};
use std::fmt;

/// The sides a desktop's icon theme takes (`hicolor`), each a PNG.
pub const SIDES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
/// The sides Windows' `.ico` holds, the executable's own icon.
pub const ICO_SIDES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// Why the icon could not be drawn.
#[derive(Debug, PartialEq, Eq)]
pub enum AppIconError {
    /// The mark's file is not one `<svg>` element.
    Mark,
    /// The icon's SVG did not parse.
    Svg,
    /// No picture of that side can be made.
    Side,
}

impl fmt::Display for AppIconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AppIconError::Mark => "the mark's file is not one svg element",
            AppIconError::Svg => "the app icon's svg did not parse",
            AppIconError::Side => "no picture of that side can be made",
        })
    }
}

impl std::error::Error for AppIconError {}

/// The icon `side` pixels square, as straight (not premultiplied) RGBA rows.
pub fn rgba(side: u32) -> Result<Vec<u8>, AppIconError> {
    let tree = usvg::Tree::from_str(&svg(side)?, &usvg::Options::default()).map_err(|_| AppIconError::Svg)?;
    let mut pixmap = tiny_skia::Pixmap::new(side, side).ok_or(AppIconError::Side)?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    Ok(pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let straight = pixel.demultiply();
            [straight.red(), straight.green(), straight.blue(), straight.alpha()]
        })
        .collect())
}

/// The icon as SVG in a unit square: the tile, and the mark's 24-unit box centred on it.
fn svg(side: u32) -> Result<String, AppIconError> {
    let icon = design::app_icon(side);
    let mark = std::str::from_utf8(Icon::Logo.bytes()).map_err(|_| AppIconError::Mark)?;
    let paths = mark
        .split_once("<svg")
        .and_then(|(_, open)| open.split_once('>'))
        .and_then(|(_, body)| body.rsplit_once("</svg>"))
        .map(|(paths, _)| paths)
        .ok_or(AppIconError::Mark)?;
    let inset = (1.0 - icon.mark_box) / 2.0;
    Ok(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 0 1 1"><rect width="1" height="1" rx="{corner}" fill="{tile}"/><svg x="{inset}" y="{inset}" width="{size}" height="{size}" viewBox="0 0 24 24" fill="none" stroke="{ink}" stroke-width="{stroke}" stroke-linecap="round">{paths}</svg></svg>"##,
        corner = icon.corner,
        tile = hex(icon.tile),
        size = icon.mark_box,
        ink = hex(icon.mark),
        stroke = icon.stroke,
    ))
}

fn hex(color: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}
