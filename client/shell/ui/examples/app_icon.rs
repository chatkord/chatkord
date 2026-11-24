//! Writes the app icon's files a desktop reads into `client/shell/assets/icon`: a PNG of each side an icon theme
//! takes and the `.ico` the Windows executable carries, to be run after the mark or `design::app_icon` changes.
#![expect(clippy::print_stdout, reason = "an example's output is what it prints")]

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use std::fs::File;
use std::path::Path;
use ui::app_icon::{ICO_SIDES, SIDES, rgba};

fn image(side: u32) -> Result<IconImage, Box<dyn std::error::Error>> {
    Ok(IconImage::from_rgba_data(side, side, rgba(side)?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/icon");
    std::fs::create_dir_all(&dir)?;
    for side in SIDES {
        image(side)?.write_png(File::create(dir.join(format!("chatkord-{side}.png")))?)?;
    }
    let mut ico = IconDir::new(ResourceType::Icon);
    for side in ICO_SIDES {
        ico.add_entry(IconDirEntry::encode_as_png(&image(side)?)?);
    }
    ico.write(File::create(dir.join("chatkord.ico"))?)?;
    println!("wrote {}", dir.display());
    Ok(())
}
