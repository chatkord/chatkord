//! Taking a listener's hook out of the list PipeWire keeps it in. Destroying a proxy does not do it: one the daemon has
//! not yet removed stays, and goes on telling its listeners, until the daemon answers.

use libspa_sys as spa;

/// Unlinks `hook` from its list and tells its owner, as SPA's inline `spa_hook_remove` does, and leaves it unlinked.
///
/// # Safety
/// `hook` was initialised by an `add_listener` (or is zeroed, never added), and the loop that walks its list is locked.
pub(crate) unsafe fn remove(hook: *mut spa::spa_hook) {
    // SAFETY: the caller says `hook` is an initialised or zeroed hook.
    let spa::spa_list { prev, next } = unsafe { (*hook).link };
    if !prev.is_null() {
        // SAFETY: the caller's promise: a linked hook's neighbours are live entries of the same list.
        unsafe { (*prev).next = next };
        // SAFETY: as above.
        unsafe { (*next).prev = prev };
        // SAFETY: as the first read; unlinked, so a second removal touches no list.
        unsafe { (*hook).link = spa::spa_list { prev: std::ptr::null_mut(), next: std::ptr::null_mut() } };
    }
    // SAFETY: as the first read.
    if let Some(removed) = unsafe { (*hook).removed } {
        // SAFETY: the owner's callback for this hook, called as `spa_hook_remove` does.
        unsafe { removed(hook) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked(list: *mut spa::spa_list, hook: *mut spa::spa_hook) {
        // SAFETY: this and each step below is `spa_list_append` on live locals of the test.
        let last = unsafe { (*list).prev };
        // SAFETY: as above.
        let link = unsafe { &raw mut (*hook).link };
        // SAFETY: as above.
        unsafe { *link = spa::spa_list { prev: last, next: list } };
        // SAFETY: as above; `last` is the list's head or one of its entries.
        unsafe { (*last).next = link };
        // SAFETY: as above.
        unsafe { (*list).prev = link };
    }

    #[test]
    fn a_removed_hook_leaves_its_neighbours_linked_to_each_other() {
        let mut list = spa::spa_list { prev: std::ptr::null_mut(), next: std::ptr::null_mut() };
        // SAFETY: plain data; zeroed is an empty hook.
        let mut first: spa::spa_hook = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        let mut second: spa::spa_hook = unsafe { std::mem::zeroed() };
        let list_at: *mut spa::spa_list = &raw mut list;
        // SAFETY: a live local; this is `spa_list_init`.
        unsafe { *list_at = spa::spa_list { prev: list_at, next: list_at } };
        let (first_at, second_at): (*mut spa::spa_hook, *mut spa::spa_hook) = (&raw mut first, &raw mut second);
        linked(list_at, first_at);
        linked(list_at, second_at);
        // SAFETY: added above, and nothing else walks the list.
        unsafe { remove(first_at) };
        // SAFETY: as above.
        let head = unsafe { *list_at };
        // SAFETY: as above.
        let second_link = unsafe { (*second_at).link };
        // SAFETY: as above.
        let second_link_at = unsafe { &raw mut (*second_at).link };
        assert_eq!(head.next, second_link_at);
        assert_eq!(second_link.prev, list_at);
    }

    #[test]
    fn a_hook_never_added_is_left_alone_and_its_owner_is_told() {
        unsafe extern "C" fn told(hook: *mut spa::spa_hook) {
            // SAFETY: the test's hook, live while it is removed.
            unsafe { (*hook).priv_ = std::ptr::dangling_mut() };
        }
        // SAFETY: zeroed is a hook in no list.
        let mut hook: spa::spa_hook = unsafe { std::mem::zeroed() };
        hook.removed = Some(told);
        // SAFETY: never added: its link is null.
        unsafe { remove(&raw mut hook) };
        assert!(hook.link.next.is_null() && !hook.priv_.is_null());
    }

    /// Adding zeroes a hook, so no `removed` of ours unlinks it: a second removal must touch nothing, even once its
    /// old neighbours have moved on.
    #[test]
    fn a_hook_removed_twice_leaves_its_list_alone() {
        let mut list = spa::spa_list { prev: std::ptr::null_mut(), next: std::ptr::null_mut() };
        // SAFETY: plain data; zeroed is an empty hook.
        let mut first: spa::spa_hook = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        let mut second: spa::spa_hook = unsafe { std::mem::zeroed() };
        let list_at: *mut spa::spa_list = &raw mut list;
        // SAFETY: a live local; this is `spa_list_init`.
        unsafe { *list_at = spa::spa_list { prev: list_at, next: list_at } };
        let (first_at, second_at): (*mut spa::spa_hook, *mut spa::spa_hook) = (&raw mut first, &raw mut second);
        linked(list_at, first_at);
        linked(list_at, second_at);
        // SAFETY: added above, and nothing else walks the list.
        unsafe { remove(first_at) };
        // SAFETY: as above.
        unsafe { remove(second_at) };
        // SAFETY: as above; `first` was removed already.
        unsafe { remove(first_at) };
        // SAFETY: as above.
        let head = unsafe { *list_at };
        assert_eq!((head.next, head.prev), (list_at, list_at), "the list is empty");
    }
}
