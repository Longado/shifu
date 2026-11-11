//! agent 文件：`<SHIFU_HOME>/agents/<name>.json`，原子写入；命名规则。

use crate::status::Status;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Res<T> = Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Claude,
    Codex,
    /// Tencent's CodeBuddy Code CLI: Claude Code's flags and hook events, so it is driven the same way.
    CodeBuddy,
    /// Moonshot's Kimi Code CLI (`kimi`; agents from the old `kimi-cli` start it too): hooks come only from its config file.
    Kimi,
    Other,
}

impl Kind {
    pub fn of_command(command: &str) -> Kind {
        match command {
            "claude" => Kind::Claude,
            "codex" => Kind::Codex,
            "codebuddy" => Kind::CodeBuddy,
            "kimi" | "kimi-cli" => Kind::Kimi,
            _ => Kind::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Claude => "claude",
            Kind::Codex => "codex",
            Kind::CodeBuddy => "codebuddy",
            Kind::Kimi => "kimi",
            Kind::Other => "other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub kind: Kind,
    pub command: String,
    pub cwd: String,
    pub created_at: u64,
    pub status: Status,
    pub status_since: u64,
    pub window_id: Option<String>,
    pub pane_id: Option<String>,
    pub session_id: Option<String>,
    pub last_message: Option<String>,
    /// Picked when the agent was created; `None` leaves it to the CLI's own default. Passed again on resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// A name the user gave the agent (right-click on its tab); shown instead of the conversation topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Agent {
    /// 显示用状态：用 tmux 窗格校正（窗格没了或已死 → 已退出）。
    /// 「其他」类命令没有 hook，启动中就当运行中。
    pub fn effective_status(&self, panes: &HashMap<String, PaneInfo>) -> Status {
        if let Some(pane) = &self.pane_id {
            match panes.get(pane) {
                None => return Status::Exited,
                Some(info) if info.dead => return Status::Exited,
                // SessionEnd can arrive while the same terminal agent is still alive. Do not offer Resume over
                // an active process; subsequent hooks restore the more specific running/waiting state.
                Some(_) if self.status == Status::Exited => return Status::Running,
                Some(_) => {}
            }
        }
        if self.kind == Kind::Other && self.status == Status::Starting {
            return Status::Running;
        }
        self.status
    }
}

/// 刚开或刚恢复时写到窗格 `@shifu_status` 上的状态（标签栏圆点用）。
/// 「其他」类命令没有 hook，之后不会再有人改它，所以直接标运行中，和侧栏的 `effective_status` 一致。
pub fn initial_tab_status(kind: Kind) -> Status {
    if kind == Kind::Other { Status::Running } else { Status::Starting }
}

/// tmux 里一个窗格现在的样子。agent 在哪一页，永远以这里的 `window_id` 为准，不看 agent 文件里存的。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneInfo {
    pub dead: bool,
    pub window_id: String,
    pub index: u32,
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn shifu_home() -> PathBuf {
    match std::env::var_os("SHIFU_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".shifu"),
    }
}

fn agents_dir() -> PathBuf {
    shifu_home().join("agents")
}

/// 名字只留 `[A-Za-z0-9._-]`，其他换成 `-`；连续的 `-` 合并，首尾的 `-` 去掉，空了用 `agent`。
pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '-' };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { "agent".to_string() } else { trimmed.to_string() }
}

/// `<agent>-<目录名>`，与已有名字重名就加 `-2`、`-3`……
pub fn make_name(command: &str, cwd: &Path, taken: &[String]) -> String {
    let program = Path::new(command).file_name().and_then(|s| s.to_str()).unwrap_or(command);
    let dir = cwd.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let base = sanitize_name(&format!("{program}-{dir}"));
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("无限序列一定有空位")
}

/// hook 从环境变量拿到的名字要能安全拼成文件名。
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn agent_path(name: &str) -> PathBuf {
    agents_dir().join(format!("{name}.json"))
}

/// 先写临时文件再 rename，读的人不会读到半个文件。
pub fn save(agent: &Agent) -> Res<()> {
    let dir = agents_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(agent).map_err(|e| format!("序列化失败: {e}"))?;
    let tmp = dir.join(format!(".{}.{}.tmp", agent.name, std::process::id()));
    fs::write(&tmp, text).map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
    let dest = agent_path(&agent.name);
    fs::rename(&tmp, &dest).map_err(|e| format!("改名到 {} 失败: {e}", dest.display()))
}

/// 文件不存在返回 `Ok(None)`。
pub fn load(name: &str) -> Res<Option<Agent>> {
    let path = agent_path(name);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{} 格式不对: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读 {} 失败: {e}", path.display())),
    }
}

pub fn remove(name: &str) -> Res<()> {
    match fs::remove_file(agent_path(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("删 {name} 失败: {e}")),
    }
}

/// 读全部 agent。坏文件不让整个列表失败，单独返回问题描述。
pub fn list() -> (Vec<Agent>, Vec<String>) {
    let dir = agents_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut agents = Vec::new();
    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_agent_file = path.extension().is_some_and(|e| e == "json")
            && !path.file_name().and_then(|n| n.to_str()).unwrap_or(".").starts_with('.');
        if !is_agent_file {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Agent>(&text) {
                Ok(agent) => agents.push(agent),
                Err(e) => problems.push(format!("{} 格式不对: {e}", path.display())),
            },
            Err(e) => problems.push(format!("读 {} 失败: {e}", path.display())),
        }
    }
    (agents, problems)
}

/// 排序用的一行：agent + 校正后的状态 + 它的窗格此刻所在的窗口（窗格没了就是 None）。
#[derive(Clone, Debug)]
pub struct Entry {
    pub agent: Agent,
    pub status: Status,
    pub window: Option<String>,
}

/// 等你批准 → 等你回复 → 运行中 → 启动中/空闲 → 已退出；同组内等得最久的在上。
pub fn sorted_entries(agents: Vec<Agent>, panes: &HashMap<String, PaneInfo>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = agents
        .into_iter()
        .map(|agent| {
            let status = agent.effective_status(panes);
            let window = agent.pane_id.as_ref().and_then(|p| panes.get(p)).map(|info| info.window_id.clone());
            Entry { agent, status, window }
        })
        .collect();
    entries.sort_by(|a, b| {
        (a.status.rank(), a.agent.status_since, &a.agent.name).cmp(&(b.status.rank(), b.agent.status_since, &b.agent.name))
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, status: Status, since: u64, pane: &str) -> Agent {
        Agent {
            name: name.to_string(),
            kind: Kind::Claude,
            command: "claude".to_string(),
            cwd: "/tmp".to_string(),
            created_at: 1,
            status,
            status_since: since,
            window_id: Some("@1".to_string()),
            pane_id: Some(pane.to_string()),
            session_id: None,
            last_message: None,
            model: None,
            effort: None,
            label: None,
        }
    }

    fn live(panes: &[&str]) -> HashMap<String, PaneInfo> {
        panes
            .iter()
            .map(|p| (p.to_string(), PaneInfo { dead: false, window_id: "@1".to_string(), index: 1 }))
            .collect()
    }

    #[test]
    fn name_basic() {
        assert_eq!(make_name("claude", Path::new("/work/myproj"), &[]), "claude-myproj");
    }

    #[test]
    fn name_collision_gets_suffix() {
        let taken = vec!["claude-myproj".to_string()];
        assert_eq!(make_name("claude", Path::new("/a/myproj"), &taken), "claude-myproj-2");
        let taken = vec!["claude-myproj".to_string(), "claude-myproj-2".to_string()];
        assert_eq!(make_name("claude", Path::new("/b/myproj"), &taken), "claude-myproj-3");
    }

    #[test]
    fn name_chinese_dir_is_replaced() {
        assert_eq!(make_name("codex", Path::new("/tmp/x/中文 目录"), &[]), "codex");
        assert_eq!(make_name("codex", Path::new("/tmp/中文 proj 2"), &[]), "codex-proj-2");
    }

    #[test]
    fn name_all_replaced_falls_back() {
        assert_eq!(sanitize_name("中文"), "agent");
        assert_eq!(sanitize_name(""), "agent");
        assert_eq!(sanitize_name("a b/c"), "a-b-c");
        assert_eq!(sanitize_name("v1.2_x"), "v1.2_x");
    }

    #[test]
    fn name_uses_program_basename() {
        assert_eq!(make_name("/usr/bin/gemini", Path::new("/w/p"), &[]), "gemini-p");
    }

    #[test]
    fn safe_names() {
        assert!(is_safe_name("claude-a.b_c"));
        assert!(!is_safe_name("../x"));
        assert!(!is_safe_name(""));
    }

    #[test]
    fn sort_groups_then_oldest_first() {
        let agents = vec![
            sample("exited", Status::Exited, 1, "%1"),
            sample("idle", Status::Idle, 5, "%2"),
            sample("run-new", Status::Running, 50, "%3"),
            sample("reply-old", Status::NeedsReply, 10, "%4"),
            sample("approve", Status::NeedsApproval, 99, "%5"),
            sample("run-old", Status::Running, 20, "%6"),
            sample("reply-new", Status::NeedsReply, 30, "%7"),
        ];
        let panes = live(&["%2", "%3", "%4", "%5", "%6", "%7"]);
        let order: Vec<String> = sorted_entries(agents, &panes).into_iter().map(|e| e.agent.name).collect();
        assert_eq!(order, ["approve", "reply-old", "reply-new", "run-old", "run-new", "idle", "exited"]);
    }

    #[test]
    fn live_pane_overrides_a_stale_session_end() {
        let a = sample("alive", Status::Exited, 100, "%7");
        assert_eq!(a.effective_status(&live(&["%7"])), Status::Running);
        assert_eq!(a.effective_status(&live(&[])), Status::Exited);
    }

    #[test]
    fn missing_or_dead_pane_sorts_as_exited() {
        let agents = vec![
            sample("gone", Status::NeedsApproval, 1, "%9"),
            sample("dead", Status::Running, 1, "%8"),
            sample("alive", Status::Idle, 100, "%7"),
        ];
        let mut panes = live(&["%7"]);
        panes.insert("%8".to_string(), PaneInfo { dead: true, window_id: "@2".to_string(), index: 1 });
        let entries = sorted_entries(agents, &panes);
        assert_eq!(entries[0].agent.name, "alive");
        assert_eq!(entries[1].status, Status::Exited);
        assert_eq!(entries[2].status, Status::Exited);
    }

    #[test]
    fn entry_window_comes_from_tmux_not_from_the_stored_id() {
        // 文件里存的是 @1（开窗时的页），但窗格已经被并排到 @7 了
        let a = sample("moved", Status::Running, 1, "%1");
        assert_eq!(a.window_id.as_deref(), Some("@1"));
        let mut panes = live(&["%1"]);
        panes.insert("%1".to_string(), PaneInfo { dead: false, window_id: "@7".to_string(), index: 2 });
        let entries = sorted_entries(vec![a, sample("gone", Status::Running, 1, "%9")], &panes);
        assert_eq!(entries[0].window.as_deref(), Some("@7"));
        assert_eq!(entries[1].window, None);
    }

    #[test]
    fn other_kind_starting_shows_running() {
        let mut a = sample("sh-x", Status::Starting, 1, "%1");
        a.kind = Kind::Other;
        assert_eq!(a.effective_status(&live(&["%1"])), Status::Running);
        assert_eq!(a.effective_status(&HashMap::new()), Status::Exited);
        a.kind = Kind::Claude;
        assert_eq!(a.effective_status(&live(&["%1"])), Status::Starting);
    }

    #[test]
    fn json_roundtrip_has_spec_fields() {
        let a = sample("claude-p", Status::NeedsReply, 7, "%1");
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(v["kind"], "claude");
        assert_eq!(v["status"], "needs_reply");
        assert!(v["session_id"].is_null());
        assert_eq!(v["pane_id"], "%1");
    }
}

#[cfg(test)]
mod initial_tab_status_tests {
    use super::*;

    #[test]
    fn hookless_commands_show_running_on_tabs() {
        assert_eq!(initial_tab_status(Kind::Other), Status::Running);
        assert_eq!(initial_tab_status(Kind::Claude), Status::Starting);
        assert_eq!(initial_tab_status(Kind::Codex), Status::Starting);
    }
}
