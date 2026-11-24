//! The applications playing sound now, heard as they come and go: what a person picks a share's sound from.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::conn::Connection;
use crate::graph::{App, Graph};
use crate::looper::PROTOCOL;
use crate::session::Error;
use crate::watch::{Heed, Watch};

struct Notify {
    last: BTreeSet<App>,
    changed: Box<dyn FnMut(Vec<App>) + Send>,
}

impl Heed for Notify {
    fn changed(&mut self, graph: &Graph, _connection: &Arc<Connection>) {
        let apps = graph.apps();
        if apps != self.last {
            (self.changed)(apps.iter().cloned().collect());
            self.last = apps;
        }
    }
}

/// A listener on the applications playing sound, this process left out. Dropping it stops listening.
pub struct Apps {
    _watch: Watch<Notify>,
}

impl Apps {
    /// Calls `changed` with every application playing sound, in name order, each time that list changes. It runs on
    /// PipeWire's thread with the loop's lock held, so it must not block or call back into this crate.
    pub fn watch(changed: impl FnMut(Vec<App>) + Send + 'static) -> Result<Self, Error> {
        let notify = Notify { last: BTreeSet::new(), changed: Box::new(changed) };
        Watch::open(c"chatkord-apps", &[PROTOCOL], false, Graph::new(std::process::id(), None), notify)
            .map(|watch| Self { _watch: watch })
    }
}
