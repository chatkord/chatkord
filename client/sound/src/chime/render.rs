//! A sound made of voices, rendered whole as the Web Audio API renders the same graph: an oscillator through a gain
//! straight to the output, each gain falling on an exponential ramp from the last value set.

use crate::speaker::RATE;

/// The level every gain is set at times its value.
const VOLUME: f32 = 2.0;
/// Where every gain's ramp ends: Web Audio cannot ramp exponentially to zero.
const FLOOR: f32 = 0.0001;

/// An oscillator's shape, as Web Audio's `OscillatorType` names it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wave {
    Sine,
    Triangle,
}

/// One oscillator and its gain. Times are seconds from the moment the sound is asked for, when every value is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Voice {
    wave: Wave,
    hz: f32,
    /// A pitch reached exponentially, from `hz` at the start, by the time given.
    glide: Option<(f32, f32)>,
    /// The gain's value, before [`VOLUME`].
    gain: f32,
    /// When the gain is set to its value again, so its ramp starts there; otherwise it starts at 0.
    held: f32,
    /// When the ramp reaches [`FLOOR`].
    fade: f32,
    start: f32,
    stop: f32,
}

impl Voice {
    /// A voice sounding from `start` to `stop`, its gain ramping down from the ask to `fade`.
    pub(crate) const fn new(wave: Wave, hz: f32, gain: f32, start: f32, fade: f32, stop: f32) -> Self {
        Self { wave, hz, glide: None, gain, held: 0.0, fade, start, stop }
    }

    /// The same voice, its pitch gliding to `hz` by `by`.
    pub(crate) const fn glide(self, hz: f32, by: f32) -> Self {
        Self { glide: Some((hz, by)), ..self }
    }

    /// The same voice, its gain set again at its start, so its ramp starts there.
    pub(crate) const fn held(self) -> Self {
        Self { held: self.start, ..self }
    }

    /// The loudest the voice can be: its gain as it is first set.
    #[cfg(test)]
    pub(crate) fn peak(&self) -> f32 {
        self.gain * VOLUME
    }

    /// When the voice has ended, in seconds.
    pub(crate) fn ends(&self) -> f32 {
        self.stop
    }

    fn level(&self, t: f32) -> f32 {
        let set = self.gain * VOLUME;
        if t <= self.held {
            return set;
        }
        if t >= self.fade {
            return FLOOR;
        }
        set * (FLOOR / set).powf((t - self.held) / (self.fade - self.held))
    }

    fn pitch(&self, t: f32) -> f32 {
        match self.glide {
            Some((to, by)) if t < by => self.hz * (to / self.hz).powf(t / by),
            Some((to, _)) => to,
            None => self.hz,
        }
    }

    /// The shape's value at `phase` turns from its start.
    fn shape(&self, phase: f64) -> f32 {
        #[expect(clippy::cast_possible_truncation, reason = "a turn's fraction, from 0 to 1")]
        let turn = phase.fract() as f32;
        match self.wave {
            Wave::Sine => (std::f32::consts::TAU * turn).sin(),
            // Rising from zero, as Web Audio's triangle does: up to 1 at a quarter, down to -1 at three quarters.
            Wave::Triangle if turn < 0.25 => 4.0 * turn,
            Wave::Triangle if turn < 0.75 => 2.0 - 4.0 * turn,
            Wave::Triangle => 4.0 * turn - 4.0,
        }
    }
}

/// The sample a time in a recipe falls on.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a recipe's times are under a second and never negative"
)]
pub(crate) fn sample_at(seconds: f32) -> usize {
    (seconds * RATE as f32).round() as usize
}

/// `voices` summed, one channel at [`RATE`] from the ask to the last voice's stop.
pub(crate) fn render(voices: &[Voice]) -> Vec<f32> {
    let samples = sample_at(voices.iter().map(Voice::ends).fold(0.0, f32::max));
    let mut out = vec![0.0; samples];
    for voice in voices {
        let (start, stop) = (sample_at(voice.start), sample_at(voice.stop).min(samples));
        let mut phase = 0.0_f64;
        for (n, sample) in out.iter_mut().enumerate().take(stop).skip(start) {
            #[expect(clippy::cast_precision_loss, reason = "a sound is under a second of samples")]
            let t = n as f32 / RATE as f32;
            *sample += voice.level(t) * voice.shape(phase);
            phase += f64::from(voice.pitch(t)) / f64::from(RATE);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gain_ramps_from_its_value_to_the_floor_and_holds_it() {
        let voice = Voice::new(Wave::Sine, 440.0, 0.05, 0.0, 0.1, 0.2);
        assert!((voice.level(0.0) - 0.1).abs() < 1e-6, "set at twice its value");
        assert!((voice.level(0.05) - (0.1_f32 * FLOOR).sqrt()).abs() < 1e-6, "halfway is the geometric mean");
        assert_eq!(voice.level(0.15), FLOOR);
    }

    #[test]
    fn a_voice_started_late_has_ramped_since_the_ask_unless_its_gain_is_held_there() {
        let late = Voice::new(Wave::Sine, 440.0, 0.05, 0.1, 0.2, 0.2);
        assert!(late.level(0.1) < 0.1 / 10.0, "half its ramp gone before it sounds: {}", late.level(0.1));
        assert!((late.held().level(0.1) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_glide_moves_exponentially_and_stays() {
        let voice = Voice::new(Wave::Sine, 900.0, 0.08, 0.0, 0.2, 0.2).glide(550.0, 0.18);
        assert!((voice.pitch(0.09) - (900.0_f32 * 550.0).sqrt()).abs() < 0.1);
        assert_eq!(voice.pitch(0.19), 550.0);
    }

    #[test]
    fn a_triangle_rises_from_zero_to_its_peak_at_a_quarter() {
        let voice = Voice::new(Wave::Triangle, 1.0, 0.0, 0.0, 1.0, 1.0);
        let at = |phase| voice.shape(phase);
        assert_eq!((at(0.0), at(0.25), at(0.5), at(0.75)), (0.0, 1.0, 0.0, -1.0));
        assert!((at(0.125) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_render_lasts_until_the_last_stop_and_is_silent_outside_each_voice() {
        let voices = [Voice::new(Wave::Sine, 600.0, 0.05, 0.1, 0.15, 0.15)];
        let rendered = render(&voices);
        assert_eq!(rendered.len(), RATE as usize * 15 / 100);
        assert!(rendered[..RATE as usize / 10].iter().all(|sample| *sample == 0.0));
    }
}
