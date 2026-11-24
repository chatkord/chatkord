//! The report's sections, made from what the window holds. Every value is a [`report::Fact`]: a count, a word written
//! here, a product name that passed its check, or a stand-in for an address.

use super::{Counted, Finished, Listing, Machine};
use crate::RailState;
use crate::call::health::{CallFigures, KEPT_CALLS, Spread};
use crate::window::Stated;
use choices::{Codec, EncoderOption, Remembered};
use report::{Build, Fact, Full, Product, Public, Report, Section, Side, Stands, millis, word};

/// The most servers the Servers section lists, two rows each.
const MAX_SERVERS: usize = 24;
const _: () = assert!(2 + 2 * MAX_SERVERS <= report::MAX_ROWS, "the servers and their count must fit a section's rows");

/// The rows a call takes: which server, its length, three spreads of three figures each, and its stalls.
const CALL_ROWS: usize = 3 + SPREAD_CALLS * 3;
/// The spreads a call states.
const SPREAD_CALLS: usize = 3;
const _: () =
    assert!(CALL_ROWS * KEPT_CALLS < report::MAX_ROWS, "the kept calls and their count must fit a section's rows");

/// What a report is made of.
pub(crate) struct Inputs<'a> {
    /// The build, as About Chatkord states it.
    pub(crate) stated: &'a Stated,
    /// The version the tree states: the report's header.
    pub(crate) version: &'static str,
    /// The machine, once the host has read it.
    pub(crate) machine: Option<&'a Machine>,
    /// This device's encoders, once the host has listed them.
    pub(crate) encoders: Option<&'a [EncoderOption]>,
    /// The devices listed so far.
    pub(crate) devices: Counted,
    /// What this device remembers.
    pub(crate) remembered: &'a Remembered,
    /// Each server's address, which is never written, and how the rail marks it.
    pub(crate) servers: Vec<(&'a str, RailState)>,
    /// The last calls.
    pub(crate) calls: &'a Finished,
    /// This run's recent log and the previous run's.
    pub(crate) logs: super::Logs,
    /// The moment, in Unix seconds, from the window's one clock.
    pub(crate) created: u64,
}

/// The client's report: the name it is saved under and what the dialog lists of it. Its text is written when the file
/// is, not here.
pub(crate) struct Assembled {
    pub(crate) report: Report,
    pub(crate) name: String,
    pub(crate) listing: Listing,
}

/// What a package built from `origin` on `channel` is: a release only from a release's tag on the stable channel, as
/// a dev build is tagged too.
fn source_of(origin: Option<&crate::Origin>, channel: channel::Channel) -> &'static str {
    match (origin, channel) {
        (Some(crate::Origin::Tag(_)), channel::Channel::Stable) => "release",
        (Some(_), _) => "pre-release",
        (None, _) => "local build",
    }
}

/// The client's report for `inputs`, listed from its own parts.
///
/// # Errors
///
/// [`Full`] when a part does not fit the report's limits, which the caps below keep from happening.
pub(crate) fn assemble(inputs: Inputs<'_>) -> Result<Assembled, Full> {
    let Inputs { stated, version, machine, encoders, devices, remembered, servers, calls, logs, created } = inputs;
    let source = source_of(stated.source.as_ref(), channel::THIS.channel);
    let mut report = Report::new(Side::Client, Build { program: channel::THIS.product, version, source }, created);
    let mut stands = Stands::new();
    report.section(build(stated)?)?;
    report.section(machine.map_or_else(|| Section::absent("System"), |machine| machine.system.clone()))?;
    report.section(machine.map_or_else(|| Section::absent("Graphics"), |machine| machine.graphics.clone()))?;
    report.section(encoder_rows(encoders)?)?;
    report.section(device_rows(devices)?)?;
    report.section(choices::debug_section(remembered)?)?;
    report.section(server_rows(&servers, &mut stands)?)?;
    report.section(call_rows(calls, &mut stands)?)?;
    report.log(logs.recent);
    if let Some(previous) = logs.previous {
        report.previous_run(previous);
    }
    Ok(Assembled { name: report.file_name(), listing: Listing::of(&report), report })
}

fn build(stated: &Stated) -> Result<Section, Full> {
    let mut rows = Section::new("Build");
    rows.push("version", &Product::fact_of(&stated.version))?;
    rows.push("licence", &Product::fact_of(stated.licence))?;
    match &stated.source {
        Some(crate::Origin::Tag(tag)) => rows.push("source tag", &Product::fact_of(tag))?,
        Some(crate::Origin::Commit(commit)) => rows.push("source commit", &Product::fact_of(commit))?,
        None => rows.push("source", &Fact::Word(word!("local build")))?,
    }
    Ok(rows)
}

/// The encoders in the order they are tried, each by the codec it makes and what it runs on.
fn encoder_rows(encoders: Option<&[EncoderOption]>) -> Result<Section, Full> {
    let Some(encoders) = encoders else { return Ok(Section::absent("Encoders")) };
    let mut rows = Section::new("Encoders");
    // A section past its rows keeps the last for the count of what it leaves out.
    let most = if encoders.len() <= report::MAX_ROWS { report::MAX_ROWS } else { report::MAX_ROWS - 1 };
    let (listed, unlisted) = split_listed(encoders, most);
    for EncoderOption { codec, on } in listed {
        rows.push(
            match codec {
                Codec::H264 => "H.264 encoder",
                Codec::Av1 => "AV1 encoder",
            },
            on,
        )?;
    }
    not_listed(&mut rows, unlisted)?;
    Ok(rows)
}

/// The first `most` of `items`, and how many are left out.
fn split_listed<T>(items: &[T], most: usize) -> (&[T], usize) {
    let (listed, rest) = items.split_at(items.len().min(most));
    (listed, rest.len())
}

/// The row that says how many items the section does not list, when there are any.
fn not_listed(rows: &mut Section, unlisted: usize) -> Result<(), Full> {
    if unlisted > 0 {
        rows.push("not listed", &unlisted)?;
    }
    Ok(())
}

fn device_rows(devices: Counted) -> Result<Section, Full> {
    let Counted { microphones, speakers, cameras, monitors } = devices;
    let mut rows = Section::new("Devices (none: not listed this run)");
    rows.push("microphones", &microphones)?;
    rows.push("speakers", &speakers)?;
    rows.push("cameras", &cameras)?;
    rows.push("monitors", &monitors)?;
    Ok(rows)
}

fn server_rows(servers: &[(&str, RailState)], stands: &mut Stands) -> Result<Section, Full> {
    let mut rows = Section::new("Servers");
    rows.push("servers", &servers.len())?;
    let (listed, unlisted) = split_listed(servers, MAX_SERVERS);
    for (address, state) in listed {
        rows.push("server", &stands.fact("server", address))?;
        rows.push("state", state)?;
    }
    not_listed(&mut rows, unlisted)?;
    Ok(rows)
}

/// One call after another, each on the stand-in of its server.
fn call_rows(calls: &Finished, stands: &mut Stands) -> Result<Section, Full> {
    let mut rows = Section::new("Last calls");
    rows.push("calls", &calls.iter().count())?;
    for (address, figures) in calls.iter() {
        let CallFigures { looks: _, round_trip, lost, sent, stalls } = figures;
        rows.push("call on", &stands.fact("server", address))?;
        rows.push("seconds", &figures.length().as_secs())?;
        spread(&mut rows, ["round trip ms least", "round trip ms middle", "round trip ms most"], *round_trip, millis)?;
        spread(
            &mut rows,
            ["audio lost per second least", "audio lost per second middle", "audio lost per second most"],
            *lost,
            |lost| lost,
        )?;
        spread(&mut rows, ["sent bit/s least", "sent bit/s middle", "sent bit/s most"], *sent, |sent| sent)?;
        rows.push("stalls", stalls)?;
    }
    Ok(rows)
}

/// A spread's three figures under `labels`, each absent when the call made no figure.
fn spread<T: Copy>(
    rows: &mut Section,
    labels: [&'static str; 3],
    spread: Option<Spread<T>>,
    count: impl Fn(T) -> u64,
) -> Result<(), Full> {
    let figures = spread.map(|Spread { least, middle, most }| [least, middle, most].map(&count));
    for (at, label) in labels.into_iter().enumerate() {
        let fact = figures.map_or(Fact::Absent, |figures| figures[at].fact());
        rows.push(label, &fact)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::health::CallLooks;

    #[test]
    fn only_a_stable_build_from_a_tag_is_a_release() {
        let tag = crate::Origin::Tag("v0.1.0-dev.20261124120000".into());
        let commit = crate::Origin::Commit("1a2b3c4d".into());
        assert_eq!(source_of(Some(&tag), channel::Channel::Stable), "release");
        assert_eq!(source_of(Some(&tag), channel::Channel::Dev), "pre-release");
        assert_eq!(source_of(Some(&commit), channel::Channel::Stable), "pre-release");
        assert_eq!(source_of(None, channel::Channel::Dev), "local build");
    }

    #[test]
    fn a_call_takes_the_rows_the_constants_count() {
        let mut calls = Finished::default();
        calls.push("server", CallFigures::of(&CallLooks::default()));
        let rows = call_rows(&calls, &mut Stands::new()).unwrap();
        assert_eq!(rows.row_count(), 1 + CALL_ROWS);
    }

    fn text_of(section: Section) -> String {
        let mut report =
            Report::new(Side::Client, Build { program: "Chatkord", version: "0", source: "local build" }, 0);
        report.section(section).unwrap();
        report.text()
    }

    #[test]
    fn encoders_past_a_sections_rows_say_how_many_are_not_listed() {
        let option = EncoderOption { codec: Codec::H264, on: choices::EncodesOn::Processor };
        let fits = encoder_rows(Some(&[option; report::MAX_ROWS])).unwrap();
        assert_eq!(fits.row_count(), report::MAX_ROWS);
        assert!(!text_of(fits).contains("not listed"));
        let over = encoder_rows(Some(&[option; report::MAX_ROWS + 3])).unwrap();
        assert_eq!(over.row_count(), report::MAX_ROWS);
        assert!(text_of(over).ends_with("  not listed: 4\n"));
    }

    #[test]
    fn servers_past_the_most_listed_say_how_many_are_not_listed() {
        let servers = vec![("example.invalid", RailState::Online); MAX_SERVERS + 6];
        let rows = server_rows(&servers, &mut Stands::new()).unwrap();
        let text = text_of(rows);
        assert!(text.contains(&format!("  servers: {}\n", MAX_SERVERS + 6)), "{text}");
        assert_eq!(text.matches("  server: ").count(), MAX_SERVERS);
        assert!(text.ends_with("  not listed: 6\n"), "{text}");
        let fits = server_rows(&servers[..MAX_SERVERS], &mut Stands::new()).unwrap();
        assert!(!text_of(fits).contains("not listed"));
    }
}
