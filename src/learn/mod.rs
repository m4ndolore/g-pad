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
    Practice(problems::Set),
    Play(games::Game),
    /// The picker: every topic and game as a tick box. Dealt by a mark in
    /// the MENU footer box; a mark in a choice box deals the chosen page.
    Menu,
    /// The skills picker: every math activity as its own tick box, reached
    /// from the menu's MATH SKILLS entry. A mark latches that one skill.
    Skills,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayPolicy {
    Earned,
    Always,
    Never,
}

/// What the menu latched: practice within a topic (the default is the full
/// mix), or one game dealt again on every NEW. Sticky until the menu says
/// otherwise — "story mode" stays story mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Practice(problems::Topic),
    Game(usize),
}

pub struct Session {
    ladder: problems::Ladder,
    rng: problems::Rng,
    rot: usize,
    game_rot: usize,
    policy: PlayPolicy,
    focus: Focus,
    earned: u32,
    /// Session points, math only: correct answers score, streaks score more.
    /// Resets every boot — the game is today's game.
    score: u32,
    /// What the last correct answer was worth, for the "+N" beside the cheer.
    last_award: u32,
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
            Page::Practice(problems::generate_set(ladder.level, problems::Topic::Mix, 0, &mut rng))
        };
        let game_rot = usize::from(policy == PlayPolicy::Always);
        let focus = Focus::Practice(problems::Topic::Mix);
        Self {
            ladder,
            rng,
            rot: 0,
            game_rot,
            policy,
            focus,
            earned: 0,
            score: 0,
            last_award: 0,
            page,
            hits: empty_hits(),
        }
    }

    pub fn level(&self) -> u8 {
        self.ladder.level
    }

    /// What the latest correct answer scored; 0 after anything else.
    pub fn last_award(&self) -> u32 {
        self.last_award
    }

    /// The graded problem of a practice page: the last on the sheet.
    pub fn as_practice(&self) -> Option<&problems::Problem> {
        match &self.page {
            Page::Practice(set) => Some(set.graded()),
            _ => None,
        }
    }

    /// Draw the current page and remember its hit map.
    pub fn draw(&mut self, surf: &mut Surface, ui_font: &FontRef) {
        self.hits = match &self.page {
            Page::Practice(set) => {
                sheet::draw(surf, ui_font, set, self.ladder.level, self.ladder.streak(), self.score)
            }
            Page::Play(g) => games::draw(surf, ui_font, g),
            Page::Menu => sheet::draw_menu(surf, ui_font, self.ladder.level),
            Page::Skills => sheet::draw_skills(surf, ui_font),
        };
    }

    /// Deal the next page (call `draw` afterwards). A game focus deals the
    /// same game afresh. Under a practice focus a play page is dealt when
    /// earned (or always, under that policy); leaving a play page under the
    /// earned policy returns to practice — the next treat is earned again.
    /// Earned treats interrupt only the default mix: a menu-chosen topic was
    /// asked for by name, and its pages keep coming until the menu says
    /// otherwise — "math mode" stays math mode.
    pub fn next(&mut self) {
        let topic = match self.focus {
            Focus::Game(g) => {
                self.page = Page::Play(games::Game::nth(g));
                return;
            }
            Focus::Practice(t) => t,
        };
        let deal_play = match self.policy {
            PlayPolicy::Always => true,
            PlayPolicy::Never => false,
            PlayPolicy::Earned => {
                topic == problems::Topic::Mix
                    && self.earned >= PLAY_EVERY
                    && matches!(self.page, Page::Practice(_))
            }
        };
        if deal_play {
            self.earned = 0;
            self.page = Page::Play(games::Game::nth(self.game_rot));
            self.game_rot += 1;
        } else {
            self.rot += 1;
            self.page = Page::Practice(problems::generate_set(self.ladder.level, topic, self.rot, &mut self.rng));
        }
    }

    /// Put the picker on the page (call `draw` afterwards).
    pub fn open_menu(&mut self) {
        self.page = Page::Menu;
    }

    /// Is a picker page up? Both the menu and the skills page route their
    /// choice marks through `choose_menu`.
    pub fn is_menu(&self) -> bool {
        matches!(self.page, Page::Menu | Page::Skills)
    }

    /// The child ticked picker box `i`: act on what it names and deal the
    /// page that follows (call `draw` afterwards). False when no such box
    /// exists. On the menu the indices mirror `sheet::MENU_ITEMS`, with the
    /// four LEVEL boxes after them; a level pick re-seats the ladder and
    /// stays on the menu so a topic can still be chosen. On the skills page
    /// the indices mirror `problems::MATH_SKILLS`.
    pub fn choose_menu(&mut self, i: usize) -> bool {
        self.focus = if self.page == Page::Skills {
            match problems::MATH_SKILLS.get(i) {
                Some(&act) => Focus::Practice(problems::Topic::Skill(act)),
                None => return false,
            }
        } else {
            match i {
                0 => Focus::Practice(problems::Topic::Math),
                1 => Focus::Practice(problems::Topic::Writing),
                2 => Focus::Practice(problems::Topic::Mix),
                3 => {
                    self.page = Page::Skills;
                    return true;
                }
                4..=6 => Focus::Game(i - 4),
                7..=10 => {
                    // A LEVEL box: re-seat the adaptive ladder there (it
                    // still walks up and down from the new rung) and stay
                    // on the menu, redrawn with the pick filled in.
                    self.ladder = problems::Ladder::new((i - 6) as u8);
                    return true;
                }
                _ => return false,
            }
        };
        // The picker never earns a treat: deal the pick itself, not a play page.
        self.earned = 0;
        match self.focus {
            Focus::Game(g) => self.page = Page::Play(games::Game::nth(g)),
            Focus::Practice(topic) => {
                self.rot += 1;
                self.page = Page::Practice(problems::generate_set(self.ladder.level, topic, self.rot, &mut self.rng));
            }
        }
        true
    }

    /// A marked practice answer came back; move the ladder, the treat
    /// counter, and the score. Play pages never call this.
    ///
    /// Scoring: a correct answer is worth `10 × level`, plus `5` per streak
    /// dot already showing (capped at four) — harder pages and runs both pay
    /// more, and the award is settled BEFORE the ladder moves so a level-up
    /// never inflates the answer that caused it.
    pub fn record(&mut self, v: Verdict) {
        if let Some(correct) = v.counts_as_correct() {
            if correct {
                self.last_award = 10 * self.ladder.level as u32 + 5 * self.ladder.streak().min(4);
                self.score += self.last_award;
                self.earned += 1;
            } else {
                self.last_award = 0;
            }
            self.ladder.record(correct);
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

    /// The pad drew its turn on the critter picture: count it, so the next
    /// instruction can tell the model some strokes are its own.
    pub fn critter_turn(&mut self) {
        if let Page::Play(games::Game::Critter { round }) = &mut self.page {
            *round = round.saturating_add(1);
        }
    }

    /// Does DONE require ink in the answer region on this page? Everywhere
    /// but the story, where an empty page just means the pad invents a hero.
    /// (The picker pages have no DONE box at all.)
    pub fn needs_ink(&self) -> bool {
        !matches!(
            self.page,
            Page::Play(games::Game::Story { .. }) | Page::Menu | Page::Skills
        )
    }

    /// The per-turn instruction for the oracle: tutor on practice pages,
    /// playmate on play pages. Travels as user text, so it works unchanged on
    /// both oracle backends and never disturbs the pad's persona.
    pub fn instruction(&self) -> String {
        match &self.page {
            Page::Practice(set) => format!(
                "LEARN MODE. You are a warm, playful tutor for a child aged four to eight \
                 who can barely read. The page shows only the child's pen work for this \
                 exercise: {brief}. \
                 Decide whether the child's writing shows the correct answer. \
                 Your reply MUST start with exactly one word: YES if it is right, \
                 ALMOST if it is nearly right (a reversed digit, shaky but readable letters), \
                 NO if it is wrong, missing, or unreadable. \
                 After the verdict word, on the same line, write ONE to THREE cheering \
                 words in CAPITAL LETTERS that a four-year-old can read alone — like \
                 GREAT JOB, WOW, SUPER STAR, SO CLOSE, TRY AGAIN. \
                 After ALMOST or NO only, add ONE more line: a hint of at most ten very \
                 simple words for a grown-up to read aloud — never the answer itself. \
                 Write nothing else: no sentences after YES, no questions, no greetings. \
                 Never mention pictures, images, or cameras.",
                brief = set.graded().brief()
            ),
            Page::Play(game) => {
                let pending = match game {
                    games::Game::Story { pending, .. } => pending.as_deref(),
                    _ => None,
                };
                game.instruction(pending)
            }
            // The picker pages have no DONE box; nothing is ever asked from them.
            Page::Menu | Page::Skills => String::new(),
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
    HitMap {
        answer: BBox::empty(),
        done: BBox::empty(),
        new: BBox::empty(),
        menu: BBox::empty(),
        choices: Vec::new(),
        blanks: Vec::new(),
    }
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
        assert_ne!(std::mem::discriminant(&a.graded().kind), std::mem::discriminant(&b.graded().kind));
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
    fn the_menu_latches_topics_and_games_on_demand() {
        let mut s = Session::start_at(1, 5);
        s.open_menu();
        assert!(s.is_menu());
        assert!(!s.needs_ink(), "the menu wants a mark, never ink");
        // Picking math deals math, and it sticks: no handwriting pages
        // sneak into the rotation.
        assert!(s.choose_menu(0));
        for _ in 0..6 {
            let Page::Practice(ref p) = s.page else { panic!("math focus must deal practice") };
            assert!(!p.items.iter().any(|q| matches!(q.kind, problems::Kind::Trace { .. })));
            s.next();
        }
        // Writing deals tracing, and only tracing.
        s.open_menu();
        assert!(s.choose_menu(1));
        for _ in 0..3 {
            let Page::Practice(ref p) = s.page else { panic!("writing focus must deal practice") };
            assert!(p.items.iter().all(|q| matches!(q.kind, problems::Kind::Trace { .. })));
            s.next();
        }
        // A game is sticky: NEW deals the same game afresh.
        s.open_menu();
        assert!(s.choose_menu(6));
        assert!(matches!(s.page, Page::Play(games::Game::Story { .. })));
        s.next();
        assert!(matches!(s.page, Page::Play(games::Game::Story { .. })));
        // Surprise mix restores the default deck; nonsense selects nothing.
        s.open_menu();
        assert!(s.choose_menu(2));
        assert!(matches!(s.page, Page::Practice(_)));
        assert!(!s.choose_menu(99));
    }

    #[test]
    fn a_level_box_reseats_the_ladder_and_keeps_the_menu_open() {
        let mut s = Session::start_at(1, 5);
        s.open_menu();
        // LEVEL boxes sit after the named entries: 7..=10 are levels 1..=4.
        assert!(s.choose_menu(9));
        assert_eq!(s.level(), 3);
        assert!(s.is_menu(), "a level pick leaves the menu open for a topic pick");
        assert!(s.choose_menu(0));
        let Page::Practice(_) = s.page else { panic!("the topic pick still deals") };
        assert_eq!(s.level(), 3, "the dealt page is at the picked level");
        // The ladder still walks from the new rung.
        for _ in 0..3 {
            s.record(Verdict::Yes);
        }
        assert_eq!(s.level(), 4);
    }

    #[test]
    fn the_skills_page_latches_one_math_skill_until_the_menu_says_otherwise() {
        let mut s = Session::start_at(2, 5);
        s.open_menu();
        assert!(s.choose_menu(3), "MATH SKILLS opens the skills page");
        assert_eq!(s.page, Page::Skills);
        assert!(s.is_menu(), "the skills page routes marks like the menu");
        assert!(!s.needs_ink());
        // Pick bar models: every dealt page is bar models, even across treats.
        let bar = problems::MATH_SKILLS.iter().position(|a| *a == problems::Activity::Bar).unwrap();
        assert!(s.choose_menu(bar));
        for _ in 0..6 {
            s.record(Verdict::Yes);
            let Page::Practice(ref p) = s.page else { panic!("a skill latch must deal practice") };
            assert!(
                p.items.iter().all(|q| matches!(q.kind, problems::Kind::Bar { .. })),
                "the latched skill drifted: {:?}",
                p.items[0].kind
            );
            s.next();
        }
        // A nonsense skill index selects nothing.
        s.open_menu();
        assert!(s.choose_menu(3));
        assert!(!s.choose_menu(problems::MATH_SKILLS.len()));
    }

    #[test]
    fn correct_answers_score_and_streaks_score_more() {
        let mut s = Session::start_at(1, 5);
        assert_eq!(s.score, 0);
        s.record(Verdict::Yes); // level 1, no streak yet: 10
        assert_eq!(s.last_award(), 10);
        s.record(Verdict::Yes); // one streak dot showing: 10 + 5
        assert_eq!(s.last_award(), 15);
        assert_eq!(s.score, 25);
        // A miss awards nothing and clears the toast, but keeps the score.
        s.record(Verdict::No);
        assert_eq!(s.last_award(), 0);
        assert_eq!(s.score, 25);
        // Unknown moves nothing at all.
        s.record(Verdict::Unknown);
        assert_eq!(s.score, 25);
        // Higher levels pay more: at level 3 a cold answer is worth 30.
        let mut s = Session::start_at(3, 5);
        s.record(Verdict::Yes);
        assert_eq!(s.last_award(), 30);
        // ALMOST is a miss for scoring, same as the ladder.
        s.record(Verdict::Almost);
        assert_eq!(s.last_award(), 0);
    }

    #[test]
    fn the_streak_bonus_is_settled_before_the_ladder_moves() {
        let mut s = Session::start_at(1, 5);
        s.record(Verdict::Yes); // 10, streak -> 1
        s.record(Verdict::Yes); // 15, streak -> 2
        s.record(Verdict::Yes); // 20, then the level-up resets the streak
        assert_eq!(s.last_award(), 20, "the level-up must not inflate its own answer");
        assert_eq!(s.level(), 2);
        s.record(Verdict::Yes); // level 2, streak reset: 20
        assert_eq!(s.last_award(), 20);
        assert_eq!(s.score, 10 + 15 + 20 + 20);
    }

    #[test]
    fn a_chosen_topic_is_never_interrupted_by_treat_pages() {
        // The bug this pins down: pick MATH from the menu, answer two
        // questions right, and the earned-treat counter dealt a game page —
        // the category "drifted". A named topic must hold its ground.
        let mut s = Session::start_at(1, 5);
        s.open_menu();
        assert!(s.choose_menu(0));
        for _ in 0..6 {
            s.record(Verdict::Yes);
            s.next();
            assert!(
                matches!(s.page, Page::Practice(_)),
                "a menu-chosen topic must never deal a play page"
            );
        }
        // The default mix still pays out the treat as before.
        s.open_menu();
        assert!(s.choose_menu(2));
        s.record(Verdict::Yes);
        s.next();
        s.record(Verdict::Yes);
        s.next();
        assert!(matches!(s.page, Page::Play(_)), "the mix still earns treats");
    }

    #[test]
    fn every_menu_box_maps_to_a_selection() {
        // The named entries, then the four LEVEL boxes after them.
        for i in 0..sheet::MENU_ITEMS.len() + 4 {
            let mut s = Session::start_at(1, 5);
            s.open_menu();
            assert!(s.choose_menu(i), "menu box {i} must select something");
            // MATH SKILLS and the level boxes keep a picker up; every other
            // pick deals a page and leaves.
            let stays = i == 3 || i >= sheet::MENU_ITEMS.len();
            assert_eq!(s.is_menu(), stays, "menu box {i} left the wrong page up");
        }
        // And every skills box latches a skill and leaves.
        for i in 0..problems::MATH_SKILLS.len() {
            let mut s = Session::start_at(1, 5);
            s.open_menu();
            assert!(s.choose_menu(3));
            assert!(s.choose_menu(i), "skill box {i} must select something");
            assert!(!s.is_menu(), "a skill pick must deal a page");
        }
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
