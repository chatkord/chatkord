//! The time zone this device keeps and the day it is there, so a message's day and time are said where the person is.

use std::time::Duration;

/// The system's time zone, or UTC where it cannot be read; read once, since a person's zone rarely moves mid-run.
pub fn local() -> jiff::tz::TimeZone {
    jiff::tz::TimeZone::system()
}

/// The day it is now in `zone`, and how long until the next one starts there; a day the calendar cannot hold ends in
/// an hour, so the caller asks again.
pub fn today(zone: &jiff::tz::TimeZone) -> (jiff::civil::Date, Duration) {
    let now = jiff::Timestamp::now().to_zoned_datetime(zone.clone());
    let today = now.date();
    let next = today.tomorrow().and_then(|tomorrow| tomorrow.to_zoned_datetime(zone.clone()));
    let wait = next.map_or(Duration::from_secs(3_600), |next| now.duration_until(&next).unsigned_abs());
    (today, wait)
}
