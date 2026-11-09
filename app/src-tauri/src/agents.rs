//! What the sidebar shows: every agent record, corrected against the live tmux panes, plus a watcher
//! that pushes the whole list to the frontend whenever it changes.

use serde::Serialize;
use shifu::agent::{self, Agent, PaneInfo};
use shifu::status::Status;
use shifu::tmux::Tmux;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const POLL: Duration = Duration::from_millis(500);
const BRANCH_TTL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentView {
    pub name: String,
    pub kind: String,
    pub cwd: String,
    pub dir_name: String,
    pub branch: Option<String>,
    /// The conversation's topic from the terminal title, cleaned; `None` when the title says nothing.
    pub title: Option<String>,
    /// The name the user gave it; shown in place of `title`.
    pub label: Option<String>,
    pub status: Status,
    pub status_since: u64,
    pub last_message: Option<String>,
    /// Claude's session id as the hooks recorded it; an optional judgement log (jev.rs) names turns by it. `None` before the first hook.
    pub session_id: Option<String>,
    pub exited: bool,
}

fn is_status_glyph(c: char) -> bool {
    // Claude puts ✳ ✶ ✻ ✽ (dingbats) or a braille spinner in front of the topic; Codex and others use shapes or dots.
    matches!(c, '\u{00B7}' | '\u{2022}' | '*' | '\u{2600}'..='\u{27BF}' | '\u{25A0}'..='\u{25FF}' | '\u{2800}'..='\u{28FF}')
}

/// The topic out of a tmux pane title: status symbols in front and ` | <folder>` behind are dropped. A title that
/// is then empty or only the program name, the folder name or the machine's host name (the shell's default)
/// says nothing, so it is `None`.
pub fn clean_title(raw: &str, dir_name: &str, hostname: &str) -> Option<String> {
    let t = raw.trim_start_matches(|c: char| c.is_whitespace() || is_status_glyph(c)).trim_end();
    let t = t.strip_suffix(&format!(" | {dir_name}")).unwrap_or(t).trim();
    let host_short = hostname.split('.').next().unwrap_or(hostname);
    let empty_names = ["", "Claude Code", "Codex", dir_name, hostname, host_short];
    if empty_names.iter().any(|n| n.eq_ignore_ascii_case(t)) { None } else { Some(t.to_string()) }
}

/// `pane_id -> raw title` from `list-panes -a -F "#{pane_id} #{pane_title}"`.
pub fn parse_titles(out: &str) -> HashMap<String, String> {
    out.lines().filter_map(|l| l.split_once(' ')).map(|(id, title)| (id.to_string(), title.to_string())).collect()
}

fn pane_titles(tmux: &Tmux) -> HashMap<String, String> {
    parse_titles(&tmux.run(["list-panes", "-a", "-F", "#{pane_id} #{pane_title}"]).unwrap_or_default())
}

/// The machine's host name; the shell sets the pane title to it, so it must not count as a topic.
fn hostname() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let out = Command::new("hostname").output().ok().filter(|o| o.status.success());
        out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    })
}

/// `branch` is looked up by the caller (it costs a process spawn); `raw_title` is the pane's tmux title.
pub fn build_view(
    agent: &Agent,
    panes: &HashMap<String, PaneInfo>,
    branch: Option<String>,
    raw_title: Option<&str>,
) -> AgentView {
    // The file says what the last hook reported; tmux knows if the pane is gone or dead.
    let status = agent.effective_status(panes);
    let dir_name = Path::new(&agent.cwd).file_name().and_then(|n| n.to_str()).unwrap_or(&agent.cwd).to_string();
    AgentView {
        name: agent.name.clone(),
        kind: agent.kind.as_str().to_string(),
        cwd: agent.cwd.clone(),
        title: raw_title.and_then(|t| clean_title(t, &dir_name, hostname())),
        label: agent.label.clone(),
        dir_name,
        branch,
        status,
        status_since: agent.status_since,
        last_message: agent.last_message.clone(),
        session_id: agent.session_id.clone(),
        exited: status == Status::Exited,
    }
}

/// Git branch per directory, looked up at most once per `BRANCH_TTL`.
#[derive(Default)]
pub struct Branches {
    seen: Mutex<HashMap<String, (Instant, Option<String>)>>,
}

impl Branches {
    fn of(&self, cwd: &str) -> Option<String> {
        let mut seen = self.seen.lock().unwrap();
        if let Some((at, branch)) = seen.get(cwd)
            && at.elapsed() < BRANCH_TTL
        {
            return branch.clone();
        }
        let out = Command::new("git").args(["-C", cwd, "branch", "--show-current"]).output().ok();
        let branch = out
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|b| !b.is_empty());
        seen.insert(cwd.to_string(), (Instant::now(), branch.clone()));
        branch
    }
}

/// All agents ordered by name; the frontend decides the display order.
pub fn snapshot(branches: &Branches) -> Vec<AgentView> {
    let (agents, _) = agent::list();
    let tmux = Tmux::from_env();
    let panes = tmux.panes();
    let titles = pane_titles(&tmux);
    let mut views: Vec<AgentView> = agents
        .iter()
        .map(|a| {
            let status = a.effective_status(&panes);
            let branch = if status == Status::Exited { None } else { branches.of(&a.cwd) };
            let raw_title = a.pane_id.as_ref().and_then(|id| titles.get(id)).map(String::as_str);
            build_view(a, &panes, branch, raw_title)
        })
        .collect();
    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}

pub fn window_title(waiting: usize) -> String {
    let name = crate::config::app_name();
    if waiting == 0 { name.to_string() } else { format!("{name} — {waiting} 个在等你") }
}

/// Window title and Dock badge for the number of agents that need the user. The frontend counts (it alone knows
/// which replies are new) and calls this through the `set_attention` command.
pub fn show_waiting(app: &AppHandle, waiting: usize) {
    let Some(window) = app.get_webview_window("main") else { return };
    if let Err(e) = window.set_title(&window_title(waiting)) {
        eprintln!("shifu-app: set title failed: {e}");
    }
    let badge = (waiting > 0).then_some(waiting as i64);
    if let Err(e) = window.set_badge_count(badge) {
        eprintln!("shifu-app: set badge failed: {e}");
    }
}

/// Reads every `POLL`, emits `agents` with the whole list only when it changed.
pub fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let branches = app.state::<Branches>();
        let mut last: Option<Vec<AgentView>> = None;
        loop {
            let views = snapshot(&branches);
            if last.as_ref() != Some(&views) {
                if let Err(e) = app.emit("agents", &views) {
                    eprintln!("shifu-app: emit agents failed: {e}");
                }
                last = Some(views);
            }
            std::thread::sleep(POLL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use shifu::agent::Kind;

    fn agent(name: &str, status: Status, pane: Option<&str>) -> Agent {
        Agent {
            name: name.to_string(),
            kind: Kind::Claude,
            command: "claude".to_string(),
            cwd: "/work/中文 proj".to_string(),
            created_at: 1,
            status,
            status_since: 42,
            window_id: None,
            pane_id: pane.map(str::to_string),
            session_id: None,
            last_message: Some("done".to_string()),
            model: None,
            effort: None,
            label: None,
        }
    }

    fn panes(list: &[(&str, bool)]) -> HashMap<String, PaneInfo> {
        list.iter().map(|(id, dead)| (id.to_string(), PaneInfo { dead: *dead, window_id: "@1".to_string(), index: 0 })).collect()
    }

    #[test]
    fn live_agent_shows_the_status_from_its_file() {
        let v = build_view(&agent("a", Status::NeedsApproval, Some("%1")), &panes(&[("%1", false)]), Some("main".into()), None);
        assert_eq!(v.status, Status::NeedsApproval);
        assert!(!v.exited);
        assert_eq!(v.dir_name, "中文 proj");
        assert_eq!(v.branch.as_deref(), Some("main"));
        assert_eq!(v.status_since, 42);
        assert_eq!(v.last_message.as_deref(), Some("done"));
    }

    #[test]
    fn dead_pane_means_exited_whatever_the_file_says() {
        let v = build_view(&agent("a", Status::Running, Some("%1")), &panes(&[("%1", true)]), None, None);
        assert_eq!(v.status, Status::Exited);
        assert!(v.exited);
    }

    #[test]
    fn missing_pane_means_exited() {
        let v = build_view(&agent("a", Status::Idle, Some("%9")), &panes(&[("%1", false)]), None, None);
        assert!(v.exited);
    }

    #[test]
    fn session_end_hook_marks_exited_even_if_the_pane_is_alive() {
        let v = build_view(&agent("a", Status::Exited, Some("%1")), &panes(&[("%1", false)]), None, None);
        assert!(v.exited);
    }

    #[test]
    fn agent_without_a_pane_yet_is_not_exited() {
        let v = build_view(&agent("a", Status::Starting, None), &panes(&[]), None, None);
        assert!(!v.exited);
        assert_eq!(v.status, Status::Starting);
    }

    #[test]
    fn json_uses_snake_case_status_names() {
        let v = build_view(&agent("a", Status::NeedsReply, Some("%1")), &panes(&[("%1", false)]), None, None);
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["status"], "needs_reply");
        assert_eq!(json["kind"], "claude");
    }

    #[test]
    fn window_title_counts_waiting_agents() {
        assert_eq!(window_title(0), "Shifu");
        assert_eq!(window_title(1), "Shifu — 1 个在等你");
        assert_eq!(window_title(3), "Shifu — 3 个在等你");
    }

    #[test]
    fn real_titles_are_cleaned_to_the_topic() {
        let host = "Someones-MacBook-Air.local";
        let c = |raw: &str, dir: &str| clean_title(raw, dir, host);
        assert_eq!(c("✳ 赌石模拟器云端同步与DeepSeek接入", "Workspace").as_deref(), Some("赌石模拟器云端同步与DeepSeek接入"));
        assert_eq!(c("检查采购总表 | Workspace", "Workspace").as_deref(), Some("检查采购总表"));
        assert_eq!(c("⠂ 修仙模拟器", "Workspace").as_deref(), Some("修仙模拟器"));
        assert_eq!(c("⠐ 修仙模拟器 | Workspace", "Workspace").as_deref(), Some("修仙模拟器"));
        // a bracket or slash is part of the name, not a status symbol
        assert_eq!(c("(草稿) notes", "w").as_deref(), Some("(草稿) notes"));
        assert_eq!(c("/tmp/a.png", "w").as_deref(), Some("/tmp/a.png"));
    }

    #[test]
    fn titles_that_say_nothing_are_none() {
        let host = "Someones-MacBook-Air.local";
        for raw in ["✳ Claude Code", "claude code", "Codex", "Workspace", "workspace", host, "Someones-MacBook-Air", "", "  ✳ ", "Claude Code | Workspace"] {
            assert_eq!(clean_title(raw, "Workspace", host), None, "{raw:?}");
        }
    }

    #[test]
    fn titles_are_read_per_pane_even_with_spaces_and_chinese() {
        let got = parse_titles("%1 ✳ 修仙 模拟器\n%2 host\n%3 \nbad");
        assert_eq!(got["%1"], "✳ 修仙 模拟器");
        assert_eq!(got["%2"], "host");
        assert_eq!(got["%3"], "");
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn view_carries_the_cleaned_title() {
        let v = build_view(&agent("a", Status::NeedsReply, Some("%1")), &panes(&[("%1", false)]), None, Some("✳ 修仙模拟器"));
        assert_eq!(v.title.as_deref(), Some("修仙模拟器"));
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["title"], "修仙模拟器");
        let v = build_view(&agent("a", Status::NeedsReply, Some("%1")), &panes(&[("%1", false)]), None, Some("中文 proj"));
        assert_eq!(v.title, None);
    }
}
