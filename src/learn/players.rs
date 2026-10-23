//! Who is playing, and what they have earned.
//!
//! Players are animals (FOX, OWL, BEAR) unless `RIDDLE_LEARN_PLAYERS` names
//! them — nothing about the children is stored or shown. Each keeps a level,
//! lifetime stars, the highest level reached, and a best race time, in
//! `$RIDDLE_DATA_DIR/learn-players` on the tablet: plain `NAME.field=value`
//! lines, keyed by name so a renamed roster keeps whoever it still names.
//!
//! Stars are fair across ages on purpose. The session score pays more at
//! higher levels; stars do not — one per right answer, one more on a run of
//! three or more — so a younger sibling on level 1 can lead the board.

use std::path::PathBuf;

/// Stars to a trophy.
pub const STARS_PER_TROPHY: u32 = 25;

/// The most players the menu's PLAYER row draws.
pub const MAX_PLAYERS: usize = 4;

const DEFAULT_NAMES: [&str; 3] = ["FOX", "OWL", "BEAR"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub name: String,
    pub level: u8,
    pub stars: u32,
    pub best_level: u8,
    /// Fastest finished race, in milliseconds.
    pub best_race_ms: Option<u64>,
}

impl Player {
    fn new(name: &str) -> Self {
        Self { name: name.to_string(), level: 1, stars: 0, best_level: 1, best_race_ms: None }
    }

    pub fn trophies(&self) -> u32 {
        self.stars / STARS_PER_TROPHY
    }
}

/// What a right answer earned beyond its stars, for the cheer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Earned {
    pub stars: u32,
    pub trophy: bool,
}

pub struct Roster {
    pub players: Vec<Player>,
    pub current: usize,
    /// None in tests and previews: nothing is written.
    path: Option<PathBuf>,
}

impl Roster {
    /// The roster from config and the tablet's file.
    pub fn load() -> Self {
        let names = names_from(std::env::var("RIDDLE_LEARN_PLAYERS").ok().as_deref());
        let path = std::env::var("RIDDLE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/home/root/g-pad-data"))
            .join("learn-players");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut r = Self::from_text(&names, &text);
        r.path = Some(path);
        r
    }

    /// A roster that never touches the disk: the default animals, fresh.
    pub fn in_memory() -> Self {
        Self::from_text(&names_from(None), "")
    }

    fn from_text(names: &[String], text: &str) -> Self {
        let mut players: Vec<Player> = names.iter().map(|n| Player::new(n)).collect();
        let mut current_name = String::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if key == "current" {
                current_name = value.trim().to_string();
                continue;
            }
            let Some((name, field)) = key.rsplit_once('.') else { continue };
            let Some(p) = players.iter_mut().find(|p| p.name == name) else { continue };
            let value = value.trim();
            match field {
                "level" => p.level = value.parse::<u8>().unwrap_or(1).clamp(1, super::problems::MAX_LEVEL),
                "stars" => p.stars = value.parse().unwrap_or(0),
                "best_level" => p.best_level = value.parse::<u8>().unwrap_or(1).clamp(1, super::problems::MAX_LEVEL),
                "race_ms" => p.best_race_ms = value.parse().ok(),
                _ => {}
            }
        }
        let current = players.iter().position(|p| p.name == current_name).unwrap_or(0);
        Self { players, current, path: None }
    }

    fn to_text(&self) -> String {
        let mut out = format!("current={}\n", self.player().name);
        for p in &self.players {
            out.push_str(&format!(
                "{0}.level={1}\n{0}.stars={2}\n{0}.best_level={3}\n",
                p.name, p.level, p.stars, p.best_level
            ));
            if let Some(ms) = p.best_race_ms {
                out.push_str(&format!("{}.race_ms={ms}\n", p.name));
            }
        }
        out
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write then rename: a flat battery mid-write must not cost a child
        // their stars.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, self.to_text()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn player(&self) -> &Player {
        &self.players[self.current]
    }

    /// Switch to player `i`; their level comes with them. False when there
    /// is no such player.
    pub fn switch(&mut self, i: usize) -> bool {
        if i >= self.players.len() {
            return false;
        }
        self.current = i;
        self.save();
        true
    }

    /// A right answer, `streak` right before it in a row.
    pub fn award(&mut self, streak: u32) -> Earned {
        let stars = 1 + u32::from(streak >= 2);
        let p = &mut self.players[self.current];
        let before = p.trophies();
        p.stars += stars;
        let trophy = p.trophies() > before;
        self.save();
        Earned { stars, trophy }
    }

    /// The ladder moved (or was re-seated) to `level`. True when it is the
    /// highest this player has reached: a new badge.
    pub fn set_level(&mut self, level: u8) -> bool {
        let p = &mut self.players[self.current];
        p.level = level;
        let badge = level > p.best_level;
        if badge {
            p.best_level = level;
        }
        self.save();
        badge
    }

    /// Player indices by stars, most first: the leaderboard's order. Ties
    /// keep roster order.
    pub fn ranking(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.players.len()).collect();
        order.sort_by(|&a, &b| self.players[b].stars.cmp(&self.players[a].stars));
        order
    }
}

/// A race time as the board prints it: minutes and seconds, "1:05".
pub fn race_time(ms: u64) -> String {
    let secs = ms.div_ceil(1000);
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Names from `RIDDLE_LEARN_PLAYERS`: comma-separated, uppercased, at most
/// four, and never a character the file format uses. The animals when it is
/// unset or names nobody.
fn names_from(config: Option<&str>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for raw in config.unwrap_or("").split(',') {
        let name: String = raw
            .trim()
            .chars()
            .filter(|c| !matches!(c, '.' | '=' | '\n' | '\r'))
            .collect::<String>()
            .to_uppercase();
        if !name.is_empty() && !names.contains(&name) && names.len() < MAX_PLAYERS {
            names.push(name);
        }
    }
    if names.is_empty() {
        names = DEFAULT_NAMES.iter().map(|s| s.to_string()).collect();
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_default_to_animals_and_config_is_cleaned() {
        assert_eq!(names_from(None), vec!["FOX", "OWL", "BEAR"]);
        assert_eq!(names_from(Some(" ")), vec!["FOX", "OWL", "BEAR"]);
        assert_eq!(names_from(Some("ava, leo,ava,x.y=z,a,b,c")), vec!["AVA", "LEO", "XYZ", "A"]);
    }

    #[test]
    fn stars_are_fair_across_levels_and_make_trophies() {
        let mut r = Roster::in_memory();
        assert_eq!(r.award(0), Earned { stars: 1, trophy: false });
        assert_eq!(r.award(1).stars, 1);
        assert_eq!(r.award(2).stars, 2, "a run of three earns a bonus star");
        r.players[0].stars = STARS_PER_TROPHY - 1;
        assert!(r.award(0).trophy, "crossing the line wins a trophy");
        assert_eq!(r.player().trophies(), 1);
        assert!(!r.award(0).trophy, "and only once");
    }

    #[test]
    fn levels_and_badges_are_per_player() {
        let mut r = Roster::in_memory();
        assert!(r.set_level(3), "a first level 3 is a badge");
        assert!(!r.set_level(2) && !r.set_level(3), "going back down, or back to it, is not");
        assert!(r.switch(1));
        assert_eq!(r.player().level, 1, "OWL keeps OWL's level");
        assert!(!r.switch(9));
        assert_eq!((r.players[0].level, r.players[0].best_level), (3, 3), "FOX kept FOX's level");
    }

    #[test]
    fn the_file_round_trips_and_ignores_strangers() {
        let mut r = Roster::in_memory();
        r.set_level(4);
        r.award(0);
        r.switch(2);
        r.players[2].best_race_ms = Some(42_000);
        let text = r.to_text() + "GHOST.stars=999\njunk\nFOX.level=99\n";
        let back = Roster::from_text(&names_from(None), &text);
        assert_eq!(back.current, 2);
        assert_eq!(back.players[0].level, super::super::problems::MAX_LEVEL, "levels clamp");
        assert_eq!(back.players[0].stars, 1);
        assert_eq!(back.players[0].best_level, 4);
        assert_eq!(back.players[2].best_race_ms, Some(42_000));
        assert!(back.players.iter().all(|p| p.name != "GHOST"));
    }

    #[test]
    fn race_times_read_as_minutes_and_seconds() {
        assert_eq!(race_time(53_000), "0:53");
        assert_eq!(race_time(65_400), "1:06");
        assert_eq!(race_time(1), "0:01");
    }

    #[test]
    fn the_board_ranks_by_stars() {
        let mut r = Roster::in_memory();
        r.players[0].stars = 5;
        r.players[1].stars = 12;
        r.players[2].stars = 5;
        assert_eq!(r.ranking(), vec![1, 0, 2]);
    }

    #[test]
    fn saving_writes_a_file_that_loads_back() {
        let dir = std::env::temp_dir().join(format!("gpad-players-{}", std::process::id()));
        let path = dir.join("learn-players");
        let mut r = Roster::in_memory();
        r.path = Some(path.clone());
        r.award(0);
        r.switch(1);
        let back = Roster::from_text(&names_from(None), &std::fs::read_to_string(&path).unwrap());
        assert_eq!((back.current, back.players[0].stars), (1, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
