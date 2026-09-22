//! A tap keyboard for the SYSTEM page. Pure model: what the keys are, what
//! a press does to the text. Drawing lives in `draw`, which lays the rows
//! out from `rows()` and gives every key a hit region.
//!
//! Built for the Wi-Fi join sheet; nothing else uses it yet.

/// What one key does. `Char` types; the rest edit or leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Shift,
    Symbols,
    Space,
    Backspace,
    Cancel,
    Go,
}

/// One key as it should be painted: what it is, what it says, and its
/// width in row units (a row's units scale to the full page width).
#[derive(Clone, Debug, PartialEq)]
pub struct Cap {
    pub key: Key,
    pub label: String,
    pub units: f32,
}

/// What a press means to the sheet that owns the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The text changed (or a mode toggled); repaint.
    Edited,
    Cancel,
    Go,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keyboard {
    pub text: String,
    /// One-shot: the next letter is upper case, then shift releases.
    pub shift: bool,
    /// The symbols page replaces the letter rows.
    pub symbols: bool,
    /// The label on the GO key: JOIN, SAVE, …
    pub go_label: &'static str,
}

const LETTER_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
const SYMBOL_ROWS: [&str; 3] = ["!@#$%^&*()", "-_=+[]{}|~", ";:'\",./?\\"];
const DIGITS: &str = "1234567890";

impl Keyboard {
    pub fn new(go_label: &'static str) -> Self {
        Self { go_label, ..Self::default() }
    }

    /// The rows to paint, top to bottom. Letters follow `shift`; the third
    /// row carries SHIFT (letters only); the fourth carries the page toggle
    /// and BACKSPACE; the last CANCEL, SPACE and GO.
    pub fn rows(&self) -> Vec<Vec<Cap>> {
        let chars = |s: &str| -> Vec<Cap> {
            s.chars()
                .map(|c| {
                    let c = if self.shift && !self.symbols { c.to_ascii_uppercase() } else { c };
                    Cap { key: Key::Char(c), label: c.to_string(), units: 1.0 }
                })
                .collect()
        };
        let cap = |key: Key, label: &str, units: f32| Cap { key, label: label.into(), units };
        let (r2, r3, r4) = if self.symbols {
            (SYMBOL_ROWS[0], SYMBOL_ROWS[1], SYMBOL_ROWS[2])
        } else {
            (LETTER_ROWS[0], LETTER_ROWS[1], LETTER_ROWS[2])
        };
        let mut third = chars(r3);
        if !self.symbols {
            third.push(cap(Key::Shift, if self.shift { "SHIFT ●" } else { "SHIFT" }, 1.0));
        }
        let mut fourth = vec![cap(Key::Symbols, if self.symbols { "ABC" } else { "#+=" }, 1.5)];
        fourth.extend(chars(r4));
        fourth.push(cap(Key::Backspace, "⌫", 1.5));
        vec![
            chars(DIGITS),
            chars(r2),
            third,
            fourth,
            vec![
                cap(Key::Cancel, "CANCEL", 2.0),
                cap(Key::Space, "", 6.0),
                cap(Key::Go, self.go_label, 2.0),
            ],
        ]
    }

    pub fn press(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(c) => {
                self.text.push(c);
                self.shift = false;
                Outcome::Edited
            }
            Key::Space => {
                self.text.push(' ');
                Outcome::Edited
            }
            Key::Backspace => {
                self.text.pop();
                Outcome::Edited
            }
            Key::Shift => {
                self.shift = !self.shift;
                Outcome::Edited
            }
            Key::Symbols => {
                self.symbols = !self.symbols;
                self.shift = false;
                Outcome::Edited
            }
            Key::Cancel => Outcome::Cancel,
            Key::Go => Outcome::Go,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(row: &[Cap]) -> String {
        row.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join("")
    }

    #[test]
    fn typing_shift_and_backspace_edit_the_text() {
        let mut kb = Keyboard::new("JOIN");
        assert_eq!(kb.press(Key::Char('a')), Outcome::Edited);
        kb.press(Key::Shift);
        assert!(kb.rows()[1].iter().all(|c| c.label.chars().all(|ch| ch.is_ascii_uppercase())), "shift shows capitals");
        kb.press(Key::Char('B'));
        assert!(!kb.shift, "shift is one-shot");
        kb.press(Key::Space);
        kb.press(Key::Char('c'));
        kb.press(Key::Backspace);
        assert_eq!(kb.text, "aB ");
        assert_eq!(kb.press(Key::Backspace), Outcome::Edited);
        assert_eq!(kb.text, "aB");
    }

    #[test]
    fn the_symbols_page_swaps_the_letter_rows_and_keeps_digits() {
        let mut kb = Keyboard::new("JOIN");
        assert_eq!(labels(&kb.rows()[0]), "1234567890");
        assert_eq!(labels(&kb.rows()[1]), "qwertyuiop");
        kb.press(Key::Symbols);
        assert_eq!(labels(&kb.rows()[0]), "1234567890");
        assert_eq!(labels(&kb.rows()[1]), "!@#$%^&*()");
        assert!(kb.rows()[2].iter().all(|c| c.key != Key::Shift), "no SHIFT on the symbols page");
        kb.press(Key::Char('#'));
        assert_eq!(kb.text, "#");
        kb.press(Key::Symbols);
        assert_eq!(labels(&kb.rows()[1]), "qwertyuiop");
    }

    #[test]
    fn every_row_has_ten_units_except_the_last_and_the_fourth() {
        let kb = Keyboard::new("JOIN");
        let units: Vec<f32> = kb.rows().iter().map(|r| r.iter().map(|c| c.units).sum()).collect();
        assert_eq!(units[0], 10.0);
        assert_eq!(units[1], 10.0);
        assert_eq!(units[2], 10.0);
        assert_eq!(units[3], 10.0, "toggle 1.5 + seven letters + backspace 1.5");
        assert_eq!(units[4], 10.0, "CANCEL 2 + SPACE 6 + GO 2");
    }

    #[test]
    fn cancel_and_go_leave_the_text_alone_and_report() {
        let mut kb = Keyboard::new("JOIN");
        kb.press(Key::Char('x'));
        assert_eq!(kb.press(Key::Cancel), Outcome::Cancel);
        assert_eq!(kb.press(Key::Go), Outcome::Go);
        assert_eq!(kb.text, "x");
        assert_eq!(kb.rows()[4][2].label, "JOIN");
    }
}
