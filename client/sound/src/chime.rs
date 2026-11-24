//! How each of the window's moments sounds: its own short sounds, written as data (a few sine and triangle voices
//! each), rendered whole and played by the system's sound server ([`crate::Sound::chime`]).

mod render;

use render::Voice;
use render::Wave::{Sine, Triangle};
use screens::Chime;
use std::sync::Arc;

/// The settings page's test sound: the call's own start.
pub const TEST: Chime = Chime::Joined;

/// The voices of `chime`, its times in seconds from the ask.
fn recipe(chime: Chime) -> &'static [Voice] {
    match chime {
        Chime::Joined => {
            const {
                &[
                    Voice::new(Sine, 392.0, 0.13, 0.0, 0.28, 0.28),
                    Voice::new(Sine, 494.0, 0.11, 0.04, 0.30, 0.30).held(),
                    Voice::new(Sine, 587.0, 0.09, 0.08, 0.32, 0.32).held(),
                    Voice::new(Triangle, 784.0, 0.045, 0.12, 0.34, 0.34).held(),
                ]
            }
        }
        Chime::Left => {
            const {
                &[
                    Voice::new(Sine, 587.0, 0.13, 0.0, 0.28, 0.28),
                    Voice::new(Sine, 494.0, 0.11, 0.04, 0.30, 0.30).held(),
                    Voice::new(Sine, 392.0, 0.09, 0.08, 0.34, 0.34).held(),
                    Voice::new(Triangle, 294.0, 0.045, 0.10, 0.34, 0.34).held(),
                ]
            }
        }
        Chime::MemberJoined => {
            const {
                &[
                    Voice::new(Sine, 494.0, 0.055, 0.0, 0.18, 0.18),
                    Voice::new(Sine, 659.0, 0.05, 0.07, 0.27, 0.27).held(),
                ]
            }
        }
        Chime::MemberLeft => {
            const {
                &[
                    Voice::new(Sine, 659.0, 0.055, 0.0, 0.18, 0.18),
                    Voice::new(Sine, 494.0, 0.05, 0.07, 0.27, 0.27).held(),
                ]
            }
        }
        Chime::Muted => const { &[Voice::new(Sine, 370.0, 0.055, 0.0, 0.08, 0.08).glide(330.0, 0.08)] },
        Chime::Unmuted => const { &[Voice::new(Sine, 440.0, 0.055, 0.0, 0.08, 0.08).glide(494.0, 0.08)] },
        Chime::Deafened => {
            const {
                &[
                    Voice::new(Sine, 392.0, 0.05, 0.0, 0.07, 0.07),
                    Voice::new(Sine, 330.0, 0.05, 0.06, 0.14, 0.14).held(),
                ]
            }
        }
        Chime::Undeafened => {
            const {
                &[
                    Voice::new(Sine, 494.0, 0.05, 0.0, 0.07, 0.07),
                    Voice::new(Sine, 587.0, 0.05, 0.06, 0.14, 0.14).held(),
                ]
            }
        }
        Chime::ShareStarted => {
            const {
                &[
                    Voice::new(Sine, 587.0, 0.07, 0.0, 0.12, 0.12),
                    Voice::new(Sine, 784.0, 0.07, 0.06, 0.18, 0.18).held(),
                    Voice::new(Sine, 988.0, 0.065, 0.12, 0.26, 0.26).held(),
                    Voice::new(Triangle, 1175.0, 0.025, 0.12, 0.26, 0.26).held(),
                ]
            }
        }
        Chime::MemberShareStarted => {
            const {
                &[
                    Voice::new(Sine, 784.0, 0.05, 0.0, 0.12, 0.12),
                    Voice::new(Sine, 988.0, 0.05, 0.07, 0.22, 0.22).held(),
                ]
            }
        }
        Chime::ShareStopped => {
            const {
                &[
                    Voice::new(Sine, 988.0, 0.07, 0.0, 0.22, 0.22).glide(587.0, 0.2),
                    Voice::new(Triangle, 1175.0, 0.025, 0.03, 0.20, 0.22).glide(784.0, 0.2),
                ]
            }
        }
        Chime::MemberShareStopped => const { &[Voice::new(Sine, 988.0, 0.05, 0.0, 0.18, 0.18).glide(740.0, 0.16)] },
        Chime::Disconnected => {
            const {
                &[
                    Voice::new(Sine, 784.0, 0.10, 0.0, 0.16, 0.16).held(),
                    Voice::new(Sine, 659.0, 0.10, 0.10, 0.26, 0.26).held(),
                    Voice::new(Sine, 554.0, 0.10, 0.20, 0.36, 0.36).held(),
                    Voice::new(Sine, 440.0, 0.11, 0.30, 0.50, 0.50).held().glide(392.0, 0.5),
                    Voice::new(Triangle, 659.0, 0.04, 0.32, 0.50, 0.50).held(),
                ]
            }
        }
        Chime::Message => {
            const {
                &[
                    Voice::new(Sine, 880.0, 0.05, 0.0, 0.09, 0.09),
                    Voice::new(Sine, 1319.0, 0.03, 0.02, 0.11, 0.11).held(),
                ]
            }
        }
    }
}

/// `chime` rendered whole, one channel at [`crate::speaker::RATE`].
pub fn sound(chime: Chime) -> Arc<[f32]> {
    render::render(recipe(chime)).into()
}

#[cfg(test)]
mod heard;

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY: [Chime; 14] = [
        Chime::Joined,
        Chime::Left,
        Chime::MemberJoined,
        Chime::MemberLeft,
        Chime::Muted,
        Chime::Unmuted,
        Chime::Deafened,
        Chime::Undeafened,
        Chime::ShareStarted,
        Chime::ShareStopped,
        Chime::MemberShareStarted,
        Chime::MemberShareStopped,
        Chime::Disconnected,
        Chime::Message,
    ];

    /// The pitch with the most energy over `samples`, to the nearest 10 Hz, by a DFT.
    fn loudest_pitch(samples: &[f32]) -> f32 {
        let rate = crate::speaker::RATE as f32;
        let power = |hz: f32| {
            let (re, im) = samples.iter().enumerate().fold((0.0_f32, 0.0_f32), |(re, im), (n, sample)| {
                let angle = std::f32::consts::TAU * hz * n as f32 / rate;
                (re + sample * angle.cos(), im - sample * angle.sin())
            });
            re * re + im * im
        };
        (20..=150).map(|tens| tens as f32 * 10.0).max_by(|a, b| power(*a).total_cmp(&power(*b))).unwrap_or(0.0)
    }

    #[test]
    fn every_moment_lasts_until_its_last_voice_stops_and_ends_quiet() {
        for chime in EVERY {
            let rendered = sound(chime);
            let last = recipe(chime).iter().map(|voice| voice.ends()).fold(0.0, f32::max);
            assert_eq!(rendered.len(), render::sample_at(last), "{chime:?}");
            assert!((60..=520).contains(&(rendered.len() * 1000 / crate::speaker::RATE as usize)), "{chime:?}");
            let tail = &rendered[rendered.len() - rendered.len() / 20..];
            assert!(tail.iter().all(|sample| sample.abs() < 0.05), "{chime:?} has faded by its end");
        }
    }

    #[test]
    fn every_moment_peaks_within_the_sum_of_its_voices() {
        for chime in EVERY {
            let bound: f32 = recipe(chime).iter().map(Voice::peak).sum();
            let peak = sound(chime).iter().map(|sample| sample.abs()).fold(0.0, f32::max);
            assert!(peak > 0.05 && peak <= bound, "{chime:?} peaks at {peak}, its voices at most {bound}");
        }
    }

    #[test]
    fn a_moment_sounds_at_the_pitch_of_its_loudest_voice() {
        // Each stretch holds only the voices that sound in it, the loudest first in its recipe.
        let pitch = |chime, seconds: f32| {
            let rendered = sound(chime);
            let until = render::sample_at(seconds).min(rendered.len());
            loudest_pitch(&rendered[..until])
        };
        for (chime, seconds, hz) in [
            (Chime::Undeafened, 0.05, 494.0),
            (Chime::Joined, 0.04, 392.0),
            (Chime::Left, 0.04, 587.0),
            (Chime::MemberLeft, 0.06, 659.0),
            (Chime::Disconnected, 0.09, 784.0),
            (Chime::Message, 0.015, 880.0),
        ] {
            let found = pitch(chime, seconds);
            assert!((found - hz).abs() <= 20.0, "{chime:?} sounds at {found} Hz, its recipe at {hz} Hz");
        }
    }

    #[test]
    fn every_moment_sounds_unlike_every_other() {
        for chime in EVERY {
            for other in EVERY.into_iter().filter(|other| *other != chime) {
                assert_ne!(recipe(chime), recipe(other), "{chime:?} sounds unlike {other:?}");
            }
        }
    }

    mod lab {
        use super::*;
        use crate::chime::heard::{self, Heard};
        use std::time::Duration;
        use testserver::sound::Lab;
        use tier::skipped;

        const WAIT: Duration = Duration::from_secs(10);
        /// How often each sound is played for its numbers.
        const PLAYS: usize = 5;
        /// From one ask to the next: a sound not started by then is not heard.
        const BETWEEN: Duration = Duration::from_secs(1);

        /// Plays every sound `plays` times on a virtual speaker, the speaker idle before each, and compares each play
        /// with its recipe; none when the machine has no sound server to do it on.
        async fn measure(test: &str, plays: usize) -> Result<Vec<(Chime, Vec<Option<Heard>>)>, String> {
            let lab = Lab::take().ok_or("the sound lab cannot be locked")?;
            let sound = lab.make(test).ok_or("no sound server to make a speaker on")?;
            let chimes = crate::Sound::start();
            let recording = sound.record(WAIT).await.ok_or("the speaker cannot be recorded")?;
            let mut asked = Vec::new();
            for chime in EVERY {
                for _ in 0..plays {
                    asked.push((chime, recording.taken()));
                    chimes.chime(chime, Some(sound.sink.clone())).await.expect("the sound starts");
                    tokio::time::sleep(BETWEEN).await;
                }
            }
            let recorded: Vec<f32> = recording.finish().iter().map(|sample| f32::from(*sample) / 32_768.0).collect();
            Ok(EVERY
                .into_iter()
                .map(|chime| {
                    let heard = asked
                        .iter()
                        .enumerate()
                        .filter(|(_, (asked, _))| *asked == chime)
                        .map(|(index, (_, at))| {
                            let until = asked.get(index + 1).map_or(recorded.len(), |(_, next)| *next);
                            heard::compare(&recorded, *at..until, &crate::chime::sound(chime))
                        })
                        .collect();
                    (chime, heard)
                })
                .collect())
        }

        /// Each sound's starts, earliest first.
        fn delays(heard: &[Option<Heard>]) -> Vec<f64> {
            let mut delays: Vec<f64> = heard.iter().flatten().map(|heard| heard.delay_ms).collect();
            delays.sort_by(f64::total_cmp);
            delays
        }

        /// For each moment, what breaks in its sound on a virtual speaker over [`PLAYS`] plays: every play heard,
        /// whole.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn each_sound_is_heard_whole_on_a_virtual_speaker() {
            let measured = match measure("measure", PLAYS).await {
                Ok(measured) => measured,
                Err(why) => return skipped(why),
            };
            for (chime, heard) in measured {
                let delays = delays(&heard);
                let gaps: usize = heard.iter().flatten().map(|heard| heard.gaps).sum();
                let clicks: usize = heard.iter().flatten().map(|heard| heard.clicks).sum();
                println!(
                    "{chime:?}: heard {} of {PLAYS}, delay median {:.1} ms, max {:.1} ms, gaps {gaps}, clicks {clicks}",
                    delays.len(),
                    delays.get(delays.len() / 2).copied().unwrap_or(f64::NAN),
                    delays.last().copied().unwrap_or(f64::NAN),
                );
                assert_eq!(delays.len(), PLAYS, "{chime:?} was heard every time it was asked for");
                assert_eq!((gaps, clicks), (0, 0), "{chime:?} came out whole");
            }
        }

        /// The call's sounds are the call's, not notifications: heard while the desktop mutes its notification
        /// sounds, as KDE's Do Not Disturb does while any app is full screen.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_sound_is_heard_while_the_desktop_mutes_notifications() {
            let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
            let Some(_muted) = lab.mute_notifications() else {
                return skipped("the session manager's settings cannot be changed");
            };
            let Some(sound) = lab.make("muted") else { return skipped("no sound server to make a speaker on") };
            let chimes = crate::Sound::start();
            let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
            chimes.chime(Chime::Joined, Some(sound.sink.clone())).await.expect("the sound starts");
            let heard = recording.heard(LOUD, BETWEEN).await;
            recording.stop();
            assert!(heard.is_ok(), "the sound was heard with notifications muted: loudest {heard:?}");
        }

        /// A sample louder than the null sink's silence and any dither.
        const LOUD: i16 = 300;

        mod timing {
            use super::*;

            /// How often each sound is played for its start.
            const STARTS: usize = 2;
            /// The latest a sound may start: the sound server's quantum (21.3 ms at 1024 samples) a few times over.
            const LATE_MS: f64 = 100.0;

            /// Each moment's sound starts within [`LATE_MS`] of being asked for, the speaker idle before it.
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn each_sound_starts_within_a_few_of_the_sound_server_s_quanta() {
                let measured = match measure("start", STARTS).await {
                    Ok(measured) => measured,
                    Err(why) => return skipped(why),
                };
                for (chime, heard) in measured {
                    let delays = delays(&heard);
                    println!("{chime:?}: started {delays:.1?} ms after it was asked for");
                    assert_eq!(delays.len(), STARTS, "{chime:?} was heard every time it was asked for");
                    assert!(delays.iter().all(|delay| *delay <= LATE_MS), "{chime:?} started within {LATE_MS} ms");
                }
            }
        }

        /// A sound asked for while one plays is cut and the newer plays, so two never sound at once: a long sound
        /// replaced by a short one falls silent long before the long one would have.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_sound_asked_while_one_plays_replaces_it() {
            let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
            let Some(sound) = lab.make("replace") else { return skipped("no sound server to make a speaker on") };
            let chimes = crate::Sound::start();
            let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
            let first = recording.taken();
            chimes.chime(Chime::Disconnected, Some(sound.sink.clone())).await.expect("the sound starts");
            tokio::time::sleep(Duration::from_millis(100)).await;
            chimes.chime(Chime::Muted, Some(sound.sink.clone())).await.expect("the newer starts");
            tokio::time::sleep(BETWEEN).await;
            let recorded: Vec<f32> = recording.finish().iter().map(|sample| f32::from(*sample) / 32_768.0).collect();
            let loud = |sample: &f32| sample.abs() > 0.01;
            let heard = &recorded[first..];
            let span = heard.iter().position(loud).zip(heard.iter().rposition(loud));
            let lasted_ms = span.map_or(0, |(on, off)| (off - on) * 1000 / heard::RATE);
            println!("from the first sound's start to the last loud sample: {lasted_ms} ms");
            // Played out, the first stays loud past 350 ms; cut at 100 ms, it and the newer end well before.
            assert!((50..280).contains(&lasted_ms), "the first was cut and the newer played: {lasted_ms} ms");
        }
    }
}
