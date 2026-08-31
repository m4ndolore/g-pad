//! Reading the tutor's reply. The protocol is one leading word — YES, ALMOST,
//! or NO — then a short sentence for the child. The word is for the sheet
//! (which mark to draw, how the ladder moves); the sentence is for the child.
//!
//! Anything that does not lead with a verdict word degrades to Unknown: the
//! feedback still reads aloud, but no mark is drawn and the ladder holds
//! still. A misread verdict must never move consequence — the same rule the
//! marking vocabulary lives by.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Yes,
    Almost,
    No,
    Unknown,
}

impl Verdict {
    pub fn counts_as_correct(self) -> Option<bool> {
        match self {
            Verdict::Yes => Some(true),
            Verdict::Almost | Verdict::No => Some(false),
            Verdict::Unknown => None,
        }
    }
}

/// Split a reply into its verdict and the sentence meant for the child.
pub fn parse(reply: &str) -> (Verdict, String) {
    let trimmed = reply.trim();
    let word: String = trimmed
        .chars()
        .take_while(|c| c.is_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    let verdict = match word.as_str() {
        "YES" => Verdict::Yes,
        "ALMOST" => Verdict::Almost,
        "NO" | "NOT" => Verdict::No,
        _ => Verdict::Unknown,
    };
    let rest = if verdict == Verdict::Unknown {
        trimmed.to_string()
    } else {
        trimmed[word.len()..]
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ':' | ';' | '!' | '-' | '—'))
            .to_string()
    };
    (verdict, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_words_are_read_through_punctuation_and_case() {
        assert_eq!(parse("YES! Wonderful counting."), (Verdict::Yes, "Wonderful counting.".into()));
        assert_eq!(parse("yes. Three and four make seven."), (Verdict::Yes, "Three and four make seven.".into()));
        assert_eq!(parse("ALMOST — your 3 is backwards."), (Verdict::Almost, "your 3 is backwards.".into()));
        assert_eq!(parse("No, try counting the dots again."), (Verdict::No, "try counting the dots again.".into()));
    }

    #[test]
    fn anything_else_is_unknown_and_keeps_the_whole_text() {
        let (v, rest) = parse("Great work on this page!");
        assert_eq!(v, Verdict::Unknown);
        assert_eq!(rest, "Great work on this page!");
        assert_eq!(parse("").0, Verdict::Unknown);
    }

    #[test]
    fn unknown_never_moves_the_ladder() {
        assert_eq!(Verdict::Unknown.counts_as_correct(), None);
        assert_eq!(Verdict::Yes.counts_as_correct(), Some(true));
        assert_eq!(Verdict::Almost.counts_as_correct(), Some(false));
    }

    #[test]
    fn a_yes_buried_in_prose_does_not_count() {
        // "Yesterday" must not read as YES.
        let (v, _) = parse("Yesterday you did better.");
        assert_eq!(v, Verdict::Unknown);
    }
}
