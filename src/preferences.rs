//! Non-secret pad preferences, stored separately from oracle credentials.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stealth,
    Guided,
}

/// Which page the pad opens as: the writing pad, or the kids' tutor.
/// "Anthink remembers the last mode per boot" — so it persists here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Pad,
    Learn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub mode: Mode,
    pub idle_send_ms: u64,
    pub page: Page,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { mode: Mode::Stealth, idle_send_ms: 0, page: Page::Pad }
    }
}

impl Preferences {
    pub fn load() -> Self {
        let saved = std::fs::read_to_string(path()).ok();
        resolve(saved.as_deref(), std::env::var("RIDDLE_MODE").ok().as_deref(),
            std::env::var("RIDDLE_IDLE_MS").ok().as_deref(),
            std::env::var("RIDDLE_PAGE").ok().as_deref())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, format!("mode={}\nidle_send_ms={}\npage={}\n",
            self.mode.as_str(), self.idle_send_ms, self.page.as_str()))
    }
}

impl Page {
    pub fn as_str(self) -> &'static str {
        match self { Self::Pad => "pad", Self::Learn => "learn" }
    }
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self { Self::Stealth => "stealth", Self::Guided => "guided" }
    }
}

fn path() -> PathBuf {
    std::env::var("RIDDLE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/root/g-pad-data"))
        .join("preferences")
}

fn mode(value: Option<&str>) -> Option<Mode> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "stealth" => Some(Mode::Stealth),
        "guided" => Some(Mode::Guided),
        _ => None,
    }
}

fn millis(value: Option<&str>) -> Option<u64> {
    value?.trim().parse().ok()
}

fn saved_value<'a>(text: Option<&'a str>, key: &str) -> Option<&'a str> {
    text?.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then_some(v.trim())
    })
}

fn page(value: Option<&str>) -> Option<Page> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "pad" => Some(Page::Pad),
        "learn" => Some(Page::Learn),
        _ => None,
    }
}

fn resolve(saved: Option<&str>, env_mode: Option<&str>, env_idle: Option<&str>, env_page: Option<&str>) -> Preferences {
    let defaults = Preferences::default();
    Preferences {
        mode: mode(saved_value(saved, "mode")).or_else(|| mode(env_mode)).unwrap_or(defaults.mode),
        idle_send_ms: millis(saved_value(saved, "idle_send_ms"))
            .or_else(|| millis(env_idle))
            .unwrap_or(defaults.idle_send_ms),
        // The environment wins for the page: RIDDLE_PAGE=learn dedicates a
        // boot to the tutor regardless of where the pad was left.
        page: page(env_page).or_else(|| page(saved_value(saved, "page"))).unwrap_or(defaults.page),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_values_win_over_environment() {
        assert_eq!(resolve(Some("mode=guided\nidle_send_ms=900\n"), Some("stealth"), Some("12"), None),
            Preferences { mode: Mode::Guided, idle_send_ms: 900, page: Page::Pad });
    }

    #[test]
    fn invalid_saved_values_fall_back_in_precedence_order() {
        assert_eq!(resolve(Some("mode=loud\nidle_send_ms=never"), Some("guided"), Some("42"), None),
            Preferences { mode: Mode::Guided, idle_send_ms: 42, page: Page::Pad });
        assert_eq!(resolve(None, Some("bad"), Some("bad"), Some("bad")), Preferences::default());
    }

    #[test]
    fn the_page_persists_but_the_environment_dedicates_a_boot() {
        assert_eq!(resolve(Some("page=learn\n"), None, None, None).page, Page::Learn);
        assert_eq!(resolve(Some("page=learn\n"), None, None, Some("pad")).page, Page::Pad);
        assert_eq!(resolve(None, None, None, Some("learn")).page, Page::Learn);
        assert_eq!(resolve(None, None, None, None).page, Page::Pad);
    }

    #[test]
    fn preference_file_is_not_an_env_or_key_file() {
        assert_eq!(std::path::Path::new("/home/root/g-pad-data/preferences").file_name().unwrap(), "preferences");
    }
}
