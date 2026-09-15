//! In-app config that beats `oracle.env` without editing it. Plain
//! `key=value` lines in `$RIDDLE_DATA_DIR/overrides`, replayed into the
//! process environment at boot and rewritten by the SYSTEM page. The
//! environment stays the config bus: everything that reads
//! `std::env::var("RIDDLE_…")` keeps working unchanged.

use std::collections::HashMap;
use std::path::PathBuf;

pub struct Overrides {
    pairs: Vec<(String, String)>,
    /// What the environment held before each key was overridden, so RESET
    /// can put `oracle.env`'s value back without a restart.
    original: HashMap<String, Option<String>>,
}

impl Overrides {
    /// Read the file and apply it to the environment.
    pub fn load() -> Self {
        let pairs = std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default();
        let mut o = Self { pairs: Vec::new(), original: HashMap::new() };
        for (k, v) in pairs {
            o.apply(&k, &v);
        }
        o
    }

    // Exercised by tests; the page uses set/reset.
    #[allow(dead_code)]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// How many keys are overridden right now.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Override one key: environment now, file for next boot.
    pub fn set(&mut self, key: &str, value: &str) {
        self.apply(key, value);
        self.save();
    }

    /// Drop one override and restore what the environment had.
    // Exercised by tests; the page uses set/reset.
    #[allow(dead_code)]
    pub fn unset(&mut self, key: &str) {
        self.pairs.retain(|(k, _)| k != key);
        self.restore(key);
        self.save();
    }

    /// Drop every override; `oracle.env` wins again.
    pub fn reset(&mut self) {
        let keys: Vec<String> = self.pairs.drain(..).map(|(k, _)| k).collect();
        for k in keys {
            self.restore(&k);
        }
        let _ = std::fs::remove_file(path());
    }

    fn apply(&mut self, key: &str, value: &str) {
        self.original.entry(key.to_string()).or_insert_with(|| std::env::var(key).ok());
        std::env::set_var(key, value);
        self.pairs.retain(|(k, _)| k != key);
        self.pairs.push((key.to_string(), value.to_string()));
    }

    /// A key that was never overridden has nothing to restore: leave the
    /// live value alone rather than strip it from the process.
    fn restore(&mut self, key: &str) {
        if let Some(orig) = self.original.remove(key) {
            match orig {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }

    /// Written to a sibling `.tmp` and renamed into place, so power loss
    /// mid-write leaves the previous file intact instead of a torn one.
    fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = p.with_extension("tmp");
        let written = std::fs::write(&tmp, serialize(&self.pairs)).and_then(|_| std::fs::rename(&tmp, &p));
        if let Err(e) = written {
            eprintln!("g-pad: overrides not saved: {e}");
        }
    }
}

pub fn parse(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() {
            continue;
        }
        out.retain(|(ek, _)| ek != k);
        out.push((k.to_string(), v.to_string()));
    }
    out
}

pub fn serialize(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
}

fn path() -> PathBuf {
    std::env::var("RIDDLE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/root/g-pad-data"))
        .join("overrides")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RIDDLE_DATA_DIR` and the keys below are process-global; cargo runs
    /// tests in parallel, so the env-mutating tests take turns.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn parse_keeps_order_skips_junk_and_takes_the_last_duplicate() {
        let got = parse("RIDDLE_PALM_MS=750\n# comment\nnot a pair\nRIDDLE_OPENAI_MODEL=a\nRIDDLE_OPENAI_MODEL=b\n =x\n");
        assert_eq!(got, vec![
            ("RIDDLE_PALM_MS".to_string(), "750".to_string()),
            ("RIDDLE_OPENAI_MODEL".to_string(), "b".to_string()),
        ]);
    }

    #[test]
    fn serialize_round_trips() {
        let pairs = vec![("A".to_string(), "1".to_string()), ("B".to_string(), "two words".to_string())];
        assert_eq!(parse(&serialize(&pairs)), pairs);
    }

    #[test]
    fn a_set_beats_the_environment_and_reset_restores_it() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gpad-ovr-set-{}", std::process::id()));
        std::env::set_var("RIDDLE_DATA_DIR", &dir);
        std::env::set_var("RIDDLE_TEST_KEY", "from-env");
        let mut o = Overrides::load();
        o.set("RIDDLE_TEST_KEY", "from-page");
        assert_eq!(std::env::var("RIDDLE_TEST_KEY").unwrap(), "from-page");
        assert_eq!(parse(&std::fs::read_to_string(dir.join("overrides")).unwrap()),
            vec![("RIDDLE_TEST_KEY".to_string(), "from-page".to_string())]);
        assert!(!dir.join("overrides.tmp").exists());
        o.reset();
        assert_eq!(std::env::var("RIDDLE_TEST_KEY").unwrap(), "from-env");
        assert!(!dir.join("overrides").exists());
        let _ = std::fs::remove_dir_all(dir);
        std::env::remove_var("RIDDLE_DATA_DIR");
    }

    #[test]
    fn unset_removes_one_key_and_restores_its_original() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gpad-ovr-unset-{}", std::process::id()));
        std::env::set_var("RIDDLE_DATA_DIR", &dir);
        std::env::remove_var("RIDDLE_UNSET_KEY");
        let mut o = Overrides::load();
        o.set("RIDDLE_UNSET_KEY", "v");
        o.unset("RIDDLE_UNSET_KEY");
        assert!(std::env::var("RIDDLE_UNSET_KEY").is_err());
        assert!(o.get("RIDDLE_UNSET_KEY").is_none());
        let _ = std::fs::remove_dir_all(dir);
        std::env::remove_var("RIDDLE_DATA_DIR");
    }

    #[test]
    fn load_replays_the_file_over_the_environment_and_unset_puts_it_back() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gpad-ovr-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("overrides"), "RIDDLE_TEST_LOAD=from-file\n").unwrap();
        std::env::set_var("RIDDLE_DATA_DIR", &dir);
        std::env::set_var("RIDDLE_TEST_LOAD", "from-env");
        let mut o = Overrides::load();
        assert_eq!(std::env::var("RIDDLE_TEST_LOAD").unwrap(), "from-file");
        assert_eq!(o.get("RIDDLE_TEST_LOAD"), Some("from-file"));
        o.unset("RIDDLE_TEST_LOAD");
        assert_eq!(std::env::var("RIDDLE_TEST_LOAD").unwrap(), "from-env");
        let _ = std::fs::remove_dir_all(dir);
        std::env::remove_var("RIDDLE_TEST_LOAD");
        std::env::remove_var("RIDDLE_DATA_DIR");
    }

    #[test]
    fn unset_of_a_key_never_overridden_leaves_the_environment_alone() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gpad-ovr-never-{}", std::process::id()));
        std::env::set_var("RIDDLE_DATA_DIR", &dir);
        std::env::set_var("RIDDLE_NEVER_SET_KEY", "from-env");
        let mut o = Overrides::load();
        o.unset("RIDDLE_NEVER_SET_KEY");
        assert_eq!(std::env::var("RIDDLE_NEVER_SET_KEY").unwrap(), "from-env");
        let _ = std::fs::remove_dir_all(dir);
        std::env::remove_var("RIDDLE_NEVER_SET_KEY");
        std::env::remove_var("RIDDLE_DATA_DIR");
    }
}
