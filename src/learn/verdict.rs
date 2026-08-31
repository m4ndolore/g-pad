//! Reading the tutor's reply. The protocol is one leading word — YES, ALMOST,
//! or NO — then one to three capital cheer words for the child, then (after
//! ALMOST or NO) one short hint line for the grown-up to read aloud. The
//! verdict word is for the sheet (which mark to draw, how the ladder moves);
//! the cheer is the child's channel; the hint is the adult's.
//!
//! Anything that does not lead with a verdict word degrades to Unknown: the
//! text still shows, but no mark is drawn and the ladder holds still. A
//! misread verdict must never move consequence — the same rule the marking
//! vocabulary lives by. And the model can ramble, but the child's channel
//! cannot: a cheer longer than three words is demoted to the hint and a
//! fixed cheer stands in, so what the child must read alone stays short.

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

    /// The cheer the sheet prints when the reply offered none it could use.
    pub fn default_cheer(self) -> &'static str {
        match self {
            Verdict::Yes => "GREAT JOB!",
            Verdict::Almost => "SO CLOSE!",
            Verdict::No => "TRY AGAIN!",
            Verdict::Unknown => "",
        }
    }
}

/// What the child and the grown-up each get to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feedback {
    /// One to three capital words the child reads alone. Never empty for a
    /// known verdict — the default cheer stands in.
    pub cheer: String,
    /// The grown-up's line, read aloud. Empty when there is nothing to add.
    pub hint: String,
}

/// Split a reply into its verdict, the child's cheer, and the grown-up hint.
pub fn parse(reply: &str) -> (Verdict, Feedback) {
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
    if verdict == Verdict::Unknown {
        return (verdict, Feedback { cheer: String::new(), hint: trimmed.to_string() });
    }
    let rest = trimmed[word.len()..]
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ':' | ';' | '!' | '-' | '—'))
        .to_string();
    let (first_line, later_lines) = match rest.split_once('\n') {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (rest.trim().to_string(), String::new()),
    };
    // A short first line is the cheer; a long one was prose and belongs to
    // the grown-up. The child's channel is at most three words, always caps.
    let (cheer, mut hint) = if !first_line.is_empty() && first_line.split_whitespace().count() <= 3 {
        (first_line.to_ascii_uppercase(), later_lines)
    } else {
        let mut h = first_line;
        if !later_lines.is_empty() {
            if !h.is_empty() {
                h.push(' ');
            }
            h.push_str(&later_lines);
        }
        (verdict.default_cheer().to_string(), h)
    };
    let cheer = if cheer.is_empty() { verdict.default_cheer().to_string() } else { cheer };
    hint = hint.split_whitespace().collect::<Vec<_>>().join(" ");
    (verdict, Feedback { cheer, hint })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_words_are_read_through_punctuation_and_case() {
        let (v, f) = parse("YES! GREAT JOB");
        assert_eq!(v, Verdict::Yes);
        assert_eq!(f.cheer, "GREAT JOB");
        assert_eq!(f.hint, "");
        let (v, f) = parse("yes. wow");
        assert_eq!(v, Verdict::Yes);
        assert_eq!(f.cheer, "WOW");
        let (v, _) = parse("No, try counting the dots again.");
        assert_eq!(v, Verdict::No);
    }

    #[test]
    fn the_hint_line_goes_to_the_grown_up() {
        let (v, f) = parse("ALMOST SO CLOSE\nYour 3 is facing the wrong way.");
        assert_eq!(v, Verdict::Almost);
        assert_eq!(f.cheer, "SO CLOSE");
        assert_eq!(f.hint, "Your 3 is facing the wrong way.");
    }

    #[test]
    fn a_rambling_reply_is_demoted_to_the_hint_and_the_cheer_stands_in() {
        let (v, f) = parse("YES Three and four make seven, what a lovely bond you wrote.");
        assert_eq!(v, Verdict::Yes);
        assert_eq!(f.cheer, "GREAT JOB!");
        assert!(f.hint.starts_with("Three and four"));
        // A bare verdict still cheers.
        let (v, f) = parse("NO");
        assert_eq!(v, Verdict::No);
        assert_eq!(f.cheer, "TRY AGAIN!");
        assert_eq!(f.hint, "");
    }

    #[test]
    fn anything_else_is_unknown_and_keeps_the_whole_text() {
        let (v, f) = parse("Great work on this page!");
        assert_eq!(v, Verdict::Unknown);
        assert_eq!(f.cheer, "");
        assert_eq!(f.hint, "Great work on this page!");
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
