//! Which machines run Claude Code, and how to reach them.
//!
//! `hub.toml` next to the binary (or at `HUB_CONFIG`):
//!
//! ```toml
//! [[client]]
//! name = "laptop"                # local, no ssh key
//!
//! [[client]]
//! name = "gauntlet"
//! ssh = "root@gauntlet"
//! projects = "/root/.claude/projects"
//! ```
//!
//! No file means one local client. The parser reads exactly this shape — a
//! whole TOML dependency for three keys is not worth its weight.

/// How the hub reaches a client's filesystem and tmux server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    Local,
    /// An ssh destination (`user@host`), used with BatchMode: the hub must
    /// never hang on a password prompt.
    Ssh(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub name: String,
    pub access: Access,
    /// Where `~/.claude/projects` lives on that machine.
    pub projects: String,
}

/// Parse `hub.toml`. Unknown keys are ignored; a client without a name is
/// dropped — an unnameable client cannot prefix session ids.
pub fn parse(text: &str) -> Vec<Client> {
    let mut out: Vec<Client> = Vec::new();
    let mut current: Option<Client> = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line == "[[client]]" {
            if let Some(c) = current.take() {
                if !c.name.is_empty() {
                    out.push(c);
                }
            }
            current = Some(Client {
                name: String::new(),
                access: Access::Local,
                projects: String::new(),
            });
            continue;
        }
        let Some(c) = current.as_mut() else { continue };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "name" => c.name = value,
            "ssh" if !value.is_empty() => c.access = Access::Ssh(value),
            "projects" => c.projects = value,
            _ => {}
        }
    }
    if let Some(c) = current.take() {
        if !c.name.is_empty() {
            out.push(c);
        }
    }
    out
}

/// The configured clients, or the one implicit local client.
pub fn load(default_projects: &str) -> Vec<Client> {
    let path = std::env::var("HUB_CONFIG").unwrap_or_else(|_| "hub.toml".to_string());
    let mut clients = std::fs::read_to_string(&path)
        .map(|t| parse(&t))
        .unwrap_or_default();
    if clients.is_empty() {
        clients.push(Client {
            name: "local".to_string(),
            access: Access::Local,
            projects: String::new(),
        });
    }
    for c in &mut clients {
        if c.projects.is_empty() {
            c.projects = if c.access == Access::Local {
                default_projects.to_string()
            } else {
                ".claude/projects".to_string()
            };
        }
    }
    clients
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_and_ssh_clients() {
        let toml = r#"
# the fleet
[[client]]
name = "laptop"

[[client]]
name = "gauntlet"
ssh = "root@gauntlet"   # over the lan
projects = "/root/.claude/projects"
"#;
        let c = parse(toml);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "laptop");
        assert_eq!(c[0].access, Access::Local);
        assert_eq!(c[1].access, Access::Ssh("root@gauntlet".into()));
        assert_eq!(c[1].projects, "/root/.claude/projects");
    }

    #[test]
    fn a_nameless_client_is_dropped_and_junk_is_ignored() {
        let c = parse("[[client]]\nssh = \"root@x\"\n\nnot toml at all\n");
        assert!(c.is_empty());
    }
}
