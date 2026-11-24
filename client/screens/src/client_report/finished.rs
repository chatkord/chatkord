//! The figures of the last calls that ended, kept by the window: at most [`KEPT_CALLS`], the oldest dropped.

use crate::call::health::{CallFigures, KEPT_CALLS};
use std::collections::VecDeque;

/// The last calls, each with the address of the server it was on, which the report turns into a stand-in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Finished {
    calls: VecDeque<(String, CallFigures)>,
}

impl Finished {
    /// A call that ended on the server at `address`.
    pub(crate) fn push(&mut self, address: &str, figures: CallFigures) {
        if self.calls.len() == KEPT_CALLS {
            self.calls.pop_front();
        }
        self.calls.push_back((address.to_owned(), figures));
    }

    /// The calls, oldest first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &CallFigures)> {
        self.calls.iter().map(|(address, figures)| (address.as_str(), figures))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::health::CallLooks;

    #[test]
    fn the_last_five_calls_are_kept_and_the_oldest_dropped() {
        let mut finished = Finished::default();
        for at in 0..KEPT_CALLS + 2 {
            let mut figures = CallFigures::of(&CallLooks::default());
            figures.looks = u64::try_from(at).unwrap();
            finished.push("server", figures);
        }
        let kept: Vec<u64> = finished.iter().map(|(_, figures)| figures.looks).collect();
        assert_eq!(kept, [2, 3, 4, 5, 6]);
    }
}
