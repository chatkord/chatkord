//! A step a test took outside its process, undone when it is dropped or when the test process dies, whichever comes
//! first: a crash runs no `Drop`, so the undoing waits in a shell of its own for its stdin to close, which the kernel
//! does as the process ends.

use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};

/// Runs `program` with its arguments once this is dropped or the test process ends.
pub struct Undo {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Undo {
    /// None when no shell would start, so the caller skips rather than leaving a step nothing undoes.
    pub fn new(program: &str, args: &[&str]) -> Option<Undo> {
        let mut child = Command::new("sh")
            .args(["-c", "cat >/dev/null; exec \"$@\"", "sh", program])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // A group of its own: a runner that ends the test's group on a timeout leaves this to do its work.
            .process_group(0)
            .spawn()
            .ok()?;
        let stdin = child.stdin.take();
        Some(Undo { child, stdin })
    }

    /// Forgets the step without undoing it: what it would undo is already gone.
    pub fn forget(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Undo {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::Undo;
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// Set in the process the test below starts and kills.
    const MARK: &str = "CHATKORD_UNDO_MARK";

    fn marked(at: &Path, within: Duration) -> bool {
        let asked = Instant::now();
        while asked.elapsed() < within {
            if at.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        at.exists()
    }

    /// The process the test kills: it takes a step whose undoing writes the mark, says so, and waits to be killed.
    #[test]
    #[ignore = "started by the test below, never on its own"]
    fn a_process_that_is_killed_with_its_step_taken() {
        // Run by hand without the mark, it takes no step.
        if let Ok(mark) = std::env::var(MARK) {
            let undo = Undo::new("touch", &[&mark]).expect("a shell");
            println!("taken");
            std::thread::sleep(Duration::from_secs(60));
            drop(undo);
        }
    }

    #[test]
    fn a_step_is_undone_when_its_test_is_killed_and_when_it_is_dropped() {
        let dir = std::env::temp_dir();
        let killed = dir.join(format!("undo-killed-{}", std::process::id()));
        let _ = std::fs::remove_file(&killed);
        let mut child = Command::new(std::env::current_exe().expect("this test"))
            .args(["--exact", "undo::tests::a_process_that_is_killed_with_its_step_taken", "--ignored", "--nocapture"])
            .env(MARK, &killed)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the test binary again");
        let mut said = String::new();
        let out = child.stdout.take().expect("its output");
        let mut lines = std::io::BufRead::lines(std::io::BufReader::new(out));
        while !said.contains("taken") {
            said = lines.next().expect("it says the step is taken").expect("a line");
        }
        assert!(!killed.exists(), "nothing is undone while the test runs");
        child.kill().expect("killed");
        child.wait().expect("reaped");
        assert!(marked(&killed, Duration::from_secs(5)), "a killed test's step is undone");

        let dropped = dir.join(format!("undo-dropped-{}", std::process::id()));
        let _ = std::fs::remove_file(&dropped);
        drop(Undo::new("touch", &[dropped.to_str().expect("a path")]).expect("a shell"));
        assert!(dropped.exists(), "dropped, the step is undone at once");
        let forgotten = dir.join(format!("undo-forgotten-{}", std::process::id()));
        Undo::new("touch", &[forgotten.to_str().expect("a path")]).expect("a shell").forget();
        assert!(!marked(&forgotten, Duration::from_millis(200)), "a forgotten step is not undone");
        for made in [killed, dropped] {
            let _ = std::fs::remove_file(made);
        }
    }
}
