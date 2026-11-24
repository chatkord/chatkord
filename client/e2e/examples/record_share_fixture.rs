#![allow(clippy::expect_used, clippy::print_stdout, reason = "a recording run by hand: a failed step ends it")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "the screen tests' painting, whose numbers are its own fixtures"
)]
//! Records the shared screen `e2e_encoded`'s lossy path feeds: 60 pictures of a bar crossing four colours, each
//! different, one key frame first, a line each: `key` or `delta`, a space and the unit in hex, so the tracked file is
//! text. Run by hand on a machine with a hardware H.264 encoder:
//! `cargo run -p e2e --example record_share_fixture -- e2e/tests/fixtures/share-h264-320x180.txt`.
#![cfg(any(target_os = "linux", windows))]

#[path = "../tests/suite/clock.rs"]
mod clock;
#[path = "../tests/suite/screen_support/mod.rs"]
mod screen_support;

use capture::Pixels;
use media::{Codec, Session};
use screen_support::{QUADRANTS, candidates_of, layout, settings};

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const PICTURES: usize = 60;

/// The four colours with a white bar `at` pixels from the left.
fn painting(at: usize) -> Vec<[u8; 3]> {
    (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).map(move |x| {
                let quadrant = QUADRANTS[usize::from(x >= WIDTH / 2) + 2 * usize::from(y >= HEIGHT / 2)];
                if x.abs_diff(at) < 4 { [255, 255, 255] } else { quadrant }
            })
        })
        .collect()
}

/// One unit as a line of the file.
fn line(unit: &media::Encoded) -> String {
    let hex: String = unit.data.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{} {hex}\n", if unit.key_frame { "key" } else { "delta" })
}

fn main() {
    let path = std::env::args().nth(1).expect("the file to write");
    let (encoder, canvas) = candidates_of(Codec::H264).into_iter().next().expect("a hardware H.264 encoder");
    let mut session =
        Session::open_screen(&encoder, settings(WIDTH, HEIGHT), layout(WIDTH, HEIGHT)).expect("the encoder opens");
    let mut surface = canvas.surface(&session, WIDTH, HEIGHT).expect("a surface");
    let mut out = String::new();
    let mut units = 0;
    for n in 0..PICTURES {
        surface.paint_rgb(&painting(n * WIDTH / PICTURES));
        for unit in session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encoded") {
            out.push_str(&line(&unit));
            units += 1;
        }
        // The one buffer is drawn into again only once the convert that reads it is done.
        session.settle().expect("the convert finished");
    }
    for unit in session.finish().expect("the encoder ends") {
        out.push_str(&line(&unit));
        units += 1;
    }
    std::fs::write(&path, &out).expect("written");
    println!("{units} units, {} bytes, by {} into {path}", out.len(), encoder.name);
}
