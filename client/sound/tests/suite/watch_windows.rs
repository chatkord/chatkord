//! The system's sound devices heard through Windows' endpoint notifications: the holder that starts the listening is
//! told once of what is there, and a later holder only of a change. A real device coming is checked by hand.

use sound::Sound;
use sound::watch::DEVICE_SETTLE;
use std::time::Duration;

#[tokio::test]
async fn the_first_holder_is_told_what_is_there_once_and_a_later_one_is_not() {
    let mut first = Sound::shared().watch().await.expect("the endpoints' notifications");
    let told = tokio::time::timeout(DEVICE_SETTLE * 4, first.changed()).await;
    assert!(told.is_ok(), "the holder that started the listening is told of the devices already there");
    let mut second = Sound::shared().watch().await.expect("the same listener");
    let quiet = tokio::time::timeout(DEVICE_SETTLE * 2 + Duration::from_millis(200), second.changed()).await;
    assert!(quiet.is_err(), "a later holder is told nothing until something changes");
}
