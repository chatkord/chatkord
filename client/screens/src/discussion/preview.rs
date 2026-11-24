//! The preview of the first link of a draft: made by this device once typing pauses (the sender's client reads the
//! page, never the server or a reader), shown above the composer where it can be taken out, and sent with the message
//! when it is ready. A link whose preview was taken out stays without one in that draft.

use super::*;
use proto::chatkord_v1::LinkPreview;
use proto::rules::{LinkTarget, preview_shape, spans_shape};

/// How long typing must pause before the first link of a draft is read.
pub const PREVIEW_PAUSE: Duration = Duration::from_millis(700);

/// A link's preview as this device made it, to go with a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewMade {
    /// The link, an `https` address the message's text holds.
    pub url: String,
    /// The page's title.
    pub title: String,
    /// The page's description; empty for none.
    pub description: String,
    /// The page's picture; none for none.
    pub picture: Option<PreviewPicture>,
}

/// The picture of a [`PreviewMade`] preview, and what the server holds of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewPicture {
    /// Sent to the server for this message and not yet attached to one.
    Uploaded(FileId),
    /// The picture of a message already sent, which an edit sends again as a new file, as the one it held is released.
    Sent(FileId),
}

impl PreviewPicture {
    /// The file.
    pub fn file(self) -> FileId {
        match self {
            PreviewPicture::Uploaded(file) | PreviewPicture::Sent(file) => file,
        }
    }
}

impl PreviewMade {
    /// The link's host, as the one parse of a link both ends share reads it; empty for a link it does not read.
    pub fn host(&self) -> &str {
        LinkTarget::parse(&self.url).map_or("", |target| target.host())
    }

    /// The preview as the wire carries it, its picture being `file`.
    pub fn wire(&self, file: Option<FileId>) -> LinkPreview {
        LinkPreview {
            url: self.url.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            file: file.map(|file| file.as_bytes().to_vec()),
        }
    }

    /// Whether the words of it are within what the server takes, for a message that holds nothing but its link.
    pub fn shaped(&self) -> bool {
        use proto::chatkord_v1::{Link, Span, span};
        let end = u32::try_from(self.url.chars().count()).unwrap_or(u32::MAX);
        let link = Span { start: 0, end, kind: Some(span::Kind::Link(Link {})) };
        self.fits(&self.url, &[link])
    }

    /// Whether the server takes it for a message of `body` with `spans`: its link one of the links sent as links, its
    /// words within their bounds. One it would refuse is left off, so a preview never holds a message back.
    pub(super) fn fits(&self, body: &str, spans: &[proto::chatkord_v1::Span]) -> bool {
        let file = self.picture.map(PreviewPicture::file);
        spans_shape(body, spans).is_ok_and(|spans| preview_shape(&self.wire(file), body, &spans).is_ok())
    }
}

/// The first link of a draft, and what became of its preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Previewing {
    /// The link.
    pub(crate) link: String,
    /// Where it stands.
    pub(crate) stage: PreviewStage,
}

/// Where a draft's preview stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreviewStage {
    /// The link was typed at this moment; it is read once typing has paused.
    Waiting(Instant),
    /// The page is being read.
    Fetching,
    /// PreviewMade, and sent with the message.
    Ready(PreviewMade),
    /// The page gave none: nothing shows, and the message goes without.
    Failed,
}

/// What the card above the composer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkPreviewShown<'a> {
    /// The page is being read: the link's host.
    Loading(&'a str),
    /// PreviewMade.
    Ready(&'a PreviewMade),
}

impl Previewing {
    /// The link's host, as the one parse of a link both ends share reads it.
    fn host(&self) -> &str {
        LinkTarget::parse(&self.link).map_or("", |target| target.host())
    }
}

impl Place {
    /// This device's choice of previews, as the window last said; turned off, every draft's preview goes.
    pub(crate) fn set_previews(&mut self, previews: choices::Previews) {
        self.previews = previews;
        if !self.previews_wanted() {
            for held in self.held.values_mut() {
                held.composing.preview = None;
            }
        }
    }

    /// Whether the first link of a draft is read: this device's choice is on and the server serves previews.
    fn previews_wanted(&self) -> bool {
        self.previews == choices::Previews::On && self.served.on(proto::rules::Switch::LinkPreviews)
    }

    /// The card above the composer of `compose`: none without a link to read, or once taken out or failed.
    pub fn link_preview(&self, compose: &Compose) -> Option<LinkPreviewShown<'_>> {
        let preview = self.held.get(compose)?.composing.preview.as_ref()?;
        match &preview.stage {
            PreviewStage::Waiting(_) | PreviewStage::Fetching => Some(LinkPreviewShown::Loading(preview.host())),
            PreviewStage::Ready(made) => Some(LinkPreviewShown::Ready(made)),
            PreviewStage::Failed => None,
        }
    }

    /// The draft of `compose` changed: its first link is the one a preview is made of, started over when it is
    /// another, none when the draft holds no link, a taken-out one or this account may not send.
    pub(super) fn link_followed(&mut self, compose: &Compose, now: Instant, may_send: bool) {
        let wanted = self.previews_wanted() && may_send;
        let composing = &mut self.held.entry(compose.clone()).or_default().composing;
        let link = composing.draft.first_link().filter(|link| wanted && !composing.declined.contains(link));
        composing.preview = match (composing.preview.take(), link) {
            (Some(held), Some(link)) if held.link == link => Some(held),
            (_, Some(link)) => Some(Previewing { link, stage: PreviewStage::Waiting(now) }),
            (_, None) => None,
        };
    }

    /// What is owed after a change: each link whose draft paused is read, its picture sent too where this account
    /// may attach files.
    pub(super) fn previews_due(&mut self, now: Instant) -> Vec<Ask> {
        let due: Vec<(Compose, String)> = self
            .held
            .iter()
            .filter_map(|(compose, held)| match held.composing.preview.as_ref()? {
                Previewing { link, stage: PreviewStage::Waiting(at) }
                    if now.saturating_duration_since(*at) >= PREVIEW_PAUSE =>
                {
                    Some((compose.clone(), link.clone()))
                }
                Previewing { .. } => None,
            })
            .collect();
        let mut todo = Vec::new();
        for (compose, link) in due {
            let picture =
                self.owner_of_compose(&compose).is_some_and(|owner| self.grants_of(&owner).holds(Grant::AttachFiles));
            if let Some(preview) = self.held.get_mut(&compose).and_then(|held| held.composing.preview.as_mut()) {
                preview.stage = PreviewStage::Fetching;
            }
            todo.push(Ask::FetchPreview { compose, link, picture });
        }
        todo
    }

    /// When the next draft's link may be read.
    pub(crate) fn preview_due(&self) -> Option<Instant> {
        self.held
            .values()
            .filter_map(|held| match held.composing.preview.as_ref()?.stage {
                PreviewStage::Waiting(at) => Some(at + PREVIEW_PAUSE),
                PreviewStage::Fetching | PreviewStage::Ready(_) | PreviewStage::Failed => None,
            })
            .min()
    }

    /// What the host made of `link`, or none when it could not: kept for the draft that still holds that link and
    /// waits for it.
    pub(super) fn previewed(&mut self, compose: &Compose, link: &str, made: Option<PreviewMade>) {
        let Some(preview) = self.held.get_mut(compose).and_then(|held| held.composing.preview.as_mut()) else {
            return;
        };
        if preview.link == link && preview.stage == PreviewStage::Fetching {
            preview.stage = made.map_or(PreviewStage::Failed, PreviewStage::Ready);
        }
    }

    /// The card's way out pressed: the draft goes without a preview of that link.
    pub(super) fn preview_removed(&mut self, compose: &Compose) {
        let composing = &mut self.held.entry(compose.clone()).or_default().composing;
        if let Some(removed) = composing.preview.take() {
            composing.declined.push(removed.link);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{
        ADA, Clock, chatting, discussion, every_switch_on, general_compose, message_id, said, standing, tell,
    };
    use crate::place::PlaceId;
    use crate::window::{Ask as WindowAsk, Message as WindowMessage, Window};
    use choices::LookPick;
    use proto::rules::Grant;

    const URL: &str = "https://blog.example.invalid/game-night";

    fn made(picture: Option<PreviewPicture>) -> PreviewMade {
        PreviewMade { url: URL.into(), title: "Game night".into(), description: String::new(), picture }
    }

    fn link_span(body: &str) -> Vec<proto::chatkord_v1::Span> {
        use proto::chatkord_v1::{Link, Span, span};
        let start = body.split(URL).next().map_or(0, |head| u32::try_from(head.chars().count()).unwrap_or(0));
        let end = start + u32::try_from(URL.chars().count()).unwrap_or(0);
        vec![Span { start, end, kind: Some(span::Kind::Link(Link {})) }]
    }

    #[test]
    fn a_preview_fits_a_message_only_where_its_link_is_one_of_the_links_sent() {
        let body = format!("this week {URL}");
        assert!(made(None).fits(&body, &link_span(&body)));
        assert!(
            made(Some(PreviewPicture::Uploaded(FileId::from_bytes([7; FileId::LEN])))).fits(&body, &link_span(&body))
        );
        assert!(!made(None).fits(&body, &[]), "the link was sent as text");
        assert!(
            !PreviewMade { url: "https://other.example.invalid/".into(), ..made(None) }.fits(&body, &link_span(&body))
        );
        assert!(
            !PreviewMade { title: String::new(), ..made(None) }.fits(&body, &link_span(&body)),
            "a title is needed"
        );
    }

    #[test]
    fn a_preview_names_its_host_by_the_parse_of_its_link() {
        assert_eq!(made(None).host(), "blog.example.invalid");
        let waiting = Previewing { link: "https://[2001:db8::1]:8443/x".into(), stage: PreviewStage::Fetching };
        assert_eq!(waiting.host(), "[2001:db8::1]");
    }

    const TEXT: &str = "this week https://blog.example.invalid/game-night";

    fn fetches(todo: &[WindowAsk]) -> Vec<(String, bool)> {
        todo.iter()
            .filter_map(|done| match done {
                WindowAsk::At(_, Ask::FetchPreview { link, picture, .. }) => Some((link.clone(), *picture)),
                _ => None,
            })
            .collect()
    }

    fn after(window: &mut Window, clock: &mut Clock, by: Duration) -> Vec<WindowAsk> {
        clock.pass(by);
        window.update(WindowMessage::Tick, clock.at())
    }

    fn writing(window: &mut Window, clock: &Clock, owls: PlaceId, text: &str) -> Vec<WindowAsk> {
        tell(window, clock, owls, discussion(DiscussionMessage::Draft(text.into())))
    }

    fn serving() -> (Window, Clock, PlaceId) {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        every_switch_on(&mut window, &clock, owls);
        (window, clock, owls)
    }

    fn sent_with(todo: &[WindowAsk]) -> Option<Option<PreviewMade>> {
        todo.iter().find_map(|done| match done {
            WindowAsk::At(_, Ask::SendMessage { preview, .. }) => Some(preview.clone()),
            _ => None,
        })
    }

    fn shown(window: &Window) -> Option<bool> {
        match window.places()[0].link_preview(&general_compose())? {
            LinkPreviewShown::Loading(_) => Some(false),
            LinkPreviewShown::Ready(_) => Some(true),
        }
    }

    #[test]
    fn the_first_link_of_a_draft_is_read_once_typing_pauses_and_its_picture_only_where_files_may_attach() {
        let (mut window, mut clock, owls) = serving();
        let typed = writing(&mut window, &clock, owls, TEXT);
        assert!(fetches(&typed).is_empty(), "not while typing");
        assert_eq!(shown(&window), Some(false), "the card waits with its host");
        let due = clock.0 + PREVIEW_PAUSE;
        assert!(typed.iter().any(|done| matches!(done, WindowAsk::WakeAt(when) if *when == due)), "{typed:?}");
        assert!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE / 2)).is_empty());
        let asked = after(&mut window, &mut clock, PREVIEW_PAUSE / 2);
        assert_eq!(fetches(&asked), [(URL.to_string(), false)], "no word to attach: no picture");
        assert!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE)).is_empty(), "asked once");
        let (mut window, mut clock, owls) = serving();
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::SendMessage, Grant::AttachFiles]);
        writing(&mut window, &clock, owls, TEXT);
        assert_eq!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE)), [(URL.to_string(), true)]);
    }

    #[test]
    fn only_the_first_link_counts_and_a_link_typed_over_starts_the_pause_again() {
        let (mut window, mut clock, owls) = serving();
        writing(&mut window, &clock, owls, "see http://example.invalid/a and https://blog.example.invalid/b");
        assert_eq!(shown(&window), None, "the first link is not https: no later one stands in");
        writing(&mut window, &clock, owls, "see https://blog.example.invalid/a");
        clock.pass(PREVIEW_PAUSE / 2);
        writing(&mut window, &clock, owls, "see https://blog.example.invalid/ab");
        assert!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE / 2)).is_empty());
        let asked = after(&mut window, &mut clock, PREVIEW_PAUSE / 2);
        assert_eq!(fetches(&asked), [("https://blog.example.invalid/ab".to_string(), false)]);
    }

    #[test]
    fn the_card_made_shows_and_goes_with_the_message_and_a_failed_read_goes_without() {
        let (mut window, mut clock, owls) = serving();
        writing(&mut window, &clock, owls, TEXT);
        after(&mut window, &mut clock, PREVIEW_PAUSE);
        let previewed =
            DiscussionMessage::Previewed { compose: general_compose(), link: URL.into(), made: Some(made(None)) };
        tell(&mut window, &clock, owls, discussion(previewed));
        assert_eq!(shown(&window), Some(true));
        let sent = tell(&mut window, &clock, owls, discussion(DiscussionMessage::Send));
        assert_eq!(sent_with(&sent), Some(Some(made(None))));
        assert_eq!(shown(&window), None, "the draft and its card are sent");
        writing(&mut window, &clock, owls, TEXT);
        after(&mut window, &mut clock, PREVIEW_PAUSE);
        let failed = DiscussionMessage::Previewed { compose: general_compose(), link: URL.into(), made: None };
        tell(&mut window, &clock, owls, discussion(failed));
        assert_eq!(shown(&window), None, "a failed read shows nothing");
        let sent = tell(&mut window, &clock, owls, discussion(DiscussionMessage::Send));
        assert_eq!(sent_with(&sent), Some(None), "the message goes, silently, without");
    }

    #[test]
    fn a_card_answered_for_a_link_since_typed_over_is_dropped() {
        let (mut window, mut clock, owls) = serving();
        writing(&mut window, &clock, owls, TEXT);
        after(&mut window, &mut clock, PREVIEW_PAUSE);
        writing(&mut window, &clock, owls, "this week https://blog.example.invalid/other");
        let late =
            DiscussionMessage::Previewed { compose: general_compose(), link: URL.into(), made: Some(made(None)) };
        tell(&mut window, &clock, owls, discussion(late));
        assert_eq!(shown(&window), Some(false), "still waiting for the link now typed");
    }

    #[test]
    fn a_card_taken_out_stays_out_for_that_link_in_that_draft_only() {
        let (mut window, mut clock, owls) = serving();
        writing(&mut window, &clock, owls, TEXT);
        after(&mut window, &mut clock, PREVIEW_PAUSE);
        let previewed =
            DiscussionMessage::Previewed { compose: general_compose(), link: URL.into(), made: Some(made(None)) };
        tell(&mut window, &clock, owls, discussion(previewed));
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::RemovePreview));
        assert_eq!(shown(&window), None);
        writing(&mut window, &clock, owls, &format!("{TEXT} and more words"));
        assert_eq!(shown(&window), None, "typing on does not bring it back");
        assert!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE)).is_empty());
        let sent = tell(&mut window, &clock, owls, discussion(DiscussionMessage::Send));
        assert_eq!(sent_with(&sent), Some(None));
        writing(&mut window, &clock, owls, TEXT);
        assert_eq!(shown(&window), Some(false), "a new draft with the same link has one again");
    }

    #[test]
    fn with_the_choice_off_or_a_server_that_does_not_offer_previews_nothing_is_read() {
        let (mut window, mut clock, owls) = serving();
        window.update(WindowMessage::Look(LookPick::Previews), clock.at());
        writing(&mut window, &clock, owls, TEXT);
        assert_eq!(shown(&window), None);
        assert!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE)).is_empty(), "off: nothing is fetched");
        window.update(WindowMessage::Look(LookPick::Previews), clock.at());
        writing(&mut window, &clock, owls, &format!("{TEXT}!"));
        assert_eq!(shown(&window), Some(false), "on again: the next draft is read");
        window.update(WindowMessage::Look(LookPick::Previews), clock.at());
        assert_eq!(shown(&window), None, "turned off while it waits: it goes");
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let mut clock = clock;
        writing(&mut window, &clock, owls, TEXT);
        assert_eq!(shown(&window), None);
        assert!(fetches(&after(&mut window, &mut clock, PREVIEW_PAUSE)).is_empty(), "not served: nothing is fetched");
    }

    #[test]
    fn a_preview_the_server_would_refuse_never_holds_the_message_back() {
        let (mut window, mut clock, owls) = serving();
        writing(&mut window, &clock, owls, TEXT);
        after(&mut window, &mut clock, PREVIEW_PAUSE);
        let overlong = PreviewMade { title: "t".repeat(proto::rules::MAX_PREVIEW_TITLE_CHARS + 1), ..made(None) };
        let previewed =
            DiscussionMessage::Previewed { compose: general_compose(), link: URL.into(), made: Some(overlong) };
        tell(&mut window, &clock, owls, discussion(previewed));
        let sent = tell(&mut window, &clock, owls, discussion(DiscussionMessage::Send));
        assert_eq!(sent_with(&sent), Some(None));
    }

    #[test]
    fn an_edit_goes_on_with_the_preview_the_message_holds_while_its_link_stands() {
        let (mut window, clock, owls) = serving();
        let card = chat::Card {
            url: URL.into(),
            host: "blog.example.invalid".into(),
            title: "Game night".into(),
            description: String::new(),
            picture: Some(chat::CardPicture {
                file: FileId::from_bytes([3; FileId::LEN]),
                width: 800,
                height: 600,
                thumbnail: FileId::from_bytes([4; FileId::LEN]),
            }),
        };
        let marks = vec![chat::Mark { range: 10..10 + URL.chars().count(), kind: chat::MarkKind::Link }];
        let mut message = said(2, 6, ADA, TEXT);
        message.body = Some(chat::Body::new(TEXT, marks).expect("a body"));
        message.preview = Some(card);
        tell(&mut window, &clock, owls, crate::harness::told(message));
        let edit = |window: &mut Window, text: &str| {
            tell(window, &clock, owls, discussion(DiscussionMessage::Edit(message_id(2))));
            tell(window, &clock, owls, discussion(DiscussionMessage::EditDraft(text.into())));
            tell(window, &clock, owls, discussion(DiscussionMessage::SaveEdit))
        };
        let edited = |todo: &[WindowAsk]| {
            todo.iter().find_map(|done| match done {
                WindowAsk::At(_, Ask::EditMessage { preview, .. }) => Some(preview.clone()),
                _ => None,
            })
        };
        let kept = edited(&edit(&mut window, &format!("{TEXT} (edited)"))).expect("an edit");
        let picture = Some(PreviewPicture::Sent(FileId::from_bytes([3; FileId::LEN])));
        assert_eq!(kept, Some(PreviewMade { picture, ..made(None) }));
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::CancelEdit));
        let dropped = edited(&edit(&mut window, "no link now")).expect("an edit");
        assert_eq!(dropped, None, "the link is gone: so is its preview");
    }
}
