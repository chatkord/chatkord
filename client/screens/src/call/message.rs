//! What is done to a call, and what its room, its host and its person tell it.

use super::*;

/// What is done to the share panel's custom bitrate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RateMessage {
    /// Custom pressed: its field starts at the rate now chosen, or keeps what is typed.
    Custom,
    /// The field's whole text after an edit, in Mbit/s.
    Typed(String),
    /// The slider moved to this step, [`Kbits::STEP`] apart from [`Kbits::LEAST`]'s.
    Slid(u16),
}

/// The call: its room, its controls, its shares and what is open over it.
#[derive(Clone, Debug)]
pub enum CallMessage {
    /// What the room says, from the loop that joined it.
    Room(RoomEvent),
    /// Leave in the call: the call goes when its loop returns.
    Leave,
    /// The call could not do what was asked, or its devices changed, in the sentence to say: a toast.
    Notice(Subject, Sentence),
    /// A device of `Subject` the call said did not answer opened, with nothing more to say: its mark is cleared.
    Answered(Subject),
    /// The call's devices, as it last found them.
    Sound(Sound),
    /// How the connection is, from the call's loop every second.
    Health(Health),
    /// The arrow beside mute: the menu of microphones and speakers opens, or closes.
    Devices,
    /// The signal bars: the popover of how the connection is opens, or closes.
    Stats,
    /// The slider in that popover moves, to a whole percent: their sound plays at it at once.
    Volume(MemberId, Audio, u16),
    /// The slider is let go, or a key moved it, to a whole percent: their sound plays at it and it is kept for the next
    /// run.
    KeepVolume(MemberId, Audio, u16),
    /// Volume on the card of someone's shared screen: the popover of how loud its sound plays here opens, or closes.
    ShareSound(MemberId),
    /// The call header's Audio controls: the panel of every sound in the call opens, or closes.
    AudioControls,
    /// Voice options open, or close.
    ViewMenu,
    /// Mute for me beside a volume: that sound is silenced on this device and kept so, or played again at the volume it
    /// had before this run's mute, else at full.
    MuteForMe(MemberId, Audio),
    /// A press outside what is open over the call, or Escape.
    CloseOver,
    /// A tab of the connection popover while one's share and camera are both sent: the one it shows.
    ShowOwn(Video),
    /// The share button: the panel that picks a screen opens, or the share stops.
    ToggleShare,
    /// The camera button: the camera turns on, or off.
    ToggleCamera,
    /// A row of that panel: how this share goes out.
    Quality(QualityPick),
    /// The panel's custom bitrate: Custom pressed, its field typed in or its slider moved.
    Rate(RateMessage),
    /// "Choose a screen or window…" in that panel, and a notice's "Share again".
    ChooseScreen,
    /// What the host lists for our own picker, again every few seconds while it is open.
    Sources(Vec<Source>),
    /// A tab of our own picker.
    SourcesTab(Tab),
    /// A card of our own picker: it is selected.
    SelectSource(SourceKey),
    /// Our own picker's Share: the selected source is shared.
    ShareSource,
    /// The desktop's picker gave a screen, which the call is to begin sending.
    Starting,
    /// What this account's own screen share reports.
    Shared(Shared),
    /// What this account's own camera reports.
    Camera(CameraSent),
    /// Pin a card: it fills the stage and the others go to the strip, in place of one pinned before.
    Pin(Pinned),
    /// Put the pinned card back in the grid.
    Unpin,
    /// The stage's toggle under a pinned card, on a stage of this height: the strip of the other cards is put away, or
    /// shown again, there.
    ToggleStrip(Stage),
    /// Full screen on the card of the screen of the person with this id, or a double-click on it: it fills the whole
    /// display, and what was open over the call closes.
    FullScreen(MemberId),
    /// Escape, Leave full screen or a double-click in full screen, or the system ended it: the window is as it was.
    LeaveFullScreen,
    /// The pointer moved or a key was pressed in full screen: its controls and the pointer show again for a while.
    Stirred,
    /// Zoom on the screen of the person with this id, shown large: the wheel and a drag zoom and move it, or it goes
    /// back to the whole picture.
    ToggleZoom(MemberId),
    /// The wheel over the zoomed screen: `notches` tenths in (positive) or out, toward the pointer `at`, in the pixels
    /// of the picture's area `area` large.
    ZoomWheel {
        /// Tenths of the picture's size, in or out.
        notches: i8,
        /// The pointer, from the area's top left.
        at: (f32, f32),
        /// The area's width and height.
        area: (f32, f32),
    },
    /// A drag over the zoomed screen moved by `by`, in the pixels of the picture's area `area` large: the picture
    /// follows it.
    Pan {
        /// How far the pointer moved.
        by: (f32, f32),
        /// The area's width and height.
        area: (f32, f32),
    },
    /// Picture in picture on the card of the person's screen or camera: it plays in a window of its own, in place of
    /// one there before, and its card says so.
    PopOut(MemberId, Video),
    /// Close picture in picture on its card, Close in its window, or the system closed that window: the screen plays in
    /// its card again.
    PopIn,
    /// Pause on a card, or Resume: the picture it pins, a person's camera or someone's screen, stops arriving here, or
    /// plays again. Nothing for a card that has no picture of another to hold.
    TogglePause(Pinned),
    /// Whether anyone takes this account's own screen or camera now, as the server told.
    OwnWatched {
        /// Which of this account's own pictures.
        video: Video,
        /// Whether anyone takes it.
        watched: OwnWatch,
    },
    /// The ⓘ on the screen of the person with this id.
    ToggleInfo(MemberId),
    /// The ⓘ on this account's own screen.
    ToggleOwnInfo,
    /// The ⓘ on this account's own camera.
    ToggleOwnCameraInfo,
    /// The applications playing sound now, as the host listed them for the own share's sound to be picked from.
    SoundApps(Vec<SoundApp>),
    /// The application whose sound the own window share sends, picked in its popover.
    PickSoundApp(SoundApp),
    /// What a screen shared here reports.
    Watched(Watched),
    /// The mute button: this account's microphone off, or on again.
    ToggleMute,
    /// The deafen button.
    ToggleDeafen,
    /// The host's loop for the call returned, once, and how it ended if it did not end by itself: what the call then holds
    /// is no longer anyone's but this window's.
    CallOver(Option<End>),
}
