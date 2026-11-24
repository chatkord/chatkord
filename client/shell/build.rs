//! On Windows, the executable carries the app icon, the window's title from the English catalog as its name, and the
//! version `terms::BUILD` carries as its version.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=assets/icon/chatkord.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let name = catalog::window_title()?;
        let (version, packed) = release()?;
        winresource::WindowsResource::new()
            .set_icon("assets/icon/chatkord.ico")
            .set("ProductName", &name)
            .set("FileDescription", &name)
            .set("FileVersion", &version)
            .set("ProductVersion", &version)
            .set_version_info(winresource::VersionInfo::FILEVERSION, packed)
            .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed)
            .compile()?;
    }
    Ok(())
}

/// The version `terms::BUILD` carries, as written (`CHATKORD_VERSION` when set, else `version.txt`), and its
/// `MAJOR.MINOR.PATCH` packed as a resource's version (16 bits each, the last zero): a pre-release suffix has no field.
#[cfg(windows)]
fn release() -> Result<(String, u64), String> {
    const RELEASE: &str = "../../version.txt";
    const STAMP: &str = "CHATKORD_VERSION";
    println!("cargo::rerun-if-env-changed={STAMP}");
    let (from, version) = match std::env::var(STAMP) {
        Ok(version) => (STAMP, version),
        Err(std::env::VarError::NotPresent) => {
            println!("cargo::rerun-if-changed={RELEASE}");
            let text = std::fs::read_to_string(RELEASE).map_err(|why| format!("{RELEASE}: {why}"))?;
            (RELEASE, text.trim_ascii().to_string())
        }
        Err(why) => return Err(format!("{STAMP}: {why}")),
    };
    let core = version.split_once('-').map_or(version.as_str(), |(core, _)| core);
    let parts: Vec<u16> =
        core.split('.').map(str::parse).collect::<Result<_, _>>().map_err(|_| format!("{from}: {version:?}"))?;
    let [major, minor, patch] = parts[..] else {
        return Err(format!("{from} holds MAJOR.MINOR.PATCH, found {version:?}"));
    };
    Ok((version, u64::from(major) << 48 | u64::from(minor) << 32 | u64::from(patch) << 16))
}

#[cfg(windows)]
mod catalog {
    use fluent_syntax::ast::{Entry, PatternElement};

    const ENGLISH: &str = "../words/catalogs/en.ftl";
    const TITLE: &str = "window-title";

    /// The `window-title` message of the English catalog, which must be plain text.
    pub(crate) fn window_title() -> Result<String, String> {
        println!("cargo::rerun-if-changed={ENGLISH}");
        let source = std::fs::read_to_string(ENGLISH).map_err(|why| format!("{ENGLISH}: {why}"))?;
        let resource =
            fluent_syntax::parser::parse(source.as_str()).map_err(|_| format!("{ENGLISH} is not a Fluent catalog"))?;
        let value = resource.body.iter().find_map(|entry| match entry {
            Entry::Message(message) if message.id.name == TITLE => message.value.as_ref(),
            _ => None,
        });
        match value.map(|pattern| pattern.elements.as_slice()) {
            Some([PatternElement::TextElement { value }]) => Ok(value.to_string()),
            _ => Err(format!("{ENGLISH} has no `{TITLE}` of plain text")),
        }
    }
}
