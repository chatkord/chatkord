//! Every task an errand starts, which nothing else in this crate may start: it is named, and a panic in it is noted by
//! that name before the task ends as a panicking task does, so no errand's end goes unseen.

use report::Word;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

fn noted(name: Word) {
    report::note!(error, "an errand panicked", errand = report::Fact::Word(name));
}

/// A future whose panic is noted by its errand's name, then continues as the panic it was.
struct Noted<F> {
    name: Word,
    future: Pin<Box<F>>,
}

impl<F: Future> Future for Noted<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let name = self.name;
        match catch_unwind(AssertUnwindSafe(|| self.future.as_mut().poll(cx))) {
            Ok(polled) => polled,
            Err(panic) => {
                noted(name);
                resume_unwind(panic)
            }
        }
    }
}

/// Runs `future` as the errand `name` on `runtime`.
#[expect(clippy::disallowed_methods, reason = "the one place an errand is spawned, so that each is named and caught")]
pub(crate) fn spawn<F>(runtime: &Runtime, name: Word, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime.spawn(Noted { name, future: Box::pin(future) })
}

/// Runs `future` as the errand `name` on the runtime this thread is in.
#[expect(clippy::disallowed_methods, reason = "the one place an errand is spawned, so that each is named and caught")]
pub(crate) fn spawn_here<F>(name: Word, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(Noted { name, future: Box::pin(future) })
}

/// Runs `work` as the errand `name` on the blocking pool.
#[expect(clippy::disallowed_methods, reason = "the one place an errand is spawned, so that each is named and caught")]
pub(crate) fn blocking<T, W>(name: Word, work: W) -> JoinHandle<T>
where
    W: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || match catch_unwind(AssertUnwindSafe(work)) {
        Ok(done) => done,
        Err(panic) => {
            noted(name);
            resume_unwind(panic)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn an_errand_that_panics_is_noted() {
        // Global: the blocking pool's thread notes too.
        let ring = report::Ring::new(std::time::SystemTime::now);
        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(ring.layer())).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let asked = spawn(&runtime, report::word!("hangs up"), async { panic!("what a person typed") });
        assert!(runtime.block_on(asked).unwrap_err().is_panic(), "the task still ends as a panic");
        let blocked = runtime.block_on(async { blocking(report::word!("reads a file"), || panic!("again")).await });
        assert!(blocked.unwrap_err().is_panic());
        let lines = ring.lines();
        let said: Vec<&str> = lines.iter().map(report::LogLine::as_str).collect();
        let said = said.join("\n");
        assert!(said.contains("an errand panicked  errand=hangs up"), "{said}");
        assert!(said.contains("errand=reads a file") && !said.contains("typed"), "{said}");
    }
}
