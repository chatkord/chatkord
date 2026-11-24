//! The applications PipeWire plays and the links that take the heard ones' sound into a share, as values: pure but for
//! a process's parent, which is given, so the plan is tested without a daemon.

use std::collections::{BTreeSet, HashMap, HashSet};

/// The class of a stream an application plays.
const PLAYER_CLASS: &str = "Stream/Output/Audio";

/// An application whose sound can go with a share, as a person picks it. A restart is the same application.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct App {
    /// What it calls itself (`application.name`): what a person reads.
    pub name: String,
    /// The program it runs (`application.process.binary`), when PipeWire was told.
    pub binary: Option<String>,
}

impl App {
    /// Whether `typed` names it: what it calls itself, or its program.
    pub fn answers_to(&self, typed: &str) -> bool {
        self.name == typed || self.binary.as_deref() == Some(typed)
    }
}

/// Which streams a share hears. This process and its children are never heard, whatever is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hears {
    /// No stream at all.
    Nothing,
    /// These applications, as a person names them.
    Apps(HashSet<App>),
    /// A window's owner: the streams of `roots` and their descendants, or of a sandboxed app that says it is `app_id`
    /// (a sandbox's process ids are its own).
    Tree {
        /// The processes whose streams it hears, by id.
        roots: Vec<u32>,
        /// The app id a sandboxed stream may say.
        app_id: String,
    },
    /// Every application's.
    AllButOurs,
}

/// The parent of a process, or `None` once there is none to read.
pub(crate) type ParentOf = Box<dyn Fn(u32) -> Option<u32> + Send>;

/// How far up a process's parents are followed: deeper than any real tree, and a bound on a loop reused ids could make.
const ANCESTORS: usize = 64;

/// Which of the share's two channels a port's sound goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    Left,
    Right,
    Both,
}

impl Side {
    /// Left-side channels go left, right-side ones right, mono and centre ones to both; LFE and unpositioned ones
    /// (`AUX0`, …) carry nothing a share's stereo should hold.
    fn of(channel: &str) -> Option<Self> {
        match channel {
            "FL" | "SL" | "RL" | "FLC" | "RLC" | "FLW" | "FLH" | "TFL" | "TFLC" | "TSL" | "TRL" | "BLC" => {
                Some(Self::Left)
            }
            "FR" | "SR" | "RR" | "FRC" | "RRC" | "FRW" | "FRH" | "TFR" | "TFRC" | "TSR" | "TRR" | "BRC" => {
                Some(Self::Right)
            }
            "MONO" | "FC" | "RC" | "TC" | "TFC" | "TRC" | "BC" | "FCH" => Some(Self::Both),
            _ => None,
        }
    }

    fn feeds(self, input: Self) -> bool {
        self == Self::Both || self == input
    }
}

/// What a global's properties say, read by key.
pub(crate) trait Props {
    fn get(&self, key: &str) -> Option<&str>;
}

/// What kind of global PipeWire announced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Node,
    Port,
    Client,
}

/// A stream an application plays, as its node's global says.
struct Player {
    /// `application.name`, or the node's name when it has none.
    name: Option<String>,
    client: Option<u32>,
}

/// A connection to the daemon, once its own properties said which process made it: the only place a native client's
/// process id and program are told, and where the pulse server puts its clients' too.
#[derive(PartialEq, Eq)]
struct Process {
    name: Option<String>,
    binary: Option<String>,
    pid: Option<u32>,
    /// What a sandboxed (Flatpak) application's portal says it is.
    app_id: Option<String>,
}

/// A link from an application's output port to the capture stream's input port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Link {
    pub(crate) output_node: u32,
    pub(crate) output_port: u32,
    pub(crate) input_node: u32,
    pub(crate) input_port: u32,
}

/// The part of PipeWire's graph a share's sound is planned from.
pub(crate) struct Graph {
    /// This process, whose own streams (the call's playback) and its children's are never offered.
    own_pid: u32,
    parent_of: ParentOf,
    /// The capture stream's `node.name`, and its node once the registry announced it.
    capture_name: Option<String>,
    capture: Option<u32>,
    /// Every stream an application plays, by node.
    players: HashMap<u32, Player>,
    /// Every client, by id; `None` until its properties arrive.
    clients: HashMap<u32, Option<Process>>,
    /// The output ports of those streams: (node, side) by port.
    outputs: HashMap<u32, (u32, Side)>,
    /// The capture stream's input ports, by port.
    inputs: HashMap<u32, Side>,
}

impl Graph {
    /// A graph for this process, with the capture stream named `capture_name` when there is one; a process's parent is
    /// read from `/proc`.
    pub(crate) fn new(own_pid: u32, capture_name: Option<String>) -> Self {
        Self::with_parents(own_pid, capture_name, Box::new(parent_in_proc))
    }

    fn with_parents(own_pid: u32, capture_name: Option<String>, parent_of: ParentOf) -> Self {
        Self {
            own_pid,
            parent_of,
            capture_name,
            capture: None,
            players: HashMap::new(),
            clients: HashMap::new(),
            outputs: HashMap::new(),
            inputs: HashMap::new(),
        }
    }

    /// A global appeared. Returns whether it is a client that must be bound for its properties: its global lacks the
    /// process id and program.
    pub(crate) fn global(&mut self, id: u32, kind: Kind, props: &impl Props) -> bool {
        match kind {
            Kind::Client => {
                self.clients.insert(id, None);
                true
            }
            Kind::Node => {
                if self.capture_name.is_some() && props.get("node.name") == self.capture_name.as_deref() {
                    self.capture = Some(id);
                }
                if props.get("media.class") == Some(PLAYER_CLASS) {
                    let name = props.get("application.name").or_else(|| props.get("node.name")).map(str::to_owned);
                    let client = props.get("client.id").and_then(|client| client.parse().ok());
                    self.players.insert(id, Player { name, client });
                }
                false
            }
            Kind::Port => {
                let (Some(node), Some(side)) = (
                    props.get("node.id").and_then(|node| node.parse::<u32>().ok()),
                    props.get("audio.channel").and_then(Side::of),
                ) else {
                    return false;
                };
                match props.get("port.direction") {
                    Some("out") if self.players.contains_key(&node) => {
                        self.outputs.insert(id, (node, side));
                    }
                    Some("in") if self.capture == Some(node) && side != Side::Both => {
                        self.inputs.insert(id, side);
                    }
                    _ => {}
                }
                false
            }
        }
    }

    /// A bound client's own properties arrived. Returns whether they changed which process it is.
    pub(crate) fn client(&mut self, id: u32, props: &impl Props) -> bool {
        let Some(slot) = self.clients.get_mut(&id) else { return false };
        let read = |key| props.get(key).map(str::to_owned);
        let process = Process {
            name: read("application.name"),
            binary: read("application.process.binary"),
            pid: props.get("application.process.id").and_then(|pid| pid.parse().ok()),
            app_id: read("pipewire.access.portal.app_id"),
        };
        let changed = slot.as_ref() != Some(&process);
        *slot = Some(process);
        changed
    }

    /// A global went. Returns whether it was one the plan reads.
    pub(crate) fn remove(&mut self, id: u32) -> bool {
        if self.capture == Some(id) {
            self.capture = None;
            self.inputs.clear();
            return true;
        }
        if self.players.remove(&id).is_some() {
            self.outputs.retain(|_, (node, _)| *node != id);
            return true;
        }
        if self.clients.remove(&id).is_some() {
            return true;
        }
        self.outputs.remove(&id).is_some() || self.inputs.remove(&id).is_some()
    }

    /// The applications playing now, this process left out, each once however many streams it has.
    pub(crate) fn apps(&self) -> BTreeSet<App> {
        self.others().map(|(_, app, _)| app).collect()
    }

    /// The links that take every stream `hears` names into the capture stream, and nothing else.
    pub(crate) fn links(&self, hears: &Hears) -> HashSet<Link> {
        let Some(capture) = self.capture else { return HashSet::new() };
        let heard: HashSet<u32> = self
            .others()
            .filter(|(_, app, process)| match hears {
                Hears::Nothing => false,
                Hears::Apps(chosen) => chosen.contains(app),
                Hears::Tree { roots, app_id } => {
                    process.pid.is_some_and(|pid| self.descends(pid, roots))
                        || process.app_id.as_deref() == Some(app_id.as_str())
                }
                Hears::AllButOurs => true,
            })
            .map(|(node, ..)| node)
            .collect();
        let mut links = HashSet::new();
        for (&output_port, &(output_node, side)) in &self.outputs {
            if !heard.contains(&output_node) {
                continue;
            }
            for (&input_port, &input) in &self.inputs {
                if side.feeds(input) {
                    links.insert(Link { output_node, output_port, input_node: capture, input_port });
                }
            }
        }
        links
    }

    /// Each player whose client said which process it is, as the application it is, this process's own and its
    /// children's left out. A player whose process is not known yet is left out too: it might be this one.
    fn others(&self) -> impl Iterator<Item = (u32, App, &Process)> {
        self.players.iter().filter_map(|(&node, player)| {
            let process = self.clients.get(&player.client?)?.as_ref()?;
            if process.pid.is_some_and(|pid| self.descends(pid, &[self.own_pid])) {
                return None;
            }
            let name = player.name.clone().or_else(|| process.name.clone())?;
            Some((node, App { name, binary: process.binary.clone() }, process))
        })
    }

    /// Whether `pid` is one of `roots` or a descendant of one.
    fn descends(&self, pid: u32, roots: &[u32]) -> bool {
        std::iter::successors(Some(pid), |&pid| (self.parent_of)(pid)).take(ANCESTORS).any(|pid| roots.contains(&pid))
    }
}

/// A process's parent, from the fourth field of `/proc/<pid>/stat` (after the name, which may hold spaces and
/// brackets); none for the first process, one that ended, or a system without `/proc`.
fn parent_in_proc(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after_name) = stat.rsplit_once(')')?;
    let parent: u32 = after_name.split_whitespace().nth(1)?.parse().ok()?;
    (parent != 0).then_some(parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN_PID: u32 = 100;
    const CAPTURE: &str = "chatkord-share-test";

    impl Props for HashMap<&str, &str> {
        fn get(&self, key: &str) -> Option<&str> {
            HashMap::get(self, key).copied()
        }
    }

    fn props<'a>(pairs: &[(&'a str, &'a str)]) -> HashMap<&'a str, &'a str> {
        pairs.iter().copied().collect()
    }

    /// Who started whom, in the tests' processes: this process (100) started a helper (110); a browser (500) started
    /// its sound service (510). Every other process has no known parent.
    fn parent(pid: u32) -> Option<u32> {
        match pid {
            110 => Some(OWN_PID),
            510 => Some(500),
            _ => None,
        }
    }

    /// A graph with the capture stream (node 1, FL in 2, FR in 3) and two players: a music player (node 10, pid 200)
    /// and this process's own call playback (node 20).
    fn graph() -> Graph {
        let mut graph = Graph::with_parents(OWN_PID, Some(CAPTURE.to_owned()), Box::new(parent));
        graph.global(1, Kind::Node, &props(&[("node.name", CAPTURE), ("media.class", "Stream/Input/Audio")]));
        for (id, channel) in [(2, "FL"), (3, "FR")] {
            let port = props(&[("node.id", "1"), ("port.direction", "in"), ("audio.channel", channel)]);
            graph.global(id, Kind::Port, &port);
        }
        play(&mut graph, 10, "Music", "music", "200", &[(11, "FL"), (12, "FR")]);
        play(&mut graph, 20, "Chatkord", "chatkord", &OWN_PID.to_string(), &[(21, "FL"), (22, "FR")]);
        graph
    }

    /// A player `node` whose client is `node + 1000`, made by process `pid` running `binary`.
    fn play(graph: &mut Graph, node: u32, name: &str, binary: &str, pid: &str, ports: &[(u32, &str)]) {
        let client = node + 1000;
        assert!(graph.global(client, Kind::Client, &props(&[("application.name", name)])));
        let client_id = client.to_string();
        let global = [("node.name", name), ("application.name", name), ("media.class", PLAYER_CLASS)];
        assert!(!graph.global(node, Kind::Node, &props(&[global.as_slice(), &[("client.id", &client_id)]].concat())));
        let node_id = node.to_string();
        for &(id, channel) in ports {
            let port = props(&[("node.id", &node_id), ("port.direction", "out"), ("audio.channel", channel)]);
            graph.global(id, Kind::Port, &port);
        }
        let process = props(&[
            ("application.name", name),
            ("application.process.binary", binary),
            ("application.process.id", pid),
        ]);
        assert!(graph.client(client, &process));
    }

    fn app(name: &str, binary: &str) -> App {
        App { name: name.to_owned(), binary: Some(binary.to_owned()) }
    }

    fn link(output_node: u32, output_port: u32, input_port: u32) -> Link {
        Link { output_node, output_port, input_node: 1, input_port }
    }

    fn apps(chosen: &[App]) -> Hears {
        Hears::Apps(chosen.iter().cloned().collect())
    }

    /// The output nodes `hears` links.
    fn linked(graph: &Graph, hears: &Hears) -> BTreeSet<u32> {
        graph.links(hears).iter().map(|link| link.output_node).collect()
    }

    #[test]
    fn a_window_is_heard_through_its_owners_process_tree_and_nothing_else() {
        let mut graph = graph();
        // The browser's window is owned by 500; its sound plays from 510, a child.
        play(&mut graph, 30, "Brave", "brave", "510", &[(31, "FL"), (32, "FR")]);
        let window = Hears::Tree { roots: vec![500], app_id: "brave-browser".to_owned() };
        assert_eq!(linked(&graph, &window), BTreeSet::from([30]));
        let elsewhere = Hears::Tree { roots: vec![700], app_id: "other".to_owned() };
        assert!(graph.links(&elsewhere).is_empty(), "an owner that plays nothing is heard as nothing");
    }

    #[test]
    fn a_sandboxed_window_is_heard_by_the_app_id_its_portal_gives() {
        let mut graph = graph();
        play(&mut graph, 30, "Player", "player", "3", &[(31, "FL")]);
        let process = props(&[
            ("application.name", "Player"),
            ("application.process.id", "3"),
            ("pipewire.access.portal.app_id", "org.example.Player"),
        ]);
        assert!(graph.client(1030, &process));
        let window = Hears::Tree { roots: vec![9000], app_id: "org.example.Player".to_owned() };
        assert_eq!(linked(&graph, &window), BTreeSet::from([30]));
    }

    #[test]
    fn a_whole_screen_hears_every_application_but_this_process_and_its_children() {
        let mut graph = graph();
        play(&mut graph, 30, "Game", "game", "300", &[(31, "FL")]);
        play(&mut graph, 40, "Helper", "chatkord-helper", "110", &[(41, "FL")]);
        assert_eq!(linked(&graph, &Hears::AllButOurs), BTreeSet::from([10, 30]));
        assert!(!graph.apps().contains(&app("Helper", "chatkord-helper")), "a child of ours is never offered");
        assert!(graph.links(&Hears::Nothing).is_empty());
    }

    #[test]
    fn only_the_chosen_applications_streams_are_linked_side_to_side() {
        let mut graph = graph();
        play(&mut graph, 30, "Game", "game", "300", &[(31, "FL"), (32, "FR")]);
        let links = graph.links(&apps(&[app("Music", "music")]));
        assert_eq!(links, HashSet::from([link(10, 11, 2), link(10, 12, 3)]));
        assert!(graph.links(&apps(&[])).is_empty());
    }

    #[test]
    fn this_process_is_never_offered_nor_linked() {
        let graph = graph();
        assert_eq!(graph.apps(), BTreeSet::from([app("Music", "music")]));
        assert!(graph.links(&apps(&[app("Chatkord", "chatkord")])).is_empty());
    }

    #[test]
    fn every_stream_of_an_application_is_linked_and_a_restart_is_followed() {
        let mut graph = graph();
        let music = apps(&[app("Music", "music")]);
        play(&mut graph, 40, "Music", "music", "200", &[(41, "MONO")]);
        assert_eq!(graph.links(&music).len(), 4);
        assert_eq!(graph.apps().len(), 1);

        // It quits: its streams, its clients and their links go.
        for id in [10, 40, 1010, 1040] {
            assert!(graph.remove(id));
        }
        assert!(graph.links(&music).is_empty());

        // It starts again under another process: the same application, linked again.
        play(&mut graph, 50, "Music", "music", "201", &[(51, "FL"), (52, "FR")]);
        assert_eq!(graph.links(&music), HashSet::from([link(50, 51, 2), link(50, 52, 3)]));
    }

    #[test]
    fn mono_and_centre_feed_both_sides_and_lfe_feeds_none() {
        let mut graph = graph();
        play(&mut graph, 60, "Film", "film", "600", &[(61, "FC"), (62, "LFE"), (63, "RR"), (64, "AUX0")]);
        let links = graph.links(&apps(&[app("Film", "film")]));
        assert_eq!(links, HashSet::from([link(60, 61, 2), link(60, 61, 3), link(60, 63, 3)]));
    }

    #[test]
    fn a_stream_whose_process_is_not_known_yet_is_not_linked() {
        let mut graph = graph();
        graph.global(1070, Kind::Client, &props(&[("application.name", "Music")]));
        let global = [("node.name", "Music"), ("media.class", PLAYER_CLASS), ("client.id", "1070")];
        graph.global(70, Kind::Node, &props(&global));
        let port = props(&[("node.id", "70"), ("port.direction", "out"), ("audio.channel", "FL")]);
        graph.global(71, Kind::Port, &port);
        let links = graph.links(&apps(&[app("Music", "music")]));
        assert!(links.iter().all(|link| link.output_node == 10));
    }

    #[test]
    fn nothing_but_a_player_is_linked_and_nothing_without_the_capture_stream() {
        let mut graph = graph();
        // A sink and its monitor port: a device, never an application.
        graph.global(80, Kind::Node, &props(&[("node.name", "Speakers"), ("media.class", "Audio/Sink")]));
        let monitor = props(&[("node.id", "80"), ("port.direction", "out"), ("audio.channel", "FL")]);
        graph.global(81, Kind::Port, &monitor);
        let everything = Hears::AllButOurs;
        assert!(graph.links(&everything).iter().all(|link| link.output_node == 10));

        assert!(graph.remove(1));
        assert!(graph.links(&everything).is_empty());
    }
}
