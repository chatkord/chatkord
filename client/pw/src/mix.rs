//! A share's sound: the heard applications' streams linked straight into a capture stream of this client's own. With no
//! sink, a session manager has nothing to make a default or move streams into; every link ends with the stream, a
//! crash included.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use libspa_sys as spa_sys;
use pipewire_sys as pw;

use crate::conn::{Connection, Proxy};
use crate::dict::{Owned, interface};
use crate::graph::{Graph, Hears, Link};
use crate::hearing::{Capture, Hearing};
use crate::lent::Lent;
use crate::looper::STREAM;
use crate::session::{Error, State};
use crate::watch::{Heed, Watch};

/// The links a share's sound is made of, kept as the graph says.
struct Links {
    hears: Hears,
    made: HashMap<Link, Proxy<LinkListener>>,
    events: Lent<pw::pw_proxy_events>,
}

/// What a link's callbacks set.
struct LinkListener {
    /// Set when the link failed or went: it is made again the next time the graph or the choice changes.
    dead: AtomicBool,
}

// SAFETY: the proxies and the lent events are only used under the loop's lock, on whichever thread holds it.
unsafe impl Send for Links {}

impl Links {
    /// Makes the links `graph` plans for what is heard, and takes down every other one.
    fn plan(&mut self, graph: &Graph, connection: &Arc<Connection>) {
        let wanted = graph.links(&self.hears);
        self.made.retain(|link, made| wanted.contains(link) && !made.data().dead.load(Ordering::Relaxed));
        for link in wanted {
            if !self.made.contains_key(&link)
                && let Some(made) = self.make(link, connection)
            {
                self.made.insert(link, made);
            }
        }
    }

    /// Asks the daemon's link factory for `link`. `None` when the ask could not even be sent.
    fn make(&self, link: Link, connection: &Arc<Connection>) -> Option<Proxy<LinkListener>> {
        let props = Owned::new(&[
            (c"link.output.node", link.output_node.to_string()),
            (c"link.output.port", link.output_port.to_string()),
            (c"link.input.node", link.input_node.to_string()),
            (c"link.input.port", link.input_port.to_string()),
            // Ours alone: it goes when this client does.
            (c"object.linger", "false".to_owned()),
        ])
        .ok()?;
        let (api, events) = (connection.api(), self.events.as_ptr());
        let held = connection.lock();
        held.create(
            c"link-factory",
            interface::LINK,
            pw::PW_VERSION_LINK,
            &props,
            LinkListener { dead: AtomicBool::new(false) },
            |proxy, hook, data| {
                // SAFETY: the proxy was just made; the hook, the events and the listener are lent and outlive it.
                unsafe { (api.proxy_add_listener)(proxy, hook, events, data) };
                Ok(())
            },
        )
    }
}

impl Heed for Links {
    fn changed(&mut self, graph: &Graph, connection: &Arc<Connection>) {
        self.plan(graph, connection);
    }
}

unsafe extern "C" fn on_link_removed(data: *mut c_void) {
    // SAFETY: `data` is a lent link listener, alive until its proxy is taken down; it is only set through its atomic.
    unsafe { &*data.cast::<LinkListener>() }.dead.store(true, Ordering::Relaxed);
}

unsafe extern "C" fn on_link_error(data: *mut c_void, _seq: i32, _res: i32, _message: *const std::ffi::c_char) {
    // SAFETY: as above.
    unsafe { on_link_removed(data) };
}

/// A share's sound, from the applications `Hears` names, heard as 16-bit stereo. Dropping it takes its stream and every
/// link down.
pub struct Mix {
    /// First, so the stream is gone before the connection it is on.
    hearing: Hearing,
    watch: Watch<Links>,
}

// SAFETY: the stream is only used under the watch's loop lock, or read for its time, which is real-time safe;
// `choose` takes that lock too.
unsafe impl Send for Mix {}
// SAFETY: as above.
unsafe impl Sync for Mix {}

impl Mix {
    /// Starts hearing what `hears` names at `rate`, and any of its streams that start later. `hear` takes the bytes of
    /// each quantum, interleaved little-endian, on PipeWire's real-time thread, so it must not block, lock or allocate.
    pub fn open(hears: Hears, rate: u32, hear: impl FnMut(&[u8]) + Send + 'static) -> Result<Self, Error> {
        static OPENED: AtomicU32 = AtomicU32::new(0);
        let own_pid = std::process::id();
        let name = format!("chatkord-share-{own_pid}-{}", OPENED.fetch_add(1, Ordering::Relaxed));
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_proxy_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_PROXY_EVENTS;
        events.removed = Some(on_link_removed);
        events.error = Some(on_link_error);
        let links = Links { hears, made: HashMap::new(), events: Lent::new(events) };
        let watch = Watch::open(c"chatkord-share", &STREAM, true, Graph::new(own_pid, Some(name.clone())), links)?;
        let hear = Box::new(hear);
        let properties = [
            (c"media.type", "Audio".to_owned()),
            (c"media.category", "Capture".to_owned()),
            (c"node.name", name.clone()),
            (c"node.description", "Chatkord share".to_owned()),
            (c"node.autoconnect", "false".to_owned()),
            (c"node.dont-reconnect", "true".to_owned()),
        ];
        let capture = Capture {
            properties: &properties,
            positions: &[spa_sys::SPA_AUDIO_CHANNEL_FL, spa_sys::SPA_AUDIO_CHANNEL_FR],
            linked_by_pipewire: false,
        };
        let hearing = watch.with(|_, _, connection| Hearing::open(connection, &capture, rate, hear))?;
        Ok(Self { hearing, watch })
    }

    /// Where its capture stream is: streaming, waiting for an application to link, or ended by the sound server.
    pub fn state(&self) -> State {
        self.hearing.state()
    }

    /// How long the applications' sound took through the graph to reach the stream, at `rate`.
    pub fn timing(&self, rate: u32) -> Option<crate::Timing> {
        self.hearing.timing(rate)
    }

    /// Hears these applications from now on instead.
    pub fn choose(&self, hears: Hears) {
        self.watch.with(|links, graph, connection| {
            links.hears = hears;
            links.plan(graph, connection);
        });
    }
}
