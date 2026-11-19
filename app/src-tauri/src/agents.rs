//! What the sidebar shows: every agent record, corrected against the live tmux panes, plus a watcher
//! that pushes the whole list to the frontend whenever it changes.

use serde::Serialize;
use shifu::agent::{self, Agent, PaneInfo};
use shifu::topic::{clean_title, hostname, pane_titles};
use shifu::status::Status;
use shifu::tmux::Tmux;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
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
    /// One line on what it waits for (see `Agent::waiting_for`).
    pub waiting_for: Option<String>,
    /// Claude's session id as the hooks recorded it; Jev's log names turns by it. `None` before the first hook.
    pub session_id: Option<String>,
    pub exited: bool,
    pub progress: Option<shifu::supervision::Progress>,
    pub progress_error: Option<String>,
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
    let (progress, progress_error) = match shifu::supervision::read(&agent::shifu_home(), agent) {
        Ok(value) => (value, None), Err(error) => (None, Some(error)),
    };
    AgentView {
        progress, progress_error,
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
        waiting_for: agent.waiting_for.clone(),
        session_id: agent.session_id.clone(),
        exited: status == Status::Exited,
    }
}

#[derive(Default)]
struct Lookup {
    at: Option<Instant>,
    branch: Option<String>,
    running: bool,
}

/// Git branch per directory, refreshed at most once per `BRANCH_TTL`.
///
/// Asking never waits for git: the answer is what the last lookup found (nothing before the first one returns),
/// and a stale directory is looked up on its own thread, one lookup per directory at a time. `git` can sit for as
/// long as a macOS folder-access prompt stays open; when the lookup ran under the lock, on the caller's thread,
/// that froze the whole window.
#[derive(Default)]
pub struct Branches {
    seen: Arc<Mutex<HashMap<String, Lookup>>>,
}

fn git_branch(cwd: &str) -> Option<String> {
    let out = Command::new("git").args(["-C", cwd, "branch", "--show-current"]).output().ok();
    out.filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty())
}

impl Branches {
    fn of(&self, cwd: &str) -> Option<String> {
        self.of_with(cwd, git_branch)
    }

    fn of_with(&self, cwd: &str, look_up: impl FnOnce(&str) -> Option<String> + Send + 'static) -> Option<String> {
        let mut seen = self.seen.lock().unwrap();
        let entry = seen.entry(cwd.to_string()).or_default();
        let fresh = entry.at.is_some_and(|at| at.elapsed() < BRANCH_TTL);
        if !fresh && !entry.running {
            entry.running = true;
            let (seen, cwd) = (Arc::clone(&self.seen), cwd.to_string());
            std::thread::spawn(move || {
                let branch = look_up(&cwd);
                seen.lock().unwrap().insert(cwd, Lookup { at: Some(Instant::now()), branch, running: false });
            });
        }
        entry.branch.clone()
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
        let mut input_error = None;
        loop {
            let (mut records, _) = agent::list();
            let tmux = Tmux::from_env();
            let panes = tmux.panes();
            for record in &mut records { record.status = record.effective_status(&panes); }
            let result = app.state::<crate::pty::Ptys>().serialize_input(|| shifu::inbox::pump_one(&agent::shifu_home(), &records, &tmux));
            let goals = crate::taskgoals::tell_newcomers(&app.state::<crate::config::Config>(), &records);
            let error = result.and(goals).err();
            if error != input_error {
                if let Some(ref message) = error { eprintln!("shifu-app: queued input: {message}"); }
                input_error = error;
            }
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
    use std::sync::mpsc;

    fn eventually(mut check: impl FnMut() -> bool) -> bool {
        (0..400).any(|_| check() || { std::thread::sleep(Duration::from_millis(5)); false })
    }

    #[test]
    fn asking_for_a_branch_never_waits_for_git() {
        let branches = Branches::default();
        let (release, held) = mpsc::channel::<()>();
        let (started, starts) = mpsc::channel::<()>();
        let stuck = move |_: &str| { started.send(()).unwrap(); held.recv().ok(); Some("main".to_string()) };
        // git sits in /stuck (a folder-access prompt): the caller gets an answer at once, and so does every other directory.
        assert_eq!(branches.of_with("/stuck", stuck), None);
        starts.recv().unwrap();
        assert_eq!(branches.of_with("/stuck", |_| panic!("a second lookup while the first one runs")), None);
        assert_eq!(branches.of_with("/other", |_| Some("dev".to_string())), None);
        assert!(eventually(|| branches.of_with("/other", |_| Some("dev".to_string())).as_deref() == Some("dev")));
        // Once git returns, the next ask has the branch.
        release.send(()).unwrap();
        assert!(eventually(|| branches.of_with("/stuck", |_| Some("main".to_string())).as_deref() == Some("main")));
    }

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
            waiting_for: Some("要合并吗？".to_string()),
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
        assert_eq!(v.waiting_for.as_deref(), Some("要合并吗？"));
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
    fn live_pane_keeps_a_stale_session_end_from_showing_the_resume_banner() {
        let v = build_view(&agent("a", Status::Exited, Some("%1")), &panes(&[("%1", false)]), None, None);
        assert_eq!(v.status, Status::Running);
        assert!(!v.exited);
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
    fn view_carries_the_cleaned_title() {
        let v = build_view(&agent("a", Status::NeedsReply, Some("%1")), &panes(&[("%1", false)]), None, Some("✳ 修仙模拟器"));
        assert_eq!(v.title.as_deref(), Some("修仙模拟器"));
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["title"], "修仙模拟器");
        let v = build_view(&agent("a", Status::NeedsReply, Some("%1")), &panes(&[("%1", false)]), None, Some("中文 proj"));
        assert_eq!(v.title, None);
    }
}
