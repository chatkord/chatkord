//! How a person's choices for their microphone (`choices::VoiceProcessing`) become the chain's steps.

use choices::{AutomaticGain, EchoCancellation, Suppression, VoiceProcessing};
use webrtc::{Gain, Noise, Processing};

/// The chain's steps for what the person chose.
pub(crate) fn of(asked: &VoiceProcessing) -> Processing {
    Processing {
        echo_cancellation: asked.echo == EchoCancellation::On,
        noise: match asked.suppression {
            Suppression::Off => Noise::Off,
            Suppression::Standard => Noise::Standard,
            Suppression::Rnnoise => Noise::Rnnoise,
        },
        gain: match asked.automatic_gain {
            AutomaticGain::On => Gain::Automatic,
            AutomaticGain::Off => Gain::Fixed(f32::from(asked.volume.db())),
        },
        gate: asked.gating().map(|threshold| f32::from(threshold.db())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use choices::{InputGain, NoiseGate, Threshold};

    #[test]
    fn the_volume_applies_only_with_automatic_gain_off_and_the_threshold_only_with_the_gate_on() {
        let kept = VoiceProcessing {
            volume: InputGain::new(6).unwrap(),
            threshold: Threshold::new(-40).unwrap(),
            ..VoiceProcessing::default()
        };
        let fresh = of(&kept);
        assert_eq!((fresh.gain, fresh.gate, fresh.noise), (Gain::Automatic, None, Noise::Rnnoise));
        let own = VoiceProcessing { automatic_gain: AutomaticGain::Off, gate: NoiseGate::On, ..kept };
        assert_eq!((of(&own).gain, of(&own).gate), (Gain::Fixed(6.0), Some(-40.0)));
    }
}
