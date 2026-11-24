//! What the place does with each message of a discussion, from the composer to the list.

use super::*;

impl Place {
    /// What the server tells of a timeline reaches each discussion open on it, the call's beside the content's.
    fn told_everywhere(&mut self, message: &DiscussionMessage, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        let mut todo = Vec::new();
        if let DiscussionMessage::Told { timeline, message } = message {
            todo.extend(self.count_told(timeline, message, told, at.wall));
        }
        for pane in Pane::ALL {
            let open = self.pane_mut(pane).and_then(|open| open.timeline_id().cloned());
            let timeline = match message {
                DiscussionMessage::Told { timeline, .. }
                | DiscussionMessage::Withdrawn { timeline, .. }
                | DiscussionMessage::ReactionsOfYours { timeline, .. } => timeline,
                _ => continue,
            };
            if open.as_ref() == Some(timeline) {
                todo.extend(self.on_pane(pane, message.clone(), at));
            }
        }
        if let DiscussionMessage::Told { message, .. } = message {
            todo.extend(self.thread_begun(message));
        }
        todo
    }

    /// `root` told with a thread: if the pane beside shows that thread yet to begin, what its composer holds moves to
    /// the thread's timeline, which the pane then reads.
    fn thread_begun(&mut self, root: &chat::Message) -> Option<Ask> {
        let summary = root.thread.as_ref()?;
        let begun = Compose::NewThread(root.id.clone());
        let first = self.pane_mut(Pane::Thread).filter(|open| open.compose == begun)?.begun(summary.timeline.clone());
        if let Some(mut held) = self.held.remove(&begun) {
            let kept = self.held.entry(Compose::Timeline(summary.timeline.clone())).or_default();
            kept.composing = std::mem::take(&mut held.composing);
            if !held.unsent.is_empty() {
                let unsent = std::mem::take(&mut held.unsent);
                self.held.insert(begun, Held { unsent, ..Held::default() });
            }
        }
        first
    }

    /// The open channel: only a signed-in server's, but for a send's answer, which finds its channel wherever it is,
    /// and for what is told of a timeline, which reaches it wherever it is open.
    pub(crate) fn on_discussion(
        &mut self,
        pane: Pane,
        message: DiscussionMessage,
        told: &mut Option<Told>,
        at: Moment,
    ) -> Vec<Ask> {
        match message {
            DiscussionMessage::Picker(message) => {
                let grants = self.discussion_grants(pane);
                let (todo, picked) = self.on_picker(pane, message, grants);
                *told = picked.map(Told::Picked).or(told.take());
                todo
            }
            DiscussionMessage::Sent { timeline, thread_of, nonce, sent } => {
                self.sent(&timeline, thread_of.as_ref(), nonce, sent.map(|sent| *sent));
                Vec::new()
            }
            DiscussionMessage::Typing { timeline, users } => {
                self.typing_now(timeline, users, at.now);
                Vec::new()
            }
            DiscussionMessage::Told { .. }
            | DiscussionMessage::Withdrawn { .. }
            | DiscussionMessage::ReactionsOfYours { .. } => self.told_everywhere(&message, told, at),
            // A composer's answers find their composer whatever is open.
            DiscussionMessage::Picked { compose, picked } => match picked {
                Ok(paths) => self.attach(&compose, paths),
                Err(NoPicker) => {
                    let composing = &mut self.held.entry(compose).or_default().composing;
                    composing.notice = Some(Sentence::Phrase(Phrase::NoFilePicker));
                    Vec::new()
                }
            },
            DiscussionMessage::Upload { compose, upload, said } => self.uploaded(&compose, upload, said),
            DiscussionMessage::Previewed { compose, link, made } => {
                self.previewed(&compose, &link, made);
                Vec::new()
            }
            // An answer about a timeline is for the pane showing it, whichever pane asked.
            DiscussionMessage::Page { ref timeline, .. }
            | DiscussionMessage::CaughtUp { ref timeline, .. }
            | DiscussionMessage::Pins { ref timeline, .. }
            | DiscussionMessage::Quoted { ref timeline, .. } => {
                let shown = self.pane_showing(timeline).unwrap_or(pane);
                self.on_pane(shown, message, at)
            }
            other => self.on_pane(pane, other, at),
        }
    }

    /// The pane whose open channel's timeline is `timeline`, if one is.
    fn pane_showing(&self, timeline: &TimelineId) -> Option<Pane> {
        Pane::ALL.into_iter().find(|pane| self.in_pane(*pane).is_some_and(|open| open.shows(timeline)))
    }

    /// What is done in the discussion `pane` names, or reported for it.
    fn on_pane(&mut self, pane: Pane, message: DiscussionMessage, at: Moment) -> Vec<Ask> {
        let Some(compose) = self.pane_mut(pane).map(|open| open.compose.clone()) else { return Vec::new() };
        let (me, grants) = (self.me().cloned(), self.discussion_grants(pane));
        // The composer's own, which reach its draft wherever the channel is open.
        match message {
            DiscussionMessage::Draft(text) => self.drafted(&compose, text, grants.holds(Grant::SendMessage), at.now),
            DiscussionMessage::Caret(caret) => {
                let composing = &mut self.held.entry(compose).or_default().composing;
                composing.caret = Some(caret);
                Vec::new()
            }
            DiscussionMessage::ChooseNext => {
                self.select_choice(&compose, 1);
                Vec::new()
            }
            DiscussionMessage::ChoosePrevious => {
                self.select_choice(&compose, -1);
                Vec::new()
            }
            DiscussionMessage::Choose(index) => {
                self.choose(&compose, index);
                Vec::new()
            }
            DiscussionMessage::Escape => {
                self.escape(&compose);
                Vec::new()
            }
            DiscussionMessage::MentionAuthor => {
                let composing = &mut self.held.entry(compose).or_default().composing;
                if let Some(reply) = composing.reply.as_mut() {
                    reply.mention_author = !reply.mention_author;
                }
                Vec::new()
            }
            DiscussionMessage::CancelReply => {
                self.held.entry(compose).or_default().composing.reply = None;
                Vec::new()
            }
            DiscussionMessage::Attach if grants.holds(Grant::AttachFiles) => {
                vec![Ask::PickFiles { compose }]
            }
            DiscussionMessage::Dropped(paths) if grants.holds(Grant::AttachFiles) => {
                self.held.entry(compose.clone()).or_default().composing.focus_asked += 1;
                self.attach(&compose, paths)
            }
            DiscussionMessage::Attach | DiscussionMessage::Dropped(_) => Vec::new(),
            DiscussionMessage::RemoveFile(upload) => self.remove_file(&compose, upload),
            DiscussionMessage::RemovePreview => {
                self.preview_removed(&compose);
                Vec::new()
            }
            DiscussionMessage::Send => self.send(pane, &compose, grants, at),
            other => self.on_open(pane, other, me.as_ref(), grants),
        }
    }

    /// What is done in, or reported for, the discussion `pane` names, past its composer.
    pub(super) fn on_open(
        &mut self,
        pane: Pane,
        message: DiscussionMessage,
        me: Option<&MemberId>,
        grants: Grants,
    ) -> Vec<Ask> {
        let mut todo = match self.open_arm(pane, message, me, grants) {
            ControlFlow::Break(done) => return done,
            ControlFlow::Continue(todo) => todo,
        };
        let Some(open) = self.pane_mut(pane) else { return todo };
        todo.extend(open.quotes_wanted());
        let mut wanted = open.thumbnails();
        // The original of the picture open over the window, which shows its thumbnail until it is held.
        wanted.extend(open.viewing.as_ref().map(|viewing| (viewing.file, proto::files::FileKind::Attachment)));
        todo.extend(self.fetch_files(wanted));
        todo
    }

    fn open_arm(
        &mut self,
        pane: Pane,
        message: DiscussionMessage,
        me: Option<&MemberId>,
        grants: Grants,
    ) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
        let previews_served = self.served.on(proto::rules::Switch::LinkPreviews);
        let markable =
            if matches!(message, DiscussionMessage::SaveEdit) { self.markable_channels() } else { Vec::new() };
        let Some(open) = self.pane_mut(pane) else { return ControlFlow::Break(Vec::new()) };
        // A choice made in the menu, or anything else done in the channel, puts it away.
        if !matches!(
            message,
            DiscussionMessage::Menu(_)
                | DiscussionMessage::Measured { .. }
                | DiscussionMessage::HeadMeasured { .. }
                | DiscussionMessage::Told { .. }
                | DiscussionMessage::Withdrawn { .. }
                | DiscussionMessage::Selected(_)
                | DiscussionMessage::Quoted { .. }
                | DiscussionMessage::ReactionsOfYours { .. }
        ) {
            open.menu = None;
        }
        let todo = match message {
            // The window opens a channel and copies text: it holds the time zone and the clipboard.
            DiscussionMessage::Open(_) | DiscussionMessage::Copy(_) | DiscussionMessage::CloseMenu => Vec::new(),
            DiscussionMessage::Selected(rows) => {
                open.selected = rows;
                Vec::new()
            }
            DiscussionMessage::Menu(id) => {
                if open.timeline().and_then(|timeline| timeline.get(&id)).is_some_and(|message| !message.deleted()) {
                    open.menu = Some(id);
                }
                Vec::new()
            }
            DiscussionMessage::Page { timeline, end, page } if open.shows(&timeline) => page_read(open, end, page),
            DiscussionMessage::Told { timeline, message } if open.shows(&timeline) => message_told(open, message),
            DiscussionMessage::Withdrawn { timeline, withdrawn } if open.shows(&timeline) => {
                message_withdrawn(open, withdrawn)
            }
            DiscussionMessage::ReactionsOfYours { timeline, message, revision, mine } if open.shows(&timeline) => {
                reactions_of_yours(open, message, revision, mine)
            }
            DiscussionMessage::CaughtUp { timeline, changes } if open.shows(&timeline) => {
                caught_up(open, timeline, changes)?
            }
            DiscussionMessage::Scrolled { from_end, viewport } => scrolled(open, from_end, viewport),
            DiscussionMessage::HeadMeasured { height } => {
                open.head = height;
                Vec::new()
            }
            DiscussionMessage::Measured { row, height } => row_measured(open, row, height),
            DiscussionMessage::JumpToPresent | DiscussionMessage::Retry => reload(open)?,
            DiscussionMessage::Jump(place) => jump_to(open, place),
            DiscussionMessage::Reply(id) => {
                let target =
                    open.timeline().and_then(|timeline| timeline.get(&id)).filter(|message| !message.deleted());
                let Some(place) = target.filter(|_| grants.holds(Grant::SendMessage)).map(|message| message.place)
                else {
                    return ControlFlow::Break(Vec::new());
                };
                let compose = open.compose.clone();
                let composing = &mut self.held.entry(compose).or_default().composing;
                composing.reply = Some(Replying { to: id, place, mention_author: true });
                composing.focus_asked += 1;
                Vec::new()
            }
            DiscussionMessage::Edit(id) => edit_started(open, id, me, grants),
            DiscussionMessage::EditDraft(text) => edit_typed(open, text),
            DiscussionMessage::SaveEdit => edit_saved(open, &markable, previews_served)?,
            DiscussionMessage::CancelEdit => {
                open.editing = None;
                Vec::new()
            }
            DiscussionMessage::Changed(changed) => edit_answered(open, changed),
            // The window asks first, and tells the place once it is confirmed.
            DiscussionMessage::Delete(_) => Vec::new(),
            DiscussionMessage::DeleteConfirmed(id) => delete_confirmed(open, &id, me, grants)?,
            DiscussionMessage::React(id, emoji) => react(open, &id, &emoji, grants),
            DiscussionMessage::Reaction(id, emoji) => toggle_reaction(open, &id, &emoji, grants)?,
            DiscussionMessage::Reacted(answer) | DiscussionMessage::Pinned(answer) => answered(open, answer),
            DiscussionMessage::Pin(id) | DiscussionMessage::Unpin(id) if grants.holds(Grant::PinMessages) => {
                pin_asked(open, &id)?
            }
            DiscussionMessage::Pin(_) | DiscussionMessage::Unpin(_) => Vec::new(),
            DiscussionMessage::TogglePins => toggle_pins(open)?,
            DiscussionMessage::MorePins => more_pins(open)?,
            DiscussionMessage::Pins { timeline, page } if open.shows(&timeline) => pins_read(open, page)?,
            DiscussionMessage::Quoted { timeline, place, page } if open.shows(&timeline) => {
                quote_read(open, place, page)?
            }
            DiscussionMessage::Reveal(id, spoiler) => {
                open.revealed.insert((id, spoiler));
                Vec::new()
            }
            DiscussionMessage::View { message, file } => view_opened(open, message, file),
            DiscussionMessage::CloseViewer => {
                open.viewing = None;
                Vec::new()
            }
            DiscussionMessage::Save { file, name } => {
                open.notice = None;
                vec![Ask::SaveFile { file, name }]
            }
            DiscussionMessage::Saved { saved, .. } => file_saved(open, saved),
            DiscussionMessage::NotOpened => {
                open.notice = Some(Sentence::Phrase(Phrase::LinkNotOpened));
                Vec::new()
            }
            // A report for a channel no longer open reaches nothing; the composer's and a send's were taken above.
            DiscussionMessage::Page { .. }
            | DiscussionMessage::Told { .. }
            | DiscussionMessage::Withdrawn { .. }
            | DiscussionMessage::CaughtUp { .. }
            | DiscussionMessage::ReactionsOfYours { .. }
            | DiscussionMessage::Pins { .. }
            | DiscussionMessage::Quoted { .. }
            | DiscussionMessage::Sent { .. }
            | DiscussionMessage::Typing { .. }
            | DiscussionMessage::Draft(_)
            | DiscussionMessage::Caret(_)
            | DiscussionMessage::ChooseNext
            | DiscussionMessage::ChoosePrevious
            | DiscussionMessage::Choose(_)
            | DiscussionMessage::Escape
            | DiscussionMessage::MentionAuthor
            | DiscussionMessage::CancelReply
            | DiscussionMessage::Attach
            | DiscussionMessage::Picked { .. }
            | DiscussionMessage::Dropped(_)
            | DiscussionMessage::Upload { .. }
            | DiscussionMessage::RemoveFile(_)
            | DiscussionMessage::RemovePreview
            | DiscussionMessage::Previewed { .. }
            | DiscussionMessage::Picker(_)
            | DiscussionMessage::Send => Vec::new(),
        };
        ControlFlow::Continue(todo)
    }
}
