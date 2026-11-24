//! A text cut to a bound in characters for a place that takes no more, an ellipsis ending what was cut.

use unicode_segmentation::UnicodeSegmentation;

/// `text` whole where it is at most `most` characters, else its first whole graphemes and an ellipsis, `most`
/// characters at most together, the space before the cut dropped.
pub(crate) fn shortened(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let room = most.saturating_sub(1);
    let mut used = 0;
    let kept: String = text
        .graphemes(true)
        .take_while(|grapheme| {
            used += grapheme.chars().count();
            used <= room
        })
        .collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::shortened;

    #[test]
    fn a_text_is_kept_whole_within_its_bound_and_ends_in_an_ellipsis_at_it() {
        assert_eq!(shortened("see you at eight", 16), "see you at eight");
        let cut = shortened("see you at eight, bring the map", 16);
        assert_eq!(cut, "see you at eigh…");
        assert_eq!(cut.chars().count(), 16, "never over what the place takes");
        assert_eq!(shortened("één twee drie", 5), "één…", "counted in characters, the space before the cut dropped");
    }

    #[test]
    fn a_cut_never_splits_a_grapheme() {
        let family = "👩\u{200d}👩\u{200d}👧";
        assert_eq!(
            shortened(&format!("ab{family}cd"), 5),
            "ab…",
            "the family is five characters: kept whole or not at all"
        );
        assert_eq!(shortened("cafe\u{301}", 5), "cafe\u{301}");
        assert_eq!(shortened("cafe\u{301}s!", 5), "caf…", "an accent stays with its letter");
    }
}
