//! Which rows of a long list to build: those in and around the viewport, between blank spaces for the rest. A row's
//! height is known only once laid out, so until then it is the average of those measured so far. Heights are kept by
//! row, so a page read above moves nothing measured.

use std::collections::HashMap;

use crate::rows::RowKey;

/// What to build for a scroll position: the rows `first..end`, and the `before` and `after` pixels round them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    /// The first row built.
    pub first: usize,
    /// One past the last row built.
    pub end: usize,
    /// The height of the rows above, drawn as one space.
    pub before: f32,
    /// The height of the rows below, drawn as one space.
    pub after: f32,
}

/// Where a list is scrolled, kept by a row so what the reader sees stays put while rows are measured, read or let go
/// elsewhere: at the newest end, or with the viewport's bottom `offset` pixels below the top of `row`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Scroll {
    /// At the newest end, following what arrives.
    #[default]
    Newest,
    /// The viewport's bottom inside or at the bottom of a row.
    At {
        /// The row.
        row: RowKey,
        /// Pixels from the row's top down to the viewport's bottom, so a row that grows, wrapped narrower, keeps
        /// what was read of it in place.
        offset: f32,
    },
}

/// The measured heights of a list's rows.
#[derive(Clone, Debug)]
pub struct Heights {
    measured: HashMap<RowKey, f32>,
    sum: f32,
    first_guess: f32,
}

impl Heights {
    /// No row measured; until one is, each is guessed `first_guess` tall.
    pub fn new(first_guess: f32) -> Heights {
        Heights { measured: HashMap::new(), sum: 0.0, first_guess }
    }

    fn guess(&self) -> f32 {
        if self.measured.is_empty() { self.first_guess } else { self.sum / self.measured.len() as f32 }
    }

    /// The height of `row`: measured, or guessed.
    pub fn height(&self, row: &RowKey) -> f32 {
        self.measured.get(row).copied().unwrap_or_else(|| self.guess())
    }

    /// Remembers a measure; answers whether it moved the row by a pixel or more, a change that shows.
    pub fn measured(&mut self, row: RowKey, height: f32) -> bool {
        let before = self.height(&row);
        match self.measured.insert(row, height) {
            Some(old) => self.sum += height - old,
            None => self.sum += height,
        }
        (height - before).abs() >= 1.0
    }

    /// Forgets every row not in `kept`, so a list let go of its far end holds no heights for it.
    pub fn retain(&mut self, kept: &[RowKey]) {
        let kept: std::collections::HashSet<&RowKey> = kept.iter().collect();
        self.measured.retain(|row, _| kept.contains(row));
        self.sum = self.measured.values().sum();
    }

    /// The whole height of `rows`.
    pub fn total(&self, rows: &[RowKey]) -> f32 {
        rows.iter().map(|row| self.height(row)).sum()
    }

    /// Where the list is scrolled when the viewport's bottom is `from_end` pixels above the end of `rows`: within half
    /// a pixel of it is the newest end.
    pub fn scroll(&self, rows: &[RowKey], from_end: f32) -> Scroll {
        if from_end < 0.5 {
            return Scroll::Newest;
        }
        let mut below = 0.0;
        for row in rows.iter().rev() {
            let height = self.height(row);
            if below + height > from_end {
                return Scroll::At { row: row.clone(), offset: below + height - from_end };
            }
            below += height;
        }
        match rows.first() {
            Some(top) => Scroll::At { row: top.clone(), offset: 0.0 },
            None => Scroll::Newest,
        }
    }

    /// How far above the end of `rows` the viewport's bottom is when the list is scrolled to `at`; none when its row
    /// is not among them.
    pub fn distance(&self, rows: &[RowKey], at: &Scroll) -> Option<f32> {
        match at {
            Scroll::Newest => Some(0.0),
            Scroll::At { row, offset } => {
                let held = rows.iter().position(|each| each == row)?;
                let below = rows.get(held + 1..).map_or(0.0, |below| self.total(below));
                Some((self.height(row) - offset).max(0.0) + below)
            }
        }
    }

    /// The rows to build of `rows` for a viewport `height` tall whose bottom is `from_end` above the list's end,
    /// `overscan` pixels more each side.
    pub fn window(&self, rows: &[RowKey], from_end: f32, height: f32, overscan: f32) -> Window {
        let offset = (self.total(rows) - height - from_end).max(0.0);
        let (from, to) = ((offset - overscan).max(0.0), offset + height + overscan);
        let (mut top, mut first) = (0.0, 0);
        while let Some(row) = rows.get(first)
            && top + self.height(row) <= from
        {
            top += self.height(row);
            first += 1;
        }
        let (mut bottom, mut end) = (top, first);
        while let Some(row) = rows.get(end)
            && bottom < to
        {
            bottom += self.height(row);
            end += 1;
        }
        let after = rows.get(end..).map_or(0.0, |rest| rest.iter().map(|row| self.height(row)).sum());
        Window { first, end, before: top, after }
    }

    /// [`Heights::window`], widened to build every row of `kept` too: rows holding a selection keep their widgets, so
    /// the selection outlives a scroll that takes them out of view.
    pub fn window_keeping(
        &self,
        rows: &[RowKey],
        from_end: f32,
        height: f32,
        overscan: f32,
        kept: &[RowKey],
    ) -> Window {
        let window = self.window(rows, from_end, height, overscan);
        let held = rows.iter().enumerate().filter(|(_, row)| kept.contains(row)).map(|(at, _)| at);
        let (Some(low), Some(high)) = (held.clone().min(), held.max()) else { return window };
        let (first, end) = (window.first.min(low), window.end.max(high + 1));
        let sum = |range: &[RowKey]| range.iter().map(|row| self.height(row)).sum();
        Window { first, end, before: rows.get(..first).map_or(0.0, sum), after: rows.get(end..).map_or(0.0, sum) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys(count: usize) -> Vec<RowKey> {
        (1..=count as u64).map(RowKey::Message).collect()
    }

    #[test]
    fn a_scroll_and_its_distance_from_the_end_are_one_position() {
        let rows = keys(100);
        let mut heights = Heights::new(50.0);
        for row in &rows {
            heights.measured(row.clone(), 50.0);
        }
        heights.measured(RowKey::Message(98), 120.0);
        assert_eq!(heights.scroll(&rows, 0.0), Scroll::Newest);
        for from_end in [10.0, 49.0, 50.0, 170.0, 171.0, 1_234.5] {
            let at = heights.scroll(&rows, from_end);
            assert_eq!(heights.distance(&rows, &at), Some(from_end), "{from_end}: {at:?}");
        }
        let at = heights.scroll(&rows, 300.0);
        heights.measured(RowKey::Message(99), 90.0);
        let moved = heights.distance(&rows, &at).expect("held");
        assert_eq!(moved, 340.0, "a row measured below moves the distance, never the row in view");
        let Scroll::At { row, .. } = &at else { panic!("scrolled up: {at:?}") };
        heights.measured(row.clone(), heights.height(row) + 30.0);
        let grown = heights.distance(&rows, &at).expect("held");
        assert_eq!(grown, 370.0, "the row in view grown, as wrapped by a narrower window, keeps its top where it was");
        assert_eq!(heights.distance(&rows[..50], &at), None, "a row not held");
    }

    #[test]
    fn what_is_built_covers_the_viewport_and_the_spaces_make_up_the_rest() {
        let (rows, heights) = (keys(1_000), Heights::new(50.0));
        for from_end in [0.0, 137.0, 24_000.0, 49_000.0] {
            let w = heights.window(&rows, from_end, 600.0, 200.0);
            let built: f32 = rows[w.first..w.end].iter().map(|row| heights.height(row)).sum();
            assert!((w.before + built + w.after - heights.total(&rows)).abs() < 0.01, "the whole list at {from_end}");
            let top = (heights.total(&rows) - 600.0 - from_end).max(0.0);
            assert!(w.before <= top && w.before + built >= (top + 600.0).min(heights.total(&rows)), "{from_end}");
            assert!(w.end - w.first < 40, "a window of a thousand rows built {} of them", w.end - w.first);
        }
    }

    #[test]
    fn a_page_read_above_moves_nothing_at_the_bottom() {
        let mut heights = Heights::new(50.0);
        let rows = keys(200);
        for row in &rows[150..] {
            heights.measured(row.clone(), 80.0);
        }
        let shown = heights.window(&rows[100..], 0.0, 600.0, 0.0);
        let after_page = heights.window(&rows, 0.0, 600.0, 0.0);
        assert_eq!(after_page.end - after_page.first, shown.end - shown.first);
        assert_eq!(rows[100 + shown.first], rows[after_page.first], "the same rows are at the bottom");
    }

    #[test]
    fn a_measure_replaces_the_guess_and_moves_the_guess_for_the_rest() {
        let mut heights = Heights::new(50.0);
        let rows = keys(10);
        assert!((heights.total(&rows) - 500.0).abs() < 0.01);
        assert!(heights.measured(rows[0].clone(), 90.0), "far from the guess: a change");
        assert!((heights.height(&rows[1]) - 90.0).abs() < 0.01, "the rest guessed at the average measured");
        assert!(!heights.measured(rows[0].clone(), 90.4), "less than a pixel is not worth a redraw");
        heights.measured(rows[1].clone(), 30.0);
        assert!((heights.height(&rows[2]) - 60.2).abs() < 0.01);
        heights.retain(&rows[1..2]);
        assert!((heights.height(&rows[2]) - 30.0).abs() < 0.01, "a let-go row's height leaves the average");
    }

    #[test]
    fn rows_holding_a_selection_are_built_wherever_the_viewport_is() {
        let (rows, heights) = (keys(1_000), Heights::new(50.0));
        let near_end = heights.window(&rows, 0.0, 600.0, 0.0);
        let kept = [rows[10].clone(), rows[12].clone()];
        let window = heights.window_keeping(&rows, 0.0, 600.0, 0.0, &kept);
        assert_eq!((window.first, window.end), (10, near_end.end), "from the first held row to the viewport");
        let built: f32 = rows[window.first..window.end].iter().map(|row| heights.height(row)).sum();
        assert!((window.before + built + window.after - heights.total(&rows)).abs() < 0.01);
        assert_eq!(heights.window_keeping(&rows, 0.0, 600.0, 0.0, &[]), near_end, "nothing held: the plain window");
    }

    #[test]
    fn the_end_of_the_list_and_an_empty_one_are_windows() {
        let (rows, heights) = (keys(100), Heights::new(40.0));
        let w = heights.window(&rows, 0.0, 600.0, 100.0);
        assert_eq!(w.end, 100);
        assert!(w.after.abs() < 0.01);
        assert_eq!(heights.window(&[], 0.0, 600.0, 100.0), Window { first: 0, end: 0, before: 0.0, after: 0.0 });
        assert_eq!(heights.window(&rows, 1.0e9, 600.0, 0.0).first, 0, "scrolled past the top: the top");
    }
}
