//! The app icon's files in `client/shell/assets/icon` are what `ui::app_icon` draws now, so a change to the mark or to
//! `design::app_icon` that was not written out (`cargo run -p ui --example app_icon`) fails here.

use ico::{IconDir, IconImage};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use ui::app_icon::{ICO_SIDES, SIDES, rgba};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/icon")
}

#[test]
fn each_png_is_the_icon_drawn_now() {
    for side in SIDES {
        let path = dir().join(format!("chatkord-{side}.png"));
        let kept = IconImage::read_png(BufReader::new(File::open(&path).unwrap())).unwrap();
        assert_eq!((kept.width(), kept.height()), (side, side), "{}", path.display());
        assert!(kept.rgba_data() == rgba(side).unwrap(), "{} is stale: run the app_icon example", path.display());
    }
}

#[test]
fn the_ico_holds_each_side_drawn_now() {
    let ico = IconDir::read(File::open(dir().join("chatkord.ico")).unwrap()).unwrap();
    let sides: Vec<u32> = ico.entries().iter().map(|entry| entry.width()).collect();
    assert_eq!(sides, ICO_SIDES);
    for entry in ico.entries() {
        let kept = entry.decode().unwrap();
        assert!(kept.rgba_data() == rgba(entry.width()).unwrap(), "the .ico's {} px is stale", entry.width());
    }
}

#[test]
fn the_tile_and_the_mark_are_in_their_colours() {
    const SIDE: usize = 256;
    const MIDDLE: usize = SIDE / 2;
    const MARK_UNITS: f32 = 24.0;
    // The mark's second bar, `M9.5 5v14`, crosses the middle row.
    const SECOND_BAR_X: f32 = 9.5;
    let icon = design::app_icon(SIDE as u32);
    let pixels = rgba(SIDE as u32).unwrap();
    let at = |x: usize, y: usize| -> [u8; 4] { pixels[(y * SIDE + x) * 4..][..4].try_into().unwrap() };
    assert_eq!(at(MIDDLE, 20), [icon.tile.r, icon.tile.g, icon.tile.b, 255], "the tile above the bars");
    assert_eq!(at(0, 0)[3], 0, "the corner is rounded off");
    let units_to_pixels = SIDE as f32 * icon.mark_box / MARK_UNITS;
    let bar = (MIDDLE as f32 + (SECOND_BAR_X - MARK_UNITS / 2.0) * units_to_pixels) as usize;
    assert_eq!(at(bar, MIDDLE), [icon.mark.r, icon.mark.g, icon.mark.b, 255], "a bar");
}
