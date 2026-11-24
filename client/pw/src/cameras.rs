//! The cameras on the camera portal's connection, as libwebrtc's own capture finds them: every node whose role is
//! `Camera`, bound for its names and the formats it offers, and heard as it comes and goes. The daemon answers in
//! order, so a sync asked after the last request says when everything asked before it has been told.

use std::collections::BTreeMap;
use std::ffi::{c_char, c_int, c_void};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use libspa_sys as spa;
use pipewire_sys as pw;

use crate::conn::{Connection, Listening, Proxy, RegistryProxy, ended};
use crate::dict::{Borrowed, interface, text};
use crate::graph::Props;
use crate::lent::Lent;
use crate::looper::PROTOCOL;
use crate::methods;
use crate::session::Error;

/// A camera the daemon offers, as last told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Camera {
    /// The node's id on this connection, which a stream connects to.
    pub node: u32,
    /// Its `node.name`, stable while it stays plugged in.
    pub name: String,
    /// Its `node.description`, the name a person knows it by.
    pub description: String,
    /// Each format it offers (`EnumFormat`), a serialised pod.
    pub formats: Vec<Vec<u8>>,
}

/// What the directory tells, from PipeWire's thread with the loop's lock held: never block in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Everything asked has been told: the cameras listed now are whole, and differ from the last time this was said.
    Listed,
    /// The camera of this `node.name` went away.
    Went(String),
    /// The daemon ended this connection: nothing more will be told.
    Lost,
}

/// The cameras as the daemon last told them whole, shared with the threads that wait for them: the only list a
/// reader is given, so none sees a camera whose formats are still being told.
#[derive(Default)]
struct Answers {
    last: Mutex<Answer>,
    given: Condvar,
}

#[derive(Default)]
struct Answer {
    /// How many answers were given.
    count: u64,
    cameras: Vec<Camera>,
    /// Whether the connection ended, so no other answer will come.
    ended: bool,
}

impl Answers {
    fn last(&self) -> MutexGuard<'_, Answer> {
        self.last.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Gives `cameras` as the newest answer, and says whether they differ from the last.
    fn give(&self, cameras: Vec<Camera>) -> bool {
        let mut last = self.last();
        let changed = last.cameras != cameras;
        last.cameras = cameras;
        last.count += 1;
        drop(last);
        self.given.notify_all();
        changed
    }

    /// Lets go of whoever waits, and whoever asks later, with the last answer: there will be no other.
    fn end(&self) {
        self.last().ended = true;
        self.given.notify_all();
    }

    /// The last answer once it is `enough`, waiting up to `within` for a newer one each time it is not; the last
    /// answer, whatever it is, when the time is up or the connection ended.
    fn enough(&self, within: Duration, enough: impl Fn(&[Camera]) -> bool) -> Vec<Camera> {
        let until = Instant::now() + within;
        let mut seen = 0;
        let mut last = self.last();
        loop {
            let left = until.saturating_duration_since(Instant::now());
            last = self
                .given
                .wait_timeout_while(last, left, |answer| answer.count == seen && !answer.ended)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            if last.ended || last.count == seen || enough(&last.cameras) || Instant::now() >= until {
                return last.cameras.clone();
            }
            seen = last.count;
        }
    }
}

/// A camera node bound for its info and parameters, and what it has said so far.
struct Node {
    proxy: Proxy<NodeListener>,
    name: Option<String>,
    description: Option<String>,
    formats: Vec<Vec<u8>>,
}

struct NodeListener {
    id: u32,
    state: *mut c_void,
}

/// What every callback shares, reached only under the loop's lock.
struct State {
    connection: Arc<Connection>,
    registry: Option<RegistryProxy>,
    nodes: BTreeMap<u32, Node>,
    node_events: Lent<pw::pw_node_events>,
    /// The sequence of the last sync asked, whether its answer is awaited, and whether anything was asked after it.
    sync: c_int,
    waiting: bool,
    asked_since: bool,
    answers: Arc<Answers>,
    told: Box<dyn FnMut(Change) + Send>,
}

impl State {
    /// Binds the camera node `id`, whose names and formats arrive at its listener. `root` is the lent state's pointer.
    fn bind(&mut self, id: u32, root: *mut c_void) {
        let Some(registry) = &self.registry else { return };
        let bound = {
            let held = self.connection.lock();
            let events = self.node_events.as_ptr();
            held.bind(
                registry,
                id,
                interface::NODE,
                pw::PW_VERSION_NODE,
                NodeListener { id, state: root },
                |proxy, hook, data| {
                    // SAFETY: the proxy was just made and the loop is locked; the hook and the events are lent and outlive
                    // it, as the state does.
                    unsafe { methods::listen_node(proxy, hook, events, data) }
                },
            )
        };
        let Some(proxy) = bound else { return };
        self.nodes.insert(id, Node { proxy, name: None, description: None, formats: Vec::new() });
        self.asked();
    }

    /// Something was asked of the daemon: a sync follows it, now or once the one awaited is answered.
    fn asked(&mut self) {
        self.asked_since = true;
        if !self.waiting {
            self.sync();
        }
    }

    fn sync(&mut self) {
        self.asked_since = false;
        let seq = self.connection.lock().sync(self.sync);
        // A sync the daemon refused is never answered: what was told stands.
        self.waiting = seq >= 0;
        if self.waiting {
            self.sync = seq;
        }
    }

    /// The awaited sync was answered: everything asked before it has been told.
    fn answered(&mut self) {
        self.waiting = false;
        if self.asked_since { self.sync() } else { self.quiet() }
    }

    /// With nothing awaited, tells a changed list and lets go of whoever waits for one.
    fn quiet(&mut self) {
        if self.waiting || self.asked_since {
            return;
        }
        if self.answers.give(self.cameras()) {
            (self.told)(Change::Listed);
        }
    }

    /// The cameras whose names are known.
    fn cameras(&self) -> Vec<Camera> {
        self.nodes
            .iter()
            .filter_map(|(id, node)| {
                Some(Camera {
                    node: *id,
                    name: node.name.clone()?,
                    description: node.description.clone()?,
                    formats: node.formats.clone(),
                })
            })
            .collect()
    }
}

/// The cameras on one connection to the daemon, heard on a loop of their own. Dropping it stops listening.
pub struct Cameras {
    /// First, so the callbacks stop before the state they reach, and the registry in it, are freed.
    _core_listening: Listening<pw::pw_core_events>,
    _listening: Listening<pw::pw_registry_events>,
    state: Lent<State>,
    answers: Arc<Answers>,
    connection: Arc<Connection>,
}

// SAFETY: every use of the state is under the loop's lock, or in `Drop` after the listeners that reach it are gone.
unsafe impl Send for Cameras {}
// SAFETY: as above: `cameras` reaches the state only under the loop's lock.
unsafe impl Sync for Cameras {}

impl Cameras {
    /// Connects on `remote`, the camera portal's connection, and tells `told` what changes.
    pub fn open(remote: OwnedFd, told: impl FnMut(Change) + Send + 'static) -> Result<Self, Error> {
        let connection = Connection::open_fd(c"chatkord-cameras", &[PROTOCOL], remote, "connect to the cameras")?;
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut node_events: pw::pw_node_events = unsafe { std::mem::zeroed() };
        node_events.version = pw::PW_VERSION_NODE_EVENTS;
        node_events.info = Some(on_info);
        node_events.param = Some(on_param);
        let answers = Arc::new(Answers::default());
        let state = Lent::new(State {
            connection: Arc::clone(&connection),
            registry: None,
            nodes: BTreeMap::new(),
            node_events: Lent::new(node_events),
            sync: 0,
            waiting: false,
            asked_since: false,
            answers: Arc::clone(&answers),
            told: Box::new(told),
        });
        // SAFETY: as above.
        let mut events: pw::pw_registry_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_REGISTRY_EVENTS;
        events.global = Some(on_global);
        events.global_remove = Some(on_global_remove);
        // SAFETY: as above.
        let mut core_events: pw::pw_core_events = unsafe { std::mem::zeroed() };
        core_events.version = pw::PW_VERSION_CORE_EVENTS;
        core_events.done = Some(on_done);
        core_events.error = Some(on_error);
        let held = connection.lock();
        let registry = held.registry()?;
        let data = state.as_ptr().cast::<c_void>();
        // SAFETY: the state is lent and outlives the listenings, which `Cameras` drops first.
        let core_listening = unsafe { held.listen_core(core_events, data) }?;
        // SAFETY: as above.
        let listening = unsafe { held.listen_registry(&registry, events, data) }?;
        // SAFETY: no callback reads the state before the loop's lock is let go, which this holds.
        let inner = unsafe { &mut *state.as_ptr() };
        inner.registry = Some(registry);
        // The registry's announcements of the nodes already there come before this sync's answer.
        inner.asked();
        drop(held);
        Ok(Self { _core_listening: core_listening, _listening: listening, state, answers, connection })
    }

    /// The cameras as the daemon last told them whole, once they are `enough`, waiting up to `within` for a newer
    /// answer each time they are not: a portal's connection is shown its cameras only once the portal has granted
    /// them, which can be after the first answer. The last answer, whatever it is, when the time is up or the
    /// connection ended.
    pub fn settled(&self, within: Duration, enough: impl Fn(&[Camera]) -> bool) -> Vec<Camera> {
        self.answers.enough(within, enough)
    }
}

impl Drop for Cameras {
    fn drop(&mut self) {
        let _lock = self.connection.lock();
        // SAFETY: the callbacks reach the state only under this same lock.
        let state = unsafe { &mut *self.state.as_ptr() };
        state.nodes.clear();
    }
}

/// Whether a global's properties say it is a camera, as libwebrtc's capture decides: a described node in the camera's
/// role. An application's stream reading a camera carries the role too, but no description.
fn is_camera(props: &impl Props) -> bool {
    props.get("node.description").is_some() && props.get("media.role") == Some("Camera")
}

unsafe extern "C" fn on_global(
    data: *mut c_void,
    id: u32,
    _permissions: u32,
    kind: *const c_char,
    _version: u32,
    props: *const spa::spa_dict,
) {
    // SAFETY: `data` is the lent state; callbacks run one at a time, under the loop's lock.
    let state = unsafe { &mut *data.cast::<State>() };
    // SAFETY: `kind` is the global's interface name, a string PipeWire owns for this call.
    let node = unsafe { text(kind) } == Some(interface::NODE.to_bytes());
    // SAFETY: `props` is the dictionary of this call.
    if node && is_camera(&unsafe { Borrowed::new(props) }) && !state.nodes.contains_key(&id) {
        state.bind(id, data);
    }
}

unsafe extern "C" fn on_global_remove(data: *mut c_void, id: u32) {
    // SAFETY: as above.
    let state = unsafe { &mut *data.cast::<State>() };
    let Some(Node { proxy, name, .. }) = state.nodes.remove(&id) else { return };
    drop(proxy);
    if let Some(name) = name {
        (state.told)(Change::Went(name));
    }
    state.quiet();
}

unsafe extern "C" fn on_info(data: *mut c_void, info: *const pw::pw_node_info) {
    // SAFETY: `data` is a bound node's lent listener, whose `state` is the directory's lent state; callbacks run one
    // at a time under the loop's lock.
    let listener = unsafe { &*data.cast::<NodeListener>() };
    // SAFETY: as above.
    let state = unsafe { &mut *listener.state.cast::<State>() };
    // SAFETY: `info` is PipeWire's for this call, or null.
    let Some(info) = (unsafe { info.as_ref() }) else { return };
    let Some(node) = state.nodes.get_mut(&listener.id) else { return };
    if info.change_mask & u64::from(pw::PW_NODE_CHANGE_MASK_PROPS) != 0 {
        // SAFETY: the node's dictionary, PipeWire's for this call.
        let props = unsafe { Borrowed::new(info.props) };
        node.name = props.get("node.name").map(str::to_owned);
        node.description = props.get("node.description").map(str::to_owned);
    }
    if info.change_mask & u64::from(pw::PW_NODE_CHANGE_MASK_PARAMS) == 0 || info.params.is_null() {
        state.quiet();
        return;
    }
    // SAFETY: a node's info holds `n_params` entries at `params`, PipeWire's for this call.
    let params = unsafe { std::slice::from_raw_parts(info.params, info.n_params as usize) };
    let readable =
        params.iter().any(|param| param.id == spa::SPA_PARAM_EnumFormat && param.flags & spa::SPA_PARAM_INFO_READ != 0);
    if readable {
        node.formats.clear();
        // SAFETY: the node's proxy is live and the loop is locked: this runs in its callback.
        if unsafe { methods::enum_params(node.proxy.raw(), spa::SPA_PARAM_EnumFormat) }.is_ok() {
            state.asked();
            return;
        }
    }
    state.quiet();
}

unsafe extern "C" fn on_param(
    data: *mut c_void,
    _seq: c_int,
    id: u32,
    _index: u32,
    _next: u32,
    param: *const spa::spa_pod,
) {
    // SAFETY: as `on_info`.
    let listener = unsafe { &*data.cast::<NodeListener>() };
    // SAFETY: as above.
    let state = unsafe { &mut *listener.state.cast::<State>() };
    // SAFETY: a param PipeWire hands a callback is null or a pod live for that call.
    let Some(pod) = (unsafe { param.as_ref() }) else { return };
    let Some(node) = state.nodes.get_mut(&listener.id).filter(|_| id == spa::SPA_PARAM_EnumFormat) else { return };
    // SAFETY: a pod is its header and the `size` bytes after it.
    let bytes =
        unsafe { std::slice::from_raw_parts(param.cast::<u8>(), size_of::<spa::spa_pod>() + pod.size as usize) };
    node.formats.push(bytes.to_vec());
}

unsafe extern "C" fn on_done(data: *mut c_void, id: u32, seq: c_int) {
    // SAFETY: `data` is the lent state; the core's listener runs on the same loop, one callback at a time.
    let state = unsafe { &mut *data.cast::<State>() };
    if id == pw::PW_ID_CORE && state.waiting && seq == state.sync {
        state.answered();
    }
}

unsafe extern "C" fn on_error(data: *mut c_void, id: u32, _seq: c_int, res: c_int, _message: *const c_char) {
    // SAFETY: as `on_done`.
    let state = unsafe { &mut *data.cast::<State>() };
    if ended(id, res) {
        (state.told)(Change::Lost);
        state.answers.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn a_camera_is_a_described_node_in_its_role_and_a_stream_reading_one_is_not() {
        let source = HashMap::from([("node.description", "Integrated Camera"), ("media.role", "Camera")]);
        let reading = HashMap::from([("node.name", "shell"), ("media.role", "Camera")]);
        let screen = HashMap::from([("node.description", "Monitor 1"), ("media.role", "Screen")]);
        assert!(is_camera(&source));
        assert!(!is_camera(&reading), "an application's own stream from a camera");
        assert!(!is_camera(&screen));
    }

    const CAMERA: &str = "camera";

    fn camera(formats: usize) -> Camera {
        Camera { node: 1, name: CAMERA.to_owned(), description: "Camera".to_owned(), formats: vec![vec![1]; formats] }
    }

    #[test]
    fn a_reader_is_given_the_last_whole_answer_never_one_still_being_told() {
        let answers = Arc::new(Answers::default());
        assert!(answers.give(vec![camera(2)]), "a first list is a change");
        let waiting = std::thread::spawn({
            let answers = Arc::clone(&answers);
            move || answers.enough(Duration::from_secs(5), |cameras| cameras.iter().all(|each| each.formats.len() == 3))
        });
        assert!(answers.give(vec![camera(3)]));
        assert_eq!(waiting.join().unwrap(), vec![camera(3)], "woken by the answer it waited for");
        assert!(!answers.give(vec![camera(3)]), "the same list again is no change");
        assert_eq!(answers.enough(Duration::ZERO, |_| true), vec![camera(3)]);
    }

    #[test]
    fn a_connection_ended_lets_a_reader_go_with_the_last_answer() {
        let answers = Arc::new(Answers::default());
        answers.give(Vec::new());
        let waiting = std::thread::spawn({
            let answers = Arc::clone(&answers);
            move || answers.enough(Duration::from_secs(60), |cameras| !cameras.is_empty())
        });
        let began = Instant::now();
        answers.end();
        assert_eq!(waiting.join().unwrap(), Vec::new());
        assert!(began.elapsed() < Duration::from_secs(30), "let go well before its minute");
    }

    #[test]
    fn a_reader_with_too_little_is_given_the_last_answer_when_its_time_is_up() {
        let answers = Answers::default();
        answers.give(vec![camera(0)]);
        assert_eq!(answers.enough(Duration::from_millis(20), |cameras| cameras.len() == 2), vec![camera(0)]);
    }
}
