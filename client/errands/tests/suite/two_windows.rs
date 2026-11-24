//! Two windows chatting in one channel of a real server, each driven by its own runner as the shell drives its window:
//! what one sends, edits and deletes the other sees as it happens, and what it missed while its connection was down
//! it catches up on once the connection is made again by itself; and they talk beside the channel, in a conversation
//! and in a thread.

use choices::{EventSounds, MessageSounds, Popups, Remembered};
use errands::mailbox::{Inbox, mailbox};
use errands::{Host, Runner, Still};
use link::{Link, Trust};
use screens::chat::{Message, MessageId, Timeline};
use screens::{DiscussionMessage, Lobby, Opening, Panel, Place, PlaceId, PlaceMessage, Status, Window, WindowMessage};
use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use terms::{Secret, ServerUrl};
use testserver::{GENERAL, Line, Server};

const PASSWORD: &str = "a test password that is long enough";
const ADA: &str = "ada";
const BEA: &str = "bea";
/// How long one step may take: a reconnect waits 1, 2 and then 4 s, each with its jitter.
const WAIT: Duration = Duration::from_secs(20);

/// A host with nothing to paint on and no window of its own to close, hide or show.
struct Quiet;

impl Host for Quiet {
    fn slot(&self, _: &terms::MemberId, _: proto::video::Video) -> call::Slot {
        call::Slot::new()
    }
    fn clear_slots(&self) {}
    fn own_screen(&self) -> call::Slot {
        call::Slot::new()
    }
    fn own_camera(&self) -> call::Slot {
        call::Slot::new()
    }
    fn painting(&self) -> picture::Painting {
        picture::Painting::default()
    }
    fn hold_picture(
        &self,
        _: screens::PlaceId,
        _: proto::files::FileId,
        _: portrait::Square,
    ) -> Vec<(screens::PlaceId, proto::files::FileId)> {
        Vec::new()
    }
    fn hold_file(
        &self,
        _: screens::PlaceId,
        _: proto::files::FileId,
        _: portrait::Decoded,
    ) -> Vec<(screens::PlaceId, proto::files::FileId)> {
        Vec::new()
    }
    fn clear_pictures(&self, _: screens::PlaceId) {}
    fn hold_stills(&self, _: Vec<Still>) {}
    fn icon(&self, _: u32) -> Option<Vec<u8>> {
        None
    }
    fn quit(&self) {}
    fn hide(&self) {}
    fn show(&self) {}
    fn come_forward(&self, _: Option<platform::activation::Activation>) {}
}

/// An account made at `url` whose token is in the keyring, so a window launched with it signs in by itself.
async fn registered(url: &ServerUrl, identity: &str, invite: &str) {
    let (link, events) = Link::connect(url, &Trust::system(), "two_windows", "test").await.expect("connected");
    let (password, invite) = (Secret::new(PASSWORD.into()), Secret::new(invite.into()));
    let ack = link.register(identity, None, &password, &invite).await.expect("registered");
    let token = Secret::new(ack.session_token.expect("a token"));
    platform::keyring::store(url, identity, None, &token).expect("kept");
    link.part().await;
    drop(events);
}

/// One window and the runner that carries out what it asks, as the shell's scene holds them.
struct Seat {
    window: Window,
    runner: Runner<Quiet>,
    inbox: Inbox,
    place: PlaceId,
}

impl Seat {
    /// A window launched signed in as `identity` at `url`, by a person who turned event sounds off; `wake` is told
    /// each time its runner tells it something.
    fn launch(url: ServerUrl, identity: &str, wake: &mpsc::Sender<()>) -> Seat {
        let wake = wake.clone();
        let (told, inbox) = mailbox(Arc::new(move || {
            let _ = wake.send(());
        }));
        let runner = Runner::new(platform::Platform::default(), told, Quiet).expect("a runtime");
        let server = screens::Server { url, name: None, colour: None, picture: None };
        let (window, todo) = Window::launch(vec![Opening::Account(server, identity.into())], None, Instant::now());
        // A test shows no popup and plays no sound on the desktop it runs on.
        let quiet = Remembered {
            event_sounds: EventSounds::Off,
            message_sounds: MessageSounds::Off,
            popups: Popups::Off,
            ..Remembered::default()
        };
        let window = window.remembering(quiet, BTreeMap::new());
        let place = window.places()[0].id();
        let mut seat = Seat { window, runner, inbox, place };
        for asked in todo {
            seat.runner.start(asked);
        }
        seat
    }

    fn tell(&mut self, message: WindowMessage) {
        for asked in self.window.update(message, errands::moment(Instant::now())) {
            self.runner.start(asked);
        }
    }

    fn discuss(&mut self, message: DiscussionMessage) {
        self.tell(WindowMessage::At(self.place, PlaceMessage::Discussion(message)));
    }

    fn talk(&mut self, message: screens::TalkMessage) {
        self.tell(WindowMessage::At(self.place, PlaceMessage::Talk(message)));
    }

    /// Something said in the thread open beside the channel.
    fn in_thread(&mut self, message: DiscussionMessage) {
        self.tell(WindowMessage::At(self.place, PlaceMessage::Thread(message)));
    }

    /// The member whose name others see is `name`, once the members are read.
    fn member(&self, name: &str) -> Option<terms::MemberId> {
        self.place().directory().people().find(|(_, person)| person.name == name).map(|(id, _)| id.clone())
    }

    /// The conversation's row with `name`, once listed.
    fn conversation_with(&self, name: &str) -> Option<(u32, Option<String>)> {
        let screens::Rows::Ready(rows) = self.place().conversation_rows() else { return None };
        let row = rows.into_iter().find(|row| row.name.as_deref() == Some(name))?;
        Some((row.standing.unread, row.preview.map(|line| line.text.to_owned())))
    }

    fn thread_timeline(&self) -> Option<&Timeline> {
        self.place().thread()?.timeline()
    }

    /// Everything its runner told since the last drain, applied; whether there was anything.
    fn drain(&mut self) -> bool {
        let told = self.inbox.drain();
        let any = !told.is_empty();
        for message in told {
            self.tell(message);
        }
        any
    }

    fn place(&self) -> &Place {
        self.window.places().iter().find(|place| place.id() == self.place).expect("its place")
    }

    fn signed_in(&self) -> bool {
        matches!(self.place().status(), Status::Signed(_))
    }

    fn reconnecting(&self) -> bool {
        matches!(self.place().status(), Status::Reconnecting(_))
    }

    /// The channel the fixture names [`GENERAL`], once listed.
    fn general(&self) -> Option<terms::ChannelId> {
        let Some(Lobby::Ready(listed)) = self.place().lobby() else { return None };
        listed.text.iter().find(|channel| channel.name == GENERAL).map(|channel| channel.id.clone())
    }

    fn timeline(&self) -> Option<&Timeline> {
        self.place().discussion()?.timeline()
    }

    /// Whether every send of its own is answered.
    fn all_sent(&self) -> bool {
        self.place().discussion().is_some_and(|open| self.place().unsent(&open.compose).is_empty())
    }

    fn message(&self, id: &MessageId) -> Option<&Message> {
        self.timeline()?.get(id)
    }

    /// The message whose text is `body`, as it is held now.
    fn saying(&self, body: &str) -> Option<&Message> {
        self.timeline()?.messages().find(|message| text(message) == Some(body))
    }

    fn send(&mut self, body: &str) {
        self.discuss(DiscussionMessage::Draft(body.into()));
        self.discuss(DiscussionMessage::Send);
    }

    /// Delete on a message, then the confirmation it asks for.
    fn delete(&mut self, id: &MessageId) {
        self.discuss(DiscussionMessage::Delete(id.clone()));
        self.tell(WindowMessage::Dialog(screens::DialogMessage::Confirm));
    }

    fn edit(&mut self, id: &MessageId, body: &str) {
        self.discuss(DiscussionMessage::Edit(id.clone()));
        self.discuss(DiscussionMessage::EditDraft(body.into()));
        self.discuss(DiscussionMessage::SaveEdit);
    }
}

fn text(message: &Message) -> Option<&str> {
    message.body.as_ref().map(screens::chat::Body::text)
}

/// Whether `seat` holds the message `id` edited to say `body`.
fn edited(seat: &Seat, id: &MessageId, body: &str) -> bool {
    seat.message(id).is_some_and(|message| text(message) == Some(body) && message.edited_at.is_some())
}

/// Whether `seat` holds the message `id` as deleted.
fn deleted(seat: &Seat, id: &MessageId) -> bool {
    seat.message(id).is_some_and(Message::deleted)
}

/// Both windows, and the one wake their runners tell.
struct Room {
    ada: Seat,
    bea: Seat,
    woken: mpsc::Receiver<()>,
}

impl Room {
    /// Drives both windows until `done` holds of them, failing as `what` once [`WAIT`] has passed.
    fn until(&mut self, what: &str, done: impl Fn(&Seat, &Seat) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            let moved = self.ada.drain() | self.bea.drain();
            if done(&self.ada, &self.bea) {
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "{what}: not within {WAIT:?}");
            if !moved {
                assert!(self.woken.recv_timeout(left).is_ok(), "{what}: not within {WAIT:?}");
            }
        }
    }

    /// Ada sends `body` and waits until her send is answered; its id.
    fn ada_sends(&mut self, body: &str) -> MessageId {
        self.ada.send(body);
        self.until(body, |ada, _| ada.all_sent() && ada.saying(body).is_some());
        self.ada.saying(body).expect("sent").id.clone()
    }
}

mod server {
    use super::*;
    use screens::TalkMessage;

    #[test]
    fn two_windows_chat_live_and_one_catches_up_after_its_connection_drops() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let setup = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (server, invites) = setup.block_on(Server::start("errands_two_windows", 2));
        let line = Line::to(&server);
        let at_server = ServerUrl::parse(&server.url()).unwrap();
        let through_line = ServerUrl::parse(&line.url()).unwrap();
        setup.block_on(async {
            registered(&at_server, ADA, &invites[0]).await;
            registered(&through_line, BEA, &invites[1]).await;
        });

        let (wake, woken) = mpsc::channel();
        let ada = Seat::launch(at_server, ADA, &wake);
        let bea = Seat::launch(through_line, BEA, &wake);
        let mut room = Room { ada, bea, woken };
        room.until("both signed in, #general listed", |ada, bea| {
            [ada, bea].iter().all(|seat| seat.signed_in() && seat.general().is_some())
        });
        for seat in [&mut room.ada, &mut room.bea] {
            let general = seat.general().unwrap();
            seat.discuss(DiscussionMessage::Open(general));
        }
        room.until("both read #general", |ada, bea| ada.timeline().is_some() && bea.timeline().is_some());

        let first = room.ada_sends("first, said live");
        room.until("Bea sees the first", |_, bea| bea.message(&first).is_some());
        let first_edited = "first, edited live";
        room.ada.edit(&first, first_edited);
        room.until("both see the first edited", |ada, bea| {
            [ada, bea].iter().all(|seat| edited(seat, &first, first_edited))
        });
        room.ada.delete(&first);
        room.until("both see the first deleted", |ada, bea| [ada, bea].iter().all(|seat| deleted(seat, &first)));

        line.cut();
        let second_edited = "second, edited while Bea was away";
        room.until("Bea's connection drops", |_, bea| bea.reconnecting());
        let second = room.ada_sends("second, said while Bea was away");
        room.ada.edit(&second, second_edited);
        room.until("Ada's edit of the second", |ada, _| edited(ada, &second, second_edited));
        let third = room.ada_sends("third, said and deleted while Bea was away");
        room.ada.delete(&third);
        room.until("Ada's delete of the third", |ada, _| deleted(ada, &third));
        room.until("Bea's try while the line is cut goes unanswered", |_, bea| {
            matches!(bea.place().panel(), Panel::NotAnswering { .. })
        });
        assert!(room.bea.message(&second).is_none(), "nothing reached Bea while away");

        line.mend();
        room.until("Bea back, caught up on what she missed", |_, bea| {
            bea.signed_in() && edited(bea, &second, second_edited) && deleted(bea, &third)
        });
        assert!(deleted(&room.bea, &first), "what she saw before stays as it was");

        room.ada.runner.part();
        room.bea.runner.part();
    }

    #[test]
    fn two_windows_talk_in_a_conversation_and_a_thread_that_marks_the_other_s_channel() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let setup = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (server, invites) = setup.block_on(Server::start("errands_two_windows_talk", 2));
        let url = ServerUrl::parse(&server.url()).unwrap();
        setup.block_on(async {
            registered(&url, ADA, &invites[0]).await;
            registered(&url, BEA, &invites[1]).await;
        });

        let (wake, woken) = mpsc::channel();
        let ada = Seat::launch(url.clone(), ADA, &wake);
        let bea = Seat::launch(url, BEA, &wake);
        let mut room = Room { ada, bea, woken };
        room.until("both signed in, #general listed, each the other known, conversations offered", |ada, bea| {
            [ada, bea].iter().all(|seat| {
                seat.signed_in()
                    && seat.general().is_some()
                    && seat.place().offers_conversations()
                    && seat.place().offers_threads()
            }) && ada.member(BEA).is_some()
                && bea.member(ADA).is_some()
        });
        for seat in [&mut room.ada, &mut room.bea] {
            let general = seat.general().unwrap();
            seat.discuss(DiscussionMessage::Open(general));
        }
        room.until("both read #general", |ada, bea| ada.timeline().is_some() && bea.timeline().is_some());

        // Ada messages Bea directly, from where she is: the conversation opens for her, and Bea is told of it.
        let bea_id = room.ada.member(BEA).unwrap();
        room.ada.talk(TalkMessage::Message(bea_id));
        room.until("Ada's conversation with Bea is open and read", |ada, _| {
            ada.place().discussion().is_some_and(|open| matches!(open.owner, screens::Owner::Conversation(_)))
                && ada.timeline().is_some()
        });
        room.ada.send("a word in private");
        room.until("Ada's send is answered", |ada, _| ada.all_sent() && ada.saying("a word in private").is_some());
        room.until("Bea sees the conversation, its message unread", |_, bea| {
            bea.conversation_with(ADA) == Some((1, Some("a word in private".to_owned())))
        });
        assert_eq!(room.bea.place().conversations_unread(), 1, "the row atop her channels counts it");
        assert!(
            room.bea.place().discussion().is_some_and(|open| matches!(open.owner, screens::Owner::Channel(_))),
            "her channel stays where she was; the conversation waits in its list"
        );

        // Ada says something in #general; Bea starts a thread on it and names Ada in her reply, so Ada follows it.
        let general = room.ada.general().unwrap();
        room.ada.discuss(DiscussionMessage::Open(general));
        room.until("Ada back in #general", |ada, _| {
            ada.place().discussion().is_some_and(|open| matches!(open.owner, screens::Owner::Channel(_)))
                && ada.timeline().is_some()
        });
        let root = room.ada_sends("who is coming on friday?");
        room.until("Bea sees the root", |_, bea| bea.message(&root).is_some());
        room.bea.talk(TalkMessage::StartThread(root.clone()));
        room.until("Bea's thread composer is open beside #general", |_, bea| bea.thread_timeline().is_some());
        room.bea.in_thread(DiscussionMessage::Draft("@ad".into()));
        room.bea.in_thread(DiscussionMessage::Choose(None));
        room.bea.in_thread(DiscussionMessage::Send);
        room.until("the root shows its thread to Ada", |ada, _| {
            ada.message(&root).is_some_and(|message| message.thread.is_some())
        });
        room.until("Bea's reply is in the thread", |_, bea| {
            bea.thread_timeline().is_some_and(|thread| thread.messages().count() == 1)
        });
        room.until("Ada's channel is marked by the thread she now follows", |ada, _| {
            let general = ada.general().unwrap();
            ada.place()
                .channel_standing(&general)
                .is_some_and(|standing| standing.unread >= 1 && standing.mentions >= 1)
                && ada.place().threads_unread() >= 1
        });
        let screens::Rows::Ready(rows) = room.ada.place().thread_rows() else {
            panic!("her followed threads are listed")
        };
        assert_eq!((rows.len(), rows[0].channel), (1, GENERAL));

        room.ada.runner.part();
        room.bea.runner.part();
    }
}
