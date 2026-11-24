//! A channel's file read: the names it gives, each of its shape. Shared by the build script, which writes them
//! into the build, and the tests, which read both channels.

use std::collections::BTreeMap;

/// Each name the file must give, once.
pub(crate) const NAMES: [&str; 7] = [
    "CHANNEL_ID",
    "CHANNEL_PRODUCT",
    "CHANNEL_FILE",
    "CHANNEL_APP_USER_MODEL_ID",
    "CHANNEL_SETUP_ID",
    "CHANNEL_UPDATE_KEY",
    "CHANNEL_FEED",
];

/// The names `text` gives as `NAME=value` or `NAME='value'`, past comments and blank lines; every one of [`NAMES`]
/// once, no other, and each of its shape.
pub(crate) fn read(text: &str) -> Result<BTreeMap<&'static str, String>, String> {
    let mut names = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let (key, value) = line.split_once('=').ok_or_else(|| format!("not NAME=value: {line:?}"))?;
        let key = NAMES.into_iter().find(|name| *name == key).ok_or_else(|| format!("an unknown name: {key:?}"))?;
        let value = match value.strip_prefix('\'') {
            Some(quoted) => quoted.strip_suffix('\'').ok_or_else(|| format!("{key}: an unclosed quote"))?,
            None => value,
        };
        if value.is_empty() || value.contains(['\'', '"', '\\', '$', '`']) {
            return Err(format!("{key}: empty, or holds a quote, `\\`, `$` or a backtick"));
        }
        if names.insert(key, value.to_string()).is_some() {
            return Err(format!("{key} given twice"));
        }
    }
    if let Some(missing) = NAMES.into_iter().find(|name| !names.contains_key(name)) {
        return Err(format!("{missing} is not given"));
    }
    shaped(&names["CHANNEL_ID"], |b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')?;
    shaped(&names["CHANNEL_FILE"], |b| b.is_ascii_alphanumeric() || b == b'-')?;
    shaped(&names["CHANNEL_APP_USER_MODEL_ID"], |b| b.is_ascii_alphanumeric() || b == b'.')?;
    shaped(&names["CHANNEL_PRODUCT"], |b| b.is_ascii_alphanumeric() || b == b' ')?;
    Ok(names)
}

/// `value` when every byte of it is `allowed`, else why not.
fn shaped(value: &str, allowed: impl Fn(u8) -> bool) -> Result<(), String> {
    if value.bytes().all(allowed) { Ok(()) } else { Err(format!("{value:?} holds a character its name may not")) }
}
