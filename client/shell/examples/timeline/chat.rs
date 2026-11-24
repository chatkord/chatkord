//! Messages for the timeline test, the same for a seed and as uneven in height as a real channel's: varied runs by one
//! author, styled text, accents and emoji, replies and reactions.

/// One run of a line, by how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Plain(String),
    Bold(String),
    Italic(String),
    Code(String),
    Mention(String),
    Link(String),
}

/// One message: who, when, and its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub author: &'static str,
    /// Minutes since the start of the log; a run by one author within `RUN_MINUTES` shares one header.
    pub minute: u32,
    /// The author this one answers, shown as a line above it.
    pub reply_to: Option<&'static str>,
    pub lines: Vec<Vec<Piece>>,
    pub reactions: Vec<(&'static str, u32)>,
}

/// A message by the same author within this many minutes of the one before, and not a reply, goes under its header.
pub(crate) const RUN_MINUTES: u32 = 7;

impl Message {
    /// Whether this message is drawn without its own header, under `before`.
    pub(crate) fn continues(&self, before: Option<&Message>) -> bool {
        before.is_some_and(|b| b.author == self.author && self.minute - b.minute < RUN_MINUTES)
            && self.reply_to.is_none()
    }
}

const AUTHORS: [&str; 6] = ["Ada", "Grace", "Bo", "Δήμητρα", "Γιώργος", "Mira"];

const ENGLISH: [&str; 24] = [
    "the", "stream", "dropped", "again", "after", "the", "key", "frame", "arrived", "late", "so", "I", "tried", "it",
    "on", "the", "laptop", "and", "it", "looks", "fine", "now", "thanks", "everyone",
];

const GREEK: [&str; 16] = [
    "καλημέρα",
    "ευχαριστώ",
    "πολύ",
    "ήχος",
    "εικόνα",
    "δεν",
    "ακούγεται",
    "καθόλου",
    "λίγο",
    "αργότερα",
    "θα",
    "ξαναδοκιμάσω",
    "όλα",
    "καλά",
    "σήμερα",
    "βράδυ",
];

const EMOJI: [&str; 4] = ["🙂", "🎧", "🔥", "👍"];

/// A small generator, so the same seed gives the same messages on every machine without a dependency.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn words(&mut self, count: usize, vocabulary: &[&str]) -> String {
        (0..count).map(|_| vocabulary[self.below(vocabulary.len())]).collect::<Vec<_>>().join(" ")
    }
}

fn line(random: &mut Random) -> Vec<Piece> {
    // Most lines are short, some run long enough to wrap twice or more in a window.
    let length = match random.below(10) {
        0 => 40 + random.below(30),
        1..=3 => 12 + random.below(14),
        _ => 2 + random.below(9),
    };
    let vocabulary: &[&str] = if random.below(4) == 0 { &GREEK } else { &ENGLISH };
    let mut pieces = vec![Piece::Plain(random.words(length, vocabulary))];
    match random.below(8) {
        0 => pieces.push(Piece::Bold(format!(" {}", random.words(2, vocabulary)))),
        1 => pieces.insert(0, Piece::Italic(format!("{} ", random.words(2, vocabulary)))),
        2 => pieces.push(Piece::Code(format!(" {}_{}", ENGLISH[random.below(ENGLISH.len())], random.below(100)))),
        3 => pieces.insert(0, Piece::Mention(format!("@{} ", AUTHORS[random.below(AUTHORS.len())]))),
        4 => pieces.push(Piece::Link(" example.invalid/clip".to_string())),
        5 => pieces.push(Piece::Plain(format!(" {}", EMOJI[random.below(EMOJI.len())]))),
        _ => {}
    }
    pieces
}

/// `count` messages, the same ones for the same `seed`.
pub(crate) fn corpus(count: usize, seed: u64) -> Vec<Message> {
    let mut random = Random(seed.max(1));
    let (mut minute, mut author) = (0u32, AUTHORS[0]);
    (0..count)
        .map(|_| {
            // Four in ten carry on a run: the same author again, a minute or two later.
            if random.below(10) < 4 {
                minute += 1 + random.below(2) as u32;
            } else {
                minute += 1 + random.below(12) as u32;
                author = AUTHORS[random.below(AUTHORS.len())];
            }
            let lines = match random.below(10) {
                0 => 4 + random.below(4),
                1..=3 => 2 + random.below(2),
                _ => 1,
            };
            let reply_to = (random.below(12) == 0).then(|| AUTHORS[random.below(AUTHORS.len())]);
            let reactions = if random.below(6) == 0 {
                (0..1 + random.below(3))
                    .map(|_| (EMOJI[random.below(EMOJI.len())], 1 + random.below(9) as u32))
                    .collect()
            } else {
                Vec::new()
            };
            Message { author, minute, reply_to, lines: (0..lines).map(|_| line(&mut random)).collect(), reactions }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_messages_and_another_seed_gives_others() {
        assert_eq!(corpus(200, 7), corpus(200, 7));
        assert_ne!(corpus(200, 7), corpus(200, 8));
    }

    #[test]
    fn a_corpus_is_as_long_as_asked_and_uneven_in_height_and_content() {
        let all = corpus(10_000, 1);
        assert_eq!(all.len(), 10_000);
        let lines: std::collections::BTreeSet<usize> = all.iter().map(|m| m.lines.len()).collect();
        assert!(lines.len() >= 4, "messages of only {lines:?} lines would make a list of even heights");
        let has = |wanted: fn(&Piece) -> bool| all.iter().flat_map(|m| m.lines.iter().flatten()).any(wanted);
        assert!(
            has(|p| matches!(p, Piece::Bold(_)))
                && has(|p| matches!(p, Piece::Italic(_)))
                && has(|p| matches!(p, Piece::Code(_)))
        );
        assert!(has(|p| matches!(p, Piece::Mention(_))) && has(|p| matches!(p, Piece::Link(_))));
        assert!(
            all.iter().any(|m| m.reply_to.is_some()) && all.iter().any(|m| !m.reactions.is_empty()),
            "no replies or reactions"
        );
        let runs = all.windows(2).filter(|w| w[1].continues(Some(&w[0]))).count();
        assert!(
            (2_000..7_000).contains(&runs),
            "{runs} of ten thousand messages continue a run: too few or too many to look like a chat"
        );
        let greek = all
            .iter()
            .flat_map(|m| m.lines.iter().flatten())
            .any(|p| matches!(p, Piece::Plain(t) if t.contains("ευχαριστώ") || t.contains("καλημέρα")));
        assert!(greek, "no Greek in ten thousand messages");
    }
}
