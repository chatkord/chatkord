//! What one server's place asks of the world: the door's errands on its own connection, kept for what follows, and
//! every ask that connection answers, its call among them.

use crate::Kept;
use crate::call::{Asks, Controls, Outlet, held_asks, run_call, stop_call};
use crate::host::Host;
use crate::keeps::Keeping;
use crate::pictures::{Holding, fetch_picture, upload_picture};
use crate::runner::Runner;
use link::Link;
use screens::{PlaceAsk, PlaceId, PlaceMessage, Row, WindowMessage};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

/// What a server's connection holds in the runner: the signed-in connection, whose events its listener tells the
/// window (or lends a call), and the errand under way, so Cancel can stop it.
#[derive(Default)]
pub(crate) struct Slot {
    pub(crate) kept: Arc<Mutex<Option<Kept>>>,
    errand: Option<tokio::task::AbortHandle>,
    /// The wait before the next try to reconnect, which a later wait replaces and a forgotten server cancels.
    waiting: Option<tokio::task::AbortHandle>,
    /// The uploads under way, so a Cancel stops one.
    uploads: std::collections::HashMap<screens::Upload, tokio::task::AbortHandle>,
}

/// The link of the connection in `kept`, open or not.
fn kept_link(kept: &Mutex<Option<Kept>>) -> Option<Link> {
    Some(kept.lock().ok()?.as_ref()?.link.clone())
}

/// The link of the connection in `kept`, while it is open.
pub(crate) fn open_link(kept: &Mutex<Option<Kept>>) -> Option<Link> {
    kept_link(kept).filter(|link| !link.is_closed())
}

/// How long a server taken off the device with no connection open is given to end its session.
const END_KEPT_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// Ends this device's session at a server, which closes the socket that asked; a server that does not answer keeps
/// the session until it lapses, and the token is forgotten here either way.
async fn sign_out(link: Option<Link>) {
    if let Some(link) = link
        && let Err(why) = link.ask(proto::chatkord_v1::SignOut {}).await
    {
        report::note!(warn, "signing out was not answered", why = why);
    }
}

/// The applications playing sound now, for a window share whose owner was not found; none where this system finds a
/// window's owner by itself, or the sound server would not say.
#[cfg(target_os = "linux")]
async fn sound_apps() -> Option<Vec<screens::SoundApp>> {
    let apps = ::sound::Sound::shared().apps().await.ok()?;
    Some(apps.into_iter().map(|app| screens::SoundApp { name: app.name, binary: app.binary }).collect())
}

#[cfg(not(target_os = "linux"))]
async fn sound_apps() -> Option<Vec<screens::SoundApp>> {
    None
}

impl<H: Host> Runner<H> {
    /// Ends this device's account at a server: its call, session (first, when `sign_out_first`), connection and token,
    /// and its row as `row` says once the keyring answered; a token the keyring kept is told to the window.
    fn end_account(
        &mut self,
        id: PlaceId,
        server: screens::Server,
        identity: Option<String>,
        sign_out_first: bool,
        row: Row,
    ) {
        stop_call(&self.running, id);
        // Before any ask the window sends after this one, so a picture asked again is held anew.
        self.host.clear_pictures(id);
        let kept = match row {
            Row::Forgotten => {
                let slot = self.slots.remove(&id).unwrap_or_default();
                if let Some(running) = slot.errand {
                    running.abort();
                }
                if let Some(waiting) = slot.waiting {
                    waiting.abort();
                }
                slot.kept
            }
            Row::SignedOut => Arc::clone(&self.slots.entry(id).or_default().kept),
        };
        let link = open_link(&kept).filter(|_| sign_out_first);
        let (mailbox, trust, platform) = (self.mailbox.clone(), self.trust.clone(), self.platform.clone());
        crate::errand::spawn(&self.runtime, report::word!("end account"), async move {
            match (&link, &identity) {
                (None, Some(identity)) if sign_out_first => {
                    let ended = crate::signin::end_kept(
                        &platform,
                        &server.url,
                        identity,
                        &trust,
                        terms::BUILD,
                        END_KEPT_WITHIN,
                    )
                    .await;
                    if let Err(why) = ended {
                        report::note!(warn, "a removed server's session was not ended there", why = why);
                    }
                }
                _ => sign_out(link).await,
            }
            let _ = kept.lock().map(|mut kept| kept.take());
            let forgotten = match &identity {
                Some(identity) => platform::keyring::forget_token(&server.url, identity).await.is_ok(),
                None => true,
            };
            let _ = match row.after(forgotten) {
                Row::SignedOut => platform::state::signed_out(&platform, server.url.clone()).await,
                Row::Forgotten => {
                    let _ = platform::volume::forget(&platform, server.url.clone()).await;
                    let _ = platform::folds::forget(&platform, server.url.clone()).await;
                    platform::state::forget_server(&platform, server.url.clone()).await
                }
            };
            if let (false, Some(identity)) = (forgotten, identity) {
                mailbox.tell(WindowMessage::TokenLeft(server, identity)).await;
            }
        });
    }

    /// Carries out what the place `id` asks, its answer told by the mailbox as that place's.
    pub(crate) fn at(&mut self, id: PlaceId, todo: PlaceAsk) {
        let mailbox = self.mailbox.clone();
        let tell = move |message: PlaceMessage| WindowMessage::At(id, message);
        let slot = self.slots.entry(id).or_default();
        let kept = Arc::clone(&slot.kept);
        match todo {
            PlaceAsk::Door(screens::DoorAsk::Go(errand)) => {
                let (trust, platform) = (self.trust.clone(), self.platform.clone());
                let running = crate::errand::spawn(&self.runtime, report::word!("carry out the door"), async move {
                    let carried = crate::carry_out(&platform, errand, &trust, terms::BUILD).await;
                    if let Some(connection) = carried.session {
                        let link = connection.link.clone();
                        // What the server greeted this session with: before the sign-in's report, so the place shows
                        // its name, colour and picture once it is in.
                        let (identity, limits) = (crate::signin::greeting(&link), link.file_limits());
                        let greeted = screens::ServerMessage::Greeted { identity, limits };
                        mailbox.tell(tell(PlaceMessage::Server(greeted))).await;
                        let built = screens::ServerMessage::Built(link.server_build().cloned());
                        mailbox.tell(tell(PlaceMessage::Server(built))).await;
                        let spoke = screens::ServerMessage::Spoke(link.server_protocol());
                        mailbox.tell(tell(PlaceMessage::Server(spoke))).await;
                        let served = screens::Served::read(&link.switched_off());
                        mailbox.tell(tell(PlaceMessage::Server(screens::ServerMessage::Serves(served)))).await;
                        let told = mailbox.clone();
                        let listened = crate::Kept::listen(connection, move |heard| {
                            let told = told.clone();
                            async move {
                                if let Some(heard) = PlaceMessage::told(&heard) {
                                    told.tell(WindowMessage::At(id, heard)).await;
                                }
                            }
                        });
                        // A connection kept before, now replaced, ends its listener as it is dropped.
                        if let Ok(mut kept) = kept.lock() {
                            *kept = Some(listened);
                        }
                    }
                    if let Some(heard) = carried.heard {
                        mailbox
                            .tell(tell(PlaceMessage::Session(screens::SessionMessage::Door(
                                screens::Message::Reported(heard),
                            ))))
                            .await;
                    }
                });
                slot.errand = Some(running.abort_handle());
            }
            PlaceAsk::Door(screens::DoorAsk::Cancel) => {
                if let Some(running) = slot.errand.take() {
                    running.abort();
                }
            }
            PlaceAsk::Door(screens::DoorAsk::ChangePassword { current, new }) => {
                let link = kept_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("change password"), async move {
                    let saved = match link {
                        Some(link) => link
                            .ask(proto::chatkord_v1::ChangePassword {
                                password: new.expose().to_string(),
                                current: current.expose().to_string(),
                            })
                            .await
                            .map(|_| ()),
                        None => Err(terms::LinkError::Offline),
                    };
                    let heard = screens::Heard::PasswordSaved(saved);
                    mailbox
                        .tell(tell(PlaceMessage::Session(screens::SessionMessage::Door(screens::Message::Reported(
                            heard,
                        )))))
                        .await;
                });
            }
            PlaceAsk::Op(asked) => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("ask an op"), async move {
                    let (op, answer) = asked.into_parts();
                    let answered = match link {
                        Some(link) => link.ask_body(op).await,
                        None => Err(terms::LinkError::Offline),
                    };
                    if let Some(message) = answer(answered) {
                        mailbox.tell(tell(message)).await;
                    }
                });
            }
            PlaceAsk::ListSoundApps => {
                crate::errand::spawn(&self.runtime, report::word!("list sound apps"), async move {
                    if let Some(apps) = sound_apps().await {
                        mailbox.tell(tell(PlaceMessage::Call(screens::CallMessage::SoundApps(apps)))).await;
                    }
                });
            }
            PlaceAsk::Resume(cursor) => {
                let link = kept_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("resume"), async move {
                    // What was missed comes as events; one that cannot be resumed was told whole, also as events.
                    if let Some(link) = link
                        && let Err(error) = link.resume(&cursor).await
                    {
                        report::note!(
                            debug,
                            "a resume went unanswered; the channels were listed whole anyway",
                            why = error
                        );
                    }
                });
            }
            PlaceAsk::Join { channel_id, volumes } => {
                let (commands, queued) = mpsc::channel(8);
                // How the call starts (muted, deafened, each member's volume) is the first of what is wanted of it.
                let starting = call::room::Wants::starting(self.next_call, volumes);
                let (wants, wanted) = watch::channel(starting);
                let leaving = call::room::Leaving::default();
                if let Ok(mut running) = self.running.lock() {
                    *running = Some((id, Asks { wants: wants.clone(), commands: commands.clone() }, leaving.clone()));
                }
                let (engines, running) = (Arc::clone(&self.engines), Arc::clone(&self.running));
                let outlet =
                    Outlet { mailbox: mailbox.clone(), host: Arc::clone(&self.host), platform: self.platform.clone() };
                let host = Arc::clone(&self.host);
                crate::errand::spawn(&self.runtime, report::word!("keep a call"), async move {
                    let controls = Controls { wanted, wants, commands: commands.clone(), queued, leaving };
                    let held = Arc::clone(&kept);
                    let call = async move { run_call(id, channel_id, &engines, &held, controls, outlet).await };
                    let finished = crate::call::kept_call(call, &*host, kept_link(&kept)).await;
                    // Only this call's own sender goes: a call begun since is another's.
                    if let Ok(mut running) = running.lock() {
                        running.take_if(|(_, held, _)| held.commands.same_channel(&commands));
                    }
                    mailbox.tell(tell(finished.told())).await;
                });
            }
            PlaceAsk::Leave => {
                // No call is held here: it ended by itself, and the host's word that it did is on its way.
                stop_call(&self.running, id);
            }
            PlaceAsk::Call(control) => {
                let Some(asks) = held_asks(&self.running, id) else { return };
                let mut act = None;
                asks.wants.send_modify(|wants| act = wants.take(control));
                if let Some(act) = act {
                    asks.act(&self.runtime, act);
                }
            }
            PlaceAsk::WatchSources => {
                let watch = crate::sources::watch(&self.runtime, mailbox, id, Arc::clone(&self.host));
                if let Some(before) = self.watching.insert(id, watch) {
                    before.abort();
                }
            }
            PlaceAsk::StopSources => {
                if let Some(watch) = self.watching.remove(&id) {
                    watch.abort();
                }
            }
            PlaceAsk::ShareSource { key, quality } => {
                if let Some(Asks { commands, .. }) = held_asks(&self.running, id) {
                    crate::errand::spawn(
                        &self.runtime,
                        report::word!("share source"),
                        crate::sources::share(id, key, quality, commands, mailbox),
                    );
                }
            }
            PlaceAsk::ChooseScreen(quality) => {
                if let Some(Asks { commands, .. }) = held_asks(&self.running, id) {
                    crate::errand::spawn(
                        &self.runtime,
                        report::word!("choose screen"),
                        crate::sources::choose_screen(id, commands, quality, mailbox),
                    );
                }
            }
            PlaceAsk::GetServerReport => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("get server report"), async move {
                    let answer = match link {
                        Some(link) => link.server_report(screens::LISTED_LOG_LINES).await,
                        None => Err(terms::ReportError::Reach(terms::LinkError::Offline)),
                    };
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::ServerReported(answer)))).await;
                });
            }
            PlaceAsk::SaveReport(download) => {
                crate::errand::spawn(&self.runtime, report::word!("save report"), async move {
                    let saved = crate::debug_report::save(download).await;
                    let report = screens::ServerAboutMessage::Report(screens::ReportMessage::Saved(saved));
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::About(report)))).await;
                });
            }
            PlaceAsk::LookUpPicture { picture } => {
                let holding = Holding { host: Arc::clone(&self.host), place: id, mailbox: mailbox.clone() };
                let platform = self.platform.clone();
                crate::errand::spawn(&self.runtime, report::word!("look up picture"), async move {
                    let told = match platform::cache::picture(&platform, picture).await {
                        Some(square) => {
                            holding.picture(picture, square).await;
                            screens::ServerMessage::PictureReady(picture)
                        }
                        None => screens::ServerMessage::PictureMissing(picture),
                    };
                    mailbox.tell(tell(PlaceMessage::Server(told))).await;
                });
            }
            PlaceAsk::FetchPicture { picture, kind } => {
                let link = open_link(&kept);
                let holding = Holding { host: Arc::clone(&self.host), place: id, mailbox: mailbox.clone() };
                let platform = self.platform.clone();
                crate::errand::spawn(&self.runtime, report::word!("fetch picture"), async move {
                    let told = match fetch_picture(&platform, link, kind, picture).await {
                        Ok(square) => {
                            holding.picture(picture, square).await;
                            screens::ServerMessage::PictureReady(picture)
                        }
                        Err(failed) => screens::ServerMessage::PictureUnfetched(picture, failed),
                    };
                    mailbox.tell(tell(PlaceMessage::Server(told))).await;
                });
            }
            PlaceAsk::PickPicture => {
                crate::errand::spawn(&self.runtime, report::word!("pick picture"), async move {
                    let picked = match platform::pick::picture().await {
                        Ok(platform::pick::Picked::File(bytes)) => screens::picture_row::Picked::File(bytes),
                        Ok(platform::pick::Picked::Nothing) => screens::picture_row::Picked::Nothing,
                        Err(platform::Unavailable) => screens::picture_row::Picked::Unreadable,
                    };
                    let message = screens::picture_row::Message::Picked(picked);
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::Picture(message)))).await;
                });
            }
            PlaceAsk::UploadPicture { bytes, kind } => {
                let link = open_link(&kept);
                let holding = Holding { host: Arc::clone(&self.host), place: id, mailbox: mailbox.clone() };
                let platform = self.platform.clone();
                crate::errand::spawn(&self.runtime, report::word!("upload picture"), async move {
                    let answer = upload_picture(&platform, link, kind, bytes, holding).await;
                    let message = screens::picture_row::Message::Uploaded(answer);
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::Picture(message)))).await;
                });
            }
            PlaceAsk::KeepVolume { server, user_id, audio, volume } => {
                drop(self.keeping.send(Keeping::Volume { server, user_id, audio, volume }));
            }
            PlaceAsk::RememberServer { server, identity } => {
                drop(self.keeping.send(Keeping::Server { server, identity }))
            }
            PlaceAsk::ListMembers => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("list members"), async move {
                    let members = match link {
                        Some(link) => link.list_members().await,
                        None => Err(terms::LinkError::Offline),
                    };
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::MembersListed(members)))).await;
                });
            }
            PlaceAsk::KeepFolded { server, folded } => drop(self.keeping.send(Keeping::Folded { server, folded })),
            PlaceAsk::SendMessage { timeline, thread_of, body, spans, files, reply, nonce, preview } => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("send message"), async move {
                    let sent = match link {
                        Some(link) => {
                            let reply = reply.as_ref().map(|(id, mention)| (id, *mention));
                            let preview = match preview {
                                Some(made) if link.switched_on(proto::rules::Switch::LinkPreviews) => {
                                    Some(crate::previews::wire(&link, made).await)
                                }
                                Some(_) | None => None,
                            };
                            let sending = link::Sending {
                                timeline: &timeline,
                                thread_of: thread_of.as_ref(),
                                body: &body,
                                spans: &spans,
                                files: &files,
                                reply,
                                nonce,
                            };
                            link.send_with_preview(sending, preview)
                                .await
                                .and_then(|message| screens::read_message(&message))
                        }
                        None => Err(terms::LinkError::Offline),
                    };
                    let sent =
                        screens::DiscussionMessage::Sent { timeline, thread_of, nonce, sent: sent.map(Box::new) };
                    mailbox.tell(tell(PlaceMessage::Discussion(sent))).await;
                });
            }
            PlaceAsk::EditMessage { message_id, body, spans, preview } => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("edit message"), async move {
                    let changed = match link {
                        Some(link) => {
                            let preview = match preview {
                                Some(made) if link.switched_on(proto::rules::Switch::LinkPreviews) => {
                                    Some(crate::previews::wire(&link, made).await)
                                }
                                Some(_) | None => None,
                            };
                            link.edit_with_preview(&message_id, &body, &spans, preview)
                                .await
                                .and_then(|message| screens::read_message(&message))
                        }
                        None => Err(terms::LinkError::Offline),
                    };
                    mailbox
                        .tell(tell(PlaceMessage::Discussion(screens::DiscussionMessage::Changed(
                            changed.map(Box::new),
                        ))))
                        .await;
                });
            }
            PlaceAsk::Upload { compose, upload, path, name } => {
                let link = open_link(&kept);
                let told = mailbox.clone();
                let holding = Holding { host: Arc::clone(&self.host), place: id, mailbox: mailbox.clone() };
                let running = crate::errand::spawn(&self.runtime, report::word!("upload"), async move {
                    let said = crate::attach::upload(link, &name, &path, |sent, of| {
                        let told = told.clone();
                        let compose = compose.clone();
                        async move {
                            let said = Ok(screens::Uploaded::Sent { sent, of });
                            let message = screens::DiscussionMessage::Upload { compose, upload, said };
                            told.tell(WindowMessage::At(id, PlaceMessage::Discussion(message))).await;
                        }
                    })
                    .await;
                    // A picture is held from this device's own copy, for its tile now and its viewer once sent.
                    if let Ok(file) = said
                        && let Some(picture) = crate::attach::picture_of(path).await
                    {
                        holding.file(file, picture).await;
                    }
                    let message =
                        screens::DiscussionMessage::Upload { compose, upload, said: said.map(screens::Uploaded::Kept) };
                    mailbox.tell(tell(PlaceMessage::Discussion(message))).await;
                });
                slot.uploads.retain(|_, running| !running.is_finished());
                slot.uploads.insert(upload, running.abort_handle());
            }
            PlaceAsk::CancelUpload { upload } => {
                if let Some(running) = slot.uploads.remove(&upload) {
                    running.abort();
                }
            }
            PlaceAsk::PickFiles { compose } => {
                crate::errand::spawn(&self.runtime, report::word!("pick files"), async move {
                    let picked = platform::pick::files().await.map_err(|platform::Unavailable| screens::NoPicker);
                    let message = screens::DiscussionMessage::Picked { compose, picked };
                    mailbox.tell(tell(PlaceMessage::Discussion(message))).await;
                });
            }
            PlaceAsk::SaveFile { file, name } => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("save file"), async move {
                    let saved = crate::attach::save(link, file, &name).await;
                    let message = screens::DiscussionMessage::Saved { file, name, saved };
                    mailbox.tell(tell(PlaceMessage::Discussion(message))).await;
                });
            }
            PlaceAsk::FetchFile { file, kind } => {
                let link = open_link(&kept);
                let holding = Holding { host: Arc::clone(&self.host), place: id, mailbox: mailbox.clone() };
                crate::errand::spawn(&self.runtime, report::word!("fetch file"), async move {
                    let fetched = match link {
                        Some(link) => link.fetch(kind, file).await,
                        None => Err(terms::FileError::Reach(terms::LinkError::Offline)),
                    };
                    let decoded = match fetched {
                        Ok(bytes) => crate::errand::blocking(report::word!("decode a file"), move || {
                            portrait::shown(&bytes, kind)
                        })
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .ok_or(terms::FileError::Unreadable { kind }),
                        Err(failed) => Err(failed),
                    };
                    let told = match decoded {
                        Ok(picture) => {
                            holding.file(file, picture).await;
                            screens::ServerMessage::FileReady(file)
                        }
                        Err(failed) => screens::ServerMessage::FileUnfetched(file, failed),
                    };
                    mailbox.tell(tell(PlaceMessage::Server(told))).await;
                });
            }
            PlaceAsk::OpenLink(target) => {
                crate::errand::spawn(&self.runtime, report::word!("open link"), async move {
                    if let Err(why) = platform::open::url(&target).await {
                        report::note!(warn, "a link was not opened", why = why);
                        let message = screens::DiscussionMessage::NotOpened;
                        mailbox.tell(tell(PlaceMessage::Discussion(message))).await;
                    }
                });
            }
            PlaceAsk::FetchPreview { compose, link: url, picture } => {
                let link = open_link(&kept);
                let trust = self.trust.clone();
                let holding = Holding { host: Arc::clone(&self.host), place: id, mailbox: mailbox.clone() };
                crate::errand::spawn(&self.runtime, report::word!("fetch preview"), async move {
                    let made = crate::previews::make(link, &trust, url.clone(), picture, holding).await;
                    let previewed = screens::DiscussionMessage::Previewed { compose, link: url, made };
                    mailbox.tell(tell(PlaceMessage::Discussion(previewed))).await;
                });
            }
            PlaceAsk::Activity { idle } => {
                report::note!(debug, "told the server this device's activity", idle = idle);
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("activity"), async move {
                    if let Some(link) = link {
                        let _ = link.tell(proto::chatkord_v1::Activity { idle }).await;
                    }
                });
            }
            // The window turns these into asks of its own, by popup, before the host sees them.
            PlaceAsk::Popup { .. } | PlaceAsk::Withdraw { .. } => {
                report::note!(warn, "a place's popup ask reached its errands: the window makes them its own");
            }
            PlaceAsk::Typing { timeline } => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("typing"), async move {
                    if let Some(link) = link {
                        let _ =
                            link.tell(proto::chatkord_v1::Typing { timeline_id: timeline.as_str().to_owned() }).await;
                    }
                });
            }
            PlaceAsk::PickEmoji => {
                crate::errand::spawn(&self.runtime, report::word!("pick emoji"), async move {
                    let picked = match platform::pick::picture().await {
                        Ok(platform::pick::Picked::File(bytes)) => screens::picture_row::Picked::File(bytes),
                        Ok(platform::pick::Picked::Nothing) => screens::picture_row::Picked::Nothing,
                        Err(platform::Unavailable) => screens::picture_row::Picked::Unreadable,
                    };
                    let message = screens::EmojiPageMessage::Picked(picked);
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::Emoji(message)))).await;
                });
            }
            PlaceAsk::UploadEmoji { bytes } => {
                let link = open_link(&kept);
                crate::errand::spawn(&self.runtime, report::word!("upload emoji"), async move {
                    let answer = match link {
                        Some(link) => link.upload(proto::files::FileKind::Emoji, bytes).await,
                        None => Err(terms::FileError::Reach(terms::LinkError::Offline)),
                    };
                    let message = screens::EmojiPageMessage::Uploaded(answer);
                    mailbox.tell(tell(PlaceMessage::Page(screens::PageMessage::Emoji(message)))).await;
                });
            }
            PlaceAsk::EndAccount { server, identity, sign_out, row } => {
                self.end_account(id, server, identity, sign_out, row);
            }
            PlaceAsk::TrustKey { server, pin } => {
                let platform = self.platform.clone();
                crate::errand::spawn(&self.runtime, report::word!("trust key"), async move {
                    if platform::state::trust_key(&platform, server, pin).await.is_err() {
                        report::note!(warn, "the server's new key was not kept");
                    }
                    mailbox.tell(tell(PlaceMessage::Session(screens::SessionMessage::KeyTrusted))).await;
                });
            }
            PlaceAsk::Wait { attempt, delay } => {
                let wait = crate::errand::spawn(&self.runtime, report::word!("wait to reconnect"), async move {
                    tokio::time::sleep(delay).await;
                    mailbox.tell(tell(PlaceMessage::Session(screens::SessionMessage::Reconnect(attempt)))).await;
                });
                if let Some(earlier) = slot.waiting.replace(wait.abort_handle()) {
                    earlier.abort();
                }
            }
        }
    }
}
