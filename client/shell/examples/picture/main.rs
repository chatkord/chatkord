#![cfg_attr(
    target_os = "linux",
    expect(
        clippy::cast_possible_truncation,
        reason = "an example's numbers are its own fixtures, bounded where it writes them"
    )
)]
//! `picture -- one | record FILE | play FILE`: one decoded gradient for `SPIKE_FRAMES` redraws, a few seconds of
//! moving 4K texture encoded into FILE, or FILE decoded into the window at 60 pictures a second, over and over.
#![expect(clippy::print_stdout, reason = "an example's output is what it prints")]

#[cfg(target_os = "linux")]
mod stream;

#[cfg(target_os = "linux")]
use picture::Slot;
#[cfg(target_os = "linux")]
use shell::watched::{Feed, Said, Watched};
#[cfg(target_os = "linux")]
use std::sync::Arc;

/// Redraws of `one` before it closes, unless `SPIKE_FRAMES` says otherwise.
#[cfg(target_os = "linux")]
const FRAMES: u32 = 20;

/// Why a picture source that decodes straight into the window cannot run here.
#[cfg(target_os = "linux")]
fn no_decoder() -> String {
    "this GPU does not decode H.264 into the window".to_string()
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), shell::Failure> {
    // The window's own lines go to stdout beside this example's, where a measurement reads them.
    tracing_subscriber::fmt().with_writer(std::io::stdout).with_max_level(tracing::Level::INFO).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let play = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["record", path] => {
            println!("{:?}", stream::record(path));
            return Ok(());
        }
        ["one"] => None,
        ["play", path] => Some((*path).to_string()),
        _ => {
            println!("usage: picture one | record FILE | play FILE");
            std::process::exit(2)
        }
    };
    shell::run(shell::event_loop()?, platform::Platform::default(), |started| {
        let (feed, said) = (Feed::default(), Said::new(Arc::clone(&started.told), String::new()));
        let node = started.painting.render_node().map(str::to_owned).ok_or_else(no_decoder);
        let shown = match &play {
            None => node.and_then(|node| stream::one_surface(&node)).map(|one| {
                let slot = Slot::new();
                slot.put_surface(one);
                feed.put(slot);
            }),
            Some(path) => node.and_then(|node| {
                let slot = Slot::new();
                let wake = Arc::clone(&started.wake);
                slot.wake_with(move || wake());
                feed.put(slot.clone());
                stream::play(path, &node, slot)
            }),
        };
        if let Err(why) = shown {
            println!("no picture to show: {why}");
        }
        let words = words::Words::new(words::Language::negotiate(&platform::language::preferred()));
        let watched = Watched::new(feed, said, words);
        Ok::<_, std::convert::Infallible>(match play {
            None => watched.finite(shell::frames().unwrap_or(FRAMES)),
            Some(_) => watched.counting(&stream::DECODED),
        })
    })
}

/// The pictures are a decoder's surfaces imported into the window, which only Linux does yet.
#[cfg(not(target_os = "linux"))]
fn main() {
    println!("picture: decoded surfaces reach the window only on Linux yet");
}
