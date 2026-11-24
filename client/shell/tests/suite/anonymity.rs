//! The client's debug report holds nothing private. Every private value a window can hold is a sentinel here (a
//! server's address and name, an account's names, a member's id and display name, a channel's name, a message, a token,
//! a device's id and name, a monitor's name, a path and a user name in the system's and the adapter's own words), the
//! report is made by the window's own collectors from a window that holds them, and no sentinel appears in it raw,
//! percent-encoded, in base64 or in hex.

use choices::{Audio, CameraSettings, Picked, Remembered, Volume, Volumes};
use proto::rules::Grant;
use screens::harness::{self, Clock};
use screens::{
    Ask, CallMessage, Flow, Health, Listed, ListedCamera, Machine, MeasuredOn, Opening, PlaceMessage, ProbeHeard,
    ReportMessage, RoundTrip, Sound, Source, SourceKey, SourceKind, Window, WindowMessage,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tracing_subscriber::layer::SubscriberExt;

const HOST: &str = "sentinel-host-0a1b.example.invalid";
const SERVER_NAME: &str = "SentinelServerName";
const IDENTITY: &str = "SentinelIdentityName";
const MEMBER_ID: &str = "sentinelmember01";
const DISPLAY_NAME: &str = "SentinelDisplayName";
const TEXT_CHANNEL: &str = "SentinelTextChannel";
const VOICE_CHANNEL: &str = "SentinelVoiceChannel";
const MESSAGE: &str = "SentinelMessageBody";
const TOKEN: &str = "hunter2-secret";
const MICROPHONE_ID: &str = "SentinelMicrophoneId";
const MICROPHONE_NAME: &str = "SentinelMicrophoneName";
const SPEAKER_ID: &str = "SentinelSpeakerId";
const SPEAKER_NAME: &str = "SentinelSpeakerName";
const CAMERA_ID: &str = "SentinelCameraId";
const CAMERA_NAME: &str = "SentinelCameraName";
const MONITOR_KEY: &str = "SentinelMonitorKey";
const MONITOR_NAME: &str = "SentinelMonitorName";
/// A folder the system might state, which a product name may not be.
const FOLDER: &str = "/srv/sentinelhome/sentinel-dir";
/// A user and a host as one text, which a product name may not be.
const OWNER: &str = "sentinel-user:sentinel-host";
/// An adapter name a driver made of a path.
const ADAPTER_PATH: &str = "/srv/sentinelhome/.cache/sentinel-adapter";

/// Every private value above, as the window is given it.
const SENTINELS: [&str; 20] = [
    HOST,
    SERVER_NAME,
    IDENTITY,
    MEMBER_ID,
    DISPLAY_NAME,
    TEXT_CHANNEL,
    VOICE_CHANNEL,
    MESSAGE,
    TOKEN,
    MICROPHONE_ID,
    MICROPHONE_NAME,
    SPEAKER_ID,
    SPEAKER_NAME,
    CAMERA_ID,
    CAMERA_NAME,
    MONITOR_KEY,
    MONITOR_NAME,
    FOLDER,
    OWNER,
    ADAPTER_PATH,
];

fn percent(text: &str) -> String {
    text.bytes().map(|byte| format!("%{byte:02X}")).collect()
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// `bytes` in base64, the standard alphabet or the URL-safe one, without padding.
fn base64(bytes: &[u8], url_safe: bool) -> String {
    let alphabet: &[u8; 64] = if url_safe {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    };
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |bits, (at, byte)| bits | u32::from(*byte) << (16 - 8 * at));
        for at in 0..=chunk.len() {
            out.push(char::from(alphabet[(bits >> (18 - 6 * at) & 63) as usize]));
        }
    }
    out
}

/// Every way a sentinel can show in a text: as it is, percent-encoded, in hex, and in base64 at each of the three
/// alignments it can sit at within a longer text.
fn forms(sentinel: &str) -> Vec<String> {
    let mut forms = vec![sentinel.to_owned(), percent(sentinel), hex(sentinel), hex(sentinel).to_uppercase()];
    for url_safe in [false, true] {
        for lead in 0..3usize {
            let mut bytes = vec![b'x'; lead];
            bytes.extend_from_slice(sentinel.as_bytes());
            let encoded = base64(&bytes, url_safe);
            // The characters holding the lead's bits are mixed with the sentinel's; the last two may be partial.
            let from = (lead * 8).div_ceil(6);
            forms.push(encoded.get(from..encoded.len() - 2).expect("a sentinel is longer than that").to_owned());
        }
    }
    forms
}

fn assert_holds_none(text: &str, sentinels: &[&str]) {
    for sentinel in sentinels {
        for form in forms(sentinel) {
            assert!(!text.contains(&form), "the report holds {sentinel:?} as {form:?}:\n{text}");
        }
    }
}

/// The run's own user, home and host, which a test on a person's machine can read; a short one would match by chance.
fn this_machine() -> Vec<String> {
    ["USERNAME", "USER", "USERPROFILE", "HOME", "LOGNAME", "COMPUTERNAME", "HOSTNAME"]
        .into_iter()
        .filter_map(|variable| std::env::var(variable).ok())
        .filter(|value| value.len() >= 5)
        .collect()
}

/// The report without its rows' names: a machine named by a common word ("desktop") matches a fixed name, which tells
/// nothing about the machine.
fn values(text: &str) -> String {
    text.lines()
        .map(|line| match line.trim_start().split_once(": ") {
            Some((name, value)) if name.chars().all(|c| c.is_ascii_lowercase() || c == ' ') => value,
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn adapter() -> iced_wgpu::wgpu::AdapterInfo {
    use iced_wgpu::wgpu::{AdapterInfo, Backend, DeviceType};
    let mut info = AdapterInfo::new(DeviceType::DiscreteGpu, Backend::Vulkan);
    info.name = ADAPTER_PATH.into();
    info.driver = "radv".into();
    info.driver_info = OWNER.into();
    info
}

/// The earlier run's log, as a file in the logs folder: notes of the shape the file layer writes, a crash note, and
/// every private value written into a line in a way a note could not have written it, and plain words in a note's own
/// shape. What the window is given is what the platform reads back from it.
///
/// The read-back admits a line by its shape and character set, not by who wrote it: a two-word name in a field and an
/// id-like token are accepted as shape (a file edited by hand can add them), while a path, an address next to a letter,
/// punctuation-bearing text and a half-written last line are refused.
struct EarlierRun {
    previous: report::Previous,
    /// The lines the read-back must refuse.
    refused: usize,
    /// The lines of a note's shape that are accepted whatever wrote them.
    by_shape: [&'static str; 2],
}

fn earlier_run() -> EarlierRun {
    let time = "2026-11-17T10:42:02.123456Z";
    let note = format!("{time}  INFO shell::window: a share started report.note=true frames=30 hardware=true");
    let by_shape = [
        format!("{time}  INFO shell::window: a share started report.note=true who=Ada Lovelace"),
        format!("{time}  INFO shell::window: a share started report.note=true id=member01"),
    ];
    let crash = format!(
        "{time} ERROR shell::logging: a thread panicked report.note=true file=window folder=src line=12 column=5"
    );
    let half = format!("{time}  INFO shell::window: a share started report.note=true frames=3");
    let tampered = [
        format!("{time}  INFO shell::window: a share started report.note=true peer=x203.0.113.7"),
        format!("{time}  INFO shell::window: a share started report.note=true peer=router 198.51.100.10"),
        format!("{time}  INFO shell::window: a share started report.note=true peer=203.0.113.7:8080"),
        format!("{time}  INFO shell::window: a share started report.note=true mail=ada@example.invalid"),
        format!("{time}  INFO shell::window: a share started report.note=true link=https://example.invalid/x"),
        format!("{time}  INFO shell::window: reached 2001:db8:0:0:0:0:0:1 report.note=true"),
        format!("{time}  INFO shell::window: card a4:83:e7:12:34:56 report.note=true"),
        format!("{time}  INFO shell::window: a share started report.note=true peer=2001:db8:0:0:0:0:0:1"),
        format!("{time}  INFO shell::window: a share started report.note=true card=a4:83:e7:12:34:56"),
        format!("{time}  INFO shell::window: opened {FOLDER} report.note=true"),
        format!("{time}  INFO shell::window: a share started report.note=true place={FOLDER}"),
        format!("{time}  INFO shell::window: a share started report.note=true host={HOST}"),
        format!("{time}  INFO shell::window: a share started report.note=true name=\"{DISPLAY_NAME}\""),
        format!("{time}  INFO shell::window: a share started report.note=true owner={OWNER}"),
        format!("{DISPLAY_NAME} said {MESSAGE} on {HOST}"),
    ];
    let folder = std::env::temp_dir().join(format!("chatkord-anonymity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let mut lines = vec![note];
    lines.extend(by_shape);
    lines.extend(tampered.iter().cloned());
    lines.push(crash);
    let text = lines.join("\n") + "\n" + &half;
    std::fs::write(folder.join("00000000000000000001-1.log"), text).unwrap();
    let read = platform::logs::read_previous(&folder).expect("the earlier run's log");
    std::fs::remove_dir_all(&folder).unwrap();
    EarlierRun {
        previous: read,
        refused: tampered.len() + 1,
        by_shape: ["info  shell a share started  who=Ada Lovelace", "info  shell a share started  id=member01"],
    }
}

/// A window holding every private value, the report asked for in it, and the report's text.
fn report_of_a_full_window() -> (Window, String) {
    report_with(Some(earlier_run().previous))
}

/// [`report_of_a_full_window`] with `previous` as the earlier run's notes.
fn report_with(previous: Option<report::Previous>) -> (Window, String) {
    let ring = report::Ring::new(std::time::SystemTime::now);
    let subscriber = tracing_subscriber::registry().with(ring.layer());
    tracing::subscriber::with_default(subscriber, || {
        let clock = Clock(Instant::now());
        let server = harness::server(HOST, SERVER_NAME);
        let (window, _) = Window::launch(vec![Opening::Account(server, IDENTITY.into())], None, clock.0);
        let mut volumes = Volumes::default();
        volumes.set(&harness::member(MEMBER_ID), Audio::Voice, Volume::SILENT);
        let picked = |id: &str, label: &str| Some(Picked { id: id.into(), label: label.into() });
        let remembered = Remembered {
            microphone: picked(MICROPHONE_ID, MICROPHONE_NAME),
            speaker: picked(SPEAKER_ID, SPEAKER_NAME),
            camera: CameraSettings { device: picked(CAMERA_ID, CAMERA_NAME), ..CameraSettings::default() },
            ..Remembered::default()
        };
        let mut window = window.remembering(remembered, BTreeMap::from([(format!("wss://{HOST}/v1"), volumes)]));
        let place = harness::id(&window, 0);
        let said = |window: &mut Window, message| harness::tell(window, &clock, place, message);
        said(&mut window, harness::flow(screens::Report::TokenLoaded(Some(terms::Secret::new(TOKEN.into())))));
        let ada = screens::Report::Accepted { token: None, named: None, standing: harness::standing_as(MEMBER_ID) };
        said(&mut window, harness::flow(ada));
        let grants = [Grant::ChannelRead, Grant::SendMessage];
        harness::standing(&mut window, &clock, place, &grants);
        let mut channels = harness::channels_holding(grants.iter().copied().collect());
        channels.text[0].name = TEXT_CHANNEL.into();
        channels.voice[0].name = VOICE_CHANNEL.into();
        said(&mut window, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(channels))));
        said(&mut window, harness::discussion(screens::DiscussionMessage::Open(harness::channel_id(harness::GENERAL))));
        let page = chat::Page {
            messages: vec![harness::said(1, 5, MEMBER_ID, MESSAGE)],
            older: false,
            newer: false,
            revision: 5,
        };
        said(&mut window, harness::first_page(page));

        // The devices the host listed, and the monitors its picker did.
        let listed = |id: &str, label: &str| Listed { asked: Some(id.into()), label: label.into() };
        let default = |label: &str| Listed { asked: None, label: label.into() };
        let sound = Sound {
            microphones: vec![default(MICROPHONE_NAME), listed(MICROPHONE_ID, MICROPHONE_NAME)],
            speakers: vec![default(SPEAKER_NAME), listed(SPEAKER_ID, SPEAKER_NAME)],
            microphone: None,
            speaker: None,
        };
        window.update(WindowMessage::Probed(ProbeHeard::Sound(sound)), clock.at());
        let cameras = vec![ListedCamera { camera: listed(CAMERA_ID, CAMERA_NAME), sixty_at: Vec::new() }];
        window.update(WindowMessage::Probed(ProbeHeard::Cameras(Ok(cameras))), clock.at());
        let encoder = choices::EncoderOption { codec: choices::Codec::H264, on: choices::EncodesOn::GraphicsCard };
        window.update(WindowMessage::Encoders(vec![encoder]), clock.at());

        // A call with the display name in it, looked at three times, and over.
        harness::in_call(&mut window, &clock, place);
        let joined = screens::RoomEvent::Joined {
            members: vec![screens::Person::new(harness::member(MEMBER_ID), DISPLAY_NAME)],
            streams: Vec::new(),
        };
        said(&mut window, PlaceMessage::Call(CallMessage::Room(joined)));
        let source = Source {
            key: SourceKey(MONITOR_KEY.into()),
            kind: SourceKind::Screen,
            title: Some(MONITOR_NAME.into()),
            size: Some((1920, 1080)),
            icon: None,
            still: None,
        };
        said(&mut window, PlaceMessage::Call(CallMessage::Sources(vec![source])));
        for (millis, lost, bits) in [(20, 0, 1_000), (40, 3, 5_000), (30, 5, 3_000)] {
            let took = Duration::from_millis(millis);
            let health = Health {
                round_trip: Some(RoundTrip { took, on: MeasuredOn::Route }),
                lost,
                sending: Flow { packets: 0, bits_per_second: bits },
                ..Health::default()
            };
            said(&mut window, PlaceMessage::Call(CallMessage::Health(health)));
        }
        said(&mut window, PlaceMessage::Call(CallMessage::CallOver(None)));

        // The machine, as the host states it: a path or an owner is withheld by the check every product name passes.
        let mut system = report::Section::new("System");
        system.push("system", &report::Product::fact_of(FOLDER)).unwrap();
        system.push("version", &report::Product::fact_of(OWNER)).unwrap();
        let graphics = shell::window::graphics(&adapter(), true);
        window.update(WindowMessage::Machine(Machine { system, graphics }), clock.at());

        report::note!(warn, "an adapter was withheld", gpu = report::Product::fact_of(ADAPTER_PATH));
        let asked = window.update(WindowMessage::Report(ReportMessage::Open), clock.at());
        assert!(matches!(&asked[..], [Ask::ReadLog, Ask::ReadNotices]), "{asked:?}");
        window.update(WindowMessage::NoticesRead(None), clock.at());
        window.update(
            WindowMessage::Report(ReportMessage::Read(screens::ReportLogs { recent: ring.recent(), previous })),
            clock.at(),
        );
        let text = window.report_shown().expect("the report is open").text().into_owned();
        (window, text)
    })
}

#[test]
fn no_private_value_of_a_full_window_reaches_its_client_report() {
    let (_, text) = report_of_a_full_window();
    assert_holds_none(&text, &SENTINELS);
    let machine = this_machine();
    assert_holds_none(&values(&text), &machine.iter().map(String::as_str).collect::<Vec<_>>());
    for kept in [
        "Chatkord debug report: client",
        "Choices",
        "  microphones: 1",
        "  speakers: 1",
        "  cameras: 1",
        "  monitors: 1",
        "  server: server-1",
        "  state: Online",
        "  call on: server-1",
        "  H.264 encoder: GraphicsCard",
        "  adapter: withheld",
        "  system: withheld",
        "an adapter was withheld",
        "gpu=withheld",
    ] {
        assert!(text.contains(kept), "the collectors left out {kept:?}:\n{text}");
    }
    assert!(text.len() <= report::MAX_REPORT_BYTES);
}

#[test]
fn an_earlier_runs_log_adds_only_the_notes_it_holds_and_counts_the_rest() {
    let EarlierRun { previous, refused: tampered, by_shape } = earlier_run();
    assert_eq!(previous.refused(), tampered, "every line changed, foreign or half written is refused whole");
    let (_, text) = report_with(Some(previous));
    assert_holds_none(&text, &SENTINELS);
    for private in ["203.0.113.7", "198.51.100.10", "ada@", "https:", "x203", "2001:db8", "a4:83"] {
        assert!(!text.contains(private), "the earlier run's section holds {private:?}:\n{text}");
    }
    let refused = format!("  lines refused: {tampered}\n");
    for kept in [
        "\nPrevious run, newest last\n",
        refused.as_str(),
        "info  shell a share started  frames=30  hardware=true",
        "error shell a thread panicked  file=window  folder=src  line=12  column=5",
    ]
    .into_iter()
    .chain(by_shape)
    {
        assert!(text.contains(kept), "the earlier run's section left out {kept:?}:\n{text}");
    }
}

#[test]
fn a_report_without_an_earlier_run_has_no_section_for_it() {
    let folder = std::env::temp_dir().join(format!("chatkord-anonymity-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let previous = platform::logs::read_previous(&folder);
    std::fs::remove_dir_all(&folder).unwrap();
    assert!(previous.is_none());
    let (_, text) = report_with(previous);
    assert!(!text.contains("Previous run"), "{text}");
}

#[test]
fn every_field_the_wire_marks_secret_is_absent_from_the_report() {
    let (_, text) = report_of_a_full_window();
    let marked = proto::filled::marked_secrets();
    assert!(!marked.is_empty(), "the schema marks no secret");
    for field in &marked {
        assert!(!text.contains(field.as_str()), "{field} is in the report:\n{text}");
    }
    assert_holds_none(&text, &[TOKEN]);
}

#[test]
fn the_listing_and_the_saved_file_are_one_text() {
    let (mut window, text) = report_of_a_full_window();
    let clock = Clock(Instant::now());
    assert_eq!(window.report_shown().map(|shown| shown.text().into_owned()), Some(text.clone()));
    let asked = window.update(WindowMessage::Report(ReportMessage::Download), clock.at());
    let [Ask::SaveReport(download)] = &asked[..] else { panic!("{asked:?}") };
    assert_eq!(download.bytes(), text.as_bytes(), "the file holds the text shown");
    assert!(
        download.name.starts_with("chatkord-client-report-") && download.name.ends_with(".txt"),
        "{}",
        download.name
    );
    let saved = screens::Saved::Written(download.name.clone());
    window.update(WindowMessage::Report(ReportMessage::Saved(saved)), clock.at());
    assert!(window.report_shown().is_none(), "saved, the dialog closes");
    assert_eq!(window.downloaded(), Some(download.name.as_str()));
}

#[test]
fn the_dialog_lists_a_logs_newest_lines_only_and_the_file_holds_them_all() {
    const NOTES: u32 = 300;
    let ring = report::Ring::new(std::time::SystemTime::now);
    let subscriber = tracing_subscriber::registry().with(ring.layer());
    let (mut window, clock) = tracing::subscriber::with_default(subscriber, || {
        let clock = Clock(Instant::now());
        let server = harness::server(HOST, SERVER_NAME);
        let (mut window, _) = Window::launch(vec![Opening::Account(server, IDENTITY.into())], None, clock.0);
        for frames in 0..NOTES {
            report::note!(info, "a share started", frames = frames);
        }
        window.update(WindowMessage::Report(ReportMessage::Open), clock.at());
        window.update(WindowMessage::NoticesRead(None), clock.at());
        let logs = screens::ReportLogs { recent: ring.recent(), previous: None };
        window.update(WindowMessage::Report(ReportMessage::Read(logs)), clock.at());
        (window, clock)
    });
    let shown = window.report_shown().expect("the report is open");
    let held = ring.recent().lines().len();
    assert!(held >= NOTES as usize, "{held}");
    let counted = |text: &str| text.lines().filter(|line| line.contains("a share started")).count();
    assert_eq!(counted(&shown.text()), NOTES as usize, "the file holds every note");
    let listed = shown.listing().text(|sentence| format!("{sentence:?}"));
    assert_eq!(counted(&listed), screens::LISTED_LOG_LINES, "the newest lines are all this test's notes");
    assert!(
        listed.contains(&format!("ReportLogCut {{ shown: {}, total: {held} }}", screens::LISTED_LOG_LINES)),
        "{listed}"
    );
    assert!(listed.ends_with(&format!("frames={}\n", NOTES - 1)) && !listed.contains("frames=0\n"), "{listed}");
    assert!(listed.starts_with("Chatkord debug report: client\n"), "{listed}");
    let whole = shown.text().into_owned();
    let asked = window.update(WindowMessage::Report(ReportMessage::Download), clock.at());
    let [Ask::SaveReport(download)] = &asked[..] else { panic!("{asked:?}") };
    assert_eq!(download.bytes(), whole.as_bytes(), "the file holds the whole text, not the listing");
}

#[test]
fn an_adapter_named_by_a_path_is_withheld_and_a_product_name_is_stated() {
    let named = |name: &str| {
        let mut info = adapter();
        info.name = name.into();
        let build = report::Build { program: "Chatkord", version: "0", source: "test" };
        let mut report = report::Report::new(report::Side::Client, build, 0);
        report.section(shell::window::graphics(&info, false)).unwrap();
        report.text()
    };
    assert!(named(ADAPTER_PATH).contains("adapter: withheld"));
    assert_holds_none(&named(ADAPTER_PATH), &[ADAPTER_PATH, OWNER]);
    let stated = named("AMD Radeon RX 9060 XT");
    assert!(stated.contains("adapter: AMD Radeon RX 9060 XT"), "{stated}");
    assert!(stated.contains("backend: Vulkan"), "{stated}");
}
