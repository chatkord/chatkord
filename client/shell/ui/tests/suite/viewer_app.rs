//! A picture opened from a message shows over the window: its thumbnail while the original is fetched, the original
//! once the host holds it.

use design::{Accent, Scheme};
use iced_core::Size;
use iced_core::image::Handle;
use proto::files::{AttachmentName, FileId, MediaType, PictureType};
use screens::chat::{Attachment, Picture};
use screens::harness::{self, ADA, Clock};
use screens::{DiscussionMessage, PlaceMessage, WindowMessage};
use std::time::Instant;
use ui::testing::{self, Renderer};
use ui::{Theme, app};

const WINDOW: Size = Size::new(1280.0, 800.0);

fn file(byte: u8) -> FileId {
    FileId::from_bytes([byte; FileId::LEN])
}

fn solid(width: u32, height: u32) -> Handle {
    let rgba = [40u8, 200, 90, 255].repeat((width * height) as usize);
    Handle::from_rgba(width, height, rgba)
}

#[test]
fn a_viewed_picture_shows_its_thumbnail_while_the_original_is_fetched() {
    let clock = Clock(Instant::now());
    let (mut window, owls) = harness::chatting(&clock);
    let (original, thumbnail) = (file(1), file(2));
    let mut message = harness::said(2, 6, ADA, "the board");
    message.attachments = vec![Attachment {
        file: original,
        name: AttachmentName::parse("board.jpg").unwrap(),
        size: 1000,
        media: MediaType::Picture(PictureType::Jpeg),
        picture: Some(Picture { width: 800, height: 600, thumbnail: Some(thumbnail) }),
    }];
    let told = DiscussionMessage::Told { timeline: harness::general_timeline(), message: Box::new(message.clone()) };
    window.update(WindowMessage::At(owls, PlaceMessage::Discussion(told)), clock.at());
    let view = DiscussionMessage::View { message: message.id.clone(), file: original };
    window.update(WindowMessage::At(owls, PlaceMessage::Discussion(view)), clock.at());
    assert!(window.places()[0].discussion().and_then(|open| open.viewed()).is_some(), "the picture is open");
    let held = solid(800, 600);
    let handles = move |asked: FileId| (asked == thumbnail).then(|| held.clone());
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let shown =
        app::view_with::<Renderer>(
            &window,
            WINDOW,
            &handles,
            &|_| iced_core::Widget::_boxed(iced_widget::Space::new()),
        );
    let image = testing::render::<WindowMessage>(&theme, shown, WINDOW, 1.0);
    let centre = image.pixel(640, 430);
    assert!(centre[1] > 150 && centre[0] < 90, "the thumbnail fills the middle: {centre:?}");
}

#[test]
fn a_viewed_picture_shows_its_original_once_held() {
    let clock = Clock(Instant::now());
    let (mut window, owls) = harness::chatting(&clock);
    let (original, thumbnail) = (file(1), file(2));
    let mut message = harness::said(2, 6, ADA, "the board");
    message.attachments = vec![Attachment {
        file: original,
        name: AttachmentName::parse("board.jpg").unwrap(),
        size: 1000,
        media: MediaType::Picture(PictureType::Jpeg),
        picture: Some(Picture { width: 4032, height: 3024, thumbnail: Some(thumbnail) }),
    }];
    let told = DiscussionMessage::Told { timeline: harness::general_timeline(), message: Box::new(message.clone()) };
    window.update(WindowMessage::At(owls, PlaceMessage::Discussion(told)), clock.at());
    let view = DiscussionMessage::View { message: message.id.clone(), file: original };
    let asked = window.update(WindowMessage::At(owls, PlaceMessage::Discussion(view)), clock.at());
    println!("asked on view: {asked:?}");
    window
        .update(WindowMessage::At(owls, PlaceMessage::Server(screens::ServerMessage::FileReady(original))), clock.at());
    println!("fetched: {:?}", window.places()[0].fetched(&original));
    let held = solid(4032, 3024);
    let handles = move |asked: FileId| (asked == original).then(|| held.clone());
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let shown =
        app::view_with::<Renderer>(
            &window,
            WINDOW,
            &handles,
            &|_| iced_core::Widget::_boxed(iced_widget::Space::new()),
        );
    let image = testing::render::<WindowMessage>(&theme, shown, WINDOW, 1.0);
    let centre = image.pixel(640, 430);
    assert!(centre[1] > 150 && centre[0] < 90, "the original fills the middle: {centre:?}");
}
