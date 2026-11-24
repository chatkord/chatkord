//! The steps a share comes down when its encoder or its rate cannot hold what was chosen, and goes back up when it
//! can. As other apps send a screen (libwebrtc's resolution-keeping preference, which LiveKit and Sharkord ask for),
//! the frame rate halves first, down to 15 a second, and only then the size comes down, so text stays sharp longest.
//! A share also begins on the first step whose floor in the [`Envelope`] table fits under the ceiling it was given:
//! a rate chosen too low for what a screen needs makes a slower picture, not a smeared one.

use crate::Envelope;
use std::num::NonZeroU32;
use std::time::Duration;

/// The fewest pictures a second a step lowers the rate to; below it the size comes down instead.
const SLOWEST: NonZeroU32 = NonZeroU32::new(15).unwrap();

/// The size a share is sent at: the screen's own, or scaled down so that its short side is the choice. Never above the
/// screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// The screen's own size.
    Source,
    /// Scaled down to this short side, a screen that size or smaller going as it is.
    ShortSide(u32),
}

impl Size {
    /// The stream's size for a screen of `width` by `height`: its largest even size, scaled down with its shape kept
    /// so that the short side is the choice, each side even. The short side, not the height, so a portrait or an
    /// ultrawide screen is treated as a landscape one of the same pixels.
    pub fn fit(self, width: u32, height: u32) -> (u32, u32) {
        let (width, height) = (width & !1, height & !1);
        let short = width.min(height);
        match self {
            Size::ShortSide(side) if side < short => {
                let scaled = |length: u32| {
                    u32::try_from(u64::from(length) * u64::from(side) / u64::from(short)).map_or(length, |l| l & !1)
                };
                (scaled(width).max(2), scaled(height).max(2))
            }
            _ => (width, height),
        }
    }
}

/// A size and frame rate a share is sent at, at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// The size it is sent at, never above the screen's.
    pub size: Size,
    /// The pictures a second it is sent at, at most.
    pub frames_per_second: NonZeroU32,
}

/// The steps from the person's choice down, and the one the share is on.
#[derive(Clone, Debug)]
pub struct Ladder {
    /// The person's choice.
    chosen: Step,
    /// From the step the share began on, the highest it goes back to, down to the lowest.
    steps: Vec<Step>,
    on: usize,
}

/// The steps below `chosen`, `chosen` first, its size coming down through the short sides of `sides` below it.
fn below(chosen: Step, sides: &[u32]) -> Vec<Step> {
    let mut steps = vec![chosen];
    let mut rate = chosen.frames_per_second;
    while let Some(half) = NonZeroU32::new(rate.get() / 2).filter(|half| *half >= SLOWEST) {
        rate = half;
        steps.push(Step { frames_per_second: rate, ..chosen });
    }
    if rate > SLOWEST {
        rate = SLOWEST;
        steps.push(Step { frames_per_second: rate, ..chosen });
    }
    let mut smaller: Vec<u32> = sides
        .iter()
        .copied()
        .filter(|side| match chosen.size {
            Size::Source => true,
            Size::ShortSide(chosen) => *side < chosen,
        })
        .collect();
    smaller.sort_unstable_by(|a, b| b.cmp(a));
    smaller.dedup();
    steps.extend(smaller.into_iter().map(|side| Step { size: Size::ShortSide(side), frames_per_second: rate }));
    steps
}

impl Ladder {
    /// The steps below `chosen`, starting on it; its size comes down through the short sides of `sides` (the sizes a
    /// person can choose, in any order) below its own.
    pub fn from(chosen: Step, sides: &[u32]) -> Self {
        Self { chosen, steps: below(chosen, sides), on: 0 }
    }

    /// The steps below `chosen`, through `sides` as [`Ladder::from`]'s, for a screen of `width` by `height`, starting on
    /// the first whose floor in the
    /// [`Envelope`] table fits under `ceiling`, in bits a second; the lowest step where none does, and `chosen` itself
    /// with no ceiling. The steps above the one it starts on are not gone back to: the ceiling holds them out.
    pub fn starting(chosen: Step, sides: &[u32], width: u32, height: u32, ceiling: Option<u64>) -> Self {
        let mut steps = below(chosen, sides);
        let start = ceiling.map_or(0, |ceiling| {
            let fits = |step: &Step| {
                let (width, height) = step.size.fit(width, height);
                Envelope::for_screen(width, height, step.frames_per_second.get(), None).min <= ceiling
            };
            steps.iter().position(fits).unwrap_or(steps.len() - 1)
        });
        steps.drain(..start);
        Self { chosen, steps, on: 0 }
    }

    /// The step the person chose.
    pub fn chosen(&self) -> Step {
        self.chosen
    }

    /// The step the share is on.
    pub fn now(&self) -> Step {
        self.steps[self.on]
    }

    /// Whether the share is on the highest step it goes to: the one it began on.
    pub fn at_start(&self) -> bool {
        self.on == 0
    }

    /// What one picture may take at the step's frame rate: the time between two.
    pub fn interval(&self) -> Duration {
        Duration::from_secs(1) / self.now().frames_per_second.get()
    }

    /// Goes down to the next step that changes what a screen of `width` by `height` is sent as; false at the bottom.
    pub fn down(&mut self, width: u32, height: u32) -> bool {
        let sent = |step: Step| (step.size.fit(width, height), step.frames_per_second);
        let now = sent(self.now());
        match (self.on + 1..self.steps.len()).find(|at| sent(self.steps[*at]) != now) {
            Some(at) => {
                self.on = at;
                true
            }
            None => false,
        }
    }

    /// Goes back up to the next step that changes what a screen of `width` by `height` is sent as; false at the one
    /// the share began on.
    pub fn up(&mut self, width: u32, height: u32) -> bool {
        let sent = |step: Step| (step.size.fit(width, height), step.frames_per_second);
        let now = sent(self.now());
        let Some(next) = (0..self.on).rev().find(|at| sent(self.steps[*at]) != now) else {
            return false;
        };
        // The highest step that sends the same, so the person's own choice is named once it is back.
        let same = sent(self.steps[next]);
        self.on = (0..=next).rev().take_while(|at| sent(self.steps[*at]) == same).last().unwrap_or(next);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: Size = Size::Source;
    const P1440: Size = Size::ShortSide(1440);
    const P1080: Size = Size::ShortSide(1080);
    const P720: Size = Size::ShortSide(720);
    const FOUR_K: (u32, u32) = (3840, 2160);
    /// The short sides a person can choose, as a share is given them.
    const SIDES: [u32; 3] = [720, 1080, 1440];

    fn step(size: Size, frames_per_second: u32) -> Step {
        Step { size, frames_per_second: NonZeroU32::new(frames_per_second).unwrap() }
    }

    fn walk_down(ladder: &mut Ladder, (width, height): (u32, u32)) -> Vec<Step> {
        let mut seen = vec![ladder.now()];
        while ladder.down(width, height) {
            seen.push(ladder.now());
        }
        seen
    }

    /// The least a step sends a screen at, from the table.
    fn floor(step: Step, (width, height): (u32, u32)) -> u64 {
        let (width, height) = step.size.fit(width, height);
        Envelope::for_screen(width, height, step.frames_per_second.get(), None).min
    }

    #[test]
    fn the_budget_of_a_picture_is_the_time_between_two_at_the_steps_rate() {
        let mut ladder = Ladder::from(step(SOURCE, 60), &SIDES);
        assert_eq!(ladder.interval(), Duration::from_secs(1) / 60);
        assert!(ladder.down(3840, 2160));
        assert_eq!(ladder.interval(), Duration::from_secs(1) / 30, "a lower step has longer");
    }

    #[test]
    fn a_screen_halves_its_frame_rate_to_15_before_its_size_comes_down() {
        let mut ladder = Ladder::from(step(SOURCE, 60), &SIDES);
        assert_eq!(
            walk_down(&mut ladder, FOUR_K),
            [step(SOURCE, 60), step(SOURCE, 30), step(SOURCE, 15), step(P1440, 15), step(P1080, 15), step(P720, 15)]
        );
        let mut ladder = Ladder::from(step(P1080, 24), &SIDES);
        assert_eq!(walk_down(&mut ladder, FOUR_K), [step(P1080, 24), step(P1080, 15), step(P720, 15)]);
    }

    #[test]
    fn a_step_that_would_send_the_same_is_passed_over_both_ways() {
        let mut ladder = Ladder::from(step(SOURCE, 15), &SIDES);
        assert_eq!(walk_down(&mut ladder, (1920, 1080)), [step(SOURCE, 15), step(P720, 15)]);
        assert!(ladder.up(1920, 1080));
        assert_eq!(ladder.now(), step(SOURCE, 15), "1440 of a 1080 screen is the screen itself");
        assert!(!ladder.up(1920, 1080), "the person's choice is the top");
    }

    #[test]
    fn the_bottom_and_the_top_say_so() {
        let mut ladder = Ladder::from(step(P720, 15), &SIDES);
        assert!(!ladder.down(3840, 2160));
        assert!(!ladder.up(3840, 2160));
    }

    #[test]
    fn a_size_scales_by_its_short_side_with_its_shape_kept_and_never_up() {
        assert_eq!(P1080.fit(3840, 2160), (1920, 1080));
        assert_eq!(P720.fit(1366, 768), (1280, 720), "rounded down to even");
        assert_eq!(P1080.fit(3440, 1440), (2580, 1080), "an ultrawide keeps its shape");
        assert_eq!(P1080.fit(1440, 2560), (1080, 1920), "a portrait screen by its short side");
        assert_eq!(P1440.fit(1920, 1080), (1920, 1080), "never up");
        assert_eq!(SOURCE.fit(1921, 1081), (1920, 1080), "the screen's own, even");
        assert_eq!(P1440.fit(3840, 2160), (2560, 1440));
        assert_eq!(P720.fit(1921, 1081), (1280, 720), "an odd screen from its even size");
        assert_eq!(P1440.fit(2560, 1440), (2560, 1440), "the same size is the screen's own");
    }

    #[test]
    fn no_ceiling_or_one_every_floor_fits_under_starts_on_the_choice() {
        let chosen = step(SOURCE, 60);
        assert_eq!(Ladder::starting(chosen, &SIDES, 3840, 2160, None).now(), chosen);
        assert_eq!(
            Ladder::starting(chosen, &SIDES, 3840, 2160, Some(floor(chosen, FOUR_K))).now(),
            chosen,
            "the floor itself"
        );
        assert_eq!(Ladder::starting(chosen, &SIDES, 3840, 2160, Some(u64::MAX)).now(), chosen);
    }

    #[test]
    fn a_ceiling_under_the_floor_of_a_4k_60_screen_halves_the_frame_rate_to_the_first_step_that_fits() {
        let chosen = step(SOURCE, 60);
        let ceiling = 4_000_000;
        assert!(floor(chosen, FOUR_K) > ceiling, "4K at 60 does not fit");
        assert!(floor(step(SOURCE, 30), FOUR_K) > ceiling, "nor at 30");
        assert!(floor(step(SOURCE, 15), FOUR_K) <= ceiling, "at 15 it does");
        assert_eq!(Ladder::starting(chosen, &SIDES, 3840, 2160, Some(ceiling)).now(), step(SOURCE, 15));
    }

    #[test]
    fn a_ceiling_even_15_a_second_does_not_fit_under_comes_down_a_size_and_one_nothing_fits_under_to_the_last() {
        let chosen = step(SOURCE, 60);
        let fifteen = floor(step(SOURCE, 15), FOUR_K);
        let ladder = Ladder::starting(chosen, &SIDES, 3840, 2160, Some(fifteen - 1));
        assert_eq!(ladder.now(), step(P1440, 15), "{} fits, 4K's {fifteen} does not", floor(step(P1440, 15), FOUR_K));
        let lowest = Ladder::starting(chosen, &SIDES, 3840, 2160, Some(1)).now();
        assert_eq!(lowest, step(P720, 15), "the least there is when none fits");
    }

    #[test]
    fn a_share_does_not_go_back_up_past_the_step_a_ceiling_began_it_on() {
        let mut ladder = Ladder::starting(step(SOURCE, 60), &SIDES, 3840, 2160, Some(4_000_000));
        assert!(ladder.at_start());
        assert!(!ladder.up(3840, 2160), "the ceiling holds 30 and 60 out");
        assert!(ladder.down(3840, 2160));
        assert!(!ladder.at_start());
        assert!(ladder.up(3840, 2160));
        assert_eq!(ladder.now(), step(SOURCE, 15));
        assert_eq!(ladder.chosen(), step(SOURCE, 60), "what the person chose is kept to be told");
    }
}
