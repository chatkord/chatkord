//! The picture viewer, a file saved and what a link did.

use super::*;

pub(super) fn view_opened(open: &mut Discussion, message: MessageId, file: FileId) -> Vec<Ask> {
    let picture = open
        .timeline()
        .and_then(|timeline| timeline.get(&message))
        .is_some_and(|held| held.attachments.iter().any(|sent| sent.file == file && sent.picture.is_some()));
    if picture {
        open.viewing = Some(Viewing { message, file });
    }
    Vec::new()
}

pub(super) fn file_saved(open: &mut Discussion, saved: Result<SaveAnswer, terms::FileError>) -> Vec<Ask> {
    open.notice = match saved {
        Ok(SaveAnswer::Saved | SaveAnswer::Dismissed) => None,
        Ok(SaveAnswer::NoDialog) => Some(Sentence::Phrase(Phrase::NoSaveDialog)),
        Err(failed) => Some(failure::file(&failed)),
    };
    Vec::new()
}
