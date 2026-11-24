//! The server's own emoji, as every member sees them: read whole at each sign-in, page by page, kept current by what the
//! server tells, and their pictures fetched as the window shows them. One list the composer's `:`, the picker and a
//! message's marks and reactions read.

use super::*;
use chat::EmojiId;

/// A custom emoji of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerEmoji {
    /// Its id, which spans and reactions name.
    pub id: EmojiId,
    /// Its name, written `:name:` in a body.
    pub name: proto::rules::EmojiName,
    /// Its picture, a 128-pixel square.
    pub file: FileId,
    /// Who added it; none once their account is gone.
    pub made_by: Option<MemberId>,
}

impl ServerEmoji {
    /// The emoji as the wire states it; none out of shape, a server not keeping the protocol.
    pub fn read(told: &proto::chatkord_v1::Emoji) -> Option<ServerEmoji> {
        Some(ServerEmoji {
            id: EmojiId::parse(&told.emoji_id)?,
            name: proto::rules::emoji_name(&told.name).ok()?,
            file: FileId::parse(&told.file_id).ok()?,
            made_by: told.made_by.as_deref().and_then(MemberId::parse),
        })
    }

    /// The text a body writes it as: `:name:`.
    pub fn written(&self) -> String {
        format!(":{}:", self.name.as_str())
    }
}

/// One page of the server's emoji, read at the edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmojiPage {
    /// The emoji, by id; one out of shape is passed over.
    pub emoji: Vec<ServerEmoji>,
    /// Where the next page starts; none on the last.
    pub next: Option<crate::Cursor>,
}

impl From<&proto::chatkord_v1::EmojiPage> for EmojiPage {
    fn from(page: &proto::chatkord_v1::EmojiPage) -> EmojiPage {
        let next =
            page.more.then(|| page.emoji.last().map(|last| crate::Cursor::from(last.emoji_id.clone()))).flatten();
        EmojiPage { emoji: page.emoji.iter().filter_map(ServerEmoji::read).collect(), next }
    }
}

/// What the server told of its emoji, or a page of them read.
#[derive(Clone, Debug)]
pub enum EmojiMessage {
    /// A page of the list, or why not.
    Page(Result<EmojiPage, LinkError>),
    /// An emoji was added.
    Added(ServerEmoji),
    /// An emoji was renamed.
    Renamed {
        /// Its id.
        id: EmojiId,
        /// Its name now.
        name: proto::rules::EmojiName,
    },
    /// An emoji was deleted, with its reactions.
    Removed(EmojiId),
}

/// The server's emoji, as last read and told.
#[derive(Clone, Debug, Default)]
pub struct EmojiList {
    /// By name, as the picker lists them.
    kept: Vec<ServerEmoji>,
    /// The pages read so far of a reading under way; what is told meanwhile reaches them too.
    reading: Option<crate::Paged<Vec<ServerEmoji>>>,
}

impl EmojiList {
    /// Every emoji, by name.
    pub fn all(&self) -> &[ServerEmoji] {
        &self.kept
    }

    /// The emoji `id` names, if the server holds it.
    pub fn get(&self, id: &EmojiId) -> Option<&ServerEmoji> {
        self.kept.iter().find(|emoji| emoji.id == *id)
    }

    /// The emoji whose name holds `query`, ignoring case, those it starts first, each group by name.
    pub fn search(&self, query: &str) -> Vec<&ServerEmoji> {
        let query = query.to_lowercase();
        let named = |emoji: &&ServerEmoji| emoji.name.as_str().to_lowercase();
        let (mut starts, mut holds): (Vec<&ServerEmoji>, Vec<&ServerEmoji>) = self
            .kept
            .iter()
            .filter(|emoji| named(emoji).contains(&query))
            .partition(|emoji| named(emoji).starts_with(&query));
        starts.append(&mut holds);
        starts
    }

    /// The first page asked again: at a sign-in, or once events were dropped.
    pub(super) fn read_again(&mut self) -> Ask {
        self.reading = Some(crate::Paged::new());
        ops::list_emoji(None)
    }

    /// What `message` changes, and the next page or the pictures it asks for.
    pub(super) fn update(&mut self, message: EmojiMessage) -> Vec<Ask> {
        match message {
            EmojiMessage::Page(Ok(page)) => {
                let Some(read) = self.reading.as_mut() else { return Vec::new() };
                read.extend(page.emoji, page.next);
                match read.next().cloned() {
                    Some(after) => return vec![ops::list_emoji(Some(after))],
                    None => {
                        self.kept = self.reading.take().map(crate::Paged::into_items).unwrap_or_default();
                        self.arrange();
                    }
                }
            }
            // What was kept stays; the next sign-in reads it again.
            EmojiMessage::Page(Err(_)) => self.reading = None,
            EmojiMessage::Added(emoji) => {
                for list in self.lists() {
                    list.retain(|held| held.id != emoji.id);
                    list.push(emoji.clone());
                }
                self.arrange();
            }
            EmojiMessage::Renamed { id, name } => {
                for list in self.lists() {
                    for held in list.iter_mut().filter(|held| held.id == id) {
                        held.name = name.clone();
                    }
                }
                self.arrange();
            }
            EmojiMessage::Removed(id) => {
                for list in self.lists() {
                    list.retain(|held| held.id != id);
                }
            }
        }
        Vec::new()
    }

    fn lists(&mut self) -> impl Iterator<Item = &mut Vec<ServerEmoji>> {
        std::iter::once(&mut self.kept).chain(self.reading.as_mut().map(crate::Paged::items_mut))
    }

    fn arrange(&mut self) {
        self.kept.sort_by_key(|emoji| emoji.name.as_str().to_lowercase());
    }
}

impl Place {
    /// The server's emoji.
    pub fn emoji(&self) -> &EmojiList {
        &self.emoji
    }

    /// Whether the host holds `file` ready to paint: an emoji's picture, a thumbnail.
    pub fn file_ready(&self, file: &FileId) -> bool {
        matches!(self.files.get(file), Some(Fetched::Ready))
    }

    /// Where the fetch of `file` stands: none before it is asked.
    pub fn fetched(&self, file: &FileId) -> Option<Fetched> {
        self.files.get(file).copied()
    }

    /// Fetches of the pictures in `wanted` not asked yet, each asked once a run.
    pub(crate) fn fetch_files(
        &mut self,
        wanted: impl IntoIterator<Item = (FileId, proto::files::FileKind)>,
    ) -> Vec<Ask> {
        wanted
            .into_iter()
            .filter_map(|(file, kind)| match self.files.entry(file) {
                std::collections::hash_map::Entry::Occupied(_) => None,
                std::collections::hash_map::Entry::Vacant(free) => {
                    free.insert(Fetched::Asked);
                    self.kinds.insert(file, kind);
                    Some(Ask::FetchFile { file, kind })
                }
            })
            .collect()
    }

    /// Server settings › Emoji: what is done there, by a holder of Manage emoji while the page is open.
    pub(super) fn on_emoji_page(&mut self, message: crate::emoji_settings::Message) -> Vec<Ask> {
        use crate::emoji_settings::Ask as PageAsk;
        if !self.grants().holds(Grant::ManageEmoji) {
            return Vec::new();
        }
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let Some(open) = opened::<EmojiSettings>(&mut signed.page) else {
            return Vec::new();
        };
        let list = &self.emoji;
        let asked = open.update(message, list.all().len(), |id| list.get(id).map(|emoji| emoji.name.clone()));
        let mut todo = Vec::new();
        for ask in asked {
            match ask {
                PageAsk::Pick => todo.push(Ask::PickEmoji),
                PageAsk::Upload(bytes) => todo.push(Ask::UploadEmoji { bytes }),
                PageAsk::Fetch(file) => todo.extend(self.fetch_files([(file, proto::files::FileKind::Emoji)])),
                PageAsk::Add { name, file } => todo.push(ops::add_emoji(&name, file)),
                PageAsk::Rename { emoji, name } => todo.push(ops::rename_emoji(&emoji, &name)),
            }
        }
        todo
    }

    /// The server's emoji: told or read.
    pub(super) fn on_emoji(&mut self, message: EmojiMessage) -> Vec<Ask> {
        let mut todo = self.emoji.update(message);
        let pictures: Vec<(FileId, proto::files::FileKind)> =
            self.emoji.all().iter().map(|emoji| (emoji.file, proto::files::FileKind::Emoji)).collect();
        todo.extend(self.fetch_files(pictures));
        todo
    }
}

/// Where a picture the window shows is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// Asked for.
    Asked,
    /// Held by the host to paint.
    Ready,
    /// Could not be had this run.
    Failed,
}
