//! A value PipeWire holds a pointer into: a listener's hook, its event table, its callbacks' state.

use std::ptr::NonNull;

/// One heap allocation whose address PipeWire keeps. Rust reaches it only through the raw pointer it was made with,
/// so moving the holder to another place or thread never invalidates what PipeWire holds, as moving a `Box` would.
pub(crate) struct Lent<T>(NonNull<T>);

impl<T> Lent<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(NonNull::from(Box::leak(Box::new(value))))
    }

    /// The pointer to hand to PipeWire, and the only way the value is read or written while PipeWire may use it.
    pub(crate) fn as_ptr(&self) -> *mut T {
        self.0.as_ptr()
    }
}

impl<T> Drop for Lent<T> {
    fn drop(&mut self) {
        // SAFETY: made from a `Box` in `new` and freed only here. Its holder drops it only once PipeWire holds no
        // pointer into it: a listener's hook taken out (`hook::remove`), or the object that held it gone.
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_lent_stays_where_it_was_lent_when_its_holder_moves() {
        let lent = Lent::new(7u32);
        let handed = lent.as_ptr();
        let moved = [lent];
        // SAFETY: the pointer was made from this allocation, which the moved holder still keeps.
        unsafe { *handed += 1 };
        assert_eq!(moved[0].as_ptr(), handed);
        // SAFETY: as above.
        assert_eq!(unsafe { *moved[0].as_ptr() }, 8);
    }
}
