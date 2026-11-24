//! The fuzz target's entry for a page the preview scan reads.

use crate::scan::{self, Head, MAX_DESCRIPTION, MAX_TITLE, Scan, ScanRefused};

/// The most bytes of a page that are also fed one at a time.
const SINGLY_UP_TO: usize = 4096;

/// `page` fed in parts that end at each of `ends`, in order.
fn fed(page: &[u8], ends: &[usize], cut: bool) -> Result<Head, ScanRefused> {
    let mut scan = Scan::default();
    let mut from = 0;
    for end in ends.iter().copied().chain([page.len()]) {
        scan.feed(page.get(from..end).unwrap_or_default());
        from = end;
    }
    scan.finish(cut)
}

/// A page as a host sends it, stopped or not at its end by the first byte's low bit, fed in two parts that end at the
/// share of the page its top six bits name, and a byte at a time when its second bit says so: whatever the bytes and
/// the parts, the scan ends in the same head or refusal as one feed of the page does, every line it keeps is within
/// its bound, free of controls and direction overrides, and in single spaces.
pub fn preview_scan(data: &[u8]) {
    let (first, page) = data.split_first().map_or((0, data), |(first, page)| (*first, page));
    let cut = first & 1 == 1;
    let whole = scan::page(page, cut);
    let split = page.len() * usize::from(first >> 2) / 64;
    assert_eq!(fed(page, &[split], cut), whole, "a page in two parts at {split}");
    if first & 2 == 2 && page.len() <= SINGLY_UP_TO {
        let ends: Vec<usize> = (1..page.len()).collect();
        assert_eq!(fed(page, &ends, cut), whole, "a page a byte at a time");
    }
    let Ok(head) = whole else { return };
    let line = |text: &Option<String>, most: usize| {
        if let Some(text) = text {
            assert!(!text.is_empty() && text.chars().count() <= most, "a line outside its bound: {text:?}");
            assert!(text.split(' ').all(|word| !word.is_empty()), "a line with a run of spaces or an edge: {text:?}");
            assert!(
                !text.chars().any(|c| (c.is_whitespace() && c != ' ')
                    || (c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))),
                "a control in a line: {text:?}"
            );
        }
    };
    line(&head.title, MAX_TITLE);
    line(&head.description, MAX_DESCRIPTION);
}

#[cfg(test)]
mod tests {
    use super::preview_scan;
    use std::path::{Path, PathBuf};

    fn seeds(target: &str) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
        proto::fuzz::seeds(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds"), target)
    }

    #[test]
    fn every_kept_page_scans_to_a_head_or_a_refusal() {
        for (_, input) in seeds("preview_scan").expect("the seeds") {
            preview_scan(&input);
        }
    }
}
