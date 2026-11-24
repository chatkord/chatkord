//! This account's own share: how it goes out, what the panel and the picker choose, and what the share reports.

use super::*;

/// This account's own share, while there is one.
#[derive(Debug)]
pub(super) struct Sharing {
    sending: Sending,
    pub(super) preview: Preview,
    /// Its sound, once the call has said.
    pub(super) sound: Option<SoundSaid>,
    /// The applications playing sound, listed by the host for a window whose owner was not found, for the sharer to
    /// pick one; none until listed.
    pub(super) apps: Option<Vec<SoundApp>>,
}

/// What the call does of this account's own screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Sending {
    /// Nothing is shared.
    #[default]
    No,
    /// The screen is picked and its first picture is awaited.
    Starting,
    /// The screen goes out, and how it is going once the call has said.
    Live(Option<Outgoing>),
}

impl Call {
    /// How the next share goes out, as the share panel shows it.
    pub fn quality(&self) -> &ShareQuality {
        &self.quality
    }

    /// The share panel's Bitrate row: the preset chosen, or Custom with its field's text, which its slider follows.
    pub fn rate(&self) -> &PresetOrCustom<Ceiling> {
        &self.rate
    }

    /// The note under the custom rate: the ceiling it makes, or why the rate typed makes none; none for a preset or
    /// a field left empty.
    pub fn rate_note(&self) -> Option<Sentence> {
        match (&self.rate, self.rate.value()) {
            (PresetOrCustom::Preset(_), _) | (PresetOrCustom::Custom(_), Err(TypedRefused::Empty)) => None,
            (PresetOrCustom::Custom(_), Ok(ceiling)) => ceiling.bits_per_second().map(Sentence::ShareRateAtMost),
            (PresetOrCustom::Custom(_), Err(TypedRefused::OutOfBounds)) => Some(Sentence::ShareRateOutOfRange {
                least: Kbits::LEAST.bits_per_second(),
                most: Kbits::MOST.bits_per_second(),
                step: u64::from(Kbits::STEP) * 1_000,
            }),
        }
    }

    /// A share may start from the panel: its custom rate, if one is chosen, is one it takes.
    fn rate_taken(&self) -> bool {
        self.rate.value().is_ok()
    }

    /// What the call does of this account's own screen.
    pub fn sending(&self) -> &Sending {
        self.sharing.as_ref().map_or(&Sending::No, |sharing| &sharing.sending)
    }

    /// How this account's own screen is going, once the call has said.
    pub fn outgoing(&self) -> Option<&Outgoing> {
        match self.sending() {
            Sending::Live(outgoing) => outgoing.as_ref(),
            Sending::No | Sending::Starting => None,
        }
    }

    /// This account's screen no longer goes out, and what was open about it is closed.
    fn stop_sending(&mut self) {
        self.sharing = None;
        if matches!(self.open, Some(Opened::OwnPicture)) {
            self.open = None;
        }
    }
}

impl Place {
    /// Which picker a share is chosen in here.
    pub(crate) fn set_chooser(&mut self, chooser: Chooser) {
        self.chooser = chooser;
    }

    /// Whether this device has an encoder for a screen, as the window found out.
    pub(crate) fn set_encodes(&mut self, encodes: bool) {
        self.encodes = encodes;
    }

    /// How a share starts from here, as Settings keeps it.
    pub(crate) fn set_share_starts(&mut self, quality: ShareQuality) {
        self.share_starts = quality;
    }

    pub(super) fn toggle_share(&mut self) -> Vec<Ask> {
        let may_share = self.can_share() == Can::Yes;
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            Some(call) if call.live() && call.joined() => match call.sharing {
                None => {
                    if may_share {
                        call.open = if matches!(call.open, Some(Opened::Share)) { None } else { Some(Opened::Share) };
                        call.quality = self.share_starts.clone();
                        call.rate = call.quality.ceiling.choice();
                    }
                    Vec::new()
                }
                Some(_) => {
                    call.stop_sending();
                    vec![Ask::Call(Control::StopShare)]
                }
            },
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn choose_screen(&mut self) -> Vec<Ask> {
        let may_share = self.can_share() == Can::Yes;
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            Some(call) if may_share && call.live() && call.joined() && call.sharing.is_none() && call.rate_taken() => {
                match self.chooser {
                    Chooser::Desktop => {
                        call.open = None;
                        vec![Ask::ChooseScreen(call.quality.clone())]
                    }
                    Chooser::Own => {
                        call.open = Some(Opened::Sources(Sources::default()));
                        vec![Ask::WatchSources]
                    }
                }
            }
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn sources_listed(&mut self, listed: Vec<Source>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(Opened::Sources(sources)) = signed.call.as_mut().and_then(|call| call.open.as_mut()) {
            sources.listed(listed);
        }
        Vec::new()
    }

    pub(super) fn show_sources_tab(&mut self, tab: Tab) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(Opened::Sources(sources)) = signed.call.as_mut().and_then(|call| call.open.as_mut()) {
            sources.show(tab);
        }
        Vec::new()
    }

    pub(super) fn select_source(&mut self, key: SourceKey) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(Opened::Sources(sources)) = signed.call.as_mut().and_then(|call| call.open.as_mut()) {
            sources.select(key);
        }
        Vec::new()
    }

    pub(super) fn share_source(&mut self) -> Vec<Ask> {
        let may_share = self.can_share() == Can::Yes;
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            Some(call) if may_share && call.live() && call.joined() && call.sharing.is_none() && call.rate_taken() => {
                let Some(Opened::Sources(sources)) = &call.open else { return Vec::new() };
                match sources.selected().cloned() {
                    Some(key) => {
                        call.open = None;
                        vec![Ask::StopSources, Ask::ShareSource { key, quality: call.quality.clone() }]
                    }
                    None => Vec::new(),
                }
            }
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn pick_quality(&mut self, pick: QualityPick) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| matches!(call.open, Some(Opened::Share))) {
            call.quality.pick(pick);
            if let QualityPick::Ceiling(ceiling) = pick {
                call.rate = ceiling.choice();
            }
        }
        Vec::new()
    }

    pub(super) fn pick_rate(&mut self, message: RateMessage) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| matches!(call.open, Some(Opened::Share))) {
            call.rate = match message {
                RateMessage::Custom => call.rate.custom(),
                RateMessage::Typed(typed) => PresetOrCustom::typed(typed),
                RateMessage::Slid(at) => PresetOrCustom::slid(u32::from(at)),
            };
            if let Ok(ceiling) = call.rate.value() {
                call.quality.ceiling = ceiling;
            }
        }
        Vec::new()
    }

    pub(super) fn share_starting(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.live()) {
            call.sharing =
                Some(Sharing { sending: Sending::Starting, preview: Preview::Waiting, sound: None, apps: None });
            call.faults.clear(Device::Share);
        }
        Vec::new()
    }

    pub(super) fn share_reported(&mut self, reported: Shared, told: &mut Option<Told>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            match reported {
                Shared::Started { .. } => {
                    call.faults.clear(Device::Share);
                    let (sound, apps) =
                        call.sharing.take().map_or((None, None), |sharing| (sharing.sound, sharing.apps));
                    call.sharing = Some(Sharing { sending: Sending::Live(None), preview: Preview::Shown, sound, apps });
                }
                Shared::Looked(outgoing) => {
                    if let Some(sharing) = &mut call.sharing {
                        sharing.sending = Sending::Live(Some(outgoing));
                    }
                }
                Shared::Failed(failed) => {
                    call.stop_sending();
                    let said = Sentence::ShareStopped(failed);
                    call.faults.set(Device::Share, said.clone());
                    *told = Some(Told::Toast(Telling::of(design::Status::Danger, said).offering(Action::ShareAgain)));
                }
                Shared::Stopped => call.stop_sending(),
                Shared::Denied => {
                    call.stop_sending();
                    let said = Sentence::Phrase(Phrase::ShareDenied);
                    call.faults.set(Device::Share, said.clone());
                    *told = Some(Told::Toast(Telling::of(design::Status::Danger, said)));
                }
                Shared::Unshown => {
                    if let Some(sharing) = &mut call.sharing {
                        sharing.preview = Preview::Unshown;
                    }
                }
                Shared::HandedOver(route) => {
                    let said = route.handed_over();
                    *told = Some(Told::Toast(Telling::of(design::Status::Warning, said)));
                }
                Shared::Sound(said) => {
                    let unfound = said == SoundSaid::NotFound;
                    if let Some(sharing) = &mut call.sharing {
                        sharing.sound = Some(said);
                        if unfound {
                            return vec![Ask::ListSoundApps];
                        }
                    }
                }
            }
        }
        Vec::new()
    }
}
