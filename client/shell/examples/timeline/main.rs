#![expect(
    clippy::cast_possible_truncation,
    reason = "an example's numbers are its own fixtures, bounded where it writes them"
)]
//! `timeline -- [COUNT] [--scroll] [--select]`: COUNT generated messages (10,000) in a virtual list; `--scroll` says
//! once a second what a frame of scrolling cost, `--select` drags a selection and says when each screenshot is due.
#![expect(clippy::print_stdout, reason = "an example's output is what it prints")]

mod chat;
mod heights;
mod rows;
mod timeline;

use std::time::Instant as Clock;

use design::Scheme;
use iced_wgpu::Renderer;
use iced_winit::core::{Element, Event, Point, Size, Theme, mouse};
use shell::{Input, Next, Redrawn, Scene};

struct Spike {
    timeline: timeline::Timeline,
    theme: Theme,
    /// Scroll by itself, a wheel turn a frame, and say what each second cost.
    scrolling: bool,
    /// Drag a selection, say so, scroll, say so, so a screenshot can be taken at each; counts frames.
    selecting: Option<u32>,
    frame_nanos: u64,
    counted: (Clock, u32),
    run_started: Clock,
    last_wheel: Clock,
}

impl Spike {
    fn report(&mut self) {
        self.counted.1 += 1;
        let seconds = self.counted.0.elapsed().as_secs_f64();
        if seconds < 1.0 {
            return;
        }
        let built = self.timeline.built();
        let frames = f64::from(self.counted.1);
        println!(
            "frames {:.0}/s, {:.2} ms a frame on the CPU, built {} of {} messages, {:.0} of {:.0} px from the newest",
            frames / seconds,
            self.frame_nanos as f64 / 1e6 / frames,
            built.built,
            built.messages,
            built.from_end,
            built.total
        );
        self.frame_nanos = 0;
        self.counted = (Clock::now(), 0);
    }

    /// The scripted selection: by frame, where the pointer goes, what it presses and when a screenshot is due.
    fn select(frame: u32, input: &mut Input<'_>) -> Next {
        let wheel =
            |y: f32| Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y } });
        match frame {
            20 => input.move_to(Point::new(100.0, 160.0)),
            22 => input.push(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))),
            24 => input.move_to(Point::new(400.0, 160.0)),
            26 => input.move_to(Point::new(700.0, 190.0)),
            28 => input.push(Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))),
            60 => println!("SHOT 1 selected"),
            200 => input.push(wheel(0.2)),
            260 => println!("SHOT 2 scrolled by a fifth of a line"),
            400 => input.push(wheel(3.0)),
            460 => println!("SHOT 3 scrolled by three more lines"),
            600 => input.push(wheel(30.0)),
            660 => println!("SHOT 4 scrolled by thirty more lines"),
            800 => return Next::Exit,
            _ => {}
        }
        Next::Again
    }
}

impl Scene for Spike {
    type Message = timeline::Event;
    type Theme = Theme;

    fn title(&self) -> String {
        "timeline".into()
    }

    fn prepare(&mut self, scheme: Scheme, _motion: design::MotionPreference) {
        self.theme = match scheme {
            Scheme::Dark => Theme::Dark,
            Scheme::Light => Theme::Light,
        };
    }

    fn theme(&self) -> &Theme {
        &self.theme
    }

    fn view(&self, _: Size) -> Element<'_, timeline::Event, Theme, Renderer> {
        self.timeline.view()
    }

    fn update(&mut self, messages: Vec<timeline::Event>) -> bool {
        let mut changed = false;
        for event in messages {
            changed |= self.timeline.update(event);
        }
        changed
    }

    fn opened(&mut self, input: &mut Input<'_>) {
        input.move_to(Point::new(600.0, 400.0));
    }

    fn redrawn(&mut self, redrawn: Redrawn, input: &mut Input<'_>) -> Next {
        self.frame_nanos += redrawn.cpu.as_nanos() as u64;
        if let Some(frame) = self.selecting {
            self.selecting = Some(frame + 1);
            if let Next::Exit = Self::select(frame, input) {
                return Next::Exit;
            }
        }
        if self.scrolling {
            if self.timeline.at_oldest() {
                println!(
                    "reached the oldest message after {:.1} s and {} frames",
                    self.run_started.elapsed().as_secs_f64(),
                    redrawn.frames
                );
                return Next::Exit;
            }
            // Three lines per sixtieth of a second whatever the panel's rate, so 120 Hz does not scroll twice as fast.
            let lines = 3.0 * self.last_wheel.elapsed().as_secs_f32() * 60.0;
            self.last_wheel = Clock::now();
            input.push(Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: lines },
            }));
        }
        self.report();
        if self.scrolling || self.selecting.is_some() { Next::Again } else { Next::Wait }
    }
}

fn main() -> Result<(), shell::Failure> {
    // The window's own lines go to stdout beside this example's, where a measurement reads them.
    tracing_subscriber::fmt().with_writer(std::io::stdout).with_max_level(tracing::Level::INFO).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count = args.iter().find_map(|arg| arg.parse().ok()).unwrap_or(10_000);
    let flag = |name: &str| args.iter().any(|arg| arg == name);
    let (scrolling, selecting) = (flag("--scroll"), flag("--select").then_some(0));
    shell::run(shell::event_loop()?, platform::Platform::default(), |_| {
        Ok::<_, std::convert::Infallible>(Spike {
            timeline: timeline::Timeline::new(count, 1),
            theme: Theme::Dark,
            scrolling,
            selecting,
            frame_nanos: 0,
            counted: (Clock::now(), 0),
            run_started: Clock::now(),
            last_wheel: Clock::now(),
        })
    })
}
