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
//!
//! The menu (`MENU`) is the one table of what kids mode offers: practice
//! topics, the skills picker, chess, and the games. Chess is the one entry
//! that leaves the sheet: it opens the chess board over the page (`Page::Chess`),
//! and the board's MENU button comes back here.

pub mod games;
pub mod players;
pub mod problems;
pub mod sheet;
pub mod verdict;

use ab_glyph::FontRef;

use crate::surface::Surface;

pub use sheet::{HitMap, Target};
pub use verdict::Verdict;

/// Correct practice answers that earn one play page.
const PLAY_EVERY: u32 = 2;

/// What a right tick says, in turn: a tick box has no tutor to vary it.
const LOCAL_CHEERS: &[&str] = &["GREAT JOB!", "WOW!", "SUPER STAR!", "YOU GOT IT!", "AMAZING!"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Practice(problems::Set),
    Play(games::Game),
    /// The picker: every topic and game as a tick box. Dealt by a mark in
    /// the MENU footer box; a mark in a choice box deals the chosen page.
    Menu,
    /// The skills picker: every math activity as its own tick box, grouped
    /// by strand, reached from the menu's MATH SKILLS entry. A mark latches
    /// that one skill; the footer's MENU box goes back to the menu.
    Skills,
    /// The chess board is up over the page. The board owns input until its
    /// MENU button closes it; drawing this page draws the menu.
    Chess,
    /// The leaderboard: every player's stars, trophies, best level and race.
    /// Its MENU box goes back to the menu.
    Trophies,
}

/// What marking a named menu entry does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Practice within a topic until the menu says otherwise.
    Topic(problems::Topic),
    /// Open the skills picker.
    Skills,
    /// Open the kids' chess board.
    Chess,
    /// Open the leaderboard.
    Trophies,
    /// Deal this game (by `games::Game::nth` index) on every NEW.
    Game(usize),
}

/// The menu's named entries, section by section, in choice-index order; the
/// PLAYER boxes follow them, then the LEVEL boxes. `sheet::draw_menu` draws this table and
/// `Session::choose_menu` reads it, so a box and its meaning cannot drift.
pub const MENU: &[(&str, &[(&str, Pick)])] = &[
    (
        "PRACTICE",
        &[
            ("MATH", Pick::Topic(problems::Topic::Math)),
            ("MATH SKILLS", Pick::Skills),
            ("WRITING", Pick::Topic(problems::Topic::Writing)),
            ("WRITE NUMBERS", Pick::Topic(problems::Topic::Numbers)),
            ("SHAPES", Pick::Topic(problems::Topic::Shapes)),
            ("SURPRISE MIX", Pick::Topic(problems::Topic::Mix)),
        ],
    ),
    (
        "GAMES",
        &[
            ("TROPHIES", Pick::Trophies),
            ("CHESS", Pick::Chess),
            ("DOODLE CRITTER", Pick::Game(0)),
            ("GUESSING GAME", Pick::Game(1)),
            ("STORY TIME", Pick::Game(2)),
        ],
    ),
];

/// The levels the menu's LEVEL row offers, after the named entries.
pub const MENU_LEVELS: u8 = problems::MAX_LEVEL;

fn menu_entries() -> impl Iterator<Item = &'static (&'static str, Pick)> {
    MENU.iter().flat_map(|(_, items)| items.iter())
}

/// The choice index of a named menu entry.
pub fn menu_index(pick: Pick) -> Option<usize> {
    menu_entries().position(|&(_, p)| p == pick)
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
    /// Which local cheer a right tick says next.
    cheer_rot: usize,
    /// Who is playing, and what everyone has earned.
    roster: players::Roster,
    /// What the latest answer won beyond its points, for the cheer.
    celebration: Option<&'static str>,
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
        // A player who has played keeps their own level; RIDDLE_LEARN_LEVEL
        // only seeds one who has not.
        let roster = players::Roster::load();
        let p = roster.player();
        let level = if p.stars > 0 || p.level > 1 { p.level } else { level };
        Self::start_with(level, now_seed(), policy, roster)
    }

    /// A session that never touches the disk: tests and previews.
    pub fn start_at(level: u8, seed: u32) -> Self {
        Self::start_with(level, seed, PlayPolicy::Earned, players::Roster::in_memory())
    }

    fn start_with(level: u8, seed: u32, policy: PlayPolicy, roster: players::Roster) -> Self {
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
            cheer_rot: 0,
            roster,
            celebration: None,
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

    #[cfg(test)]
    pub fn roster(&self) -> &players::Roster {
        &self.roster
    }

    /// What the latest right answer won beyond its points — NEW TROPHY! or
    /// LEVEL UP! — taken once, for the cheer.
    pub fn take_celebration(&mut self) -> Option<&'static str> {
        self.celebration.take()
    }

    /// The graded problem of a practice page: the last on the sheet.
    pub fn as_practice(&self) -> Option<&problems::Problem> {
        match &self.page {
            Page::Practice(set) => Some(set.graded()),
            _ => None,
        }
    }

    /// After a correct answer on a bond chain, move to its next blank. False
    /// on any other page, or when the blank just answered was the chain's last.
    pub fn advance_chain(&mut self) -> bool {
        let Page::Practice(set) = &mut self.page else { return false };
        match set.items.last_mut().map(|p| &mut p.kind) {
            Some(problems::Kind::BondChain(chain)) => chain.advance(),
            _ => false,
        }
    }

    /// Draw the current page and remember its hit map.
    pub fn draw(&mut self, surf: &mut Surface, ui_font: &FontRef) {
        self.hits = match &self.page {
            Page::Practice(set) => {
                let header = sheet::Header {
                    player: &self.roster.player().name,
                    level: self.ladder.level,
                    streak: self.ladder.streak(),
                    score: self.score,
                };
                sheet::draw(surf, ui_font, set, &header)
            }
            Page::Play(g) => games::draw(surf, ui_font, g),
            Page::Menu | Page::Chess => {
                self.page = Page::Menu;
                self.redraw_picker(surf, ui_font)
            }
            Page::Skills => sheet::draw_skills(surf, ui_font),
            Page::Trophies => sheet::draw_trophies(surf, ui_font, &self.roster),
        };
    }

    /// Repaint the picker under a mark that was just absorbed, and return
    /// its hit map. The skills page and the menu are the only pickers.
    pub fn redraw_picker(&self, surf: &mut Surface, ui_font: &FontRef) -> HitMap {
        match self.page {
            Page::Skills => sheet::draw_skills(surf, ui_font),
            _ => {
                let names: Vec<&str> = self.roster.players.iter().map(|p| p.name.as_str()).collect();
                sheet::draw_menu(surf, ui_font, self.ladder.level, &names, self.roster.current)
            }
        }
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
    /// exists. On the menu the indices follow `MENU`, with the LEVEL boxes
    /// after them; a level pick re-seats the ladder and stays on the menu so
    /// a topic can still be chosen, and CHESS leaves `Page::Chess` up for the
    /// caller to open the board. On the skills page the indices follow
    /// `problems::math_skills()`.
    pub fn choose_menu(&mut self, i: usize) -> bool {
        self.focus = if self.page == Page::Skills {
            match problems::math_skills().get(i) {
                Some(&act) => Focus::Practice(problems::Topic::Skill(act)),
                None => return false,
            }
        } else {
            match menu_entries().nth(i).map(|&(_, p)| p) {
                Some(Pick::Topic(t)) => Focus::Practice(t),
                Some(Pick::Game(g)) => Focus::Game(g),
                Some(Pick::Skills) => {
                    self.page = Page::Skills;
                    return true;
                }
                Some(Pick::Chess) => {
                    self.page = Page::Chess;
                    return true;
                }
                Some(Pick::Trophies) => {
                    self.page = Page::Trophies;
                    return true;
                }
                None => {
                    // A PLAYER box: switch to them and seat the ladder on
                    // their level. A LEVEL box after those: re-seat the
                    // ladder there (it still walks up and down from the new
                    // rung). Either way the menu stays up, redrawn with the
                    // pick filled in, so a topic can still be chosen.
                    let k = i - menu_entries().count();
                    let n = self.roster.players.len();
                    if k < n {
                        self.roster.switch(k);
                        self.ladder = problems::Ladder::new(self.roster.player().level);
                        return true;
                    }
                    let k = k - n;
                    if k >= MENU_LEVELS as usize {
                        return false;
                    }
                    self.ladder = problems::Ladder::new(k as u8 + 1);
                    self.roster.set_level(self.ladder.level);
                    return true;
                }
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
            self.celebration = None;
            if correct {
                self.last_award = 10 * self.ladder.level as u32 + 5 * self.ladder.streak().min(4);
                self.score += self.last_award;
                self.earned += 1;
                if self.roster.award(self.ladder.streak()).trophy {
                    self.celebration = Some("NEW TROPHY!");
                }
            } else {
                self.last_award = 0;
            }
            let before = self.ladder.level;
            self.ladder.record(correct);
            if self.ladder.level != before {
                let badge = self.roster.set_level(self.ladder.level);
                if badge && self.celebration.is_none() {
                    self.celebration = Some("LEVEL UP!");
                }
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

    /// The pad drew its turn on the critter picture: count it, so the next
    /// instruction can tell the model some strokes are its own.
    pub fn critter_turn(&mut self) {
        if let Page::Play(games::Game::Critter { round }) = &mut self.page {
            *round = round.saturating_add(1);
        }
    }

    /// Does DONE require ink in the answer region on this page? Everywhere
    /// but the story, where an empty page just means the pad invents a hero.
    /// (The picker and tick-box pages have no DONE box at all.)
    pub fn needs_ink(&self) -> bool {
        !self.is_choice_page()
            && !matches!(
                self.page,
                Page::Play(games::Game::Story { .. }) | Page::Menu | Page::Skills | Page::Chess | Page::Trophies
            )
    }

    /// Is this a practice page answered by ticking a box? Its ticks are
    /// marked here, on the tablet, and never reach the tutor.
    pub fn is_choice_page(&self) -> bool {
        self.as_practice().is_some_and(|p| p.choice_answer().is_some())
    }

    /// The child ticked box `i` on a tick-box page: the tutor-shaped reply
    /// for it (`YES` and a cheer, or `NO`, a cheer, and the grown-up's
    /// hint), so the verdict travels the same road an oracle reply does.
    /// None when this is not a tick-box page or there is no such box.
    pub fn answer_choice(&mut self, i: usize) -> Option<String> {
        let p = self.as_practice()?;
        let right = p.choice_answer()?;
        if i >= 3 {
            return None;
        }
        if i == right {
            self.cheer_rot += 1;
            Some(format!("YES {}", LOCAL_CHEERS[self.cheer_rot % LOCAL_CHEERS.len()]))
        } else {
            Some(format!("NO TRY AGAIN!\n{}", p.choice_hint()))
        }
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
            // The picker pages, the board, and the leaderboard have no DONE
            // box; nothing is ever asked from them.
            Page::Menu | Page::Skills | Page::Chess | Page::Trophies => String::new(),
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
    HitMap::choices_only(Vec::new())
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

    fn pick(p: Pick) -> usize {
        menu_index(p).expect("on the menu")
    }

    const MATH: Pick = Pick::Topic(problems::Topic::Math);
    const WRITING: Pick = Pick::Topic(problems::Topic::Writing);
    const MIX: Pick = Pick::Topic(problems::Topic::Mix);

    fn player_box(i: usize) -> usize {
        menu_entries().count() + i
    }

    fn level_box(level: u8) -> usize {
        menu_entries().count() + players::Roster::in_memory().players.len() + level as usize - 1
    }

    #[test]
    fn the_menu_latches_topics_and_games_on_demand() {
        let mut s = Session::start_at(1, 5);
        s.open_menu();
        assert!(s.is_menu());
        assert!(!s.needs_ink(), "the menu wants a mark, never ink");
        // Picking math deals math, and it sticks: no handwriting pages
        // sneak into the rotation.
        assert!(s.choose_menu(pick(MATH)));
        for _ in 0..6 {
            let Page::Practice(ref p) = s.page else { panic!("math focus must deal practice") };
            assert!(!p.items.iter().any(|q| matches!(q.kind, problems::Kind::Trace { .. })));
            s.next();
        }
        // Writing deals tracing, and only tracing.
        s.open_menu();
        assert!(s.choose_menu(pick(WRITING)));
        for _ in 0..3 {
            let Page::Practice(ref p) = s.page else { panic!("writing focus must deal practice") };
            assert!(p.items.iter().all(|q| matches!(q.kind, problems::Kind::Trace { .. })));
            s.next();
        }
        // A game is sticky: NEW deals the same game afresh.
        s.open_menu();
        assert!(s.choose_menu(pick(Pick::Game(2))));
        assert!(matches!(s.page, Page::Play(games::Game::Story { .. })));
        s.next();
        assert!(matches!(s.page, Page::Play(games::Game::Story { .. })));
        // Surprise mix restores the default deck; nonsense selects nothing.
        s.open_menu();
        assert!(s.choose_menu(pick(MIX)));
        assert!(matches!(s.page, Page::Practice(_)));
        s.open_menu();
        assert!(!s.choose_menu(level_box(MENU_LEVELS + 1)));
    }

    #[test]
    fn a_level_box_reseats_the_ladder_and_keeps_the_menu_open() {
        let mut s = Session::start_at(1, 5);
        s.open_menu();
        // LEVEL boxes sit after the named entries.
        assert!(s.choose_menu(level_box(3)));
        assert_eq!(s.level(), 3);
        assert!(s.is_menu(), "a level pick leaves the menu open for a topic pick");
        assert!(s.choose_menu(pick(MATH)));
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
        assert!(s.choose_menu(pick(Pick::Skills)), "MATH SKILLS opens the skills page");
        assert_eq!(s.page, Page::Skills);
        assert!(s.is_menu(), "the skills page routes marks like the menu");
        assert!(!s.needs_ink());
        // Pick bar models: every dealt page is bar models, even across treats.
        let bar = problems::math_skills().iter().position(|a| *a == problems::Activity::Bar).unwrap();
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
        assert!(s.choose_menu(pick(Pick::Skills)));
        assert!(!s.choose_menu(problems::math_skills().len()));
    }

    #[test]
    fn chess_opens_over_the_menu_and_keeps_the_focus() {
        let mut s = Session::start_at(2, 5);
        s.open_menu();
        assert!(s.choose_menu(pick(WRITING)));
        s.open_menu();
        assert!(s.choose_menu(pick(Pick::Chess)));
        assert_eq!(s.page, Page::Chess, "the caller opens the board");
        assert!(!s.needs_ink() && !s.is_menu());
        assert!(s.instruction().is_empty(), "nothing is asked from the board");
        // Coming back deals the writing the child was on.
        s.next();
        let Page::Practice(ref p) = s.page else { panic!("practice resumes") };
        assert!(p.items.iter().all(|q| matches!(q.kind, problems::Kind::Trace { .. })));
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
        assert!(s.choose_menu(pick(MATH)));
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
        assert!(s.choose_menu(pick(MIX)));
        s.record(Verdict::Yes);
        s.next();
        s.record(Verdict::Yes);
        s.next();
        assert!(matches!(s.page, Page::Play(_)), "the mix still earns treats");
    }

    #[test]
    fn every_menu_box_maps_to_a_selection() {
        // The named entries, then the PLAYER boxes, then the LEVEL boxes.
        let named = menu_entries().count();
        let players = players::Roster::in_memory().players.len();
        for i in 0..named + players + MENU_LEVELS as usize {
            let mut s = Session::start_at(1, 5);
            s.open_menu();
            assert!(s.choose_menu(i), "menu box {i} must select something");
            // MATH SKILLS and the player and level boxes keep a picker up,
            // CHESS leaves the board's marker, TROPHIES the leaderboard, and
            // every other pick deals a page.
            let stays = i == pick(Pick::Skills) || i >= named;
            assert_eq!(s.is_menu(), stays, "menu box {i} left the wrong page up");
            assert_eq!(s.page == Page::Chess, i == pick(Pick::Chess));
            assert_eq!(s.page == Page::Trophies, i == pick(Pick::Trophies));
        }
        // And every skills box latches a skill and leaves.
        for i in 0..problems::math_skills().len() {
            let mut s = Session::start_at(1, 5);
            s.open_menu();
            assert!(s.choose_menu(pick(Pick::Skills)));
            assert!(s.choose_menu(i), "skill box {i} must select something");
            assert!(!s.is_menu(), "a skill pick must deal a page");
        }
    }

    #[test]
    fn each_player_keeps_their_own_level_and_stars() {
        let mut s = Session::start_at(1, 5);
        s.open_menu();
        // FOX climbs to level 2.
        for _ in 0..3 {
            s.record(Verdict::Yes);
        }
        assert_eq!(s.level(), 2);
        assert_eq!(s.roster().players[0].stars, 1 + 1 + 2);
        // OWL starts at their own level 1, and earns their own stars.
        assert!(s.choose_menu(player_box(1)));
        assert!(s.is_menu(), "a player pick keeps the menu up");
        assert_eq!((s.roster().current, s.level()), (1, 1));
        s.record(Verdict::Yes);
        assert_eq!(s.roster().players[1].stars, 1);
        // Back to FOX: level 2 again.
        assert!(s.choose_menu(player_box(0)));
        assert_eq!(s.level(), 2);
        // A level box sets the current player's level.
        assert!(s.choose_menu(level_box(4)));
        assert_eq!(s.roster().player().level, 4);
    }

    #[test]
    fn a_trophy_or_a_new_level_changes_the_cheer_once() {
        let mut s = Session::start_at(1, 5);
        s.record(Verdict::Yes);
        assert_eq!(s.take_celebration(), None);
        s.record(Verdict::Yes);
        s.record(Verdict::Yes); // the third right answer climbs to level 2
        assert_eq!(s.take_celebration(), Some("LEVEL UP!"));
        assert_eq!(s.take_celebration(), None, "a celebration is said once");
        // Falling back and climbing again to a level already reached is no
        // new badge.
        s.record(Verdict::No);
        s.record(Verdict::No);
        assert_eq!(s.level(), 1);
        for _ in 0..3 {
            s.record(Verdict::Yes);
        }
        assert_eq!(s.level(), 2);
        assert_ne!(s.take_celebration(), Some("LEVEL UP!"));
        // Enough stars win a trophy.
        while s.roster().player().stars + 2 < players::STARS_PER_TROPHY {
            s.record(Verdict::Yes);
            s.take_celebration();
        }
        let mut won = false;
        for _ in 0..3 {
            s.record(Verdict::Yes);
            won |= s.take_celebration() == Some("NEW TROPHY!");
        }
        assert!(won);
    }

    #[test]
    fn a_tick_box_page_is_marked_here_and_reads_like_the_tutor() {
        let mut s = Session::start_at(2, 5);
        s.open_menu();
        assert!(s.choose_menu(pick(Pick::Skills)));
        let name = problems::math_skills().iter().position(|a| *a == problems::Activity::NameShape).unwrap();
        assert!(s.choose_menu(name));
        assert!(s.is_choice_page());
        assert!(!s.needs_ink(), "a tick needs no ink in a blank");
        let right = s.as_practice().unwrap().choice_answer().unwrap();
        let wrong = (right + 1) % 3;
        let (v, f) = verdict::parse(&s.answer_choice(wrong).unwrap());
        assert_eq!(v, Verdict::No);
        assert!(!f.hint.is_empty(), "a wrong tick tells the grown-up how to look again");
        let (v, f) = verdict::parse(&s.answer_choice(right).unwrap());
        assert_eq!(v, Verdict::Yes);
        assert!(f.cheer.split_whitespace().count() <= 3);
        assert!(s.answer_choice(3).is_none(), "there is no fourth box");
        // Written answers are not tick boxes.
        let mut s = Session::start_at(2, 5);
        s.open_menu();
        assert!(s.choose_menu(pick(MATH)));
        assert!(!s.is_choice_page());
        assert!(s.answer_choice(0).is_none());
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
