//! What an imported picture held, let go of on a thread of its own once wgpu is done with the picture: a decoder's
//! last surface ends its device there, under the loader's lock, which a survey or a decoder's open may hold for a while,
//! so the thread that renders never waits for it.

use std::sync::mpsc;

/// Hands what a picture held to the release thread. The thread ends once every handle to it is gone.
#[derive(Clone)]
pub(crate) struct Releaser(mpsc::Sender<Box<dyn Send>>);

impl Releaser {
    /// Starts the release thread; `Err` where the system would not start one.
    pub(crate) fn start() -> std::io::Result<Self> {
        let (give, taken) = mpsc::channel::<Box<dyn Send>>();
        std::thread::Builder::new().name("picture release".into()).spawn(move || taken.into_iter().for_each(drop))?;
        Ok(Self(give))
    }

    /// Lets go of `held` on the release thread; where that thread has ended, here.
    pub(crate) fn give(&self, held: impl Send + 'static) {
        drop(self.0.send(Box::new(held)));
    }
}

#[cfg(test)]
mod tests {
    use super::Releaser;
    use std::sync::mpsc;
    use std::thread::{self, ThreadId};
    use std::time::Duration;

    struct Told(mpsc::Sender<ThreadId>);

    impl Drop for Told {
        fn drop(&mut self) {
            let _ = self.0.send(thread::current().id());
        }
    }

    #[test]
    fn what_a_picture_held_is_let_go_of_on_the_release_thread_not_the_giver() {
        let releaser = Releaser::start().unwrap();
        let (told, heard) = mpsc::channel();
        releaser.give(Told(told));
        let ended_on = heard.recv_timeout(Duration::from_secs(5)).expect("let go of");
        assert_ne!(ended_on, thread::current().id());
    }
}
