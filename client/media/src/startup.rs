//! What a start of the program reads and does before its first thread, while it chooses which drivers the loaders
//! take: whether it is still single-threaded, the GPUs the kernel lists and which of them sleep, a child of its own
//! waited for within a bound, and a file kept whole.

use std::fs::DirEntry;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

/// Where the kernel lists the GPUs.
pub const DRM: &str = "/sys/class/drm";
/// What every render node's path starts with, its number after it.
const NODE: &str = "/dev/dri/renderD";
/// The most of `/proc/self/status` read: it is about 1.5 KB, with `Threads:` in its first half.
const STATUS: u16 = 16384;
/// How often a child is looked at while it is waited for.
const POLL: Duration = Duration::from_millis(50);

/// Whether this process has one thread, as the kernel says: a status that cannot be read proves nothing.
pub fn single_threaded() -> bool {
    let mut status = String::new();
    let read = std::fs::File::open("/proc/self/status")
        .and_then(|file| file.take(u64::from(STATUS)).read_to_string(&mut status));
    read.is_ok() && threads(&status) == Some(1)
}

/// The thread count in the text of `/proc/self/status`: its `Threads:` line, a whole number or none.
pub fn threads(status: &str) -> Option<u32> {
    status.lines().find_map(|line| line.strip_prefix("Threads:")).and_then(|count| count.trim().parse().ok())
}

/// The render nodes listed under `root` ([`DRM`], or a stand-in for it), in no order: each a folder whose `device`
/// is its GPU's.
pub fn render_nodes(root: &str) -> impl Iterator<Item = DirEntry> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
}

/// The device path of the render node `entry` lists, as `/dev/dri/renderD128`.
pub fn node_path(entry: &DirEntry) -> String {
    format!("/dev/dri/{}", entry.file_name().to_string_lossy())
}

/// Whether the kernel has put the GPU of the render node `entry` lists to sleep: asking its driver anything wakes it.
pub fn asleep(entry: &DirEntry) -> bool {
    std::fs::read_to_string(entry.path().join("device/power/runtime_status"))
        .is_ok_and(|status| status.trim() == "suspended")
}

/// Whether `token` names a render node: `/dev/dri/renderD` and a number.
pub fn is_render_node(token: &str) -> bool {
    token.strip_prefix(NODE).is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// `child`'s end if it comes within `bound`; past it the child is killed and reaped, and there is none.
pub fn within(child: &mut Child, bound: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + bound;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

/// `text` in `file`, whole or not at all: a start reading it never sees a half-written one, nor does one process
/// write into another's.
pub fn keep(file: &Path, text: &str) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let partial = file.with_extension(format!("partial-{}", std::process::id()));
    std::fs::write(&partial, text)?;
    std::fs::rename(partial, file)
}

#[cfg(test)]
mod tests {
    use super::{asleep, is_render_node, keep, node_path, render_nodes, threads};

    #[test]
    fn the_thread_count_is_read_from_the_status_line_or_not_at_all() {
        let status = |threads: &str| format!("Name:\tshell\nState:\tR (running)\nThreads:{threads}\nSigQ:\t0/1\n");
        assert_eq!(threads(&status("\t1")), Some(1));
        assert_eq!(threads(&status("\t3")), Some(3));
        assert_eq!(threads("Name:\tshell\nState:\tR (running)\n"), None, "no Threads line");
        assert_eq!(threads(&status("\tmany")), None);
        assert_eq!(threads(&status("\t-1")), None);
        assert_eq!(threads(&status("")), None);
        assert_eq!(threads(""), None);
    }

    #[test]
    fn a_render_node_is_named_by_its_number_and_only_a_suspended_gpu_sleeps() {
        assert!(is_render_node("/dev/dri/renderD128"));
        for not in ["/dev/dri/renderD", "/dev/dri/card0", "/dev/dri/renderD12a", "renderD128"] {
            assert!(!is_render_node(not), "{not}");
        }
        let root = std::env::temp_dir().join(format!("media-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (node, status) in
            [("renderD128", Some("suspended\n")), ("renderD129", Some("active\n")), ("renderD130", None)]
        {
            let power = root.join(node).join("device/power");
            std::fs::create_dir_all(&power).unwrap();
            if let Some(status) = status {
                std::fs::write(power.join("runtime_status"), status).unwrap();
            }
        }
        std::fs::create_dir_all(root.join("card0/device/power")).unwrap();
        let mut sleeping: Vec<String> =
            render_nodes(root.to_str().unwrap()).filter(asleep).map(|entry| node_path(&entry)).collect();
        sleeping.sort();
        let listed = render_nodes(root.to_str().unwrap()).count();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(sleeping, ["/dev/dri/renderD128"], "awake, unmanaged and card nodes are not asleep");
        assert_eq!(listed, 3, "a card node is no render node");
    }

    #[test]
    fn a_kept_file_is_whole_and_leaves_nothing_beside_it() {
        let folder = std::env::temp_dir().join(format!("media-startup-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let file = folder.join("state/kept.txt");
        keep(&file, "an earlier text").unwrap();
        keep(&file, "the text").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let beside = std::fs::read_dir(file.parent().unwrap()).unwrap().count();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!((text.as_str(), beside), ("the text", 1));
    }
}
