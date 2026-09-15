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

// Only `load` runs today; the SYSTEM page is the caller for the rest.
#[allow(dead_code)]
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

    fn restore(&mut self, key: &str) {
        match self.original.remove(key).flatten() {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&p, serialize(&self.pairs)) {
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
}
