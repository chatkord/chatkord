//! FFmpeg's own log, taken from its stderr and noted at debug by level alone, so it is quiet by default and a host's
//! subscriber can show that it spoke.

use crate::av;

/// Takes FFmpeg's log for the rest of the process. Called before any work that may make FFmpeg log.
pub(crate) fn forward() {
    av::forward_log(noted);
}

/// Notes the level of a line FFmpeg logged, never the line: its text names paths, devices and addresses.
fn noted(level: i32) {
    report::note!(debug, "FFmpeg logged", source = report::Fact::Word(report::word!("ffmpeg")), level = level);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    /// Keeps the level of every event it is given.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<String>>>);

    impl Visit for Kept {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "level" {
                self.0.lock().unwrap().push(format!("{value:?}"));
            }
        }
    }

    impl Subscriber for Kept {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            event.record(&mut self.clone());
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    #[test]
    fn a_line_ffmpeg_logs_reaches_tracing_at_debug_as_its_level_alone() {
        let kept = Kept::default();
        tracing::subscriber::with_default(kept.clone(), || {
            forward();
            av::log_error_line();
        });
        assert_eq!(*kept.0.lock().unwrap(), [av::ERROR_LEVEL.to_string()], "the level, never the text");
    }
}
