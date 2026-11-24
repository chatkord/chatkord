//! How the connection is: the looks the call keeps and the figures it comes to.

use super::*;

impl Call {
    /// How the connection is, as of the last look.
    pub fn health(&self) -> Option<&Health> {
        self.health.as_ref()
    }

    /// The round trips of the last looks.
    pub fn round_trips(&self) -> &RoundTrips {
        &self.round_trips
    }

    /// What the call came to so far, as the debug report states a finished one.
    pub(crate) fn figures(&self) -> crate::call::health::CallFigures {
        crate::call::health::CallFigures::of(&self.looks)
    }
}

impl Place {
    /// The figures of the calls that ended here since this was last asked, oldest first.
    pub(crate) fn take_finished(&mut self) -> Vec<crate::call::health::CallFigures> {
        match &mut self.status {
            Status::Signed(signed) => std::mem::take(&mut signed.finished),
            Status::Door(_) | Status::Reconnecting(_) => Vec::new(),
        }
    }

    pub(super) fn health_looked(&mut self, health: Health) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.round_trips.push(health.round_trip);
            call.looks.push(&health);
            call.health = Some(health);
        }
        Vec::new()
    }
}
