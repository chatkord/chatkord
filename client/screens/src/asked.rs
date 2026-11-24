//! Where one ask a screen makes of a server stands: never asked or answered, asked and waiting, or refused for a
//! reason the screen says. One state, so an ask asking and refused at once cannot be held.

/// An ask's progress, `R` being why the server said no.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Asked<R> {
    /// Nothing asked, or the last ask was taken.
    #[default]
    Idle,
    /// Asked; no answer yet.
    Asking,
    /// Refused, for this reason.
    Refused(R),
}

impl<R: Copy> Asked<R> {
    /// Whether an answer is awaited.
    pub fn asking(&self) -> bool {
        matches!(self, Self::Asking)
    }

    /// Why the last ask was refused; none while asking or once one was taken.
    pub fn refused(&self) -> Option<R> {
        match self {
            Self::Refused(why) => Some(*why),
            Self::Idle | Self::Asking => None,
        }
    }

    /// Asks again: the last refusal goes.
    pub(crate) fn ask(&mut self) {
        *self = Self::Asking;
    }

    /// The answer: taken, or refused for `why`.
    pub(crate) fn answered(&mut self, refused: Option<R>) {
        *self = refused.map_or(Self::Idle, Self::Refused);
    }

    /// Forgets the last refusal, as typing again does; an ask still waiting stays.
    pub(crate) fn forget(&mut self) {
        if let Self::Refused(_) = self {
            *self = Self::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ask_is_idle_asking_or_refused_and_never_two_at_once() {
        let mut asked = Asked::<u8>::default();
        assert_eq!((asked.asking(), asked.refused()), (false, None));
        asked.ask();
        assert_eq!((asked.asking(), asked.refused()), (true, None));
        asked.answered(Some(7));
        assert_eq!((asked.asking(), asked.refused()), (false, Some(7)));
        asked.ask();
        assert_eq!(asked.refused(), None, "asking again drops the last refusal");
        asked.forget();
        assert!(asked.asking(), "forgetting a refusal never forgets an ask in flight");
        asked.answered(None);
        assert_eq!(asked, Asked::Idle);
    }
}
