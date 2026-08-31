//! Learn mode — the kids' tutor surface of anthink.
//!
//! A challenge renders, the child answers in their own hand, and anchored
//! marks carry the commands: a mark in DONE commits the answer region to the
//! oracle for marking, a mark in NEW deals a fresh page. Free-floating shape
//! recognition is never used; every consequential mark is a bbox lookup
//! against the hit map the sheet returned when it was drawn.
//!
//! Two decks share the page. **Practice** is Singapore Math's concrete →
//! pictorial → abstract ladder plus handwriting; problems are generated
//! locally and the oracle only ever READS the child's ink. **Play** is the
//! reward: every couple of right answers deals a game page (doodle critter,
//! reverse Pictionary, a choose-your-own-adventure) — same pen, same marks,
//! more giggling. `RIDDLE_LEARN_PLAY` = `earned` (default) | `always` |
//! `never` sets the mix.

pub mod games;
pub mod problems;
pub mod sheet;
pub mod verdict;

use ab_glyph::FontRef;

use crate::fb::BBox;
use crate::surface::Surface;

pub use sheet::{HitMap, Target};
pub use verdict::Verdict;

/// Correct practice answers that earn one play page.
const PLAY_EVERY: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Practice(problems::Problem),
    Play(games::Game),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayPolicy {
    Earned,
    Always,
    Never,
}

pub struct Session {
    ladder: problems::Ladder,
    rng: problems::Rng,
    rot: usize,
    game_rot: usize,
    policy: PlayPolicy,
    earned: u32,
    pub page: Page,
    pub hits: HitMap,
}

impl Session {
    pub fn start() -> Self {
        let level: u8 = std::env::var("RIDDLE_LEARN_LEVEL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let policy = match std::env::var("RIDDLE_LEARN_PLAY").as_deref() {
            Ok("always") => PlayPolicy::Always,
            Ok("never") => PlayPolicy::Never,
            _ => PlayPolicy::Earned,
        };
        Self::start_with(level, now_seed(), policy)
    }

    pub fn start_at(level: u8, seed: u32) -> Self {
        Self::start_with(level, seed, PlayPolicy::Earned)
    }

    fn start_with(level: u8, seed: u32, policy: PlayPolicy) -> Self {
        let mut rng = problems::Rng::new(seed);
        let ladder = problems::Ladder::new(level);
        let page = if policy == PlayPolicy::Always {
            Page::Play(games::Game::nth(0))
        } else {
            Page::Practice(problems::generate(ladder.level, 0, &mut rng))
        };
        let game_rot = usize::from(policy == PlayPolicy::Always);
        Self { ladder, rng, rot: 0, game_rot, policy, earned: 0, page, hits: empty_hits() }
    }

    pub fn level(&self) -> u8 {
        self.ladder.level
    }

    pub fn as_practice(&self) -> Option<&problems::Problem> {
        match &self.page {
            Page::Practice(p) => Some(p),
            Page::Play(_) => None,
        }
    }

    /// Draw the current page and remember its hit map.
    pub fn draw(&mut self, surf: &mut Surface, ui_font: &FontRef) {
        self.hits = match &self.page {
            Page::Practice(p) => sheet::draw(surf, ui_font, p, self.ladder.level, self.ladder.streak()),
            Page::Play(g) => games::draw(surf, ui_font, g),
        };
    }

    /// Deal the next page (call `draw` afterwards). A play page is dealt when
    /// earned (or always, under that policy); leaving a play page under the
    /// earned policy returns to practice — the next treat is earned again.
    pub fn next(&mut self) {
        let deal_play = match self.policy {
            PlayPolicy::Always => true,
            PlayPolicy::Never => false,
            PlayPolicy::Earned => {
                self.earned >= PLAY_EVERY && matches!(self.page, Page::Practice(_))
            }
        };
        if deal_play {
            self.earned = 0;
            self.page = Page::Play(games::Game::nth(self.game_rot));
            self.game_rot += 1;
        } else {
            self.rot += 1;
            self.page = Page::Practice(problems::generate(self.ladder.level, self.rot, &mut self.rng));
        }
    }

    /// A marked practice answer came back; move the ladder (and the treat
    /// counter). Play pages never call this.
    pub fn record(&mut self, v: Verdict) {
        if let Some(correct) = v.counts_as_correct() {
            self.ladder.record(correct);
            if correct {
                self.earned += 1;
            }
        }
    }

    /// The child ticked story choice box `i`: remember it as pending and
    /// return its label for the ask. `None` when there is no such box.
    pub fn choose(&mut self, i: usize) -> Option<String> {
        if let Page::Play(games::Game::Story { choices, pending, .. }) = &mut self.page {
            let label = choices.get(i)?.clone();
            *pending = Some(label.clone());
            return Some(label);
        }
        None
    }

    /// A story beat arrived: log the pending choice with it, put the new
    /// choices on offer.
    pub fn story_advance(&mut self, beat: String, next_choices: Vec<String>) {
        if let Page::Play(games::Game::Story { log, choices, pending }) = &mut self.page {
            log.push((pending.take().unwrap_or_default(), beat));
            *choices = next_choices;
        }
    }

    /// Prior story beats as oracle dialogue, oldest first, so the tale stays
    /// coherent across turns without any new transport.
    pub fn story_history(&self) -> Vec<(String, String)> {
        match &self.page {
            Page::Play(games::Game::Story { log, .. }) => log
                .iter()
                .map(|(choice, beat)| {
                    let you = if choice.is_empty() {
                        "(the story begins)".to_string()
                    } else {
                        format!("I choose: {choice}")
                    };
                    (you, beat.clone())
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The pad took a decorating turn on the critter.
    pub fn critter_turn(&mut self) {
        if let Page::Play(games::Game::Critter { round }) = &mut self.page {
            *round = round.saturating_add(1);
        }
    }

    /// Does DONE require ink in the answer region on this page? Everywhere
    /// but the story, where an empty page just means the pad invents a hero.
    pub fn needs_ink(&self) -> bool {
        !matches!(self.page, Page::Play(games::Game::Story { .. }))
    }

    /// The per-turn instruction for the oracle: tutor on practice pages,
    /// playmate on play pages. Travels as user text, so it works unchanged on
    /// both oracle backends and never disturbs the pad's persona.
    pub fn instruction(&self) -> String {
        match &self.page {
            Page::Practice(problem) => format!(
                "LEARN MODE. You are a warm, playful tutor for a child aged four to eight. \
                 The page shows only the child's pen work for this exercise: {brief}. \
                 Decide whether the child's writing shows the correct answer. \
                 Your reply MUST start with exactly one word: YES if it is right, \
                 ALMOST if it is nearly right (a reversed digit, shaky but readable letters), \
                 NO if it is wrong, missing, or unreadable. \
                 Then one short cheerful sentence for the child — at most twelve very simple words. \
                 After NO, add a tiny hint but never the answer. \
                 After YES, you may add one fun follow-up thought about the same numbers or word. \
                 Never mention pictures, images, or cameras.",
                brief = problem.brief()
            ),
            Page::Play(game) => {
                let pending = match game {
                    games::Game::Story { pending, .. } => pending.as_deref(),
                    _ => None,
                };
                game.instruction(pending)
            }
        }
    }
}

fn now_seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0x5EED)
}

fn empty_hits() -> HitMap {
    HitMap { answer: BBox::empty(), done: BBox::empty(), new: BBox::empty(), choices: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_deals_a_new_problem_each_time() {
        let mut s = Session::start_at(2, 99);
        let first = s.page.clone();
        s.next();
        // Both are practice pages, of different activities.
        let (Page::Practice(a), Page::Practice(b)) = (&first, &s.page) else {
            panic!("earned policy starts in practice");
        };
        assert_ne!(std::mem::discriminant(&a.kind), std::mem::discriminant(&b.kind));
    }

    #[test]
    fn the_instruction_names_the_protocol_and_the_answer() {
        let s = Session::start_at(1, 7);
        let i = s.instruction();
        assert!(i.contains("YES"));
        assert!(i.contains("ALMOST"));
        assert!(i.contains(&s.as_practice().unwrap().expected()));
    }

    #[test]
    fn recording_yes_answers_eventually_raises_the_level() {
        let mut s = Session::start_at(1, 5);
        for _ in 0..3 {
            s.record(Verdict::Yes);
        }
        assert_eq!(s.level(), 2);
        s.record(Verdict::Unknown); // must not move anything
        assert_eq!(s.level(), 2);
    }

    #[test]
    fn two_right_answers_earn_one_play_page_then_practice_resumes() {
        let mut s = Session::start_at(1, 5);
        s.record(Verdict::Yes);
        s.next();
        assert!(matches!(s.page, Page::Practice(_)), "one right answer is not enough");
        s.record(Verdict::Yes);
        s.next();
        assert!(matches!(s.page, Page::Play(_)), "two right answers earn the treat");
        s.next();
        assert!(matches!(s.page, Page::Practice(_)), "leaving a play page returns to work");
        // Wrong answers never earn play pages.
        let mut s = Session::start_at(1, 5);
        for _ in 0..6 {
            s.record(Verdict::No);
            s.next();
        }
        assert!(matches!(s.page, Page::Practice(_)));
    }

    #[test]
    fn a_story_remembers_its_beats_and_pending_choice() {
        let mut s = Session::start_at(1, 5);
        s.page = Page::Play(games::Game::story());
        assert!(!s.needs_ink(), "an empty page may still start a story");
        s.story_advance("A potato woke up.".into(), vec!["RUN".into(), "NAP".into(), "SING".into()]);
        let label = s.choose(1).unwrap();
        assert_eq!(label, "NAP");
        assert!(s.instruction().contains("NAP"), "the ask must carry the choice");
        s.story_advance("It napped heroically.".into(), vec!["A".into(), "B".into(), "C".into()]);
        let h = s.story_history();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].0, "(the story begins)");
        assert_eq!(h[1].0, "I choose: NAP");
        assert!(s.choose(9).is_none());
    }
}
