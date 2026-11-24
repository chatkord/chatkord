//! What a control is, apart from where it stands: an id derived from the control it is under, its role's kind, its label
//! and its place among siblings alike. The access tree names its nodes with it, and the focus finds a control again by
//! it after the interface was built anew.

use accesskit::NodeId;
use design::Semantics;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::discriminant;

/// What makes siblings alike: their kind and label, counted among them by [`child_id`].
pub(crate) fn sibling_key(kind: impl Hash, label: impl Hash) -> u64 {
    let mut alike = DefaultHasher::new();
    (kind, label).hash(&mut alike);
    alike.finish()
}

/// The id of the `nth` child of `parent` that is of `kind` and called `label`.
pub(crate) fn child_id(parent: NodeId, kind: impl Hash, label: impl Hash, nth: u32) -> NodeId {
    let mut path = DefaultHasher::new();
    (parent, kind, label, nth).hash(&mut path);
    NodeId(path.finish())
}

/// The window's id, which every other is derived from.
pub(crate) fn window_id() -> NodeId {
    NodeId(DefaultHasher::new().finish())
}

/// A control met, and how many children it has had.
#[derive(Debug)]
struct Met {
    id: NodeId,
    /// How many children so far are alike, by [`sibling_key`].
    seen: HashMap<u64, u32>,
    /// It is operated: a focusable inside it that is not its own belongs to it.
    operated: bool,
    /// A focusable was met as its own.
    focusable: bool,
}

/// The ids of the controls an operation meets, found as the access tree finds them: a control reported by
/// [`Operation::custom`](iced_core::widget::Operation::custom) is under the one whose traversal it is in.
#[derive(Debug)]
pub(crate) struct Lineage {
    met: Vec<Met>,
    /// The control each open traversal is under, as an index into `met`.
    under: Vec<usize>,
    /// The control just reported, whose traversal, if one opens next, holds its content.
    armed: Option<usize>,
}

impl Default for Lineage {
    fn default() -> Self {
        let window = Met { id: window_id(), seen: HashMap::new(), operated: false, focusable: false };
        Lineage { met: vec![window], under: vec![0], armed: None }
    }
}

impl Lineage {
    fn parent(&self) -> usize {
        self.under.last().copied().unwrap_or(0)
    }

    /// A traversal opens: under the control just reported, else under the one around it.
    pub(crate) fn enter(&mut self) {
        let parent = self.armed.take().unwrap_or_else(|| self.parent());
        self.under.push(parent);
    }

    /// The traversal ends.
    pub(crate) fn leave(&mut self) {
        self.under.pop();
        self.armed = None;
    }

    /// Something that holds no content of a control was met (a container, a list, a text): the control reported
    /// before it holds none for the traversal that follows.
    pub(crate) fn release(&mut self) {
        self.armed = None;
    }

    /// A control said what it is.
    pub(crate) fn report(&mut self, semantics: &Semantics) {
        let kind = discriminant(&semantics.role);
        let parent = self.parent();
        let Some(above) = self.met.get_mut(parent) else { return };
        let nth = above.seen.entry(sibling_key(kind, &semantics.label)).or_default();
        let id = child_id(above.id, kind, &semantics.label, *nth);
        *nth += 1;
        self.met.push(Met { id, seen: HashMap::new(), operated: semantics.role.operated(), focusable: false });
        self.armed = Some(self.met.len() - 1);
    }

    /// The control a focusable just met belongs to: the one that reported it, or the control whose content it is when
    /// that control does not take the focus itself (a field inside its label).
    pub(crate) fn focusable(&mut self) -> Option<NodeId> {
        let owner = self.armed.or_else(|| {
            let parent = self.parent();
            let above = self.met.get(parent)?;
            (above.operated && !above.focusable).then_some(parent)
        })?;
        let met = self.met.get_mut(owner)?;
        met.focusable = true;
        Some(met.id)
    }
}
