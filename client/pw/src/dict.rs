//! PipeWire's dictionaries: one read in a callback, and one built for a call that takes one.

use std::ffi::{CStr, CString, c_char};

use libspa_sys as spa;

use crate::graph::Props;
use crate::session::Error;

/// A dictionary PipeWire handed to a callback, valid for that call.
pub(crate) struct Borrowed<'a>(&'a [spa::spa_dict_item]);

impl Borrowed<'_> {
    /// # Safety
    /// `dict` is null or a dictionary that outlives the returned value.
    pub(crate) unsafe fn new<'a>(dict: *const spa::spa_dict) -> Borrowed<'a> {
        // SAFETY: as the caller says.
        let Some(dict) = (unsafe { dict.as_ref() }) else {
            return Borrowed(&[]);
        };
        if dict.items.is_null() {
            return Borrowed(&[]);
        }
        // SAFETY: as the caller says; a dictionary's `items` holds `n_items` entries.
        Borrowed(unsafe { std::slice::from_raw_parts(dict.items, dict.n_items as usize) })
    }
}

impl Props for Borrowed<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find_map(|item| {
            // SAFETY: a dictionary's keys and values are NUL-terminated strings, or null, alive as long as it is.
            let name = unsafe { text(item.key) };
            // SAFETY: as above.
            let value = unsafe { text(item.value) };
            (name? == key.as_bytes()).then(|| std::str::from_utf8(value?).ok()).flatten()
        })
    }
}

/// A dictionary of our own, for a call that copies what it needs from it.
pub(crate) struct Owned {
    _strings: Vec<CString>,
    items: Vec<spa::spa_dict_item>,
}

impl Owned {
    pub(crate) fn new(pairs: &[(&CStr, String)]) -> Result<Self, Error> {
        let values = pairs
            .iter()
            .map(|(_, value)| CString::new(value.as_str()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Error::Refused("name a property"))?;
        let items = pairs
            .iter()
            .zip(&values)
            .map(|((key, _), value)| spa::spa_dict_item { key: key.as_ptr(), value: value.as_ptr() })
            .collect();
        Ok(Self { _strings: values, items })
    }

    /// The dictionary, valid while `self` is.
    pub(crate) fn raw(&self) -> spa::spa_dict {
        spa::spa_dict {
            flags: 0,
            n_items: u32::try_from(self.items.len()).unwrap_or(u32::MAX),
            items: self.items.as_ptr(),
        }
    }
}

/// The type names of the objects this crate binds and makes: PipeWire's protocol strings, the same in every release.
/// Named here because the headers build them of nested macros, which bindgen does not evaluate on every system.
pub(crate) mod interface {
    use std::ffi::CStr;

    pub(crate) const NODE: &CStr = c"PipeWire:Interface:Node";
    pub(crate) const PORT: &CStr = c"PipeWire:Interface:Port";
    pub(crate) const LINK: &CStr = c"PipeWire:Interface:Link";
    pub(crate) const CLIENT: &CStr = c"PipeWire:Interface:Client";
}

/// # Safety
/// `at` is null or a NUL-terminated string that outlives the returned bytes.
pub(crate) unsafe fn text<'a>(at: *const c_char) -> Option<&'a [u8]> {
    // SAFETY: as the caller says.
    (!at.is_null()).then(|| unsafe { CStr::from_ptr(at) }.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_built_dictionary_is_read_back_by_key() {
        let owned =
            Owned::new(&[(c"link.output.port", "41".to_owned()), (c"object.linger", "false".to_owned())]).unwrap();
        let raw = owned.raw();
        // SAFETY: `raw` points into `owned`, alive for this test.
        let read = unsafe { Borrowed::new(&raw const raw) };
        assert_eq!(read.get("link.output.port"), Some("41"));
        assert_eq!(read.get("object.linger"), Some("false"));
        assert_eq!(read.get("link.input.port"), None);
    }

    #[test]
    fn a_value_with_a_nul_is_refused() {
        assert!(Owned::new(&[(c"node.name", "a\0b".to_owned())]).is_err());
    }
}
