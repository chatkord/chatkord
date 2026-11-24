//! What the debug report says of this device's choices. Every field is named, so a new choice stops the build until it
//! is a public fact or is bound to `_` as private: a device or a server's member never gets in by being forgotten.

use crate::{
    CallChat, CallView, CameraSettings, EmojiPicks, Look, Members, Remembered, ShareQuality, Startup, State,
    VoiceProcessing,
};
use design::{Contrast, MessageScale, Preference};
use report::{Fact, Full, Section, word};

const SHARE_ROWS: usize = 6;
const LOOK_ROWS: usize = 14;
const PROCESSING_ROWS: usize = 6;
const CAMERA_ROWS: usize = 4;
/// The rows of a device that offers a startup choice.
const STARTUP_ROWS: usize = 3;
/// The rows `debug_section` pushes outside the helpers, at most: two picks, two call states, three switches, two for
/// language and updates, two for emoji.
const OWN_ROWS: usize = 11;
/// The most rows the section holds; a test counts the real section against it.
const ROWS: usize = OWN_ROWS + SHARE_ROWS + LOOK_ROWS + PROCESSING_ROWS + STARTUP_ROWS + CAMERA_ROWS;
const _: () = assert!(ROWS <= report::MAX_ROWS, "the choices section must fit a report's rows");

/// The section titled "Choices" for `remembered`.
///
/// # Errors
///
/// [`Full`] when a row does not fit the section, which its fixed rows never do.
pub fn debug_section(remembered: &Remembered) -> Result<Section, Full> {
    let Remembered {
        // A device is named by what it is; the report counts devices elsewhere and never names one.
        microphone,
        speaker,
        next_call,
        share,
        look,
        event_sounds,
        message_sounds,
        popups,
        processing,
        startup,
        camera,
        language,
        auto_update,
        emoji,
    } = remembered;
    let mut rows = Section::new("Choices");
    rows.push("microphone picked", &microphone.is_some())?;
    rows.push("speaker picked", &speaker.is_some())?;
    let State { mic_muted, deafened } = next_call;
    rows.push("next call starts muted", mic_muted)?;
    rows.push("next call starts deafened", deafened)?;
    share_rows(&mut rows, share)?;
    look_rows(&mut rows, look)?;
    rows.push("event sounds", event_sounds)?;
    rows.push("message sounds", message_sounds)?;
    rows.push("desktop popups", popups)?;
    processing_rows(&mut rows, processing)?;
    match startup {
        Some(Startup { at_sign_in, starts_in, close_to }) => {
            rows.push("open at sign-in", at_sign_in)?;
            rows.push("starts in", starts_in)?;
            rows.push("closing the window", close_to)?;
        }
        None => rows.push("startup offered", &false)?,
    }
    camera_rows(&mut rows, camera)?;
    rows.push("language", &language.map_or(Fact::Absent, |language| report::Product::fact_of(language.tag())))?;
    rows.push("updates by themselves", auto_update)?;
    let (tone, recent) = EmojiPicks::facts(emoji);
    rows.push("emoji skin tone", &tone)?;
    rows.push("recent emoji", &recent)?;
    Ok(rows)
}

fn share_rows(rows: &mut Section, share: &ShareQuality) -> Result<(), Full> {
    let ShareQuality { resolution, frame_rate, ceiling, codec, sound } = share;
    rows.push("share resolution", resolution)?;
    rows.push("share frame rate", frame_rate)?;
    rows.push("share ceiling", ceiling)?;
    rows.push("share codec", codec)?;
    rows.push("share sound of a screen", &sound.screen)?;
    rows.push("share sound of a window", &sound.window)
}

fn look_rows(rows: &mut Section, look: &Look) -> Result<(), Full> {
    let Look { theme, contrast, zoom, pane, call, members, messages, call_chat, previews } = look;
    rows.push("theme", &theme_word(*theme))?;
    rows.push("contrast", &contrast_word(*contrast))?;
    rows.push("zoom percent", &zoom.percent())?;
    rows.push("server pane width", &pane.whole())?;
    let CallView { people, controls, own_share } = call;
    rows.push("call people", people)?;
    rows.push("call controls", controls)?;
    rows.push("call own share", own_share)?;
    let Members { shown, width } = members;
    rows.push("members column", shown)?;
    rows.push("members column width", &width.whole())?;
    let MessageScale { size, gap } = messages;
    rows.push("message size px", &size.px())?;
    rows.push("message gap px", &gap.px())?;
    let CallChat { shown, width } = call_chat;
    rows.push("call chat", shown)?;
    rows.push("call chat width", &width.whole())?;
    rows.push("link previews", previews)
}

fn processing_rows(rows: &mut Section, processing: &VoiceProcessing) -> Result<(), Full> {
    let VoiceProcessing { echo, suppression, automatic_gain, volume, gate, threshold } = processing;
    rows.push("echo cancellation", echo)?;
    rows.push("noise suppression", suppression)?;
    rows.push("automatic gain", automatic_gain)?;
    rows.push("input gain db", &volume.db())?;
    rows.push("noise gate", gate)?;
    rows.push("gate threshold db", &threshold.db())
}

fn camera_rows(rows: &mut Section, camera: &CameraSettings) -> Result<(), Full> {
    let CameraSettings { device, resolution, rate, mirror } = camera;
    rows.push("camera picked", &device.is_some())?;
    rows.push("camera resolution", resolution)?;
    rows.push("camera rate", rate)?;
    rows.push("camera mirrored", mirror)
}

fn theme_word(theme: Preference) -> Fact {
    Fact::Word(match theme {
        Preference::System => word!("system"),
        Preference::Light => word!("light"),
        Preference::Dark => word!("dark"),
    })
}

fn contrast_word(contrast: Contrast) -> Fact {
    Fact::Word(match contrast {
        Contrast::Standard => word!("standard"),
        Contrast::More => word!("more"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_holds_the_rows_the_constants_count() {
        let remembered = Remembered { startup: Some(Startup::default()), ..Remembered::default() };
        assert_eq!(debug_section(&remembered).unwrap().row_count(), ROWS);
    }
}
