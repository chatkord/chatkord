//! What this account sends of itself: its screen's look, its sound and its camera.

use super::*;

impl Call {
    /// This account's own share's sound, while sharing and once the call has said.
    pub fn own_sound(&self) -> Option<&SoundSaid> {
        self.sharing.as_ref()?.sound.as_ref()
    }

    /// The applications the own window share's sound may be picked from, and the one it sends, once the host listed
    /// them for a window whose owner was not found.
    pub fn own_sound_apps(&self) -> Option<(&[SoundApp], Option<&str>)> {
        let sharing = self.sharing.as_ref()?;
        let picked = match &sharing.sound {
            Some(SoundSaid::App(name)) => Some(name.as_str()),
            _ => None,
        };
        sharing.apps.as_deref().map(|apps| (apps, picked))
    }

    /// What the connection popover shows of this account's own share and camera: the one sent alone, or a tab each
    /// and the one picked; none until the call has looked at one.
    pub fn own_video(&self) -> Option<OwnVideo<'_>> {
        match (self.outgoing(), self.own_camera.outgoing()) {
            (Some(share), Some(camera)) => {
                let outgoing = match self.own_shown {
                    Video::Screen => share,
                    Video::Camera => camera,
                };
                Some(OwnVideo::Tabbed { shown: self.own_shown, outgoing, watch: self.own_watch(self.own_shown) })
            }
            (Some(share), None) => Some(OwnVideo::Alone(Video::Screen, share, self.own_watch(Video::Screen))),
            (None, Some(camera)) => Some(OwnVideo::Alone(Video::Camera, camera, self.own_watch(Video::Camera))),
            (None, None) => None,
        }
    }

    /// Whether anyone takes this account's own picture of `video`; taken until the server says otherwise.
    pub fn own_watch(&self, video: Video) -> OwnWatch {
        self.own_watch.get(&video).copied().unwrap_or_default()
    }

    /// The popover about this account's own screen as it goes out, once the call has looked: with its size alone and a
    /// note that it sends nothing while nobody watches.
    pub fn own_about(&self, words: &words::Words) -> Option<About> {
        let outgoing = self.outgoing()?;
        Some(About::sent_while(self.own_watch(Video::Screen), outgoing, self.own_sound(), words))
    }

    /// The popover about this account's own camera as it goes out, once the call has looked.
    pub fn own_camera_about(&self, words: &words::Words) -> Option<About> {
        let outgoing = self.own_camera.outgoing()?;
        Some(About::sent_camera(self.own_watch(Video::Camera), outgoing, words))
    }

    /// Whether this account's own camera fills its tile and the call has looked at how it goes out.
    pub(super) fn own_camera_pictured(&self) -> bool {
        self.own_camera.outgoing().is_some() && self.tiles.iter().any(|tile| tile.own() && tile.camera.pictured())
    }
}

impl Place {
    /// The camera as the window keeps it chosen. A camera on, or opening, is opened again with a new ask; the person is
    /// told nothing until it is sent.
    pub(crate) fn set_camera(&mut self, settings: &choices::CameraSettings) -> Option<Ask> {
        let ask = settings.ask();
        let changed = self.camera_asked != ask;
        self.camera_asked = ask;
        self.mirror = settings.mirror;
        let Status::Signed(signed) = &mut self.status else { return None };
        let call = signed.call.as_mut().filter(|call| changed && call.live() && call.joined())?;
        match call.own_camera {
            OwnCamera::Off => None,
            OwnCamera::Starting | OwnCamera::On(_) => {
                call.own_camera = OwnCamera::Starting;
                Some(Ask::Call(Control::Camera(Some(self.camera_asked.clone()))))
            }
        }
    }

    /// Whether one's own camera is shown mirrored, as the window keeps it chosen.
    pub fn mirror(&self) -> choices::Mirror {
        self.mirror
    }

    /// This account's own camera in the call held here, as the call last told; off with no call.
    pub fn own_camera(&self) -> &OwnCamera {
        const OFF: &OwnCamera = &OwnCamera::Off;
        self.call().map_or(OFF, |call| &call.own_camera)
    }

    pub(super) fn toggle_camera(&mut self) -> Vec<Ask> {
        let may_camera = self.can_camera() == CanCamera::Yes;
        let camera_ask = self.camera_asked.clone();
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            Some(call) if call.live() && call.joined() => match call.own_camera {
                OwnCamera::Off if may_camera => {
                    call.own_camera = OwnCamera::Starting;
                    vec![Ask::Call(Control::Camera(Some(camera_ask.clone())))]
                }
                OwnCamera::Off => Vec::new(),
                OwnCamera::Starting | OwnCamera::On(_) => {
                    call.own_camera = OwnCamera::Off;
                    vec![Ask::Call(Control::Camera(None))]
                }
            },
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn camera_reported(&mut self, reported: CameraSent, told: &mut Option<Told>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.own_camera = match reported {
                CameraSent::Started { .. } => {
                    call.faults.clear(Device::Camera);
                    OwnCamera::On(None)
                }
                // A look that lands after the camera was let go says nothing of it.
                CameraSent::Looked(outgoing) => match call.own_camera {
                    OwnCamera::On(_) => OwnCamera::On(Some(outgoing)),
                    OwnCamera::Off | OwnCamera::Starting => return Vec::new(),
                },
                CameraSent::Stopped => OwnCamera::Off,
                CameraSent::Failed(failed) => {
                    let said = words::failure::of(&failed);
                    call.faults.set(Device::Camera, said.clone());
                    *told = Some(Told::Toast(Telling::of(design::Status::Danger, said).offering(Action::TurnOnCamera)));
                    OwnCamera::Off
                }
                CameraSent::Denied => {
                    let said = Sentence::Phrase(Phrase::CameraNotAllowed);
                    call.faults.set(Device::Camera, said.clone());
                    *told = Some(Told::Toast(Telling::of(design::Status::Danger, said)));
                    OwnCamera::Off
                }
            };
        }
        Vec::new()
    }

    pub(super) fn show_own(&mut self, video: Video) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.own_shown = video;
        }
        Vec::new()
    }

    pub(super) fn own_watched(&mut self, video: Video, watched: OwnWatch) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.own_watch.insert(video, watched);
        }
        Vec::new()
    }

    pub(super) fn sound_apps_listed(&mut self, apps: Vec<SoundApp>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(sharing) = signed.call.as_mut().and_then(|call| call.sharing.as_mut()) {
            sharing.apps = Some(apps);
        }
        Vec::new()
    }

    pub(super) fn pick_sound_app(&mut self, app: SoundApp) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_ref().and_then(|call| call.sharing.as_ref()) {
            Some(sharing) if sharing.apps.as_ref().is_some_and(|apps| apps.contains(&app)) => {
                vec![Ask::Call(Control::ShareSoundOf(app))]
            }
            Some(_) | None => Vec::new(),
        }
    }
}
