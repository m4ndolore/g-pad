//! The live half of session truth: what the pane is showing right now.
//!
//! A transcript says what was said; only the pane says whether the session is
//! streaming, sitting at a permission prompt, or done. Classification is
//! conservative by design — unrecognized content reads as RUNNING, never
//! WAITING. A false "needs you" spends the reviewer's attention; a false
//! "running" only delays.
//!
//! Nudges land here too. The pad POSTs a mark; what keystrokes that becomes
//! is this module's business alone, so a future hooks-based transport can
//! replace it without the pad changing.

use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub command: String,
    pub path: String,
}

/// What a captured pane says the session is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneState {
    Running,
    Waiting,
    Done,
    /// The multi-session picker UI, not a session. Typing into it would
    /// start a new task, so it can never be a nudge target.
    Picker,
}

impl PaneState {
    pub fn word(self) -> &'static str {
        match self {
            PaneState::Running => "running",
            PaneState::Waiting => "waiting",
            PaneState::Done | PaneState::Picker => "done",
        }
    }
}

/// Every pane on the server that is running the claude CLI.
pub fn claude_panes() -> Vec<Pane> {
    let out = Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{pane_id}\t#{pane_current_command}\t#{pane_current_path}"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (id, command, path) = (f.next()?, f.next()?, f.next()?);
            command.starts_with("claude").then(|| Pane {
                id: id.to_string(),
                command: command.to_string(),
                path: path.to_string(),
            })
        })
        .collect()
}

/// The last screenful of a pane, which is where every prompt lives.
pub fn capture(pane_id: &str) -> String {
    let out = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", pane_id, "-S", "-40"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// Read a captured pane. Markers observed on real panes, 2026-08-30.
pub fn classify(captured: &str) -> PaneState {
    // The fleet picker announces itself with its new-task prompt.
    if captured.contains("describe a task for a new session") {
        return PaneState::Picker;
    }
    // A permission or question dialog: numbered options under a question.
    if captured.contains("Do you want") || captured.contains("❯ 1.") {
        return PaneState::Waiting;
    }
    // A streaming turn always offers the interrupt.
    if captured.contains("esc to interrupt") {
        return PaneState::Running;
    }
    // A finished turn signs off with its wall-clock.
    if captured.contains("· done") || captured.contains("new task?") {
        return PaneState::Done;
    }
    PaneState::Running
}

/// What the pad may ask a pane to do.
pub enum Nudge {
    /// Accept the pending prompt's first option.
    Tick,
    /// Back out of the pending prompt.
    Strike,
    /// The reviewer's own words, sent as a turn.
    Text(String),
}

/// Carry a nudge into a pane. Refuses the picker: a mark aimed at a session
/// must never become a new task in the fleet UI.
pub fn nudge(pane: &Pane, n: &Nudge) -> Result<(), String> {
    if classify(&capture(&pane.id)) == PaneState::Picker {
        return Err(format!("pane {} is the session picker, not a session", pane.id));
    }
    let status = match n {
        Nudge::Tick => send_keys(&pane.id, &["1"], false),
        Nudge::Strike => send_keys(&pane.id, &["Escape"], false),
        Nudge::Text(t) => {
            send_keys(&pane.id, &[t.as_str()], true)?;
            send_keys(&pane.id, &["Enter"], false)
        }
    };
    status
}

/// `literal` sends the argument as characters; otherwise tmux reads key names.
fn send_keys(pane_id: &str, keys: &[&str], literal: bool) -> Result<(), String> {
    let mut cmd = Command::new("tmux");
    cmd.args(["send-keys", "-t", pane_id]);
    if literal {
        cmd.arg("-l");
    }
    cmd.args(keys);
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("tmux send-keys exited {s}")),
        Err(e) => Err(format!("tmux send-keys: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from real captures, 2026-08-30.
    const PICKER: &str = "Needs input\n✻ homelab deployment architecture\n❯ describe a task for a new session\n  ⏵⏵ auto mode";
    const IDLE: &str = "  result: fully green at 1009/1009.\n✻ Sautéed for 5m 12s · done 8:50 PM · 1 shell still running\n❯ now update the deck";
    const STREAMING: &str = "✶ Simmering… (esc to interrupt · ctrl+t to hide todos)";
    const PERMISSION: &str = "Do you want to make this edit to brief.rs?\n❯ 1. Yes\n  2. Yes, allow all edits during this session\n  3. No";

    #[test]
    fn real_captures_classify_correctly() {
        assert_eq!(classify(PICKER), PaneState::Picker);
        assert_eq!(classify(IDLE), PaneState::Done);
        assert_eq!(classify(STREAMING), PaneState::Running);
        assert_eq!(classify(PERMISSION), PaneState::Waiting);
    }

    #[test]
    fn unknown_content_reads_as_running_never_waiting() {
        assert_eq!(classify(""), PaneState::Running);
        assert_eq!(classify("$ make -j8\ncc -O2 …"), PaneState::Running);
    }

    #[test]
    fn the_picker_is_shown_as_done_never_waiting() {
        // The picker's "awaiting input" lines describe other sessions; the
        // pane itself must not surface as a session needing a human.
        assert_eq!(PaneState::Picker.word(), "done");
    }
}
