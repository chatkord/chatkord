//! The build's channel (`CHATKORD_CHANNEL`: `stable`, the default, or `dev`) and its names, read from
//! `channels/<channel>.env`, the file the packaging scripts source too. A channel that is neither, a missing or
//! repeated name, or a name of the wrong shape stops the build.

use std::error::Error;
use std::path::Path;
use std::{env, fs};

#[path = "src/names.rs"]
mod names;

use names::read;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-env-changed=CHATKORD_CHANNEL");
    let here = env::var("CARGO_MANIFEST_DIR")?;
    println!("cargo::rerun-if-changed={here}/channels");
    let chosen = env::var("CHATKORD_CHANNEL").unwrap_or_else(|_| "stable".to_string());
    let variant = match chosen.as_str() {
        "stable" => "Stable",
        "dev" => "Dev",
        other => return Err(format!("CHATKORD_CHANNEL is stable or dev, not {other:?}").into()),
    };
    let file = Path::new(&here).join("channels").join(format!("{chosen}.env"));
    let names = read(&fs::read_to_string(&file)?).map_err(|why| format!("{}: {why}", file.display()))?;
    let name = |key: &str| names[key].as_str();
    let written = format!(
        "Identity {{ channel: Channel::{variant}, id: {:?}, product: {:?}, file: {:?}, app_user_model_id: {:?}, \
         update_key: {:?} }}",
        name("CHANNEL_ID"),
        name("CHANNEL_PRODUCT"),
        name("CHANNEL_FILE"),
        name("CHANNEL_APP_USER_MODEL_ID"),
        name("CHANNEL_UPDATE_KEY"),
    );
    fs::write(Path::new(&env::var("OUT_DIR")?).join("identity.rs"), written)?;
    Ok(())
}
