//! The tray icon: the desktop's StatusNotifierItem on Linux, the notification area's icon on Windows, given its name
//! and menu as values and telling which entry was picked. What it is called, which entries there are, what they say
//! and what each does is decided by the caller.

use crate::Unavailable;
use screens::{TrayMenu, TrayPick};
use std::sync::Arc;

#[cfg(windows)]
mod windows;

/// One size of the icon: `side` pixels square, RGBA row by row.
pub struct Picture {
    /// Its width and height.
    pub side: u32,
    /// Its pixels, four bytes each.
    pub rgba: Vec<u8>,
}

/// What the icon tells of a pick, on the tray's own thread: it must not block.
pub type Picked = Arc<dyn Fn(TrayPick) + Send + Sync>;

/// The icon, shown until this is dropped.
pub struct TrayIcon {
    #[cfg(target_os = "linux")]
    handle: ksni::Handle<linux::Icon>,
    #[cfg(windows)]
    icon: windows::Icon,
}

impl TrayIcon {
    /// Shows the icon named and with the menu `menu` holds. A desktop that shows no tray (no StatusNotifierWatcher
    /// answers, no notification area takes the icon) is [`Unavailable`], and so is a system without one here yet.
    pub async fn show(menu: TrayMenu, pictures: Vec<Picture>, picked: Picked) -> Result<TrayIcon, Unavailable> {
        #[cfg(target_os = "linux")]
        {
            use ksni::TrayMethods;
            let icon = linux::Icon { menu, pictures: pictures.iter().map(linux::argb).collect(), picked };
            icon.spawn().await.map(|handle| TrayIcon { handle }).map_err(|_| Unavailable)
        }
        #[cfg(windows)]
        {
            crate::blocking(move || windows::Icon::show(menu, &pictures, picked)).await.map(|icon| TrayIcon { icon })
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (menu, pictures, picked);
            Err(Unavailable)
        }
    }

    /// Shows `menu`'s name and entries in place of those shown.
    pub async fn update(&self, menu: TrayMenu) {
        #[cfg(target_os = "linux")]
        self.handle.update(move |icon| icon.menu = menu).await;
        #[cfg(windows)]
        self.icon.update(menu);
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = menu;
    }
}

#[cfg(target_os = "linux")]
impl Drop for TrayIcon {
    fn drop(&mut self) {
        drop(self.handle.shutdown());
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use ksni::menu::{CheckmarkItem, StandardItem};
    use ksni::{MenuItem, Tray};

    pub(super) struct Icon {
        pub(super) menu: TrayMenu,
        pub(super) pictures: Vec<ksni::Icon>,
        pub(super) picked: Picked,
    }

    /// RGBA as the item's ARGB, alpha first.
    pub(super) fn argb(picture: &Picture) -> ksni::Icon {
        let data = picture.rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect();
        let side = i32::try_from(picture.side).unwrap_or(0);
        ksni::Icon { width: side, height: side, data }
    }

    impl Tray for Icon {
        fn id(&self) -> String {
            "chatkord".into()
        }

        fn title(&self) -> String {
            self.menu.title.clone()
        }

        fn icon_name(&self) -> String {
            "chatkord".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.pictures.clone()
        }

        /// A press on the icon itself opens the window, as the menu's first entry does.
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.picked)(TrayPick::Open);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            self.menu
                .items
                .iter()
                .map(|item| {
                    let pick = item.pick;
                    let activate = Box::new(move |icon: &mut Self| (icon.picked)(pick));
                    match item.ticked {
                        Some(checked) => {
                            CheckmarkItem { label: item.label.clone(), checked, activate, ..Default::default() }.into()
                        }
                        None => StandardItem { label: item.label.clone(), activate, ..Default::default() }.into(),
                    }
                })
                .collect()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_icon_is_named_and_lists_its_entries_as_the_window_says() {
            let item = |pick, label: &str, ticked| screens::TrayItem { pick, label: label.into(), ticked };
            let menu = TrayMenu {
                title: "Chatkord (essai)".into(),
                items: vec![item(TrayPick::Open, "Ouvrir Chatkord", None), item(TrayPick::Mute, "Muet", Some(true))],
            };
            let icon = Icon { menu, pictures: Vec::new(), picked: Arc::new(|_| {}) };
            let labels: Vec<(String, Option<bool>)> = icon
                .menu()
                .into_iter()
                .filter_map(|item| match item {
                    MenuItem::Standard(standard) => Some((standard.label, None)),
                    MenuItem::Checkmark(check) => Some((check.label, Some(check.checked))),
                    _ => None,
                })
                .collect();
            assert_eq!(icon.title(), "Chatkord (essai)");
            assert_eq!(labels, [("Ouvrir Chatkord".to_string(), None), ("Muet".to_string(), Some(true))]);
        }

        #[test]
        fn a_picture_is_handed_over_alpha_first() {
            let picture = Picture { side: 1, rgba: vec![10, 20, 30, 255] };
            let icon = argb(&picture);
            assert_eq!((icon.width, icon.height, icon.data), (1, 1, vec![255, 10, 20, 30]));
        }
    }
}
