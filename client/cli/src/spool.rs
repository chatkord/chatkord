//! Bytes for a file, written by a thread of their own, so neither an engine thread nor an async task waits on the
//! disk. A writer that cannot keep up is an error, never a file with a hole in it.

use std::io::{self, BufWriter, Write};
use std::mem;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};

/// How many bytes are gathered before they are handed to the writer.
const CHUNK: usize = 1 << 20;
/// How many chunks may wait for the writer: 64 MiB, a few seconds of a 4K screen recorded raw.
const WAITING: usize = 64;

/// A file being written by its own thread. [`Spool::close`] writes what is left and waits for the thread.
pub(crate) struct Spool {
    chunk: Vec<u8>,
    to: SyncSender<Vec<u8>>,
    writer: JoinHandle<io::Result<()>>,
}

impl Spool {
    pub(crate) fn start(out: impl Write + Send + 'static) -> Self {
        let (to, from) = sync_channel(WAITING);
        let writer = thread::spawn(move || write(BufWriter::new(out), &from));
        Self { chunk: Vec::with_capacity(CHUNK), to, writer }
    }

    /// Hands the last bytes over and waits until the writer has written and flushed everything. It blocks: call it off
    /// an async task.
    pub(crate) fn close(self) -> io::Result<()> {
        let Self { chunk, to, writer } = self;
        let handed =
            if chunk.is_empty() { Ok(()) } else { to.send(chunk).map_err(|_| io::ErrorKind::BrokenPipe.into()) };
        drop(to);
        let written = writer.join().unwrap_or_else(|_| Err(io::ErrorKind::Other.into()));
        handed.and(written)
    }
}

impl Write for Spool {
    /// Copies `bytes` and returns at once; a writer that is a whole spool behind is [`io::ErrorKind::WouldBlock`].
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.chunk.extend_from_slice(bytes);
        if self.chunk.len() >= CHUNK {
            let full = mem::replace(&mut self.chunk, Vec::with_capacity(CHUNK));
            match self.to.try_send(full) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => return Err(io::ErrorKind::WouldBlock.into()),
                Err(TrySendError::Disconnected(_)) => return Err(io::ErrorKind::BrokenPipe.into()),
            }
        }
        Ok(bytes.len())
    }

    /// The writer flushes when the spool is closed.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write(mut out: BufWriter<impl Write>, from: &Receiver<Vec<u8>>) -> io::Result<()> {
    for chunk in from {
        out.write_all(&chunk)?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Keeps what it is given.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<u8>>>);

    impl Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A disk that takes a second for every write.
    struct Slow;

    impl Write for Slow {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            thread::sleep(Duration::from_secs(1));
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn every_byte_given_is_written_once_the_spool_closes() {
        let kept = Kept::default();
        let mut spool = Spool::start(kept.clone());
        let bytes: Vec<u8> = (0..3 * CHUNK + 7).map(|at| u8::try_from(at % 251).expect("below 251")).collect();
        for piece in bytes.chunks(4096) {
            spool.write_all(piece).unwrap();
        }
        spool.close().unwrap();
        assert!(*kept.0.lock().unwrap() == bytes, "the file holds every byte in order");
    }

    #[test]
    fn a_writer_that_falls_behind_is_an_error_at_once_not_a_wait() {
        let mut spool = Spool::start(Slow);
        let piece = vec![0_u8; CHUNK];
        let started = Instant::now();
        let refused = (0..2 * WAITING).map(|_| spool.write_all(&piece)).find_map(Result::err).expect("it fell behind");
        assert_eq!(refused.kind(), io::ErrorKind::WouldBlock);
        assert!(started.elapsed() < Duration::from_secs(1), "the caller never waited on the disk");
    }
}
