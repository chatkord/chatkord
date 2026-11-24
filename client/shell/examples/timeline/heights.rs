//! Which messages of a long list to build: those in and around the viewport, between blank spaces for the rest. A
//! height is known only once laid out, so until then it is the average of those measured so far.

/// What to build for a scroll position: the messages `first..end`, and the `before` and `after` pixels round them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Window {
    pub first: usize,
    pub end: usize,
    pub before: f32,
    pub after: f32,
}

pub(crate) struct Heights {
    measured: Vec<Option<f32>>,
    sum: f32,
    count: usize,
    /// The guess before anything is measured.
    first_guess: f32,
}

impl Heights {
    pub(crate) fn new(messages: usize, first_guess: f32) -> Self {
        Self { measured: vec![None; messages], sum: 0.0, count: 0, first_guess }
    }

    fn guess(&self) -> f32 {
        if self.count == 0 { self.first_guess } else { self.sum / self.count as f32 }
    }

    pub(crate) fn height(&self, message: usize) -> f32 {
        self.measured.get(message).copied().flatten().unwrap_or_else(|| self.guess())
    }

    /// Remembers a measure; true when it moved the message by a pixel or more, a change that shows.
    pub(crate) fn measured(&mut self, message: usize, height: f32) -> bool {
        let guess = self.guess();
        let Some(slot) = self.measured.get_mut(message) else { return false };
        let before = slot.unwrap_or(guess);
        match slot.replace(height) {
            Some(old) => self.sum += height - old,
            None => {
                self.sum += height;
                self.count += 1;
            }
        }
        (height - before).abs() >= 1.0
    }

    pub(crate) fn total(&self) -> f32 {
        (0..self.measured.len()).map(|n| self.height(n)).sum()
    }

    /// The messages to build for a viewport `height` tall scrolled `offset` down, `overscan` pixels more each side.
    pub(crate) fn window(&self, offset: f32, height: f32, overscan: f32) -> Window {
        let (from, to) = ((offset - overscan).max(0.0), offset + height + overscan);
        let (mut top, mut first) = (0.0, 0);
        while first < self.measured.len() && top + self.height(first) <= from {
            top += self.height(first);
            first += 1;
        }
        let (mut bottom, mut end) = (top, first);
        while end < self.measured.len() && bottom < to {
            bottom += self.height(end);
            end += 1;
        }
        let after = (end..self.measured.len()).map(|n| self.height(n)).sum();
        Window { first, end, before: top, after }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_built_covers_the_viewport_and_the_spaces_make_up_the_rest() {
        let heights = Heights::new(1_000, 50.0);
        for offset in [0.0, 137.0, 24_000.0, 49_000.0] {
            let w = heights.window(offset, 600.0, 200.0);
            let built: f32 = (w.first..w.end).map(|n| heights.height(n)).sum();
            assert!(
                (w.before + built + w.after - heights.total()).abs() < 0.01,
                "the three parts are the whole list at {offset}"
            );
            assert!(
                w.before <= offset.max(0.0) && w.before + built >= (offset + 600.0).min(heights.total()),
                "the viewport at {offset} is built"
            );
            assert!(w.end - w.first < 40, "a window of a thousand messages built {} of them", w.end - w.first);
        }
    }

    #[test]
    fn a_measure_replaces_the_guess_for_that_message_and_moves_the_guess_for_the_rest() {
        let mut heights = Heights::new(10, 50.0);
        assert!((heights.total() - 500.0).abs() < 0.01);
        assert!(heights.measured(0, 90.0), "a message far from the guess is a change");
        assert!((heights.height(0) - 90.0).abs() < 0.01);
        assert!((heights.height(1) - 90.0).abs() < 0.01, "the rest are guessed at the average of what is measured");
        assert!(!heights.measured(0, 90.4), "less than a pixel is not a change worth a redraw");
        heights.measured(1, 30.0);
        assert!((heights.height(2) - 60.2).abs() < 0.01, "the guess is the average of 90.4 and 30");
    }

    #[test]
    fn the_end_of_the_list_and_an_empty_one_are_windows_and_not_panics() {
        let heights = Heights::new(100, 40.0);
        let w = heights.window(1.0e9, 600.0, 100.0);
        assert_eq!(w.end, 100);
        assert!(w.after.abs() < 0.01);
        let none = Heights::new(0, 40.0);
        assert_eq!(none.window(0.0, 600.0, 100.0), Window { first: 0, end: 0, before: 0.0, after: 0.0 });
    }
}
