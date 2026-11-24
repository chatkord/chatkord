//! Where the main window stood, kept for the next run as Discord and Slack keep it: its size, whether it was
//! maximised, and its place on the monitor it was on, in the kept file's `window` section. A monitor that is no longer there as it was
//! (unplugged, moved, rescaled) gives the place up, and the system places the window at its size, held to the
//! largest monitor. Read before any runtime runs and written as the window goes, so both block, on a small file.

use crate::Platform;
use crate::state::{change_kept_now, read_kept_now};
use serde::{Deserialize, Serialize};

const FILE: &str = "window";

/// A monitor as the window system lists it: its place and size in physical pixels, and its scale.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Monitor {
    /// Its left edge on the desktop.
    pub x: i32,
    /// Its top edge on the desktop.
    pub y: i32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// Physical pixels to a logical one.
    pub scale: f64,
}

/// The main window's place: what it opens at next run.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Placement {
    /// Its inner width in logical pixels, as it was when last neither maximised nor over the whole monitor.
    pub width: f64,
    /// Its inner height, likewise.
    pub height: f64,
    /// Its outer top-left corner in physical pixels; none where the system places windows itself.
    pub at: Option<(i32, i32)>,
    /// Whether it was maximised.
    pub maximized: bool,
    /// The monitor it stood on, which `at` means a place on.
    pub monitor: Option<Monitor>,
}

/// The section: nothing until a window has gone once.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Held {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    main: Option<Placement>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

/// The window's place as it went last, fitted to `monitors`, the ones there now; none before a first run ends.
pub fn placement(platform: &Platform, monitors: &[Monitor]) -> Option<Placement> {
    read_kept_now::<Held>(platform).main.map(|kept| fitted(kept, monitors))
}

/// Keeps `placement` for the next run.
pub fn keep(platform: &Platform, placement: Placement) -> Result<(), crate::Unavailable> {
    change_kept_now(platform, |held: &mut Held| held.main = Some(placement))
}

/// `kept` where it was, while its monitor is there as it was; else at its size, held to the largest monitor, with no
/// place, so the system puts it on one.
fn fitted(kept: Placement, monitors: &[Monitor]) -> Placement {
    if kept.monitor.is_some_and(|monitor| monitors.contains(&monitor)) {
        return kept;
    }
    let largest = |side: fn(&Monitor) -> f64| {
        monitors.iter().map(side).fold(None, |most: Option<f64>, side| Some(most.map_or(side, |most| most.max(side))))
    };
    let width = largest(|monitor| f64::from(monitor.width) / monitor.scale).map_or(kept.width, |w| kept.width.min(w));
    let height =
        largest(|monitor| f64::from(monitor.height) / monitor.scale).map_or(kept.height, |h| kept.height.min(h));
    Placement { width, height, at: None, monitor: None, ..kept }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{raw_kept_in, read_kept_in, write_kept_in};

    const WIDE: Monitor = Monitor { x: 0, y: 0, width: 2560, height: 1440, scale: 1.0 };
    const BESIDE: Monitor = Monitor { x: 2560, y: 0, width: 1920, height: 1080, scale: 1.5 };

    fn on_beside() -> Placement {
        Placement { width: 1100.0, height: 700.0, at: Some((2700, 100)), maximized: false, monitor: Some(BESIDE) }
    }

    #[test]
    fn a_window_on_a_monitor_still_there_opens_where_it_was() {
        assert_eq!(fitted(on_beside(), &[WIDE, BESIDE]), on_beside());
    }

    #[test]
    fn a_window_whose_monitor_went_or_changed_is_placed_by_the_system_at_a_size_that_fits() {
        let gone = fitted(on_beside(), &[WIDE]);
        assert_eq!((gone.at, gone.monitor), (None, None), "no place on a monitor that is gone");
        assert_eq!((gone.width, gone.height), (1100.0, 700.0), "a size the monitor left holds");
        let rescaled = Monitor { scale: 1.0, ..BESIDE };
        assert_eq!(fitted(on_beside(), &[WIDE, rescaled]).at, None, "a monitor rescaled is another one");
        let small = Monitor { x: 0, y: 0, width: 1280, height: 720, scale: 2.0 };
        let held = fitted(on_beside(), &[small]);
        assert_eq!((held.width, held.height), (640.0, 360.0), "held to the largest monitor, in logical pixels");
        assert!(!held.maximized);
        let maximized = fitted(Placement { maximized: true, ..on_beside() }, &[WIDE]);
        assert!(maximized.maximized, "maximised it stays, wherever the system puts it");
    }

    #[test]
    fn a_placement_reads_back_as_kept_and_another_shape_is_none() {
        let folder = std::env::temp_dir().join(format!("chatkord-window-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        write_kept_in(&folder, &Held { main: Some(on_beside()) }).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).main, Some(on_beside()));
        raw_kept_in::<Held>(&folder, r#"{"main":"elsewhere"}"#).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).main, None, "a file of another shape is none, not a failure");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
