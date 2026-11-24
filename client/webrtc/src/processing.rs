//! What a call's microphone goes through before it is sent, in this order: a fixed gain when automatic gain is off,
//! the WebRTC audio processor (echo cancellation, the high-pass filter, its noise suppression, automatic gain), RNNoise
//! in place of the processor's suppressor when chosen, then the noise gate. Values retune in place; only a step of the
//! processor turned on or off builds it again, which makes its echo canceller learn the room again.

use libwebrtc::native::apm::AudioProcessingModule;
use nnnoiseless::DenoiseState;

/// The rate the steps run at.
const RATE_HZ: i32 = 48_000;
/// The samples of one 10 ms frame.
const FRAME: usize = 480;
/// A frame of the microphone.
pub(crate) type Mono = [i16; FRAME];

/// How the microphone is processed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Processing {
    /// Whether what the speaker plays is cancelled from the microphone.
    pub echo_cancellation: bool,
    /// Which noise suppressor runs.
    pub noise: Noise,
    /// How loud the microphone is made.
    pub gain: Gain,
    /// The noise gate's threshold in dB below full scale, as a frame's root mean square; none: no gate.
    pub gate: Option<f32>,
}

#[cfg(test)]
impl Processing {
    /// Every step of the processor, as the engine's own module ran them on a call's microphone.
    pub(crate) const PROCESSOR: Self =
        Self { echo_cancellation: true, noise: Noise::Standard, gain: Gain::Automatic, gate: None };
}

/// Which noise suppressor runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Noise {
    /// None.
    Off,
    /// The audio processor's own.
    Standard,
    /// RNNoise, a recurrent network: more of what is not a voice taken out, typing and fans among it.
    Rnnoise,
}

/// How loud the microphone is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gain {
    /// The audio processor's adaptive gain.
    Automatic,
    /// A fixed gain in dB, applied before the processor.
    Fixed(f32),
}

/// Which steps the audio processor was built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Built {
    echo_cancellation: bool,
    gain_control: bool,
    noise_suppression: bool,
}

impl Built {
    fn of(processing: &Processing) -> Self {
        Self {
            echo_cancellation: processing.echo_cancellation,
            gain_control: processing.gain == Gain::Automatic,
            noise_suppression: processing.noise == Noise::Standard,
        }
    }

    fn processor(self) -> AudioProcessingModule {
        AudioProcessingModule::new(self.echo_cancellation, self.gain_control, true, self.noise_suppression)
    }
}

/// What one frame was after the steps before the gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Heard {
    /// The loudest sample.
    pub(crate) peak: u16,
    /// Its root mean square, in dB below full scale.
    pub(crate) loudness: f32,
}

/// The steps of one chain, on its own thread.
pub(crate) struct Steps {
    built: Built,
    processor: AudioProcessingModule,
    rnnoise: Option<Box<DenoiseState<'static>>>,
    /// The fixed gain as a factor; one with automatic gain.
    factor: f32,
    gate: Option<Gate>,
}

impl Steps {
    pub(crate) fn new(processing: &Processing) -> Self {
        let built = Built::of(processing);
        let mut steps = Self { built, processor: built.processor(), rnnoise: None, factor: 1.0, gate: None };
        steps.set(processing);
        steps
    }

    /// Takes `processing` from the next frame: a step of the processor turned on or off builds it again.
    pub(crate) fn set(&mut self, processing: &Processing) {
        let built = Built::of(processing);
        if built != self.built {
            (self.built, self.processor) = (built, built.processor());
        }
        match (processing.noise, self.rnnoise.is_some()) {
            (Noise::Rnnoise, false) => self.rnnoise = Some(DenoiseState::new()),
            (Noise::Off | Noise::Standard, true) => self.rnnoise = None,
            _ => {}
        }
        self.factor = match processing.gain {
            Gain::Automatic => 1.0,
            Gain::Fixed(db) => factor(db),
        };
        self.gate = processing.gate.map(|threshold| {
            let mut gate = self.gate.take().unwrap_or_default();
            gate.threshold = threshold;
            gate
        });
    }

    /// Processes one microphone frame in place, the speaker's sound `delay_ms` behind it; what it was before the
    /// gate.
    pub(crate) fn capture(&mut self, frame: &mut Mono, delay_ms: i32) -> Heard {
        if self.factor != 1.0 {
            for sample in frame.iter_mut() {
                *sample = saturated(f32::from(*sample) * self.factor);
            }
        }
        let _ = self.processor.set_stream_delay_ms(delay_ms);
        let _ = self.processor.process_stream(frame, RATE_HZ, 1);
        if let Some(rnnoise) = &mut self.rnnoise {
            let input: [f32; FRAME] = frame.map(f32::from);
            let mut output = [0.0; FRAME];
            rnnoise.process_frame(&mut output, &input);
            for (sample, out) in frame.iter_mut().zip(output) {
                *sample = saturated(out);
            }
        }
        let heard = heard(frame);
        if let Some(gate) = &mut self.gate {
            gate.apply(frame, heard.loudness);
        }
        heard
    }

    /// Hands the processor what the speaker plays, interleaved stereo, so its echo canceller knows the echo.
    pub(crate) fn reverse(&mut self, frame: &mut [i16]) {
        let _ = self.processor.process_reverse_stream(frame, RATE_HZ, 2);
    }
}

/// `db` as a factor of amplitude.
fn factor(db: f32) -> f32 {
    10_f32.powf(db / 20.0)
}

#[expect(clippy::cast_possible_truncation, reason = "clamped to a sample's range first")]
fn saturated(sample: f32) -> i16 {
    sample.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

/// The loudest sample of `frame` and its root mean square in dB below full scale; silence is -100 dB.
fn heard(frame: &Mono) -> Heard {
    let peak = frame.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0);
    #[expect(clippy::cast_precision_loss, reason = "480 samples")]
    let mean = frame.iter().map(|sample| f64::from(*sample).powi(2)).sum::<f64>() / FRAME as f64;
    #[expect(clippy::cast_possible_truncation, reason = "a level in dB")]
    let loudness = (10.0 * (mean / f64::from(i16::MAX).powi(2)).log10()).max(-100.0) as f32;
    Heard { peak, loudness }
}

/// The noise gate: open while the frames' loudness is at or over the threshold, closed once it has stayed under the
/// threshold less the hysteresis for the hold, ramping linearly so it closes to exact zeros, which the encoder can
/// leave unsent. It starts closed.
#[derive(Debug)]
struct Gate {
    threshold: f32,
    /// The detector: the loudness, followed faster as it rises than as it falls, so a word's start is not cut.
    envelope: f32,
    open: bool,
    /// The samples it stays open once under the threshold less the hysteresis.
    held: usize,
    /// The gain applied now, from 0 to 1.
    gain: f32,
}

impl Default for Gate {
    fn default() -> Self {
        Self { threshold: 0.0, envelope: -100.0, open: false, held: 0, gain: 0.0 }
    }
}

impl Gate {
    const HYSTERESIS_DB: f32 = 3.0;
    /// 20 ms to follow a rise, 10 ms a fall: a faster rise reads the threshold several dB hot.
    const RISE: f32 = 0.393;
    const FALL: f32 = 0.632;
    /// 100 ms.
    const HOLD: usize = 4_800;
    /// 5 ms to open, 150 ms to close, in samples.
    const ATTACK: f32 = 240.0;
    const RELEASE: f32 = 7_200.0;

    fn apply(&mut self, frame: &mut Mono, loudness: f32) {
        let follow = if loudness > self.envelope { Self::RISE } else { Self::FALL };
        self.envelope += follow * (loudness - self.envelope);
        if self.envelope >= self.threshold {
            self.open = true;
        }
        if self.open {
            if self.envelope >= self.threshold - Self::HYSTERESIS_DB {
                self.held = Self::HOLD;
            } else if self.held > 0 {
                self.held = self.held.saturating_sub(FRAME);
            } else {
                self.open = false;
            }
        }
        let (target, step) = if self.open { (1.0, 1.0 / Self::ATTACK) } else { (0.0, 1.0 / Self::RELEASE) };
        for sample in frame.iter_mut() {
            self.gain =
                if self.gain < target { (self.gain + step).min(target) } else { (self.gain - step).max(target) };
            *sample = saturated(f32::from(*sample) * self.gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of a 440 Hz tone whose root mean square is `db` below full scale, `n` frames in.
    fn tone(db: f32, n: usize) -> Mono {
        let amplitude = factor(db) * f32::from(i16::MAX) * std::f32::consts::SQRT_2;
        std::array::from_fn(|i| {
            #[expect(clippy::cast_precision_loss, reason = "a sample's index")]
            let t = (n * FRAME + i) as f32 / 48_000.0;
            saturated(amplitude * (2.0 * std::f32::consts::PI * 440.0 * t).sin())
        })
    }

    fn gated(threshold: f32) -> Gate {
        Gate { threshold, ..Gate::default() }
    }

    fn passed(gate: &mut Gate, db: f32, frames: usize, from: usize) -> Vec<Mono> {
        (from..from + frames)
            .map(|n| {
                let mut frame = tone(db, n);
                let loudness = heard(&frame).loudness;
                gate.apply(&mut frame, loudness);
                frame
            })
            .collect()
    }

    #[test]
    fn a_frame_s_loudness_is_its_root_mean_square_below_full_scale() {
        assert!((heard(&tone(-20.0, 0)).loudness + 20.0).abs() < 0.2);
        assert_eq!(heard(&[0; FRAME]).loudness, -100.0, "silence has a floor");
    }

    #[test]
    fn a_voice_over_the_threshold_opens_the_gate_and_passes_whole() {
        let mut gate = gated(-40.0);
        let frames = passed(&mut gate, -20.0, 20, 0);
        let last = frames.last().unwrap();
        assert_eq!(last, &tone(-20.0, 19), "open, a frame passes untouched");
    }

    #[test]
    fn under_the_threshold_it_closes_to_exact_zeros_after_its_hold_and_release() {
        let mut gate = gated(-40.0);
        passed(&mut gate, -20.0, 20, 0);
        let quiet = passed(&mut gate, -60.0, 40, 20);
        assert!(quiet[..10].iter().any(|frame| frame.iter().any(|sample| *sample != 0)), "held at first");
        assert!(quiet[30..].iter().all(|frame| frame.iter().all(|sample| *sample == 0)), "then exact zeros");
    }

    #[test]
    fn a_level_about_the_threshold_does_not_chatter() {
        let mut gate = gated(-40.0);
        passed(&mut gate, -20.0, 20, 0);
        let mut flips = 0;
        let mut was = gate.open;
        for n in 0..200 {
            let db = if n % 2 == 0 { -41.0 } else { -39.0 };
            passed(&mut gate, db, 1, 20 + n);
            flips += usize::from(gate.open != was);
            was = gate.open;
        }
        assert_eq!(flips, 0, "within the hysteresis it stays as it was");
    }

    #[test]
    fn a_quiet_room_never_opens_it() {
        let mut gate = gated(-40.0);
        let frames = passed(&mut gate, -50.0, 100, 0);
        assert!(frames.iter().all(|frame| frame.iter().all(|sample| *sample == 0)));
    }

    #[test]
    fn a_fixed_gain_scales_before_the_processor_and_automatic_gain_leaves_it() {
        assert!((factor(6.0) - 1.995).abs() < 0.01, "+6 dB doubles");
        let fixed = Processing { gain: Gain::Fixed(6.0), noise: Noise::Off, ..Processing::PROCESSOR };
        assert!((Steps::new(&fixed).factor - factor(6.0)).abs() < f32::EPSILON);
        assert!((Steps::new(&Processing::PROCESSOR).factor - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn retuning_keeps_the_processor_and_a_step_turned_off_builds_it_again() {
        let standard = Processing { noise: Noise::Standard, ..Processing::PROCESSOR };
        let mut steps = Steps::new(&standard);
        steps.set(&Processing { gate: Some(-48.0), ..standard });
        assert_eq!(steps.built, Built::of(&standard), "a gate is not the processor's");
        steps.set(&Processing { noise: Noise::Rnnoise, ..standard });
        assert!(steps.rnnoise.is_some() && !steps.built.noise_suppression, "RNNoise in place of the processor's");
        steps.set(&standard);
        assert!(steps.rnnoise.is_none() && steps.built.noise_suppression);
    }

    /// White noise whose root mean square is `db` below full scale, the same every run.
    struct White(u32, f32);

    impl White {
        fn new(db: f32) -> Self {
            Self(0x1234_5678, factor(db) * f32::from(i16::MAX) * 3_f32.sqrt())
        }

        fn frame(&mut self) -> Mono {
            std::array::from_fn(|_| {
                self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                #[expect(clippy::cast_precision_loss, reason = "a uniform draw")]
                let uniform = (self.0 >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
                saturated(uniform * self.1)
            })
        }
    }

    /// A voiced vowel, as speech is to a voice detector: harmonics of 120 Hz shaped by two formants (700 and 1 200 Hz),
    /// in syllables four times a second, its root mean square while it sounds `db` below full scale.
    fn vowel(db: f32, n: usize) -> Mono {
        let formant = |f: f32, centre: f32| 1.0 / (1.0 + ((f - centre) / 150.0).powi(2));
        let weights: Vec<(f32, f32)> =
            (1..40).map(|k| k as f32 * 120.0).map(|f| (f, formant(f, 700.0) + 0.5 * formant(f, 1_200.0))).collect();
        let norm = (weights.iter().map(|(_, w)| w * w / 2.0).sum::<f32>()).sqrt();
        let amplitude = factor(db) * f32::from(i16::MAX) / norm;
        std::array::from_fn(|i| {
            #[expect(clippy::cast_precision_loss, reason = "a sample's index")]
            let t = (n * FRAME + i) as f32 / 48_000.0;
            let syllable = (2.0 * std::f32::consts::PI * 4.0 * t).sin().max(0.0).sqrt();
            let voiced: f32 = weights.iter().map(|(f, w)| w * (2.0 * std::f32::consts::PI * f * t).sin()).sum();
            saturated(amplitude * syllable * voiced)
        })
    }

    /// What `processing` sends of five seconds of `signal`: the mean loudness of the last second in dB below full
    /// scale, and the time one frame took on average.
    fn sent(processing: &Processing, mut signal: impl FnMut(usize) -> Mono) -> (f32, std::time::Duration) {
        let mut steps = Steps::new(processing);
        let started = std::time::Instant::now();
        let mut last = Vec::new();
        for n in 0..500 {
            let mut frame = signal(n);
            steps.capture(&mut frame, 0);
            if n >= 400 {
                last.push(heard(&frame).loudness);
            }
        }
        #[expect(clippy::cast_precision_loss, reason = "100 frames")]
        let mean = last.iter().sum::<f32>() / last.len() as f32;
        (mean, started.elapsed() / 500)
    }

    #[test]
    fn each_suppressor_and_the_gain_measured_on_steady_noise_and_a_quiet_voice() {
        let mut table = Vec::new();
        let mut floor = |noise_kind, gain| {
            let processing = Processing { echo_cancellation: false, noise: noise_kind, gain, gate: None };
            let mut noise = White::new(-40.0);
            let (db, took) = sent(&processing, |_| noise.frame());
            table.push(format!("{noise_kind:?} {gain:?}: {db:.1} dB in {took:?} a frame"));
            db
        };
        let off = floor(Noise::Off, Gain::Fixed(0.0));
        let standard = floor(Noise::Standard, Gain::Fixed(0.0));
        let strong = floor(Noise::Rnnoise, Gain::Fixed(0.0));
        let standard_agc = floor(Noise::Standard, Gain::Automatic);
        let strong_agc = floor(Noise::Rnnoise, Gain::Automatic);
        let quiet = |gain| {
            let processing = Processing { echo_cancellation: false, noise: Noise::Off, gain, gate: None };
            sent(&processing, |n| vowel(-45.0, n)).0
        };
        let (lifted, kept) = (quiet(Gain::Automatic), quiet(Gain::Fixed(0.0)));
        println!("{}", table.join("\n"));
        println!("a -45 dB voice: {lifted:.1} dB with automatic gain, {kept:.1} dB without");
        println!(
            "floors against no suppression, gain fixed: standard {:.1} dB, RNNoise {:.1} dB; with automatic gain {:.1}, {:.1}",
            standard - off,
            strong - off,
            standard_agc - off,
            strong_agc - off
        );
        assert!(standard < off - 6.0, "the processor's suppressor takes 6 dB or more of steady noise");
        assert!(strong < standard - 6.0, "RNNoise 6 dB more");
        assert!(lifted > kept + 6.0, "automatic gain lifts a quiet voice by 6 dB or more");
    }

    #[test]
    fn a_closed_gate_is_exact_silence_and_a_voice_opens_it_on_its_second_frame() {
        let gated =
            Processing { echo_cancellation: false, noise: Noise::Off, gain: Gain::Fixed(0.0), gate: Some(-40.0) };
        let mut steps = Steps::new(&gated);
        let silent = |frame: &Mono| frame.iter().all(|sample| *sample == 0);
        let quiet: Vec<bool> = (0..30)
            .map(|n| {
                let mut frame = tone(-60.0, n);
                steps.capture(&mut frame, 0);
                silent(&frame)
            })
            .collect();
        assert!(
            quiet.iter().all(|silent| *silent),
            "the closed gate gives exact silence, which Opus DTX sends cheaply"
        );
        let loud: Vec<bool> = (30..40)
            .map(|n| {
                let mut frame = tone(-20.0, n);
                steps.capture(&mut frame, 0);
                silent(&frame)
            })
            .collect();
        // The detector follows a rise over 20 ms, so the gate opens on a voice's second frame.
        assert!(loud[1..].iter().all(|silent| !silent), "a voice passes from the frame the gate opens: {loud:?}");
    }
}
