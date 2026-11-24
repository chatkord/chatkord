//! Our own picker's stills as image handles, by their id, made once when the errands list the sources: a refresh
//! replaces them all.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use iced_widget::image;
use screens::Painting;

/// The handles of the pictures the picker shows now, by their id.
#[derive(Clone, Default)]
pub(crate) struct Stills(Arc<Mutex<HashMap<Painting, image::Handle>>>);

impl Stills {
    pub(crate) fn get(&self, painting: Painting) -> Option<image::Handle> {
        self.0.lock().ok().and_then(|held| held.get(&painting).cloned())
    }

    /// Holds `made` in place of every picture held before.
    pub(crate) fn replace(&self, made: Vec<errands::Still>) {
        let made = made
            .into_iter()
            .map(|still| (still.painting, image::Handle::from_rgba(still.width, still.height, still.rgba)))
            .collect();
        if let Ok(mut held) = self.0.lock() {
            *held = made;
        }
    }
}
