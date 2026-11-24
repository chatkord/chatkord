//! Server settings › Webhooks, for whoever holds `MANAGE_WEBHOOKS`: the server's webhooks listed, one made in a channel
//! the maker sees, its URL shown once, and one given a new secret or deleted after a confirmation. The page holds the
//! dialog it is in as its stage, so a secret never leaves it except as the text a person copies.

use crate::loaded::Loaded;
use proto::chatkord_v1::Webhook as Wire;
use proto::rules::{MAX_NAME_CHARS, TextRefused, text_shape};
use terms::{ChannelId, LinkError, MemberId, Secret};
use words::{Phrase, Sentence};

pub use terms::WebhookId;

/// A webhook as the list shows it: never its secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    id: WebhookId,
    channel: ChannelId,
    name: String,
    mention_everyone: bool,
    made_by: Option<MemberId>,
    created_at: i64,
}

impl Listed {
    /// A webhook the wire states, or none when it names no id or no channel, a server not keeping the protocol.
    pub(crate) fn read(told: &Wire) -> Option<Listed> {
        Some(Listed {
            id: WebhookId::parse(&told.webhook_id)?,
            channel: ChannelId::parse(&told.channel_id)?,
            name: told.name.clone(),
            mention_everyone: told.mention_everyone,
            made_by: told.made_by.as_deref().and_then(MemberId::parse),
            created_at: told.created_at,
        })
    }

    /// Its id at the server.
    pub fn id(&self) -> &WebhookId {
        &self.id
    }

    /// The channel it posts in.
    pub fn channel(&self) -> &ChannelId {
        &self.channel
    }

    /// The name it posts under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether its posts may name `@everyone` and `@here`.
    pub fn mention_everyone(&self) -> bool {
        self.mention_everyone
    }

    /// Who made it; none once that account is gone.
    pub fn made_by(&self) -> Option<&MemberId> {
        self.made_by.as_ref()
    }

    /// When it was made, in seconds since the epoch.
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
}

/// A channel a webhook may post in, as the form offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Postable<'a> {
    /// The channel, by its id.
    pub id: &'a ChannelId,
    /// Its name.
    pub name: &'a str,
}

/// What is wrong with the form making a webhook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The name is empty, too long or holds a character no name may.
    Name(TextRefused),
    /// No text channel is chosen, or the one chosen is not among those the maker sees.
    Channel,
    /// The server would not make it, or did not answer.
    Server(LinkError),
}

impl Problem {
    /// What it says, in one sentence.
    pub fn said(&self) -> Sentence {
        match self {
            Problem::Name(refused) => Sentence::webhook_name_refused(*refused),
            Problem::Channel => Sentence::Phrase(Phrase::WebhookChannelNeeded),
            Problem::Server(failed) => words::failure::said(failed),
        }
    }

    /// The field it stands under: the server's refusal has none of its own and stands under the name, the field first
    /// typed in.
    pub const fn under(&self) -> FormField {
        match self {
            Problem::Name(_) | Problem::Server(_) => FormField::Name,
            Problem::Channel => FormField::Channel,
        }
    }
}

/// A field of the form making a webhook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    /// The name it posts under.
    Name,
    /// The channel it posts in.
    Channel,
}

/// The form making a webhook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Making {
    name: String,
    channel: Option<ChannelId>,
    channel_menu: bool,
    mention_everyone: bool,
    asking: bool,
    problem: Option<Problem>,
}

impl Making {
    /// The name as typed, untrimmed.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The channel chosen.
    pub fn channel(&self) -> Option<&ChannelId> {
        self.channel.as_ref()
    }

    /// Whether the Channel field's menu is open.
    pub fn channel_menu_open(&self) -> bool {
        self.channel_menu
    }

    /// Whether its posts may name `@everyone` and `@here`.
    pub fn mention_everyone(&self) -> bool {
        self.mention_everyone
    }

    /// Whether the server is being asked to make it.
    pub fn asking(&self) -> bool {
        self.asking
    }

    /// What is wrong, once a try was made.
    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    /// What is wrong, said under `field`.
    pub fn said_under(&self, field: FormField) -> Option<Sentence> {
        self.problem.as_ref().filter(|problem| problem.under() == field).map(Problem::said)
    }

    /// Whether the name is what is wrong, so its field shows refused.
    pub fn name_refused(&self) -> bool {
        matches!(self.problem, Some(Problem::Name(_)))
    }
}

/// The webhook just made or given a new secret, with its URL, shown once.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    webhook: Listed,
    url: Secret,
    copied: bool,
}

impl Made {
    /// The webhook, without its secret.
    pub fn webhook(&self) -> &Listed {
        &self.webhook
    }

    /// The URL a program posts to, with the secret in it.
    pub fn url(&self) -> &Secret {
        &self.url
    }

    /// Whether the URL went onto the clipboard.
    pub fn copied(&self) -> bool {
        self.copied
    }
}

/// What a confirmation is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// A new secret, which stops the old URL at once.
    Regenerate,
    /// The webhook gone: its URL stops working and its posts stay.
    Delete,
}

/// A confirmation, asked before a webhook is regenerated or deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirming {
    act: Act,
    webhook: Listed,
    asking: bool,
    refused: Option<LinkError>,
}

impl Confirming {
    /// What is confirmed.
    pub fn act(&self) -> Act {
        self.act
    }

    /// The webhook it is about.
    pub fn webhook(&self) -> &Listed {
        &self.webhook
    }

    /// Whether the server is being asked.
    pub fn asking(&self) -> bool {
        self.asking
    }

    /// Why the server would not, once it answered so.
    pub fn refused(&self) -> Option<&LinkError> {
        self.refused.as_ref()
    }
}

/// The dialog the page is in, if one.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Stage {
    /// The list alone.
    #[default]
    Listed,
    /// The form making a webhook.
    Making(Making),
    /// A webhook made or given a new secret, its URL shown.
    Made(Made),
    /// A regenerate or a delete awaiting its confirmation.
    Confirming(Confirming),
}

/// What a person does on the page.
#[derive(Clone, Debug)]
pub enum Message {
    /// Open the form making a webhook.
    Make,
    /// The name's whole text after an edit.
    Name(String),
    /// The Channel field pressed: its menu opens, or closes while it is open.
    ChannelMenu,
    /// A channel chosen; the field's menu closes.
    Channel(ChannelId),
    /// "May mention everyone" turned on or off.
    MentionEveryone(bool),
    /// Make it with what the form holds.
    Submit,
    /// Put the URL on the clipboard: the window's to carry out.
    CopyUrl,
    /// Ask to give the webhook a new secret.
    Regenerate(WebhookId),
    /// Ask to delete the webhook.
    Delete(WebhookId),
    /// Confirm what the confirmation asks.
    Confirm,
    /// Close the dialog, or leave the form or the confirmation.
    Cancel,
    /// Ask for the list again after it could not be had.
    Retry,
}

/// What the page asks of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    List,
    Create { channel: ChannelId, name: String, mention_everyone: bool },
    Regenerate(WebhookId),
    Delete(WebhookId),
}

/// What the place knows that the page reads: the text channels the asker sees, and whether the asker may let a webhook
/// mention everyone.
pub(crate) struct Context<'a> {
    pub(crate) channels: &'a [ChannelId],
    pub(crate) may_mention_everyone: bool,
}

/// The page, open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Webhooks {
    pub(crate) list: Loaded<Vec<Listed>>,
    pub(crate) stage: Stage,
}

impl Webhooks {
    /// The webhooks, as the server last listed them, oldest first.
    pub fn list(&self) -> &Loaded<Vec<Listed>> {
        &self.list
    }

    /// The dialog the page is in.
    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    pub(crate) fn update(&mut self, message: Message, context: &Context<'_>) -> Option<Ask> {
        match (message, &mut self.stage) {
            (Message::Make, Stage::Listed) => {
                self.stage = Stage::Making(Making {
                    name: String::new(),
                    channel: context.channels.first().cloned(),
                    channel_menu: false,
                    mention_everyone: false,
                    asking: false,
                    problem: None,
                });
                None
            }
            (Message::Name(name), Stage::Making(making)) if !making.asking => {
                making.name = name;
                making.problem = None;
                None
            }
            (Message::ChannelMenu, Stage::Making(making)) if !making.asking => {
                making.channel_menu = !making.channel_menu;
                None
            }
            (Message::Channel(channel), Stage::Making(making)) if !making.asking => {
                making.channel = Some(channel);
                making.channel_menu = false;
                making.problem = None;
                None
            }
            (Message::MentionEveryone(allowed), Stage::Making(making)) if !making.asking => {
                making.mention_everyone = allowed && context.may_mention_everyone;
                making.channel_menu = false;
                None
            }
            (Message::Submit, Stage::Making(making)) if !making.asking => {
                let name = match text_shape(&making.name, MAX_NAME_CHARS) {
                    Ok(name) => name.to_owned(),
                    Err(refused) => {
                        making.problem = Some(Problem::Name(refused));
                        return None;
                    }
                };
                let Some(channel) = making.channel.clone().filter(|chosen| context.channels.contains(chosen)) else {
                    making.problem = Some(Problem::Channel);
                    return None;
                };
                making.asking = true;
                making.problem = None;
                Some(Ask::Create {
                    channel,
                    name,
                    mention_everyone: making.mention_everyone && context.may_mention_everyone,
                })
            }
            (Message::Regenerate(id), Stage::Listed) => self.confirm(Act::Regenerate, &id),
            (Message::Delete(id), Stage::Listed) => self.confirm(Act::Delete, &id),
            (Message::Confirm, Stage::Confirming(confirming)) if !confirming.asking => {
                confirming.asking = true;
                confirming.refused = None;
                let id = confirming.webhook.id.clone();
                Some(match confirming.act {
                    Act::Regenerate => Ask::Regenerate(id),
                    Act::Delete => Ask::Delete(id),
                })
            }
            (Message::Cancel, Stage::Making(making)) if !making.asking => {
                self.stage = Stage::Listed;
                None
            }
            (Message::Cancel, Stage::Confirming(confirming)) if !confirming.asking => {
                self.stage = Stage::Listed;
                None
            }
            (Message::Cancel, Stage::Made(_)) => {
                self.stage = Stage::Listed;
                None
            }
            (Message::Retry, _) if matches!(self.list, Loaded::Failed(_)) => {
                self.list = Loaded::Loading;
                Some(Ask::List)
            }
            _ => None,
        }
    }

    fn confirm(&mut self, act: Act, id: &WebhookId) -> Option<Ask> {
        let Loaded::Ready(listed) = &self.list else { return None };
        let webhook = listed.iter().find(|listed| listed.id == *id)?.clone();
        self.stage = Stage::Confirming(Confirming { act, webhook, asking: false, refused: None });
        None
    }

    /// The server's list, or why there is none; a webhook out of shape is passed over.
    pub(crate) fn listed(&mut self, answer: Result<Vec<Wire>, LinkError>) {
        self.list = match answer {
            Ok(told) => Loaded::Ready(told.iter().filter_map(Listed::read).collect()),
            Err(failed) => Loaded::Failed(failed),
        };
    }

    /// The server's answer to making a webhook or giving one a new secret: its URL at the server `host`, shown once
    /// and the list read again; or why not, said where the ask was made.
    pub(crate) fn made(&mut self, answer: Result<(Wire, Secret), LinkError>, host: &str) -> Option<Ask> {
        match (answer, &mut self.stage) {
            (Ok((told, token)), Stage::Making(_) | Stage::Confirming(Confirming { act: Act::Regenerate, .. })) => {
                let Some(webhook) = Listed::read(&told) else {
                    return self.refused(LinkError::Malformed);
                };
                let url = Secret::new(proto::envelope::webhook_url(host, token.expose()));
                self.stage = Stage::Made(Made { webhook, url, copied: false });
                Some(Ask::List)
            }
            (Err(failed), _) => self.refused(failed),
            (Ok(_), _) => None,
        }
    }

    /// The server's answer to deleting a webhook: the list read again, or why not.
    pub(crate) fn deleted(&mut self, answer: Result<(), LinkError>) -> Option<Ask> {
        match (answer, &self.stage) {
            (Ok(()), Stage::Confirming(Confirming { act: Act::Delete, .. })) => {
                self.stage = Stage::Listed;
                Some(Ask::List)
            }
            (Err(failed), _) => self.refused(failed),
            (Ok(()), _) => None,
        }
    }

    fn refused(&mut self, failed: LinkError) -> Option<Ask> {
        match &mut self.stage {
            Stage::Making(making) => {
                making.asking = false;
                making.problem = Some(Problem::Server(failed));
            }
            Stage::Confirming(confirming) => {
                confirming.asking = false;
                confirming.refused = Some(failed);
            }
            Stage::Listed | Stage::Made(_) => {}
        }
        None
    }

    /// The URL to put on the clipboard, while a made webhook shows it.
    pub(crate) fn url(&self) -> Option<&Secret> {
        match &self.stage {
            Stage::Made(made) => Some(&made.url),
            Stage::Listed | Stage::Making(_) | Stage::Confirming(_) => None,
        }
    }

    /// The URL went onto the clipboard, or could not.
    pub(crate) fn copied(&mut self, done: bool) {
        if let Stage::Made(made) = &mut self.stage {
            made.copied = done;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::chatkord_v1::ErrorCode;

    const GENERAL: &str = "general";
    const SECRET: &str = "S3cretTokenS3cretTokenS3cretToken";

    fn channel() -> ChannelId {
        ChannelId::parse(GENERAL).expect("a channel id")
    }

    fn wire(id: &str, name: &str) -> Wire {
        Wire {
            webhook_id: id.into(),
            channel_id: GENERAL.into(),
            name: name.into(),
            mention_everyone: false,
            made_by: Some("ada".into()),
            created_at: 1_763_035_200,
        }
    }

    fn context(channels: &[ChannelId]) -> Context<'_> {
        Context { channels, may_mention_everyone: false }
    }

    fn listed(page: &mut Webhooks) {
        page.listed(Ok(vec![wire("w1", "Builds"), wire("", "No id"), wire("w2", "Alerts")]));
    }

    #[test]
    fn the_list_passes_over_a_webhook_out_of_shape_and_a_failed_one_can_be_asked_again() {
        let mut page = Webhooks::default();
        listed(&mut page);
        let Loaded::Ready(shown) = page.list() else { panic!("not listed") };
        assert_eq!(shown.iter().map(Listed::name).collect::<Vec<_>>(), ["Builds", "Alerts"]);
        page.listed(Err(LinkError::Timeout));
        assert_eq!(page.list(), &Loaded::Failed(LinkError::Timeout));
        let channels = [channel()];
        assert_eq!(page.update(Message::Retry, &context(&channels)), Some(Ask::List));
        assert_eq!(page.list(), &Loaded::Loading);
    }

    #[test]
    fn a_webhook_is_made_in_a_channel_the_maker_sees_with_a_name_in_bounds() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        page.update(Message::Make, &context(&channels));
        assert_eq!(page.update(Message::Submit, &context(&channels)), None, "no name");
        assert!(
            matches!(page.stage(), Stage::Making(making) if making.problem() == Some(&Problem::Name(TextRefused::Empty)))
        );
        page.update(Message::Name("Builds".into()), &context(&channels));
        page.update(Message::Channel(ChannelId::parse("secret").expect("a channel id")), &context(&channels));
        assert_eq!(page.update(Message::Submit, &context(&channels)), None, "a channel not among those seen");
        assert!(matches!(page.stage(), Stage::Making(making) if making.problem() == Some(&Problem::Channel)));
        page.update(Message::Channel(channel()), &context(&channels));
        assert_eq!(
            page.update(Message::Submit, &context(&channels)),
            Some(Ask::Create { channel: channel(), name: "Builds".into(), mention_everyone: false })
        );
        assert_eq!(page.update(Message::Submit, &context(&channels)), None, "asked once");
    }

    #[test]
    fn what_is_wrong_is_said_under_the_field_it_is_about() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        page.update(Message::Make, &context(&channels));
        page.update(Message::Submit, &context(&channels));
        let making = |page: &Webhooks| match page.stage() {
            Stage::Making(making) => making.clone(),
            other => panic!("not making: {other:?}"),
        };
        let needed = Some(Sentence::Phrase(Phrase::WebhookNameNeeded));
        assert_eq!((making(&page).said_under(FormField::Name), making(&page).name_refused()), (needed, true));
        assert_eq!(making(&page).said_under(FormField::Channel), None);
        page.update(Message::Name("x".repeat(MAX_NAME_CHARS + 1)), &context(&channels));
        page.update(Message::Submit, &context(&channels));
        assert_eq!(making(&page).said_under(FormField::Name), Some(Sentence::TextTooLong));
        page.update(Message::Name("Bell\x07".into()), &context(&channels));
        page.update(Message::Submit, &context(&channels));
        assert_eq!(making(&page).said_under(FormField::Name), Some(Sentence::Phrase(Phrase::PlainTextOnly)));
        page.update(Message::Name("Builds".into()), &context(&channels));
        page.update(Message::Submit, &context(&[]));
        let channel_needed = Some(Sentence::Phrase(Phrase::WebhookChannelNeeded));
        assert_eq!(
            (making(&page).said_under(FormField::Channel), making(&page).name_refused()),
            (channel_needed, false)
        );
        page.update(Message::Submit, &context(&channels));
        page.made(Err(LinkError::Timeout), "chat.example.invalid");
        let failed = Some(words::failure::said(&LinkError::Timeout));
        assert_eq!((making(&page).said_under(FormField::Name), making(&page).name_refused()), (failed, false));
    }

    #[test]
    fn the_channel_menu_toggles_and_closes_on_a_pick() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        page.update(Message::Make, &context(&channels));
        let open = |page: &Webhooks| matches!(page.stage(), Stage::Making(making) if making.channel_menu_open());
        page.update(Message::ChannelMenu, &context(&channels));
        assert!(open(&page));
        page.update(Message::ChannelMenu, &context(&channels));
        assert!(!open(&page), "a second press closes it");
        page.update(Message::ChannelMenu, &context(&channels));
        page.update(Message::Channel(channel()), &context(&channels));
        assert!(!open(&page), "a pick closes it");
    }

    #[test]
    fn mentioning_everyone_is_only_a_choice_for_whoever_may() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        page.update(Message::Make, &context(&channels));
        page.update(Message::Name("Builds".into()), &context(&channels));
        page.update(Message::MentionEveryone(true), &context(&channels));
        assert!(matches!(page.stage(), Stage::Making(making) if !making.mention_everyone()));
        let allowed = Context { channels: &channels, may_mention_everyone: true };
        page.update(Message::MentionEveryone(true), &allowed);
        assert_eq!(
            page.update(Message::Submit, &allowed),
            Some(Ask::Create { channel: channel(), name: "Builds".into(), mention_everyone: true })
        );
    }

    #[test]
    fn what_is_made_shows_its_url_once_and_the_list_is_read_again() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        page.update(Message::Make, &context(&channels));
        page.update(Message::Name("Builds".into()), &context(&channels));
        page.update(Message::Submit, &context(&channels));
        let asked = page.made(Ok((wire("w1", "Builds"), Secret::new(SECRET.into()))), "chat.example.invalid");
        assert_eq!(asked, Some(Ask::List));
        let expected = format!("https://chat.example.invalid/v1/webhooks/{SECRET}");
        assert_eq!(page.url().map(Secret::expose), Some(expected.as_str()));
        let Stage::Made(made) = page.stage() else { panic!("not made") };
        assert!(!format!("{made:?}").contains(SECRET), "never printed");
        page.copied(true);
        assert!(matches!(page.stage(), Stage::Made(made) if made.copied()));
        page.update(Message::Cancel, &context(&channels));
        assert!(page.url().is_none() && matches!(page.stage(), Stage::Listed), "gone once closed");
    }

    #[test]
    fn a_refusal_stays_in_the_form_and_the_form_can_ask_again() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        page.update(Message::Make, &context(&channels));
        page.update(Message::Name("Builds".into()), &context(&channels));
        page.update(Message::Submit, &context(&channels));
        let refused = LinkError::Refused(ErrorCode::InvalidRequest);
        assert_eq!(page.made(Err(refused.clone()), "chat.example.invalid"), None);
        assert!(
            matches!(page.stage(), Stage::Making(making) if !making.asking() && making.problem() == Some(&Problem::Server(refused)))
        );
        assert!(page.update(Message::Submit, &context(&channels)).is_some());
    }

    #[test]
    fn regenerating_and_deleting_are_asked_only_after_their_confirmation() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        listed(&mut page);
        let w1 = WebhookId::parse("w1").expect("an id");
        assert_eq!(page.update(Message::Regenerate(w1.clone()), &context(&channels)), None, "confirmation first");
        assert!(matches!(page.stage(), Stage::Confirming(confirming) if confirming.act() == Act::Regenerate));
        page.update(Message::Cancel, &context(&channels));
        assert!(matches!(page.stage(), Stage::Listed));
        page.update(Message::Regenerate(w1.clone()), &context(&channels));
        assert_eq!(page.update(Message::Confirm, &context(&channels)), Some(Ask::Regenerate(w1.clone())));
        let again = page.made(Ok((wire("w1", "Builds"), Secret::new(SECRET.into()))), "chat.example.invalid");
        assert_eq!((again, page.url().is_some()), (Some(Ask::List), true));
        page.update(Message::Cancel, &context(&channels));
        page.update(Message::Delete(w1.clone()), &context(&channels));
        assert_eq!(page.update(Message::Confirm, &context(&channels)), Some(Ask::Delete(w1)));
        assert_eq!(page.deleted(Ok(())), Some(Ask::List));
        assert!(matches!(page.stage(), Stage::Listed));
    }

    #[test]
    fn a_delete_the_server_refuses_stays_in_its_confirmation_with_the_reason() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        listed(&mut page);
        page.update(Message::Delete(WebhookId::parse("w2").expect("an id")), &context(&channels));
        page.update(Message::Confirm, &context(&channels));
        let refused = LinkError::Refused(ErrorCode::NotFound);
        assert_eq!(page.deleted(Err(refused.clone())), None);
        assert!(
            matches!(page.stage(), Stage::Confirming(confirming) if !confirming.asking() && confirming.refused() == Some(&refused))
        );
    }

    #[test]
    fn an_unknown_webhook_opens_no_confirmation() {
        let channels = [channel()];
        let mut page = Webhooks::default();
        listed(&mut page);
        page.update(Message::Delete(WebhookId::parse("nobody").expect("an id")), &context(&channels));
        assert!(matches!(page.stage(), Stage::Listed));
    }
}
