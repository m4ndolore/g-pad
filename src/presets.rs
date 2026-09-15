//! Oracle presets from settings.schema.json — the file the bundle already
//! ships for the off-device editor. The SYSTEM page reads the same file, so
//! a preset added over ssh needs no rebuild. Loaded from
//! `$RIDDLE_PRESETS_FILE`, else `settings.schema.json` beside the binary,
//! else the copy compiled in.

use crate::brief::{json_field, split_objects};

// Nothing reads a preset yet; the SYSTEM page is the consumer.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub models: Vec<String>,
    pub base: String,
    pub model: String,
    pub reasoning: String,
}

// The SYSTEM page is the caller.
#[allow(dead_code)]
pub fn load() -> Vec<Preset> {
    let from_env = std::env::var("RIDDLE_PRESETS_FILE").ok().map(std::path::PathBuf::from);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("settings.schema.json")));
    for path in [from_env, beside].into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let p = parse(&text);
            if !p.is_empty() {
                return p;
            }
        }
    }
    parse(include_str!("../settings.schema.json"))
}

pub fn parse(json: &str) -> Vec<Preset> {
    split_objects(json, "presets")
        .into_iter()
        .filter_map(|block| {
            let name = json_field(&block, "name")?;
            Some(Preset {
                name,
                models: json_string_array(&block, "models"),
                base: json_field(&block, "RIDDLE_OPENAI_BASE").unwrap_or_default(),
                model: json_field(&block, "RIDDLE_OPENAI_MODEL").unwrap_or_default(),
                reasoning: json_field(&block, "RIDDLE_OPENAI_REASONING").unwrap_or_default(),
            })
        })
        .collect()
}

/// Which preset the environment's base URL belongs to, trailing slash ignored.
// The SYSTEM page is the caller.
#[allow(dead_code)]
pub fn active(presets: &[Preset], base: Option<&str>) -> Option<usize> {
    let base = base?.trim_end_matches('/');
    presets.iter().position(|p| p.base.trim_end_matches('/') == base)
}

/// `"key": ["a", "b"]` → the strings. Enough for a list of model names.
fn json_string_array(block: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let Some(i) = block.find(&needle) else { return Vec::new() };
    let rest = &block[i + needle.len()..];
    let Some(open) = rest.find('[') else { return Vec::new() };
    let Some(close) = rest[open..].find(']') else { return Vec::new() };
    rest[open + 1..open + close]
        .split(',')
        .filter_map(|s| s.trim().strip_prefix('"')?.strip_suffix('"').map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"title":"t","presets":[
      {"name":"Vellum","models":["vellum-capture","vellum-ask"],"values":{"RIDDLE_OPENAI_BASE":"https://v/v1","RIDDLE_OPENAI_MODEL":"vellum-capture","RIDDLE_OPENAI_REASONING":""}},
      {"name":"OpenAI","models":["gpt-4o-mini"],"values":{"RIDDLE_OPENAI_BASE":"https://api.openai.com/v1","RIDDLE_OPENAI_MODEL":"gpt-4o-mini","RIDDLE_OPENAI_REASONING":"low"}}
    ]}"#;

    #[test]
    fn presets_parse_names_models_and_values() {
        let p = parse(SAMPLE);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "Vellum");
        assert_eq!(p[0].models, vec!["vellum-capture", "vellum-ask"]);
        assert_eq!(p[0].base, "https://v/v1");
        assert_eq!(p[1].reasoning, "low");
        assert_eq!(p[0].reasoning, "");
    }

    #[test]
    fn the_active_preset_is_the_one_whose_base_matches() {
        let p = parse(SAMPLE);
        assert_eq!(active(&p, Some("https://v/v1/")), Some(0));
        assert_eq!(active(&p, Some("https://elsewhere")), None);
        assert_eq!(active(&p, None), None);
    }

    #[test]
    fn the_shipped_schema_parses_and_leads_with_vellum() {
        let p = parse(include_str!("../settings.schema.json"));
        assert!(p.len() >= 4);
        assert_eq!(p[0].name, "Vellum");
        assert!(p.iter().all(|x| !x.models.is_empty()));
    }
}
