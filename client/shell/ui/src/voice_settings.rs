//! Settings › Voice and video: the microphones with how loud the one in use is and its test, what the microphone goes
//! through before it is sent, the speakers with a test sound, and the cameras with the picture asked, a preview and the
//! mirror; what each list holds and what each setting may be is `screens`' to say.

use crate::account_page::{Open, pages};
use crate::labels::{label, phrase};
use crate::picture::{Painted, Picture};
use crate::{
    Button, ListRow, Renderer, Surfaced, TextClass, Theme, coloured, flush_card, headed_card, level_bar_marked, text,
};
use choices::{
    AutomaticGain, CameraRate, CameraResolution, EchoCancellation, InputGain, Mirror, NoiseGate, Suppression,
    Threshold, VoiceProcessing, on_the_meter,
};
use design::{
    CAMERA_PREVIEW_HEIGHT, Content, DECIBEL_FIGURE, DEVICE_COLUMN_WIDTH, IconSize, Radius, Role, Space, Surface,
    TypeStyle,
};
use iced_core::{Alignment, Element, Length, Widget, alignment};
use iced_widget::{Column, Row, container};
use screens::{CameraTest, DeviceOption, VoiceProbe, WindowMessage as Message};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    fold: screens::frame::SettingsShown,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let remembered = window.voice();
    let probe = window.voice_probe();
    let microphones = probe.map(|probe| probe.microphones(remembered, words)).unwrap_or_default();
    let speakers = probe.map(|probe| probe.speakers(remembered, words)).unwrap_or_default();

    let microphone = crate::settings_section(
        phrase(Phrase::Microphone, words),
        None,
        headed_card(
            phrase(Phrase::Microphone, words),
            meter(probe, remembered.processing, words),
            microphones.iter().map(|each| option(each, Message::PickMicrophone)).collect(),
        ),
    );
    let play = Button::new(phrase(Phrase::PlayTestSound, words)).compact();
    let play = if probe.is_some_and(VoiceProbe::can_play) { play.on_press(Message::PlayTestSound) } else { play };
    let speaker = crate::settings_section(
        phrase(Phrase::Speaker, words),
        Some(play.view::<R>()),
        flush_card(
            phrase(Phrase::Speaker, words),
            speakers.iter().map(|each| option(each, Message::PickSpeaker)).collect(),
        ),
    );
    let processing = processing(remembered.processing, probe, words);
    let camera = camera(window, probe, picture, words);
    let cards = container(Column::with_children([microphone, processing, speaker, camera]).spacing(Space::L.px()))
        .width(Length::Fill.max(DEVICE_COLUMN_WIDTH));

    let close = Message::CloseDevice;
    let shown = probe.map(VoiceProbe::notices).unwrap_or_default();
    let cards = crate::callouts_above(&shown, DEVICE_COLUMN_WIDTH, Message::DismissProbeNotice, cards._boxed(), words);
    let frame = crate::settings_frame(
        &phrase(Phrase::SettingsLabel, words),
        pages(window, Open::VoiceAndVideo, words),
        phrase(Phrase::VoiceAndVideo, words),
        None,
        cards,
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    crate::on_escape(frame, close, None)._boxed()
}

/// How loud the microphone is, the gate's threshold marked on it when the gate is on, with the test's button and what
/// to do to see it; or why there is nothing to see.
fn meter<'a, R: Renderer + 'a>(probe: Option<&VoiceProbe>, kept: VoiceProcessing, words: &'a Words) -> Part<'a, R> {
    if let Some(why) = probe.and_then(VoiceProbe::failed) {
        return crate::refusal(words.say(&words::failure::of(why)));
    }
    let level = probe.map_or(0, VoiceProbe::level);
    let gating = kept.gating();
    let testing = probe.is_some_and(VoiceProbe::is_testing);
    let test = Button::new(phrase(if testing { Phrase::StopTest } else { Phrase::TestVoice }, words)).compact();
    let test = if probe.is_some_and(VoiceProbe::can_play) { test.on_press(Message::TestMicrophone) } else { test };
    let mut row = vec![
        container(level_bar_marked::<Message, R>(level, gating.map(|at| on_the_meter(i16::from(at.db())))))
            .width(Length::Fill)
            .center_y(Length::Shrink)
            ._boxed(),
    ];
    if let Some(at) = gating {
        let said = words.say(&Sentence::Decibels(at.db()));
        row.push(coloured(said, TypeStyle::Mono, TextClass::Content(Content::Secondary))._boxed());
    }
    row.push(test.view::<R>());
    let hint = if testing { Phrase::TestingVoice } else { Phrase::TestMicrophone };
    Column::with_children([
        Row::with_children(row).spacing(Space::S.px()).align_y(Alignment::Center)._boxed(),
        coloured(words.phrase(hint).to_string(), TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xs.px())
    ._boxed()
}

/// The cameras, each by name and the one in use ticked, the picture and rate asked of it, the preview with its test,
/// the mirror switch and the one that starts what is watched paused.
fn camera<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    probe: Option<&'a VoiceProbe>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let kept = &window.voice().camera;
    let cameras = probe.map(|probe| probe.cameras(kept, words)).unwrap_or_default();
    let mut rows: Vec<Part<'a, R>> = cameras.iter().map(|each| option(each, Message::PickCamera)).collect();
    let setting = |row: Part<'a, R>| container(row).padding(crate::ROW_PADDING).width(Length::Fill)._boxed();
    let resolutions =
        CameraResolution::ALL.map(|each| (label(&words.camera_resolution(each)), Message::PickCameraResolution(each)));
    let chosen = CameraResolution::ALL.iter().position(|each| *each == kept.resolution).unwrap_or(0);
    rows.push(setting(crate::choice(phrase(Phrase::CameraResolution, words), resolutions.into(), chosen, false)));
    let offered = probe.map_or_else(|| vec![CameraRate::Thirty, CameraRate::Fifteen], |probe| probe.camera_rates(kept));
    let shown = probe.map_or(kept.rate, |probe| probe.camera_rate(kept));
    let chosen = offered.iter().position(|each| *each == shown).unwrap_or(0);
    let rates: Vec<_> =
        offered.into_iter().map(|each| (label(&words.camera_rate(each)), Message::PickCameraRate(each))).collect();
    rows.push(setting(crate::choice(phrase(Phrase::CameraFrameRate, words), rates, chosen, false)));
    rows.push(setting(preview(window, probe, picture, words)));
    rows.push(setting(crate::switch(
        &phrase(Phrase::MirrorCamera, words),
        None,
        kept.mirror == Mirror::On,
        Message::ToggleMirror,
    )));
    crate::settings_section(phrase(Phrase::Camera, words), None, flush_card(phrase(Phrase::Camera, words), rows))
}

/// The camera's picture at 16:9: the call's while a call sends it, else the test's, its start or why it stopped; with
/// the test's button, which a call sending the camera leaves out, since one camera is opened once.
fn preview<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    probe: Option<&'a VoiceProbe>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let live = window.camera_live();
    let test = probe.map_or(&CameraTest::Idle, VoiceProbe::camera_test);
    let shown = picture(Painted::OwnCamera(window.voice().camera.mirror, Radius::M));
    let failed = probe.and_then(VoiceProbe::cameras_failed).or(match test {
        CameraTest::Failed(why) => Some(why),
        CameraTest::Idle | CameraTest::Starting | CameraTest::On => None,
    });
    let inside: Part<'a, R> = match (live, test, failed) {
        (true, ..) | (false, CameraTest::On, _) => shown,
        (false, _, Some(why)) => Column::with_children([
            crate::icon(crate::Icon::VideoOff, IconSize::L, crate::IconClass::Role(Role::Content(Content::Secondary)))
                ._boxed(),
            coloured(words.say(&words::failure::of(why)), TypeStyle::BodyStrong, TextClass::Content(Content::Primary))
                .align_x(alignment::Horizontal::Center)
                ._boxed(),
        ])
        .spacing(Space::S.px())
        .align_x(Alignment::Center)
        ._boxed(),
        (false, CameraTest::Starting, None) => Column::with_children([
            crate::spinner(design::Role::Accent(design::AccentRole::Text)),
            text(words.phrase(Phrase::StartingCamera).to_string(), TypeStyle::Small)._boxed(),
        ])
        .spacing(Space::S.px())
        .align_x(Alignment::Center)
        ._boxed(),
        (false, CameraTest::Idle | CameraTest::Failed(_), None) => iced_widget::Space::new()._boxed(),
    };
    let area = container(inside)
        .width(Length::Fill)
        .height(Length::Fixed(CAMERA_PREVIEW_HEIGHT))
        .center_x(Length::Fill)
        .center_y(Length::Fixed(CAMERA_PREVIEW_HEIGHT))
        .clip(true)
        .class(Surfaced::Fill {
            fill: design::Role::Surface(Surface::Inset),
            text: design::Role::Content(Content::Primary),
            radius: Radius::M,
        });
    let mut column = vec![area._boxed()];
    if !live {
        let held = test.held();
        let button = Button::new(phrase(if held { Phrase::StopTest } else { Phrase::TestCamera }, words)).compact();
        let button = if probe.is_some() { button.on_press(Message::TestCamera) } else { button };
        column.push(button.view::<R>());
    }
    Column::with_children(column).spacing(Space::M.px())._boxed()
}

/// What the microphone goes through before it is sent: echo cancellation, the noise suppressor, automatic gain or a
/// volume of one's own, and the noise gate with its threshold.
fn processing<'a, R: Renderer + 'a>(
    kept: VoiceProcessing,
    probe: Option<&VoiceProbe>,
    words: &'a Words,
) -> Part<'a, R> {
    let set = Message::SetProcessing;
    let echo_on = kept.echo == EchoCancellation::On;
    let echo = crate::switch(
        &phrase(Phrase::EchoCancellation, words),
        Some(&words.phrase(Phrase::EchoCancellationHint)),
        echo_on,
        set(VoiceProcessing { echo: if echo_on { EchoCancellation::Off } else { EchoCancellation::On }, ..kept }),
    );
    let kinds = [
        (Suppression::Off, Phrase::SuppressionOff),
        (Suppression::Standard, Phrase::SuppressionStandard),
        (Suppression::Rnnoise, Phrase::SuppressionRnnoise),
    ];
    let chosen = kinds.iter().position(|(kind, _)| *kind == kept.suppression).unwrap_or(0);
    let options = kinds
        .iter()
        .map(|(kind, named)| (phrase(*named, words), set(VoiceProcessing { suppression: *kind, ..kept })))
        .collect();
    let mut noise = vec![crate::choice(phrase(Phrase::NoiseSuppression, words), options, chosen, false)];
    if kept.suppression == Suppression::Rnnoise {
        noise.push(hint(words.phrase(Phrase::SuppressionRnnoiseHint).to_string()));
    }
    let gain_on = kept.automatic_gain == AutomaticGain::On;
    let gain = crate::switch(
        &phrase(Phrase::AutomaticGain, words),
        Some(&words.phrase(Phrase::AutomaticGainHint)),
        gain_on,
        set(VoiceProcessing { automatic_gain: if gain_on { AutomaticGain::Off } else { AutomaticGain::On }, ..kept }),
    );
    let volume = volume(kept, words);
    let gate_on = kept.gate == NoiseGate::On;
    let mut gate = vec![crate::switch(
        &phrase(Phrase::NoiseGate, words),
        Some(&words.phrase(Phrase::NoiseGateHint)),
        gate_on,
        set(VoiceProcessing { gate: if gate_on { NoiseGate::Off } else { NoiseGate::On }, ..kept }),
    )];
    if gate_on {
        gate.push(threshold(kept, words));
        let room = Button::new(phrase(Phrase::SetFromRoom, words)).compact();
        let room = if probe.and_then(VoiceProbe::room).is_some() { room.on_press(Message::GateFromRoom) } else { room };
        gate.push(container(room.view::<R>()).width(Length::Fill)._boxed());
    }
    crate::settings_section(
        phrase(Phrase::VoiceProcessing, words),
        None,
        crate::list_card(
            phrase(Phrase::VoiceProcessing, words),
            vec![
                echo,
                Column::with_children(noise).spacing(Space::Xs.px())._boxed(),
                gain,
                volume,
                Column::with_children(gate).spacing(Space::S.px())._boxed(),
            ],
            None,
        ),
    )
}

/// The volume of one's own, offered only while automatic gain is off.
fn volume<'a, R: Renderer + 'a>(kept: VoiceProcessing, words: &'a Words) -> Part<'a, R> {
    let least = *InputGain::RANGE.start();
    let at = |db: i8| u16::try_from(i16::from(db) - i16::from(least)).unwrap_or(0);
    let from = move |step: u16| InputGain::new(i8::try_from(i32::from(step) + i32::from(least)).unwrap_or(0));
    let span = 0..=at(*InputGain::RANGE.end());
    let name = phrase(Phrase::MicrophoneVolume, words);
    let rail = match kept.automatic_gain {
        AutomaticGain::On => crate::slider_unoffered(name.clone(), span, at(kept.volume.db())),
        AutomaticGain::Off => {
            let moved = move |step| VoiceProcessing { volume: from(step).unwrap_or(kept.volume), ..kept };
            crate::slider(
                name.clone(),
                span,
                at(kept.volume.db()),
                move |step| Message::TuneProcessing(moved(step)),
                move |step| Message::SetProcessing(moved(step)),
            )
        }
    };
    let mut lines =
        vec![text(name.as_str().to_string(), TypeStyle::BodyStrong)._boxed(), figured(rail, kept.volume.db(), words)];
    if kept.automatic_gain == AutomaticGain::On {
        lines.push(hint(words.phrase(Phrase::MicrophoneVolumeHint).to_string()));
    }
    Column::with_children(lines).spacing(Space::Xs.px())._boxed()
}

/// The gate's threshold, while the gate is on.
fn threshold<'a, R: Renderer + 'a>(kept: VoiceProcessing, words: &'a Words) -> Part<'a, R> {
    let least = *Threshold::RANGE.start();
    let at = |db: i8| u16::try_from(i16::from(db) - i16::from(least)).unwrap_or(0);
    let from = move |step: u16| Threshold::new(i8::try_from(i32::from(step) + i32::from(least)).unwrap_or(0));
    let moved = move |step| VoiceProcessing { threshold: from(step).unwrap_or(kept.threshold), ..kept };
    let name = phrase(Phrase::GateThreshold, words);
    let rail = crate::slider(
        name.clone(),
        0..=at(*Threshold::RANGE.end()),
        at(kept.threshold.db()),
        move |step| Message::TuneProcessing(moved(step)),
        move |step| Message::SetProcessing(moved(step)),
    );
    Column::with_children([
        text(name.as_str().to_string(), TypeStyle::Label)._boxed(),
        figured(rail, kept.threshold.db(), words),
    ])
    .spacing(Space::Xs.px())
    ._boxed()
}

/// A slider with its value in dB at its right.
fn figured<'a, R: Renderer + 'a>(rail: Part<'a, R>, db: i8, words: &'a Words) -> Part<'a, R> {
    Row::with_children([
        rail,
        container(text(words.say(&Sentence::Decibels(db)), TypeStyle::Mono))
            .align_right(Length::Fixed(DECIBEL_FIGURE))
            ._boxed(),
    ])
    .spacing(Space::S.px() + Space::Xxs.px())
    .align_y(Alignment::Center)
    ._boxed()
}

fn hint<'a, R: Renderer + 'a>(said: String) -> Part<'a, R> {
    coloured(said, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed()
}

/// A device in a list: its mark, its name (wrapped, for a real device's is long), the note beside it, and, unless it is
/// a saved device that is not here, what picking it sends.
fn option<'a, R: Renderer + 'a>(each: &DeviceOption, pick: fn(Option<choices::Picked>) -> Message) -> Part<'a, R> {
    let mut row = ListRow::<Message, R>::new(label(&each.name)).radio(each.ticked()).wrapped();
    if let Some(note) = &each.note {
        row = row.noted(note);
    }
    if !each.away() {
        row = row.on_press(pick(each.asked.clone()));
    }
    row.view()
}
