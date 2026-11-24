//! A list the server gives a page at a time: the items read so far, and where the next page starts.

/// Where a page after the first starts, exactly as the server gave it: never made here, only handed back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor(String);

impl Cursor {
    /// The cursor as the wire carries it.
    pub fn wire(&self) -> &str {
        &self.0
    }

    /// The cursor as the wire carries it, to hand back.
    pub fn into_wire(self) -> String {
        self.0
    }
}

impl From<String> for Cursor {
    fn from(wire: String) -> Cursor {
        Cursor(wire)
    }
}

impl From<&str> for Cursor {
    fn from(wire: &str) -> Cursor {
        Cursor(wire.to_owned())
    }
}

/// A list read page by page: the items read so far, in the collection the reader keeps them in, and where the page
/// after them starts, `K`: the server's opaque [`Cursor`], or the key of the last item read where the server pages
/// from one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paged<C, K = Cursor> {
    items: C,
    next: Option<K>,
}

impl<C: Default, K> Default for Paged<C, K> {
    fn default() -> Self {
        Paged { items: C::default(), next: None }
    }
}

impl<C: Default, K> Paged<C, K> {
    /// A list with nothing read yet.
    pub fn new() -> Paged<C, K> {
        Paged::default()
    }
}

impl<C, K> Paged<C, K> {
    /// A page read: its items join the list, and `next` is where the page after it starts, none once the last page
    /// is in.
    pub fn extend<I>(&mut self, items: impl IntoIterator<Item = I>, next: Option<K>)
    where
        C: Extend<I>,
    {
        self.items.extend(items);
        self.next = next;
    }

    /// Whether the server has more pages after the last read.
    pub fn more(&self) -> bool {
        self.next.is_some()
    }

    /// Where the page after the last read starts, while there is one.
    pub fn next(&self) -> Option<&K> {
        self.next.as_ref()
    }

    /// The items read so far.
    pub fn items(&self) -> &C {
        &self.items
    }

    /// The items read so far, to change as events tell them.
    pub fn items_mut(&mut self) -> &mut C {
        &mut self.items
    }

    /// The whole list, once every page is read.
    pub fn into_items(self) -> C {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_join_in_order_and_the_last_one_ends_the_list() {
        let mut list: Paged<Vec<u8>> = Paged::new();
        assert!(!list.more());
        list.extend([1, 2], Some("2".into()));
        assert_eq!(list.next(), Some(&Cursor::from("2")));
        list.extend([3], None);
        assert!(!list.more());
        assert_eq!(list.into_items(), [1, 2, 3]);
    }
}
