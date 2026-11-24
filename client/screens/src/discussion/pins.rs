//! The pinned messages of a discussion and pinning one.

use super::*;
use crate::place::ops;

pub(super) fn pin_asked(open: &mut Discussion, id: &MessageId) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let message = open.timeline().and_then(|timeline| timeline.get(id));
    let pinned = message.is_some_and(|message| message.pinned_at.is_some());
    let pins = matches!(message, Some(message) if !message.deleted());
    let held_in_pins = open.pins.as_ref().is_some_and(|pins| pins.pinned.items().iter().any(|held| held.id == *id));
    if !(pins || held_in_pins) {
        return ControlFlow::Break(Vec::new());
    }
    ControlFlow::Continue(vec![ops::pin(id, !(pinned || held_in_pins))])
}

pub(super) fn toggle_pins(open: &mut Discussion) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    if open.pins.take().is_some() {
        return ControlFlow::Break(Vec::new());
    }
    let Some(timeline) = open.timeline_id().cloned() else { return ControlFlow::Break(Vec::new()) };
    open.pins = Some(Pins { reading: true, ..Pins::default() });
    ControlFlow::Continue(vec![ops::list_pins(timeline, None)])
}

pub(super) fn more_pins(open: &mut Discussion) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let Some(pins) = open.pins.as_mut().filter(|pins| !pins.reading) else { return ControlFlow::Break(Vec::new()) };
    let Some(before) = pins.pinned.next().cloned() else { return ControlFlow::Break(Vec::new()) };
    pins.reading = true;
    let Some(timeline) = open.timeline_id().cloned() else { return ControlFlow::Break(Vec::new()) };
    ControlFlow::Continue(vec![ops::list_pins(timeline, Some(before))])
}

pub(super) fn pins_read(open: &mut Discussion, page: Result<chat::Page, LinkError>) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let Some(pins) = open.pins.as_mut() else { return ControlFlow::Break(Vec::new()) };
    pins.reading = false;
    match page {
        Ok(page) => {
            pins.failed = None;
            let held = pins.pinned.items();
            let fresh: Vec<chat::Message> =
                page.messages.into_iter().filter(|message| !held.iter().any(|held| held.id == message.id)).collect();
            let last = fresh.last().or(held.last());
            let before = last.and_then(|last| {
                Some(proto::chatkord_v1::PinsBefore { pinned_at: last.pinned_at?, place: last.place })
            });
            pins.pinned.extend(fresh, before.filter(|_| page.older));
        }
        Err(failed) => pins.failed = Some(failed),
    }
    ControlFlow::Continue(Vec::new())
}
